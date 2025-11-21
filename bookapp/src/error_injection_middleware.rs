use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{extract::Request, middleware::Next, response::IntoResponse, Extension, Json, Router};
use error_injection_dal::{
    ErrorInjectionConfig, ErrorInjectionConfigInput, ErrorInjectionRepository,
};
use hyper::StatusCode;
use matchit::Router as MatchRouter;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

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

/// Implementation of `ErrorInjectionConfigStore` trait using PostgreSQL via the DAL.
#[derive(Clone)]
pub struct PostgresErrorInjectionConfigStore {
    repo: ErrorInjectionRepository,
}

impl PostgresErrorInjectionConfigStore {
    /// Creates a new instance of `PostgresErrorInjectionConfigStore`.
    pub fn new(repo: ErrorInjectionRepository) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl ErrorInjectionConfigStore for PostgresErrorInjectionConfigStore {
    async fn get_all_configs(&self) -> anyhow::Result<Vec<ErrorInjectionConfig>> {
        Ok(self.repo.list_all().await?)
    }

    async fn get_configs_for_method(
        &self,
        method: &str,
    ) -> anyhow::Result<Vec<ErrorInjectionConfig>> {
        Ok(self.repo.list_for_method(method).await?)
    }

    async fn create_config(
        &self,
        input: ErrorInjectionConfigInput,
    ) -> anyhow::Result<ErrorInjectionConfig> {
        Ok(self.repo.create(input).await?)
    }

    async fn update_config(
        &self,
        id: i32,
        input: ErrorInjectionConfigInput,
    ) -> anyhow::Result<ErrorInjectionConfig> {
        Ok(self.repo.update(id, input).await?)
    }

    async fn delete_config(&self, id: i32) -> anyhow::Result<()> {
        self.repo.delete(id).await?;
        Ok(())
    }
}

/// Cached wrapper around ErrorInjectionConfigStore to reduce database load
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct CachedErrorInjectionConfigStore {
    inner: Arc<dyn ErrorInjectionConfigStore>,
    cache: Arc<RwLock<HashMap<String, (Vec<ErrorInjectionConfig>, Instant)>>>,
    cache_ttl: Duration,
}

impl CachedErrorInjectionConfigStore {
    pub fn new(inner: Arc<dyn ErrorInjectionConfigStore>) -> Self {
        Self {
            inner,
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(60), // 1 minute cache
        }
    }

    async fn is_cache_valid(&self, key: &str) -> bool {
        let cache = self.cache.read().await;
        if let Some((_, timestamp)) = cache.get(key) {
            timestamp.elapsed() < self.cache_ttl
        } else {
            false
        }
    }
}

#[async_trait]
impl ErrorInjectionConfigStore for CachedErrorInjectionConfigStore {
    async fn get_all_configs(&self) -> anyhow::Result<Vec<ErrorInjectionConfig>> {
        const CACHE_KEY: &str = "all_configs";

        if self.is_cache_valid(CACHE_KEY).await {
            let cache = self.cache.read().await;
            if let Some((configs, _)) = cache.get(CACHE_KEY) {
                return Ok(configs.clone());
            }
        }

        // Cache miss - fetch from database
        let configs = self.inner.get_all_configs().await?;

        // Update cache
        let mut cache = self.cache.write().await;
        cache.insert(CACHE_KEY.to_string(), (configs.clone(), Instant::now()));

        Ok(configs)
    }

    async fn get_configs_for_method(
        &self,
        method: &str,
    ) -> anyhow::Result<Vec<ErrorInjectionConfig>> {
        let cache_key = format!("method_{}", method);

        if self.is_cache_valid(&cache_key).await {
            let cache = self.cache.read().await;
            if let Some((configs, _)) = cache.get(&cache_key) {
                return Ok(configs.clone());
            }
        }

        // Cache miss - fetch from database
        let configs = self.inner.get_configs_for_method(method).await?;

        // Update cache
        let mut cache = self.cache.write().await;
        cache.insert(cache_key, (configs.clone(), Instant::now()));

        Ok(configs)
    }

    async fn create_config(
        &self,
        input: ErrorInjectionConfigInput,
    ) -> anyhow::Result<ErrorInjectionConfig> {
        let result = self.inner.create_config(input).await?;

        // Invalidate cache on write operations
        let mut cache = self.cache.write().await;
        cache.clear();

        Ok(result)
    }

    async fn update_config(
        &self,
        id: i32,
        input: ErrorInjectionConfigInput,
    ) -> anyhow::Result<ErrorInjectionConfig> {
        let result = self.inner.update_config(id, input).await?;

        // Invalidate cache on write operations
        let mut cache = self.cache.write().await;
        cache.clear();

        Ok(result)
    }

    async fn delete_config(&self, id: i32) -> anyhow::Result<()> {
        let result = self.inner.delete_config(id).await;

        // Invalidate cache on write operations
        let mut cache = self.cache.write().await;
        cache.clear();

        result
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
    if let Err(err) = validate_endpoint_pattern(&config.endpoint_pattern) {
        tracing::warn!(?err, pattern = %config.endpoint_pattern, "Invalid error injection pattern");
        return Err(StatusCode::BAD_REQUEST);
    }

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
    if let Err(err) = validate_endpoint_pattern(&config.endpoint_pattern) {
        tracing::warn!(?err, pattern = %config.endpoint_pattern, id, "Invalid error injection pattern");
        return Err(StatusCode::BAD_REQUEST);
    }

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
/// use error_injection_dal::ErrorInjectionRepository;
/// use bookapp::error_injection_middleware::{error_injection_middleware, PostgresErrorInjectionConfigStore, ErrorInjectionConfigStore};
/// use bookapp::database::DatabasePools;
///
/// fn router(connection_pools: DatabasePools) -> Router {
///     // Create the ErrorInjectionConfigStore
///     let error_injection_store: Arc<dyn ErrorInjectionConfigStore> = Arc::new(
///         PostgresErrorInjectionConfigStore::new(ErrorInjectionRepository::new(
///             connection_pools.write_pool.clone(),
///             connection_pools.read_pool.clone(),
///         ))
///     );
///
///     Router::new()
///         .layer(Extension(connection_pools))
///         .layer(axum::middleware::from_fn_with_state(error_injection_store, error_injection_middleware))
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
    tracing::debug!(path = %path, method = %method, "Checking latency injection config");

    // Query the store for matching error injection configurations
    if let Some(config) = get_matching_error_injection_config(store, &path, &method).await {
        tracing::Span::current().record("error_rate", config.error_rate);

        // Inject latency if configured
        if let Some(latency_ms) = config.latency_ms {
            if latency_ms > 0 {
                tracing::debug!(
                    path = path,
                    method = method,
                    latency_ms = latency_ms,
                    "Injecting latency"
                );
                tokio::time::sleep(Duration::from_millis(latency_ms as u64)).await;
            }
        }

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

            // Use tracing::error! for proper Sentry integration
            // This will be automatically captured by the Sentry tracing layer
            tracing::error!(
                error_type = "injected_error",
                endpoint = path.as_str(),
                method = method.as_str(),
                status_code = config.error_code,
                error_rate = config.error_rate,
                "Error injection triggered: {}",
                body
            );

            // Test Sentry structured logging capabilities
            // This demonstrates the enhanced log capture with filtering
            sentry::with_scope(
                |scope| {
                    scope.set_tag("error_type", "injected_error");
                    scope.set_tag("endpoint", &path);
                    scope.set_tag("method", &method);
                    scope.set_extra("status_code", config.error_code.into());
                    scope.set_extra("error_rate", config.error_rate.into());
                },
                || {
                    // Use structured logging with Sentry capture
                    tracing::info!(
                        injected_error = true,
                        endpoint = %path,
                        method = %method,
                        status = config.error_code,
                        "Testing Sentry structured log capture"
                    );
                },
            );

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
    let mut patterns = Vec::with_capacity(configs.len());

    for config in configs {
        tracing::debug!(pattern = %config.endpoint_pattern, "Registering latency pattern");
        patterns.push(config.endpoint_pattern.clone());
        if let Err(err) = router.insert(config.endpoint_pattern.as_str(), config.clone()) {
            tracing::warn!(
                ?err,
                pattern = %config.endpoint_pattern,
                http_method = %method,
                "Failed to register latency pattern"
            );
        }
    }

    match router.at(path) {
        Ok(matched) => {
            let config = matched.value.clone();
            tracing::info!(config = ?config, "Latency config matched");
            Some(config)
        }
        Err(err) => {
            tracing::warn!(
                ?err,
                path = %path,
                http_method = %method,
                available_patterns = ?patterns,
                "Latency config lookup failed"
            );
            None
        }
    }
}

fn validate_endpoint_pattern(pattern: &str) -> Result<(), matchit::InsertError> {
    let mut router = MatchRouter::<()>::new();
    router.insert(pattern, ())
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
