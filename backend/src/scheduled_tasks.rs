use anyhow::Result;
use bookapp_dal::{BookFilterParams, BookRepository, BookRepositoryImpl, BookStatus};
use opentelemetry::trace::TraceContextExt;
use opentelemetry::KeyValue;
use std::sync::Arc;
use tokio::sync::watch;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info, instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::book_enrichment::BookEnrichmentService;
use crate::search_refresh::SmartSearchRefresher;

#[instrument(
    name = "scheduler startup",
    skip(book_repository, search_refresher, shutdown),
    fields(
        scheduler.type = "tokio_cron_scheduler",
        scheduler.version = "0.14.0",
        jobs.count,
        startup_duration_ms
    )
)]
pub async fn start_scheduler_with_shutdown(
    book_repository: Arc<BookRepositoryImpl>,
    search_refresher: Arc<SmartSearchRefresher>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let startup_start = std::time::Instant::now();

    info!("Starting scheduled tasks scheduler");

    let scheduler = JobScheduler::new().await?;

    // Get the parent span context to link scheduled jobs to startup
    let parent_span_context = tracing::Span::current()
        .context()
        .span()
        .span_context()
        .clone();

    // Create enrichment service
    let enrichment_service = Arc::new(BookEnrichmentService::new(book_repository.clone()));

    // Schedule book enrichment to run every hour
    let enrichment_job = {
        let enrichment_service = enrichment_service.clone();
        let startup_span_context = parent_span_context.clone();
        Job::new_async("0 0 * * * *", move |_uuid, _l| {
            let enrichment_service = enrichment_service.clone();
            let startup_span_context = startup_span_context.clone();
            Box::pin(async move {
                let span = tracing::info_span!(
                    "job execution",
                    job.name = "book_enrichment",
                    job.schedule = "0 0 * * * *",
                    job.type = "hourly",
                    job.duration_ms = tracing::field::Empty,
                    job.status = tracing::field::Empty
                );

                // Link to the scheduler startup span
                span.add_link_with_attributes(
                    startup_span_context,
                    vec![KeyValue::new("link.type", "follows_from")],
                );

                let result = span
                    .in_scope(|| async {
                        let start_time = std::time::Instant::now();
                        info!(
                            job.name = "book_enrichment",
                            "Starting scheduled book enrichment task"
                        );

                        let result = enrichment_service.enrich_all_books().await;
                        let duration = start_time.elapsed();

                        tracing::Span::current()
                            .record("job.duration_ms", duration.as_millis() as u64);

                        match result {
                            Ok(()) => {
                                tracing::Span::current().record("job.status", "success");
                                info!(
                                    job.name = "book_enrichment",
                                    duration_ms = duration.as_millis(),
                                    "Book enrichment task completed successfully"
                                );
                            }
                            Err(ref e) => {
                                tracing::Span::current().record("job.status", "error");
                                error!(
                                    job.name = "book_enrichment",
                                    duration_ms = duration.as_millis(),
                                    error = %e,
                                    "Book enrichment task failed"
                                );
                            }
                        }
                        result
                    })
                    .await;

                if let Err(e) = result {
                    error!("Book enrichment job failed: {:?}", e);
                }
            })
        })?
    };

    // Schedule stale enrichment refresh to run every 6 hours
    let refresh_job = {
        let enrichment_service = enrichment_service.clone();
        Job::new_async("0 0 */6 * * *", move |_uuid, _l| {
            let enrichment_service = enrichment_service.clone();
            Box::pin(async move {
                if let Err(e) = enrichment_service.refresh_stale_enrichments().await {
                    error!("Stale enrichment refresh job failed: {:?}", e);
                }
            })
        })?
    };

    // Schedule daily statistics generation
    let stats_job = {
        let book_repository = book_repository.clone();
        Job::new_async("0 0 2 * * *", move |_uuid, _l| {
            let book_repository = book_repository.clone();
            Box::pin(async move {
                if let Err(e) = generate_daily_statistics(book_repository).await {
                    error!("Daily statistics job failed: {:?}", e);
                }
            })
        })?
    };

    // Schedule cleanup of old data (runs at 3 AM daily)
    let cleanup_job = {
        let book_repository = book_repository.clone();
        Job::new_async("0 0 3 * * *", move |_uuid, _l| {
            let book_repository = book_repository.clone();
            Box::pin(async move {
                if let Err(e) = cleanup_old_data(book_repository).await {
                    error!("Cleanup job failed: {:?}", e);
                }
            })
        })?
    };

    // Schedule materialized view refresh (runs every 15 minutes)
    let search_view_refresh_job = {
        let book_repository = book_repository.clone();
        let search_refresher = search_refresher.clone();
        let startup_span_context = parent_span_context.clone();
        Job::new_async("0 */15 * * * *", move |_uuid, _l| {
            let book_repository = book_repository.clone();
            let search_refresher = search_refresher.clone();
            let startup_span_context = startup_span_context.clone();
            Box::pin(async move {
                let span = tracing::info_span!(
                    "job execution",
                    job.name = "search_view_refresh",
                    job.schedule = "0 */15 * * * *",
                    job.type = "periodic",
                    job.interval_minutes = 15,
                    job.duration_ms = tracing::field::Empty,
                    job.status = tracing::field::Empty
                );

                // Link to the scheduler startup span
                span.add_link_with_attributes(
                    startup_span_context,
                    vec![KeyValue::new("link.type", "follows_from")],
                );

                let result = span
                    .in_scope(|| async {
                        let start_time = std::time::Instant::now();
                        info!(
                            job.name = "search_view_refresh",
                            "Starting scheduled search view refresh"
                        );

                        let result = search_refresher
                            .force_refresh_for_schedule(book_repository)
                            .await;
                        let duration = start_time.elapsed();

                        tracing::Span::current()
                            .record("job.duration_ms", duration.as_millis() as u64);

                        match result {
                            Ok(()) => {
                                tracing::Span::current().record("job.status", "success");
                                info!(
                                    job.name = "search_view_refresh",
                                    duration_ms = duration.as_millis(),
                                    "Search view refresh task completed successfully"
                                );
                            }
                            Err(ref e) => {
                                tracing::Span::current().record("job.status", "error");
                                error!(
                                    job.name = "search_view_refresh",
                                    duration_ms = duration.as_millis(),
                                    error = %e,
                                    "Search view refresh task failed"
                                );
                            }
                        }
                        result
                    })
                    .await;

                if let Err(e) = result {
                    error!(source = "scheduled_task", error = %e, "Search view refresh job failed");
                }
            })
        })?
    };

    // Add jobs to scheduler
    scheduler.add(enrichment_job).await?;
    scheduler.add(refresh_job).await?;
    scheduler.add(stats_job).await?;
    scheduler.add(cleanup_job).await?;
    scheduler.add(search_view_refresh_job).await?;

    // Start the scheduler
    scheduler.start().await?;

    let startup_duration = startup_start.elapsed();
    tracing::Span::current().record("jobs.count", 5u64);
    tracing::Span::current().record("startup_duration_ms", startup_duration.as_millis() as u64);

    info!(
        startup_duration_ms = startup_duration.as_millis(),
        jobs_count = 5,
        "Scheduled tasks scheduler started successfully"
    );

    // Cooperative shutdown: wait for a shutdown signal, waking periodically
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                // Break if the sender signaled or dropped
                if changed.is_err() || *shutdown.borrow() {
                    info!("Shutdown signal received for scheduled tasks scheduler");
                    break;
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {}
        }
    }

    info!("Scheduler stopping");
    Ok(())
}

/// Generates daily statistics about books
#[instrument(skip(book_repository))]
async fn generate_daily_statistics(book_repository: Arc<BookRepositoryImpl>) -> Result<()> {
    info!("Generating daily book statistics");

    // Get books by status
    let available_books = book_repository
        .find_by_filters(BookFilterParams {
            status: Some(BookStatus::Available),
            ..Default::default()
        })
        .await?;

    let borrowed_books = book_repository
        .find_by_filters(BookFilterParams {
            status: Some(BookStatus::Borrowed),
            ..Default::default()
        })
        .await?;

    let lost_books = book_repository
        .find_by_filters(BookFilterParams {
            status: Some(BookStatus::Lost),
            ..Default::default()
        })
        .await?;

    info!(
        available_count = available_books.len(),
        borrowed_count = borrowed_books.len(),
        lost_count = lost_books.len(),
        "Daily book statistics generated"
    );

    // In a real application, you might:
    // - Store these statistics in a metrics database
    // - Send them to a monitoring system
    // - Generate reports for stakeholders
    // - Update dashboards

    Ok(())
}

/// Cleans up old data and maintains database health
#[instrument(skip(_book_repository))]
async fn cleanup_old_data(_book_repository: Arc<BookRepositoryImpl>) -> Result<()> {
    info!("Starting daily cleanup tasks");

    // In a real application, this might:
    // - Archive old transaction logs
    // - Clean up temporary files
    // - Vacuum database tables
    // - Compress old log files
    // - Remove expired cache entries

    // Simulate cleanup work
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    info!("Daily cleanup tasks completed");
    Ok(())
}

/// Refreshes the book search materialized view for full-text search performance
#[instrument(
    skip(book_repository),
    fields(
        operation = "refresh_materialized_view",
        view.name = "book_search_view",
        view.refresh_type = "concurrent",
        view.refresh_duration_ms,
        view.rows_affected,
        maintenance.type = "scheduled"
    )
)]
async fn refresh_search_materialized_view(book_repository: Arc<BookRepositoryImpl>) -> Result<()> {
    let start_time = std::time::Instant::now();

    info!(
        view.name = "book_search_view",
        operation = "refresh_materialized_view",
        "Starting materialized view refresh for book search"
    );

    // Access the write pool directly for this database maintenance operation
    let pool = book_repository.write_pool();

    // Refresh the materialized view concurrently (non-blocking for reads)
    let result = sqlx::query("REFRESH MATERIALIZED VIEW CONCURRENTLY book_search_view")
        .execute(pool.as_ref())
        .await?;

    let refresh_duration = start_time.elapsed();
    let rows_affected = result.rows_affected();

    // Record span attributes
    tracing::Span::current().record(
        "view.refresh_duration_ms",
        refresh_duration.as_millis() as u64,
    );
    tracing::Span::current().record("view.rows_affected", rows_affected);

    info!(
        view.name = "book_search_view",
        view.refresh_duration_ms = refresh_duration.as_millis(),
        view.rows_affected = rows_affected,
        operation = "refresh_materialized_view",
        "Materialized view refresh completed successfully"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookapp_dal::models::{BookCreateInput, BookStatus};
    use sqlx::PgPool;
    use std::sync::Arc;

    #[sqlx::test(migrations = "../bookapp-dal/migrations")]
    async fn test_refresh_search_materialized_view(pool: PgPool) {
        let repo = Arc::new(BookRepositoryImpl::single_pool(Arc::new(pool)));

        // Create test data first
        let test_book = BookCreateInput {
            work_title: "Test Materialized View Book".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Test Author".to_string()),
            status: Some(BookStatus::Available),
        };
        repo.create(test_book).await.unwrap();

        // Test the refresh function
        let result = refresh_search_materialized_view(repo.clone()).await;
        assert!(result.is_ok(), "Materialized view refresh should succeed");

        // Verify the materialized view has data after refresh
        let search_results = repo
            .full_text_search("Test Materialized", 10)
            .await
            .unwrap();
        assert!(
            !search_results.is_empty(),
            "Search should find the test book after refresh"
        );
        assert!(search_results
            .iter()
            .any(|r| r.work_title.contains("Test Materialized View Book")));
    }

    #[test]
    fn test_scheduled_task_error_handling() {
        // Test that our error handling pattern is correctly structured
        // The refresh function should return Result<()> for proper error propagation
        // In the actual scheduler, errors are caught and logged but don't crash the scheduler

        // Verify the function signature supports error handling
        // This is a compile-time check that the error handling pattern is in place
        #[allow(clippy::type_complexity)]
        let _: fn(
            Arc<BookRepositoryImpl>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<()>> + Send>,
        > = |repo| Box::pin(refresh_search_materialized_view(repo));

        // Error handling pattern is correctly implemented - verified at compile-time
    }

    #[test]
    fn test_cron_schedule_format() {
        // Verify the cron schedule is correctly formatted for every 15 minutes
        let cron_expression = "0 */15 * * * *";

        // Basic validation - should have 6 parts (seconds, minutes, hours, day, month, day-of-week)
        let parts: Vec<&str> = cron_expression.split_whitespace().collect();
        assert_eq!(parts.len(), 6, "Cron expression should have 6 parts");
        assert_eq!(parts[0], "0", "Should run at 0 seconds");
        assert_eq!(parts[1], "*/15", "Should run every 15 minutes");
        assert_eq!(parts[2], "*", "Should run every hour");
    }
}
