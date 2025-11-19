use sqlx::Error as SqlxError;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum DalError {
    #[error("Book not found: {id}")]
    BookNotFound { id: Uuid },

    #[error("Unique constraint violation on {table} ({constraint})")]
    AlreadyExists { table: String, constraint: String },

    #[error("Foreign key violation on constraint: {constraint}")]
    ForeignKeyViolation { constraint: String },

    #[error("Database error: {0}")]
    Database(SqlxError),

    #[error("Connection pool error")]
    ConnectionPool,

    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
}

impl From<SqlxError> for DalError {
    fn from(error: SqlxError) -> Self {
        if let Some(db_err) = error.as_database_error() {
            if let Some(code) = db_err.code().as_deref() {
                match code {
                    "23505" => {
                        let table = db_err.table().unwrap_or("unknown_table").to_string();
                        let constraint = db_err
                            .constraint()
                            .unwrap_or("unknown_constraint")
                            .to_string();
                        return DalError::AlreadyExists { table, constraint };
                    }
                    "23503" => {
                        let constraint = db_err.constraint().unwrap_or("unknown").to_string();
                        return DalError::ForeignKeyViolation { constraint };
                    }
                    _ => {}
                }
            }
        }
        DalError::Database(error)
    }
}

pub type Result<T> = std::result::Result<T, DalError>;
