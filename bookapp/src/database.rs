use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use sqlx_tracing::{Pool as TracedPool, PoolBuilder as TracedPoolBuilder};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// Type alias for the traced PostgreSQL pool
pub type TracedPgPool = TracedPool<sqlx::Postgres>;

/// Database connection configuration
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 2,
            acquire_timeout: Duration::from_secs(3),
            idle_timeout: Duration::from_secs(60),
            max_lifetime: Duration::from_secs(1800),
        }
    }
}

/// Paired raw and traced pools
struct PoolPair {
    raw: Arc<PgPool>,
    traced: Arc<TracedPgPool>,
}

/// Database pools for read and write operations with OpenTelemetry tracing
#[derive(Clone)]
pub struct DatabasePools {
    /// Traced pool for regular queries (creates OTel spans)
    pub write_pool: Arc<TracedPgPool>,
    pub read_pool: Arc<TracedPgPool>,
    /// Raw pools for transactions and migrations (TracedPgPool doesn't support Deref)
    raw_write_pool: Arc<PgPool>,
    raw_read_pool: Arc<PgPool>,
}

impl DatabasePools {
    /// Create database pools with separate read and write connections
    /// Pools are wrapped with sqlx-tracing for automatic OpenTelemetry span creation
    pub async fn new(
        write_url: &str,
        read_url: Option<&str>,
        config: Option<DatabaseConfig>,
    ) -> Result<Self> {
        let config = config.unwrap_or_default();

        info!(write_url = write_url, "Creating write pool");
        let write_pair = create_pool_pair(write_url, &config, "bookapp-write").await?;

        let read_pair = if let Some(read_url) = read_url {
            info!(read_url = read_url, "Creating separate read pool");
            create_pool_pair(read_url, &config, "bookapp-read").await?
        } else {
            info!("Using write pool for read operations");
            PoolPair {
                raw: write_pair.raw.clone(),
                traced: write_pair.traced.clone(),
            }
        };

        // Run migrations on write pool only (use raw pool)
        debug!("Running migrations on write pool");
        let mut bookapp_migrator = sqlx::migrate!("../bookapp-dal/migrations");
        bookapp_migrator.set_ignore_missing(true);
        bookapp_migrator.run(&*write_pair.raw).await?;
        debug!("Running error injection migrations on write pool");
        let mut error_injection_migrator = sqlx::migrate!("../error-injection-dal/migrations");
        error_injection_migrator.set_ignore_missing(true);
        error_injection_migrator.run(&*write_pair.raw).await?;

        Ok(Self {
            write_pool: write_pair.traced,
            read_pool: read_pair.traced,
            raw_write_pool: write_pair.raw,
            raw_read_pool: read_pair.raw,
        })
    }

    /// Create database pools with a single connection (development/testing)
    pub async fn single(database_url: &str, config: Option<DatabaseConfig>) -> Result<Self> {
        Self::new(database_url, None, config).await
    }

    /// Create database pools from an existing PgPool (for tests with transactional test harnesses)
    pub fn from_pg_pool(pool: PgPool) -> Self {
        let raw = Arc::new(pool.clone());
        let traced = Arc::new(
            TracedPoolBuilder::from(pool)
                .with_name("test-pool")
                .with_database("bookapp")
                .with_host("db")
                .with_port(5432)
                .build(),
        );
        Self {
            write_pool: traced.clone(),
            read_pool: traced,
            raw_write_pool: raw.clone(),
            raw_read_pool: raw,
        }
    }

    /// Get the underlying PgPool for the write connection (for transactions and admin operations)
    pub fn write_pg_pool(&self) -> Arc<PgPool> {
        self.raw_write_pool.clone()
    }

    /// Get the underlying PgPool for the read connection
    pub fn read_pg_pool(&self) -> Arc<PgPool> {
        self.raw_read_pool.clone()
    }
}

/// Create both raw and traced PostgreSQL pools with OpenTelemetry semantic convention attributes
async fn create_pool_pair(
    database_url: &str,
    config: &DatabaseConfig,
    pool_name: &str,
) -> Result<PoolPair> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(config.acquire_timeout)
        .idle_timeout(config.idle_timeout)
        .max_lifetime(config.max_lifetime)
        .connect(database_url)
        .await?;

    let raw = Arc::new(pool.clone());

    // Wrap with sqlx-tracing for automatic span creation
    // This adds db.system, db.name, db.statement attributes to spans
    let traced_pool = TracedPoolBuilder::from(pool)
        .with_name(pool_name)
        .with_database("bookapp")
        .with_host("db")
        .with_port(5432)
        .build();

    info!(
        pool_name = pool_name,
        "Created traced database pool with OTel instrumentation"
    );

    Ok(PoolPair {
        raw,
        traced: Arc::new(traced_pool),
    })
}
