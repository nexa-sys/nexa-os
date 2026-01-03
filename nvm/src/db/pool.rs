//! Database Connection Pool - Connection Management
//!
//! Provides connection pool management and health checks.

use std::sync::Arc;
use parking_lot::RwLock;

#[cfg(feature = "database")]
use sqlx::postgres::PgPool;

use super::{DatabaseConfig, DatabaseError};

/// Connection pool wrapper with health monitoring
pub struct ConnectionPool {
    #[cfg(feature = "database")]
    inner: Option<PgPool>,
    config: DatabaseConfig,
    stats: Arc<RwLock<PoolStats>>,
}

/// Pool statistics
#[derive(Debug, Default, Clone)]
pub struct PoolStats {
    pub total_connections: u32,
    pub idle_connections: u32,
    pub active_connections: u32,
    pub total_queries: u64,
    pub failed_queries: u64,
    pub last_health_check: Option<std::time::Instant>,
    pub is_healthy: bool,
}

impl ConnectionPool {
    pub fn new(config: DatabaseConfig) -> Self {
        Self {
            #[cfg(feature = "database")]
            inner: None,
            config,
            stats: Arc::new(RwLock::new(PoolStats::default())),
        }
    }

    /// Initialize connection pool
    #[cfg(feature = "database")]
    pub async fn connect(&mut self) -> Result<(), DatabaseError> {
        use sqlx::postgres::PgPoolOptions;
        use std::time::Duration;

        let pool = PgPoolOptions::new()
            .max_connections(self.config.max_connections)
            .min_connections(self.config.min_connections)
            .acquire_timeout(Duration::from_secs(self.config.connect_timeout))
            .idle_timeout(Duration::from_secs(self.config.idle_timeout))
            .test_before_acquire(true)
            .connect(&self.config.url)
            .await
            .map_err(|e| DatabaseError::ConnectionFailed(e.to_string()))?;

        self.inner = Some(pool);
        self.update_stats();
        
        Ok(())
    }

    /// Get pool reference
    #[cfg(feature = "database")]
    pub fn get(&self) -> Result<&PgPool, DatabaseError> {
        self.inner.as_ref().ok_or(DatabaseError::NotConnected)
    }

    /// Update pool statistics
    #[cfg(feature = "database")]
    pub fn update_stats(&self) {
        if let Some(pool) = &self.inner {
            let mut stats = self.stats.write();
            stats.total_connections = pool.size();
            stats.idle_connections = pool.num_idle() as u32;
            stats.active_connections = stats.total_connections - stats.idle_connections;
            stats.last_health_check = Some(std::time::Instant::now());
            stats.is_healthy = true;
        }
    }

    /// Health check
    #[cfg(feature = "database")]
    pub async fn health_check(&self) -> Result<bool, DatabaseError> {
        let pool = self.get()?;
        
        let result: Result<(i32,), _> = sqlx::query_as("SELECT 1")
            .fetch_one(pool)
            .await;

        let healthy = result.is_ok();
        
        {
            let mut stats = self.stats.write();
            stats.is_healthy = healthy;
            stats.last_health_check = Some(std::time::Instant::now());
        }

        Ok(healthy)
    }

    /// Get current stats
    pub fn stats(&self) -> PoolStats {
        self.stats.read().clone()
    }

    /// Close pool
    #[cfg(feature = "database")]
    pub async fn close(&mut self) {
        if let Some(pool) = self.inner.take() {
            pool.close().await;
        }
    }
}

/// Database health status for API responses
#[derive(Debug, serde::Serialize)]
pub struct DatabaseHealth {
    pub connected: bool,
    pub pool_size: u32,
    pub active_connections: u32,
    pub idle_connections: u32,
    pub latency_ms: Option<u64>,
}

/// Check database health (for /api/health endpoint)
#[cfg(feature = "database")]
pub async fn check_health(pool: &PgPool) -> DatabaseHealth {
    use std::time::Instant;
    
    let start = Instant::now();
    let connected = sqlx::query("SELECT 1")
        .fetch_one(pool)
        .await
        .is_ok();
    let latency = start.elapsed().as_millis() as u64;

    DatabaseHealth {
        connected,
        pool_size: pool.size(),
        active_connections: (pool.size() - pool.num_idle() as u32),
        idle_connections: pool.num_idle() as u32,
        latency_ms: if connected { Some(latency) } else { None },
    }
}
