use crate::database::DatabasePools;
use anyhow::Result as AnyhowResult;
use axum::extract::{Path, Query};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use bookapp_dal::models::BookStatus;
use bookapp_dal::models::{
    Author, AuthorCreateInput, BookCreateInput, BookSearchResult, EditionCreateInput,
    EventCreateInput, SeriesCreateInput, SeriesWorksAssociationCreateInput, WorkCreateInput,
};
use bookapp_dal::repository::{
    AuthorRepositoryImpl, EditionRepositoryImpl, EventRepositoryImpl, SeriesRepositoryImpl,
    WorkRepositoryImpl,
};
use bookapp_dal::{Book, BookRepositoryImpl};
use once_cell::sync::Lazy;
use rdkafka::producer::FutureProducer;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::Level;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use utoipa::OpenApi;
use uuid::Uuid;

const DEFAULT_BOOKS_LIMIT: i64 = 100;
const MAX_BOOKS_LIMIT: i64 = 500;
const MAX_SEARCH_LIMIT: i64 = 50;
const SHORT_QUERY_LIMIT: i64 = 10;
const LETTER_CACHE_TTL_SECS: u64 = 5;

static LETTER_SEARCH_CACHE: Lazy<SearchCache> =
    Lazy::new(|| SearchCache::new(Duration::from_secs(LETTER_CACHE_TTL_SECS)));

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListBooksParams {
    /// Maximum number of books to return (default 100, max 500)
    #[serde(default = "default_books_limit")]
    #[param(minimum = 1, maximum = 500)]
    pub limit: i64,
    /// Number of books to skip before starting the page (default 0)
    #[serde(default)]
    #[param(minimum = 0)]
    pub offset: i64,
}

const fn default_books_limit() -> i64 {
    DEFAULT_BOOKS_LIMIT
}

struct CachedSearchEntry {
    expires_at: Instant,
    results: Vec<BookSearchResult>,
}

struct SearchCache {
    ttl: Duration,
    inner: RwLock<HashMap<String, CachedSearchEntry>>,
}

impl SearchCache {
    fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            inner: RwLock::new(HashMap::new()),
        }
    }

    async fn get(&self, key: &str) -> Option<Vec<BookSearchResult>> {
        let now = Instant::now();
        let mut guard = self.inner.write().await;
        if let Some(entry) = guard.get(key) {
            if entry.expires_at > now {
                return Some(entry.results.clone());
            }
        }
        guard.remove(key);
        None
    }

    async fn store(&self, key: String, results: Vec<BookSearchResult>) {
        let expires_at = Instant::now() + self.ttl;
        let mut guard = self.inner.write().await;
        guard.insert(
            key,
            CachedSearchEntry {
                expires_at,
                results,
            },
        );
    }
}

#[derive(Debug, Copy, Clone)]
struct Pagination {
    limit: i64,
    offset: i64,
    clamped: bool,
}

impl From<&ListBooksParams> for Pagination {
    fn from(params: &ListBooksParams) -> Self {
        let mut clamped = false;
        let mut limit = params.limit;
        if limit < 1 || limit > MAX_BOOKS_LIMIT {
            clamped = true;
            limit = limit.clamp(1, MAX_BOOKS_LIMIT);
        }
        let mut offset = params.offset;
        if offset < 0 {
            clamped = true;
            offset = 0;
        }
        Pagination {
            limit,
            offset,
            clamped,
        }
    }
}

impl Pagination {
    fn headers(&self, returned: usize) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-pagination-limit",
            HeaderValue::from_str(&self.limit.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
        headers.insert(
            "x-pagination-offset",
            HeaderValue::from_str(&self.offset.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
        headers.insert(
            "x-pagination-next-offset",
            HeaderValue::from_str(&(self.offset + returned as i64).to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
        headers
    }
}

#[utoipa::path(
    get,
    path = "/books",
    params(ListBooksParams),
    responses(
        (status = 200, description = "List books", body = [Book], headers(
            ("x-pagination-limit" = i64, description = "Sanitized page size that was applied"),
            ("x-pagination-offset" = i64, description = "Offset supplied (or defaulted) for this page"),
            ("x-pagination-next-offset" = i64, description = "Offset to request the next page")
        )),
        (status = 503, description = "Database temporarily unavailable")
    ),
    tag = "Books"
)]
#[tracing::instrument(
    skip(db_pools),
    fields(
        num_books,
        books.limit,
        books.offset,
        http.request.method = "GET",
        http.route = "/books",
        url.path = "/books"
    )
)]
async fn get_all_books(
    Extension(db_pools): Extension<DatabasePools>,
    Query(params): Query<ListBooksParams>,
) -> Result<(HeaderMap, Json<Vec<Book>>), StatusCode> {
    let pagination = Pagination::from(&params);
    tracing::Span::current().record("books.limit", pagination.limit);
    tracing::Span::current().record("books.offset", pagination.offset);
    tracing::Span::current().record("books.limit_clamped", pagination.clamped);
    tracing::info!("Listing books with pagination");

    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.find_all(pagination.limit, pagination.offset).await {
        Ok(books) => {
            tracing::Span::current().record("num_books", books.len() as i64);
            tracing::Span::current().record("http.response.status_code", 200);
            let headers = pagination.headers(books.len());
            Ok((headers, Json(books)))
        }
        Err(e) => {
            tracing::error!(error_details=%e, "Failed to get all books");
            tracing::Span::current().record("http.response.status_code", 503);
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

#[utoipa::path(
    get,
    path = "/books/id_list",
    params(ListBooksParams),
    responses(
        (status = 200, description = "List book IDs", body = [Uuid], headers(
            ("x-pagination-limit" = i64, description = "Sanitized page size that was applied"),
            ("x-pagination-offset" = i64, description = "Offset supplied (or defaulted) for this page"),
            ("x-pagination-next-offset" = i64, description = "Offset to request the next page")
        )),
        (status = 503, description = "Database temporarily unavailable")
    ),
    tag = "Books"
)]
#[tracing::instrument(
    skip(db_pools),
    fields(
        num_books,
        books.limit,
        books.offset,
        http.request.method = "GET",
        http.route = "/books/id_list",
        url.path = "/books/id_list"
    )
)]
async fn get_book_ids(
    Extension(db_pools): Extension<DatabasePools>,
    Query(params): Query<ListBooksParams>,
) -> Result<(HeaderMap, Json<Vec<Uuid>>), StatusCode> {
    let pagination = Pagination::from(&params);
    tracing::Span::current().record("books.limit", pagination.limit);
    tracing::Span::current().record("books.offset", pagination.offset);
    tracing::Span::current().record("books.limit_clamped", pagination.clamped);
    tracing::info!("Listing book IDs with pagination");

    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.list_ids(pagination.limit, pagination.offset).await {
        Ok(ids) => {
            tracing::Span::current().record("books.ids_returned", ids.len() as i64);
            tracing::Span::current().record("http.response.status_code", 200);
            let headers = pagination.headers(ids.len());
            Ok((headers, Json(ids)))
        }
        Err(e) => {
            tracing::error!(error_details=%e, "Failed to get book ids");
            tracing::Span::current().record("http.response.status_code", 503);
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

#[utoipa::path(
    get,
    path = "/books/{id}",
    responses(
        (status = 200, description = "Book found", body = Book),
        (status = 404, description = "Book not found")
    ),
    params(
        ("id" = Uuid, Path, description = "Book ID")
    ),
    tag = "Books"
)]
#[tracing::instrument(skip(db_pools), ret(level = Level::TRACE), fields(book_id = %id, http.request.method = "GET", http.route = "/books/{id}", url.path = tracing::field::Empty))]
async fn get_book(
    Extension(db_pools): Extension<DatabasePools>,
    Path(id): Path<Uuid>,
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

    // Add event to current span with book_id as attribute instead of high-cardinality metric dimension
    let span = tracing::Span::current();
    span.record("book_id", tracing::field::display(id));
    span.record("url.path", format!("/books/{}", id));

    // Increment counter with low-cardinality dimensions (e.g., operation type)
    counter.add(1, &[opentelemetry::KeyValue::new("operation", "get_book")]);

    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.find_by_id(id).await {
        Ok(Some(book)) => {
            span.record("http.response.status_code", 200);
            Ok(Json(book))
        }
        Ok(None) => {
            span.record("http.response.status_code", 404);
            Err(StatusCode::NOT_FOUND)
        }
        Err(_) => {
            span.record("http.response.status_code", 500);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/books/{id}",
    responses(
        (status = 200, description = "Book deleted successfully"),
        (status = 404, description = "Book not found")
    ),
    params(
        ("id" = Uuid, Path, description = "Book ID")
    ),
    tag = "Books"
)]
#[tracing::instrument(skip(db_pools))]
async fn delete_book(
    Extension(db_pools): Extension<DatabasePools>,
    Path(id): Path<Uuid>,
) -> Result<(), StatusCode> {
    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    match repo.delete(id).await {
        Ok(()) => Ok(()),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

#[utoipa::path(
    patch,
    path = "/books/{id}",
    request_body = BookCreateInput,
    responses(
        (status = 200, description = "Book updated successfully", body = i32),
        (status = 404, description = "Book not found")
    ),
    params(
        ("id" = Uuid, Path, description = "Book ID")
    ),
    tag = "Books"
)]
#[tracing::instrument(skip(db_pools), fields(work.id = %id, work.title = %book_data.work_title, http.request.method = "PUT", http.route = "/books/{id}", url.path = tracing::field::Empty))]
async fn update_book(
    Extension(db_pools): Extension<DatabasePools>,
    Path(id): Path<Uuid>,
    Json(book_data): Json<BookCreateInput>,
) -> Result<Json<i32>, StatusCode> {
    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);

    let span = tracing::Span::current();
    span.record("url.path", format!("/books/{}", id));

    // Load current normalized book (work + primary author), then update title/status
    let existing = repo
        .find_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut current = if let Some(b) = existing {
        b
    } else {
        // Book doesn't exist, return 0 rows affected (like SQL UPDATE would)
        span.record("http.response.status_code", 200);
        return Ok(Json(0));
    };

    // Apply updates from the normalized input
    current.work_title = book_data.work_title;
    if let Some(status) = book_data.status {
        current.status = status;
    }

    match repo.update(current).await {
        Ok(rows_affected) => {
            span.record("db.rows_affected", rows_affected);
            span.record("http.response.status_code", 200);
            Ok(Json(rows_affected))
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to update book");
            span.record("http.response.status_code", 404);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

#[utoipa::path(
    post,
    path = "/books/add",
    request_body = BookCreateInput,
    responses(
        (status = 201, description = "Book created successfully", body = Uuid)
    ),
    tag = "Books"
)]
#[tracing::instrument(skip(db_pools, _producer))]
async fn create_book(
    Extension(db_pools): Extension<DatabasePools>,
    Extension(_producer): Extension<FutureProducer>,
    Json(book): Json<BookCreateInput>,
) -> Result<(StatusCode, Json<Uuid>), StatusCode> {
    let book_repo =
        BookRepositoryImpl::new(db_pools.write_pool.clone(), db_pools.read_pool.clone());
    let event_repo = EventRepositoryImpl::new(db_pools.write_pool.clone(), db_pools.read_pool);

    match book_repo.create(book.clone()).await {
        Ok(new_id) => {
            // Create domain event for outbox pattern
            if let Err(e) = create_book_created_event(&event_repo, new_id, &book).await {
                tracing::error!(
                    error = %e,
                    book_id = %new_id,
                    "Failed to create BookCreated domain event"
                );
                // Note: We don't fail the request since the book was successfully created
            }

            Ok((StatusCode::CREATED, Json(new_id)))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[tracing::instrument(skip(db_pools), fields(num_books))]
async fn bulk_create_books(
    Extension(db_pools): Extension<DatabasePools>,
    Json(payload): Json<Vec<BookCreateInput>>,
) -> Result<(StatusCode, Json<Vec<Uuid>>), StatusCode> {
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
async fn queue_background_ingestion_task(producer: &FutureProducer, new_id: Uuid) {
    // Prepare message
    let book_message = crate::book_ingestion::BookIngestionMessage { book_id: new_id };

    // Get current OpenTelemetry context from the current tracing span
    let otel_context = tracing::Span::current().context();

    // Send message to Kafka
    if let Err(e) =
        crate::book_ingestion::send_book_ingestion_message(producer, &book_message, &otel_context)
            .await
    {
        tracing::error!(error = format!("{e:#}"), book_id = %new_id, "Failed to send Kafka message");
        // Set span status to error
        tracing::Span::current().set_attribute("otel.status_code", "ERROR");
    } else {
        tracing::info!(book_id = %new_id, "Sent Kafka message");
    }
}

pub fn book_service() -> Router {
    Router::new()
        .route("/", get(get_all_books))
        .route("/id_list", get(get_book_ids))
        .route("/search", get(search_books))
        .route("/add", post(create_book))
        .route("/bulk_add", post(bulk_create_books))
        .route(
            "/{id}",
            get(get_book).patch(update_book).delete(delete_book),
        )
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
) -> Result<(StatusCode, Json<Uuid>), StatusCode> {
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
) -> Result<(StatusCode, Json<Uuid>), StatusCode> {
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
) -> Result<(StatusCode, Json<Uuid>), StatusCode> {
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
) -> Result<(StatusCode, Json<Uuid>), StatusCode> {
    let repo = SeriesRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    let id = repo
        .create(input)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(id)))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SearchParams {
    /// Search query string
    q: String,
    /// Maximum number of results to return
    #[serde(default = "default_limit")]
    #[param(minimum = 1, maximum = 50)]
    limit: i64,
}

fn default_limit() -> i64 {
    20
}

fn sanitize_search_limit(requested: i64, query_len: usize) -> i64 {
    let clamped = requested.clamp(1, MAX_SEARCH_LIMIT);
    if query_len <= 1 {
        clamped.min(SHORT_QUERY_LIMIT)
    } else {
        clamped
    }
}

fn should_cache_query(query: &str) -> bool {
    query.chars().count() == 1
}

fn cache_key(query: &str) -> String {
    query.trim().to_lowercase()
}

#[utoipa::path(
    get,
    path = "/books/search",
    params(SearchParams),
    responses(
        (status = 200, description = "Search results", body = [BookSearchResult])
    ),
    tag = "Books"
)]
#[tracing::instrument(
    skip(db_pools),
    fields(
        search.query = %params.q,
        search.requested_limit = %params.limit,
        search.effective_limit = tracing::field::Empty,
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
    let trimmed_query = params.q.trim();
    let effective_query = trimmed_query.to_owned();
    let query_len = effective_query.chars().count();
    let effective_limit = sanitize_search_limit(params.limit, query_len);
    tracing::Span::current().record("search.effective_limit", effective_limit);
    let should_cache = should_cache_query(&effective_query);

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
    if effective_query.is_empty() {
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

    if effective_query.len() > 200 {
        search_counter.add(
            1,
            &[
                opentelemetry::KeyValue::new("status", "error"),
                opentelemetry::KeyValue::new("error_type", "query_too_long"),
            ],
        );
        tracing::warn!(
            search.query_length = effective_query.len(),
            search.error = "query_too_long",
            "Search query exceeds maximum length"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    tracing::info!(search.query = %effective_query, search.limit = effective_limit, "Processing book search request");

    let repo = BookRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);
    let mut cache_status = "miss";
    let cache_key = if should_cache {
        Some(cache_key(&effective_query))
    } else {
        None
    };
    let db_fetch = async {
        repo.full_text_search(&effective_query, effective_limit)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    };

    let results = if let Some(key) = cache_key.as_ref() {
        if let Some(cached) = LETTER_SEARCH_CACHE.get(key).await {
            cache_status = "hit";
            cached
        } else {
            let fresh = db_fetch.await?;
            LETTER_SEARCH_CACHE.store(key.clone(), fresh.clone()).await;
            fresh
        }
    } else {
        db_fetch.await?
    };

    tracing::Span::current().record("search.cache_status", cache_status);

    match Ok(results) {
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
        Err(status) => {
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
                search.query = %effective_query,
                search.execution_time_ms = execution_time.as_millis(),
                "Book search failed"
            );

            Err(status)
        }
    }
}

#[derive(Debug, Deserialize)]
struct SeriesWorkAddInput {
    work_id: Uuid,
    #[serde(default)]
    primary_work: Option<bool>,
    #[serde(default)]
    order_id: Option<i32>,
}

#[tracing::instrument(skip(db_pools))]
async fn add_work_to_series(
    Extension(db_pools): Extension<DatabasePools>,
    Path(series_id): Path<Uuid>,
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

#[derive(OpenApi)]
#[openapi(
    paths(
        get_all_books,
        get_book_ids,
        get_book,
        create_book,
        update_book,
        delete_book,
        search_books
    ),
    components(
        schemas(Book, BookCreateInput, BookStatus, BookSearchResult)
    ),
    tags(
        (name = "Books", description = "Book management API endpoints")
    ),
    info(
        title = "Book Service API",
        description = "API for managing books in the library with distributed tracing support",
        version = "1.0.0",
        contact(
            name = "BookApp API",
            url = "http://localhost:8000"
        )
    ),
    servers(
        (url = "http://localhost:8000", description = "Local development server")
    )
)]
pub struct ApiDoc;

#[allow(dead_code)]
async fn serve_openapi() -> axum::response::Json<utoipa::openapi::OpenApi> {
    axum::response::Json(ApiDoc::openapi())
}

/// Create a BookCreated domain event for the outbox pattern
#[tracing::instrument(skip(event_repo), fields(book_id = %book_id))]
async fn create_book_created_event(
    event_repo: &EventRepositoryImpl,
    book_id: Uuid,
    book: &BookCreateInput,
) -> AnyhowResult<i64> {
    use opentelemetry::trace::TraceContextExt;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    // Get current trace context for event correlation
    let span_context = tracing::Span::current().context();
    let otel_span = span_context.span();
    let trace_id = format!("{:032x}", otel_span.span_context().trace_id());
    let span_id = format!("{:016x}", otel_span.span_context().span_id());

    let event = EventCreateInput {
        aggregate_type: "Book".to_string(),
        aggregate_id: book_id.to_string(),
        event_type: "BookCreated".to_string(),
        payload: serde_json::json!({
            "book_id": book_id,
            "work_title": book.work_title,
            "primary_author_name": book.primary_author_name,
            "status": book.status
        }),
        headers: Some(serde_json::json!({})),
        trace_id: Some(trace_id),
        span_id: Some(span_id),
        source_service: Some("bookapp".to_string()),
        version: Some(1),
        topic: Some("book_ingestion".to_string()),
        published_at: None,
        publish_attempts: None,
        publish_error: None,
    };

    event_repo
        .append(event)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create event: {}", e))
}

#[cfg(test)]
mod search_limit_tests {
    use super::sanitize_search_limit;

    #[test]
    fn clamps_to_max_limit() {
        assert_eq!(sanitize_search_limit(500, 5), 50);
        assert_eq!(sanitize_search_limit(-5, 5), 1);
    }

    #[test]
    fn short_queries_get_lower_cap() {
        assert_eq!(sanitize_search_limit(40, 1), 10);
        assert_eq!(sanitize_search_limit(5, 1), 5);
    }
}

pub fn openapi_router() -> Router {
    use axum::{response::Html, routing::get};
    use utoipa_swagger_ui::SwaggerUi;

    let debug_route = Router::new().route(
        "/debug",
        get(|| async { Html("<h1>OpenAPI router is working!</h1>") }),
    );

    Router::new()
        .merge(api_router())
        .merge(debug_route)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
}
