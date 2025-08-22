//! Tokio runtime metrics collection for OpenTelemetry.
//!
//! This module provides utilities to collect and export Tokio runtime metrics
//! through OpenTelemetry, giving visibility into async task performance,
//! scheduler behavior, and runtime health.
//!
//! # Overview
//!
//! The Tokio runtime provides detailed metrics about:
//! - Task scheduling and execution
//! - Thread pool utilization
//! - Blocking operations
//! - Resource usage patterns
//!
//! These metrics are exposed through OpenTelemetry and can be scraped by
//! the OpenTelemetry collector for visualization in Grafana.
//!
//! # Usage
//!
//! ```rust,no_run
//! use observability_utils::TokioRuntimeMetrics;
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Initialize OpenTelemetry first
//!     let config = observability_utils::ObservabilityConfig::new("my-service");
//!     let (tracer, meter, logger, _guard) = observability_utils::init_tracing(config);
//!     
//!     // Start tokio metrics collection
//!     let runtime_metrics = TokioRuntimeMetrics::new("my-service");
//!     let _handle = runtime_metrics.start_collection(Duration::from_secs(5));
//!     
//!     // Your application code here
//! }
//! ```

use opentelemetry::metrics::{Counter, Gauge, Meter, MeterProvider};
use opentelemetry::{global, KeyValue};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};

/// Tokio runtime metrics collector that exports metrics to OpenTelemetry.
///
/// This collector gathers runtime statistics from the Tokio runtime and
/// exports them as OpenTelemetry metrics for monitoring and alerting.
///
/// # Metrics Exported
///
/// - `tokio.workers.count` - Number of worker threads
/// - `tokio.workers.active` - Number of active worker threads
/// - `tokio.tasks.spawned` - Total tasks spawned (counter)
/// - `tokio.tasks.active` - Currently active tasks
/// - `tokio.blocking.threads` - Number of blocking threads
/// - `tokio.budget.forced_yields` - Forced yields due to budget exhaustion
pub struct TokioRuntimeMetrics {
    service_name: String,
    meter: Meter,
    workers_count: Gauge<u64>,
    workers_active: Gauge<u64>,
    tasks_spawned: Counter<u64>,
    tasks_active: Gauge<u64>,
    blocking_threads: Gauge<u64>,
    budget_forced_yields: Counter<u64>,
}

impl TokioRuntimeMetrics {
    /// Creates a new Tokio runtime metrics collector.
    ///
    /// # Arguments
    ///
    /// * `service_name` - Name of the service for metric labels
    pub fn new(service_name: impl Into<String>, meter_provider: &dyn MeterProvider) -> Self {
        let service_name = service_name.into();
        let meter = meter_provider.meter("tokio_runtime_metrics");

        // Create all the metrics instruments
        let workers_count = meter
            .u64_gauge("tokio.workers.count")
            .with_description("Number of worker threads in the Tokio runtime")
            .build();
        
        let workers_active = meter
            .u64_gauge("tokio.workers.active") 
            .with_description("Number of active worker threads")
            .build();
        
        let tasks_spawned = meter
            .u64_counter("tokio.tasks.spawned")
            .with_description("Total number of tasks spawned")
            .build();
        
        let tasks_active = meter
            .u64_gauge("tokio.tasks.active")
            .with_description("Number of currently active tasks")
            .build();
        
        let blocking_threads = meter
            .u64_gauge("tokio.blocking.threads")
            .with_description("Number of blocking threads")
            .build();
        
        let budget_forced_yields = meter
            .u64_counter("tokio.budget.forced_yields")
            .with_description("Number of forced yields due to budget exhaustion")
            .build();

        Self {
            service_name,
            meter,
            workers_count,
            workers_active,
            tasks_spawned,
            tasks_active,
            blocking_threads,
            budget_forced_yields,
        }
    }

    /// Starts collecting Tokio runtime metrics at the specified interval.
    ///
    /// Returns a join handle that can be used to stop the collection.
    ///
    /// # Arguments
    ///
    /// * `interval` - How often to collect and export metrics
    ///
    /// # Returns
    ///
    /// A `JoinHandle` that will run the collection loop. Drop or abort
    /// this handle to stop metric collection.
    pub fn start_collection(self, interval: Duration) -> JoinHandle<()> {
        let metrics_collector = Arc::new(self);
        
        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            interval_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            
            tracing::info!("Starting Tokio runtime metrics collection every {:?}", interval);
            
            loop {
                interval_timer.tick().await;
                
                if let Err(e) = metrics_collector.collect_metrics().await {
                    error!("Failed to collect Tokio runtime metrics: {:?}", e);
                }
            }
        })
    }

    /// Collects and exports current Tokio runtime metrics.
    ///
    /// This method is called automatically by `start_collection` but can also
    /// be called manually for one-time metric collection.
    async fn collect_metrics(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check if we can access runtime metrics
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let runtime_metrics = handle.metrics();
                
                // Create common labels
                let service_label = KeyValue::new("service.name", self.service_name.clone());
                let labels = &[service_label];
                
                // Record the metrics
                self.workers_count.record(runtime_metrics.num_workers() as u64, labels);
                self.workers_active.record(runtime_metrics.num_alive_tasks() as u64, labels);
                self.tasks_spawned.add(runtime_metrics.spawned_tasks_count(), labels);
                self.tasks_active.record(runtime_metrics.num_alive_tasks() as u64, labels);
                self.blocking_threads.record(runtime_metrics.num_blocking_threads() as u64, labels);
                self.budget_forced_yields.add(runtime_metrics.budget_forced_yield_count(), labels);
                
                tracing::info!(
                    service = %self.service_name,
                    workers = runtime_metrics.num_workers(),
                    active_tasks = runtime_metrics.num_alive_tasks(),
                    spawned_tasks = runtime_metrics.spawned_tasks_count(),
                    blocking_threads = runtime_metrics.num_blocking_threads(),
                    "Collected Tokio runtime metrics"
                );
                
                Ok(())
            }
            Err(_) => {
                warn!("No Tokio runtime found - cannot collect runtime metrics");
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_tokio_metrics_creation() {
        let metrics = TokioRuntimeMetrics::new("test-service");
        assert_eq!(metrics.service_name, "test-service");
    }

    #[tokio::test]
    async fn test_metrics_collection() {
        let metrics = TokioRuntimeMetrics::new("test-service");
        
        // This should not panic even without proper OpenTelemetry setup
        let result = metrics.collect_metrics().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_collection_task_lifecycle() {
        let metrics = TokioRuntimeMetrics::new("test-service");
        let handle = metrics.start_collection(Duration::from_millis(10));
        
        // Let it run briefly
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Stop collection
        handle.abort();
        let result = handle.await;
        assert!(result.is_err()); // Expected to be aborted
    }
}