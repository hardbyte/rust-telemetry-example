use crate::error::{DalError, Result};
use crate::models::{Book, BookCreateInput, BookFilterParams, BookSearchParams, BookStatus};
use crate::repository::traits::BookRepository;
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, warn};

pub struct BookRepositoryImpl {
    write_pool: Arc<PgPool>,
    read_pool: Arc<PgPool>,
}

impl BookRepositoryImpl {
    /// Create repository with separate read and write pools
    pub fn new(write_pool: Arc<PgPool>, read_pool: Arc<PgPool>) -> Self {
        Self {
            write_pool,
            read_pool,
        }
    }

    /// Create repository with single pool for both read and write operations
    pub fn single_pool(pool: Arc<PgPool>) -> Self {
        Self {
            write_pool: pool.clone(),
            read_pool: pool,
        }
    }
}

#[async_trait]
impl BookRepository for BookRepositoryImpl {
    #[tracing::instrument(name = "get_all_books_from_db", level = tracing::Level::DEBUG, skip(self))]
    async fn find_all(&self) -> Result<Vec<Book>> {
        debug!("Getting all books from database");

        let books = sqlx::query_as!(
            Book,
            r#"
            SELECT 
                id, 
                title, 
                author, 
                status as "status: BookStatus" 
            FROM books 
            ORDER BY title, author
            "#
        )
        .fetch_all(self.read_pool.as_ref())
        .await?;

        Ok(books)
    }

    #[tracing::instrument(name = "get_book_by_id", skip(self), fields(book.id = %id))]
    async fn find_by_id(&self, id: i32) -> Result<Option<Book>> {
        let book = sqlx::query_as!(
            Book,
            r#"
            SELECT
                id,
                title,
                author,
                status as "status!: BookStatus"
            FROM books
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(self.read_pool.as_ref())
        .await?;

        Ok(book)
    }

    #[tracing::instrument(name = "create_book_in_db", skip(self, input), fields(book.title = %input.title, book.author = %input.author))]
    async fn create(&self, input: BookCreateInput) -> Result<i32> {
        let status = input.status.unwrap_or_default();

        let result = sqlx::query!(
            r#"
            INSERT INTO books (title, author, status) 
            VALUES ($1, $2, $3) 
            RETURNING id
            "#,
            input.title,
            input.author,
            status as BookStatus,
        )
        .fetch_one(self.write_pool.as_ref())
        .await?;

        Ok(result.id)
    }

    #[tracing::instrument(name = "update_book_in_db", skip(self), fields(book.id = %book.id, book.title = %book.title, book.author = %book.author))]
    async fn update(&self, book: Book) -> Result<i32> {
        let result = sqlx::query!(
            r#"
            UPDATE books
            SET
                author = $2,
                title = $3,
                status = $4
            WHERE id = $1
            "#,
            book.id,
            book.author,
            book.title,
            book.status as BookStatus
        )
        .execute(self.write_pool.as_ref())
        .await?;

        let rows_affected: i32 = result.rows_affected().try_into().unwrap_or(0);
        tracing::Span::current().record("db.rows_affected", rows_affected);

        if rows_affected == 0 {
            warn!("Update operation affected 0 rows - book may not exist");
        } else {
            debug!("Successfully updated book");
        }

        Ok(rows_affected)
    }

    #[tracing::instrument(name = "delete_book_from_db", skip(self), fields(book.id = %id))]
    async fn delete(&self, id: i32) -> Result<()> {
        let result = sqlx::query!("DELETE FROM books WHERE id = $1", id)
            .execute(self.write_pool.as_ref())
            .await?;

        if result.rows_affected() == 0 {
            return Err(DalError::BookNotFound { id });
        }

        Ok(())
    }

    #[tracing::instrument(name = "bulk_create_books_in_db", skip(self, books), fields(num_books = books.len()))]
    async fn bulk_create(&self, books: &[BookCreateInput]) -> Result<Vec<i32>> {
        if books.is_empty() {
            return Ok(Vec::new());
        }

        let mut query_builder: sqlx::QueryBuilder<sqlx::Postgres> =
            sqlx::QueryBuilder::new("INSERT INTO books (title, author, status) ");

        query_builder.push_values(books.iter(), |mut b, book| {
            let status = book.status.clone().unwrap_or_default();
            b.push_bind(&book.title)
                .push_bind(&book.author)
                .push_bind(status as BookStatus);
        });
        query_builder.push(" RETURNING id");

        let rows = query_builder
            .build_query_as::<(i32,)>()
            .fetch_all(self.write_pool.as_ref())
            .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    async fn find_by_filters(&self, params: BookFilterParams) -> Result<Vec<Book>> {
        // Handle different combinations of filters
        match (
            params.status,
            params.author_pattern.as_ref(),
            params.title_pattern.as_ref(),
        ) {
            (Some(status), Some(author), Some(title)) => sqlx::query_as!(
                Book,
                r#"
                    SELECT 
                        id,
                        title,
                        author,
                        status as "status!: BookStatus"
                    FROM books
                    WHERE status = $1 
                        AND author ILIKE '%' || $2 || '%'
                        AND title ILIKE '%' || $3 || '%'
                    ORDER BY title, author
                    LIMIT $4
                    OFFSET $5
                    "#,
                status as BookStatus,
                author,
                title,
                params.limit.unwrap_or(100),
                params.offset.unwrap_or(0)
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into),
            (Some(status), Some(author), None) => sqlx::query_as!(
                Book,
                r#"
                    SELECT 
                        id,
                        title,
                        author,
                        status as "status!: BookStatus"
                    FROM books
                    WHERE status = $1 AND author ILIKE '%' || $2 || '%'
                    ORDER BY title, author
                    LIMIT $3
                    OFFSET $4
                    "#,
                status as BookStatus,
                author,
                params.limit.unwrap_or(100),
                params.offset.unwrap_or(0)
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into),
            (Some(status), None, Some(title)) => sqlx::query_as!(
                Book,
                r#"
                    SELECT 
                        id,
                        title,
                        author,
                        status as "status!: BookStatus"
                    FROM books
                    WHERE status = $1 AND title ILIKE '%' || $2 || '%'
                    ORDER BY title, author
                    LIMIT $3
                    OFFSET $4
                    "#,
                status as BookStatus,
                title,
                params.limit.unwrap_or(100),
                params.offset.unwrap_or(0)
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into),
            (Some(status), None, None) => sqlx::query_as!(
                Book,
                r#"
                    SELECT 
                        id,
                        title,
                        author,
                        status as "status!: BookStatus"
                    FROM books
                    WHERE status = $1
                    ORDER BY title, author
                    LIMIT $2
                    OFFSET $3
                    "#,
                status as BookStatus,
                params.limit.unwrap_or(100),
                params.offset.unwrap_or(0)
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into),
            (None, Some(author), Some(title)) => sqlx::query_as!(
                Book,
                r#"
                    SELECT 
                        id,
                        title,
                        author,
                        status as "status!: BookStatus"
                    FROM books
                    WHERE author ILIKE '%' || $1 || '%'
                        AND title ILIKE '%' || $2 || '%'
                    ORDER BY title, author
                    LIMIT $3
                    OFFSET $4
                    "#,
                author,
                title,
                params.limit.unwrap_or(100),
                params.offset.unwrap_or(0)
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into),
            (None, Some(author), None) => sqlx::query_as!(
                Book,
                r#"
                    SELECT 
                        id,
                        title,
                        author,
                        status as "status!: BookStatus"
                    FROM books
                    WHERE author ILIKE '%' || $1 || '%'
                    ORDER BY title, author
                    LIMIT $2
                    OFFSET $3
                    "#,
                author,
                params.limit.unwrap_or(100),
                params.offset.unwrap_or(0)
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into),
            (None, None, Some(title)) => sqlx::query_as!(
                Book,
                r#"
                    SELECT 
                        id,
                        title,
                        author,
                        status as "status!: BookStatus"
                    FROM books
                    WHERE title ILIKE '%' || $1 || '%'
                    ORDER BY title, author
                    LIMIT $2
                    OFFSET $3
                    "#,
                title,
                params.limit.unwrap_or(100),
                params.offset.unwrap_or(0)
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into),
            (None, None, None) => sqlx::query_as!(
                Book,
                r#"
                    SELECT 
                        id,
                        title,
                        author,
                        status as "status!: BookStatus"
                    FROM books
                    ORDER BY title, author
                    LIMIT $1
                    OFFSET $2
                    "#,
                params.limit.unwrap_or(100),
                params.offset.unwrap_or(0)
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into),
        }
    }

    async fn search_books(&self, params: BookSearchParams) -> Result<Vec<Book>> {
        // Simplified search for now - can be enhanced later with conditional queries
        let offset = (params.page - 1) * params.per_page;

        if let Some(search_term) = params.search_term {
            sqlx::query_as!(
                Book,
                r#"
                SELECT 
                    id,
                    title,
                    author,
                    status as "status!: BookStatus"
                FROM books
                WHERE 
                    title ILIKE '%' || $1 || '%' 
                    OR author ILIKE '%' || $1 || '%'
                ORDER BY title
                LIMIT $2
                OFFSET $3
                "#,
                search_term,
                params.per_page,
                offset
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into)
        } else {
            sqlx::query_as!(
                Book,
                r#"
                SELECT 
                    id,
                    title,
                    author,
                    status as "status!: BookStatus"
                FROM books
                ORDER BY title
                LIMIT $1
                OFFSET $2
                "#,
                params.per_page,
                offset
            )
            .fetch_all(self.read_pool.as_ref())
            .await
            .map_err(Into::into)
        }
    }

    async fn update_status(&self, id: i32, status: BookStatus) -> Result<bool> {
        let result = sqlx::query!(
            r#"
            UPDATE books 
            SET status = $2 
            WHERE id = $1
            "#,
            id,
            status as BookStatus
        )
        .execute(self.write_pool.as_ref())
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BookFilterParams, BookSearchParams, BookStatus, SortField, SortOrder};
    use sqlx::PgPool;

    #[sqlx::test]
    async fn test_find_all_books(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));
        let books = repo.find_all().await.unwrap();

        // Should have books from migrations
        assert!(!books.is_empty());
        assert!(books.len() >= 90); // We have 100 books in the migration
    }

    #[sqlx::test]
    async fn test_create_and_find_book(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        let input = BookCreateInput {
            title: "Test Book".to_string(),
            author: "Test Author".to_string(),
            status: Some(BookStatus::Available),
        };

        let book_id = repo.create(input).await.unwrap();
        assert!(book_id > 0);

        let found_book = repo.find_by_id(book_id).await.unwrap();
        assert!(found_book.is_some());

        let book = found_book.unwrap();
        assert_eq!(book.title, "Test Book");
        assert_eq!(book.author, "Test Author");
        assert_eq!(book.status, BookStatus::Available);
    }

    #[sqlx::test]
    async fn test_update_book(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a book first
        let input = BookCreateInput {
            title: "Original Title".to_string(),
            author: "Original Author".to_string(),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        // Update the book
        let updated_book = Book {
            id: book_id,
            title: "Updated Title".to_string(),
            author: "Updated Author".to_string(),
            status: BookStatus::Borrowed,
        };

        let rows_affected = repo.update(updated_book).await.unwrap();
        assert_eq!(rows_affected, 1);

        // Verify the update
        let found_book = repo.find_by_id(book_id).await.unwrap().unwrap();
        assert_eq!(found_book.title, "Updated Title");
        assert_eq!(found_book.author, "Updated Author");
        assert_eq!(found_book.status, BookStatus::Borrowed);
    }

    #[sqlx::test]
    async fn test_delete_book(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a book first
        let input = BookCreateInput {
            title: "To Be Deleted".to_string(),
            author: "Delete Author".to_string(),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        // Verify it exists
        assert!(repo.find_by_id(book_id).await.unwrap().is_some());

        // Delete it
        repo.delete(book_id).await.unwrap();

        // Verify it's gone
        assert!(repo.find_by_id(book_id).await.unwrap().is_none());
    }

    #[sqlx::test]
    async fn test_bulk_create(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        let books = vec![
            BookCreateInput {
                title: "Bulk Book 1".to_string(),
                author: "Bulk Author 1".to_string(),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                title: "Bulk Book 2".to_string(),
                author: "Bulk Author 2".to_string(),
                status: Some(BookStatus::Borrowed),
            },
            BookCreateInput {
                title: "Bulk Book 3".to_string(),
                author: "Bulk Author 3".to_string(),
                status: None, // Should default to Available
            },
        ];

        let ids = repo.bulk_create(&books).await.unwrap();
        assert_eq!(ids.len(), 3);

        // Verify all books were created
        for id in ids {
            let book = repo.find_by_id(id).await.unwrap();
            assert!(book.is_some());
        }
    }

    #[sqlx::test]
    async fn test_find_by_filters(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create test books
        let test_books = vec![
            BookCreateInput {
                title: "Filter Test Book 1".to_string(),
                author: "Smith".to_string(),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                title: "Filter Test Book 2".to_string(),
                author: "Johnson".to_string(),
                status: Some(BookStatus::Borrowed),
            },
        ];

        repo.bulk_create(&test_books).await.unwrap();

        // Test filter by status
        let params = BookFilterParams {
            status: Some(BookStatus::Available),
            ..Default::default()
        };

        let available_books = repo.find_by_filters(params).await.unwrap();
        assert!(!available_books.is_empty());

        // Test filter by author pattern
        let params = BookFilterParams {
            author_pattern: Some("Smith".to_string()),
            ..Default::default()
        };

        let smith_books = repo.find_by_filters(params).await.unwrap();
        assert!(!smith_books.is_empty());
        assert!(smith_books.iter().all(|book| book.author.contains("Smith")));
    }

    #[sqlx::test]
    async fn test_search_books(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a test book with unique content
        let unique_title = "Unique Search Test Book";
        let test_book = BookCreateInput {
            title: unique_title.to_string(),
            author: "Search Author".to_string(),
            status: Some(BookStatus::Available),
        };

        repo.create(test_book).await.unwrap();

        // Search for it
        let search_params = BookSearchParams {
            search_term: Some("Unique Search".to_string()),
            statuses: vec![],
            min_id: None,
            sort_by: SortField::Title,
            sort_order: SortOrder::Asc,
            page: 1,
            per_page: 10,
        };

        let results = repo.search_books(search_params).await.unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|book| book.title.contains("Unique Search")));
    }

    #[sqlx::test]
    async fn test_update_status(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a book
        let input = BookCreateInput {
            title: "Status Test Book".to_string(),
            author: "Status Author".to_string(),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        // Update status
        let success = repo.update_status(book_id, BookStatus::Lost).await.unwrap();
        assert!(success);

        // Verify the status change
        let book = repo.find_by_id(book_id).await.unwrap().unwrap();
        assert_eq!(book.status, BookStatus::Lost);

        // Test updating non-existent book
        let success = repo
            .update_status(99999, BookStatus::Available)
            .await
            .unwrap();
        assert!(!success);
    }
}
