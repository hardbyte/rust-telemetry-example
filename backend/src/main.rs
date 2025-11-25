use anyhow::Result;
use backend::{book_ingestion, scheduled_tasks, search_refresh};
use book_ingestion::OutboxPublisherConfig;
use rdkafka::producer::FutureProducer;
use rdkafka::ClientConfig;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;
use tracing::info;

use bookapp_dal::BookRepositoryImpl;
use sqlx::postgres::PgPoolOptions;
use sqlx_tracing::PoolBuilder as TracedPoolBuilder;
use std::str::FromStr;

async fn create_producer_with_retry(kafka_broker: &str) -> Result<FutureProducer> {
    let mut attempts = 0;
    let max_attempts = 30; // ~30 seconds with 1 second sleep
    let retry_delay = Duration::from_secs(1);

    loop {
        attempts += 1;
        tracing::info!(
            "Attempt {}/{} to connect to Kafka brokers at {}",
            attempts,
            max_attempts,
            kafka_broker
        );

        let producer_result = ClientConfig::new()
            .set("bootstrap.servers", kafka_broker)
            .set("message.timeout.ms", "5000")
            .set("queue.buffering.max.ms", "50")
            .create::<FutureProducer>();

        match producer_result {
            Ok(producer) => {
                tracing::info!(
                    "Successfully connected to Kafka after {} attempts.",
                    attempts
                );
                return Ok(producer);
            }
            Err(error) => {
                if attempts >= max_attempts {
                    tracing::error!(
                        ?error,
                        "Failed to connect to Kafka after {} attempts.",
                        max_attempts
                    );
                    return Err(anyhow::anyhow!("Failed to connect to Kafka: {error}"));
                }
                tracing::warn!(
                    ?error,
                    "Kafka connection failed, retrying in {:?}",
                    retry_delay
                );
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenv::dotenv().ok();

    // Initialize tracing and observability
    let observability_config =
        observability_utils::ObservabilityConfig::new("backend").with_console_port(6670);
    let (
        _trace_provider,
        meter_provider,
        _log_provider,
        sentry_guard,
        _task_tracking_registrations,
    ) = observability_utils::init_tracing(observability_config.clone());

    info!("Starting backend service");

    // Initialize database pool
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let max_connections = std::env::var("BACKEND_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(32);
    let raw_pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(&db_url)
        .await?;
    info!(max_connections, "Created backend database pool");

    // Keep raw pool for transactions, wrap a clone for traced queries
    let raw_db_pool = Arc::new(raw_pool.clone());
    let traced_pool = TracedPoolBuilder::from(raw_pool)
        .with_name("backend-pool")
        .with_database("bookapp")
        .with_host("db")
        .with_port(5432)
        .build();
    let db_pool = Arc::new(traced_pool);

    // Start tokio runtime metrics collection
    let _tokio_metrics_handle =
        observability_utils::start_tokio_metrics(&observability_config, &meter_provider);

    // Start tokio task-level metrics collection
    let _task_metrics_handle =
        observability_utils::start_task_metrics(&observability_config, &meter_provider);

    // Create repository for database operations
    let book_repository = Arc::new(BookRepositoryImpl::new(
        db_pool.clone(),
        db_pool.clone(),
        raw_db_pool.clone(),
    ));

    // Configure smart search refresher
    let min_interval_secs = std::env::var("SEARCH_REFRESH_MIN_INTERVAL_SECS")
        .ok()
        .and_then(|v| u64::from_str(&v).ok())
        .unwrap_or(30);
    let max_staleness_secs = std::env::var("SEARCH_REFRESH_MAX_STALENESS_SECS")
        .ok()
        .and_then(|v| u64::from_str(&v).ok())
        .unwrap_or(300);
    let search_refresher = Arc::new(search_refresh::SmartSearchRefresher::new(
        min_interval_secs,
        max_staleness_secs,
    ));
    // Register OTel observable gauges for refresher metrics
    let _refresh_obs_regs = search_refresher.register_observables();

    // Kafka producer for outbox publishing
    let kafka_broker =
        std::env::var("KAFKA_BROKER_URL").unwrap_or_else(|_| "kafka:9092".to_string());
    let producer = create_producer_with_retry(&kafka_broker).await?;

    let dlq_topic =
        std::env::var("BOOK_INGESTION_DLQ_TOPIC").unwrap_or_else(|_| "book_ingestion.dlq".into());

    // Outbox publisher configuration
    let outbox_config = OutboxPublisherConfig {
        default_topic: std::env::var("OUTBOX_DEFAULT_TOPIC").ok(),
        batch_size: std::env::var("OUTBOX_BATCH_SIZE")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(100),
        poll_interval: Duration::from_millis(
            std::env::var("OUTBOX_POLL_MS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(5000),
        ),
    };

    // Start background services with cancellation support
    let (shutdown_tx, shutdown_rx) = watch::channel::<bool>(false);

    let mut kafka_task = tokio::spawn({
        let mut rx = shutdown_rx.clone();
        let book_repository = book_repository.clone();
        let search_refresher = search_refresher.clone();
        let consumer_producer = producer.clone();
        let dlq_topic = dlq_topic.clone();
        async move {
            tokio::select! {
                res = book_ingestion::run_consumer(book_repository, search_refresher, consumer_producer, dlq_topic) => {
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
        let search_refresher = search_refresher.clone();
        async move {
            tokio::select! {
                res = scheduled_tasks::start_scheduler_with_shutdown(book_repository, search_refresher, rx_for_sched) => {
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

    let mut outbox_task = tokio::spawn({
        let mut rx = shutdown_rx.clone();
        let pool = raw_db_pool.clone();
        let producer = producer.clone();
        let config = outbox_config.clone();
        async move {
            tokio::select! {
                res = book_ingestion::start_outbox_publisher(pool, producer, config, rx.clone()) => {
                    if let Err(e) = res {
                        tracing::error!("Outbox publisher error: {:?}", e);
                    }
                }
                _ = async {
                    while !*rx.borrow() {
                        if rx.changed().await.is_err() {
                            break;
                        }
                    }
                } => {
                    info!("Shutdown signal received for Outbox publisher task");
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
        _ = &mut outbox_task => {
            tracing::error!("Outbox publisher task terminated unexpectedly");
        }
    }

    // Broadcast shutdown and wait for tasks to finish
    let _ = shutdown_tx.send(true);
    info!("Waiting for background tasks to stop...");
    let _ = kafka_task.await;
    let _ = scheduler_task.await;
    let _ = outbox_task.await;

    // OpenTelemetry Providers will be dropped on exit
    drop(sentry_guard);

    info!("Backend service shutdown complete");
    Ok(())
}
