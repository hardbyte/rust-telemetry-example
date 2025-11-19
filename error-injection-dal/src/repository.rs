use crate::models::{ErrorInjectionConfig, ErrorInjectionConfigInput};
use sqlx::{Executor, PgPool, Postgres};
use std::sync::Arc;
use tracing::{instrument, Level};

pub type Result<T> = std::result::Result<T, sqlx::Error>;

#[derive(Clone)]
pub struct ErrorInjectionRepository {
    write_pool: Arc<PgPool>,
    read_pool: Arc<PgPool>,
}

impl ErrorInjectionRepository {
    pub fn new(write_pool: Arc<PgPool>, read_pool: Arc<PgPool>) -> Self {
        Self {
            write_pool,
            read_pool,
        }
    }

    pub fn single_pool(pool: Arc<PgPool>) -> Self {
        Self {
            write_pool: pool.clone(),
            read_pool: pool,
        }
    }

    #[instrument(name = "error_injection.list_all_with", skip_all, level = Level::DEBUG)]
    pub async fn list_all_with<'e, E>(&self, exec: E) -> Result<Vec<ErrorInjectionConfig>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let configs = sqlx::query_as!(
            ErrorInjectionConfig,
            r#"
            SELECT
                id,
                endpoint_pattern,
                http_method,
                error_rate,
                error_code,
                error_message,
                latency_ms
            FROM error_injection.config
            ORDER BY endpoint_pattern, http_method
            LIMIT 1000
            "#
        )
        .fetch_all(exec)
        .await?;

        Ok(configs)
    }

    #[instrument(
        name = "error_injection.list_for_method_with",
        skip_all,
        level = Level::DEBUG,
        fields(method = %method)
    )]
    pub async fn list_for_method_with<'e, E>(
        &self,
        exec: E,
        method: &str,
    ) -> Result<Vec<ErrorInjectionConfig>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let configs = sqlx::query_as!(
            ErrorInjectionConfig,
            r#"
            SELECT
                id,
                endpoint_pattern,
                http_method,
                error_rate,
                error_code,
                error_message,
                latency_ms
            FROM error_injection.config
            WHERE http_method = $1
            ORDER BY endpoint_pattern
            LIMIT 1000
            "#,
            method
        )
        .fetch_all(exec)
        .await?;

        Ok(configs)
    }

    #[instrument(
        name = "error_injection.create_with",
        skip_all,
        level = Level::DEBUG,
        fields(endpoint = %input.endpoint_pattern, method = %input.http_method)
    )]
    pub async fn create_with<'e, E>(
        &self,
        exec: E,
        input: ErrorInjectionConfigInput,
    ) -> Result<ErrorInjectionConfig>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let created = sqlx::query_as!(
            ErrorInjectionConfig,
            r#"
            INSERT INTO error_injection.config (
                endpoint_pattern,
                http_method,
                error_rate,
                error_code,
                error_message,
                latency_ms
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING
                id,
                endpoint_pattern,
                http_method,
                error_rate,
                error_code,
                error_message,
                latency_ms
            "#,
            input.endpoint_pattern,
            input.http_method,
            input.error_rate,
            input.error_code,
            input.error_message,
            input.latency_ms
        )
        .fetch_one(exec)
        .await?;

        Ok(created)
    }

    #[instrument(
        name = "error_injection.update_with",
        skip_all,
        level = Level::DEBUG,
        fields(config.id = id)
    )]
    pub async fn update_with<'e, E>(
        &self,
        exec: E,
        id: i32,
        input: ErrorInjectionConfigInput,
    ) -> Result<ErrorInjectionConfig>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let updated = sqlx::query_as!(
            ErrorInjectionConfig,
            r#"
            UPDATE error_injection.config
            SET
                endpoint_pattern = $2,
                http_method = $3,
                error_rate = $4,
                error_code = $5,
                error_message = $6,
                latency_ms = $7,
                updated_at = NOW()
            WHERE id = $1
            RETURNING
                id,
                endpoint_pattern,
                http_method,
                error_rate,
                error_code,
                error_message,
                latency_ms
            "#,
            id,
            input.endpoint_pattern,
            input.http_method,
            input.error_rate,
            input.error_code,
            input.error_message,
            input.latency_ms
        )
        .fetch_one(exec)
        .await?;

        Ok(updated)
    }

    #[instrument(
        name = "error_injection.delete_with",
        skip_all,
        level = Level::DEBUG,
        fields(config.id = id)
    )]
    pub async fn delete_with<'e, E>(&self, exec: E, id: i32) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"
            DELETE FROM error_injection.config
            WHERE id = $1
            "#,
            id
        )
        .execute(exec)
        .await?;

        Ok(())
    }

    #[instrument(name = "error_injection.list_all", skip(self), level = Level::DEBUG)]
    pub async fn list_all(&self) -> Result<Vec<ErrorInjectionConfig>> {
        self.list_all_with(&*self.read_pool).await
    }

    #[instrument(
        name = "error_injection.list_for_method",
        skip(self),
        level = Level::DEBUG,
        fields(method = %method)
    )]
    pub async fn list_for_method(&self, method: &str) -> Result<Vec<ErrorInjectionConfig>> {
        self.list_for_method_with(&*self.read_pool, method).await
    }

    #[instrument(name = "error_injection.create", skip(self), level = Level::DEBUG)]
    pub async fn create(&self, input: ErrorInjectionConfigInput) -> Result<ErrorInjectionConfig> {
        self.create_with(&*self.write_pool, input).await
    }

    #[instrument(name = "error_injection.update", skip(self), level = Level::DEBUG, fields(config.id = id))]
    pub async fn update(
        &self,
        id: i32,
        input: ErrorInjectionConfigInput,
    ) -> Result<ErrorInjectionConfig> {
        self.update_with(&*self.write_pool, id, input).await
    }

    #[instrument(name = "error_injection.delete", skip(self), level = Level::DEBUG, fields(config.id = id))]
    pub async fn delete(&self, id: i32) -> Result<()> {
        self.delete_with(&*self.write_pool, id).await
    }
}
