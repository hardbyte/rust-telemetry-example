use opentelemetry::metrics::MeterProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use tokio_otel_metrics::TokioRuntimeMetrics;

#[tokio::test]
async fn test_runtime_metrics_registration() {
    let meter_provider = SdkMeterProvider::default();
    let meter = meter_provider.meter("test_runtime");

    // Should successfully register without errors
    let registrations = TokioRuntimeMetrics::register(&meter).expect("Failed to register metrics");

    // Should have at least basic runtime metrics
    assert!(!registrations.is_empty(), "No metrics were registered");

    // Should have at least 2 registrations (basic + worker metrics)
    assert!(
        registrations.len() >= 2,
        "Expected at least 2 metric registrations, got {}",
        registrations.len()
    );
}

#[tokio::test]
async fn test_metric_collection() {
    let meter_provider = SdkMeterProvider::default();
    let meter = meter_provider.meter("test_collection");

    let registrations = TokioRuntimeMetrics::register(&meter).expect("Failed to register metrics");

    // Test that we can collect from each registration
    for registration in &registrations {
        let metrics = registration.collect();
        assert!(
            !metrics.is_empty(),
            "Registration '{}' produced no metrics",
            registration.name()
        );

        // Verify metric values are reasonable
        for (name, value) in &metrics {
            assert!(!name.is_empty(), "Metric name should not be empty");
            // Basic sanity check - values should be reasonable for a test runtime
            assert!(
                *value < 1_000_000,
                "Metric '{}' has unreasonably high value: {}",
                name,
                value
            );
        }
    }
}

#[tokio::test]
async fn test_metrics_with_workload() {
    let meter_provider = SdkMeterProvider::default();
    let meter = meter_provider.meter("test_workload");

    let registrations = TokioRuntimeMetrics::register(&meter).expect("Failed to register metrics");

    // Create some workload to generate metrics
    let tasks: Vec<_> = (0..5)
        .map(|i| {
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(10 + i * 5)).await;
                i * i
            })
        })
        .collect();

    // Wait for tasks to complete
    let _results: Vec<_> = futures::future::join_all(tasks).await;

    // Collect metrics after workload
    let basic_registration = registrations
        .iter()
        .find(|r| r.name() == "basic_runtime")
        .expect("Basic runtime metrics not found");
    let metrics = basic_registration.collect();

    // Should have collected some basic metrics
    let alive_tasks_metric = metrics.iter().find(|(name, _)| name == "tokio_alive_tasks");
    assert!(
        alive_tasks_metric.is_some(),
        "Should have alive_tasks metric"
    );

    let workers_count_metric = metrics
        .iter()
        .find(|(name, _)| name == "tokio_workers_count");
    assert!(
        workers_count_metric.is_some(),
        "Should have workers_count metric"
    );

    if let Some((_, workers)) = workers_count_metric {
        assert!(*workers > 0, "Should have at least one worker thread");
    }
}

#[tokio::test]
async fn test_worker_specific_metrics() {
    let meter_provider = SdkMeterProvider::default();
    let meter = meter_provider.meter("test_workers");

    let registrations = TokioRuntimeMetrics::register(&meter).expect("Failed to register metrics");

    // Find worker metrics registration
    let worker_registration = registrations
        .iter()
        .find(|r| r.name() == "worker_metrics")
        .expect("Worker metrics not found");
    let worker_metrics = worker_registration.collect();

    // Should have worker-specific metrics
    assert!(!worker_metrics.is_empty(), "Should have worker metrics");

    // Check that we have metrics for at least one worker
    let has_worker_metric = worker_metrics
        .iter()
        .any(|(name, _)| name.contains("worker_id="));
    assert!(
        has_worker_metric,
        "Should have at least one worker-specific metric"
    );
}

#[cfg(target_has_atomic = "64")]
#[tokio::test]
async fn test_advanced_metrics() {
    let meter_provider = SdkMeterProvider::default();
    let meter = meter_provider.meter("test_advanced");

    let registrations = TokioRuntimeMetrics::register(&meter).expect("Failed to register metrics");

    // Should have advanced metrics on 64-bit platforms
    let advanced_registration = registrations
        .iter()
        .find(|r| r.name() == "advanced_runtime");
    assert!(
        advanced_registration.is_some(),
        "Should have advanced runtime metrics on 64-bit platforms"
    );

    if let Some(registration) = advanced_registration {
        let metrics = registration.collect();

        // Should have spawned tasks total
        let spawned_tasks_metric = metrics
            .iter()
            .find(|(name, _)| name == "tokio_spawned_tasks_total");
        assert!(
            spawned_tasks_metric.is_some(),
            "Should have spawned_tasks_total metric"
        );
    }
}

// Add futures to dev-dependencies in Cargo.toml for this test
#[tokio::test]
async fn test_concurrent_registration() {
    // Test that multiple concurrent registrations work correctly
    let meter_provider = std::sync::Arc::new(SdkMeterProvider::default());

    let tasks: Vec<_> = (0..3)
        .map(|i| {
            let provider = meter_provider.clone();
            tokio::spawn(async move {
                let meter = match i {
                    0 => provider.meter("test_concurrent_0"),
                    1 => provider.meter("test_concurrent_1"),
                    _ => provider.meter("test_concurrent_2"),
                };
                TokioRuntimeMetrics::register(&meter)
            })
        })
        .collect();

    let results: Vec<_> = futures::future::join_all(tasks).await;

    // All registrations should succeed
    for (i, result) in results.into_iter().enumerate() {
        let registration_result = result.unwrap_or_else(|_| panic!("Task {} panicked", i));
        assert!(
            registration_result.is_ok(),
            "Registration {} failed: {:?}",
            i,
            registration_result
        );
    }
}
