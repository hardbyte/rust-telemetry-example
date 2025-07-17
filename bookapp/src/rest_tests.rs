#[cfg(test)]
mod tests {

    use crate::book_details::{BookDetailsProvider, StubBookDetailsProvider};
    use crate::db::BookStatus;
    use crate::{book_ingestion, db};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Extension,
    };
    use dotenv::dotenv;
    use rdkafka::producer::FutureProducer;
    use serde_json::Value;
    use sqlx::PgPool;
    use std::sync::Arc;
    use tower::ServiceExt;

    // Helper to setup a transactional test app
    async fn setup_transactional_test_app(pool: PgPool) -> axum::Router {
        dotenv().ok();
        let producer: FutureProducer = book_ingestion::create_producer().unwrap();
        axum::Router::new()
            .nest_service("/books", crate::rest::book_service())
            .layer(Extension(
                Arc::new(StubBookDetailsProvider) as Arc<dyn BookDetailsProvider>
            ))
            .layer(Extension(pool))
            .layer(Extension(producer))
    }

    // Helper to deserialize response body to JSON
    async fn get_response_json(response: axum::response::Response) -> Value {
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body_bytes).unwrap()
    }

    #[sqlx::test]
    async fn test_get_all_books(pool: PgPool) {
        let app = setup_transactional_test_app(pool).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/books")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let json = get_response_json(response).await;
        assert!(json.is_array(), "Response should be an array of books");
    }

    #[sqlx::test]
    async fn test_get_existing_book(pool: PgPool) {
        // Create a book to ensure it exists
        let book_id = db::create_book(
            &pool,
            "Test Author".to_string(),
            "Test Title".to_string(),
            BookStatus::Available,
        )
        .await
        .unwrap();

        let app = setup_transactional_test_app(pool).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/books/{}", book_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let json = get_response_json(response).await;
        assert_eq!(json["id"], book_id);
        assert_eq!(json["author"], "Test Author");
        assert_eq!(json["title"], "Test Title");
        assert_eq!(json["status"], "Available");
    }

    #[sqlx::test]
    async fn test_get_nonexistent_book(pool: PgPool) {
        let app = setup_transactional_test_app(pool).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/books/99999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn test_update_existing_book(pool: PgPool) {
        // Create a book to update
        let book_id = db::create_book(
            &pool,
            "Original Author".to_string(),
            "Original Title".to_string(),
            BookStatus::Available,
        )
        .await
        .unwrap();

        let app = setup_transactional_test_app(pool.clone()).await;
        let req = Request::builder()
            .method("PATCH")
            .uri(format!("/books/{}", book_id))
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"author":"Updated Author","title":"Updated Title"}"#,
            ))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify the book was actually updated
        let updated_book = db::get_book(&pool, book_id).await.unwrap();
        assert_eq!(updated_book.author, "Updated Author");
        assert_eq!(updated_book.title, "Updated Title");
    }

    #[sqlx::test]
    async fn test_update_nonexistent_book(pool: PgPool) {
        let app = setup_transactional_test_app(pool).await;
        let req = Request::builder()
            .method("PATCH")
            .uri("/books/99999")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"author":"A","title":"T"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        // The update_book handler returns OK even if the book doesn't exist
        // because it returns the rows_affected as i32, which will be 0 for non-existent books
        assert_eq!(response.status(), StatusCode::OK);

        let json = get_response_json(response).await;
        assert_eq!(json, 0); // 0 rows affected
    }

    #[sqlx::test]
    async fn test_update_book_invalid_json(pool: PgPool) {
        let book_id = db::create_book(
            &pool,
            "Author".to_string(),
            "Title".to_string(),
            BookStatus::Available,
        )
        .await
        .unwrap();

        let app = setup_transactional_test_app(pool).await;
        let req = Request::builder()
            .method("PATCH")
            .uri(format!("/books/{}", book_id))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"invalid json"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[sqlx::test]
    async fn test_create_book_success(pool: PgPool) {
        let app = setup_transactional_test_app(pool.clone()).await;
        let req = Request::builder()
            .method("POST")
            .uri("/books/add")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"author":"New Author","title":"New Title"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let json = get_response_json(response).await;
        let book_id: i32 = json.as_i64().unwrap() as i32;

        // Verify the book was actually created
        let created_book = db::get_book(&pool, book_id).await.unwrap();
        assert_eq!(created_book.author, "New Author");
        assert_eq!(created_book.title, "New Title");
        assert!(matches!(created_book.status, BookStatus::Available));
    }

    #[sqlx::test]
    async fn test_create_book_invalid_json(pool: PgPool) {
        let app = setup_transactional_test_app(pool).await;
        let req = Request::builder()
            .method("POST")
            .uri("/books/add")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"invalid": json}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[sqlx::test]
    async fn test_create_book_missing_fields(pool: PgPool) {
        let app = setup_transactional_test_app(pool).await;
        let req = Request::builder()
            .method("POST")
            .uri("/books/add")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"author":"Author Only"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        // Axum returns 422 UNPROCESSABLE_ENTITY for missing required fields during JSON deserialization
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[sqlx::test]
    async fn test_bulk_create_books_success(pool: PgPool) {
        let app = setup_transactional_test_app(pool.clone()).await;
        let req = Request::builder()
            .method("POST")
            .uri("/books/bulk_add")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"[{"author":"Author1","title":"Title1"},{"author":"Author2","title":"Title2"}]"#,
            ))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let json = get_response_json(response).await;
        assert!(json.is_array(), "Response should be an array of book IDs");
        let book_ids = json.as_array().unwrap();
        assert_eq!(book_ids.len(), 2);

        // Verify both books were created
        for book_id_value in book_ids {
            let book_id = book_id_value.as_i64().unwrap() as i32;
            let book = db::get_book(&pool, book_id).await.unwrap();
            assert!(["Author1", "Author2"].contains(&book.author.as_str()));
            assert!(["Title1", "Title2"].contains(&book.title.as_str()));
        }
    }

    #[sqlx::test]
    async fn test_bulk_create_books_empty_array(pool: PgPool) {
        let app = setup_transactional_test_app(pool).await;
        let req = Request::builder()
            .method("POST")
            .uri("/books/bulk_add")
            .header("content-type", "application/json")
            .body(Body::from(r#"[]"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let json = get_response_json(response).await;
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[sqlx::test]
    async fn test_bulk_create_books_invalid_json(pool: PgPool) {
        let app = setup_transactional_test_app(pool).await;
        let req = Request::builder()
            .method("POST")
            .uri("/books/bulk_add")
            .header("content-type", "application/json")
            .body(Body::from(r#"not an array"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[sqlx::test]
    async fn test_delete_existing_book(pool: PgPool) {
        // Create a book to delete
        let book_id = db::create_book(
            &pool,
            "To Delete Author".to_string(),
            "To Delete Title".to_string(),
            BookStatus::Available,
        )
        .await
        .unwrap();

        let app = setup_transactional_test_app(pool.clone()).await;
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/books/{}", book_id))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify the book is actually deleted by trying to fetch it
        let get_req = Request::builder()
            .uri(format!("/books/{}", book_id))
            .body(Body::empty())
            .unwrap();
        let get_response = app.oneshot(get_req).await.unwrap();
        assert_eq!(get_response.status(), StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn test_delete_nonexistent_book(pool: PgPool) {
        let app = setup_transactional_test_app(pool).await;
        let req = Request::builder()
            .method("DELETE")
            .uri("/books/99999")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        // The delete_book handler returns OK even if the book doesn't exist
        // because it doesn't check if the deletion actually affected any rows
        assert_eq!(response.status(), StatusCode::OK);
    }
}
