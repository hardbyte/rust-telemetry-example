use crate::error::{DalError, Result};
use crate::models::{Edition, EditionCreateInput};
use crate::TracedPgPool;
use sqlx::{Executor, PgPool, Postgres};
use std::sync::Arc;
use tracing::{debug, instrument};
use uuid::Uuid;

pub struct EditionRepositoryImpl {
    write_pool: Arc<TracedPgPool>,
    read_pool: Arc<TracedPgPool>,
}

impl EditionRepositoryImpl {
    /// Create repository with separate read and write pools (traced for OTel)
    pub fn new(write_pool: Arc<TracedPgPool>, read_pool: Arc<TracedPgPool>) -> Self {
        Self {
            write_pool,
            read_pool,
        }
    }

    /// Create repository with a single pool for both read and write operations
    pub fn single_pool(pool: Arc<TracedPgPool>) -> Self {
        Self {
            write_pool: pool.clone(),
            read_pool: pool,
        }
    }

    /// Create repository from untraced PgPool (for tests and backwards compatibility)
    pub fn from_pg_pool(pool: Arc<PgPool>) -> Self {
        use sqlx_tracing::PoolBuilder;
        let traced = Arc::new(
            PoolBuilder::from((*pool).clone())
                .with_name("test-pool")
                .with_database("bookapp")
                .build(),
        );
        Self {
            write_pool: traced.clone(),
            read_pool: traced,
        }
    }

    // Executor-aware variants for transactional use

    #[instrument(
        name = "create_edition_with",
        skip_all,
        fields(work.id = %input.work_id, edition.isbn = %input.isbn)
    )]
    pub async fn create_with<'e, E>(&self, exec: E, input: EditionCreateInput) -> Result<Uuid>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let row = sqlx::query!(
            r#"
            INSERT INTO editions
                (work_id, isbn, title, publisher, publication_date, "language", page_count, format)
            VALUES
                ($1,      $2,   $3,    $4,        $5,               $6,        $7,         $8)
            RETURNING id
            "#,
            input.work_id,
            input.isbn,
            input.title,
            input.publisher,
            input.publication_date,
            input.language,
            input.page_count,
            input.format
        )
        .fetch_one(exec)
        .await?;

        Ok(row.id)
    }

    #[instrument(name = "find_edition_by_id_with", skip_all, fields(edition.id = %id))]
    pub async fn find_by_id_with<'e, E>(&self, exec: E, id: Uuid) -> Result<Option<Edition>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let edition = sqlx::query_as!(
            Edition,
            r#"
            SELECT
                id,
                work_id,
                isbn,
                title,
                publisher,
                publication_date,
                "language",
                page_count,
                format,
                created_at,
                updated_at
            FROM editions
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(exec)
        .await?;

        Ok(edition)
    }

    #[instrument(name = "find_edition_by_isbn_with", skip_all, fields(edition.isbn = %isbn))]
    pub async fn find_by_isbn_with<'e, E>(&self, exec: E, isbn: &str) -> Result<Option<Edition>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let edition = sqlx::query_as!(
            Edition,
            r#"
            SELECT
                id,
                work_id,
                isbn,
                title,
                publisher,
                publication_date,
                "language",
                page_count,
                format,
                created_at,
                updated_at
            FROM editions
            WHERE isbn = $1
            "#,
            isbn
        )
        .fetch_optional(exec)
        .await?;

        Ok(edition)
    }

    #[instrument(name = "list_editions_by_work_with", skip_all, fields(work.id = %work_id))]
    pub async fn list_by_work_with<'e, E>(&self, exec: E, work_id: Uuid) -> Result<Vec<Edition>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        debug!("Listing editions for work_id={} (executor-aware)", work_id);
        let rows = sqlx::query_as!(
            Edition,
            r#"
            SELECT
                id,
                work_id,
                isbn,
                title,
                publisher,
                publication_date,
                "language",
                page_count,
                format,
                created_at,
                updated_at
            FROM editions
            WHERE work_id = $1
            ORDER BY publication_date NULLS LAST, id
            "#,
            work_id
        )
        .fetch_all(exec)
        .await?;

        Ok(rows)
    }

    #[instrument(name = "delete_edition_with", skip_all, fields(edition.id = %id))]
    pub async fn delete_with<'e, E>(&self, exec: E, id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query!(
            r#"
            DELETE FROM editions
            WHERE id = $1
            "#,
            id
        )
        .execute(exec)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DalError::InvalidInput {
                message: format!("Edition not found: {id}"),
            });
        }

        Ok(())
    }

    pub async fn create(&self, input: EditionCreateInput) -> Result<Uuid> {
        self.create_with(&*self.write_pool, input).await
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Edition>> {
        self.find_by_id_with(&*self.read_pool, id).await
    }

    pub async fn find_by_isbn(&self, isbn: &str) -> Result<Option<Edition>> {
        self.find_by_isbn_with(&*self.read_pool, isbn).await
    }

    pub async fn list_by_work(&self, work_id: Uuid) -> Result<Vec<Edition>> {
        self.list_by_work_with(&*self.read_pool, work_id).await
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        self.delete_with(&*self.write_pool, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    async fn insert_work(pool: &PgPool, title: &str) -> Uuid {
        let row = sqlx::query!(
            r#"
            INSERT INTO works (title)
            VALUES ($1)
            RETURNING id
            "#,
            title
        )
        .fetch_one(pool)
        .await
        .expect("insert work");
        row.id
    }

    #[sqlx::test]
    async fn test_create_and_find_by_id(pool: PgPool) {
        let repo = EditionRepositoryImpl::from_pg_pool(Arc::new(pool.clone()));
        let work_id = insert_work(&pool, "The Silmarillion").await;

        let id = repo
            .create(EditionCreateInput {
                work_id,
                isbn: "9780261102736".to_string(),
                title: Some("The Silmarillion (HarperCollins)".to_string()),
                publisher: Some("HarperCollins".to_string()),
                publication_date: None,
                language: Some("en".to_string()),
                page_count: Some(480),
                format: Some("paperback".to_string()),
            })
            .await
            .unwrap();

        assert!(!id.is_nil());

        let found = repo.find_by_id(id).await.unwrap();
        assert!(found.is_some());
        let ed = found.unwrap();
        assert_eq!(ed.id, id);
        assert_eq!(ed.work_id, work_id);
        assert_eq!(ed.isbn, "9780261102736");
        assert_eq!(ed.language.as_deref(), Some("en"));
        assert_eq!(ed.page_count, Some(480));
    }

    #[sqlx::test]
    async fn test_find_by_isbn(pool: PgPool) {
        let repo = EditionRepositoryImpl::from_pg_pool(Arc::new(pool.clone()));
        let work_id = insert_work(&pool, "Dune").await;

        let isbn = "9780441172719";
        let id = repo
            .create(EditionCreateInput {
                work_id,
                isbn: isbn.to_string(),
                title: Some("Dune".to_string()),
                publisher: Some("Ace".to_string()),
                publication_date: None,
                language: Some("en".to_string()),
                page_count: Some(896),
                format: Some("paperback".to_string()),
            })
            .await
            .unwrap();

        let found = repo.find_by_isbn(isbn).await.unwrap();
        assert!(found.is_some());
        let ed = found.unwrap();
        assert_eq!(ed.id, id);
        assert_eq!(ed.isbn, isbn);
    }

    #[sqlx::test]
    async fn test_list_by_work(pool: PgPool) {
        let repo = EditionRepositoryImpl::from_pg_pool(Arc::new(pool.clone()));
        let work_id = insert_work(&pool, "1984").await;

        let _ = repo
            .create(EditionCreateInput {
                work_id,
                isbn: "9780451524935".to_string(),
                title: Some("1984 (Signet)".to_string()),
                publisher: Some("Signet".to_string()),
                publication_date: None,
                language: Some("en".to_string()),
                page_count: Some(328),
                format: Some("paperback".to_string()),
            })
            .await
            .unwrap();

        let _ = repo
            .create(EditionCreateInput {
                work_id,
                isbn: "9780141036144".to_string(),
                title: Some("1984 (Penguin)".to_string()),
                publisher: Some("Penguin".to_string()),
                publication_date: None,
                language: Some("en".to_string()),
                page_count: Some(336),
                format: Some("paperback".to_string()),
            })
            .await
            .unwrap();

        let list = repo.list_by_work(work_id).await.unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|e| e.isbn == "9780451524935"));
        assert!(list.iter().any(|e| e.isbn == "9780141036144"));
    }

    #[sqlx::test]
    async fn test_delete_edition(pool: PgPool) {
        let repo = EditionRepositoryImpl::from_pg_pool(Arc::new(pool.clone()));
        let work_id = insert_work(&pool, "Temp Work").await;

        let id = repo
            .create(EditionCreateInput {
                work_id,
                isbn: "1111111111111".to_string(),
                title: Some("Temp Edition".to_string()),
                publisher: None,
                publication_date: None,
                language: None,
                page_count: None,
                format: None,
            })
            .await
            .unwrap();

        assert!(repo.find_by_id(id).await.unwrap().is_some());
        repo.delete(id).await.unwrap();
        assert!(repo.find_by_id(id).await.unwrap().is_none());
    }

    #[sqlx::test]
    async fn test_delete_nonexistent_edition(pool: PgPool) {
        let repo = EditionRepositoryImpl::from_pg_pool(Arc::new(pool));
        let res = repo.delete(Uuid::nil()).await;
        assert!(res.is_err());
    }
}
