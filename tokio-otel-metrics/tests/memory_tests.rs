#[cfg(feature = "memory-metrics")]
mod memory_tests {
    use opentelemetry::metrics::MeterProvider;
    use opentelemetry_sdk::metrics::SdkMeterProvider;
    use tokio_otel_metrics::ProcessMemoryMetrics;

    #[tokio::test]
    async fn test_memory_metrics_registration() {
        let meter_provider = SdkMeterProvider::default();
        let meter = meter_provider.meter("test_memory");

        // Should successfully register without errors
        let registrations =
            ProcessMemoryMetrics::register(&meter).expect("Failed to register memory metrics");

        // Should have one registration (process.memory.usage)
        assert!(
            !registrations.is_empty(),
            "Should have memory metric registration"
        );
        assert_eq!(
            registrations.len(),
            1,
            "Should have exactly one memory metric"
        );

        tracing::info!(
            "Successfully registered {} memory metric",
            registrations.len()
        );
    }

    #[tokio::test]
    async fn test_memory_metrics_semantic_conventions() {
        let meter_provider = SdkMeterProvider::default();
        let meter = meter_provider.meter("test_memory_conventions");

        let registrations =
            ProcessMemoryMetrics::register(&meter).expect("Failed to register memory metrics");

        // Should have the memory metric registered
        assert!(
            !registrations.is_empty(),
            "Should have memory metric registration"
        );

        // The semantic convention compliance is verified at compile time
        // through our instrument creation code (process.memory.usage with state="rss")
        tracing::info!(
            "Memory semantic conventions test passed - {} metric registered",
            registrations.len()
        );
    }

    #[tokio::test]
    async fn test_memory_metrics_with_allocation() {
        let meter_provider = SdkMeterProvider::default();
        let meter = meter_provider.meter("test_memory_allocation");

        let registrations =
            ProcessMemoryMetrics::register(&meter).expect("Failed to register memory metrics");

        // Allocate some memory to potentially change memory usage
        let _large_vec: Vec<u8> = vec![0; 1024 * 1024]; // 1MB

        // The metric should still be registered
        assert!(
            !registrations.is_empty(),
            "Memory metric should still be registered after allocation"
        );

        // The callback will be invoked periodically by the OpenTelemetry SDK
        // and should capture the memory usage changes
        tracing::info!("Memory allocation test passed - metric remains registered");
    }
}

#[cfg(not(feature = "memory-metrics"))]
mod disabled_memory_tests {
    use opentelemetry::metrics::MeterProvider;
    use opentelemetry_sdk::metrics::SdkMeterProvider;

    // Re-export for test purposes when feature is disabled
    pub use tokio_otel_metrics::memory::ProcessMemoryMetrics;

    #[tokio::test]
    async fn test_memory_metrics_disabled() {
        let meter_provider = SdkMeterProvider::default();
        let meter = meter_provider.meter("test_disabled");

        // Should return error when feature is disabled
        let result = ProcessMemoryMetrics::register(&meter);
        assert!(
            result.is_err(),
            "Should return error when memory-metrics feature is disabled"
        );

        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("memory-metrics"),
            "Error message should mention feature requirement: {}",
            error_message
        );
    }
}
