use crate::db::{Book, BookCreateIn, BookStatus};
use crate::{db, reqwest_traced_client};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use axum::{extract, http::Request, Extension, Json, Router};

use opentelemetry::trace::TraceContextExt;
use rdkafka::producer::FutureProducer;
use sqlx::PgPool;
use tracing::{Instrument, Level};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use client::Client;

#[tracing::instrument(skip(con), fields(num_books))]
async fn get_all_books(Extension(con): Extension<PgPool>) -> Result<Json<Vec<Book>>, StatusCode> {
    tracing::info!("Getting all books");

    match db::get_all_books(&con).await {
        Ok(books) => {
            // Now let's add an attribute to the tracing span with the number of books
            tracing::Span::current().record("num_books", books.len() as i64);

            // Fetch 5 book details from the backend service using reqwest-tracing client
            let _book_details = crate::reqwest_traced_client::fetch_bulk_book_details(&books).await;

            let _book_detail_res =
                get_book_details_with_progenitor_client(books.first().unwrap().id).await;
            let span = tracing::info_span!("tokio_spawned_requests");

            let book_details_futures = books
                .iter()
                .take(5)
                .map(|b: &Book| b.id)
                .map(|id| {
                    tokio::spawn(
                        get_book_details_with_progenitor_client(id).instrument(span.clone()),
                    )
                })
                .collect::<Vec<_>>();

            let all_book_details = futures::future::join_all(book_details_futures).await;

            tracing::info!(
                num_books = all_book_details.len(),
                "Got all book details using progenitor"
            );

            Ok(Json(books))
        }
        Err(e) => {
            tracing::error!(error_details=%e, "Failed to get all books");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

#[tracing::instrument(fields(otel.kind = "client"))]
async fn get_book_details_with_progenitor_client(
    book_id: i32,
) -> Result<client::ResponseValue<client::types::Book>, client::Error> {
    // Fetch a single book detail using the progenitor generated client
    let progenitor_client = Client::new("http://backend:8000", client::ClientState::default());

    progenitor_client.get_book().id(book_id).send().await
}

#[tracing::instrument(skip(con), ret(level = Level::TRACE))]
async fn get_book(
    Extension(con): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<Book>, StatusCode> {
    // Metrics can be added to the tracing span directly
    // due to the MetricsLayer
    // https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/struct.MetricsLayer.html
    tracing::trace!(
        monotonic_counter.queried_books = 1,
        book_id = id.to_string()
    );

    let meter = opentelemetry::global::meter("bookapp");

    // Create a Counter Instrument.
    let counter = meter
        .u64_counter("my_book_counter")
        .with_description("Retrieval of a book")
        .build();

    // Add 1 for this book_id to the counter. Wouldn't actually want to have book_id as a dimension
    counter.add(
        1,
        &[opentelemetry::KeyValue::new("book_id", id.to_string())],
    );

    if let Ok(book) = db::get_book(&con, id).await {
        Ok(Json(book))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[tracing::instrument(skip(con))]
async fn delete_book(
    Extension(con): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<(), StatusCode> {
    if let Ok(_book) = db::delete_book(&con, id).await {
        Ok(())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[tracing::instrument(skip(con))]
async fn update_book(
    Extension(con): Extension<PgPool>,
    Path(id): Path<i32>,
    Json(book_data): Json<BookCreateIn>,
) -> Result<Json<i32>, StatusCode> {
    let book = Book {
        id,
        author: book_data.author,
        title: book_data.title,
        status: BookStatus::Available,
    };
    if let Ok(id) = db::update_book(&con, book).await {
        Ok(Json(id))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[tracing::instrument(skip(con, producer))]
async fn create_book(
    Extension(con): Extension<PgPool>,
    Extension(producer): Extension<FutureProducer>,
    Json(book): Json<BookCreateIn>,
) -> Result<Json<i32>, StatusCode> {
    if let Ok(new_id) = db::create_book(&con, book.author, book.title, book.status).await {
        queue_background_ingestion_task(&producer, new_id).await;

        Ok(Json(new_id))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[tracing::instrument(skip(producer), fields(otel.kind = "producer"))]
async fn queue_background_ingestion_task(producer: &FutureProducer, new_id: i32) {
    // Prepare message
    let book_message = crate::book_ingestion::BookIngestionMessage { book_id: new_id };

    // Get current OpenTelemetry context from the current tracing span
    let otel_context = tracing::Span::current().context();

    // Send message to Kafka
    if let Err(e) =
        crate::book_ingestion::send_book_ingestion_message(&producer, &book_message, &otel_context)
            .await
    {
        tracing::error!(
            error = format!("{e:#}"),
            book_id = new_id,
            "Failed to send Kafka message"
        );
        // Set span status to error
        tracing::Span::current().set_attribute("otel.status_code", "ERROR");
    } else {
        tracing::info!(book_id = new_id, "Sent Kafka message");
    }
}

pub fn book_service() -> Router {
    Router::new()
        .route("/", get(get_all_books))
        .route("/{id}", get(get_book))
        .route("/{id}", patch(update_book))
        .route("/add", post(create_book))
        .route("/{id}", delete(delete_book))
}
