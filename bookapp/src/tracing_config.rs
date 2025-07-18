use crate::sentry_correlation::SentryOtelCorrelationLayer;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{LogExporter, WithExportConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

fn init_meter_provider() -> Result<SdkMeterProvider, opentelemetry_otlp::ExporterBuildError> {
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_timeout(std::time::Duration::from_secs(10))
        .build()?;

    let provider = SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    "bookapp",
                )])
                .build(),
        )
        .build();

    let cloned_provider = provider.clone();
    opentelemetry::global::set_meter_provider(cloned_provider);
    Ok(provider)
}

fn init_logger_provider() -> Result<SdkLoggerProvider, opentelemetry_otlp::ExporterBuildError> {
    // Note Opentelemetry does not provide a global API to manage the logger provider.
    let exporter = LogExporter::builder().with_tonic().build()?;

    Ok(SdkLoggerProvider::builder()
        //.with_resource()
        .with_batch_exporter(exporter)
        .build())
}

pub fn init_tracing() -> (
    SdkTracerProvider,
    SdkMeterProvider,
    SdkLoggerProvider,
    sentry::ClientInitGuard,
) {
    // Initialize Sentry first - inline to avoid guard dropping
    let sentry_dsn = std::env::var("SENTRY_DSN").unwrap_or_else(|_| {
        tracing::warn!("SENTRY_DSN environment variable not set - Sentry integration disabled");
        String::new()
    });

    let service_name = std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "bookapp".to_string());
    let environment =
        std::env::var("SENTRY_ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
    let release = std::env::var("SENTRY_RELEASE").unwrap_or_else(|_| format!("{service_name}@dev"));

    let sentry_guard = if sentry_dsn.is_empty() {
        // If no DSN provided, initialize with default (disabled) options
        sentry::init(sentry::ClientOptions::default())
    } else {
        sentry::init((
            sentry_dsn,
            sentry::ClientOptions {
                release: Some(release.into()),
                environment: Some(environment.into()),
                traces_sample_rate: 0.1, // Sample 10% of transactions for performance monitoring
                debug: false,            // Disable debug mode for production
                enable_logs: true,       // Enable structured log capture
                before_send: Some(Arc::new(move |mut event| {
                    // Filter out health check and metrics endpoints
                    if let Some(request) = &event.request {
                        if let Some(url) = &request.url {
                            let url_str = url.as_str();
                            if url_str.contains("/health") || url_str.contains("/metrics") {
                                return None;
                            }
                        }
                    }

                    // Add service context
                    event
                        .tags
                        .insert("service".to_string(), "bookapp".to_string());

                    // Remove sensitive server information
                    event.server_name = None;

                    Some(event)
                })),
                send_default_pii: false, // Disable PII by default for security
                ..Default::default()
            },
        ))
    };

    // Set up OpenTelemetry propagation
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    // Metrics
    let meter_provider = init_meter_provider().unwrap();
    let opentelemetry_metrics_layer =
        tracing_opentelemetry::MetricsLayer::new(meter_provider.clone());

    // Tracing
    // Uses OTEL_EXPORTER_OTLP_TRACES_ENDPOINT
    // Assumes a GRPC endpoint (e.g., port 4317)
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to create OTLP span exporter");

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    "bookapp",
                )])
                .build(),
        )
        .build();

    // Explicitly set the tracer provider globally
    // Setting global tracer provider is required if other parts of the application
    // uses global::tracer() or global::tracer_with_version() to get a tracer.
    // Cloning simply creates a new reference to the same tracer provider. It is
    // important to hold on to the tracer_provider here, to invoke
    // shutdown on it when application ends.
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
    let service_name = std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "bookapp".to_string());
    let tracing_opentelemetry_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer_provider.tracer(service_name))
        .with_filter(tracing_level_filter);

    // Configure the stdout fmt layer
    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact();

    let stdout_layer = tracing_subscriber::fmt::layer().event_format(format);

    // Layer that directly sends log events to OTEL
    // Note this won't have trace context because that's only known about by the tracing system
    // not the opentelemetry system. https://github.com/open-telemetry/opentelemetry-rust/issues/1378
    let log_provider = init_logger_provider().unwrap();
    // Add a tracing filter to filter events from crates used by opentelemetry-otlp.
    // The filter levels are set as follows:
    // - Allow `info` level and above by default.
    // - Restrict `hyper`, `tonic`, and `reqwest` to `error` level logs only.
    // This ensures events generated from these crates within the OTLP Exporter are not looped back,
    // thus preventing infinite event generation.
    // Note: This will also drop events from these crates used outside the OTLP Exporter.
    // For more details, see: https://github.com/open-telemetry/opentelemetry-rust/issues/761
    let otel_log_filter =
        tracing_subscriber::EnvFilter::new("info,backend=debug,bookapp=debug,sqlx=info")
            .add_directive("hyper=error".parse().unwrap())
            .add_directive("tonic=error".parse().unwrap())
            .add_directive("reqwest=error".parse().unwrap());

    let otel_log_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&log_provider)
            .with_filter(otel_log_filter);

    // Sentry tracing layer for error capture and performance monitoring
    // Configure to capture errors and warnings with OpenTelemetry correlation
    let sentry_layer =
        sentry::integrations::tracing::layer().event_filter(|md| match *md.level() {
            tracing::Level::ERROR => sentry::integrations::tracing::EventFilter::Event,
            tracing::Level::WARN => sentry::integrations::tracing::EventFilter::Breadcrumb,
            tracing::Level::INFO => sentry::integrations::tracing::EventFilter::Log,
            tracing::Level::DEBUG => sentry::integrations::tracing::EventFilter::Ignore,
            _ => sentry::integrations::tracing::EventFilter::Ignore,
        });

    // Build the subscriber by combining layers
    // IMPORTANT: Layer order matters!
    // 1. OpenTelemetry layer creates trace context
    // 2. Custom correlation layer extracts OTel context for Sentry
    // 3. Sentry layer captures events with correlation
    let subscriber = tracing_subscriber::Registry::default()
        .with(
            console_subscriber::ConsoleLayer::builder()
                .with_default_env()
                .server_addr(([0, 0, 0, 0], 6669))
                .spawn(),
        )
        .with(tracing_opentelemetry_layer) // OpenTelemetry layer first to create trace context
        .with(SentryOtelCorrelationLayer::new()) // Custom layer to add OTel context to Sentry
        .with(sentry_layer) // Sentry layer captures events with correlation
        .with(otel_log_layer)
        .with(opentelemetry_metrics_layer)
        .with(stdout_layer.with_filter(tracing_subscriber::EnvFilter::from_default_env()));

    // Set the subscriber as the global default
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");

    // Return the tracer, meter and logger provider as a tuple for shutdown
    (tracer_provider, meter_provider, log_provider, sentry_guard)
}
