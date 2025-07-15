use async_trait::async_trait;
use client::Client;
use tracing::instrument;
use crate::db::Book;

/// A trait for providing detailed book information from external sources
#[async_trait]
pub trait BookDetailsProvider: Send + Sync {
    /// Enriches a collection of books with additional details from external sources
    async fn enrich_book_details(&self, books: &[Book]);
}

/// Remote implementation of BookDetailsProvider that fetches data from the backend service
#[derive(Debug)]
pub struct RemoteBookDetailsProvider;

#[async_trait]
impl BookDetailsProvider for RemoteBookDetailsProvider {
    #[instrument(skip(self, books), fields(num_books = books.len()))]
    async fn enrich_book_details(&self, books: &[Book]) {
        tracing::info!("Enriching book details for {} books", books.len());
        
        for book in books {
            // Call the progenitor client to get additional details
            if let Ok(_details) = self.get_book_details(book.id).await {
                tracing::debug!(
                    book_id = book.id,
                    "Successfully enriched book details"
                );
            }
        }
    }
}

impl RemoteBookDetailsProvider {
    #[instrument(fields(book_id, otel.kind = "Client"))]
    async fn get_book_details(
        &self,
        book_id: i32,
    ) -> Result<client::ResponseValue<client::types::Book>, client::Error> {
        // Fetch a single book detail using the progenitor generated client
        let progenitor_client = Client::new("http://backend:8000", client::ClientState::default());
        progenitor_client.get_book().id(book_id).send().await
    }
}

/// Stub implementation of BookDetailsProvider for testing
pub struct StubBookDetailsProvider;

#[async_trait]
impl BookDetailsProvider for StubBookDetailsProvider {
    #[instrument(skip(self, books), fields(num_books = books.len()))]
    async fn enrich_book_details(&self, books: &[Book]) {
        tracing::info!("Using stub book details provider for {} books", books.len());
        // No-op implementation for testing
    }
}
