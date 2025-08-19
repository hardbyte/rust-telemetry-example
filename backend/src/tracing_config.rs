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

    let service_name = std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "backend".to_string());

    let provider = SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    service_name,
                )])
                .build(),
        )
        .build();

    let cloned_provider = provider.clone();
    opentelemetry::global::set_meter_provider(cloned_provider);
    Ok(provider)
}

fn init_logger_provider() -> Result<SdkLoggerProvider, opentelemetry_otlp::ExporterBuildError> {
    let exporter = LogExporter::builder().with_tonic().build()?;

    Ok(SdkLoggerProvider::builder()
        .with_batch_exporter(exporter)
        .build())
}

pub fn init_tracing() -> (
    SdkTracerProvider,
    SdkMeterProvider,
    SdkLoggerProvider,
    sentry::ClientInitGuard,
) {
    let service_name = std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "backend".to_string());
    let environment =
        std::env::var("SENTRY_ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
    let release = std::env::var("SENTRY_RELEASE").unwrap_or_else(|_| format!("{service_name}@dev"));

    // Initialize Sentry
    let sentry_dsn = std::env::var("SENTRY_DSN").unwrap_or_else(|_| {
        tracing::warn!("SENTRY_DSN environment variable not set - Sentry integration disabled");
        String::new()
    });

    let sentry_guard = if sentry_dsn.is_empty() {
        sentry::init(sentry::ClientOptions::default())
    } else {
        sentry::init((
            sentry_dsn,
            sentry::ClientOptions {
                release: Some(release.into()),
                environment: Some(environment.into()),
                traces_sample_rate: 0.1,
                debug: false,
                enable_logs: true,
                before_send: Some(Arc::new(move |mut event| {
                    // Add service context
                    event
                        .tags
                        .insert("service".to_string(), service_name.clone());

                    // Remove sensitive server information
                    event.server_name = None;

                    Some(event)
                })),
                send_default_pii: false,
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
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to create OTLP span exporter");

    let service_name_for_trace =
        std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "backend".to_string());

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    service_name_for_trace.clone(),
                )])
                .build(),
        )
        .build();

    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    // Filter the tracing layer
    let tracing_level_filter = tracing_subscriber::filter::Targets::new()
        .with_target("backend", tracing::Level::TRACE)
        .with_target("bookapp", tracing::Level::TRACE)
        .with_target("sqlx", tracing::Level::DEBUG)
        .with_target("rdkafka", tracing::Level::INFO)
        .with_target("otel::tracing", tracing::Level::INFO)
        .with_default(tracing::Level::INFO);

    // Turn our OTLP pipeline into a tracing layer
    let tracing_opentelemetry_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer_provider.tracer(service_name_for_trace))
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
    let log_provider = init_logger_provider().unwrap();
    let otel_log_filter =
        tracing_subscriber::EnvFilter::new("info,backend=debug,bookapp=debug,sqlx=info")
            .add_directive("hyper=error".parse().unwrap())
            .add_directive("tonic=error".parse().unwrap())
            .add_directive("reqwest=error".parse().unwrap());

    let otel_log_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&log_provider)
            .with_filter(otel_log_filter);

    // Sentry tracing layer for error capture and performance monitoring
    let sentry_layer =
        sentry::integrations::tracing::layer().event_filter(|md| match *md.level() {
            tracing::Level::ERROR => sentry::integrations::tracing::EventFilter::Event,
            tracing::Level::WARN => sentry::integrations::tracing::EventFilter::Breadcrumb,
            tracing::Level::INFO => sentry::integrations::tracing::EventFilter::Log,
            tracing::Level::DEBUG => sentry::integrations::tracing::EventFilter::Ignore,
            _ => sentry::integrations::tracing::EventFilter::Ignore,
        });

    // Build the subscriber by combining layers
    let subscriber = tracing_subscriber::Registry::default()
        .with(
            console_subscriber::ConsoleLayer::builder()
                .with_default_env()
                .server_addr(([0, 0, 0, 0], 6670)) // Different port from main app
                .spawn(),
        )
        .with(tracing_opentelemetry_layer)
        .with(SentryOtelCorrelationLayer::new())
        .with(sentry_layer)
        .with(otel_log_layer)
        .with(opentelemetry_metrics_layer)
        .with(stdout_layer.with_filter(tracing_subscriber::EnvFilter::from_default_env()));

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");

    (tracer_provider, meter_provider, log_provider, sentry_guard)
}
