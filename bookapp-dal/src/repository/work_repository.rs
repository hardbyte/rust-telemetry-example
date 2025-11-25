use crate::error::{DalError, Result};
use crate::models::{Work, WorkAuthor, WorkAuthorCreateInput, WorkCreateInput};
use crate::TracedPgPool;
use sqlx::{Executor, PgPool, Postgres};
use std::sync::Arc;
use tracing::{debug, instrument, warn};
use uuid::Uuid;

pub struct WorkRepositoryImpl {
    write_pool: Arc<TracedPgPool>,
    read_pool: Arc<TracedPgPool>,
}

impl WorkRepositoryImpl {
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
        name = "create_work_with",
        skip_all,
        fields(work.title = %input.title)
    )]
    pub async fn create_with<'e, E>(&self, exec: E, input: WorkCreateInput) -> Result<Uuid>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let row = sqlx::query!(
            r#"
            INSERT INTO works (title, original_language, description, publication_year)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
            input.title,
            input.original_language,
            input.description,
            input.publication_year
        )
        .fetch_one(exec)
        .await?;

        Ok(row.id)
    }

    #[instrument(name = "find_work_by_id_with", skip_all, fields(work.id = %id))]
    pub async fn find_by_id_with<'e, E>(&self, exec: E, id: Uuid) -> Result<Option<Work>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let work = sqlx::query_as!(
            Work,
            r#"
            SELECT
                id,
                title,
                original_language,
                description,
                publication_year,
                created_at,
                updated_at
            FROM works
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(exec)
        .await?;

        Ok(work)
    }

    #[instrument(name = "find_all_works_with", skip_all)]
    pub async fn find_all_with<'e, E>(&self, exec: E) -> Result<Vec<Work>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        debug!("Fetching all works (executor-aware)");
        let works = sqlx::query_as!(
            Work,
            r#"
            SELECT
                id,
                title,
                original_language,
                description,
                publication_year,
                created_at,
                updated_at
            FROM works
            ORDER BY title, id
            "#
        )
        .fetch_all(exec)
        .await?;

        Ok(works)
    }

    #[instrument(
        name = "update_work_with",
        skip_all,
        fields(work.id = %work.id, work.title = %work.title)
    )]
    pub async fn update_with<'e, E>(&self, exec: E, work: Work) -> Result<i32>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query!(
            r#"
            UPDATE works
            SET
                title = $2,
                original_language = $3,
                description = $4,
                publication_year = $5
            WHERE id = $1
            "#,
            work.id,
            work.title,
            work.original_language,
            work.description,
            work.publication_year
        )
        .execute(exec)
        .await?;

        let rows_affected: i32 = result.rows_affected().try_into().unwrap_or(0);
        if rows_affected == 0 {
            warn!("Update affected 0 rows - work may not exist");
        } else {
            debug!("Successfully updated work");
        }

        Ok(rows_affected)
    }

    #[instrument(name = "delete_work_with", skip_all, fields(work.id = %id))]
    pub async fn delete_with<'e, E>(&self, exec: E, id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query!(
            r#"
            DELETE FROM works
            WHERE id = $1
            "#,
            id
        )
        .execute(exec)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DalError::InvalidInput {
                message: format!("Work not found: {id}"),
            });
        }

        Ok(())
    }

    #[instrument(
        name = "add_author_to_work_with",
        skip_all,
        fields(work.id = %assoc.work_id, author.id = %assoc.author_id)
    )]
    pub async fn add_author_with<'e, E>(&self, exec: E, assoc: WorkAuthorCreateInput) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let role = assoc.role.unwrap_or_else(|| "Author".to_string());
        let primary_author = assoc.primary_author.unwrap_or(false);
        let ord = assoc.ord;

        sqlx::query!(
            r#"
            INSERT INTO work_authors (work_id, author_id, role, primary_author, ord)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (work_id, author_id) DO UPDATE
            SET role = EXCLUDED.role,
                primary_author = EXCLUDED.primary_author,
                ord = EXCLUDED.ord
            "#,
            assoc.work_id,
            assoc.author_id,
            role,
            primary_author,
            ord
        )
        .execute(exec)
        .await?;

        Ok(())
    }

    #[instrument(name = "list_authors_for_work_with", skip_all, fields(work.id = %work_id))]
    pub async fn list_authors_with<'e, E>(&self, exec: E, work_id: Uuid) -> Result<Vec<WorkAuthor>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let rows = sqlx::query_as!(
            WorkAuthor,
            r#"
            SELECT
                work_id,
                author_id,
                role,
                primary_author,
                ord,
                created_at,
                updated_at
            FROM work_authors
            WHERE work_id = $1
            ORDER BY ord NULLS LAST, created_at, author_id
            "#,
            work_id
        )
        .fetch_all(exec)
        .await?;

        Ok(rows)
    }

    #[instrument(name = "create_work", skip(self, input), fields(work.title = %input.title))]
    pub async fn create(&self, input: WorkCreateInput) -> Result<Uuid> {
        self.create_with(&*self.write_pool, input).await
    }

    #[instrument(name = "find_work_by_id", skip(self), fields(work.id = %id))]
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Work>> {
        self.find_by_id_with(&*self.read_pool, id).await
    }

    #[instrument(name = "find_all_works", skip(self))]
    pub async fn find_all(&self) -> Result<Vec<Work>> {
        self.find_all_with(&*self.read_pool).await
    }

    #[instrument(
        name = "update_work",
        skip(self),
        fields(work.id = %work.id, work.title = %work.title)
    )]
    pub async fn update(&self, work: Work) -> Result<i32> {
        self.update_with(&*self.write_pool, work).await
    }

    #[instrument(name = "delete_work", skip(self), fields(work.id = %id))]
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        self.delete_with(&*self.write_pool, id).await
    }

    #[instrument(
        name = "add_author_to_work",
        skip(self, assoc),
        fields(work.id = %assoc.work_id, author.id = %assoc.author_id)
    )]
    pub async fn add_author(&self, assoc: WorkAuthorCreateInput) -> Result<()> {
        self.add_author_with(&*self.write_pool, assoc).await
    }

    #[instrument(name = "list_authors_for_work", skip(self), fields(work.id = %work_id))]
    pub async fn list_authors(&self, work_id: Uuid) -> Result<Vec<WorkAuthor>> {
        self.list_authors_with(&*self.read_pool, work_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    #[sqlx::test]
    async fn test_create_and_find_work(pool: PgPool) {
        let repo = WorkRepositoryImpl::from_pg_pool(Arc::new(pool));

        let input = WorkCreateInput {
            title: "The Lord of the Rings".to_string(),
            original_language: Some("en".to_string()),
            description: Some("Epic high-fantasy novel".to_string()),
            publication_year: Some(1954),
        };

        let id = repo.create(input).await.unwrap();
        assert!(!id.is_nil());

        let found = repo.find_by_id(id).await.unwrap();
        assert!(found.is_some());
        let work = found.unwrap();
        assert_eq!(work.title, "The Lord of the Rings");
        assert_eq!(work.original_language.as_deref(), Some("en"));
        assert_eq!(work.publication_year, Some(1954));
        assert!(work.created_at.is_some());
        assert!(work.updated_at.is_some());
    }

    #[sqlx::test]
    async fn test_update_work(pool: PgPool) {
        let repo = WorkRepositoryImpl::from_pg_pool(Arc::new(pool));

        let id = repo
            .create(WorkCreateInput {
                title: "Dune".to_string(),
                original_language: Some("en".to_string()),
                description: Some("Sci-fi".to_string()),
                publication_year: Some(1965),
            })
            .await
            .unwrap();

        let mut work = repo.find_by_id(id).await.unwrap().unwrap();
        work.description = Some("Science fiction classic".to_string());
        work.publication_year = Some(1966); // hypothetical updated metadata
        let rows = repo.update(work).await.unwrap();
        assert_eq!(rows, 1);

        let updated = repo.find_by_id(id).await.unwrap().unwrap();
        assert_eq!(
            updated.description.as_deref(),
            Some("Science fiction classic")
        );
        assert_eq!(updated.publication_year, Some(1966));
    }

    #[sqlx::test]
    async fn test_work_add_and_list_authors(pool: PgPool) {
        let repo = WorkRepositoryImpl::from_pg_pool(Arc::new(pool.clone()));

        // Create a work
        let work_id = repo
            .create(WorkCreateInput {
                title: "1984".to_string(),
                original_language: Some("en".to_string()),
                description: Some("Dystopian fiction".to_string()),
                publication_year: Some(1949),
            })
            .await
            .unwrap();

        // Insert an author directly
        let row = sqlx::query!(
            r#"
            INSERT INTO authors (name, sort_name)
            VALUES ($1, $2)
            RETURNING id
            "#,
            "George Orwell",
            "Orwell, George"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let author_id = row.id;

        // Associate author with work
        repo.add_author(WorkAuthorCreateInput {
            work_id,
            author_id,
            role: Some("Author".to_string()),
            primary_author: Some(true),
            ord: Some(1),
        })
        .await
        .unwrap();

        // List associations
        let authors = repo.list_authors(work_id).await.unwrap();
        assert_eq!(authors.len(), 1);
        let assoc = &authors[0];
        assert_eq!(assoc.work_id, work_id);
        assert_eq!(assoc.author_id, author_id);
        assert_eq!(assoc.role, "Author");
        assert!(assoc.primary_author);
        assert_eq!(assoc.ord, Some(1));
    }

    #[sqlx::test]
    async fn test_delete_work(pool: PgPool) {
        let repo = WorkRepositoryImpl::from_pg_pool(Arc::new(pool));

        let id = repo
            .create(WorkCreateInput {
                title: "Temp Work".to_string(),
                original_language: None,
                description: None,
                publication_year: None,
            })
            .await
            .unwrap();

        // Ensure it exists
        assert!(repo.find_by_id(id).await.unwrap().is_some());

        // Delete
        repo.delete(id).await.unwrap();

        // Ensure it's gone
        assert!(repo.find_by_id(id).await.unwrap().is_none());
    }
}
