use std::time::Duration;

use bookapp_dal::Book;
use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Extension};
use reqwest_tracing::TracingMiddleware;
use tracing::instrument;
use uuid::Uuid;

fn traced_client() -> ClientWithMiddleware {
    let reqwest_client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client");

    // Propagate OTEL context on outbound HTTP calls for clarity in traces.
    ClientBuilder::new(reqwest_client)
        .with_init(Extension(
            reqwest_tracing::OtelPathNames::known_paths(["/books/{id}"]).unwrap(),
        ))
        .with(TracingMiddleware::default())
        .build()
}

#[instrument(skip(book_ids), fields(num_ids = book_ids.len()))]
pub async fn fetch_books_sequential_via_api(
    base_url: &str,
    book_ids: &[Uuid],
) -> reqwest_middleware::Result<Vec<Book>> {
    let base = base_url.trim_end_matches('/');
    let client = traced_client();
    let mut books = Vec::with_capacity(book_ids.len());

    for id in book_ids {
        let url = format!("{}/books/{}", base, id);
        let response = client.get(url).send().await?;
        if response.status().is_success() {
            let book = response
                .json::<Book>()
                .await
                .map_err(reqwest_middleware::Error::from)?;
            books.push(book);
        }
    }

    Ok(books)
}

#[instrument(skip(repo, book_ids), fields(num_ids = book_ids.len()))]
pub async fn fetch_books_sequential_from_db(
    repo: &bookapp_dal::BookRepositoryImpl,
    book_ids: &[Uuid],
) -> bookapp_dal::Result<Vec<Book>> {
    let mut books = Vec::with_capacity(book_ids.len());
    for id in book_ids {
        if let Some(book) = repo.find_by_id(*id).await? {
            books.push(book);
        }
    }
    Ok(books)
}
