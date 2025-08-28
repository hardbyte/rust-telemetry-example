//! Tokio runtime metrics collection using OpenTelemetry observables.

use opentelemetry::metrics::Meter;

#[cfg(tokio_unstable)]
use tokio::runtime::Handle;

/// Tokio runtime metrics collector using OpenTelemetry callback-based instruments.
///
/// This collector creates observable instruments that are updated automatically via
/// callbacks with runtime metrics from the current Tokio runtime. The metrics collection
/// follows OpenTelemetry semantic conventions and is designed to be efficient and
/// compatible with OpenTelemetry 0.30+.
pub struct TokioRuntimeMetrics {
    #[cfg(tokio_unstable)]
    _runtime_handle: Handle,
    #[cfg(not(tokio_unstable))]
    _phantom: std::marker::PhantomData<()>,
}

impl TokioRuntimeMetrics {
    /// Register Tokio runtime metrics with the provided meter.
    ///
    /// This creates observable instruments following OpenTelemetry semantic conventions
    /// and registers callbacks to collect runtime metrics. The returned MetricRegistrations
    /// handle can be dropped to unregister all metrics when no longer needed.
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
    /// use opentelemetry::metrics::MeterProvider;
    /// use opentelemetry_sdk::metrics::SdkMeterProvider;
    /// use tokio_otel_metrics::TokioRuntimeMetrics;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let meter_provider = SdkMeterProvider::default();
    ///     let meter = meter_provider.meter("tokio_runtime");
    ///     
    ///     let _registrations = TokioRuntimeMetrics::register(&meter)?;
    ///     
    ///     // Metrics are now being collected automatically
    ///     // Registrations will be cleaned up when dropped
    ///     
    ///     Ok(())
    /// }
    /// ```
    pub fn register(meter: &Meter) -> crate::Result<crate::MetricRegistrations> {
        #[cfg(tokio_unstable)]
        {
            Self::register_with_runtime_metrics(meter)
        }
        #[cfg(not(tokio_unstable))]
        {
            Self::register_fallback(meter)
        }
    }

    #[cfg(tokio_unstable)]
    fn register_with_runtime_metrics(meter: &Meter) -> crate::Result<crate::MetricRegistrations> {
        let runtime_handle = Handle::try_current().map_err(|_| crate::Error::NoTokioRuntime)?;

        // Create instruments following OpenTelemetry semantic conventions
        let workers_gauge = {
            let handle = runtime_handle.clone();
            meter
                .u64_observable_gauge("tokio.runtime.workers")
                .with_description("Number of worker threads in the Tokio runtime")
                .with_unit("1")
                .with_callback(move |observer| {
                    let metrics = handle.metrics();
                    observer.observe(metrics.num_workers() as u64, &[]);
                })
                .build()
        };

        let tasks_active = {
            let handle = runtime_handle.clone();
            meter
                .u64_observable_gauge("tokio.runtime.tasks.active")
                .with_description("Number of currently active tasks")
                .with_unit("1")
                .with_callback(move |observer| {
                    let metrics = handle.metrics();
                    observer.observe(metrics.num_alive_tasks() as u64, &[]);
                })
                .build()
        };

        let queue_depth = {
            let handle = runtime_handle.clone();
            meter
                .u64_observable_gauge("tokio.runtime.queue.depth")
                .with_description("Depth of the global task queue")
                .with_unit("1")
                .with_callback(move |observer| {
                    let metrics = handle.metrics();
                    observer.observe(metrics.global_queue_depth() as u64, &[]);
                })
                .build()
        };

        let blocking_threads = {
            let handle = runtime_handle.clone();
            meter
                .u64_observable_gauge("tokio.runtime.threads.blocking")
                .with_description("Number of blocking threads")
                .with_unit("1")
                .with_callback(move |observer| {
                    let metrics = handle.metrics();
                    observer.observe(metrics.num_blocking_threads() as u64, &[]);
                })
                .build()
        };

        let mut registrations = vec![
            crate::ObservableHandle::new(workers_gauge),
            crate::ObservableHandle::new(tasks_active),
            crate::ObservableHandle::new(queue_depth),
            crate::ObservableHandle::new(blocking_threads),
        ];

        // Worker-specific metrics (gated behind worker-metrics feature)
        #[cfg(feature = "worker-metrics")]
        {
            let worker_busy_time = {
                let handle = runtime_handle.clone();
                meter
                    .f64_observable_counter("tokio.runtime.worker.busy_time")
                    .with_description("Total busy time for each worker")
                    .with_unit("s")
                    .with_callback(move |observer| {
                        let metrics = handle.metrics();
                        for worker_id in 0..metrics.num_workers() {
                            let worker_attrs =
                                &[opentelemetry::KeyValue::new("worker.id", worker_id as i64)];
                            observer.observe(
                                metrics.worker_total_busy_duration(worker_id).as_secs_f64(),
                                worker_attrs,
                            );
                        }
                    })
                    .build()
            };

            let worker_parks = {
                let handle = runtime_handle.clone();
                meter
                    .u64_observable_counter("tokio.runtime.worker.parks")
                    .with_description("Number of times each worker has parked")
                    .with_unit("1")
                    .with_callback(move |observer| {
                        let metrics = handle.metrics();
                        for worker_id in 0..metrics.num_workers() {
                            let worker_attrs =
                                &[opentelemetry::KeyValue::new("worker.id", worker_id as i64)];
                            observer.observe(metrics.worker_park_count(worker_id), worker_attrs);
                        }
                    })
                    .build()
            };

            let worker_polls = {
                let handle = runtime_handle.clone();
                meter
                    .u64_observable_counter("tokio.runtime.worker.polls")
                    .with_description("Number of tasks polled by each worker")
                    .with_unit("1")
                    .with_callback(move |observer| {
                        let metrics = handle.metrics();
                        for worker_id in 0..metrics.num_workers() {
                            let worker_attrs =
                                &[opentelemetry::KeyValue::new("worker.id", worker_id as i64)];
                            observer.observe(metrics.worker_poll_count(worker_id), worker_attrs);
                        }
                    })
                    .build()
            };

            let worker_steals = {
                let handle = runtime_handle.clone();
                meter
                    .u64_observable_counter("tokio.runtime.worker.steals")
                    .with_description("Number of tasks stolen by each worker")
                    .with_unit("1")
                    .with_callback(move |observer| {
                        let metrics = handle.metrics();
                        for worker_id in 0..metrics.num_workers() {
                            let worker_attrs =
                                &[opentelemetry::KeyValue::new("worker.id", worker_id as i64)];
                            observer.observe(metrics.worker_steal_count(worker_id), worker_attrs);
                        }
                    })
                    .build()
            };

            let worker_overflows = {
                let handle = runtime_handle.clone();
                meter
                    .u64_observable_counter("tokio.runtime.worker.overflows")
                    .with_description("Number of overflow events by each worker")
                    .with_unit("1")
                    .with_callback(move |observer| {
                        let metrics = handle.metrics();
                        for worker_id in 0..metrics.num_workers() {
                            let worker_attrs =
                                &[opentelemetry::KeyValue::new("worker.id", worker_id as i64)];
                            observer
                                .observe(metrics.worker_overflow_count(worker_id), worker_attrs);
                        }
                    })
                    .build()
            };

            let worker_queue_depth = {
                let handle = runtime_handle.clone();
                meter
                    .u64_observable_gauge("tokio.runtime.worker.queue.depth")
                    .with_description("Local queue depth for each worker")
                    .with_unit("1")
                    .with_callback(move |observer| {
                        let metrics = handle.metrics();
                        for worker_id in 0..metrics.num_workers() {
                            let worker_attrs =
                                &[opentelemetry::KeyValue::new("worker.id", worker_id as i64)];
                            observer.observe(
                                metrics.worker_local_queue_depth(worker_id) as u64,
                                worker_attrs,
                            );
                        }
                    })
                    .build()
            };

            registrations.extend([
                crate::ObservableHandle::new(worker_busy_time),
                crate::ObservableHandle::new(worker_parks),
                crate::ObservableHandle::new(worker_polls),
                crate::ObservableHandle::new(worker_steals),
                crate::ObservableHandle::new(worker_overflows),
                crate::ObservableHandle::new(worker_queue_depth),
            ]);
        }

        // Additional metrics requiring target_has_atomic = "64"
        #[cfg(target_has_atomic = "64")]
        {
            let tasks_spawned = {
                let handle = runtime_handle.clone();
                meter
                    .u64_observable_counter("tokio.runtime.tasks.spawned")
                    .with_description("Total number of tasks spawned since runtime creation")
                    .with_unit("1")
                    .with_callback(move |observer| {
                        let metrics = handle.metrics();
                        observer.observe(metrics.spawned_tasks_count(), &[]);
                    })
                    .build()
            };

            let blocking_queue_depth = {
                let handle = runtime_handle.clone();
                meter
                    .u64_observable_gauge("tokio.runtime.blocking.queue.depth")
                    .with_description("Depth of the blocking task queue")
                    .with_unit("1")
                    .with_callback(move |observer| {
                        let metrics = handle.metrics();
                        observer.observe(metrics.blocking_queue_depth() as u64, &[]);
                    })
                    .build()
            };

            registrations.push(crate::ObservableHandle::new(tasks_spawned));
            registrations.push(crate::ObservableHandle::new(blocking_queue_depth));
        }

        let metric_registrations = crate::MetricRegistrations::new(registrations);

        let worker_metrics_status = if cfg!(feature = "worker-metrics") {
            "enabled"
        } else {
            "disabled"
        };

        #[cfg(target_has_atomic = "64")]
        tracing::info!(
            "Registered {} Tokio runtime metrics (worker-metrics: {})",
            metric_registrations.len(),
            worker_metrics_status
        );
        #[cfg(not(target_has_atomic = "64"))]
        tracing::info!(
            "Registered {} Tokio runtime metrics (counters only, worker-metrics: {})",
            metric_registrations.len(),
            worker_metrics_status
        );

        Ok(metric_registrations)
    }

    #[cfg(not(tokio_unstable))]
    fn register_fallback(meter: &Meter) -> crate::Result<crate::MetricRegistrations> {
        // When tokio_unstable is not available, provide a minimal set of static metrics
        // This ensures the crate still works but with limited functionality

        // Basic worker count (static approximation based on available parallelism)
        let workers_gauge = {
            meter
                .u64_observable_gauge("tokio.runtime.workers")
                .with_description("Estimated number of worker threads (requires tokio_unstable for accurate data)")
                .with_unit("1")
                .with_callback(move |observer| {
                    // Fallback: use available parallelism as estimate
                    let estimated_workers = std::thread::available_parallelism()
                        .map(|n| n.get() as u64)
                        .unwrap_or(1);
                    observer.observe(estimated_workers, &[]);
                })
                .build()
        };

        // Limited fallback metric for active tasks (tokio_unstable required for accurate data)
        let tasks_active = {
            meter
                .i64_observable_up_down_counter("tokio.runtime.tasks.active")
                .with_description(
                    "Number of currently active tasks (requires tokio_unstable for data)",
                )
                .with_unit("1")
                .with_callback(move |observer| {
                    // Without tokio_unstable, accurate task counting is unavailable
                    observer.observe(0, &[]);
                })
                .build()
        };

        let registrations = vec![
            crate::ObservableHandle::new(workers_gauge),
            crate::ObservableHandle::new(tasks_active),
        ];

        let metric_registrations = crate::MetricRegistrations::new(registrations);

        tracing::warn!(
            "Registered {} fallback Tokio metrics (tokio_unstable not enabled - limited functionality)",
            metric_registrations.len()
        );
        tracing::info!("For full metrics, compile with RUSTFLAGS=\"--cfg tokio_unstable\"");

        Ok(metric_registrations)
    }
}
