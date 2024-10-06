use axum::{extract::Request, middleware::Next, response::IntoResponse, Extension, Json, Router};
use axum::extract::Path;
use hyper::StatusCode;
use rand::Rng;
use sqlx::{FromRow, PgPool};

use serde::{Deserialize, Serialize};
use axum::{
    routing::{get, post, put, delete},
};

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
struct ErrorInjectionConfigInput {
    endpoint_pattern: String,
    http_method: String,
    error_rate: f64,
    error_code: i32,
    error_message: Option<String>,
}


pub async fn get_all_error_configs(connection_pool: &PgPool) -> anyhow::Result<Vec<ErrorInjectionConfig>> {
    let configs: Vec<ErrorInjectionConfig> = sqlx::query_as(
        r#"
        SELECT id, endpoint_pattern, http_method, error_rate, error_code, error_message
        FROM error_injection_config
        LIMIT 1000
        "#
    )
        .fetch_all(connection_pool)
        .await?;

    Ok(configs)
}

pub async fn get_error_configs_for_method(connection_pool: &PgPool, method: &str) -> anyhow::Result<Vec<ErrorInjectionConfig>> {
    let configs: Vec<ErrorInjectionConfig> = sqlx::query_as(
        r#"
        SELECT id, endpoint_pattern, http_method, error_rate, error_code, error_message
        FROM error_injection_config
        WHERE http_method = $1
        LIMIT 100
        "#
    )
        .bind(method)
        .fetch_all(connection_pool)
        .await?;

    Ok(configs)
}


async fn get_all_configs_handler(
    Extension(pool): Extension<PgPool>,
) -> Result<Json<Vec<ErrorInjectionConfig>>, StatusCode> {
    let configs = get_all_error_configs(&pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(configs))
}


async fn create_config(
    Extension(pool): Extension<PgPool>,
    Json(config): Json<ErrorInjectionConfigInput>,
) -> Result<Json<ErrorInjectionConfig>, StatusCode> {
    let inserted_config = sqlx::query_as::<_, ErrorInjectionConfig>(
        r#"
        INSERT INTO error_injection_config (endpoint_pattern, http_method, error_rate, error_code, error_message)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, endpoint_pattern, http_method, error_rate, error_code, error_message
        "#
    )
        .bind(config.endpoint_pattern)
        .bind(config.http_method)
        .bind(config.error_rate)
        .bind(config.error_code)
        .bind(config.error_message)
        .fetch_one(&pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(inserted_config))
}

async fn update_config(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    Json(config): Json<ErrorInjectionConfigInput>,
) -> Result<Json<ErrorInjectionConfig>, StatusCode> {
    let updated_config = sqlx::query_as::<_, ErrorInjectionConfig>(

        r#"
        UPDATE error_injection_config
        SET endpoint_pattern = $2, http_method = $3, error_rate = $4, error_code = $5, error_message = $6
        WHERE id = $1
        RETURNING id, endpoint_pattern, http_method, error_rate, error_code, error_message
        "#)
        .bind(id)
        .bind(config.endpoint_pattern)
        .bind(config.http_method)
        .bind(config.error_rate)
        .bind(config.error_code)
        .bind(config.error_message
    )
        .fetch_one(&pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(updated_config))
}

async fn delete_config(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query(
        r#"
        DELETE FROM error_injection_config WHERE id = $1
        "#
    )
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::NO_CONTENT)
}



pub async fn error_injection_middleware(
    req: Request,
    next: Next,
) -> impl IntoResponse {
    let path = req.uri().path().to_string();
    let method = req.method().as_str().to_string();

    // Get the database connection pool from the request extensions
    let pool = if let Some(pool) = req.extensions().get::<PgPool>() {
        pool.clone()
    } else {
        // Proceed if no pool found (should not happen)
        return next.run(req).await;
    };

    // Query the database for matching error injection configurations
    if let Some(config) = get_matching_error_injection_config(&pool, &path, &method).await {
        // Generate a random number between 0.0 and 1.0
        let mut rng = rand::thread_rng();
        let random_value: f64 = rng.gen();

        if random_value < config.error_rate {
            // Inject an error
            let status_code = StatusCode::from_u16(config.error_code as u16)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let body = config
                .error_message
                .unwrap_or_else(|| status_code.canonical_reason().unwrap_or("Error").to_string());
            return (status_code, body).into_response();
        }
    }

    // Run the next middleware or handler
    next.run(req).await
}

async fn get_matching_error_injection_config(
    pool: &PgPool,
    path: &str,
    method: &str,
) -> Option<ErrorInjectionConfig> {

    // Fetch all configurations for the given HTTP method
    let configs = get_error_configs_for_method(pool, method)
        .await
        .ok()?;


    // Use matchit crate for path matching
    let mut router = matchit::Router::new();

    for config in configs {
        // Add the endpoint_pattern to the router
        let _ = router.insert(&config.endpoint_pattern, config.clone());
    }

    if let Ok(matched) = router.at(path) {
        let config = matched.value.clone();
        Some(config)
    } else {
        None
    }
}

pub fn error_injection_service() -> Router {
    Router::new()
        .route("/", get(get_all_configs_handler).post(create_config))
        .route("/:id", put(update_config).delete(delete_config))
}