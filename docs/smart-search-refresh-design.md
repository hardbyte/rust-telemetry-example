# Smart Search Index Refresh Design

> **Note:** The system now maintains a dedicated `book_search_index` table that is updated incrementally by the backend Kafka consumer. The Smart Search Refresh logic described below remains in place as a safety net that can rebuild the index if the incremental path falls behind or becomes inconsistent.

## Context & Problem Statement

The current book ingestion system triggers a full materialized view refresh for every individual book created, which creates significant scalability bottlenecks:

```rust
// Current problematic approach
async fn background_process_new_book(book_id: i32, book_repository: Arc<BookRepositoryImpl>) -> Result<()> {
    // This runs for EVERY book ingestion
    sqlx::query("REFRESH MATERIALIZED VIEW CONCURRENTLY book_search_index")
        .execute(pool.as_ref())
        .await?;
}
```

### Performance Issues
- **Lock Contention**: Multiple concurrent `REFRESH MATERIALIZED VIEW CONCURRENTLY` commands queue behind each other
- **Resource Waste**: Full table scan and rebuild for single book changes
- **I/O Amplification**: Each refresh processes entire `books` table regardless of change size
- **Blocking Behavior**: Even "concurrent" refreshes have internal locking that serializes operations

### Requirements
- Maintain search index freshness for new books
- Support high-throughput book ingestion (100s of books/minute)
- Use only PostgreSQL (no external search engines)
- Preserve observability and error handling
- Minimal complexity increase

## Solution: Smart Refresh with Circuit Breaker

### Core Principles
1. **Adaptive Refresh**: Only refresh when needed and not too frequently
2. **Circuit Breaking**: Prevent concurrent refreshes and refresh storms
3. **Observability**: Track refresh patterns and performance
4. **Graceful Degradation**: System remains functional even if refresh fails

### Architecture

```
Book Ingestion → Smart Refresh Manager → PostgreSQL Materialized View
     ↓                    ↓                        ↓
   High Rate         Rate Limiting           Batch Updates
   Events           Circuit Breaking         Efficient Refresh
```

## Implementation

### 1. Smart Refresh Manager

```rust
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{instrument, info, warn, error};

#[derive(Debug)]
pub struct SmartSearchRefresher {
    /// Timestamp of last successful refresh (seconds since UNIX epoch)
    last_refresh: AtomicU64,
    
    /// Prevents concurrent refresh operations
    refresh_in_progress: AtomicBool,
    
    /// Minimum seconds between refreshes (circuit breaker)
    min_interval_secs: u64,
    
    /// Maximum seconds since last refresh before forcing one
    max_staleness_secs: u64,
    
    /// Count of pending updates since last refresh
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
    
    /// Called when a book is created/updated
    pub fn notify_book_changed(&self) {
        self.pending_updates.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Smart refresh decision based on circuit breaker logic
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
    pub async fn maybe_refresh(&self, book_repository: Arc<BookRepositoryImpl>) -> Result<RefreshDecision> {
        let now_secs = current_timestamp_secs();
        let last_refresh = self.last_refresh.load(Ordering::Relaxed);
        let age_secs = now_secs - last_refresh;
        let pending = self.pending_updates.load(Ordering::Relaxed);
        
        // Record metrics in span
        tracing::Span::current().record("pending_updates", pending);
        tracing::Span::current().record("last_refresh_age_secs", age_secs);
        
        let decision = self.should_refresh(age_secs, pending);
        tracing::Span::current().record("refresh_decision", &format!("{:?}", decision));
        
        match decision {
            RefreshDecision::Skip(reason) => {
                tracing::Span::current().record("circuit_breaker.state", "closed");
                info!(
                    reason = %reason,
                    pending_updates = pending,
                    age_secs = age_secs,
                    "Skipping search index refresh"
                );
                Ok(decision)
            }
            RefreshDecision::Refresh => {
                tracing::Span::current().record("circuit_breaker.state", "open");
                self.execute_refresh(book_repository).await
            }
        }
    }
    
    fn should_refresh(&self, age_secs: u64, pending_updates: u64) -> RefreshDecision {
        // Circuit breaker: prevent concurrent refreshes
        if self.refresh_in_progress.load(Ordering::Relaxed) {
            return RefreshDecision::Skip("refresh already in progress".into());
        }
        
        // No changes to refresh
        if pending_updates == 0 {
            return RefreshDecision::Skip("no pending updates".into());
        }
        
        // Force refresh if too stale
        if age_secs > self.max_staleness_secs {
            return RefreshDecision::Refresh;
        }
        
        // Rate limit: respect minimum interval
        if age_secs < self.min_interval_secs {
            return RefreshDecision::Skip(format!(
                "rate limited: {}s < {}s minimum interval", 
                age_secs, self.min_interval_secs
            ));
        }
        
        RefreshDecision::Refresh
    }
    
    #[instrument(skip(self, book_repository), fields(refresh_duration_ms, rows_affected))]
    async fn execute_refresh(&self, book_repository: Arc<BookRepositoryImpl>) -> Result<RefreshDecision> {
        // Atomic check-and-set for refresh in progress
        if self.refresh_in_progress
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return Ok(RefreshDecision::Skip("concurrent refresh detected".into()));
        }
        
        let start_time = std::time::Instant::now();
        let result = self.do_refresh(book_repository).await;
        let refresh_duration = start_time.elapsed();
        
        // Always clear the in-progress flag
        self.refresh_in_progress.store(false, Ordering::Relaxed);
        
        // Record metrics
        tracing::Span::current().record("refresh_duration_ms", refresh_duration.as_millis() as u64);
        
        match result {
            Ok(rows_affected) => {
                let now_secs = current_timestamp_secs();
                self.last_refresh.store(now_secs, Ordering::Relaxed);
                self.pending_updates.store(0, Ordering::Relaxed);
                
                tracing::Span::current().record("rows_affected", rows_affected);
                
                info!(
                    refresh_duration_ms = refresh_duration.as_millis(),
                    rows_affected = rows_affected,
                    "Search index refresh completed successfully"
                );
                
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
        let result = sqlx::query("REFRESH MATERIALIZED VIEW CONCURRENTLY book_search_index")
            .execute(pool.as_ref())
            .await?;
        Ok(result.rows_affected())
    }
}

#[derive(Debug, Clone)]
pub enum RefreshDecision {
    Skip(String),
    Refresh,
    Refreshed { duration: std::time::Duration, rows_affected: u64 },
}

fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
```

### 2. Integration with Book Ingestion

```rust
// In book_ingestion.rs
use crate::search_refresh::SmartSearchRefresher;
use std::sync::Arc;

pub struct BookIngestionService {
    book_repository: Arc<BookRepositoryImpl>,
    search_refresher: Arc<SmartSearchRefresher>,
}

impl BookIngestionService {
    pub fn new(book_repository: Arc<BookRepositoryImpl>) -> Self {
        Self {
            book_repository,
            // Configure: min 30s interval, max 5min staleness
            search_refresher: Arc::new(SmartSearchRefresher::new(30, 300)),
        }
    }
}

#[instrument(
    skip(book_repository, search_refresher), 
    fields(book_id, search_refresh.decision)
)]
async fn background_process_new_book(
    book_id: i32,
    book_repository: Arc<BookRepositoryImpl>,
    search_refresher: Arc<SmartSearchRefresher>,
) -> Result<()> {
    info!(book_id = book_id, "Starting background processing for new book");
    
    // Notify the refresher about the change
    search_refresher.notify_book_changed();
    
    // Attempt smart refresh
    match search_refresher.maybe_refresh(book_repository.clone()).await {
        Ok(decision) => {
            tracing::Span::current().record("search_refresh.decision", &format!("{:?}", decision));
            match decision {
                RefreshDecision::Refreshed { duration, rows_affected } => {
                    info!(
                        book_id = book_id,
                        refresh_duration_ms = duration.as_millis(),
                        rows_affected = rows_affected,
                        "Search index refreshed for new book"
                    );
                }
                RefreshDecision::Skip(reason) => {
                    info!(
                        book_id = book_id,
                        reason = %reason,
                        "Search index refresh skipped"
                    );
                }
                RefreshDecision::Refresh => {
                    // This shouldn't happen in practice
                    warn!(book_id = book_id, "Unexpected refresh decision state");
                }
            }
        }
        Err(e) => {
            // Don't fail the entire book processing if search refresh fails
            error!(
                book_id = book_id,
                error = %e,
                "Search index refresh failed, continuing with book processing"
            );
        }
    }
    
    info!(book_id = book_id, "Background processing for new book completed");
    Ok(())
}
```

### 3. Configuration & Monitoring

```rust
#[derive(Debug, Clone)]
pub struct SearchRefreshConfig {
    /// Minimum seconds between refreshes (circuit breaker)
    pub min_interval_secs: u64,
    
    /// Maximum seconds before forcing a refresh
    pub max_staleness_secs: u64,
    
    /// Enable/disable smart refresh entirely
    pub enabled: bool,
}

impl Default for SearchRefreshConfig {
    fn default() -> Self {
        Self {
            min_interval_secs: 30,    // At most one refresh per 30 seconds
            max_staleness_secs: 300,  // Force refresh after 5 minutes
            enabled: true,
        }
    }
}

// Metrics for monitoring
pub struct SearchRefreshMetrics {
    pub total_refresh_requests: u64,
    pub refreshes_executed: u64,
    pub refreshes_skipped: u64,
    pub avg_refresh_duration_ms: f64,
    pub last_refresh_timestamp: u64,
    pub pending_updates: u64,
}
```

## Decision Matrix

The smart refresher uses this decision logic:

| Condition | Action | Reason |
|-----------|--------|--------|
| `refresh_in_progress == true` | Skip | Prevent concurrent refreshes |
| `pending_updates == 0` | Skip | No changes to refresh |
| `age > max_staleness_secs` | Refresh | Force refresh when too stale |
| `age < min_interval_secs` | Skip | Rate limiting |
| Default | Refresh | Safe to refresh |

## Performance Characteristics

### Before (Per-Book Refresh)
- **Throughput**: ~10 books/minute (limited by refresh time)
- **Latency**: 200-2000ms per book
- **Resource Usage**: High (full table scan per book)
- **Search Freshness**: Immediate

### After (Smart Refresh)
- **Throughput**: 100s of books/minute (limited by business logic)
- **Latency**: 1-10ms per book (except refresh triggers)
- **Resource Usage**: Low (batched refreshes)
- **Search Freshness**: 30 seconds to 5 minutes

## Observability

The implementation provides comprehensive tracing:

```
INFO background_process_new_book{book_id=123 search_refresh.decision="Refreshed { duration: 450ms, rows_affected: 1 }"}
INFO background_process_new_book{book_id=124 search_refresh.decision="Skip(rate limited: 15s < 30s minimum interval)"}
INFO background_process_new_book{book_id=125 search_refresh.decision="Skip(refresh already in progress)"}
```

Grafana dashboard queries:
```promql
# Refresh rate
rate(search_refresh_total[5m])

# Average refresh duration
avg(search_refresh_duration_seconds)

# Circuit breaker state
search_refresh_skipped_total / search_refresh_requests_total
```

## Error Handling & Resilience

1. **Refresh Failures**: Don't block book processing
2. **Circuit Breaker**: Prevents refresh storms during outages
3. **Atomic Operations**: Prevent race conditions in refresh state
4. **Graceful Degradation**: System works even if refreshes fail

## Testing Strategy

```rust
#[tokio::test]
async fn test_smart_refresh_rate_limiting() {
    let refresher = SmartSearchRefresher::new(30, 300);
    
    // First refresh should execute
    refresher.notify_book_changed();
    let result1 = refresher.maybe_refresh(repo.clone()).await.unwrap();
    assert!(matches!(result1, RefreshDecision::Refreshed { .. }));
    
    // Immediate second refresh should be skipped
    refresher.notify_book_changed();
    let result2 = refresher.maybe_refresh(repo.clone()).await.unwrap();
    assert!(matches!(result2, RefreshDecision::Skip(_)));
}

#[tokio::test]
async fn test_smart_refresh_circuit_breaker() {
    let refresher = Arc::new(SmartSearchRefresher::new(1, 300));
    
    // Start concurrent refreshes
    let handles: Vec<_> = (0..10).map(|_| {
        let refresher = refresher.clone();
        let repo = repo.clone();
        tokio::spawn(async move {
            refresher.notify_book_changed();
            refresher.maybe_refresh(repo).await
        })
    }).collect();
    
    let results = futures::future::join_all(handles).await;
    
    // Only one should execute, others should be skipped
    let executed = results.iter()
        .filter(|r| matches!(r, Ok(Ok(RefreshDecision::Refreshed { .. }))))
        .count();
    assert_eq!(executed, 1);
}
```

## Migration Path

1. **Phase 1**: Add `SmartSearchRefresher` alongside existing refresh
2. **Phase 2**: Route ingestion through smart refresher but keep fallback
3. **Phase 3**: Remove direct refresh calls, rely on smart refresher + scheduled backup
4. **Phase 4**: Monitor and tune parameters based on production metrics

This approach provides **high performance** while maintaining **search freshness** using only PostgreSQL, with comprehensive **observability** and **error resilience**.
