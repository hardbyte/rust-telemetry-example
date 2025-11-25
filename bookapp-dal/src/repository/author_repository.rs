use crate::error::{DalError, Result};
use crate::models::{Author, AuthorCreateInput};
use crate::TracedPgPool;
use sqlx::{Executor, PgPool, Postgres};
use std::sync::Arc;
use tracing::{debug, instrument, warn};
use uuid::Uuid;

pub struct AuthorRepositoryImpl {
    write_pool: Arc<TracedPgPool>,
    read_pool: Arc<TracedPgPool>,
}

impl AuthorRepositoryImpl {
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
        name = "create_author_with",
        skip_all,
        fields(author.name = %input.name)
    )]
    pub async fn create_with<'e, E>(&self, exec: E, input: AuthorCreateInput) -> Result<Uuid>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let row = sqlx::query!(
            r#"
            INSERT INTO authors (name, sort_name)
            VALUES ($1, $2)
            RETURNING id
            "#,
            input.name,
            input.sort_name
        )
        .fetch_one(exec)
        .await?;

        Ok(row.id)
    }

    #[instrument(name = "find_author_by_id_with", skip_all, fields(author.id = %id))]
    pub async fn find_by_id_with<'e, E>(&self, exec: E, id: Uuid) -> Result<Option<Author>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let author = sqlx::query_as!(
            Author,
            r#"
            SELECT
                id,
                name,
                sort_name,
                created_at,
                updated_at
            FROM authors
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(exec)
        .await?;

        Ok(author)
    }

    #[instrument(name = "find_all_authors_with", skip_all)]
    pub async fn find_all_with<'e, E>(&self, exec: E) -> Result<Vec<Author>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        debug!("Fetching all authors (executor-aware)");
        let authors = sqlx::query_as!(
            Author,
            r#"
            SELECT
                id,
                name,
                sort_name,
                created_at,
                updated_at
            FROM authors
            ORDER BY sort_name NULLS LAST, name, id
            "#
        )
        .fetch_all(exec)
        .await?;

        Ok(authors)
    }

    #[instrument(
        name = "update_author_with",
        skip_all,
        fields(author.id = %author.id, author.name = %author.name)
    )]
    pub async fn update_with<'e, E>(&self, exec: E, author: Author) -> Result<i32>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query!(
            r#"
            UPDATE authors
            SET
                name = $2,
                sort_name = $3
            WHERE id = $1
            "#,
            author.id,
            author.name,
            author.sort_name
        )
        .execute(exec)
        .await?;

        let rows_affected: i32 = result.rows_affected().try_into().unwrap_or(0);
        if rows_affected == 0 {
            warn!("Update affected 0 rows - author may not exist");
        } else {
            debug!("Successfully updated author");
        }

        Ok(rows_affected)
    }

    #[instrument(name = "delete_author_with", skip_all, fields(author.id = %id))]
    pub async fn delete_with<'e, E>(&self, exec: E, id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query!(
            r#"
            DELETE FROM authors
            WHERE id = $1
            "#,
            id
        )
        .execute(exec)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DalError::InvalidInput {
                message: format!("Author not found: {id}"),
            });
        }

        Ok(())
    }
    #[instrument(name = "create_author", skip(self, input), fields(author.name = %input.name))]
    pub async fn create(&self, input: AuthorCreateInput) -> Result<Uuid> {
        self.create_with(&*self.write_pool, input).await
    }

    #[instrument(name = "find_author_by_id", skip(self), fields(author.id = %id))]
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Author>> {
        self.find_by_id_with(&*self.read_pool, id).await
    }

    #[instrument(name = "find_all_authors", skip(self))]
    pub async fn find_all(&self) -> Result<Vec<Author>> {
        self.find_all_with(&*self.read_pool).await
    }

    #[instrument(name = "update_author", skip(self), fields(author.id = %author.id, author.name = %author.name))]
    pub async fn update(&self, author: Author) -> Result<i32> {
        self.update_with(&*self.write_pool, author).await
    }

    #[instrument(name = "delete_author", skip(self), fields(author.id = %id))]
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        self.delete_with(&*self.write_pool, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    #[sqlx::test]
    async fn test_create_and_find_author(pool: PgPool) {
        let repo = AuthorRepositoryImpl::from_pg_pool(Arc::new(pool));

        let input = AuthorCreateInput {
            name: "J. R. R. Tolkien".to_string(),
            sort_name: Some("Tolkien, J. R. R.".to_string()),
        };

        let author_id = repo.create(input).await.unwrap();
        assert!(!author_id.is_nil());

        let found = repo.find_by_id(author_id).await.unwrap();
        assert!(found.is_some());
        let author = found.unwrap();
        assert_eq!(author.name, "J. R. R. Tolkien");
        assert_eq!(author.sort_name.as_deref(), Some("Tolkien, J. R. R."));
        assert!(author.created_at.is_some());
        assert!(author.updated_at.is_some());
    }

    #[sqlx::test]
    async fn test_find_all_authors(pool: PgPool) {
        let repo = AuthorRepositoryImpl::from_pg_pool(Arc::new(pool));

        // Seed a couple of authors
        let _ = repo
            .create(AuthorCreateInput {
                name: "Frank Herbert".to_string(),
                sort_name: Some("Herbert, Frank".to_string()),
            })
            .await
            .unwrap();
        let _ = repo
            .create(AuthorCreateInput {
                name: "George Orwell".to_string(),
                sort_name: Some("Orwell, George".to_string()),
            })
            .await
            .unwrap();

        let all = repo.find_all().await.unwrap();
        assert!(!all.is_empty());
        assert!(
            all.iter().any(|a| a.name == "Frank Herbert")
                && all.iter().any(|a| a.name == "George Orwell")
        );
    }

    #[sqlx::test]
    async fn test_update_author(pool: PgPool) {
        let repo = AuthorRepositoryImpl::from_pg_pool(Arc::new(pool));

        let id = repo
            .create(AuthorCreateInput {
                name: "Pat Rothfuss".to_string(),
                sort_name: Some("Rothfuss, Patrick".to_string()),
            })
            .await
            .unwrap();

        let mut author = repo.find_by_id(id).await.unwrap().unwrap();
        author.name = "Patrick Rothfuss".to_string();
        author.sort_name = Some("Rothfuss, Patrick".to_string());

        let rows = repo.update(author).await.unwrap();
        assert_eq!(rows, 1);

        let updated = repo.find_by_id(id).await.unwrap().unwrap();
        assert_eq!(updated.name, "Patrick Rothfuss");
        assert_eq!(updated.sort_name.as_deref(), Some("Rothfuss, Patrick"));
    }

    #[sqlx::test]
    async fn test_delete_author(pool: PgPool) {
        let repo = AuthorRepositoryImpl::from_pg_pool(Arc::new(pool));

        let id = repo
            .create(AuthorCreateInput {
                name: "Temp Author".to_string(),
                sort_name: None,
            })
            .await
            .unwrap();

        // Ensure it exists
        assert!(repo.find_by_id(id).await.unwrap().is_some());

        // Delete it
        repo.delete(id).await.unwrap();

        // Ensure it's gone
        assert!(repo.find_by_id(id).await.unwrap().is_none());
    }

    #[sqlx::test]
    async fn test_delete_nonexistent_author(pool: PgPool) {
        let repo = AuthorRepositoryImpl::from_pg_pool(Arc::new(pool));
        let res = repo.delete(Uuid::nil()).await;
        assert!(res.is_err());
    }
}
