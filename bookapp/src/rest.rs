use crate::db::{Book, BookCreateIn};
use crate::{db, reqwest_traced_client};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use axum::{extract, http::Request, Extension, Json, Router};
use opentelemetry::trace::TraceContextExt;
use sqlx::SqlitePool;
use tracing::{debug, info, Level};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use client::Client;

#[tracing::instrument(skip(con), fields(num_books))]
async fn get_all_books(
    Extension(con): Extension<SqlitePool>,
) -> Result<Json<Vec<Book>>, StatusCode> {
    info!("Getting all books info level");

    if let Ok(books) = db::get_all_books(&con).await {
        // Now let's add an attribute to the tracing span with the number of books
        tracing::Span::current().record("num_books", &books.len());

        // Fetch 5 book details from the backend service using reqwest-tracing client
        //let _book_details = crate::reqwest_traced_client::fetch_bulk_book_details(&books).await;

        let _book_detail_res =
            get_book_details_with_progenitor_client(books.first().unwrap().id).await;

        tracing::info!("Got one book using progenitor");

        Ok(Json(books))
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}

#[tracing::instrument()]
async fn get_book_details_with_progenitor_client(
    book_id: i32,
) -> Result<client::ResponseValue<client::types::Book>, client::Error> {
    // Fetch a single book detail using the progenitor generated client
    let progenitor_client = Client::new("http://backend:8000", client::ClientState::default());

    progenitor_client.get_book().id(book_id).send().await
}

#[tracing::instrument(skip(con), ret(level = Level::TRACE))]
async fn get_book(
    Extension(con): Extension<SqlitePool>,
    Path(id): Path<i32>,
) -> Result<Json<Book>, StatusCode> {
    let trace_id = tracing_opentelemetry_instrumentation_sdk::find_current_trace_id();
    debug!(
        "trace id according to tracing_opentelemetry_instrumentation: {}",
        trace_id.unwrap_or("not-set".into())
    );

    let span = tracing::Span::current();
    let otel_context = span.context();
    let trace_id2 = otel_context.span().span_context().trace_id().to_string();

    debug!("trace id from tracing: {}", trace_id2);

    if let Ok(book) = db::get_book(&con, id).await {
        Ok(Json(book))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[tracing::instrument(skip(con))]
async fn delete_book(
    Extension(con): Extension<SqlitePool>,
    Path(id): Path<i32>,
) -> Result<(), StatusCode> {
    if let Ok(book) = db::delete_book(&con, id).await {
        Ok(())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[tracing::instrument(skip(con))]
async fn update_book(
    Extension(con): Extension<SqlitePool>,
    Path(id): Path<i32>,
    extract::Json(book_data): extract::Json<BookCreateIn>,
) -> Result<Json<i32>, StatusCode> {
    let book = Book {
        id,
        author: book_data.author,
        title: book_data.title,
    };
    if let Ok(id) = db::update_book(&con, book).await {
        Ok(Json(id))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[tracing::instrument(skip(con))]
async fn create_book(
    Extension(con): Extension<SqlitePool>,
    extract::Json(book): extract::Json<BookCreateIn>,
) -> Result<Json<i32>, StatusCode> {
    if let Ok(new_id) = db::create_book(&con, book.author, book.title).await {
        Ok(Json(new_id))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

pub fn book_service() -> Router {
    Router::new()
        .route("/", get(get_all_books))
        .route("/:id", get(get_book))
        .route("/:id", patch(update_book))
        .route("/add", post(create_book))
        .route("/:id", delete(delete_book))
}
