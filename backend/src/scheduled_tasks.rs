use anyhow::Result;
use bookapp_dal::{BookFilterParams, BookRepository, BookRepositoryImpl, BookStatus};
use std::sync::Arc;
use tokio::sync::watch;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info, instrument};

use crate::book_enrichment::BookEnrichmentService;

/// Starts the scheduler for periodic background tasks
#[instrument(skip(book_repository))]
pub async fn start_scheduler(book_repository: Arc<BookRepositoryImpl>) -> Result<()> {
    // Backward-compatible behavior: create a shutdown channel that never triggers.
    // Callers that need cooperative shutdown should use `start_scheduler_with_shutdown`.
    let (_tx, rx) = watch::channel::<bool>(false);
    start_scheduler_with_shutdown(book_repository, rx).await
}

#[instrument(skip(book_repository, shutdown))]
pub async fn start_scheduler_with_shutdown(
    book_repository: Arc<BookRepositoryImpl>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    info!("Starting scheduled tasks scheduler");

    let scheduler = JobScheduler::new().await?;

    // Create enrichment service
    let enrichment_service = Arc::new(BookEnrichmentService::new(book_repository.clone()));

    // Schedule book enrichment to run every hour
    let enrichment_job = {
        let enrichment_service = enrichment_service.clone();
        Job::new_async("0 0 * * * *", move |_uuid, _l| {
            let enrichment_service = enrichment_service.clone();
            Box::pin(async move {
                if let Err(e) = enrichment_service.enrich_all_books().await {
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

    // Add jobs to scheduler
    scheduler.add(enrichment_job).await?;
    scheduler.add(refresh_job).await?;
    scheduler.add(stats_job).await?;
    scheduler.add(cleanup_job).await?;

    // Start the scheduler
    scheduler.start().await?;

    info!("Scheduled tasks scheduler started successfully");

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
