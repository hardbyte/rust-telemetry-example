#[cfg(test)]
mod tests {

    use crate::book_details::{BookDetailsProvider, StubBookDetailsProvider};
    use crate::book_ingestion;
    use crate::database::DatabasePools;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Extension,
    };
    use bookapp_dal::models::BookCreateInput;
    use bookapp_dal::models::BookStatus;
    use bookapp_dal::repository::{
        BookRepository, BookRepositoryImpl, EditionRepositoryImpl, EventRepositoryImpl,
        SeriesRepositoryImpl,
    };
    use bookapp_dal::repository::{EditionRepository, EventRepository, SeriesRepository};
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
        let db_pools = DatabasePools {
            write_pool: Arc::new(pool.clone()),
            read_pool: Arc::new(pool),
        };
        axum::Router::new()
            .nest_service("/books", crate::rest::book_service())
            .layer(Extension(
                Arc::new(StubBookDetailsProvider) as Arc<dyn BookDetailsProvider>
            ))
            .layer(Extension(db_pools))
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
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool.clone()));
        let input = BookCreateInput {
            work_title: "Test Title".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Test Author".to_string()),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        let app = setup_transactional_test_app(pool).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/books/{book_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let json = get_response_json(response).await;
        assert_eq!(json["id"], book_id);
        assert_eq!(json["work_title"], "Test Title");
        assert_eq!(json["primary_author_name"], "Test Author");
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
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool.clone()));
        let input = BookCreateInput {
            work_title: "Original Title".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Original Author".to_string()),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        let app = setup_transactional_test_app(pool.clone()).await;
        let req = Request::builder()
            .method("PATCH")
            .uri(format!("/books/{book_id}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"work_title":"Updated Title"}"#))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify the book was actually updated
        let updated_book = repo.find_by_id(book_id).await.unwrap().unwrap();
        assert_eq!(updated_book.work_title, "Updated Title");
    }

    #[sqlx::test]
    async fn test_update_nonexistent_book(pool: PgPool) {
        let app = setup_transactional_test_app(pool).await;
        let req = Request::builder()
            .method("PATCH")
            .uri("/books/99999")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"work_title":"T","primary_author_name":"A"}"#,
            ))
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
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool.clone()));
        let input = BookCreateInput {
            work_title: "Title".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Author".to_string()),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        let app = setup_transactional_test_app(pool).await;
        let req = Request::builder()
            .method("PATCH")
            .uri(format!("/books/{book_id}"))
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
            .body(Body::from(
                r#"{"work_title":"New Title","primary_author_name":"New Author"}"#,
            ))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let json = get_response_json(response).await;
        let book_id: i32 = json.as_i64().unwrap() as i32;

        // Verify the book was actually created
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool.clone()));
        let created_book = repo.find_by_id(book_id).await.unwrap().unwrap();
        assert_eq!(created_book.primary_author_name, "New Author");
        assert_eq!(created_book.work_title, "New Title");
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
            .body(Body::from(r#"{"primary_author_name":"Author Only"}"#))
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
                r#"[{"work_title":"Title1","primary_author_name":"Author1"},{"work_title":"Title2","primary_author_name":"Author2"}]"#,
            ))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let json = get_response_json(response).await;
        assert!(json.is_array(), "Response should be an array of book IDs");
        let book_ids = json.as_array().unwrap();
        assert_eq!(book_ids.len(), 2);

        // Verify both books were created
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool.clone()));
        for book_id_value in book_ids {
            let book_id = book_id_value.as_i64().unwrap() as i32;
            let book = repo.find_by_id(book_id).await.unwrap().unwrap();
            assert!(["Author1", "Author2"].contains(&book.primary_author_name.as_str()));
            assert!(["Title1", "Title2"].contains(&book.work_title.as_str()));
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
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool.clone()));
        let input = BookCreateInput {
            work_title: "To Delete Title".to_string(),
            primary_author_id: None,
            primary_author_name: Some("To Delete Author".to_string()),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        let app = setup_transactional_test_app(pool.clone()).await;
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/books/{book_id}"))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify the book is actually deleted by trying to fetch it
        let get_req = Request::builder()
            .uri(format!("/books/{book_id}"))
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

        // The delete_book handler should return NOT_FOUND if the book doesn't exist
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // Helper to setup a full test app including all new services
    async fn setup_full_test_app(pool: PgPool) -> axum::Router {
        dotenv().ok();
        let producer: FutureProducer = book_ingestion::create_producer().unwrap();
        let db_pools = DatabasePools {
            write_pool: Arc::new(pool.clone()),
            read_pool: Arc::new(pool),
        };
        crate::rest::api_router()
            .layer(Extension(
                Arc::new(StubBookDetailsProvider) as Arc<dyn BookDetailsProvider>
            ))
            .layer(Extension(db_pools))
            .layer(Extension(producer))
    }

    #[sqlx::test]
    async fn test_authors_create_and_list(pool: PgPool) {
        let app = setup_full_test_app(pool.clone()).await;

        // Create an author
        let req = Request::builder()
            .method("POST")
            .uri("/authors/add")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"name":"Test Author","sort_name":"Author, Test"}"#,
            ))
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // List authors
        let list_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/authors")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = list_resp.status();
        if status != StatusCode::OK {
            let body_bytes = axum::body::to_bytes(list_resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let body_str = String::from_utf8_lossy(&body_bytes);
            panic!("GET /authors/ returned {}, body: {}", status, body_str);
        }

        let json = get_response_json(list_resp).await;
        assert!(json.is_array());
        let arr = json.as_array().unwrap();
        assert!(arr.iter().any(|a| a["name"] == "Test Author"));
    }

    #[sqlx::test]
    async fn test_works_create_creates_event(pool: PgPool) {
        let app = setup_full_test_app(pool.clone()).await;

        // Create a work via REST
        let req = Request::builder()
            .method("POST")
            .uri("/works/add")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"Integration Work"}"#))
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let work_id_json = get_response_json(response).await;
        let work_id: i32 = work_id_json.as_i64().unwrap() as i32;

        // Verify an outbox event exists for this work
        let events = EventRepositoryImpl::single_pool(Arc::new(pool))
            .list_for_aggregate("work", &work_id.to_string())
            .await
            .unwrap();

        assert!(
            events.iter().any(|e| e.event_type == "created"),
            "Expected 'created' event for work"
        );
    }

    #[sqlx::test]
    async fn test_editions_create_for_work(pool: PgPool) {
        let app = setup_full_test_app(pool.clone()).await;

        // Create a work first
        let req_work = Request::builder()
            .method("POST")
            .uri("/works/add")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"Edition Work"}"#))
            .unwrap();
        let resp_work = app.clone().oneshot(req_work).await.unwrap();
        assert_eq!(resp_work.status(), StatusCode::CREATED);
        let work_id: i32 = get_response_json(resp_work).await.as_i64().unwrap() as i32;

        // Create an edition for that work
        let req_edition = Request::builder()
            .method("POST")
            .uri("/editions/add")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"work_id":{},"isbn":"1234567890123","title":"Edition Title"}}"#,
                work_id
            )))
            .unwrap();
        let resp_edition = app.clone().oneshot(req_edition).await.unwrap();
        assert_eq!(resp_edition.status(), StatusCode::CREATED);

        let edition_id: i32 = get_response_json(resp_edition).await.as_i64().unwrap() as i32;
        // Verify via DAL
        let edition = EditionRepositoryImpl::single_pool(Arc::new(pool))
            .find_by_id(edition_id)
            .await
            .unwrap()
            .expect("edition exists");
        assert_eq!(edition.work_id, work_id);
        assert_eq!(edition.isbn, "1234567890123");
    }

    #[sqlx::test]
    async fn test_series_create_and_add_work(pool: PgPool) {
        let app = setup_full_test_app(pool.clone()).await;

        // Create a series
        let req_series = Request::builder()
            .method("POST")
            .uri("/series/add")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"Test Series"}"#))
            .unwrap();
        let resp_series = app.clone().oneshot(req_series).await.unwrap();
        assert_eq!(resp_series.status(), StatusCode::CREATED);
        let series_id: i32 = get_response_json(resp_series).await.as_i64().unwrap() as i32;

        // Create a work
        let req_work = Request::builder()
            .method("POST")
            .uri("/works/add")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"Series Work"}"#))
            .unwrap();
        let resp_work = app.clone().oneshot(req_work).await.unwrap();
        assert_eq!(resp_work.status(), StatusCode::CREATED);
        let work_id: i32 = get_response_json(resp_work).await.as_i64().unwrap() as i32;

        // Add work to series
        let req_assoc = Request::builder()
            .method("POST")
            .uri(format!("/series/{}/works/add", series_id))
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"work_id":{},"primary_work":true,"order_id":1}}"#,
                work_id
            )))
            .unwrap();
        let resp_assoc = app.clone().oneshot(req_assoc).await.unwrap();
        assert_eq!(resp_assoc.status(), StatusCode::NO_CONTENT);

        // Verify association via DAL
        let assocs = SeriesRepositoryImpl::single_pool(Arc::new(pool))
            .list_works(series_id)
            .await
            .unwrap();
        assert!(
            assocs
                .iter()
                .any(|a| a.work_id == work_id && a.primary_work),
            "Expected association with primary_work = true"
        );
    }

    #[sqlx::test(migrations = "../bookapp-dal/migrations")]
    async fn test_search_books_endpoint(pool: PgPool) {
        let app = setup_full_test_app(pool.clone()).await;

        // Create test books with searchable content
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool.clone()));
        let test_books = vec![
            BookCreateInput {
                work_title: "The Lord of the Rings".to_string(),
                primary_author_id: None,
                primary_author_name: Some("J.R.R. Tolkien".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "Harry Potter and the Philosopher's Stone".to_string(),
                primary_author_id: None,
                primary_author_name: Some("J.K. Rowling".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "A Game of Thrones".to_string(),
                primary_author_id: None,
                primary_author_name: Some("George R.R. Martin".to_string()),
                status: Some(BookStatus::Available),
            },
        ];
        repo.bulk_create(&test_books).await.unwrap();

        // Refresh materialized view to make books searchable
        sqlx::query("REFRESH MATERIALIZED VIEW book_search_view")
            .execute(repo.write_pool().as_ref())
            .await
            .unwrap();

        // Test successful search
        let req = Request::builder()
            .method("GET")
            .uri("/books/search?q=Harry%20Potter&limit=10")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let json = get_response_json(response).await;
        assert!(json.is_array());
        let results = json.as_array().unwrap();
        assert!(!results.is_empty(), "Should find Harry Potter book");
        assert!(results
            .iter()
            .any(|r| r["work_title"].as_str().unwrap().contains("Harry Potter")));
        assert!(
            results[0]["rank"].is_number(),
            "Results should include relevance rank"
        );

        // Test search by author
        let req = Request::builder()
            .method("GET")
            .uri("/books/search?q=Tolkien&limit=5")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let json = get_response_json(response).await;
        let results = json.as_array().unwrap();
        assert!(!results.is_empty(), "Should find Tolkien's book");
        assert!(results.iter().any(|r| r["primary_author_name"]
            .as_str()
            .unwrap()
            .contains("Tolkien")));

        // Test search with no results
        let req = Request::builder()
            .method("GET")
            .uri("/books/search?q=nonexistent%20book%20title%20xyz&limit=10")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let json = get_response_json(response).await;
        let results = json.as_array().unwrap();
        assert!(
            results.is_empty(),
            "Should return empty array for no matches"
        );

        // Test search with missing query parameter
        let req = Request::builder()
            .method("GET")
            .uri("/books/search")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "Should require 'q' parameter"
        );

        // Test search with limit parameter
        let req = Request::builder()
            .method("GET")
            .uri("/books/search?q=Potter&limit=1")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let json = get_response_json(response).await;
        let results = json.as_array().unwrap();
        assert!(results.len() <= 1, "Should respect limit parameter");
    }
}
