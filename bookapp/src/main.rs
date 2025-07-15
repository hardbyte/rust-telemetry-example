mod book_ingestion;
mod db;
mod error_injection_middleware;
mod reqwest_traced_client;
mod rest;
mod sentry_correlation;
mod topic_management;
mod tracing_config;
#[cfg(test)]
mod rest_tests;
mod book_details;

use std::sync::Arc;
use crate::book_details::{BookDetailsProvider, RemoteBookDetailsProvider};

use anyhow::{Ok, Result};
use axum::{Extension, Router};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use sentry_tower::NewSentryLayer;
use rdkafka::producer::FutureProducer;
use tokio::signal::unix::{signal, SignalKind};

use crate::db::init_db;
use sqlx::PgPool;
use tokio::task;
use tracing::info;

fn router(connection_pool: PgPool, producer: FutureProducer) -> Router {
    // Create the ErrorInjectionConfigStore
    let error_injection_store = std::sync::Arc::new(
        error_injection_middleware::PostgresErrorInjectionConfigStore::new(connection_pool.clone()),
    )
        as std::sync::Arc<dyn error_injection_middleware::ErrorInjectionConfigStore>;

    Router::new()
        .nest_service("/books", rest::book_service())
        .layer(Extension(Arc::new(RemoteBookDetailsProvider) as Arc<dyn BookDetailsProvider>))
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
        .layer(Extension(connection_pool))
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
        .layer(OtelInResponseLayer::default())
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
    //.route("/health", get(health)) // request processed without span / trace
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load env vars
    dotenv::dotenv().ok();
    let enable_kafka_consumer =
        std::env::var("ENABLE_KAFKA_CONSUMER").unwrap_or_else(|_| "false".to_string()) == "true";
    let enable_kafka_producer =
        std::env::var("ENABLE_KAFKA_PRODUCER").unwrap_or_else(|_| "false".to_string()) == "true";

    let (trace_provider, meter_provider, log_provider, sentry_guard) = tracing_config::init_tracing();

    // Init db
    info!("Setting up Database");
    let connection_pool = init_db().await?;

    // Create Kafka admin client
    let admin_client = topic_management::create_admin_client()?;

    // Ensure the topic exists
    topic_management::ensure_topic_exists(&admin_client, "book_ingestion").await?;

    if enable_kafka_consumer {
        // Start Kafka consumer in a background task
        info!("Starting Kafka consumer");
        task::spawn(async move {
            if let Err(e) = book_ingestion::run_consumer().await {
                tracing::error!("Kafka consumer error: {:?}", e);
            }
        });
    }

    if enable_kafka_producer {
        info!("Setting up Kafka Producer");

        // Initialize Kafka producer
        let producer: FutureProducer = book_ingestion::create_producer()?;

        // Build the application router
        let app = router(connection_pool, producer);

        // Start the server
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;

        info!("Starting webserver");
        let server = axum::serve(listener, app)
            .with_graceful_shutdown(async {
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

