use crate::db::Book;
use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Extension};
use reqwest_tracing::{ReqwestOtelSpanBackend, TracingMiddleware};
use std::iter::Take;
use std::slice::Iter;
use tracing::instrument;

#[tracing::instrument(skip(books))]
pub(crate) async fn fetch_bulk_book_details(books: &Vec<Book>) -> Vec<String> {
    let reqwest_client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();

    let http_client = ClientBuilder::new(reqwest_client)
        // This extension would set the span name to `backend-client`
        // Instead of the request path e.g. `GET /books/{id}`
        // .with_init(Extension(reqwest_tracing::OtelName(
        //     "backend-client".into(),
        // )))
        .with_init(Extension(
            reqwest_tracing::OtelPathNames::known_paths(["/books/{id}"]).unwrap(),
        ))
        // Trace HTTP requests. See the tracing crate to make use of these traces.
        .with(TracingMiddleware::default())
        //.with(TracingMiddleware::<reqwest_tracing::SpanBackendWithUrl>::new())
        .build();

    // Run each query to backend sequentially (should propagate context):
    let mut seq_book_details = Vec::new();

    fetch_some_books_sequentially(&http_client, &mut seq_book_details, &books).await;

    // Run queries to backend in parallel:
    fetch_some_books_in_parallel(http_client, &books).await;

    seq_book_details
}

#[instrument(skip_all)]
async fn fetch_some_books_in_parallel(http_client: ClientWithMiddleware, some_books: &Vec<Book>) {
    let futures = some_books.into_iter().take(5).map(|book| {
        let http_client = http_client.clone();
        async move {
            tracing::debug!(id = book.id, "Getting one book from backend");
            let r = http_client
                .get(format!("http://backend:8000/books/{}", book.id))
                .send()
                .await
                .expect("failed to get response from backend");

            r.text().await.unwrap()
        }
    });

    let _book_details: Vec<String> = futures::future::join_all(futures).await;
}

#[instrument(skip_all)]
async fn fetch_some_books_sequentially(
    http_client: &ClientWithMiddleware,
    seq_book_details: &mut Vec<String>,
    some_books: &Vec<Book>,
) {
    for book in some_books.into_iter().take(5) {
        let r = http_client
            .get(format!("http://backend:8000/books/{}", book.id))
            .send()
            // Can also go here:
            //.with_extension(reqwest_tracing::OtelPathNames::known_paths(["/books/{id}"])?)
            .await
            .expect("failed to get response from backend");

        let book_detail = r.text().await.unwrap();
        seq_book_details.push(book_detail);
    }
}
