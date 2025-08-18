use crate::book_details::BookDetailsProvider;
use crate::database::DatabasePools;
use crate::db::{Book, BookCreateIn, BookRepository, BookRepositoryImpl, BookStatus};
use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use axum::{Extension, Json, Router};
use rdkafka::producer::FutureProducer;
use std::sync::Arc;
use tracing::Level;
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[tracing::instrument(skip(db_pools, details), fields(num_books))]
async fn get_all_books(
    Extension(db_pools): Extension<DatabasePools>,
    Extension(details): Extension<Arc<dyn BookDetailsProvider>>,
) -> Result<Json<Vec<Book>>, StatusCode> {
    tracing::info!("Getting all books");
    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.find_all().await {
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

#[tracing::instrument(skip(db_pools), ret(level = Level::TRACE))]
async fn get_book(
    Extension(db_pools): Extension<DatabasePools>,
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

    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.find_by_id(id).await {
        Ok(Some(book)) => Ok(Json(book)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[tracing::instrument(skip(db_pools))]
async fn delete_book(
    Extension(db_pools): Extension<DatabasePools>,
    Path(id): Path<i32>,
) -> Result<(), StatusCode> {
    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.delete(id).await {
        Ok(()) => Ok(()),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

#[tracing::instrument(skip(db_pools), fields(book.id = %id, book.author = %book_data.author, book.title = %book_data.title))]
async fn update_book(
    Extension(db_pools): Extension<DatabasePools>,
    Path(id): Path<i32>,
    Json(book_data): Json<BookCreateIn>,
) -> Result<Json<i32>, StatusCode> {
    let book = Book {
        id,
        author: book_data.author,
        title: book_data.title,
        status: book_data.status.unwrap_or(BookStatus::Available),
    };

    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.update(book).await {
        Ok(rows_affected) => {
            tracing::Span::current().record("db.rows_affected", rows_affected);
            Ok(Json(rows_affected))
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to update book");
            Err(StatusCode::NOT_FOUND)
        }
    }
}

#[tracing::instrument(skip(db_pools, producer))]
async fn create_book(
    Extension(db_pools): Extension<DatabasePools>,
    Extension(producer): Extension<FutureProducer>,
    Json(book): Json<BookCreateIn>,
) -> Result<(StatusCode, Json<i32>), StatusCode> {
    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.create(book).await {
        Ok(new_id) => {
            queue_background_ingestion_task(&producer, new_id).await;
            Ok((StatusCode::CREATED, Json(new_id)))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[tracing::instrument(skip(db_pools), fields(num_books))]
async fn bulk_create_books(
    Extension(db_pools): Extension<DatabasePools>,
    Json(payload): Json<Vec<BookCreateIn>>,
) -> Result<(StatusCode, Json<Vec<i32>>), StatusCode> {
    let num = payload.len() as i64;
    tracing::Span::current().record("num_books", num);

    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.bulk_create(&payload).await {
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
