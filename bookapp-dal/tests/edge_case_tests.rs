use std::sync::Arc;

use bookapp_dal::models::{BookCreateInput, BookStatus};
use bookapp_dal::repository::BookRepositoryImpl;
use sqlx::PgPool;
use uuid::Uuid;

fn repository_from(pool: &PgPool) -> Arc<BookRepositoryImpl> {
    Arc::new(BookRepositoryImpl::single_pool(Arc::new(pool.clone())))
}

#[sqlx::test(migrations = "./migrations")]
async fn create_requires_author_information(pool: PgPool) {
    let repo = repository_from(&pool);

    let result = repo
        .create(BookCreateInput {
            work_title: "Missing author".to_string(),
            primary_author_id: None,
            primary_author_name: None,
            status: None,
        })
        .await;

    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn long_titles_round_trip(pool: PgPool) {
    let repo = repository_from(&pool);

    let long_title = "A".repeat(512);
    let book_id = repo
        .create(BookCreateInput {
            work_title: long_title.clone(),
            primary_author_id: None,
            primary_author_name: Some("Edge Author".to_string()),
            status: Some(BookStatus::Available),
        })
        .await
        .expect("create long title");

    let stored = repo
        .find_by_id(book_id)
        .await
        .expect("find by id")
        .expect("book present");

    assert_eq!(stored.work_title, long_title);
    assert_eq!(stored.status, BookStatus::Available);
}

#[sqlx::test(migrations = "./migrations")]
async fn unicode_content_is_preserved(pool: PgPool) {
    let repo = repository_from(&pool);

    let title = "📚 漢字とemojiのタイトル";
    let author = "Автор 😀";

    let book_id = repo
        .create(BookCreateInput {
            work_title: title.to_string(),
            primary_author_id: None,
            primary_author_name: Some(author.to_string()),
            status: Some(BookStatus::Borrowed),
        })
        .await
        .expect("create unicode book");

    let stored = repo
        .find_by_id(book_id)
        .await
        .expect("find unicode book")
        .expect("book present");

    assert_eq!(stored.work_title, title);
    assert_eq!(stored.primary_author_name, author);
}

#[sqlx::test(migrations = "./migrations")]
async fn search_respects_limit(pool: PgPool) {
    let repo = repository_from(&pool);

    for idx in 0..8 {
        repo.create(BookCreateInput {
            work_title: format!("Searchable {}", idx),
            primary_author_id: None,
            primary_author_name: Some("Searcher".to_string()),
            status: Some(BookStatus::Available),
        })
        .await
        .unwrap();
    }

    let results = repo
        .find_by_filters(bookapp_dal::models::BookFilterParams {
            status: None,
            work_title_pattern: Some("Searchable".to_string()),
            primary_author_pattern: None,
            limit: Some(3),
            offset: Some(0),
        })
        .await
        .expect("filter books");

    assert!(results.len() <= 3);
}

#[sqlx::test(migrations = "./migrations")]
async fn invalid_ids_return_none(pool: PgPool) {
    let repo = repository_from(&pool);

    let result = repo.find_by_id(Uuid::nil()).await.expect("query succeeds");
    assert!(result.is_none());
}
