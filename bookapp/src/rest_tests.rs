use axum::{body::Body, http::{Request, StatusCode}, Extension};
use tower::ServiceExt;
use dotenv::dotenv;
use rdkafka::producer::FutureProducer;
use std::sync::Arc;
use crate::db;
use crate::book_ingestion;
use crate::book_details::{BookDetailsProvider, StubBookDetailsProvider};

// Build an app with PgPool + FutureProducer + StubBookDetailsProvider layered in, and nest under /books
async fn setup_test_app() -> axum::Router {
    dotenv().ok();
    let pool = db::init_db().await.unwrap();
    let producer: FutureProducer = book_ingestion::create_producer().unwrap();
    axum::Router::new()
        .nest_service("/books", crate::rest::book_service())
        .layer(Extension(Arc::new(StubBookDetailsProvider) as Arc<dyn BookDetailsProvider>))
        .layer(Extension(pool))
        .layer(Extension(producer))
}

#[tokio::test]
async fn test_get_all_books() {
    let app = setup_test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/books").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_book() {
    let app = setup_test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/books/1").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(matches!(response.status(), StatusCode::OK | StatusCode::NOT_FOUND));
}

#[tokio::test]
async fn test_update_book() {
    let app = setup_test_app().await;
    let req = Request::builder()
        .method("PATCH")
        .uri("/books/1")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"author":"A","title":"T"}"#))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert!(matches!(response.status(), StatusCode::OK | StatusCode::NOT_FOUND));
}

#[tokio::test]
async fn test_create_book() {
    let app = setup_test_app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/books/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"author":"A","title":"T"}"#))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_bulk_create_books() {
    let app = setup_test_app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/books/bulk_add")
        .header("content-type", "application/json")
        .body(Body::from(r#"[{"author":"A","title":"T"}]"#))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_delete_book() {
    let app = setup_test_app().await;
    let req = Request::builder()
        .method("DELETE")
        .uri("/books/1")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert!(matches!(response.status(), StatusCode::OK | StatusCode::NOT_FOUND));
}
