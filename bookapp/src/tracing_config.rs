use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

pub fn init_tracing() {
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    // Uses OTEL_EXPORTER_OTLP_TRACES_ENDPOINT
    // Assumes a GRPC endpoint (e.g port 4317)
    let exporter = opentelemetry_otlp::new_exporter().tonic();

    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default()
                .with_resource(opentelemetry_sdk::Resource::default()),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .expect("Failed to create tracer provider");

    // Explicitly set the tracer provider globally. Note this is now required
    // https://github.com/open-telemetry/opentelemetry-rust/blob/main/opentelemetry-otlp/CHANGELOG.md#v0170
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    // Filter the tracing layer - we can add custom filters that only impact the tracing layer
    let tracing_level_filter = tracing_subscriber::filter::Targets::new()
        .with_target("bookapp", tracing::Level::TRACE)
        .with_target("backend", tracing::Level::TRACE)
        .with_target("sqlx", tracing::Level::DEBUG)
        .with_target("tower_http", tracing::Level::INFO)
        .with_target("hyper_util", tracing::Level::INFO)
        .with_target("h2", tracing::Level::WARN)
        // Note an optional feature flag crate sets this most important trace from tracing to info level
        .with_target("otel::tracing", tracing::Level::INFO)
        .with_default(tracing::Level::INFO);

    // turn our OTLP pipeline into a tracing layer
    let tracing_opentelemetry_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer_provider.tracer("bookapp"))
        .with_filter(tracing_level_filter);

    // Configure the stdout fmt layer
    let fmt_layer = tracing_subscriber::fmt::layer();

    let subscriber = tracing_subscriber::Registry::default()
        .with(fmt_layer.with_filter(tracing_subscriber::EnvFilter::from_default_env()))
        .with(tracing_opentelemetry_layer);

    // Set the subscriber as the global default
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");
}
