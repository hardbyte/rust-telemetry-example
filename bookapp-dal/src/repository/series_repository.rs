use crate::error::{DalError, Result};
use crate::models::{
    Series, SeriesCreateInput, SeriesWorksAssociation, SeriesWorksAssociationCreateInput,
};
use sqlx::{Executor, PgPool, Postgres};
use std::sync::Arc;
use tracing::{debug, instrument};
use uuid::Uuid;

pub struct SeriesRepositoryImpl {
    write_pool: Arc<PgPool>,
    read_pool: Arc<PgPool>,
}

impl SeriesRepositoryImpl {
    /// Create repository with separate read and write pools
    pub fn new(write_pool: Arc<PgPool>, read_pool: Arc<PgPool>) -> Self {
        Self {
            write_pool,
            read_pool,
        }
    }

    /// Create repository with a single pool for both read and write operations
    pub fn single_pool(pool: Arc<PgPool>) -> Self {
        Self {
            write_pool: pool.clone(),
            read_pool: pool,
        }
    }

    // Executor-aware variants for transactional use

    #[instrument(name = "create_series_with", skip_all, fields(series.name = %input.name))]
    pub async fn create_with<'e, E>(&self, exec: E, input: SeriesCreateInput) -> Result<Uuid>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let row = sqlx::query!(
            r#"
            INSERT INTO series (name, description)
            VALUES ($1, $2)
            RETURNING id
            "#,
            input.name,
            input.description
        )
        .fetch_one(exec)
        .await?;

        Ok(row.id)
    }

    #[instrument(name = "find_series_by_id_with", skip_all, fields(series.id = %id))]
    pub async fn find_by_id_with<'e, E>(&self, exec: E, id: Uuid) -> Result<Option<Series>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let series = sqlx::query_as!(
            Series,
            r#"
            SELECT
                id,
                name,
                description,
                created_at,
                updated_at
            FROM series
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(exec)
        .await?;

        Ok(series)
    }

    #[instrument(name = "find_all_series_with", skip_all)]
    pub async fn find_all_with<'e, E>(&self, exec: E) -> Result<Vec<Series>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        debug!("Fetching all series (executor-aware)");
        let rows = sqlx::query_as!(
            Series,
            r#"
            SELECT
                id,
                name,
                description,
                created_at,
                updated_at
            FROM series
            ORDER BY name, id
            "#
        )
        .fetch_all(exec)
        .await?;

        Ok(rows)
    }

    #[instrument(name = "delete_series_with", skip_all, fields(series.id = %id))]
    pub async fn delete_with<'e, E>(&self, exec: E, id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query!(
            r#"
            DELETE FROM series
            WHERE id = $1
            "#,
            id
        )
        .execute(exec)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DalError::InvalidInput {
                message: format!("Series not found: {id}"),
            });
        }

        Ok(())
    }

    #[instrument(
        name = "add_work_to_series_with",
        skip_all,
        fields(series.id = %assoc.series_id, work.id = %assoc.work_id)
    )]
    pub async fn add_work_with<'e, E>(
        &self,
        exec: E,
        assoc: SeriesWorksAssociationCreateInput,
    ) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let primary_work = assoc.primary_work.unwrap_or(true);
        let order_id = assoc.order_id;

        sqlx::query!(
            r#"
            INSERT INTO series_works_association (series_id, work_id, primary_work, order_id)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (series_id, work_id) DO UPDATE
            SET primary_work = EXCLUDED.primary_work,
                order_id = EXCLUDED.order_id
            "#,
            assoc.series_id,
            assoc.work_id,
            primary_work,
            order_id
        )
        .execute(exec)
        .await?;

        Ok(())
    }

    #[instrument(name = "list_works_for_series_with", skip_all, fields(series.id = %series_id))]
    pub async fn list_works_with<'e, E>(
        &self,
        exec: E,
        series_id: Uuid,
    ) -> Result<Vec<SeriesWorksAssociation>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let rows = sqlx::query_as!(
            SeriesWorksAssociation,
            r#"
            SELECT
                series_id,
                work_id,
                primary_work,
                order_id,
                created_at,
                updated_at
            FROM series_works_association
            WHERE series_id = $1
            ORDER BY order_id NULLS LAST, work_id
            "#,
            series_id
        )
        .fetch_all(exec)
        .await?;

        Ok(rows)
    }

    #[instrument(
        name = "remove_work_from_series_with",
        skip_all,
        fields(series.id = %series_id, work.id = %work_id)
    )]
    pub async fn remove_work_with<'e, E>(
        &self,
        exec: E,
        series_id: Uuid,
        work_id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query!(
            r#"
            DELETE FROM series_works_association
            WHERE series_id = $1 AND work_id = $2
            "#,
            series_id,
            work_id
        )
        .execute(exec)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DalError::InvalidInput {
                message: format!("Association not found: series_id={series_id}, work_id={work_id}"),
            });
        }

        Ok(())
    }

    #[instrument(name = "create_series", skip(self, input), fields(series.name = %input.name))]
    pub async fn create(&self, input: SeriesCreateInput) -> Result<Uuid> {
        self.create_with(&*self.write_pool, input).await
    }

    #[instrument(name = "find_series_by_id", skip(self), fields(series.id = %id))]
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Series>> {
        self.find_by_id_with(&*self.read_pool, id).await
    }

    #[instrument(name = "find_all_series", skip(self))]
    pub async fn find_all(&self) -> Result<Vec<Series>> {
        self.find_all_with(&*self.read_pool).await
    }

    #[instrument(name = "delete_series", skip(self), fields(series.id = %id))]
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        self.delete_with(&*self.write_pool, id).await
    }

    #[instrument(
        name = "add_work_to_series",
        skip(self, assoc),
        fields(series.id = %assoc.series_id, work.id = %assoc.work_id)
    )]
    pub async fn add_work(&self, assoc: SeriesWorksAssociationCreateInput) -> Result<()> {
        self.add_work_with(&*self.write_pool, assoc).await
    }

    #[instrument(name = "list_works_for_series", skip(self), fields(series.id = %series_id))]
    pub async fn list_works(&self, series_id: Uuid) -> Result<Vec<SeriesWorksAssociation>> {
        self.list_works_with(&*self.read_pool, series_id).await
    }

    #[instrument(
        name = "remove_work_from_series",
        skip(self),
        fields(series.id = %series_id, work.id = %work_id)
    )]
    pub async fn remove_work(&self, series_id: Uuid, work_id: Uuid) -> Result<()> {
        self.remove_work_with(&*self.write_pool, series_id, work_id)
            .await
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
    async fn test_create_and_find_series(pool: PgPool) {
        let repo = SeriesRepositoryImpl::single_pool(Arc::new(pool));

        let id = repo
            .create(SeriesCreateInput {
                name: "The Wheel of Time".to_string(),
                description: Some("Epic fantasy series".to_string()),
            })
            .await
            .unwrap();

        assert!(!id.is_nil());

        let s = repo.find_by_id(id).await.unwrap().unwrap();
        assert_eq!(s.id, id);
        assert_eq!(s.name, "The Wheel of Time");
        assert_eq!(s.description.as_deref(), Some("Epic fantasy series"));
        assert!(s.created_at.is_some());
        assert!(s.updated_at.is_some());
    }

    #[sqlx::test]
    async fn test_add_and_list_works(pool: PgPool) {
        let repo = SeriesRepositoryImpl::single_pool(Arc::new(pool.clone()));

        let series_id = repo
            .create(SeriesCreateInput {
                name: "The Lord of the Rings".to_string(),
                description: Some("Classic high-fantasy".to_string()),
            })
            .await
            .unwrap();

        let work1 = insert_work(&pool, "The Fellowship of the Ring").await;
        let work2 = insert_work(&pool, "The Two Towers").await;
        let work3 = insert_work(&pool, "The Return of the King").await;

        repo.add_work(SeriesWorksAssociationCreateInput {
            series_id,
            work_id: work1,
            primary_work: Some(true),
            order_id: Some(1),
        })
        .await
        .unwrap();

        repo.add_work(SeriesWorksAssociationCreateInput {
            series_id,
            work_id: work2,
            primary_work: Some(true),
            order_id: Some(2),
        })
        .await
        .unwrap();

        repo.add_work(SeriesWorksAssociationCreateInput {
            series_id,
            work_id: work3,
            primary_work: Some(true),
            order_id: Some(3),
        })
        .await
        .unwrap();

        let list = repo.list_works(series_id).await.unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].work_id, work1);
        assert_eq!(list[1].work_id, work2);
        assert_eq!(list[2].work_id, work3);
        assert!(list.iter().all(|a| a.primary_work));
    }

    #[sqlx::test]
    async fn test_remove_work(pool: PgPool) {
        let repo = SeriesRepositoryImpl::single_pool(Arc::new(pool.clone()));

        let series_id = repo
            .create(SeriesCreateInput {
                name: "Dune Saga".to_string(),
                description: None,
            })
            .await
            .unwrap();

        let work_id = insert_work(&pool, "Dune").await;

        repo.add_work(SeriesWorksAssociationCreateInput {
            series_id,
            work_id,
            primary_work: Some(true),
            order_id: Some(1),
        })
        .await
        .unwrap();

        // Ensure association exists
        let list = repo.list_works(series_id).await.unwrap();
        assert_eq!(list.len(), 1);

        // Remove association
        repo.remove_work(series_id, work_id).await.unwrap();

        // Ensure it's gone
        let list = repo.list_works(series_id).await.unwrap();
        assert!(list.is_empty());
    }

    #[sqlx::test]
    async fn test_delete_series_cascade(pool: PgPool) {
        let repo = SeriesRepositoryImpl::single_pool(Arc::new(pool.clone()));

        let series_id = repo
            .create(SeriesCreateInput {
                name: "Test Series".to_string(),
                description: None,
            })
            .await
            .unwrap();

        let work_id = insert_work(&pool, "Temp Work").await;

        repo.add_work(SeriesWorksAssociationCreateInput {
            series_id,
            work_id,
            primary_work: Some(true),
            order_id: Some(1),
        })
        .await
        .unwrap();

        // Delete the series, should cascade delete association
        repo.delete(series_id).await.unwrap();

        // Listing works should return empty (association rows removed)
        let list = repo.list_works(series_id).await.unwrap();
        assert!(list.is_empty());

        // Series should be gone
        let s = repo.find_by_id(series_id).await.unwrap();
        assert!(s.is_none());
    }

    #[sqlx::test]
    async fn test_delete_nonexistent_series(pool: PgPool) {
        let repo = SeriesRepositoryImpl::single_pool(Arc::new(pool));
        let res = repo.delete(Uuid::nil()).await;
        assert!(res.is_err());
    }

    #[sqlx::test]
    async fn test_remove_nonexistent_association(pool: PgPool) {
        let repo = SeriesRepositoryImpl::single_pool(Arc::new(pool));
        let res = repo.remove_work(Uuid::nil(), Uuid::nil()).await;
        assert!(res.is_err());
    }
}
