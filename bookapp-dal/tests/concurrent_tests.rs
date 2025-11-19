use std::sync::Arc;

use bookapp_dal::models::{BookCreateInput, BookStatus};
use bookapp_dal::repository::BookRepositoryImpl;
use futures::future::join_all;
use sqlx::PgPool;
use tokio::sync::Semaphore;

fn repository_from(pool: &PgPool) -> Arc<BookRepositoryImpl> {
    Arc::new(BookRepositoryImpl::single_pool(Arc::new(pool.clone())))
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_creations_succeed(pool: PgPool) {
    let repo = repository_from(&pool);

    let tasks = (0..32).map(|idx| {
        let repo = Arc::clone(&repo);
        tokio::spawn(async move {
            repo.create(BookCreateInput {
                work_title: format!("Concurrent Book {idx}"),
                primary_author_id: None,
                primary_author_name: Some(format!("Author {}", idx % 4)),
                status: Some(BookStatus::Available),
            })
            .await
        })
    });

    let results = join_all(tasks).await;
    assert!(results.iter().all(|r| matches!(r, Ok(Ok(_)))));
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_reads_and_writes_do_not_panic(pool: PgPool) {
    let repo = repository_from(&pool);

    let seed_id = repo
        .create(BookCreateInput {
            work_title: "Seed Book".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Seed Author".to_string()),
            status: Some(BookStatus::Available),
        })
        .await
        .unwrap();

    let mut tasks = Vec::new();

    for _ in 0..8 {
        let repo = Arc::clone(&repo);
        tasks.push(tokio::spawn(async move {
            for _ in 0..10 {
                let _ = repo.find_by_id(seed_id).await;
            }
        }));
    }

    for idx in 0..8 {
        let repo = Arc::clone(&repo);
        tasks.push(tokio::spawn(async move {
            for inner in 0..5 {
                let _ = repo
                    .create(BookCreateInput {
                        work_title: format!("Writer {idx} - {inner}"),
                        primary_author_id: None,
                        primary_author_name: Some(format!("Writer {idx}")),
                        status: Some(BookStatus::Available),
                    })
                    .await;
            }
        }));
    }

    let results = join_all(tasks).await;
    assert!(results.into_iter().all(|r| r.is_ok()));
}

#[sqlx::test(migrations = "./migrations")]
async fn test_connection_pool_exhaustion(pool: PgPool) {
    let repo = repository_from(&pool);
    let write_pool = repo.write_pool().clone();
    let semaphore = Arc::new(Semaphore::new(write_pool.size() as usize * 2));

    let tasks: Vec<_> = (0..100)
        .map(|i| {
            let repo = Arc::clone(&repo);
            let sem = Arc::clone(&semaphore);
            tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();

                let input = BookCreateInput {
                    work_title: format!("Pool Test Book {}", i),
                    primary_author_id: None,
                    primary_author_name: Some(format!("Pool Author {}", i % 5)),
                    status: Some(BookStatus::Available),
                };

                match tokio::time::timeout(tokio::time::Duration::from_secs(5), repo.create(input))
                    .await
                {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(e)) => Err(format!("Creation failed: {}", e)),
                    Err(_) => Err("Timeout waiting for pool connection".to_string()),
                }
            })
        })
        .collect();

    let results = join_all(tasks).await;
    let successful = results.iter().filter(|r| matches!(r, Ok(Ok(())))).count();
    assert!(
        successful >= 90,
        "At least 90% of operations should succeed"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_concurrent_search_operations(pool: PgPool) {
    let repo = repository_from(&pool);

    for i in 0..10 {
        let input = BookCreateInput {
            work_title: format!("Searchable Book {}", i),
            primary_author_id: None,
            primary_author_name: Some(format!("Search Author {}", i % 3)),
            status: Some(BookStatus::Available),
        };
        repo.create(input).await.unwrap();
    }

    let tasks: Vec<_> = (0..30)
        .map(|i| {
            let repo = Arc::clone(&repo);
            tokio::spawn(async move {
                repo.find_by_filters(bookapp_dal::models::BookFilterParams {
                    status: None,
                    work_title_pattern: Some(format!("Searchable Book {}", i % 5)),
                    primary_author_pattern: None,
                    limit: Some(5),
                    offset: Some(0),
                })
                .await
            })
        })
        .collect();

    let results = join_all(tasks).await;
    assert!(results.iter().all(|r| matches!(r, Ok(Ok(_)))));
}
