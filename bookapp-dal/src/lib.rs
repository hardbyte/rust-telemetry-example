pub mod error;
pub mod models;
pub mod repository;

// Re-export commonly used types
pub use error::{DalError, Result};
pub use models::*;
pub use repository::BookRepositoryImpl;

// Re-export sqlx types for convenience
pub use sqlx::PgPool;

// Re-export traced pool type for OpenTelemetry instrumentation
pub use sqlx_tracing::Pool as TracedPool;

/// Type alias for traced PostgreSQL pool
pub type TracedPgPool = TracedPool<sqlx::Postgres>;
