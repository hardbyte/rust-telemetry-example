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
//! - **Performance Metrics**: Busy time, poll counts, park counts per worker
//! - **Memory Metrics**: Optional process memory usage (with `memory-metrics` feature)
//! - **OpenTelemetry 0.30+ Compatible**: Works with modern OpenTelemetry ecosystem
//! - **Zero Runtime Overhead**: Uses OpenTelemetry's efficient callback-based collection
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use opentelemetry_sdk::metrics::SdkMeterProvider;
//! use tokio_otel_metrics::TokioRuntimeMetrics;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
//! - `tokio_workers_count` - Number of worker threads
//! - `tokio_alive_tasks` - Number of alive tasks
//! - `tokio_global_queue_depth` - Global task queue depth
//! - `tokio_blocking_threads` - Number of blocking threads
//! - `tokio_spawned_tasks_total` - Total spawned tasks (requires `target_has_atomic="64"`)
//! - `tokio_blocking_queue_depth` - Blocking queue depth (requires `target_has_atomic="64"`)
//!
//! ### Per-Worker Metrics
//! - `tokio_worker_busy_duration_seconds` - Time each worker has been busy
//! - `tokio_worker_park_count` - Number of times each worker has parked
//! - `tokio_worker_poll_count` - Number of tasks polled by each worker
//!
//! ### Memory Metrics (with `memory-metrics` feature)
//! - `process_memory_usage_bytes` - Process memory usage
//!
//! ## Compilation Flags
//!
//! For maximum metrics coverage, compile with:
//! ```bash
//! RUSTFLAGS="--cfg tokio_unstable" cargo build
//! ```
//!
//! This enables additional unstable Tokio metrics.

use opentelemetry::metrics::Meter;

mod runtime;
pub mod task;

pub use runtime::TokioRuntimeMetrics;
pub use task::TaskMetrics;

#[cfg(feature = "memory-metrics")]
mod memory;

#[cfg(feature = "memory-metrics")]
pub use memory::ProcessMemoryMetrics;

/// Result type for metric registration operations.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A simplified observable registration for demonstration.
///
/// In a real implementation, this would integrate with OpenTelemetry's callback system.
/// For now, this serves as a placeholder to demonstrate the API design.
pub struct ObservableRegistration {
    name: String,
    callback: Box<dyn Fn() -> Vec<(String, u64)> + Send + Sync>,
}

impl ObservableRegistration {
    pub fn new<F>(name: &str, callback: F) -> Self
    where
        F: Fn() -> Vec<(String, u64)> + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            callback: Box::new(callback),
        }
    }

    /// Get the name of this registration.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Manually collect metrics (for testing purposes).
    pub fn collect(&self) -> Vec<(String, u64)> {
        (self.callback)()
    }
}

impl std::fmt::Debug for ObservableRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObservableRegistration")
            .field("name", &self.name)
            .field("callback", &"<callback>")
            .finish()
    }
}

/// Convenience function to register all available metrics.
///
/// This function registers both Tokio runtime metrics and (optionally) memory metrics
/// if the `memory-metrics` feature is enabled.
///
/// # Example
///
/// ```rust,no_run
/// use opentelemetry_sdk::metrics::SdkMeterProvider;
/// use tokio_otel_metrics::register_all_metrics;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let meter_provider = SdkMeterProvider::default();
///     let meter = meter_provider.meter("tokio");
///     
///     let _registrations = register_all_metrics(&meter)?;
///     
///     // Your app code...
///     
///     Ok(())
/// }
/// ```
pub fn register_all_metrics(meter: &Meter) -> Result<Vec<Box<dyn std::any::Any>>> {
    let mut registrations: Vec<Box<dyn std::any::Any>> = Vec::new();

    // Register Tokio runtime metrics
    let tokio_regs = TokioRuntimeMetrics::register(meter)?;
    for reg in tokio_regs {
        registrations.push(Box::new(reg));
    }

    // Register memory metrics if feature is enabled
    #[cfg(feature = "memory-metrics")]
    {
        let memory_reg = ProcessMemoryMetrics::register(meter)?;
        registrations.push(Box::new(memory_reg));
    }

    tracing::info!("Registered {} metric callbacks", registrations.len());
    Ok(registrations)
}
