use axum::http::StatusCode;
use axum::{Extension, extract, Json, Router};

use axum::extract::{Path, Query};
use axum::routing::{delete, get, patch, post};
use sqlx::SqlitePool;
use tracing::{debug, info};
use crate::db;
use crate::db::{Book, BookCreateIn};




//#[tracing::instrument(skip(con), fields(num_books))]
async fn get_all_books(
    Extension(con): Extension<SqlitePool>,
) -> Result<Json<Vec<Book>>, StatusCode> {

    info!("Getting all books info level");

    if let Ok(books) = db::get_all_books(&con).await {

        // Now let's add an attribute to the tracing span with the number of books
        tracing::Span::current()
            .record("num_books", &books.len());

        // Fetch the book details from the backend service
        let book_details = crate::client::fetch_bulk_book_details(&books).await;

        Ok(Json(books))
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}

#[tracing::instrument(skip(con))]
async fn get_book(
    Extension(con): Extension<SqlitePool>,
    Path(id): Path<i32>
) -> Result<Json<Book>, StatusCode> {

    let trace_id = tracing_opentelemetry_instrumentation_sdk::find_current_trace_id();
    debug!("trace id: {}", trace_id.unwrap());

    if let Ok(book) = db::get_book(&con, id).await {
        Ok(Json(book))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[tracing::instrument(skip(con))]
async fn delete_book(
    Extension(con): Extension<SqlitePool>,
    Path(id): Path<i32>
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
    let book = Book { id, author: book_data.author, title: book_data.title };
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

#[cfg(test)]
mod test {
    use super::*;

    use axum_test_helper::TestClient;
    use crate::db::init_db;

    // async fn setup_tests() -> TestClient {
    //     dotenv::dotenv().ok();
    //     let con = init_db().await.unwrap();
    //     let app = crate::router(con);
    //     TestClient::new(app)
    // }

    // #[tokio::test]
    // async fn test_get_all_books() {
    //     let client = setup_tests().await;
    //     let res = client.get("/books").send().await;
    //     assert_eq!(res.status(), StatusCode::OK);
    //     let books = res.json().await;
    //     assert!(!books.is_empty());
    //
    // }
}