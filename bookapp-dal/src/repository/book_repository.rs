use crate::error::{DalError, Result};
use crate::models::{
    Book, BookCreateInput, BookFilterParams, BookSearchParams, BookSearchResult, BookStatus,
};
use crate::repository::traits::BookRepository;
use async_trait::async_trait;
use sqlx::{Executor, PgPool, Postgres};
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

    /// Get access to the write pool for administrative operations
    pub fn write_pool(&self) -> &Arc<PgPool> {
        &self.write_pool
    }

    /// Executor-aware variant: find a book by id using a provided executor (pool or transaction)
    pub async fn find_by_id_with<'e, E>(&self, exec: E, id: i32) -> Result<Option<Book>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, Book>(
            r#"
            SELECT
                w.id                         as "id!",
                w.id                         as "work_id!",
                w.title                      as "work_title!",
                a.id                         as "primary_author_id!",
                a.name                       as "primary_author_name!",
                'available'::book_status     as "status!: BookStatus"
            FROM works w
            JOIN work_authors wa
              ON wa.work_id = w.id
             AND wa.primary_author = true
            JOIN authors a
              ON a.id = wa.author_id
            WHERE w.id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(exec)
        .await
        .map_err(Into::into)
    }

    /// Executor-aware variant: create a new book (normalized: works + authors + association)
    pub async fn create_with(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        input: BookCreateInput,
    ) -> Result<i32> {
        // 1) Insert the work
        let work_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO works (title)
            VALUES ($1)
            RETURNING id
            "#,
        )
        .bind(&input.work_title)
        .fetch_one(tx.as_mut())
        .await?;

        // 2) Resolve/create primary author
        let author_id = if let Some(id) = input.primary_author_id {
            id
        } else if let Some(name) = input.primary_author_name.clone() {
            sqlx::query_scalar(
                r#"
                INSERT INTO authors (name, sort_name)
                VALUES ($1, NULL)
                ON CONFLICT (name)
                DO UPDATE SET sort_name = COALESCE(EXCLUDED.sort_name, authors.sort_name)
                RETURNING id
                "#,
            )
            .bind(&name)
            .fetch_one(tx.as_mut())
            .await?
        } else {
            return Err(crate::error::DalError::InvalidInput {
                message: "primary_author_id or primary_author_name must be provided".into(),
            });
        };

        // 3) Upsert association as primary author
        sqlx::query(
            r#"
            INSERT INTO work_authors (work_id, author_id, role, primary_author, ord)
            VALUES ($1, $2, 'Author', true, 1)
            ON CONFLICT (work_id, author_id)
            DO UPDATE SET primary_author = true, ord = 1
            "#,
        )
        .bind(work_id)
        .bind(author_id)
        .execute(tx.as_mut())
        .await?;

        Ok(work_id)
    }

    /// Executor-aware variant: update an existing book (normalized: works)
    pub async fn update_with<'e, E>(&self, exec: E, book: Book) -> Result<i32>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query(
            r#"
            UPDATE works
            SET
                title = $2
            WHERE id = $1
            "#,
        )
        .bind(book.id)
        .bind(&book.work_title)
        .execute(exec)
        .await?;

        let rows_affected: i32 = result.rows_affected().try_into().unwrap_or(0);
        if rows_affected == 0 {
            warn!("Update operation affected 0 rows - book may not exist");
        } else {
            debug!("Successfully updated book");
        }
        Ok(rows_affected)
    }

    /// Executor-aware variant: delete a book (normalized: works)
    pub async fn delete_with<'e, E>(&self, exec: E, id: i32) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query("DELETE FROM works WHERE id = $1")
            .bind(id)
            .execute(exec)
            .await?;

        if result.rows_affected() == 0 {
            return Err(DalError::BookNotFound { id });
        }

        Ok(())
    }

    /// Executor-aware variant: bulk create books (normalized: per-item create)
    pub async fn bulk_create_with(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        books: &[BookCreateInput],
    ) -> Result<Vec<i32>> {
        if books.is_empty() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::with_capacity(books.len());
        for input in books {
            // Insert work
            let work_id: i32 = sqlx::query_scalar(
                r#"
                INSERT INTO works (title)
                VALUES ($1)
                RETURNING id
                "#,
            )
            .bind(&input.work_title)
            .fetch_one(tx.as_mut())
            .await?;

            // Resolve/create author
            let author_id = if let Some(id) = input.primary_author_id {
                id
            } else if let Some(name) = input.primary_author_name.clone() {
                sqlx::query_scalar(
                    r#"
                    INSERT INTO authors (name, sort_name)
                    VALUES ($1, NULL)
                    ON CONFLICT (name)
                    DO UPDATE SET sort_name = COALESCE(EXCLUDED.sort_name, authors.sort_name)
                    RETURNING id
                    "#,
                )
                .bind(&name)
                .fetch_one(tx.as_mut())
                .await?
            } else {
                return Err(crate::error::DalError::InvalidInput {
                    message: "primary_author_id or primary_author_name must be provided".into(),
                });
            };

            // Upsert association
            sqlx::query(
                r#"
                INSERT INTO work_authors (work_id, author_id, role, primary_author, ord)
                VALUES ($1, $2, 'Author', true, 1)
                ON CONFLICT (work_id, author_id)
                DO UPDATE SET primary_author = true, ord = 1
                "#,
            )
            .bind(work_id)
            .bind(author_id)
            .execute(tx.as_mut())
            .await?;

            ids.push(work_id);
        }

        Ok(ids)
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
                w.id                         as "id!",
                w.id                         as "work_id!",
                w.title                      as "work_title!",
                a.id                         as "primary_author_id!",
                a.name                       as "primary_author_name!",
                'available'::book_status     as "status!: _"
            FROM works w
            JOIN work_authors wa
              ON wa.work_id = w.id
             AND wa.primary_author = true
            JOIN authors a
              ON a.id = wa.author_id
            ORDER BY w.title, a.name
            "#
        )
        .fetch_all(self.read_pool.as_ref())
        .await?;

        Ok(books)
    }

    #[tracing::instrument(name = "get_book_by_id", skip(self), fields(book.id = %id, db.operation = "select", db.collection.name = "works"))]
    async fn find_by_id(&self, id: i32) -> Result<Option<Book>> {
        let sql = r#"
            SELECT
                w.id                         as "id!",
                w.id                         as "work_id!",
                w.title                      as "work_title!",
                a.id                         as "primary_author_id!",
                a.name                       as "primary_author_name!",
                'available'::book_status     as "status!: BookStatus"
            FROM works w
            JOIN work_authors wa
              ON wa.work_id = w.id
             AND wa.primary_author = true
            JOIN authors a
              ON a.id = wa.author_id
            WHERE w.id = $1
            "#;

        // Record sanitized SQL statement in span
        tracing::Span::current().record("db.statement", sql.trim());
        tracing::Span::current().record("db.system", "postgresql");

        let book = sqlx::query_as!(
            Book,
            r#"
            SELECT
                w.id                         as "id!",
                w.id                         as "work_id!",
                w.title                      as "work_title!",
                a.id                         as "primary_author_id!",
                a.name                       as "primary_author_name!",
                'available'::book_status     as "status!: BookStatus"
            FROM works w
            JOIN work_authors wa
              ON wa.work_id = w.id
             AND wa.primary_author = true
            JOIN authors a
              ON a.id = wa.author_id
            WHERE w.id = $1
            "#,
            id
        )
        .fetch_optional(self.read_pool.as_ref())
        .await?;

        Ok(book)
    }

    #[tracing::instrument(name = "create_work_with_primary_author", skip(self, input), fields(work.title = %input.work_title))]
    async fn create(&self, input: BookCreateInput) -> Result<i32> {
        // 1) Insert the work
        let work_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO works (title)
            VALUES ($1)
            RETURNING id
            "#,
        )
        .bind(&input.work_title)
        .fetch_one(self.write_pool.as_ref())
        .await?;

        // 2) Resolve/create primary author
        let author_id = if let Some(id) = input.primary_author_id {
            id
        } else if let Some(name) = input.primary_author_name.clone() {
            let row = sqlx::query!(
                r#"
                INSERT INTO authors (name, sort_name)
                VALUES ($1, NULL)
                ON CONFLICT (name)
                DO UPDATE SET sort_name = COALESCE(EXCLUDED.sort_name, authors.sort_name)
                RETURNING id
                "#,
                name
            )
            .fetch_one(self.write_pool.as_ref())
            .await?;
            row.id
        } else {
            return Err(crate::error::DalError::InvalidInput {
                message: "primary_author_id or primary_author_name must be provided".into(),
            });
        };

        // 3) Upsert association as primary author
        sqlx::query!(
            r#"
            INSERT INTO work_authors (work_id, author_id, role, primary_author, ord)
            VALUES ($1, $2, 'Author', true, 1)
            ON CONFLICT (work_id, author_id)
            DO UPDATE SET primary_author = true, ord = 1
            "#,
            work_id,
            author_id
        )
        .execute(self.write_pool.as_ref())
        .await?;

        Ok(work_id)
    }

    #[tracing::instrument(name = "update_book_in_db", skip(self), fields(book.id = %book.id, book.work_title = %book.work_title))]
    async fn update(&self, book: Book) -> Result<i32> {
        let result = sqlx::query(
            r#"
            UPDATE works
            SET
                title = $2
            WHERE id = $1
            "#,
        )
        .bind(book.id)
        .bind(&book.work_title)
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
        let result = sqlx::query!("DELETE FROM works WHERE id = $1", id)
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

        // Use a single transaction to atomically create all works + author associations
        let mut tx = self.write_pool.begin().await?;

        let ids = self.bulk_create_with(&mut tx, books).await?;

        tx.commit().await?;
        Ok(ids)
    }

    async fn find_by_filters(&self, params: BookFilterParams) -> Result<Vec<Book>> {
        // Use QueryBuilder for dynamic query construction - much cleaner than large match statements
        let mut query = sqlx::QueryBuilder::new(
            r#"
            SELECT
                w.id,
                w.id as work_id,
                w.title as work_title,
                a.id as primary_author_id,
                a.name as primary_author_name,
                'available'::book_status as status
            FROM works w
            JOIN work_authors wa
              ON wa.work_id = w.id
             AND wa.primary_author = true
            JOIN authors a
              ON a.id = wa.author_id
            "#,
        );

        // Dynamically append WHERE clauses based on provided filters
        let mut where_added = false;

        if let Some(ref author_pattern) = params.primary_author_pattern {
            query.push(" WHERE a.name ILIKE ");
            query.push_bind(format!("%{}%", author_pattern));
            where_added = true;
        }

        if let Some(ref title_pattern) = params.work_title_pattern {
            if where_added {
                query.push(" AND ");
            } else {
                query.push(" WHERE ");
            }
            query.push("w.title ILIKE ");
            query.push_bind(format!("%{}%", title_pattern));
        }

        // Add ordering and pagination
        query.push(" ORDER BY w.title, a.name");
        query.push(" LIMIT ");
        query.push_bind(params.limit.unwrap_or(100));
        query.push(" OFFSET ");
        query.push_bind(params.offset.unwrap_or(0));

        let books = query
            .build_query_as::<Book>()
            .fetch_all(self.read_pool.as_ref())
            .await?;

        Ok(books)
    }

    async fn search_books(&self, params: BookSearchParams) -> Result<Vec<Book>> {
        // Simplified search for now - can be enhanced later with conditional queries
        let offset = (params.page - 1) * params.per_page;

        if let Some(search_term) = params.search_term {
            sqlx::query_as!(
                Book,
                r#"
                SELECT
                    w.id                         as "id!",
                    w.id                         as "work_id!",
                    w.title                      as "work_title!",
                    a.id                         as "primary_author_id!",
                    a.name                       as "primary_author_name!",
                    'available'::book_status     as "status!: BookStatus"
                FROM works w
                JOIN work_authors wa
                  ON wa.work_id = w.id
                 AND wa.primary_author = true
                JOIN authors a
                  ON a.id = wa.author_id
                WHERE
                    w.title ILIKE '%' || $1 || '%'
                    OR a.name  ILIKE '%' || $1 || '%'
                ORDER BY w.title, a.name
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
                    w.id                         as "id!",
                    w.id                         as "work_id!",
                    w.title                      as "work_title!",
                    a.id                         as "primary_author_id!",
                    a.name                       as "primary_author_name!",
                    'available'::book_status     as "status!: BookStatus"
                FROM works w
                JOIN work_authors wa
                  ON wa.work_id = w.id
                 AND wa.primary_author = true
                JOIN authors a
                  ON a.id = wa.author_id
                ORDER BY w.title, a.name
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

    #[tracing::instrument(
        name = "full_text_search_books_in_db",
        skip(self),
        fields(
            search.query = %query,
            search.limit = %limit,
            search.result_count,
            search.max_rank,
            search.min_rank,
            search.execution_time_ms,
            db.operation = "search",
            db.collection.name = "book_search_view"
        )
    )]
    async fn full_text_search(&self, query: &str, limit: i64) -> Result<Vec<BookSearchResult>> {
        let start_time = std::time::Instant::now();

        tracing::info!(
            search.query = %query,
            search.limit = %limit,
            "Starting full-text search operation"
        );

        let results = sqlx::query_as!(
            BookSearchResult,
            r#"
            SELECT
                sv.work_id as "work_id!",
                w.title as "work_title!",
                a.name as "primary_author_name!",
                ts_rank(sv.document, websearch_to_tsquery('english', $1))::real as "rank!",
                ts_headline('english', w.title, websearch_to_tsquery('english', $1),
                    'StartSel=<em>, StopSel=</em>, MinWords=5, MaxWords=10') as "headline"
            FROM book_search_view sv
            JOIN works w ON w.id = sv.work_id
            -- Join to get the primary author for display
            JOIN work_authors wa ON w.id = wa.work_id AND wa.primary_author = true
            JOIN authors a ON a.id = wa.author_id
            WHERE sv.document @@ websearch_to_tsquery('english', $1)
            ORDER BY ts_rank(sv.document, websearch_to_tsquery('english', $1)) DESC
            LIMIT $2
            "#,
            query,
            limit
        )
        .fetch_all(self.read_pool.as_ref())
        .await?;

        let execution_time = start_time.elapsed();

        // Record search metrics and attributes
        let result_count = results.len();
        let max_rank = results.first().map(|r| r.rank).unwrap_or(0.0);
        let min_rank = results.last().map(|r| r.rank).unwrap_or(0.0);

        // Record span attributes for observability
        tracing::Span::current().record("search.result_count", result_count);
        tracing::Span::current().record("search.max_rank", max_rank as f64);
        tracing::Span::current().record("search.min_rank", min_rank as f64);
        tracing::Span::current().record(
            "search.execution_time_ms",
            execution_time.as_millis() as u64,
        );

        tracing::info!(
            search.result_count = result_count,
            search.max_rank = max_rank,
            search.min_rank = min_rank,
            search.execution_time_ms = execution_time.as_millis(),
            search.has_results = !results.is_empty(),
            "Full-text search completed successfully"
        );

        Ok(results)
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

        // Create a test book first since there's no seed data
        let input = BookCreateInput {
            work_title: "Find All Test Book".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Find All Test Author".to_string()),
            status: Some(BookStatus::Available),
        };
        repo.create(input).await.unwrap();

        let books = repo.find_all().await.unwrap();

        assert!(!books.is_empty());
    }

    #[sqlx::test]
    async fn test_create_and_find_book(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        let input = BookCreateInput {
            work_title: "Test Book".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Test Author".to_string()),
            status: Some(BookStatus::Available),
        };

        let book_id = repo.create(input).await.unwrap();
        assert!(book_id > 0);

        let found_book = repo.find_by_id(book_id).await.unwrap();
        assert!(found_book.is_some());

        let book = found_book.unwrap();
        assert_eq!(book.work_title, "Test Book");
        assert_eq!(book.primary_author_name, "Test Author");
        assert_eq!(book.status, BookStatus::Available);
    }

    #[sqlx::test]
    async fn test_update_book(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a book first
        let input = BookCreateInput {
            work_title: "Original Title".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Original Author".to_string()),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        // Update the book (only title can be updated now - status is always Available)
        let mut current = repo.find_by_id(book_id).await.unwrap().unwrap();
        current.work_title = "Updated Title".to_string();

        let rows_affected = repo.update(current).await.unwrap();
        assert_eq!(rows_affected, 1);

        // Verify the update
        let found_book = repo.find_by_id(book_id).await.unwrap().unwrap();
        assert_eq!(found_book.work_title, "Updated Title");
        assert_eq!(found_book.status, BookStatus::Available); // Status is always Available now
    }

    #[sqlx::test]
    async fn test_delete_book(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a book first
        let input = BookCreateInput {
            work_title: "To Be Deleted".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Delete Author".to_string()),
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
                work_title: "Bulk Book 1".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Bulk Author 1".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "Bulk Book 2".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Bulk Author 2".to_string()),
                status: Some(BookStatus::Borrowed),
            },
            BookCreateInput {
                work_title: "Bulk Book 3".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Bulk Author 3".to_string()),
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
                work_title: "Filter Test Book 1".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Smith".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "Filter Test Book 2".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Johnson".to_string()),
                status: Some(BookStatus::Borrowed),
            },
        ];

        repo.bulk_create(&test_books).await.unwrap();

        // Status filtering no longer supported - all books show as Available
        // Test that we get all books when no filters are provided
        let params = BookFilterParams {
            ..Default::default()
        };

        let all_books = repo.find_by_filters(params).await.unwrap();
        assert!(!all_books.is_empty());

        // Test filter by primary author pattern
        let params = BookFilterParams {
            primary_author_pattern: Some("Smith".to_string()),
            ..Default::default()
        };

        let smith_books = repo.find_by_filters(params).await.unwrap();
        assert!(!smith_books.is_empty());
        assert!(smith_books
            .iter()
            .all(|book| book.primary_author_name.contains("Smith")));
    }

    #[sqlx::test]
    async fn test_search_books(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a test book with unique content
        let unique_title = "Unique Search Test Book";
        let test_book = BookCreateInput {
            work_title: unique_title.to_string(),
            primary_author_id: None,
            primary_author_name: Some("Search Author".to_string()),
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
            .any(|book| book.work_title.contains("Unique Search")));
    }

    #[sqlx::test]
    async fn test_full_text_search(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create test data that will be indexed in the materialized view
        let test_books = vec![
            BookCreateInput {
                work_title: "Harry Potter and the Philosopher's Stone".to_string(),
                primary_author_id: None,
                primary_author_name: Some("J.K. Rowling".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "A Game of Thrones".to_string(),
                primary_author_id: None,
                primary_author_name: Some("George R.R. Martin".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "The Fellowship of the Ring".to_string(),
                primary_author_id: None,
                primary_author_name: Some("J.R.R. Tolkien".to_string()),
                status: Some(BookStatus::Available),
            },
        ];

        repo.bulk_create(&test_books).await.unwrap();

        // Manually refresh the materialized view for testing
        sqlx::query("REFRESH MATERIALIZED VIEW book_search_view")
            .execute(repo.write_pool().as_ref())
            .await
            .unwrap();

        // Test search by title
        let results = repo.full_text_search("Harry Potter", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|r| r.work_title.contains("Harry Potter")));
        assert!(results[0].rank > 0.0);

        // Test search by author
        let results = repo.full_text_search("Rowling", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|r| r.primary_author_name.contains("Rowling")));

        // Test search with multiple terms
        let results = repo.full_text_search("Game Thrones", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|r| r.work_title.contains("Game of Thrones")));

        // Test search with no results
        let results = repo
            .full_text_search("nonexistent book title", 10)
            .await
            .unwrap();
        assert!(results.is_empty());

        // Test search ordering by relevance (rank)
        let results = repo.full_text_search("Ring", 10).await.unwrap();
        if results.len() > 1 {
            // Results should be ordered by rank descending
            assert!(results[0].rank >= results[1].rank);
        }
    }
}
