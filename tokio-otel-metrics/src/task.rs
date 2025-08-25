//! Task-level metrics collection for Tokio applications.
//!
//! This module provides tools for collecting detailed metrics about individual
//! tasks in a Tokio runtime, including execution time, poll counts, and lifecycle events.

use opentelemetry::metrics::Meter;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::Id;
use tracing::debug;

/// Task metrics collector that tracks detailed task-level performance data.
///
/// This collector maintains statistics for individual tasks and provides
/// aggregated metrics suitable for OpenTelemetry observability.
#[derive(Debug)]
pub struct TaskMetrics {
    /// Map of task ID to task metadata
    tasks: Arc<Mutex<HashMap<Id, TaskMetadata>>>,
    /// Global task statistics
    stats: Arc<Mutex<TaskStats>>,
}

/// Metadata tracked for each task
#[derive(Debug, Clone)]
struct TaskMetadata {
    /// Task name/label for identification
    name: String,
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

/// Aggregated task statistics for metrics export
#[derive(Debug, Default)]
pub struct TaskStats {
    /// Total number of tasks spawned
    spawned_count: u64,
    /// Number of currently active tasks
    active_count: u64,
    /// Number of completed tasks
    completed_count: u64,
    /// Total polls across all tasks
    total_polls: u64,
    /// Total time spent in task polls
    total_poll_duration: Duration,
    /// Number of tasks that experienced first poll delay > 100ms
    slow_start_tasks: u64,
}

impl TaskMetrics {
    /// Create a new task metrics collector
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            stats: Arc::new(Mutex::new(TaskStats::default())),
        }
    }

    /// Register a new task with the collector
    pub fn task_spawned(&self, task_id: Id, name: impl Into<String>) {
        let name = name.into();
        let metadata = TaskMetadata {
            name: name.clone(),
            created_at: Instant::now(),
            first_poll_at: None,
            completed_at: None,
            poll_count: 0,
            total_poll_duration: Duration::ZERO,
            active: true,
        };

        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(task_id, metadata);

        let mut stats = self.stats.lock().unwrap();
        stats.spawned_count += 1;
        stats.active_count += 1;

        debug!("Task spawned: {} ({:?})", name, task_id);
    }

    /// Record that a task was polled
    pub fn task_polled(&self, task_id: Id, poll_duration: Duration) {
        let now = Instant::now();

        let mut tasks = self.tasks.lock().unwrap();
        if let Some(metadata) = tasks.get_mut(&task_id) {
            // Track first poll timing
            if metadata.first_poll_at.is_none() {
                metadata.first_poll_at = Some(now);

                // Check for slow start (> 100ms from spawn to first poll)
                let first_poll_delay = now.duration_since(metadata.created_at);
                if first_poll_delay > Duration::from_millis(100) {
                    let mut stats = self.stats.lock().unwrap();
                    stats.slow_start_tasks += 1;
                }
            }

            metadata.poll_count += 1;
            metadata.total_poll_duration += poll_duration;

            let mut stats = self.stats.lock().unwrap();
            stats.total_polls += 1;
            stats.total_poll_duration += poll_duration;

            debug!(
                "Task polled: {} (polls: {}, duration: {:?})",
                metadata.name, metadata.poll_count, poll_duration
            );
        }
    }

    /// Record that a task completed
    pub fn task_completed(&self, task_id: Id) {
        let now = Instant::now();

        let mut tasks = self.tasks.lock().unwrap();
        if let Some(metadata) = tasks.get_mut(&task_id) {
            if metadata.active {
                metadata.completed_at = Some(now);
                metadata.active = false;

                let mut stats = self.stats.lock().unwrap();
                stats.active_count -= 1;
                stats.completed_count += 1;

                debug!(
                    "Task completed: {} (lifetime: {:?}, polls: {})",
                    metadata.name,
                    now.duration_since(metadata.created_at),
                    metadata.poll_count
                );
            }
        }
    }

    /// Register OpenTelemetry metrics for task observability
    pub fn register_metrics(
        &self,
        meter: &Meter,
    ) -> crate::Result<Vec<crate::ObservableRegistration>> {
        let mut registrations = Vec::new();

        // Task count metrics with proper OpenTelemetry callbacks
        let stats_active = Arc::clone(&self.stats);
        let _active_tasks_gauge = meter
            .u64_observable_gauge("tokio_task_active_count")
            .with_description("Number of currently active tasks")
            .with_callback(move |observer| {
                let stats = stats_active.lock().unwrap();
                observer.observe(stats.active_count, &[]);
            })
            .build();

        let stats_completed = Arc::clone(&self.stats);
        let _completed_tasks_gauge = meter
            .u64_observable_gauge("tokio_task_completed_count")
            .with_description("Total number of completed tasks")
            .with_callback(move |observer| {
                let stats = stats_completed.lock().unwrap();
                observer.observe(stats.completed_count, &[]);
            })
            .build();

        let stats_spawned = Arc::clone(&self.stats);
        let _spawned_tasks_gauge = meter
            .u64_observable_gauge("tokio_task_spawned_count")
            .with_description("Total number of tasks spawned")
            .with_callback(move |observer| {
                let stats = stats_spawned.lock().unwrap();
                observer.observe(stats.spawned_count, &[]);
            })
            .build();

        // Task performance metrics
        let stats_polls = Arc::clone(&self.stats);
        let _total_polls_gauge = meter
            .u64_observable_gauge("tokio_task_total_polls")
            .with_description("Total number of task polls across all tasks")
            .with_callback(move |observer| {
                let stats = stats_polls.lock().unwrap();
                observer.observe(stats.total_polls, &[]);
            })
            .build();

        let stats_slow = Arc::clone(&self.stats);
        let _slow_start_tasks_gauge = meter
            .u64_observable_gauge("tokio_task_slow_start_count")
            .with_description("Number of tasks with > 100ms delay to first poll")
            .with_callback(move |observer| {
                let stats = stats_slow.lock().unwrap();
                observer.observe(stats.slow_start_tasks, &[]);
            })
            .build();

        let stats_duration = Arc::clone(&self.stats);
        let _avg_poll_duration_gauge = meter
            .f64_observable_gauge("tokio_task_avg_poll_duration_seconds")
            .with_description("Average task poll duration across all tasks")
            .with_callback(move |observer| {
                let stats = stats_duration.lock().unwrap();
                let avg_poll_duration_seconds = if stats.total_polls > 0 {
                    stats.total_poll_duration.as_secs_f64() / stats.total_polls as f64
                } else {
                    0.0
                };
                observer.observe(avg_poll_duration_seconds, &[]);
            })
            .build();

        // Create a basic registration for API compatibility
        let basic_registration =
            crate::ObservableRegistration::new("tokio_task_metrics", move || {
                vec![] // All metrics are now handled by OpenTelemetry callbacks
            });
        registrations.push(basic_registration);

        tracing::info!("Successfully registered task-level metrics with OpenTelemetry callbacks");
        Ok(registrations)
    }

    /// Get current task statistics (for debugging/testing)
    pub fn get_stats(&self) -> TaskStats {
        let stats = self.stats.lock().unwrap();
        TaskStats {
            spawned_count: stats.spawned_count,
            active_count: stats.active_count,
            completed_count: stats.completed_count,
            total_polls: stats.total_polls,
            total_poll_duration: stats.total_poll_duration,
            slow_start_tasks: stats.slow_start_tasks,
        }
    }

    /// Clear all collected metrics (useful for testing)
    pub fn clear(&self) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.clear();

        let mut stats = self.stats.lock().unwrap();
        *stats = TaskStats::default();
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
