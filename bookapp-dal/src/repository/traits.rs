use crate::error::Result;
use crate::models::{Book, BookCreateInput, BookFilterParams, BookSearchParams, BookStatus};
use async_trait::async_trait;

#[async_trait]
pub trait BookRepository: Send + Sync {
    async fn find_all(&self) -> Result<Vec<Book>>;
    async fn find_by_id(&self, id: i32) -> Result<Option<Book>>;
    async fn create(&self, input: BookCreateInput) -> Result<i32>;
    async fn update(&self, book: Book) -> Result<i32>;
    async fn delete(&self, id: i32) -> Result<()>;
    async fn bulk_create(&self, books: &[BookCreateInput]) -> Result<Vec<i32>>;
    async fn find_by_filters(&self, params: BookFilterParams) -> Result<Vec<Book>>;
    async fn search_books(&self, params: BookSearchParams) -> Result<Vec<Book>>;
    async fn update_status(&self, id: i32, status: BookStatus) -> Result<bool>;
}