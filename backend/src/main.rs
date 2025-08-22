mod book_enrichment;
mod book_ingestion;
mod scheduled_tasks;

use anyhow::Result;
use book_ingestion::OutboxPublisherConfig;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::error::RDKafkaErrorCode::TopicAlreadyExists;
use rdkafka::producer::FutureProducer;
use rdkafka::ClientConfig;
use std::sync::Arc;
use std::time::Duration;
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
    let observability_config = observability_utils::ObservabilityConfig::new("backend")
        .with_console_port(6670);
    let (trace_provider, meter_provider, log_provider, sentry_guard) =
        observability_utils::init_tracing(observability_config.clone());

    info!("Starting backend service");

    // Initialize database pool
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    let db_pool = Arc::new(db_pool);

    // Start tokio runtime metrics collection
    let _tokio_metrics_handle = observability_utils::start_tokio_metrics(&observability_config, &meter_provider);
    
    // Start tokio task-level metrics collection  
    let _task_metrics_handle = observability_utils::start_task_metrics(&observability_config, &meter_provider);

    // Create repository for database operations
    let book_repository = Arc::new(BookRepositoryImpl::new(db_pool.clone(), db_pool.clone()));

    // Kafka producer for outbox publishing
    let kafka_broker =
        std::env::var("KAFKA_BROKER_URL").unwrap_or_else(|_| "kafka:9092".to_string());
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &kafka_broker)
        .set("message.timeout.ms", "5000")
        .set("queue.buffering.max.ms", "50")
        .create()
        .expect("Failed to create Kafka producer");

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
                .unwrap_or(500),
        ),
    };

    // Ensure Kafka topics exist (outbox and book_ingestion)
    let outbox_topic = outbox_config
        .default_topic
        .clone()
        .unwrap_or_else(|| "domain.events".to_string());
    let admin_client: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", &kafka_broker)
        .create()
        .expect("Failed to create Kafka admin client");
    let new_topics = vec![
        NewTopic::new(&outbox_topic, 1, TopicReplication::Fixed(1)),
        NewTopic::new("book_ingestion", 1, TopicReplication::Fixed(1)),
    ];
    match admin_client
        .create_topics(&new_topics, &AdminOptions::new())
        .await
    {
        Ok(results) => {
            for res in results {
                match res {
                    Ok(topic) => tracing::info!(%topic, "Created Kafka topic"),
                    Err((topic, err)) => {
                        if err == TopicAlreadyExists {
                            tracing::info!(%topic, "Kafka topic already exists");
                        } else {
                            tracing::warn!(%topic, ?err, "Failed to create topic");
                        }
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = ?e, "Failed to create Kafka topics");
        }
    }

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

    let mut outbox_task = tokio::spawn({
        let mut rx = shutdown_rx.clone();
        let pool = db_pool.clone();
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
