use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

fn init_meter_provider() -> opentelemetry_sdk::metrics::SdkMeterProvider {
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_timeout(std::time::Duration::from_secs(10));

    let provider = opentelemetry_otlp::new_pipeline()
        .metrics(opentelemetry_sdk::runtime::Tokio)
        .with_period(std::time::Duration::from_secs(5))
        .with_timeout(std::time::Duration::from_secs(10))
        .with_exporter(exporter)
        .with_resource(opentelemetry_sdk::Resource::default())
        .with_resource(opentelemetry_sdk::Resource::new(
            vec![opentelemetry::KeyValue::new("service.name", "bookapp")]
        ))
        .build()
        .unwrap();
    let cloned_provider = provider.clone();
    opentelemetry::global::set_meter_provider(cloned_provider);
    provider
}

fn init_logger_provider() -> opentelemetry_sdk::logs::LoggerProvider {
    let exporter = opentelemetry_otlp::new_exporter().tonic();
    let provider = opentelemetry_otlp::new_pipeline()
        .logging()
        .with_exporter(exporter)
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .unwrap();

    provider
}


pub fn init_tracing() {
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    // Uses OTEL_EXPORTER_OTLP_TRACES_ENDPOINT
    // Assumes a GRPC endpoint (e.g port 4317)
    let exporter = opentelemetry_otlp::new_exporter().tonic();

    // Metrics
    let meter_provider = init_meter_provider();
    let opentelemetry_metrics_layer = tracing_opentelemetry::MetricsLayer::new(meter_provider);

    // Tracing
    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default()
                .with_resource(opentelemetry_sdk::Resource::default())
                .with_max_events_per_span(256),
        )
        .with_batch_config(opentelemetry_sdk::trace::BatchConfigBuilder::default()
                               .with_max_queue_size(4096)
                               .with_max_export_batch_size(512)
                               .with_max_concurrent_exports(4)
                               .build()
        )
        // a batch exporter is recommended as the simple exporter will export each span synchronously on dropping
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
    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact();

    let fmt_layer = tracing_subscriber::fmt::layer()
        .event_format(format);


    // Logs to OTEL
    // Note this won't have trace context because that's only known about by the tracing system
    // not the opentelemetry system. https://github.com/open-telemetry/opentelemetry-rust/issues/1378
    let log_provider = init_logger_provider();
    // Add a tracing filter to filter events from crates used by opentelemetry-otlp.
    // The filter levels are set as follows:
    // - Allow `info` level and above by default.
    // - Restrict `hyper`, `tonic`, and `reqwest` to `error` level logs only.
    // This ensures events generated from these crates within the OTLP Exporter are not looped back,
    // thus preventing infinite event generation.
    // Note: This will also drop events from these crates used outside the OTLP Exporter.
    // For more details, see: https://github.com/open-telemetry/opentelemetry-rust/issues/761
    let otel_log_filter = tracing_subscriber::EnvFilter::new("info,backend=debug,bookapp=debug,sqlx=info")
        .add_directive("hyper=error".parse().unwrap())
        .add_directive("tonic=error".parse().unwrap())
        .add_directive("reqwest=error".parse().unwrap());

    let otel_log_layer = opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&log_provider)
        .with_filter(otel_log_filter);

    // Build the subscriber by combining layers
    let subscriber = tracing_subscriber::Registry::default()
        .with(otel_log_layer)
        .with(opentelemetry_metrics_layer)
        .with(tracing_opentelemetry_layer)
        .with(fmt_layer.with_filter(tracing_subscriber::EnvFilter::from_default_env()));

    // Set the subscriber as the global default
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");
}
