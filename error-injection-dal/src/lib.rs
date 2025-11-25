pub mod models;
pub mod repository;

pub use models::{ErrorInjectionConfig, ErrorInjectionConfigInput};
pub use repository::ErrorInjectionRepository;

/// Type alias for the traced PostgreSQL pool (sqlx-tracing wrapper)
pub type TracedPgPool = sqlx_tracing::Pool<sqlx::Postgres>;

// Re-export raw PgPool for transactions and migrations
pub use sqlx::PgPool;
