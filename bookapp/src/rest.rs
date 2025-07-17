use crate::book_details::BookDetailsProvider;
use crate::db;
use crate::db::{Book, BookCreateIn, BookStatus};
use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use axum::{Extension, Json, Router};
use rdkafka::producer::FutureProducer;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::Level;
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[tracing::instrument(skip(con, details), fields(num_books))]
async fn get_all_books(
    Extension(con): Extension<PgPool>,
    Extension(details): Extension<Arc<dyn BookDetailsProvider>>,
) -> Result<Json<Vec<Book>>, StatusCode> {
    tracing::info!("Getting all books");
    match db::get_all_books(&con).await {
        Ok(books) => {
            tracing::Span::current().record("num_books", books.len() as i64);
            // delegate to injected provider
            details.enrich_book_details(&books).await;
            Ok(Json(books))
        }
        Err(e) => {
            tracing::error!(error_details=%e, "Failed to get all books");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
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
) -> Result<(StatusCode, Json<i32>), StatusCode> {
    let status = book.status.unwrap_or(BookStatus::Available);
    if let Ok(new_id) = db::create_book(&con, book.author, book.title, status).await {
        queue_background_ingestion_task(&producer, new_id).await;
        Ok((StatusCode::CREATED, Json(new_id)))
    } else {
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

#[tracing::instrument(skip(con), fields(num_books))]
async fn bulk_create_books(
    Extension(con): Extension<PgPool>,
    Json(payload): Json<Vec<BookCreateIn>>,
) -> Result<(StatusCode, Json<Vec<i32>>), StatusCode> {
    let num = payload.len() as i64;
    tracing::Span::current().record("num_books", num);

    match db::bulk_insert_books(&con, &payload).await {
        Ok(ids) => Ok((StatusCode::CREATED, Json(ids))),
        Err(e) => {
            tracing::error!(error=%e, "bulk insert failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[tracing::instrument(skip(producer), fields(otel.kind = "Producer"))]
async fn queue_background_ingestion_task(producer: &FutureProducer, new_id: i32) {
    // Prepare message
    let book_message = crate::book_ingestion::BookIngestionMessage { book_id: new_id };

    // Get current OpenTelemetry context from the current tracing span
    let otel_context = tracing::Span::current().context();

    // Send message to Kafka
    if let Err(e) =
        crate::book_ingestion::send_book_ingestion_message(producer, &book_message, &otel_context)
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
        .route("/bulk_add", post(bulk_create_books))
        .route("/{id}", delete(delete_book))
}
