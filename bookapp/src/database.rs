use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

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

/// Database pools for read and write operations
#[derive(Debug, Clone)]
pub struct DatabasePools {
    pub write_pool: Arc<PgPool>,
    pub read_pool: Arc<PgPool>,
}

impl DatabasePools {
    /// Create database pools with separate read and write connections
    pub async fn new(
        write_url: &str,
        read_url: Option<&str>,
        config: Option<DatabaseConfig>,
    ) -> Result<Self> {
        let config = config.unwrap_or_default();
        
        info!(write_url = write_url, "Creating write pool");
        let write_pool = create_pool(write_url, &config).await?;
        
        let read_pool = if let Some(read_url) = read_url {
            info!(read_url = read_url, "Creating separate read pool");
            create_pool(read_url, &config).await?
        } else {
            info!("Using write pool for read operations");
            write_pool.clone()
        };

        // Run migrations on write pool only
        debug!("Running migrations on write pool");
        sqlx::migrate!("../bookapp-dal/migrations")
            .run(write_pool.as_ref())
            .await?;

        Ok(Self {
            write_pool,
            read_pool,
        })
    }

    /// Create database pools with a single connection (development/testing)
    pub async fn single(database_url: &str, config: Option<DatabaseConfig>) -> Result<Self> {
        Self::new(database_url, None, config).await
    }
}

async fn create_pool(database_url: &str, config: &DatabaseConfig) -> Result<Arc<PgPool>> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(config.acquire_timeout)
        .idle_timeout(config.idle_timeout)
        .max_lifetime(config.max_lifetime)
        .connect(database_url)
        .await?;

    Ok(Arc::new(pool))
}