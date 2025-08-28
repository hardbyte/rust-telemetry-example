//! # tokio-otel-metrics
//!
//! OpenTelemetry metrics collection for Tokio runtime, compatible with OpenTelemetry 0.30+.
//!
//! This crate provides utilities to collect and export comprehensive Tokio runtime metrics
//! through OpenTelemetry, giving visibility into async task performance, scheduler behavior,
//! and runtime health.
//!
//! ## Features
//!
//! - **Runtime Metrics**: Task counts, queue depths, worker thread statistics  
//! - **Performance Metrics**: Busy time, poll counts, park counts per worker (with `worker-metrics` feature)
//! - **Memory Metrics**: Optional process memory usage (with `memory-metrics` feature)
//! - **OpenTelemetry 0.30+ Compatible**: Works with modern OpenTelemetry ecosystem
//! - **Zero Runtime Overhead**: Uses OpenTelemetry's efficient callback-based collection
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use opentelemetry::metrics::MeterProvider;
//! use opentelemetry_sdk::metrics::SdkMeterProvider;
//! use tokio_otel_metrics::TokioRuntimeMetrics;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//!     let meter_provider = SdkMeterProvider::default();
//!     let meter = meter_provider.meter("tokio_runtime");
//!     
//!     // Register metrics - they'll be collected automatically
//!     let _registrations = TokioRuntimeMetrics::register(&meter)?;
//!     
//!     // Your application code here...
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Available Metrics
//!
//! ### Runtime-Level Metrics
//! - `tokio.runtime.workers` (gauge, "1") - Number of worker threads
//! - `tokio.runtime.tasks.active` (gauge, "1") - Number of alive tasks
//! - `tokio.runtime.queue.depth` (gauge, "1") - Global task queue depth
//! - `tokio.runtime.threads.blocking` (gauge, "1") - Number of blocking threads
//! - `tokio.runtime.tasks.spawned` (counter, "1") - Total spawned tasks
//!
//! ### Per-Worker Metrics (with `worker-metrics` feature and worker.id attribute)  
//! - `tokio.runtime.worker.busy_time` (counter, "s") - Time each worker has been busy
//! - `tokio.runtime.worker.parks` (counter, "1") - Number of times each worker has parked
//! - `tokio.runtime.worker.polls` (counter, "1") - Number of tasks polled by each worker
//! - `tokio.runtime.worker.steals` (counter, "1") - Number of tasks stolen by each worker
//! - `tokio.runtime.worker.overflows` (counter, "1") - Number of overflow events by each worker
//! - `tokio.runtime.worker.queue.depth` (gauge, "1") - Local queue depth for each worker
//!
//! ### Task-Level Metrics
//! - `tokio.runtime.tasks.completed` (counter, "1") - Number of completed tasks
//! - `tokio.runtime.tasks.slow_start` (counter, "1") - Tasks with >100ms spawn-to-poll delay
//! - `tokio.runtime.poll.duration.total` (counter, "s") - Total time spent in polls
//! - `tokio.runtime.poll.duration` (histogram, "s") - Distribution of individual task poll durations
//!
//! ### Memory Metrics (with `memory-metrics` feature)
//! - `process.memory.usage` (gauge, "By") - Process memory usage with state="rss"
//!
//! ## Compilation Flags
//!
//! For maximum metrics coverage, compile with:
//! ```bash
//! RUSTFLAGS="--cfg tokio_unstable" cargo build
//! ```
//!
//! This enables additional unstable Tokio metrics.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![warn(missing_docs)]

use opentelemetry::metrics::Meter;

mod runtime;
pub mod task;
pub mod task_spawner;

pub use runtime::TokioRuntimeMetrics;
pub use task::{PerTaskTrackingConfig, TaskMetrics, TaskStatsSnapshot};
pub use task_spawner::TaskSpawner;

#[cfg(feature = "tracing-layer")]
#[cfg_attr(docsrs, doc(cfg(feature = "tracing-layer")))]
pub use task::TaskTrackingLayer;

pub mod memory;

#[cfg(feature = "memory-metrics")]
#[cfg_attr(docsrs, doc(cfg(feature = "memory-metrics")))]
pub use memory::ProcessMemoryMetrics;

/// Custom error types for this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Required feature not enabled.
    #[error("The '{feature}' feature is required but not enabled")]
    FeatureNotEnabled {
        /// The name of the required feature.
        feature: &'static str,
    },

    /// Could not find an active Tokio runtime context.
    #[error("Could not find an active Tokio runtime context")]
    NoTokioRuntime,

    /// General error for other cases.
    #[error("General error: {0}")]
    General(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Result type for metric registration operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Convenience function to register all available metrics.
///
/// This function registers both Tokio runtime metrics and (optionally) memory metrics
/// if the `memory-metrics` feature is enabled. Returns CallbackRegistration handles
/// that can be dropped to unregister the metrics.
///
/// # Example
///
/// ```rust,no_run
/// use opentelemetry::metrics::MeterProvider;
/// use opentelemetry_sdk::metrics::SdkMeterProvider;
/// use tokio_otel_metrics::register_all_metrics;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///     let meter_provider = SdkMeterProvider::default();
///     let meter = meter_provider.meter("tokio");
///     
///     let _registrations = register_all_metrics(&meter)?;
///     
///     // Your app code...
///     // Registrations are automatically cleaned up when dropped
///     
///     Ok(())
/// }
/// ```
/// Handle to an observable instrument registration that can be dropped to unregister.
pub struct ObservableHandle {
    _inner: Box<dyn std::any::Any + Send + Sync>,
}

impl std::fmt::Debug for ObservableHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObservableHandle")
            .field("_inner", &"<opaque>")
            .finish()
    }
}

impl ObservableHandle {
    /// Create a new observable handle.
    pub fn new<T: 'static + Send + Sync>(inner: T) -> Self {
        Self {
            _inner: Box::new(inner),
        }
    }
}

/// RAII wrapper for multiple observable metric registrations.
///
/// This type provides automatic cleanup of all registered metrics when dropped,
/// ensuring proper resource management and preventing metric registration leaks.
#[derive(Debug)]
pub struct MetricRegistrations {
    pub(crate) handles: Vec<ObservableHandle>,
}

impl MetricRegistrations {
    /// Create a new metric registrations container.
    pub fn new(handles: Vec<ObservableHandle>) -> Self {
        Self { handles }
    }

    /// Get the number of registered metrics.
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Check if no metrics are registered.
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

/// Convenience function to register all available metrics.
///
/// This function registers both Tokio runtime metrics and (optionally) memory metrics
/// if the `memory-metrics` feature is enabled. Returns a MetricRegistrations handle
/// that can be dropped to unregister all metrics.
///
/// # Example
///
/// ```rust,no_run
/// use opentelemetry::metrics::MeterProvider;
/// use opentelemetry_sdk::metrics::SdkMeterProvider;
/// use tokio_otel_metrics::register_all_metrics;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///     let meter_provider = SdkMeterProvider::default();
///     let meter = meter_provider.meter("tokio");
///     
///     let _registrations = register_all_metrics(&meter)?;
///     
///     // Your app code..
///     // Registrations are automatically cleaned up when dropped
///     
///     Ok(())
/// }
/// ```
pub fn register_all_metrics(meter: &Meter) -> Result<MetricRegistrations> {
    let mut all_handles = Vec::new();

    // Register Tokio runtime metrics
    let tokio_regs = TokioRuntimeMetrics::register(meter)?;
    all_handles.extend(tokio_regs.handles);

    // Register memory metrics if feature is enabled
    #[cfg(feature = "memory-metrics")]
    {
        let memory_regs = ProcessMemoryMetrics::register(meter)?;
        all_handles.extend(memory_regs.handles);
    }

    let metric_registrations = MetricRegistrations::new(all_handles);
    tracing::info!("Registered {} metric callbacks", metric_registrations.len());
    Ok(metric_registrations)
}
