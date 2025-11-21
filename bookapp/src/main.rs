use bookapp::book_ingestion;
use bookapp::database;
use bookapp::error_injection_middleware;
use bookapp::rest;
use bookapp::topic_management;

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use axum::{Extension, Json, Router};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use opentelemetry::metrics::MeterProvider;
use rdkafka::{config::FromClientConfig, producer::FutureProducer};
use sentry_tower::NewSentryLayer;
use serde_json::{json, Value};
use tokio::signal::unix::{signal, SignalKind};

use bookapp::database::DatabasePools;
use error_injection_dal::ErrorInjectionRepository;

use tracing::info;

async fn health() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "bookapp"
    }))
}

fn router(db_pools: DatabasePools, producer: FutureProducer) -> Router {
    // Create the ErrorInjectionConfigStore with caching
    let error_injection_repo =
        ErrorInjectionRepository::new(db_pools.write_pool.clone(), db_pools.read_pool.clone());
    let postgres_store = Arc::new(
        error_injection_middleware::PostgresErrorInjectionConfigStore::new(error_injection_repo),
    ) as Arc<dyn error_injection_middleware::ErrorInjectionConfigStore>;

    let error_injection_store =
        Arc::new(error_injection_middleware::CachedErrorInjectionConfigStore::new(postgres_store))
            as Arc<dyn error_injection_middleware::ErrorInjectionConfigStore>;

    Router::new()
        .merge(rest::openapi_router())
        .layer(Extension(producer))
        // Our custom error injection layer can inject errors
        // This layer itself can be traced - so needs to be added before our OtelAxumLayer
        .layer(axum::middleware::from_fn_with_state(
            error_injection_store.clone(),
            error_injection_middleware::error_injection_middleware,
        ))
        .nest_service(
            "/error-injection",
            error_injection_middleware::error_injection_service(error_injection_store.clone()),
        )
        .layer(Extension(db_pools))
        // Sentry Tower middleware for HTTP request tracking and error capture
        .layer(NewSentryLayer::new_from_top())
        // This layer creates a new Tracing span called "request" for each request,
        // it logs headers etc but on its own doesn't do the OTEL trace context propagation.
        // .layer(ServiceBuilder::new().layer(
        //     TraceLayer::new_for_http()
        //         .make_span_with(DefaultMakeSpan::new()
        //             .include_headers(true)
        //             .level(tracing::Level::INFO))
        //
        // ))
        // include trace context as header into the response
        .layer(OtelInResponseLayer)
        // start OpenTelemetry trace on incoming request
        // as long as not filtered out!
        .layer(OtelAxumLayer::default())
        .layer(
            tower_otel_http_metrics::HTTPMetricsLayerBuilder::builder()
                .with_meter(opentelemetry::global::meter(env!("CARGO_CRATE_NAME")))
                .build()
                .expect("Failed to build otel metrics layer"),
        )
        // Other non-traced routes can go after this:
        .route("/health", axum::routing::get(health)) // request processed without span / trace
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load env vars
    dotenv::dotenv().ok();
    let enable_kafka_producer =
        std::env::var("ENABLE_KAFKA_PRODUCER").unwrap_or_else(|_| "false".to_string()) == "true";

    let observability_config =
        observability_utils::ObservabilityConfig::new("bookapp").with_per_task_tracking(true);
    let (trace_provider, meter_provider, log_provider, sentry_guard, _task_tracking_registrations) =
        observability_utils::init_tracing(observability_config.clone());

    info!("Tracing initialized successfully");

    // Initialize Tokio metrics collection
    if let Ok(tokio_registrations) =
        observability_utils::start_tokio_metrics(&observability_config, &meter_provider)
    {
        info!(
            "Successfully initialized {} Tokio metric registrations",
            tokio_registrations.len()
        );
    } else {
        info!("Failed to initialize Tokio metrics");
    }

    if let Ok(task_registrations) =
        observability_utils::start_task_metrics(&observability_config, &meter_provider)
    {
        info!(
            "Successfully initialized {} task metric registrations",
            task_registrations.len()
        );
    } else {
        info!("Failed to initialize task metrics");
    }

    // Initialize per-task tracking metrics
    if let Ok(per_task_registrations) =
        observability_utils::start_per_task_tracking(&observability_config, &meter_provider).await
    {
        info!(
            "Successfully initialized {} per-task tracking metric registrations",
            per_task_registrations.len()
        );
    } else {
        info!("Failed to initialize per-task tracking");
    }

    // Init db
    info!("Setting up Database");
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    // High-throughput database pool configuration (env-overridable)
    let db_config = database::DatabaseConfig {
        max_connections: env_u32("DATABASE_POOL_MAX_CONNECTIONS", 100),
        min_connections: env_u32("DATABASE_POOL_MIN_CONNECTIONS", 20),
        acquire_timeout: env_duration_millis("DATABASE_POOL_ACQUIRE_TIMEOUT_MS", 1_000),
        idle_timeout: env_duration_secs("DATABASE_POOL_IDLE_TIMEOUT_SECS", 30),
        max_lifetime: env_duration_secs("DATABASE_POOL_MAX_LIFETIME_SECS", 600),
    };
    let db_pools = DatabasePools::single(&db_url, Some(db_config)).await?;

    info!("Creating simple test metrics to debug OTLP export...");

    // Create a simple counter that should definitely work
    let test_meter = meter_provider.meter("debug_test");
    let simple_counter = test_meter.u64_counter("debug_simple_counter").build();
    simple_counter.add(42, &[]);
    info!("Created simple counter with value 42");

    // Force a metric export immediately
    if let Err(e) = meter_provider.force_flush() {
        info!("Error force-flushing metrics: {:?}", e);
    } else {
        info!("Successfully force-flushed metrics");
    }

    if enable_kafka_producer {
        // Create Kafka admin client
        let admin_client = topic_management::create_admin_client()?;

        // Ensure the topic exists
        topic_management::ensure_topic_exists(&admin_client, "book_ingestion").await?;
        info!("Setting up Kafka Producer");

        // Initialize Kafka producer
        let producer: FutureProducer = book_ingestion::create_producer()?;

        // Build the application router
        let app = router(db_pools, producer);

        // Start the server
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;

        info!("Starting webserver");
        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            let mut signal_terminate = signal(SignalKind::terminate()).unwrap();
            let mut signal_interrupt = signal(SignalKind::interrupt()).unwrap();

            tokio::select! {
                _ = signal_terminate.recv() => tracing::debug!("Received SIGTERM."),
                _ = signal_interrupt.recv() => tracing::debug!("Received SIGINT."),
            }
        });

        tokio::select! {
            _ = server => tracing::info!("Server has shut down gracefully."),
            else => tracing::error!("Server encountered an error."),
        }
    } else {
        info!("Running without Kafka producer - creating minimal server");

        // Build the application router without producer
        let producer: FutureProducer =
            rdkafka::producer::FutureProducer::from_config(&rdkafka::ClientConfig::new())
                .expect("Failed to create minimal producer");
        let app = router(db_pools, producer);

        // Start the server
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
        info!("Starting webserver without Kafka");

        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            let mut signal_terminate = signal(SignalKind::terminate()).unwrap();
            let mut signal_interrupt = signal(SignalKind::interrupt()).unwrap();

            tokio::select! {
                _ = signal_terminate.recv() => tracing::debug!("Received SIGTERM."),
                _ = signal_interrupt.recv() => tracing::debug!("Received SIGINT."),
            }
        });

        tokio::select! {
            _ = server => tracing::info!("Server has shut down gracefully."),
            else => tracing::error!("Server encountered an error."),
        }
    }

    info!("Shutting down OpenTelemetry");

    if let Err(e) = trace_provider.shutdown() {
        tracing::error!("Error shutting down trace provider: {:?}", e);
    }
    if let Err(e) = meter_provider.shutdown() {
        tracing::error!("Error shutting down meter provider: {:?}", e);
    }
    if let Err(e) = log_provider.shutdown() {
        tracing::error!("Error shutting down log provider: {:?}", e);
    }

    // Keep Sentry guard alive until here, then let it drop naturally for clean shutdown
    drop(sentry_guard);

    info!("Shutdown complete");

    Ok(())
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

fn env_duration_secs(key: &str, default_secs: u64) -> Duration {
    Duration::from_secs(env_u64(key, default_secs))
}

fn env_duration_millis(key: &str, default_ms: u64) -> Duration {
    Duration::from_millis(env_u64(key, default_ms))
}
