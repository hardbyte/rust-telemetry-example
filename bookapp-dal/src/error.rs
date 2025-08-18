use thiserror::Error;

#[derive(Debug, Error)]
pub enum DalError {
    #[error("Book not found: {id}")]
    BookNotFound { id: i32 },
    
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    
    #[error("Connection pool error")]
    ConnectionPool,
    
    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
}

pub type Result<T> = std::result::Result<T, DalError>;