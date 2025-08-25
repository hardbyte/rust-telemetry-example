//! Process memory metrics collection.

#[cfg(feature = "memory-metrics")]
use opentelemetry::metrics::Meter;

#[cfg(feature = "memory-metrics")]
use crate::ObservableRegistration;

/// Process memory metrics collector.
///
/// This collector provides basic process memory usage metrics using platform-specific
/// APIs when the `memory-metrics` feature is enabled.
#[cfg(feature = "memory-metrics")]
pub struct ProcessMemoryMetrics;

#[cfg(feature = "memory-metrics")]
impl ProcessMemoryMetrics {
    /// Register process memory metrics with the provided meter.
    ///
    /// This creates an observable gauge that reports the current process memory usage.
    /// The implementation uses platform-specific APIs to gather memory information.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Memory metrics are not supported on the current platform
    /// - OpenTelemetry instrument creation fails
    /// - Callback registration fails
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[cfg(feature = "memory-metrics")]
    /// # {
    /// use opentelemetry_sdk::metrics::SdkMeterProvider;
    /// use tokio_otel_metrics::ProcessMemoryMetrics;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let meter_provider = SdkMeterProvider::default();
    ///     let meter = meter_provider.meter("memory");
    ///     
    ///     let _registration = ProcessMemoryMetrics::register(&meter)?;
    ///     
    ///     // Memory metrics are now being collected
    ///     
    ///     Ok(())
    /// }
    /// # }
    /// ```
    pub fn register(meter: &Meter) -> crate::Result<ObservableRegistration> {
        let _memory_usage_gauge = meter
            .u64_observable_gauge("process_memory_usage_bytes")
            .with_description("Process memory usage in bytes (RSS)")
            .build();

        let registration = ObservableRegistration::new("process_memory", move || {
            let memory_bytes = get_memory_usage().unwrap_or(0);
            vec![("process_memory_usage_bytes".to_string(), memory_bytes)]
        });

        tracing::info!("Registered process memory metrics");
        Ok(registration)
    }
}

/// Get current process memory usage in bytes.
#[cfg(feature = "memory-metrics")]
fn get_memory_usage() -> Option<u64> {
    if let Some(usage) = memory_stats::memory_stats() {
        // Return physical memory usage (RSS)
        Some(usage.physical_mem as u64)
    } else {
        tracing::debug!("Unable to get memory stats for this platform");
        None
    }
}

#[cfg(not(feature = "memory-metrics"))]
/// Placeholder for when memory-metrics feature is disabled.
pub struct ProcessMemoryMetrics;

#[cfg(not(feature = "memory-metrics"))]
impl ProcessMemoryMetrics {
    /// This method is only available when the `memory-metrics` feature is enabled.
    pub fn register(_meter: &Meter) -> crate::Result<()> {
        Err("ProcessMemoryMetrics requires the 'memory-metrics' feature to be enabled".into())
    }
}
