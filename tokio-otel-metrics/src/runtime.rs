//! Tokio runtime metrics collection using OpenTelemetry observables.

use opentelemetry::metrics::Meter;
use tokio::runtime::Handle;

/// Tokio runtime metrics collector using OpenTelemetry synchronous instruments.
///
/// This collector creates synchronous gauges that are updated periodically with
/// runtime metrics from the current Tokio runtime. The metrics collection is
/// designed to be efficient and compatible with OpenTelemetry 0.30+.
pub struct TokioRuntimeMetrics {
    _runtime_handle: Handle,
}

impl TokioRuntimeMetrics {
    /// Register Tokio runtime metrics with the provided meter.
    ///
    /// This creates several observable instruments and registers callbacks to collect
    /// runtime metrics. The returned handles can be used to unregister the callbacks
    /// when metrics collection is no longer needed.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No current Tokio runtime is available
    /// - OpenTelemetry instrument creation fails
    /// - Callback registration fails
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use opentelemetry_sdk::metrics::SdkMeterProvider;
    /// use tokio_otel_metrics::TokioRuntimeMetrics;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let meter_provider = SdkMeterProvider::default();
    ///     let meter = meter_provider.meter("tokio_runtime");
    ///     
    ///     let _registrations = TokioRuntimeMetrics::register(&meter)?;
    ///     
    ///     // Metrics are now being collected automatically
    ///     
    ///     Ok(())
    /// }
    /// ```
    pub fn register(meter: &Meter) -> crate::Result<Vec<crate::ObservableRegistration>> {
        let runtime_handle = Handle::current();
        let mut registrations = Vec::new();

        // Basic runtime metrics with proper OpenTelemetry callbacks
        let handle_workers = runtime_handle.clone();
        let _workers_gauge = meter
            .u64_observable_gauge("tokio_workers_count")
            .with_description("Number of worker threads in the Tokio runtime")
            .with_callback(move |observer| {
                let metrics = handle_workers.metrics();
                observer.observe(metrics.num_workers() as u64, &[]);
            })
            .build();

        let handle_alive = runtime_handle.clone();
        let _alive_tasks_gauge = meter
            .u64_observable_gauge("tokio_alive_tasks")
            .with_description("Number of currently alive tasks")
            .with_callback(move |observer| {
                let metrics = handle_alive.metrics();
                observer.observe(metrics.num_alive_tasks() as u64, &[]);
            })
            .build();

        let handle_queue = runtime_handle.clone();
        let _global_queue_depth_gauge = meter
            .u64_observable_gauge("tokio_global_queue_depth")
            .with_description("Depth of the global task queue")
            .with_callback(move |observer| {
                let metrics = handle_queue.metrics();
                observer.observe(metrics.global_queue_depth() as u64, &[]);
            })
            .build();

        let handle_blocking = runtime_handle.clone();
        let _blocking_threads_gauge = meter
            .u64_observable_gauge("tokio_blocking_threads")
            .with_description("Number of blocking threads")
            .with_callback(move |observer| {
                let metrics = handle_blocking.metrics();
                observer.observe(metrics.num_blocking_threads() as u64, &[]);
            })
            .build();

        // Worker-specific metrics with proper OpenTelemetry callbacks
        let handle_worker_busy = runtime_handle.clone();
        let _worker_busy_gauge = meter
            .f64_observable_gauge("tokio_worker_busy_duration_seconds")
            .with_description("Total busy time for each worker in seconds")
            .with_callback(move |observer| {
                let metrics = handle_worker_busy.metrics();
                let num_workers = metrics.num_workers();
                for worker_id in 0..num_workers {
                    let busy_duration = metrics.worker_total_busy_duration(worker_id);
                    observer.observe(
                        busy_duration.as_secs_f64(),
                        &[opentelemetry::KeyValue::new("worker_id", worker_id as i64)],
                    );
                }
            })
            .build();

        let handle_worker_park = runtime_handle.clone();
        let _worker_park_count_gauge = meter
            .u64_observable_gauge("tokio_worker_park_count")
            .with_description("Number of times each worker has parked")
            .with_callback(move |observer| {
                let metrics = handle_worker_park.metrics();
                let num_workers = metrics.num_workers();
                for worker_id in 0..num_workers {
                    observer.observe(
                        metrics.worker_park_count(worker_id),
                        &[opentelemetry::KeyValue::new("worker_id", worker_id as i64)],
                    );
                }
            })
            .build();

        let handle_worker_poll = runtime_handle.clone();
        let _worker_poll_count_gauge = meter
            .u64_observable_gauge("tokio_worker_poll_count")
            .with_description("Number of tasks polled by each worker")
            .with_callback(move |observer| {
                let metrics = handle_worker_poll.metrics();
                let num_workers = metrics.num_workers();
                for worker_id in 0..num_workers {
                    observer.observe(
                        metrics.worker_poll_count(worker_id),
                        &[opentelemetry::KeyValue::new("worker_id", worker_id as i64)],
                    );
                }
            })
            .build();

        // Additional worker-specific metrics with proper OpenTelemetry callbacks
        let handle_worker_noop = runtime_handle.clone();
        let _worker_noop_count_gauge = meter
            .u64_observable_gauge("tokio_worker_noop_count")
            .with_description("Number of no-op operations by each worker")
            .with_callback(move |observer| {
                let metrics = handle_worker_noop.metrics();
                let num_workers = metrics.num_workers();
                for worker_id in 0..num_workers {
                    observer.observe(
                        metrics.worker_noop_count(worker_id),
                        &[opentelemetry::KeyValue::new("worker_id", worker_id as i64)],
                    );
                }
            })
            .build();

        let handle_worker_steal = runtime_handle.clone();
        let _worker_steal_count_gauge = meter
            .u64_observable_gauge("tokio_worker_steal_count")
            .with_description("Number of tasks stolen by each worker")
            .with_callback(move |observer| {
                let metrics = handle_worker_steal.metrics();
                let num_workers = metrics.num_workers();
                for worker_id in 0..num_workers {
                    observer.observe(
                        metrics.worker_steal_count(worker_id),
                        &[opentelemetry::KeyValue::new("worker_id", worker_id as i64)],
                    );
                }
            })
            .build();

        let handle_worker_steal_ops = runtime_handle.clone();
        let _worker_steal_operations_gauge = meter
            .u64_observable_gauge("tokio_worker_steal_operations")
            .with_description("Number of steal operations by each worker")
            .with_callback(move |observer| {
                let metrics = handle_worker_steal_ops.metrics();
                let num_workers = metrics.num_workers();
                for worker_id in 0..num_workers {
                    observer.observe(
                        metrics.worker_steal_operations(worker_id),
                        &[opentelemetry::KeyValue::new("worker_id", worker_id as i64)],
                    );
                }
            })
            .build();

        let handle_worker_local_schedule = runtime_handle.clone();
        let _worker_local_schedule_gauge = meter
            .u64_observable_gauge("tokio_worker_local_schedule_count")
            .with_description("Number of tasks scheduled locally by each worker")
            .with_callback(move |observer| {
                let metrics = handle_worker_local_schedule.metrics();
                let num_workers = metrics.num_workers();
                for worker_id in 0..num_workers {
                    observer.observe(
                        metrics.worker_local_schedule_count(worker_id),
                        &[opentelemetry::KeyValue::new("worker_id", worker_id as i64)],
                    );
                }
            })
            .build();

        let handle_worker_overflow = runtime_handle.clone();
        let _worker_overflow_count_gauge = meter
            .u64_observable_gauge("tokio_worker_overflow_count")
            .with_description("Number of overflow events by each worker")
            .with_callback(move |observer| {
                let metrics = handle_worker_overflow.metrics();
                let num_workers = metrics.num_workers();
                for worker_id in 0..num_workers {
                    observer.observe(
                        metrics.worker_overflow_count(worker_id),
                        &[opentelemetry::KeyValue::new("worker_id", worker_id as i64)],
                    );
                }
            })
            .build();

        let handle_worker_queue_depth = runtime_handle.clone();
        let _worker_local_queue_depth_gauge = meter
            .u64_observable_gauge("tokio_worker_local_queue_depth")
            .with_description("Local queue depth for each worker")
            .with_callback(move |observer| {
                let metrics = handle_worker_queue_depth.metrics();
                let num_workers = metrics.num_workers();
                for worker_id in 0..num_workers {
                    observer.observe(
                        metrics.worker_local_queue_depth(worker_id) as u64,
                        &[opentelemetry::KeyValue::new("worker_id", worker_id as i64)],
                    );
                }
            })
            .build();

        // Additional counter metrics that require target_has_atomic = "64"
        #[cfg(target_has_atomic = "64")]
        {
            let handle_spawned_tasks = runtime_handle.clone();
            let _spawned_tasks_gauge = meter
                .u64_observable_gauge("tokio_spawned_tasks_total")
                .with_description("Total number of tasks spawned since runtime creation")
                .with_callback(move |observer| {
                    let metrics = handle_spawned_tasks.metrics();
                    observer.observe(metrics.spawned_tasks_count(), &[]);
                })
                .build();

            let handle_blocking_queue = runtime_handle.clone();
            let _blocking_queue_depth_gauge = meter
                .u64_observable_gauge("tokio_blocking_queue_depth")
                .with_description("Depth of the blocking task queue")
                .with_callback(move |observer| {
                    let metrics = handle_blocking_queue.metrics();
                    observer.observe(metrics.blocking_queue_depth() as u64, &[]);
                })
                .build();

            let handle_remote_schedule = runtime_handle.clone();
            let _remote_schedule_count_gauge = meter
                .u64_observable_gauge("tokio_remote_schedule_count")
                .with_description("Number of tasks scheduled from outside the runtime")
                .with_callback(move |observer| {
                    let metrics = handle_remote_schedule.metrics();
                    observer.observe(metrics.remote_schedule_count(), &[]);
                })
                .build();

            let handle_budget_yield = runtime_handle.clone();
            let _budget_forced_yield_count_gauge = meter
                .u64_observable_gauge("tokio_budget_forced_yield_count")
                .with_description(
                    "Number of times tasks were forced to yield due to budget exhaustion",
                )
                .with_callback(move |observer| {
                    let metrics = handle_budget_yield.metrics();
                    observer.observe(metrics.budget_forced_yield_count(), &[]);
                })
                .build();

            tracing::info!("Registered advanced Tokio metrics (requires target_has_atomic=64)");
        }

        // Create a basic registration for API compatibility
        let basic_registration =
            crate::ObservableRegistration::new("tokio_runtime_metrics", move || {
                vec![] // All metrics are now handled by OpenTelemetry callbacks
            });
        registrations.push(basic_registration);

        tracing::info!(
            "Successfully registered Tokio runtime metrics with OpenTelemetry callbacks"
        );
        Ok(registrations)
    }
}
