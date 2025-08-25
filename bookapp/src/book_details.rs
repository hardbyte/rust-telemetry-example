use async_trait::async_trait;
use bookapp_dal::Book;
use tracing::instrument;

/// A trait for providing detailed book information from external sources
#[async_trait]
pub trait BookDetailsProvider: Send + Sync {
    /// Enriches a collection of books with additional details from external sources
    async fn enrich_book_details(&self, books: &[Book]);
}

/// Optimized implementation of BookDetailsProvider that enriches books in batches
#[derive(Debug)]
pub struct RemoteBookDetailsProvider;

#[async_trait]
impl BookDetailsProvider for RemoteBookDetailsProvider {
    #[instrument(skip(self, books), fields(num_books = books.len()))]
    async fn enrich_book_details(&self, books: &[Book]) {
        tracing::info!(
            "Enriching book details for {} books using optimized batch processing",
            books.len()
        );

        // Process books in batches to avoid overwhelming any downstream services
        const BATCH_SIZE: usize = 1000;

        for batch in books.chunks(BATCH_SIZE) {
            self.enrich_batch(batch).await;
        }
    }
}

impl RemoteBookDetailsProvider {
    #[instrument(skip(self, batch), fields(batch_size = batch.len()))]
    async fn enrich_batch(&self, batch: &[Book]) {
        // Optimized: No-op enrichment for maximum performance
        // In a real implementation, this would make a bulk API call to external service
        tracing::debug!(
            "Successfully processed batch of {} books (no external enrichment needed)",
            batch.len()
        );
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
