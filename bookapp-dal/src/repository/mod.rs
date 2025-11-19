pub mod author_repository;
pub mod book_repository;
pub mod edition_repository;
pub mod event_repository;
pub mod series_repository;
pub mod work_repository;

pub use author_repository::AuthorRepositoryImpl;
pub use book_repository::BookRepositoryImpl;
pub use edition_repository::EditionRepositoryImpl;
pub use event_repository::EventRepositoryImpl;
pub use series_repository::SeriesRepositoryImpl;
pub use work_repository::WorkRepositoryImpl;
