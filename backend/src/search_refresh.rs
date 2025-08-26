use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use once_cell::sync::OnceCell;
use opentelemetry::metrics::{Counter, Histogram, Meter, UpDownCounter};
use opentelemetry::KeyValue;
use tracing::{error, info, instrument};

use bookapp_dal::BookRepositoryImpl;

#[derive(Debug)]
pub struct SmartSearchRefresher {
    last_refresh: AtomicU64,
    refresh_in_progress: AtomicBool,
    min_interval_secs: u64,
    max_staleness_secs: u64,
    pending_updates: AtomicU64,
}

impl SmartSearchRefresher {
    pub fn new(min_interval_secs: u64, max_staleness_secs: u64) -> Self {
        Self {
            last_refresh: AtomicU64::new(0),
            refresh_in_progress: AtomicBool::new(false),
            min_interval_secs,
            max_staleness_secs,
            pending_updates: AtomicU64::new(0),
        }
    }

    pub fn notify_book_changed(&self) {
        self.pending_updates.fetch_add(1, Ordering::Relaxed);
        metrics()
            .pending
            .add(1, &[KeyValue::new("view.name", "book_search_view")]);
    }

    /// Force a refresh for scheduled tasks (bypasses smart logic)
    #[instrument(skip(self, book_repository))]
    pub async fn force_refresh_for_schedule(&self, book_repository: Arc<BookRepositoryImpl>) -> Result<()> {
        // For scheduled refresh, we bypass the smart logic and always refresh
        self.execute_refresh(book_repository, "scheduled").await?;
        Ok(())
    }

    #[instrument(
        skip(self, book_repository),
        fields(
            pending_updates,
            last_refresh_age_secs,
            refresh_decision,
            refresh_duration_ms,
            circuit_breaker.state
        )
    )]
    pub async fn maybe_refresh(
        &self,
        book_repository: Arc<BookRepositoryImpl>,
    ) -> Result<RefreshDecision> {
        let common_attrs = [KeyValue::new("view.name", "book_search_view")];
        metrics().requests.add(1, &common_attrs);
        let now_secs = current_timestamp_secs();
        let last_refresh = self.last_refresh.load(Ordering::Relaxed);
        let age_secs = now_secs.saturating_sub(last_refresh);
        let pending = self.pending_updates.load(Ordering::Relaxed);

        tracing::Span::current().record("pending_updates", pending);
        tracing::Span::current().record("last_refresh_age_secs", age_secs);

        let decision = self.should_refresh(age_secs, pending);
        tracing::Span::current().record("refresh_decision", format!("{:?}", decision));

        match decision {
            RefreshDecision::Skip(reason) => {
                tracing::Span::current().record("circuit_breaker.state", "closed");
                metrics().skipped.add(
                    1,
                    &[
                        KeyValue::new("reason", reason.clone()),
                        KeyValue::new("circuit_breaker.state", "closed"),
                        KeyValue::new("view.name", "book_search_view"),
                    ],
                );
                info!(
                    reason = %reason,
                    pending_updates = pending,
                    age_secs = age_secs,
                    "Skipping search index refresh"
                );
                Ok(RefreshDecision::Skip(reason))
            }
            RefreshDecision::Refresh => {
                tracing::Span::current().record("circuit_breaker.state", "open");
                // Determine trigger for metrics
                let trigger = if age_secs > self.max_staleness_secs {
                    "stale_force"
                } else {
                    "interval_ok"
                };
                self.execute_refresh(book_repository, trigger).await
            }
            // Unreachable here, but handle exhaustively for the compiler
            RefreshDecision::Refreshed { .. } => {
                tracing::Span::current().record("circuit_breaker.state", "closed");
                info!("Refresh already completed by another caller");
                Ok(RefreshDecision::Skip("already refreshed".into()))
            }
        }
    }

    fn should_refresh(&self, age_secs: u64, pending_updates: u64) -> RefreshDecision {
        if self.refresh_in_progress.load(Ordering::Relaxed) {
            return RefreshDecision::Skip("refresh already in progress".into());
        }
        if pending_updates == 0 {
            return RefreshDecision::Skip("no pending updates".into());
        }
        if age_secs > self.max_staleness_secs {
            return RefreshDecision::Refresh;
        }
        if age_secs < self.min_interval_secs {
            return RefreshDecision::Skip(format!(
                "rate limited: {}s < {}s minimum interval",
                age_secs, self.min_interval_secs
            ));
        }
        RefreshDecision::Refresh
    }

    #[instrument(
        skip(self, book_repository),
        fields(refresh_duration_ms, rows_affected)
    )]
    async fn execute_refresh(
        &self,
        book_repository: Arc<BookRepositoryImpl>,
        trigger: &'static str,
    ) -> Result<RefreshDecision> {
        if self
            .refresh_in_progress
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return Ok(RefreshDecision::Skip("concurrent refresh detected".into()));
        }

        let start_time = std::time::Instant::now();
        let result = self.do_refresh(book_repository).await;
        let refresh_duration = start_time.elapsed();

        self.refresh_in_progress.store(false, Ordering::Relaxed);

        tracing::Span::current().record("refresh_duration_ms", refresh_duration.as_millis() as u64);

        match result {
            Ok(rows_affected) => {
                let now_secs = current_timestamp_secs();
                let prev_last = self.last_refresh.swap(now_secs, Ordering::Relaxed);
                let prev_pending = self.pending_updates.swap(0, Ordering::Relaxed);

                tracing::Span::current().record("rows_affected", rows_affected);
                info!(
                    refresh_duration_ms = refresh_duration.as_millis(),
                    rows_affected = rows_affected,
                    "Search index refresh completed successfully"
                );
                metrics().executed.add(
                    1,
                    &[
                        KeyValue::new("trigger", trigger),
                        KeyValue::new("circuit_breaker.state", "open"),
                        KeyValue::new("view.name", "book_search_view"),
                    ],
                );
                metrics().duration.record(
                    refresh_duration.as_secs_f64(),
                    &[
                        KeyValue::new("trigger", trigger),
                        KeyValue::new("view.name", "book_search_view"),
                    ],
                );
                // Adjust gauges using up/down counters
                if prev_pending > 0 {
                    metrics().pending.add(
                        -(prev_pending as i64),
                        &[KeyValue::new("view.name", "book_search_view")],
                    );
                }
                // Set last refresh timestamp by adding delta from previous
                let delta = if prev_last == 0 {
                    now_secs as i64
                } else {
                    now_secs as i64 - prev_last as i64
                };
                if delta != 0 {
                    metrics()
                        .last_ts
                        .add(delta, &[KeyValue::new("view.name", "book_search_view")]);
                }

                Ok(RefreshDecision::Refreshed {
                    duration: refresh_duration,
                    rows_affected,
                })
            }
            Err(e) => {
                error!(
                    error = %e,
                    refresh_duration_ms = refresh_duration.as_millis(),
                    "Search index refresh failed"
                );
                Err(e)
            }
        }
    }

    async fn do_refresh(&self, book_repository: Arc<BookRepositoryImpl>) -> Result<u64> {
        let pool = book_repository.write_pool();
        let result = sqlx::query("REFRESH MATERIALIZED VIEW CONCURRENTLY book_search_view")
            .execute(pool.as_ref())
            .await?;
        Ok(result.rows_affected())
    }
}

#[derive(Debug, Clone)]
pub enum RefreshDecision {
    Skip(String),
    Refresh,
    Refreshed {
        duration: std::time::Duration,
        rows_affected: u64,
    },
}

fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone)]
pub struct SearchRefreshConfig {
    pub min_interval_secs: u64,
    pub max_staleness_secs: u64,
    pub enabled: bool,
}

impl Default for SearchRefreshConfig {
    fn default() -> Self {
        Self {
            min_interval_secs: 30,
            max_staleness_secs: 300,
            enabled: true,
        }
    }
}

struct RefreshMetrics {
    requests: Counter<u64>,
    executed: Counter<u64>,
    skipped: Counter<u64>,
    duration: Histogram<f64>,
    pending: UpDownCounter<i64>,
    last_ts: UpDownCounter<i64>,
}

static METRICS: OnceCell<RefreshMetrics> = OnceCell::new();

fn metrics() -> &'static RefreshMetrics {
    METRICS.get_or_init(|| {
        let meter: Meter = opentelemetry::global::meter("backend");
        let requests = meter
            .u64_counter("search_refresh_requests")
            .with_description("Total smart search refresh requests")
            .build();
        let executed = meter
            .u64_counter("search_refresh_executed")
            .with_description("Total smart search refreshes executed")
            .build();
        let skipped = meter
            .u64_counter("search_refresh_skipped")
            .with_description("Total smart search refreshes skipped")
            .build();
        let duration = meter
            .f64_histogram("search_refresh_duration_seconds")
            .with_description("Duration of search index refresh operations in seconds")
            .build();
        let pending = meter
            .i64_up_down_counter("search_refresh_pending_updates")
            .with_description("Current number of pending search refresh updates")
            .build();
        let last_ts = meter
            .i64_up_down_counter("search_refresh_last_refresh_timestamp_seconds")
            .with_description("Unix timestamp (seconds) of last successful search refresh")
            .build();
        RefreshMetrics {
            requests,
            executed,
            skipped,
            duration,
            pending,
            last_ts,
        }
    })
}

impl SmartSearchRefresher {
    pub fn register_observables(self: &Arc<Self>) -> Vec<Box<dyn std::any::Any + Send + Sync>> {
        // Observables are not registered with this SDK version; using UpDownCounters instead.
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip_when_no_pending() {
        let refresher = SmartSearchRefresher::new(30, 300);
        let age = 10;
        let decision = refresher.should_refresh(age, 0);
        match decision {
            RefreshDecision::Skip(reason) => assert!(reason.contains("no pending")),
            _ => panic!("expected skip"),
        }
    }

    #[test]
    fn test_circuit_breaker_in_progress() {
        let refresher = SmartSearchRefresher::new(30, 300);
        refresher.refresh_in_progress.store(true, Ordering::Relaxed);
        let decision = refresher.should_refresh(999, 10);
        match decision {
            RefreshDecision::Skip(reason) => assert!(reason.contains("in progress")),
            _ => panic!("expected skip due to in-progress"),
        }
    }

    #[test]
    fn test_rate_limited() {
        let refresher = SmartSearchRefresher::new(30, 300);
        // Simulate a recent refresh by setting last_refresh to now
        let age = 10; // < min_interval
        let decision = refresher.should_refresh(age, 5);
        match decision {
            RefreshDecision::Skip(reason) => assert!(reason.contains("rate limited")),
            _ => panic!("expected skip due to rate limiting"),
        }
    }

    #[test]
    fn test_force_due_to_staleness() {
        let refresher = SmartSearchRefresher::new(30, 300);
        let age = 1000; // > max_staleness
        let decision = refresher.should_refresh(age, 5);
        match decision {
            RefreshDecision::Refresh => {}
            _ => panic!("expected refresh due to staleness"),
        }
    }
}
