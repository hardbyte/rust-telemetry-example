use reqwest::{Client};
use reqwest_middleware::{ClientBuilder, Extension};
use reqwest_tracing::TracingMiddleware;
use futures::future::join_all;
use tracing::debug;
use crate::db::Book;

#[tracing::instrument(skip(books))]
pub(crate) async fn fetch_bulk_book_details(books: &Vec<Book>) -> Vec<String> {
    let reqwest_client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();

    let http_client = ClientBuilder::new(reqwest_client)
        // Inserts the extension before the request is started
        .with_init(Extension(reqwest_tracing::OtelName("backend-client".into())))
        // Trace HTTP requests. See the tracing crate to make use of these traces.
        .with(TracingMiddleware::default())
        .build();


    // Run each query to backend sequentially (should propagate context):
    let mut seq_book_details = Vec::new();
    for book in books.iter().take(5) {

        let r = http_client.get(
            format!("http://backend:8000/books/{}", book.id)
        )
            .send()
            .await
            .expect("failed to get response from backend");

        let book_detail = r.text().await.unwrap();
        seq_book_details.push(book_detail);
    }

    // Run queries to backend in parallel:
    // let futures = books
    //     .into_iter()
    //     .take(5)
    //     .map(|book| {
    //         let http_client = http_client.clone();
    //         async move {
    //             debug!(id=book.id, "Getting one book frob backend");
    //             let r = http_client.get(format!("http://backend:8000/books/{}", book.id))
    //                 .send()
    //                 .await
    //                 .expect("failed to get response from backend");
    //
    //             r.text().await.unwrap()
    //         }
    //     });
    //
    // let book_details: Vec<String> = join_all(futures).await;

    seq_book_details
}
