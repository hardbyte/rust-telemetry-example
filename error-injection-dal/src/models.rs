use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ErrorInjectionConfig {
    pub id: i32,
    pub endpoint_pattern: String,
    pub http_method: String,
    pub error_rate: f64,
    pub error_code: i32,
    pub error_message: Option<String>,
    pub latency_ms: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInjectionConfigInput {
    pub endpoint_pattern: String,
    pub http_method: String,
    pub error_rate: f64,
    pub error_code: i32,
    pub error_message: Option<String>,
    pub latency_ms: Option<i32>,
}
