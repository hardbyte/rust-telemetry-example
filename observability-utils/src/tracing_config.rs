use crate::sentry_correlation::SentryOtelCorrelationLayer;
use opentelemetry::metrics::MeterProvider;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{LogExporter, WithExportConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

/// Configuration for observability initialization
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    pub service_name: String,
    pub enable_tokio_metrics: bool,
    pub tokio_metrics_interval: std::time::Duration,
    pub console_port: Option<u16>,
}

impl ObservabilityConfig {
    /// Create a new configuration with the specified service name
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            enable_tokio_metrics: true,
            tokio_metrics_interval: std::time::Duration::from_secs(5),
            console_port: None,
        }
    }

    /// Enable or disable tokio runtime metrics collection
    pub fn with_tokio_metrics(mut self, enabled: bool) -> Self {
        self.enable_tokio_metrics = enabled;
        self
    }

    /// Set the interval for tokio metrics collection
    pub fn with_tokio_metrics_interval(mut self, interval: std::time::Duration) -> Self {
        self.tokio_metrics_interval = interval;
        self
    }

    /// Set the console port for tokio-console
    pub fn with_console_port(mut self, port: u16) -> Self {
        self.console_port = Some(port);
        self
    }
}

fn init_meter_provider(
    service_name: &str,
) -> Result<SdkMeterProvider, opentelemetry_otlp::ExporterBuildError> {
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_timeout(std::time::Duration::from_secs(5))
        .build()?;

    // Configure metrics collection with reasonable intervals
    let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter)
        .with_interval(std::time::Duration::from_secs(10)) // Export every 10 seconds
        .build();

    let provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    service_name.to_string(),
                )])
                .build(),
        )
        .build();

    let cloned_provider = provider.clone();
    opentelemetry::global::set_meter_provider(cloned_provider);
    Ok(provider)
}

fn init_logger_provider() -> Result<SdkLoggerProvider, opentelemetry_otlp::ExporterBuildError> {
    let exporter = LogExporter::builder()
        .with_tonic()
        .with_timeout(std::time::Duration::from_secs(5))
        .build()?;

    // Configure log processor with safe batch limits
    let batch_config = opentelemetry_sdk::logs::BatchConfigBuilder::default()
        .with_max_queue_size(512) // Reduced queue size
        .with_scheduled_delay(std::time::Duration::from_millis(500)) // Faster export
        .with_max_export_batch_size(256) // Smaller batches
        .build();

    let batch_processor = opentelemetry_sdk::logs::BatchLogProcessor::builder(exporter)
        .with_batch_config(batch_config)
        .build();

    Ok(SdkLoggerProvider::builder()
        .with_log_processor(batch_processor)
        .build())
}

/// Initialize tracing, metrics, logging, and Sentry with the given configuration
pub fn init_tracing(
    config: ObservabilityConfig,
) -> (
    SdkTracerProvider,
    SdkMeterProvider,
    SdkLoggerProvider,
    sentry::ClientInitGuard,
) {
    let environment =
        std::env::var("SENTRY_ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
    let release =
        std::env::var("SENTRY_RELEASE").unwrap_or_else(|_| format!("{}@dev", config.service_name));

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
                before_send: Some(Arc::new({
                    let service_name = config.service_name.clone();
                    move |mut event| {
                        // Filter out health check and metrics endpoints for bookapp
                        if service_name == "bookapp" {
                            if let Some(request) = &event.request {
                                if let Some(url) = &request.url {
                                    let url_str = url.as_str();
                                    if url_str.contains("/health") || url_str.contains("/metrics") {
                                        return None;
                                    }
                                }
                            }
                        }

                        // Add service context
                        event
                            .tags
                            .insert("service".to_string(), service_name.clone());

                        // Remove sensitive server information
                        event.server_name = None;

                        Some(event)
                    }
                })),
                send_default_pii: false,
                ..Default::default()
            },
        ))
    };

    // Set up OpenTelemetry propagation
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    // Metrics
    let meter_provider = init_meter_provider(&config.service_name).unwrap();
    let opentelemetry_metrics_layer =
        tracing_opentelemetry::MetricsLayer::new(meter_provider.clone());

    // Tracing
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("Failed to create OTLP span exporter");

    // Configure BatchSpanProcessor with safe limits to prevent stack overflow
    let batch_config = opentelemetry_sdk::trace::BatchConfigBuilder::default()
        .with_max_queue_size(512) // Reduced from default 2048 to prevent memory overflow
        .with_scheduled_delay(std::time::Duration::from_millis(500)) // Faster export to reduce queue buildup
        .with_max_export_batch_size(256) // Smaller batches for more predictable memory usage
        .build();

    let batch_processor = opentelemetry_sdk::trace::BatchSpanProcessor::builder(exporter)
        .with_batch_config(batch_config)
        .build();

    let tracer_provider = SdkTracerProvider::builder()
        .with_span_processor(batch_processor)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    config.service_name.clone(),
                )])
                .build(),
        )
        .build();

    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    // Filter the tracing layer
    let tracing_level_filter = tracing_subscriber::filter::Targets::new()
        .with_target("bookapp", tracing::Level::TRACE)
        .with_target("backend", tracing::Level::TRACE)
        .with_target("observability_utils", tracing::Level::TRACE)
        .with_target("sqlx", tracing::Level::DEBUG)
        .with_target("rdkafka", tracing::Level::INFO)
        .with_target("tower_http", tracing::Level::INFO)
        .with_target("hyper_util", tracing::Level::INFO)
        .with_target("h2", tracing::Level::WARN)
        .with_target("otel::tracing", tracing::Level::INFO)
        .with_default(tracing::Level::INFO);

    // Turn our OTLP pipeline into a tracing layer
    let tracing_opentelemetry_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer_provider.tracer(config.service_name.clone()))
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
    let otel_log_filter = tracing_subscriber::EnvFilter::new(
        "info,backend=debug,bookapp=debug,observability_utils=debug,sqlx=info",
    )
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
        .with(tracing_opentelemetry_layer)
        .with(SentryOtelCorrelationLayer::new())
        .with(sentry_layer)
        .with(otel_log_layer)
        .with(opentelemetry_metrics_layer)
        .with(stdout_layer.with_filter(tracing_subscriber::EnvFilter::from_default_env()));

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");

    (tracer_provider, meter_provider, log_provider, sentry_guard)
}

/// Initialize tokio runtime metrics collection using our enhanced implementation
pub fn init_tokio_runtime_metrics(
    meter_provider: &SdkMeterProvider,
) -> Result<Vec<crate::ObservableRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    use crate::TokioRuntimeMetrics;

    let meter = meter_provider.meter("tokio_runtime");

    // Register tokio runtime metrics using our enhanced implementation
    let registrations = TokioRuntimeMetrics::register(&meter)?;

    tracing::info!(
        "Enhanced Tokio runtime metrics registered successfully ({} callbacks)",
        registrations.len()
    );
    Ok(registrations)
}

/// Start Tokio runtime metrics collection
pub fn start_tokio_metrics(
    config: &ObservabilityConfig,
    meter_provider: &SdkMeterProvider,
) -> Result<Vec<crate::ObservableRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!(
        "start_tokio_metrics called with enable_tokio_metrics={}",
        config.enable_tokio_metrics
    );

    if !config.enable_tokio_metrics {
        tracing::info!("Tokio metrics collection disabled by configuration");
        return Ok(Vec::new());
    }

    // Create a simple test counter to verify OpenTelemetry metrics are working
    tracing::info!("Creating test counter...");
    let test_meter = meter_provider.meter("test_metrics");
    let test_counter = test_meter.u64_counter("test_counter").build();
    test_counter.add(1, &[]);
    tracing::info!("Created test counter metric with value 1");

    tracing::info!("Calling init_tokio_runtime_metrics...");
    let result = init_tokio_runtime_metrics(meter_provider);
    tracing::info!("init_tokio_runtime_metrics result: {:?}", result.is_ok());
    result
}

/// Start task-level metrics collection
pub fn start_task_metrics(
    _config: &ObservabilityConfig,
    meter_provider: &SdkMeterProvider,
) -> Result<Vec<crate::ObservableRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    use crate::TaskMetrics;

    let meter = meter_provider.meter("tokio_tasks");
    let task_metrics = TaskMetrics::new();
    let registrations = task_metrics.register_metrics(&meter)?;

    tracing::info!(
        "Task-level metrics registered successfully ({} callbacks)",
        registrations.len()
    );
    Ok(registrations)
}
