use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{extract::Request, middleware::Next, response::IntoResponse, Extension, Json, Router};
use hyper::StatusCode;
use matchit::Router as MatchRouter;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize, FromRow)]
pub struct ErrorInjectionConfig {
    id: i32,
    /// The endpoint pattern to match (e.g., "/books/:id").
    endpoint_pattern: String,
    /// The HTTP method to match (e.g., "GET", "POST").
    http_method: String,
    /// The rate at which to inject errors (between 0.0 and 1.0).
    error_rate: f64,
    /// The HTTP status code to return when injecting an error.
    error_code: i32,
    /// Optional custom error message to return.
    error_message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorInjectionConfigInput {
    endpoint_pattern: String,
    http_method: String,
    error_rate: f64,
    error_code: i32,
    error_message: Option<String>,
}

/// Trait that defines the storage interface for error injection configurations.
///
/// This allows for different storage backends (e.g., PostgreSQL, in-memory, etc.)
#[async_trait]
pub trait ErrorInjectionConfigStore: Send + Sync + 'static {
    /// Retrieves all error injection configurations.
    async fn get_all_configs(&self) -> anyhow::Result<Vec<ErrorInjectionConfig>>;

    /// Retrieves all error injection configurations for a specific HTTP method.
    ///
    /// # Arguments
    ///
    /// * `method` - The HTTP method to filter configurations by (e.g., "GET").
    async fn get_configs_for_method(
        &self,
        method: &str,
    ) -> anyhow::Result<Vec<ErrorInjectionConfig>>;

    /// Creates a new error injection configuration.
    ///
    /// # Arguments
    ///
    /// * `input` - The input data for the new configuration.
    async fn create_config(
        &self,
        input: ErrorInjectionConfigInput,
    ) -> anyhow::Result<ErrorInjectionConfig>;

    /// Updates an existing error injection configuration.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the configuration to update.
    /// * `input` - The updated data for the configuration.
    async fn update_config(
        &self,
        id: i32,
        input: ErrorInjectionConfigInput,
    ) -> anyhow::Result<ErrorInjectionConfig>;

    /// Deletes an error injection configuration.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the configuration to delete.
    async fn delete_config(&self, id: i32) -> anyhow::Result<()>;
}

/// Implementation of `ErrorInjectionConfigStore` trait using PostgreSQL as the storage backend.
#[derive(Clone)]
pub struct PostgresErrorInjectionConfigStore {
    /// The PostgreSQL connection pool.
    pool: PgPool,
}

impl PostgresErrorInjectionConfigStore {
    /// Creates a new instance of `PostgresErrorInjectionConfigStore`.
    ///
    /// # Arguments
    ///
    /// * `pool` - The PostgreSQL connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ErrorInjectionConfigStore for PostgresErrorInjectionConfigStore {
    async fn get_all_configs(&self) -> anyhow::Result<Vec<ErrorInjectionConfig>> {
        let configs: Vec<ErrorInjectionConfig> = sqlx::query_as(
            r#"
            SELECT id, endpoint_pattern, http_method, error_rate, error_code, error_message
            FROM error_injection_config
            LIMIT 1000
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(configs)
    }

    async fn get_configs_for_method(
        &self,
        method: &str,
    ) -> anyhow::Result<Vec<ErrorInjectionConfig>> {
        let configs: Vec<ErrorInjectionConfig> = sqlx::query_as(
            r#"
            SELECT id, endpoint_pattern, http_method, error_rate, error_code, error_message
            FROM error_injection_config
            WHERE http_method = $1
            LIMIT 100
            "#,
        )
        .bind(method)
        .fetch_all(&self.pool)
        .await?;

        Ok(configs)
    }

    async fn create_config(
        &self,
        input: ErrorInjectionConfigInput,
    ) -> anyhow::Result<ErrorInjectionConfig> {
        let inserted_config = sqlx::query_as::<_, ErrorInjectionConfig>(
            r#"
            INSERT INTO error_injection_config (endpoint_pattern, http_method, error_rate, error_code, error_message)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, endpoint_pattern, http_method, error_rate, error_code, error_message
            "#
        )
            .bind(input.endpoint_pattern)
            .bind(input.http_method)
            .bind(input.error_rate)
            .bind(input.error_code)
            .bind(input.error_message)
            .fetch_one(&self.pool)
            .await?;

        Ok(inserted_config)
    }

    async fn update_config(
        &self,
        id: i32,
        input: ErrorInjectionConfigInput,
    ) -> anyhow::Result<ErrorInjectionConfig> {
        let updated_config = sqlx::query_as::<_, ErrorInjectionConfig>(
            r#"
            UPDATE error_injection_config
            SET endpoint_pattern = $2, http_method = $3, error_rate = $4, error_code = $5, error_message = $6
            WHERE id = $1
            RETURNING id, endpoint_pattern, http_method, error_rate, error_code, error_message
            "#
        )
            .bind(id)
            .bind(input.endpoint_pattern)
            .bind(input.http_method)
            .bind(input.error_rate)
            .bind(input.error_code)
            .bind(input.error_message)
            .fetch_one(&self.pool)
            .await?;

        Ok(updated_config)
    }

    async fn delete_config(&self, id: i32) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            DELETE FROM error_injection_config WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

/// Handler to retrieve all error injection configurations.
///
/// GET /error-injection-configs
pub async fn get_all_configs_handler(
    Extension(store): Extension<Arc<dyn ErrorInjectionConfigStore>>,
) -> Result<Json<Vec<ErrorInjectionConfig>>, StatusCode> {
    let configs = store
        .get_all_configs()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(configs))
}

/// Handler to create a new error injection configuration.
///
/// POST /error-injection-configs
///
/// # Request Body
///
/// JSON representation of `ErrorInjectionConfigInput`.
#[tracing::instrument(skip_all)]
pub async fn create_config(
    Extension(store): Extension<Arc<dyn ErrorInjectionConfigStore>>,
    Json(config): Json<ErrorInjectionConfigInput>,
) -> Result<Json<ErrorInjectionConfig>, StatusCode> {
    let inserted_config = store
        .create_config(config)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(inserted_config))
}

/// Handler to update an existing error injection configuration.
///
/// PUT /error-injection-configs/:id
///
/// # Path Parameters
///
/// * `id` - The ID of the configuration to update.
///
/// # Request Body
///
/// JSON representation of `ErrorInjectionConfigInput`.
#[tracing::instrument(skip_all, fields(id))]
pub async fn update_config(
    Extension(store): Extension<Arc<dyn ErrorInjectionConfigStore>>,
    Path(id): Path<i32>,
    Json(config): Json<ErrorInjectionConfigInput>,
) -> Result<Json<ErrorInjectionConfig>, StatusCode> {
    let updated_config = store
        .update_config(id, config)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(updated_config))
}

/// Handler to delete an error injection configuration.
///
/// DELETE /error-injection-configs/:id
///
/// # Path Parameters
///
/// * `id` - The ID of the configuration to delete.
#[tracing::instrument(skip_all, fields(id))]
pub async fn delete_config(
    Extension(store): Extension<Arc<dyn ErrorInjectionConfigStore>>,
    Path(id): Path<i32>,
) -> Result<StatusCode, StatusCode> {
    store
        .delete_config(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Middleware that injects errors into requests based on the error injection configurations.
///
/// This middleware intercepts incoming requests, checks if there is a matching error injection configuration,
/// and, based on the error rate, may inject an error response.
///
/// # Example Usage
///
/// ```rust
/// use std::sync::Arc;
/// use axum::{Router, Extension};
/// use sqlx::PgPool;
/// use middleware::{error_injection_middleware, PostgresErrorInjectionConfigStore};
///
/// fn router(connection_pool: PgPool) -> Router {
///     // Create the ErrorInjectionConfigStore
///     let error_injection_store: Arc<dyn ErrorInjectionConfigStore> = Arc::new(
///         PostgresErrorInjectionConfigStore::new(connection_pool.clone())
///     );
///
///     Router::new()
///         .layer(Extension(connection_pool))
///         .layer(Extension(error_injection_store))
///         .layer(axum::middleware::from_fn(error_injection_middleware))
///         // ... other routes and layers ...
/// }
/// ```
///
/// # Arguments
///
/// * `req` - The incoming request.
/// * `next` - The next middleware or handler in the chain.
///
/// # Returns
///
/// Either an error response or the result of the next middleware/handler.
#[tracing::instrument(skip_all,
    fields(
        method = req.method().to_string(),
        path = req.uri().path().to_string(),
        error_rate,
    )
)]
pub async fn error_injection_middleware(
    State(store): State<Arc<dyn ErrorInjectionConfigStore>>,
    req: Request,
    next: Next,
) -> impl IntoResponse {
    let path = req.uri().path().to_string();
    let method = req.method().as_str().to_string();

    // Query the store for matching error injection configurations
    if let Some(config) = get_matching_error_injection_config(store, &path, &method).await {
        tracing::Span::current().record("error_rate", &config.error_rate);

        // Generate a random number between 0.0 and 1.0
        let mut rng = rand::rng();
        let random_value: f64 = rng.random();

        if random_value < config.error_rate {
            tracing::debug!(
                path = path,
                method = method,
                injected_status_code = config.error_code,
                "Injecting an error"
            );
            let status_code = StatusCode::from_u16(config.error_code as u16)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let body = config.error_message.unwrap_or_else(|| {
                status_code
                    .canonical_reason()
                    .unwrap_or("Injected Error")
                    .to_string()
            });
            return (status_code, body).into_response();
        }
    } else {
        tracing::trace!(
            path = path,
            method = method,
            "No error injection configured for this endpoint"
        );
    }

    // Run the next middleware or handler
    next.run(req).await
}

/// Retrieves a matching error injection configuration for the given path and method.
///
/// # Arguments
///
/// * `store` - The error injection configuration store.
/// * `path` - The request path.
/// * `method` - The HTTP method.
///
/// # Returns
///
/// An `Option<ErrorInjectionConfig>` that matches the request.
#[tracing::instrument(skip(store), fields(
    num_configs = tracing::field::Empty
))]
async fn get_matching_error_injection_config(
    store: Arc<dyn ErrorInjectionConfigStore>,
    path: &str,
    method: &str,
) -> Option<ErrorInjectionConfig> {
    // Fetch all configurations for the given HTTP method
    let configs = store.get_configs_for_method(method).await.ok()?;
    tracing::Span::current().record("num_configs", configs.len());

    // Use matchit crate for path matching
    let mut router = MatchRouter::new();

    for config in configs {
        // Add the endpoint_pattern to the router
        let _ = router.insert(&config.endpoint_pattern, config.clone());
    }

    if let Ok(matched) = router.at(path) {
        let config = matched.value.clone();
        tracing::trace!(config = ?config, "There was a matching error injection config");
        Some(config)
    } else {
        None
    }
}

/// Creates a router for the error injection configuration service.
///
/// The service provides endpoints to manage error injection configurations:
///
/// - GET `/error-injection-configs`
/// - POST `/error-injection-configs`
/// - PUT `/error-injection-configs/:id`
/// - DELETE `/error-injection-configs/:id`
///
/// # Returns
///
/// A `Router` instance with the configured routes.
pub fn error_injection_service(
    error_injection_store: Arc<dyn ErrorInjectionConfigStore>,
) -> Router {
    Router::new()
        .route("/", get(get_all_configs_handler).post(create_config))
        .route("/{id}", put(update_config).delete(delete_config))
        .layer(Extension(error_injection_store))
}
