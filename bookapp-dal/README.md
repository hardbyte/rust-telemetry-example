# BookApp Data Access Layer (DAL)

A dedicated Rust crate providing database access functionality for the BookApp telemetry example. This crate implements the repository pattern with async traits, compile-time SQL verification, and comprehensive error handling.

## Features

- **Repository Pattern**: Clean separation between business logic and data access
- **Async/Await Support**: Built on `sqlx` with full async support
- **Compile-time SQL Verification**: Uses SQLx macros for type-safe, verified queries
- **Custom Error Types**: Structured error handling with `DalError`
- **Advanced Filtering**: Dynamic queries with multiple filter combinations
- **Bulk Operations**: Efficient multi-row database operations
- **PostgreSQL Integration**: Native PostgreSQL enum and type support
- **Read/Write Pool Separation**: Supports separate pools for read replicas and write masters
- **Dependency Injection**: Pools are injected from application layer, not created by DAL
- **Comprehensive Testing**: Full test coverage with `#[sqlx::test]`

## Architecture

### Models (`models.rs`)

Core domain models representing the book entity and related types:

```rust
pub struct Book {
    pub id: i32,
    pub title: String,
    pub author: String,
    pub status: BookStatus,
}

pub struct BookCreateInput {
    pub title: String,
    pub author: String,
    pub status: Option<BookStatus>,
}

#[derive(Default)]
pub enum BookStatus {
    #[default]
    Available,
    Borrowed,
    Lost,
}
```

### Repository Pattern (`repository/`)

Async traits provide a clean abstraction over database operations:

```rust
#[async_trait]
pub trait BookRepository: Send + Sync {
    async fn create(&self, input: BookCreateInput) -> Result<i32>;
    async fn get_by_id(&self, id: i32) -> Result<Book>;
    async fn get_all(&self) -> Result<Vec<Book>>;
    async fn update(&self, book: Book) -> Result<i32>;
    async fn delete(&self, id: i32) -> Result<Book>;
    async fn bulk_create(&self, inputs: &[BookCreateInput]) -> Result<Vec<i32>>;
    async fn find_by_filters(&self, params: BookFilterParams) -> Result<Vec<Book>>;
}
```

### Error Handling (`error.rs`)

Structured error types using `thiserror`:

```rust
#[derive(Error, Debug)]
pub enum DalError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    
    #[error("Book not found with id: {id}")]
    BookNotFound { id: i32 },
    
    #[error("Validation error: {message}")]
    Validation { message: String },
}
```

## Usage

### Repository Initialization

The DAL receives connection pools from the application layer and supports read/write separation:

```rust
use bookapp_dal::{BookRepositoryImpl};
use std::sync::Arc;
use sqlx::PgPool;

// Single pool for both read and write operations (development/testing)
let pool = Arc::new(pg_pool);
let repo = BookRepositoryImpl::single_pool(pool);

// Separate read and write pools (production with read replicas)
let write_pool = Arc::new(write_pg_pool);
let read_pool = Arc::new(read_pg_pool);
let repo = BookRepositoryImpl::new(write_pool, read_pool);
```

### Basic Operations

```rust
use bookapp_dal::{BookCreateInput, BookStatus};

// Create a book
let input = BookCreateInput {
    title: "The Rust Programming Language".to_string(),
    author: "Steve Klabnik".to_string(),
    status: Some(BookStatus::Available),
};
let book_id = repo.create(input).await?;

// Get a book
let book = repo.get_by_id(book_id).await?;

// Update a book
let mut updated_book = book;
updated_book.status = BookStatus::Borrowed;
let rows_affected = repo.update(updated_book).await?;

// Delete a book
let deleted_book = repo.delete(book_id).await?;
```

### Advanced Filtering

```rust
use bookapp_dal::BookFilterParams;

// Search with multiple filters
let params = BookFilterParams {
    status: Some(BookStatus::Available),
    author_pattern: Some("Klabnik".to_string()),
    title_pattern: Some("Rust".to_string()),
    limit: Some(10),
    offset: Some(0),
};
let books = repo.find_by_filters(params).await?;
```

### Bulk Operations

```rust
let inputs = vec![
    BookCreateInput {
        title: "Book 1".to_string(),
        author: "Author 1".to_string(),
        status: None, // Defaults to Available
    },
    BookCreateInput {
        title: "Book 2".to_string(),
        author: "Author 2".to_string(),
        status: Some(BookStatus::Borrowed),
    },
];

let book_ids = repo.bulk_create(&inputs).await?;
```

## Database Schema

The DAL manages the following PostgreSQL schema:

```sql
-- Book status enum
CREATE TYPE book_status AS ENUM ('available', 'borrowed', 'lost');

-- Main books table
CREATE TABLE books (
    id SERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    author TEXT NOT NULL,
    status book_status NOT NULL DEFAULT 'available'
);
```

## Migrations

Database migrations are managed through `sqlx migrate`:

```bash
# Run migrations
cd bookapp-dal
sqlx migrate run

# Add new migration
sqlx migrate add create_new_table

# Revert last migration
sqlx migrate revert
```

### Migration Files

Located in `bookapp-dal/migrations/`:
- `20240101000000_create_books_table.sql`: Initial schema setup

## Testing

The crate includes comprehensive test coverage using `#[sqlx::test]`:

```bash
# Run all tests
cargo test --package bookapp-dal

# Run specific test
cargo test --package bookapp-dal test_create_book

# Run tests with output
cargo test --package bookapp-dal -- --nocapture
```

### Test Features

- **Transactional Tests**: Each test runs in its own transaction
- **Automatic Setup**: Database schema applied automatically
- **Isolated Environment**: Tests don't interfere with each other
- **Comprehensive Coverage**: All repository methods tested

## Compile-time Query Verification

SQLx provides compile-time verification of SQL queries. When modifying queries:

1. Ensure database is running:
   ```bash
   docker compose up -d db
   ```

2. Set connection string:
   ```bash
   export DATABASE_URL="postgres://postgres:password@localhost:5432/bookapp"
   ```

3. Run migrations:
   ```bash
   cd bookapp-dal
   sqlx migrate run
   ```

4. Prepare query metadata:
   ```bash
   cargo sqlx prepare
   ```

5. Commit the updated `.sqlx/` directory

## Configuration

### Environment Variables

- `DATABASE_URL`: PostgreSQL connection string (required)
- `DATABASE_MAX_CONNECTIONS`: Connection pool size (default: 10)
- `DATABASE_CONNECT_TIMEOUT`: Connection timeout in seconds (default: 30)

### Connection Pool Settings

```rust
use bookapp_dal::{create_pool_with_config, PoolConfig};

let config = PoolConfig {
    max_connections: 20,
    connect_timeout: Duration::from_secs(60),
    idle_timeout: Some(Duration::from_secs(300)),
};

let pool = create_pool_with_config(&database_url, config).await?;
```

## Integration with BookApp

The DAL integrates seamlessly with the main BookApp service:

```rust
// In bookapp/src/db.rs
pub use bookapp_dal::{
    create_pool as init_db,
    Book, BookCreateInput as BookCreateIn, BookRepository, 
    BookRepositoryImpl, BookStatus, DalError, PgPool,
};

// Legacy wrapper functions for backward compatibility
pub async fn get_all_books(pool: &PgPool) -> Result<Vec<Book>, anyhow::Error> {
    let repo = BookRepositoryImpl::new(pool.clone());
    Ok(repo.get_all().await?)
}
```

## Performance Considerations

- **Connection Pooling**: Configured for optimal concurrent access
- **Prepared Statements**: All queries use prepared statements via SQLx
- **Bulk Operations**: Efficient batch processing for multiple records
- **Indexing**: Database indexes on frequently queried columns
- **Async Design**: Non-blocking operations throughout

## Error Handling Best Practices

1. **Structured Errors**: Use `DalError` for database-specific errors
2. **Context Preservation**: Errors include relevant context (IDs, messages)
3. **Conversion Support**: Automatic conversion to `anyhow::Error`
4. **Logging Integration**: Errors include details for observability

## Dependencies

Core dependencies and their versions:

```toml
[dependencies]
anyhow = "1.0.98"
async-trait = "0.1.84"
serde = { version = "1.0.219", features = ["derive"] }
sqlx = { version = "0.8.6", features = ["runtime-tokio", "postgres", "macros", "migrate"] }
thiserror = "1.0.69"
tracing = "0.1.41"

[dev-dependencies]
tokio-test = "0.4.4"
```

## Contributing

When contributing to the DAL:

1. Follow the repository pattern for new operations
2. Add comprehensive tests for new functionality
3. Update `.sqlx/` metadata when modifying queries
4. Document public APIs with examples
5. Consider performance implications of new queries

## License

This crate is part of the Rust Telemetry Example project and follows the same licensing terms.