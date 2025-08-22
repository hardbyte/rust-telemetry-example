//! Observability utilities for the bookapp microservices ecosystem.
//!
//! This crate provides shared observability components for OpenTelemetry tracing,
//! metrics, logging, and Sentry error tracking integration.

pub mod sentry_correlation;
pub mod tokio_metrics;
pub mod tokio_task_metrics;
pub mod tracing_config;

pub use sentry_correlation::SentryOtelCorrelationLayer;
pub use tokio_metrics::TokioRuntimeMetrics;
pub use tokio_task_metrics::TokioTaskMetrics;
pub use tracing_config::{init_tracing, start_tokio_metrics, start_task_metrics, ObservabilityConfig};