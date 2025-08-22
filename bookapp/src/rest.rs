use crate::book_details::BookDetailsProvider;
use crate::database::DatabasePools;
use crate::db::{
    Book, BookRepository, BookRepositoryImpl,
};
use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use bookapp_dal::models::{
    Author, AuthorCreateInput, BookCreateInput, BookSearchResult,
    EditionCreateInput, EventCreateInput, SeriesCreateInput,
    SeriesWorksAssociationCreateInput, WorkCreateInput,
};
use bookapp_dal::repository::{
    AuthorRepository, AuthorRepositoryImpl, EditionRepository, EditionRepositoryImpl,
    EventRepositoryImpl, SeriesRepository, SeriesRepositoryImpl, WorkRepositoryImpl,
};
use rdkafka::producer::FutureProducer;
use serde::Deserialize;
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

#[tracing::instrument(skip(db_pools), fields(work.id = %id, work.title = %book_data.work_title))]
async fn update_book(
    Extension(db_pools): Extension<DatabasePools>,
    Path(id): Path<i32>,
    Json(book_data): Json<BookCreateInput>,
) -> Result<Json<i32>, StatusCode> {
    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);

    // Load current normalized book (work + primary author), then update title/status
    let existing = repo
        .find_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut current = if let Some(b) = existing {
        b
    } else {
        // Book doesn't exist, return 0 rows affected (like SQL UPDATE would)
        return Ok(Json(0));
    };

    // Apply updates from the normalized input
    current.work_title = book_data.work_title;
    if let Some(status) = book_data.status {
        current.status = status;
    }

    match repo.update(current).await {
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
    Json(book): Json<BookCreateInput>,
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
    Json(payload): Json<Vec<BookCreateInput>>,
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
        .route("/search", get(search_books))
        .route("/add", post(create_book))
        .route("/bulk_add", post(bulk_create_books))
        .route("/{id}", get(get_book).patch(update_book).delete(delete_book))
}

#[tracing::instrument(skip(db_pools))]
async fn get_all_authors(
    Extension(db_pools): Extension<DatabasePools>,
) -> Result<Json<Vec<Author>>, StatusCode> {
    let repo = AuthorRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    repo.find_all()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[tracing::instrument(skip(db_pools))]
async fn create_author(
    Extension(db_pools): Extension<DatabasePools>,
    Json(input): Json<AuthorCreateInput>,
) -> Result<(StatusCode, Json<i32>), StatusCode> {
    let repo = AuthorRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    let id = repo
        .create(input)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(id)))
}

pub fn author_service() -> Router {
    Router::new()
        .route("/", get(get_all_authors))
        .route("/add", post(create_author))
}

#[tracing::instrument(skip(db_pools))]
async fn create_work(
    Extension(db_pools): Extension<DatabasePools>,
    Json(input): Json<WorkCreateInput>,
) -> Result<(StatusCode, Json<i32>), StatusCode> {
    let work_repo =
        WorkRepositoryImpl::new(db_pools.write_pool.clone(), db_pools.read_pool.clone());
    let event_repo =
        EventRepositoryImpl::new(db_pools.write_pool.clone(), db_pools.read_pool.clone());

    let mut tx = db_pools
        .write_pool
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let work_id = work_repo
        .create_with(tx.as_mut(), input)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Append a generic event in the same transaction (transactional outbox pattern)
    event_repo
        .append_with(
            tx.as_mut(),
            EventCreateInput {
                aggregate_type: "work".to_string(),
                aggregate_id: work_id.to_string(),
                event_type: "created".to_string(),
                payload: serde_json::json!({ "work_id": work_id }),
                headers: None,
                trace_id: None,
                span_id: None,
                source_service: Some("bookapp".to_string()),
                topic: Some("domain.events".to_string()),
                version: Some(1),
                published_at: None,
                publish_attempts: None,
                publish_error: None,
            },
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::CREATED, Json(work_id)))
}

pub fn work_service() -> Router {
    Router::new().route("/add", post(create_work))
}

#[tracing::instrument(skip(db_pools))]
async fn create_edition(
    Extension(db_pools): Extension<DatabasePools>,
    Json(input): Json<EditionCreateInput>,
) -> Result<(StatusCode, Json<i32>), StatusCode> {
    let repo = EditionRepositoryImpl::new(db_pools.write_pool.clone(), db_pools.read_pool.clone());
    let id = repo
        .create(input)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(id)))
}

pub fn edition_service() -> Router {
    Router::new().route("/add", post(create_edition))
}

#[tracing::instrument(skip(db_pools))]
async fn create_series(
    Extension(db_pools): Extension<DatabasePools>,
    Json(input): Json<SeriesCreateInput>,
) -> Result<(StatusCode, Json<i32>), StatusCode> {
    let repo = SeriesRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    let id = repo
        .create(input)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(id)))
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    q: String,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    20
}

#[tracing::instrument(
    skip(db_pools),
    fields(
        search.query = %params.q,
        search.limit = %params.limit,
        search.result_count,
        search.cache_status = "miss",
        http.method = "GET",
        http.route = "/books/search",
        user.operation = "search_books"
    )
)]
async fn search_books(
    Extension(db_pools): Extension<DatabasePools>,
    axum::extract::Query(params): axum::extract::Query<SearchParams>,
) -> Result<Json<Vec<BookSearchResult>>, StatusCode> {
    let start_time = std::time::Instant::now();

    // OpenTelemetry metrics
    let meter = opentelemetry::global::meter("bookapp");
    let search_counter = meter
        .u64_counter("book_search_requests_total")
        .with_description("Total number of book search requests")
        .build();

    let search_duration = meter
        .f64_histogram("book_search_duration_seconds")
        .with_description("Duration of book search operations")
        .build();

    let search_results_counter = meter
        .u64_counter("book_search_results_total")
        .with_description("Total number of search results returned")
        .build();

    // Validate query parameters
    if params.q.trim().is_empty() {
        search_counter.add(
            1,
            &[
                opentelemetry::KeyValue::new("status", "error"),
                opentelemetry::KeyValue::new("error_type", "empty_query"),
            ],
        );
        tracing::warn!(
            search.query = %params.q,
            search.error = "empty_query",
            "Search request with empty query"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    if params.q.len() > 200 {
        search_counter.add(
            1,
            &[
                opentelemetry::KeyValue::new("status", "error"),
                opentelemetry::KeyValue::new("error_type", "query_too_long"),
            ],
        );
        tracing::warn!(
            search.query_length = params.q.len(),
            search.error = "query_too_long",
            "Search query exceeds maximum length"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    tracing::info!(
        search.query = %params.q,
        search.limit = %params.limit,
        "Processing book search request"
    );

    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.full_text_search(&params.q, params.limit).await {
        Ok(results) => {
            let execution_time = start_time.elapsed();
            let result_count = results.len();

            // Record metrics
            search_counter.add(1, &[opentelemetry::KeyValue::new("status", "success")]);
            search_duration.record(execution_time.as_secs_f64(), &[]);
            search_results_counter.add(result_count as u64, &[]);

            // Record span attributes
            tracing::Span::current().record("search.result_count", result_count);

            // Log search completion with metrics
            tracing::info!(
                search.result_count = result_count,
                search.execution_time_ms = execution_time.as_millis(),
                search.query_length = params.q.len(),
                search.has_results = !results.is_empty(),
                "Book search completed successfully"
            );

            Ok(Json(results))
        }
        Err(e) => {
            let execution_time = start_time.elapsed();

            // Record error metrics
            search_counter.add(
                1,
                &[
                    opentelemetry::KeyValue::new("status", "error"),
                    opentelemetry::KeyValue::new("error_type", "database_error"),
                ],
            );
            search_duration.record(
                execution_time.as_secs_f64(),
                &[opentelemetry::KeyValue::new("error", "true")],
            );

            tracing::error!(
                error = %e,
                search.query = %params.q,
                search.execution_time_ms = execution_time.as_millis(),
                "Book search failed"
            );

            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Deserialize)]
struct SeriesWorkAddInput {
    work_id: i32,
    #[serde(default)]
    primary_work: Option<bool>,
    #[serde(default)]
    order_id: Option<i32>,
}

#[tracing::instrument(skip(db_pools))]
async fn add_work_to_series(
    Extension(db_pools): Extension<DatabasePools>,
    Path(series_id): Path<i32>,
    Json(input): Json<SeriesWorkAddInput>,
) -> Result<StatusCode, StatusCode> {
    let repo = SeriesRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    repo.add_work(SeriesWorksAssociationCreateInput {
        series_id,
        work_id: input.work_id,
        primary_work: input.primary_work,
        order_id: input.order_id,
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::NO_CONTENT)
}

pub fn series_service() -> Router {
    Router::new()
        .route("/add", post(create_series))
        .route("/{id}/works/add", post(add_work_to_series))
}

pub fn api_router() -> Router {
    Router::new()
        .nest("/books", book_service())
        .nest("/authors", author_service())
        .nest("/works", work_service())
        .nest("/editions", edition_service())
        .nest("/series", series_service())
}
