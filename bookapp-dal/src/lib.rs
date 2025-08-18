pub mod error;
pub mod models;
pub mod repository;

// Re-export commonly used types
pub use error::{DalError, Result};
pub use models::*;
pub use repository::{BookRepository, BookRepositoryImpl};

// Re-export sqlx::PgPool for convenience
pub use sqlx::PgPool;
