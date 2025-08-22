use crate::sentry_correlation::SentryOtelCorrelationLayer;
use crate::tokio_metrics::TokioRuntimeMetrics;
use crate::tokio_task_metrics::TokioTaskMetrics;
use std::panic;
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
    pub console_subscriber_port: u16,
    pub enable_console_subscriber: bool,
    pub enable_tokio_metrics: bool,
    pub tokio_metrics_interval: std::time::Duration,
    pub enable_task_metrics: bool,
    pub task_metrics_interval: std::time::Duration,
}

impl ObservabilityConfig {
    /// Create a new configuration with the specified service name
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            console_subscriber_port: 6669,
            enable_console_subscriber: true,
            enable_tokio_metrics: true,
            tokio_metrics_interval: std::time::Duration::from_secs(5),
            enable_task_metrics: true,
            task_metrics_interval: std::time::Duration::from_secs(10),
        }
    }

    /// Set the console subscriber port
    pub fn with_console_port(mut self, port: u16) -> Self {
        self.console_subscriber_port = port;
        self
    }

    /// Disable console subscriber
    pub fn without_console_subscriber(mut self) -> Self {
        self.enable_console_subscriber = false;
        self
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

    /// Enable or disable task-level metrics collection
    pub fn with_task_metrics(mut self, enabled: bool) -> Self {
        self.enable_task_metrics = enabled;
        self
    }

    /// Set the interval for task metrics collection
    pub fn with_task_metrics_interval(mut self, interval: std::time::Duration) -> Self {
        self.task_metrics_interval = interval;
        self
    }
}

fn init_meter_provider(service_name: &str) -> Result<SdkMeterProvider, opentelemetry_otlp::ExporterBuildError> {
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
    let exporter = LogExporter::builder().with_tonic().build()?;

    Ok(SdkLoggerProvider::builder()
        .with_batch_exporter(exporter)
        .build())
}

/// Initialize tracing, metrics, logging, and Sentry with the given configuration
pub fn init_tracing(config: ObservabilityConfig) -> (
    SdkTracerProvider,
    SdkMeterProvider,
    SdkLoggerProvider,
    sentry::ClientInitGuard,
) {
    let environment =
        std::env::var("SENTRY_ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
    let release = std::env::var("SENTRY_RELEASE").unwrap_or_else(|_| format!("{}@dev", config.service_name));

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
        .build()
        .expect("Failed to create OTLP span exporter");

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
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
    let otel_log_filter =
        tracing_subscriber::EnvFilter::new("info,backend=debug,bookapp=debug,observability_utils=debug,sqlx=info")
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
    // Note: We always include console subscriber for simplicity, just with different ports
    let subscriber = tracing_subscriber::Registry::default()
        .with(
            console_subscriber::ConsoleLayer::builder()
                .with_default_env()
                .server_addr(([0, 0, 0, 0], config.console_subscriber_port))
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

/// Start tokio runtime metrics collection
pub fn start_tokio_metrics(config: &ObservabilityConfig, meter_provider: &SdkMeterProvider) -> Option<tokio::task::JoinHandle<()>> {
    if config.enable_tokio_metrics {
        tracing::info!("Starting Tokio runtime metrics collection for service: {}", config.service_name);
        
        // Test if we can access runtime metrics immediately
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let metrics = handle.metrics();
                tracing::info!(
                    "Tokio runtime accessible: workers={}, alive_tasks={}", 
                    metrics.num_workers(), 
                    metrics.num_alive_tasks()
                );
            }
            Err(_) => {
                tracing::warn!("Tokio runtime not accessible during metrics init");
                return None;
            }
        }
        
        let runtime_metrics = TokioRuntimeMetrics::new(config.service_name.clone(), meter_provider);
        Some(runtime_metrics.start_collection(config.tokio_metrics_interval))
    } else {
        tracing::info!("Tokio runtime metrics disabled for service: {}", config.service_name);
        None
    }
}

/// Start tokio task-level metrics collection
pub fn start_task_metrics(config: &ObservabilityConfig, meter_provider: &SdkMeterProvider) -> Option<tokio::task::JoinHandle<()>> {
    // Always log to ensure function is being called
    tracing::info!("Task metrics function called - enabled: {} for service: {}", 
        config.enable_task_metrics, config.service_name);
    
    if config.enable_task_metrics {
        tracing::info!("Starting Tokio task-level metrics collection for service: {} on port {}", 
            config.service_name, config.console_subscriber_port);
        
        let task_metrics = TokioTaskMetrics::new(
            config.service_name.clone(), 
            config.console_subscriber_port,
            meter_provider
        );
        let handle = task_metrics.start_collection(config.task_metrics_interval);
        tracing::info!("Task metrics collection started successfully for {}", config.service_name);
        Some(handle)
    } else {
        tracing::info!("Tokio task-level metrics disabled for service: {}", config.service_name);
        None
    }
}