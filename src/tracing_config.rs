use opentelemetry::global;
use opentelemetry_sdk::{
    trace::TracerProvider
};

use tracing::Subscriber;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{
    Layer,
    filter::{LevelFilter},
    util::SubscriberInitExt,
};
use tracing_subscriber::filter::FilterExt;

pub fn init_tracing() {
    // TODO propagator

    // Note this automatically uses OTEL_EXPORTER_OTLP_TRACES_ENDPOINT
    // But assumes a GRPC endpoint (e.g port 4317 rather than 4318)
    let exporter = opentelemetry_otlp::new_exporter().tonic();

    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            opentelemetry_sdk::trace::config()
                .with_resource(opentelemetry_sdk::Resource::default()),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .expect("Failed to create tracer provider");

    // Set the global tracer provider
    //global::set_tracer_provider(tracer_provider.clone());

    // Get a global tracer from the global provider:
    //let tracer = global::tracer("bookapp");

    // Filter the tracing layer - we can add custom filters that only impact the tracing layer
    let tracing_level_filter = tracing_subscriber::filter::Targets::new()
        .with_target("bookapp", tracing::Level::DEBUG)
        .with_target("sqlx", tracing::Level::DEBUG)
        .with_target("tower_http", tracing::Level::INFO)
        .with_default(tracing::Level::INFO);


    // turn our OTLP pipeline into a tracing layer
    let tracing_opentelemetry_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer_provider)
        .with_filter(tracing_level_filter);


    // Configure the stdout fmt layer
    let fmt_layer = tracing_subscriber::fmt::layer();

    let subscriber = tracing_subscriber::Registry::default()
        .with(fmt_layer.with_filter(tracing_subscriber::EnvFilter::from_default_env()))
        .with(tracing_opentelemetry_layer);

    // Set the subscriber as the global default
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set subscriber");

}