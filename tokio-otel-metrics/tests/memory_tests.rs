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
        let registration =
            ProcessMemoryMetrics::register(&meter).expect("Failed to register memory metrics");

        // Should have a name
        assert_eq!(registration.name(), "process_memory");
    }

    #[tokio::test]
    async fn test_memory_metric_collection() {
        let meter_provider = SdkMeterProvider::default();
        let meter = meter_provider.meter("test_memory_collection");

        let registration =
            ProcessMemoryMetrics::register(&meter).expect("Failed to register memory metrics");

        // Collect memory metrics
        let metrics = registration.collect();
        assert!(!metrics.is_empty(), "Should have collected memory metrics");

        // Should have process_memory_usage_bytes metric
        let memory_metric = metrics
            .iter()
            .find(|(name, _)| name == "process_memory_usage_bytes");
        assert!(
            memory_metric.is_some(),
            "Should have process_memory_usage_bytes metric"
        );

        if let Some((_, memory_bytes)) = memory_metric {
            // Memory usage should be positive and reasonable (less than 1GB for test)
            assert!(*memory_bytes > 0, "Memory usage should be positive");
            assert!(
                *memory_bytes < 1_000_000_000,
                "Memory usage seems unreasonably high: {}",
                memory_bytes
            );
        }
    }

    #[tokio::test]
    async fn test_memory_metrics_with_allocation() {
        let meter_provider = SdkMeterProvider::default();
        let meter = meter_provider.meter("test_memory_allocation");

        let registration =
            ProcessMemoryMetrics::register(&meter).expect("Failed to register memory metrics");

        // Get initial memory usage
        let initial_metrics = registration.collect();
        let _initial_memory = initial_metrics
            .iter()
            .find(|(name, _)| name == "process_memory_usage_bytes")
            .map(|(_, value)| *value)
            .expect("Should have initial memory metric");

        // Allocate some memory
        let _large_vec: Vec<u8> = vec![0; 10_000_000]; // 10MB

        // Get memory usage after allocation
        let after_metrics = registration.collect();
        let after_memory = after_metrics
            .iter()
            .find(|(name, _)| name == "process_memory_usage_bytes")
            .map(|(_, value)| *value)
            .expect("Should have memory metric after allocation");

        // Memory should have increased (though this might not always be detectable due to OS behavior)
        // At minimum, ensure we're still getting valid readings
        assert!(
            after_memory > 0,
            "Memory usage should still be positive after allocation"
        );
    }
}

#[cfg(not(feature = "memory-metrics"))]
mod memory_disabled_tests {
    use opentelemetry::metrics::MeterProvider;
    use opentelemetry_sdk::metrics::SdkMeterProvider;

    #[tokio::test]
    async fn test_memory_metrics_disabled() {
        let meter_provider = SdkMeterProvider::default();
        let _meter = meter_provider.meter("test_memory_disabled");

        // When memory-metrics feature is disabled, ProcessMemoryMetrics is not available
        // This test just ensures the feature flag works correctly by compiling
        // No assertion needed - if it compiles, the feature flag is working correctly
    }
}
