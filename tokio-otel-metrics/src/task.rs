//! Task-level metrics collection for Tokio applications.
//!
//! This module provides tools for collecting detailed metrics about individual
//! tasks in a Tokio runtime, including execution time, poll counts, and lifecycle events.

use opentelemetry::{metrics::Meter, KeyValue};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::Id;
use tracing::debug;

#[cfg(feature = "tracing-layer")]
use tracing::{span, Subscriber};
#[cfg(feature = "tracing-layer")]
use tracing_core::span::Attributes;

/// Configuration for per-task tracking to control cardinality
#[derive(Debug, Clone)]
pub struct PerTaskTrackingConfig {
    /// Enable individual task tracking with labels
    pub enabled: bool,
    /// Maximum number of individual tasks to track (controls cardinality)
    pub max_tracked_tasks: usize,
    /// Minimum stack size threshold to track individual tasks (bytes)
    pub min_stack_threshold: u64,
    /// Minimum task lifetime threshold to track individual tasks
    pub min_lifetime_threshold: Duration,
}

impl Default for PerTaskTrackingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_tracked_tasks: 256,
            min_stack_threshold: 1024,                       // >1KB
            min_lifetime_threshold: Duration::from_secs(10), // >10s
        }
    }
}

/// Task metrics collector that tracks detailed task-level performance data.
///
/// This collector maintains statistics for individual tasks and provides
/// aggregated metrics suitable for OpenTelemetry observability.
#[derive(Debug)]
pub struct TaskMetrics {
    /// Map of task ID to task metadata
    tasks: Arc<Mutex<HashMap<Id, TaskMetadata>>>,
    /// Global task statistics (atomic for performance)
    stats: Arc<TaskStats>,
    /// Configuration for per-task tracking
    per_task_config: PerTaskTrackingConfig,
    /// Optional histogram for poll duration latency tracking
    poll_duration_histogram: Arc<Mutex<Option<opentelemetry::metrics::Histogram<f64>>>>,
}

/// Metadata tracked for each task
#[derive(Debug, Clone)]
struct TaskMetadata {
    /// Task name/label for identification
    name: String,
    /// Spawn location (file:line format)
    #[allow(dead_code)]
    location: Option<String>,
    /// Stack size in bytes (if available)
    stack_size: Option<u64>,
    /// When the task was first spawned
    created_at: Instant,
    /// When the task was first polled
    first_poll_at: Option<Instant>,
    /// When the task completed
    completed_at: Option<Instant>,
    /// Number of times the task has been polled
    poll_count: u64,
    /// Total time spent polling this task
    total_poll_duration: Duration,
    /// Whether the task is still active
    active: bool,
}

/// Aggregated task statistics for metrics export using atomic operations for performance
#[derive(Debug)]
pub struct TaskStats {
    /// Total number of tasks spawned
    spawned_count: AtomicU64,
    /// Number of currently active tasks
    active_count: AtomicU64,
    /// Number of completed tasks
    completed_count: AtomicU64,
    /// Total polls across all tasks
    total_polls: AtomicU64,
    /// Total time spent in task polls (as nanoseconds for atomic operations)
    total_poll_duration_nanos: AtomicU64,
    /// Number of tasks that experienced first poll delay > 100ms
    slow_start_tasks: AtomicU64,
    /// Number of tasks with large stack usage (>1024 bytes)
    large_stack_tasks: AtomicU64,
    /// Total stack bytes across all active tasks
    total_stack_bytes: AtomicU64,
    /// Highest stack size seen
    max_stack_size: AtomicU64,
}

impl Default for TaskStats {
    fn default() -> Self {
        Self {
            spawned_count: AtomicU64::new(0),
            active_count: AtomicU64::new(0),
            completed_count: AtomicU64::new(0),
            total_polls: AtomicU64::new(0),
            total_poll_duration_nanos: AtomicU64::new(0),
            slow_start_tasks: AtomicU64::new(0),
            large_stack_tasks: AtomicU64::new(0),
            total_stack_bytes: AtomicU64::new(0),
            max_stack_size: AtomicU64::new(0),
        }
    }
}

/// Snapshot of task statistics for API consumers
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatsSnapshot {
    /// Total number of tasks spawned
    pub spawned_count: u64,
    /// Number of currently active tasks
    pub active_count: u64,
    /// Number of completed tasks
    pub completed_count: u64,
    /// Total polls across all tasks
    pub total_polls: u64,
    /// Total time spent in task polls
    pub total_poll_duration: Duration,
    /// Number of tasks that experienced first poll delay > 100ms
    pub slow_start_tasks: u64,
    /// Number of tasks with large stack usage (>1024 bytes)
    pub large_stack_tasks: u64,
    /// Total stack bytes across all active tasks
    pub total_stack_bytes: u64,
    /// Highest stack size seen
    pub max_stack_size: u64,
}

impl TaskMetrics {
    /// Create a new task metrics collector with default configuration
    pub fn new() -> Self {
        Self::with_config(PerTaskTrackingConfig::default())
    }

    /// Create a new task metrics collector with custom configuration
    pub fn with_config(config: PerTaskTrackingConfig) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            stats: Arc::new(TaskStats::default()),
            per_task_config: config,
            poll_duration_histogram: Arc::new(Mutex::new(None)),
        }
    }

    /// Register a new task with the collector
    pub fn task_spawned(&self, task_id: Id, name: impl Into<String>) {
        self.task_spawned_with_details(task_id, name, None, None);
    }

    /// Register a new task with the collector including location and stack size
    pub fn task_spawned_with_details(
        &self,
        task_id: Id,
        name: impl Into<String>,
        location: Option<String>,
        stack_size: Option<u64>,
    ) {
        let name = name.into();

        // Check if this is a large stack task
        if let Some(size) = stack_size {
            if size > 1024 {
                self.stats.large_stack_tasks.fetch_add(1, Ordering::Relaxed);
            }
            self.stats
                .total_stack_bytes
                .fetch_add(size, Ordering::Relaxed);

            // Update max stack size
            let current_max = self.stats.max_stack_size.load(Ordering::Relaxed);
            if size > current_max {
                self.stats.max_stack_size.store(size, Ordering::Relaxed);
            }
        }

        let metadata = TaskMetadata {
            name: name.clone(),
            location: location.clone(),
            stack_size,
            created_at: Instant::now(),
            first_poll_at: None,
            completed_at: None,
            poll_count: 0,
            total_poll_duration: Duration::ZERO,
            active: true,
        };

        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(task_id, metadata);
        drop(tasks); // Release lock early

        self.stats.spawned_count.fetch_add(1, Ordering::Relaxed);
        self.stats.active_count.fetch_add(1, Ordering::Relaxed);

        debug!(
            "Task spawned: {} ({:?}) location={:?} stack_bytes={:?}",
            name, task_id, location, stack_size
        );
    }

    /// Record that a task was polled
    pub fn task_polled(&self, task_id: Id, poll_duration: Duration) {
        let now = Instant::now();

        // Minimize critical section by extracting only what we need
        let (_is_first_poll, task_name, poll_count) = {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(metadata) = tasks.get_mut(&task_id) {
                let is_first = metadata.first_poll_at.is_none();
                if is_first {
                    metadata.first_poll_at = Some(now);

                    // Check for slow start (> 100ms from spawn to first poll)
                    let first_poll_delay = now.duration_since(metadata.created_at);
                    if first_poll_delay > Duration::from_millis(100) {
                        self.stats.slow_start_tasks.fetch_add(1, Ordering::Relaxed);
                    }
                }

                metadata.poll_count += 1;
                metadata.total_poll_duration += poll_duration;

                (is_first, metadata.name.clone(), metadata.poll_count)
            } else {
                return; // Task not found
            }
        }; // Lock is released here

        // Update global stats atomically (outside the lock)
        self.stats.total_polls.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_poll_duration_nanos
            .fetch_add(poll_duration.as_nanos() as u64, Ordering::Relaxed);

        // Record poll duration in histogram if available
        if let Ok(histogram_guard) = self.poll_duration_histogram.try_lock() {
            if let Some(ref histogram) = *histogram_guard {
                histogram.record(poll_duration.as_secs_f64(), &[]);
            }
        }

        debug!(
            "Task polled: {} (polls: {}, duration: {:?})",
            task_name, poll_count, poll_duration
        );
    }

    /// Record that a task completed
    pub fn task_completed(&self, task_id: Id) {
        let now = Instant::now();

        let task_info = {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(metadata) = tasks.get_mut(&task_id) {
                if metadata.active {
                    metadata.completed_at = Some(now);
                    metadata.active = false;

                    Some((
                        metadata.name.clone(),
                        metadata.created_at,
                        metadata.poll_count,
                        metadata.stack_size,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        }; // Lock is released here

        if let Some((task_name, created_at, poll_count, stack_size)) = task_info {
            // Update global stats atomically (outside the lock)
            self.stats.active_count.fetch_sub(1, Ordering::Relaxed);
            self.stats.completed_count.fetch_add(1, Ordering::Relaxed);

            // Subtract stack size from total (task is no longer active)
            if let Some(size) = stack_size {
                self.stats
                    .total_stack_bytes
                    .fetch_sub(size, Ordering::Relaxed);
            }

            debug!(
                "Task completed: {} (lifetime: {:?}, polls: {}, stack_bytes: {:?})",
                task_name,
                now.duration_since(created_at),
                poll_count,
                stack_size
            );
        }
    }

    /// Register OpenTelemetry metrics for task observability following semantic conventions
    pub fn register_metrics(&self, meter: &Meter) -> crate::Result<crate::MetricRegistrations> {
        // Create instruments for task-level metrics (distinct from runtime-level metrics)
        // Note: tokio.runtime.tasks.* metrics are defined in runtime.rs to avoid conflicts
        let tasks_completed = {
            let stats = Arc::clone(&self.stats);
            meter
                .u64_observable_counter("tokio.runtime.tasks.completed")
                .with_description("Total number of manually tracked completed tasks")
                .with_unit("1")
                .with_callback(move |observer| {
                    let count = stats.completed_count.load(Ordering::Relaxed);
                    observer.observe(count, &[]);
                })
                .build()
        };

        let tasks_slow_start = {
            let stats = Arc::clone(&self.stats);
            meter
                .u64_observable_counter("tokio.runtime.tasks.slow_start")
                .with_description("Number of tasks with >100ms delay to first poll")
                .with_unit("1")
                .with_callback(move |observer| {
                    let count = stats.slow_start_tasks.load(Ordering::Relaxed);
                    observer.observe(count, &[]);
                })
                .build()
        };

        let tasks_large_stack = {
            let stats = Arc::clone(&self.stats);
            meter
                .u64_observable_counter("tokio.runtime.tasks.large_stack")
                .with_description("Number of tasks with stack usage >1024 bytes")
                .with_unit("1")
                .with_callback(move |observer| {
                    let count = stats.large_stack_tasks.load(Ordering::Relaxed);
                    observer.observe(count, &[]);
                })
                .build()
        };

        let stack_bytes_total = {
            let stats = Arc::clone(&self.stats);
            meter
                .u64_observable_gauge("tokio.runtime.tasks.stack_bytes.total")
                .with_description("Total stack bytes across all active tasks")
                .with_unit("By")
                .with_callback(move |observer| {
                    let bytes = stats.total_stack_bytes.load(Ordering::Relaxed);
                    observer.observe(bytes, &[]);
                })
                .build()
        };

        let stack_bytes_max = {
            let stats = Arc::clone(&self.stats);
            meter
                .u64_observable_gauge("tokio.runtime.tasks.stack_bytes.max")
                .with_description("Maximum stack size seen across all tasks")
                .with_unit("By")
                .with_callback(move |observer| {
                    let bytes = stats.max_stack_size.load(Ordering::Relaxed);
                    observer.observe(bytes, &[]);
                })
                .build()
        };

        let polls_total = {
            let stats = Arc::clone(&self.stats);
            meter
                .u64_observable_counter("tokio.runtime.polls.total")
                .with_description("Total number of task polls across all tasks")
                .with_unit("1")
                .with_callback(move |observer| {
                    let count = stats.total_polls.load(Ordering::Relaxed);
                    observer.observe(count, &[]);
                })
                .build()
        };

        let poll_duration_total = {
            let stats = Arc::clone(&self.stats);
            meter
                .f64_observable_counter("tokio.runtime.poll.duration.total")
                .with_description("Total time spent in task polls")
                .with_unit("s")
                .with_callback(move |observer| {
                    let nanos = stats.total_poll_duration_nanos.load(Ordering::Relaxed);
                    let duration = Duration::from_nanos(nanos);
                    observer.observe(duration.as_secs_f64(), &[]);
                })
                .build()
        };

        // Create histogram for poll duration latency tracking
        let poll_duration_histogram = meter
            .f64_histogram("tokio.runtime.poll.duration")
            .with_description("Distribution of task poll durations")
            .with_unit("s")
            .build();

        // Store the histogram for use in task_polled
        {
            let mut histogram_guard = self.poll_duration_histogram.lock().unwrap();
            *histogram_guard = Some(poll_duration_histogram);
        }
        // Note: Histograms don't need to be added to registrations since they're not observable instruments

        let mut registrations = vec![
            crate::ObservableHandle::new(tasks_completed),
            crate::ObservableHandle::new(tasks_slow_start),
            crate::ObservableHandle::new(tasks_large_stack),
            crate::ObservableHandle::new(stack_bytes_total),
            crate::ObservableHandle::new(stack_bytes_max),
            crate::ObservableHandle::new(polls_total),
            crate::ObservableHandle::new(poll_duration_total),
        ];

        // Add per-task metrics if enabled (aggregated by task_name for low cardinality)
        if self.per_task_config.enabled {
            let per_task_lifetime_by_name = {
                let tasks = Arc::clone(&self.tasks);
                let config = self.per_task_config.clone();
                meter
                    .f64_observable_gauge("tokio_task_lifetime_by_name_seconds")
                    .with_description(
                        "Average task lifetime grouped by task name (low cardinality)",
                    )
                    .with_unit("s")
                    .with_callback(move |observer| {
                        let task_map = tasks.lock().unwrap();
                        let mut task_name_stats: std::collections::HashMap<String, (f64, u32)> =
                            std::collections::HashMap::new();

                        // Aggregate by task name
                        for (_task_id, metadata) in task_map.iter() {
                            let lifetime = metadata.created_at.elapsed();
                            if lifetime >= config.min_lifetime_threshold {
                                let entry = task_name_stats
                                    .entry(metadata.name.clone())
                                    .or_insert((0.0, 0));
                                entry.0 += lifetime.as_secs_f64();
                                entry.1 += 1;
                            }
                        }

                        // Emit average lifetime per task name (limited cardinality)
                        for (task_name, (total_lifetime, count)) in task_name_stats.iter() {
                            if *count > 0 {
                                let avg_lifetime = total_lifetime / (*count as f64);
                                let labels = [KeyValue::new("task_name", task_name.clone())];
                                observer.observe(avg_lifetime, &labels);
                            }
                        }
                    })
                    .build()
            };

            let per_task_count_by_name = {
                let tasks = Arc::clone(&self.tasks);
                let config = self.per_task_config.clone();
                meter
                    .u64_observable_gauge("tokio_task_count_by_name")
                    .with_description(
                        "Number of active tasks grouped by task name (low cardinality)",
                    )
                    .with_unit("1")
                    .with_callback(move |observer| {
                        let task_map = tasks.lock().unwrap();
                        let mut task_name_counts: std::collections::HashMap<String, u64> =
                            std::collections::HashMap::new();

                        // Count by task name
                        for (_task_id, metadata) in task_map.iter() {
                            let lifetime = metadata.created_at.elapsed();
                            if lifetime >= config.min_lifetime_threshold {
                                let count =
                                    task_name_counts.entry(metadata.name.clone()).or_insert(0);
                                *count += 1;
                            }
                        }

                        // Emit count per task name (limited cardinality)
                        for (task_name, count) in task_name_counts.iter() {
                            let labels = [KeyValue::new("task_name", task_name.clone())];
                            observer.observe(*count, &labels);
                        }
                    })
                    .build()
            };

            registrations.push(crate::ObservableHandle::new(per_task_lifetime_by_name));
            registrations.push(crate::ObservableHandle::new(per_task_count_by_name));
        }

        let metric_registrations = crate::MetricRegistrations::new(registrations);
        if self.per_task_config.enabled {
            tracing::info!(
                "Successfully registered {} task-level metrics with per-task tracking and poll duration histogram (max: {}, stack_threshold: {}B, lifetime_threshold: {}s)",
                metric_registrations.len(),
                self.per_task_config.max_tracked_tasks,
                self.per_task_config.min_stack_threshold,
                self.per_task_config.min_lifetime_threshold.as_secs()
            );
        } else {
            tracing::info!(
                "Successfully registered {} task-level metrics with poll duration histogram (per-task tracking disabled)",
                metric_registrations.len()
            );
        }
        Ok(metric_registrations)
    }

    /// Get current task statistics (for debugging/testing)
    pub fn get_stats(&self) -> TaskStatsSnapshot {
        TaskStatsSnapshot {
            spawned_count: self.stats.spawned_count.load(Ordering::Relaxed),
            active_count: self.stats.active_count.load(Ordering::Relaxed),
            completed_count: self.stats.completed_count.load(Ordering::Relaxed),
            total_polls: self.stats.total_polls.load(Ordering::Relaxed),
            total_poll_duration: Duration::from_nanos(
                self.stats.total_poll_duration_nanos.load(Ordering::Relaxed),
            ),
            slow_start_tasks: self.stats.slow_start_tasks.load(Ordering::Relaxed),
            large_stack_tasks: self.stats.large_stack_tasks.load(Ordering::Relaxed),
            total_stack_bytes: self.stats.total_stack_bytes.load(Ordering::Relaxed),
            max_stack_size: self.stats.max_stack_size.load(Ordering::Relaxed),
        }
    }

    /// Clear all collected metrics (useful for testing)
    pub fn clear(&self) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.clear();

        // Reset all atomic counters
        self.stats.spawned_count.store(0, Ordering::Relaxed);
        self.stats.active_count.store(0, Ordering::Relaxed);
        self.stats.completed_count.store(0, Ordering::Relaxed);
        self.stats.total_polls.store(0, Ordering::Relaxed);
        self.stats
            .total_poll_duration_nanos
            .store(0, Ordering::Relaxed);
        self.stats.slow_start_tasks.store(0, Ordering::Relaxed);
        self.stats.large_stack_tasks.store(0, Ordering::Relaxed);
        self.stats.total_stack_bytes.store(0, Ordering::Relaxed);
        self.stats.max_stack_size.store(0, Ordering::Relaxed);

        // Clear histogram (note: this doesn't reset its internal state, just removes our reference)
        let mut histogram_guard = self.poll_duration_histogram.lock().unwrap();
        *histogram_guard = None;
    }
}

impl Default for TaskMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to create a global task metrics collector
static TASK_METRICS: once_cell::sync::Lazy<TaskMetrics> =
    once_cell::sync::Lazy::new(TaskMetrics::new);

/// Get the global task metrics collector instance
pub fn global_task_metrics() -> &'static TaskMetrics {
    &TASK_METRICS
}

#[cfg(feature = "tracing-layer")]
/// A tracing layer that automatically tracks Tokio task metrics.
///
/// This layer hooks into the tracing instrumentation to automatically
/// detect task spawning, polling, and completion events, providing an
/// alternative to manual TaskSpawner usage.
///
/// ## Limitations
///
/// Due to how Tokio's tracing integration works, this layer has some limitations:
///
/// - **Task completion tracking**: The `on_close` hook may be called from a different
///   execution context where `tokio::task::try_id()` returns `None` or a different
///   task ID. This implementation stores task IDs in span extensions to mitigate this,
///   but some completion events may still be missed in edge cases.
///
/// - **Span-to-task mapping**: There's no guaranteed 1:1 mapping between tracing
///   spans and Tokio tasks, especially for nested or concurrent operations.
///
/// For more reliable task tracking, consider using the manual `TaskMetrics` API
/// directly in critical code paths.
///
/// # Example
/// ```rust,no_run
/// use tracing_subscriber::prelude::*;
/// use tokio_otel_metrics::task::{TaskTrackingLayer, PerTaskTrackingConfig};
///
/// let config = PerTaskTrackingConfig {
///     enabled: true,
///     max_tracked_tasks: 100,
///     ..Default::default()
/// };
///
/// let task_layer = TaskTrackingLayer::with_config(config);
///
/// tracing_subscriber::registry()
///     .with(task_layer)
///     .with(tracing_subscriber::fmt::layer())
///     .init();
///
/// // Now all Tokio tasks will be automatically tracked!
/// tokio::spawn(async {
///     // This task will be automatically monitored
///     println!("Hello from tracked task!");
/// });
/// ```
#[derive(Debug, Clone)]
pub struct TaskTrackingLayer {
    metrics: Arc<TaskMetrics>,
}

#[cfg(feature = "tracing-layer")]
impl TaskTrackingLayer {
    /// Create a new task tracking layer with default configuration
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(TaskMetrics::new()),
        }
    }

    /// Create a new task tracking layer with custom configuration
    pub fn with_config(config: PerTaskTrackingConfig) -> Self {
        Self {
            metrics: Arc::new(TaskMetrics::with_config(config)),
        }
    }

    /// Get access to the underlying metrics collector for registration
    pub fn metrics(&self) -> &TaskMetrics {
        &self.metrics
    }
}

#[cfg(feature = "tracing-layer")]
impl Default for TaskTrackingLayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tracing-layer")]
impl<S> tracing_subscriber::Layer<S> for TaskTrackingLayer
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &Attributes<'_>,
        id: &span::Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        // Look for tokio task spawn spans specifically
        let meta = attrs.metadata();
        if meta.target() == "tokio::task" && meta.name() == "runtime.spawn" {
            // Try to extract the actual task ID from the span or runtime context
            if let Some(task_id) = tokio::task::try_id() {
                let task_name = format!(
                    "{}:{}",
                    meta.file().unwrap_or("unknown"),
                    meta.line().unwrap_or(0)
                );
                let location = meta
                    .file()
                    .map(|f| format!("{}:{}", f, meta.line().unwrap_or(0)));

                self.metrics
                    .task_spawned_with_details(task_id, task_name, location, None);

                // Store the task ID in the span's extensions for reliable retrieval later
                if let Some(span) = ctx.span(id) {
                    span.extensions_mut().insert(StoredTaskId(task_id));
                }
            }
        }
    }

    fn on_enter(&self, id: &span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let meta = span.metadata();
            if meta.target() == "tokio::task" && meta.name() == "runtime.spawn" {
                // Check if we have a task ID for this span.
                if span.extensions().get::<StoredTaskId>().is_some() {
                    // Record when task polling starts.
                    span.extensions_mut().insert(TaskPollStart(Instant::now()));
                }
            }
        }
    }

    fn on_exit(&self, id: &span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let meta = span.metadata();
            if meta.target() == "tokio::task" && meta.name() == "runtime.spawn" {
                // Use the reliably stored task ID.
                let task_id = span
                    .extensions()
                    .get::<StoredTaskId>()
                    .map(|stored| stored.0);

                if let Some(task_id) = task_id {
                    // Calculate poll duration and record it.
                    // FIX: Remove the TaskPollStart extension after reading it.
                    if let Some(start) = span.extensions_mut().remove::<TaskPollStart>() {
                        let poll_duration = start.0.elapsed();
                        self.metrics.task_polled(task_id, poll_duration);
                    }
                }
            }
        }
    }

    fn on_close(&self, id: span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // `on_close` can be unreliable. We must be very defensive here to prevent panics.
        // We retrieve the span and its extensions *before* they are potentially dropped.
        if let Some(span) = ctx.span(&id) {
            let meta = span.metadata();
            if meta.target() == "tokio::task" && meta.name() == "runtime.spawn" {
                // The most reliable way to identify the task is from the extension we stored in `on_new_span`.
                // Calling `tokio::task::try_id()` here is not safe, as this code may execute
                // in a completely different thread context long after the task has completed.
                if let Some(stored_task_id) = span.extensions().get::<StoredTaskId>() {
                    self.metrics.task_completed(stored_task_id.0);
                } else {
                    // If we couldn't find a stored task ID, we cannot reliably report completion.
                    // We log this for debugging but do NOT attempt to use `try_id()` as a fallback
                    // to avoid incorrect metrics or panics.
                    tracing::debug!(
                        span_id = ?id,
                        span_name = span.name(),
                        "TaskTrackingLayer: Could not find StoredTaskId in span extensions during on_close. Task completion will not be recorded."
                    );
                }
            }
        } else {
            tracing::debug!(
                span_id = ?id,
                "TaskTrackingLayer: Span data was not available in on_close. Task completion may be missed."
            );
        }
    }
}

#[cfg(feature = "tracing-layer")]
/// Helper struct to track when task polling starts
#[derive(Debug, Clone, Copy)]
struct TaskPollStart(Instant);

#[cfg(feature = "tracing-layer")]
/// Helper struct to store task ID in span extensions for reliable retrieval
#[derive(Debug, Clone, Copy)]
struct StoredTaskId(Id);

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_task_lifecycle() {
        let metrics = TaskMetrics::new();
        // Use actual tokio spawn to get real task ID
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        });
        let task_id = handle.id();

        // Spawn task
        metrics.task_spawned(task_id, "test_task");
        let stats = metrics.get_stats();
        assert_eq!(stats.spawned_count, 1);
        assert_eq!(stats.active_count, 1);

        // Poll task
        metrics.task_polled(task_id, Duration::from_millis(5));
        let stats = metrics.get_stats();
        assert_eq!(stats.total_polls, 1);

        // Complete task
        metrics.task_completed(task_id);
        let _ = handle.await; // Ensure task completes
        let stats = metrics.get_stats();
        assert_eq!(stats.active_count, 0);
        assert_eq!(stats.completed_count, 1);
    }

    #[tokio::test]
    async fn test_slow_start_detection() {
        let metrics = TaskMetrics::new();
        // Use actual tokio spawn to get real task ID
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        });
        let task_id = handle.id();

        // Spawn task
        metrics.task_spawned(task_id, "slow_task");

        // Simulate delay before first poll
        tokio::time::sleep(Duration::from_millis(101)).await;
        metrics.task_polled(task_id, Duration::from_millis(1));

        let _ = handle.await; // Ensure task completes
        let stats = metrics.get_stats();
        assert_eq!(stats.slow_start_tasks, 1);
    }
}
