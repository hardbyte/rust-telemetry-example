use crate::error::Result;
use crate::models::{
    Author, AuthorCreateInput, Book, BookCreateInput, BookFilterParams, BookSearchParams,
    BookSearchResult, Edition, EditionCreateInput, Event, EventCreateInput, Series,
    SeriesCreateInput, SeriesWorksAssociation, SeriesWorksAssociationCreateInput, Work, WorkAuthor,
    WorkAuthorCreateInput, WorkCreateInput,
};
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
    async fn full_text_search(&self, query: &str, limit: i64) -> Result<Vec<BookSearchResult>>;
}

#[async_trait]
pub trait AuthorRepository: Send + Sync {
    async fn create(&self, input: AuthorCreateInput) -> Result<i32>;
    async fn find_by_id(&self, id: i32) -> Result<Option<Author>>;
    async fn find_all(&self) -> Result<Vec<Author>>;
    async fn update(&self, author: Author) -> Result<i32>;
    async fn delete(&self, id: i32) -> Result<()>;
}

#[async_trait]
pub trait WorkRepository: Send + Sync {
    async fn create(&self, input: WorkCreateInput) -> Result<i32>;
    async fn find_by_id(&self, id: i32) -> Result<Option<Work>>;
    async fn find_all(&self) -> Result<Vec<Work>>;
    async fn update(&self, work: Work) -> Result<i32>;
    async fn delete(&self, id: i32) -> Result<()>;

    // Associations
    async fn add_author(&self, assoc: WorkAuthorCreateInput) -> Result<()>;
    async fn list_authors(&self, work_id: i32) -> Result<Vec<WorkAuthor>>;
}

#[async_trait]
pub trait EditionRepository: Send + Sync {
    async fn create(&self, input: EditionCreateInput) -> Result<i32>;
    async fn find_by_id(&self, id: i32) -> Result<Option<Edition>>;
    async fn find_by_isbn(&self, isbn: &str) -> Result<Option<Edition>>;
    async fn list_by_work(&self, work_id: i32) -> Result<Vec<Edition>>;
    async fn delete(&self, id: i32) -> Result<()>;
}

#[async_trait]
pub trait SeriesRepository: Send + Sync {
    async fn create(&self, input: SeriesCreateInput) -> Result<i32>;
    async fn find_by_id(&self, id: i32) -> Result<Option<Series>>;
    async fn find_all(&self) -> Result<Vec<Series>>;
    async fn delete(&self, id: i32) -> Result<()>;

    // Associations
    async fn add_work(&self, assoc: SeriesWorksAssociationCreateInput) -> Result<()>;
    async fn list_works(&self, series_id: i32) -> Result<Vec<SeriesWorksAssociation>>;
    async fn remove_work(&self, series_id: i32, work_id: i32) -> Result<()>;
}

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn append(&self, input: EventCreateInput) -> Result<i64>;
    async fn list_for_aggregate(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<Event>>;
    async fn list_by_type(&self, event_type: &str, limit: i64) -> Result<Vec<Event>>;
}
