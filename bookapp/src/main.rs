mod book_ingestion;
mod db;
mod reqwest_traced_client;
mod rest;
mod topic_management;
mod tracing_config;

use opentelemetry::global;

use tracing_subscriber;

use anyhow::{Ok, Result};
use axum::{Extension, Router};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tokio::signal::unix::{signal, SignalKind};

use crate::db::init_db;
use sqlx::PgPool;
use tokio::task;
use tracing::info;

fn router(connection_pool: PgPool, producer: FutureProducer) -> Router {
    Router::new()
        .nest_service("/books", rest::book_service())
        .layer(Extension(connection_pool))
        .layer(Extension(producer))
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

    tracing_config::init_tracing();

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
        axum::serve(listener, app)
            .with_graceful_shutdown(async {

                let mut signal_terminate = signal(SignalKind::terminate()).unwrap();
                let mut signal_interrupt = signal(SignalKind::interrupt()).unwrap();

                tokio::select! {
                    _ = signal_terminate.recv() => tracing::debug!("Received SIGTERM."),
                    _ = signal_interrupt.recv() => tracing::debug!("Received SIGINT."),
                }
            })
            .await?;
    }

    info!("Shutting down");
    // Shutdown OpenTelemetry
    global::shutdown_tracer_provider();
    info!("Shut down");

    Ok(())
}
