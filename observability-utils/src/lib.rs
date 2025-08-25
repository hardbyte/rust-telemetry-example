//! Observability utilities for the bookapp microservices ecosystem.
//!
//! This crate provides shared observability components for OpenTelemetry tracing,
//! metrics, logging, and Sentry error tracking integration.

pub mod sentry_correlation;
// pub mod simple_tokio_metrics; // Disabled - replaced by tokio-otel-metrics crate
pub mod tracing_config;

pub use sentry_correlation::SentryOtelCorrelationLayer;
// pub use simple_tokio_metrics::{SimpleTokioMetrics, SimpleMemoryMetrics}; // Disabled
pub use tracing_config::{
    init_tokio_runtime_metrics, init_tracing, start_task_metrics, start_tokio_metrics,
    ObservabilityConfig,
};

// Re-export the enhanced tokio-otel-metrics crate
pub use tokio_otel_metrics::{ObservableRegistration, TaskMetrics, TokioRuntimeMetrics};
