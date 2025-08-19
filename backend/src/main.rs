mod book_enrichment;
mod book_ingestion;
mod scheduled_tasks;
mod sentry_correlation;
mod tracing_config;

use anyhow::Result;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;
use tracing::info;

use bookapp_dal::BookRepositoryImpl;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenv::dotenv().ok();

    // Initialize tracing and observability
    let (trace_provider, meter_provider, log_provider, sentry_guard) =
        tracing_config::init_tracing();

    info!("Starting backend service");

    // Initialize database pool
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    let db_pool = Arc::new(db_pool);

    // Create repository for database operations
    let book_repository = Arc::new(BookRepositoryImpl::new(db_pool.clone(), db_pool.clone()));

    // Start background services with cancellation support
    let (shutdown_tx, shutdown_rx) = watch::channel::<bool>(false);

    let mut kafka_task = tokio::spawn({
        let mut rx = shutdown_rx.clone();
        let book_repository = book_repository.clone();
        async move {
            tokio::select! {
                res = book_ingestion::run_consumer(book_repository) => {
                    if let Err(e) = res {
                        tracing::error!("Kafka consumer error: {:?}", e);
                    }
                }
                _ = async {
                    while !*rx.borrow() {
                        if rx.changed().await.is_err() {
                            break;
                        }
                    }
                } => {
                    info!("Shutdown signal received for Kafka consumer task");
                }
            }
        }
    });

    let mut scheduler_task = tokio::spawn({
        let rx_for_sched = shutdown_rx.clone();
        let mut rx_for_wait = shutdown_rx.clone();
        let book_repository = book_repository.clone();
        async move {
            tokio::select! {
                res = scheduled_tasks::start_scheduler_with_shutdown(book_repository, rx_for_sched) => {
                    if let Err(e) = res {
                        tracing::error!("Scheduler error: {:?}", e);
                    }
                }
                _ = async {
                    while !*rx_for_wait.borrow() {
                        if rx_for_wait.changed().await.is_err() {
                            break;
                        }
                    }
                } => {
                    info!("Shutdown signal received for Scheduler task");
                }
            }
        }
    });

    // Wait for shutdown signal
    let mut signal_terminate = signal(SignalKind::terminate())?;
    let mut signal_interrupt = signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = signal_terminate.recv() => {
            info!("Received SIGTERM, initiating shutdown");
        }
        _ = signal_interrupt.recv() => {
            info!("Received SIGINT, initiating shutdown");
        }
        _ = &mut kafka_task => {
            tracing::error!("Kafka consumer task terminated unexpectedly");
        }
        _ = &mut scheduler_task => {
            tracing::error!("Scheduler task terminated unexpectedly");
        }
    }

    // Broadcast shutdown and wait for tasks to finish
    let _ = shutdown_tx.send(true);
    info!("Waiting for background tasks to stop...");
    let _ = kafka_task.await;
    let _ = scheduler_task.await;

    info!("Shutting down OpenTelemetry");

    // Shutdown OpenTelemetry providers
    if let Err(e) = trace_provider.shutdown() {
        tracing::error!("Error shutting down trace provider: {:?}", e);
    }
    if let Err(e) = meter_provider.shutdown() {
        tracing::error!("Error shutting down meter provider: {:?}", e);
    }
    if let Err(e) = log_provider.shutdown() {
        tracing::error!("Error shutting down log provider: {:?}", e);
    }

    // Keep Sentry guard alive until here
    drop(sentry_guard);

    info!("Backend service shutdown complete");
    Ok(())
}
