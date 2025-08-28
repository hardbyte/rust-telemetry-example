//! Process memory metrics collection.

#[cfg(feature = "memory-metrics")]
use opentelemetry::metrics::Meter;

/// Process memory metrics collector.
///
/// This collector provides basic process memory usage metrics using platform-specific
/// APIs when the `memory-metrics` feature is enabled.
#[cfg(feature = "memory-metrics")]
#[cfg_attr(docsrs, doc(cfg(feature = "memory-metrics")))]
pub struct ProcessMemoryMetrics;

#[cfg(feature = "memory-metrics")]
#[cfg_attr(docsrs, doc(cfg(feature = "memory-metrics")))]
impl ProcessMemoryMetrics {
    /// Register process memory metrics with the provided meter.
    ///
    /// This creates an observable gauge following OpenTelemetry semantic conventions
    /// that reports the current process memory usage. The implementation uses
    /// platform-specific APIs to gather memory information.
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
    /// use opentelemetry::metrics::MeterProvider;
    /// use opentelemetry_sdk::metrics::SdkMeterProvider;
    /// use tokio_otel_metrics::ProcessMemoryMetrics;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let meter_provider = SdkMeterProvider::default();
    ///     let meter = meter_provider.meter("memory");
    ///     
    ///     let _registrations = ProcessMemoryMetrics::register(&meter)?;
    ///     
    ///     // Memory metrics are now being collected
    ///     // Registrations will be cleaned up when dropped
    ///     
    ///     Ok(())
    /// }
    /// # }
    /// ```
    pub fn register(meter: &Meter) -> crate::Result<crate::MetricRegistrations> {
        // Create instrument following OpenTelemetry semantic conventions
        let memory_usage = meter
            .u64_observable_gauge("process.memory.usage")
            .with_description("Process memory usage")
            .with_unit("By")
            .with_callback(move |observer| {
                if let Some(memory_bytes) = get_memory_usage() {
                    // Use semantic convention attribute: state="rss" for resident set size
                    observer.observe(
                        memory_bytes,
                        &[opentelemetry::KeyValue::new("state", "rss")],
                    );
                }
            })
            .build();

        let metric_registrations =
            crate::MetricRegistrations::new(vec![crate::ObservableHandle::new(memory_usage)]);
        tracing::info!(
            "Registered {} process memory metrics with semantic conventions",
            metric_registrations.len()
        );
        Ok(metric_registrations)
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
use opentelemetry::metrics::Meter;

#[cfg(not(feature = "memory-metrics"))]
/// Disabled memory metrics collector (requires memory-metrics feature).
pub struct ProcessMemoryMetrics;

#[cfg(not(feature = "memory-metrics"))]
impl ProcessMemoryMetrics {
    /// This method is only available when the `memory-metrics` feature is enabled.
    pub fn register(_meter: &Meter) -> crate::Result<crate::MetricRegistrations> {
        Err(crate::Error::FeatureNotEnabled {
            feature: "memory-metrics",
        })
    }
}
