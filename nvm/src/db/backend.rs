//! Database Backend Abstraction Layer
//!
//! Provides a unified interface for PostgreSQL and SQLite backends.
//! Automatically selects the appropriate backend based on configuration.

use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;

#[cfg(feature = "postgres")]
use sqlx::postgres::{PgPool, PgPoolOptions};

#[cfg(feature = "sqlite")]
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions, SqliteConnectOptions};

use super::DatabaseError;

/// Database backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    /// PostgreSQL (production)
    PostgreSQL,
    /// SQLite (development/testing)
    SQLite,
    /// In-memory SQLite (testing only)
    SQLiteMemory,
}

impl Default for DatabaseBackend {
    fn default() -> Self {
        // Default to SQLite for easy development
        DatabaseBackend::SQLite
    }
}

impl std::fmt::Display for DatabaseBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseBackend::PostgreSQL => write!(f, "PostgreSQL"),
            DatabaseBackend::SQLite => write!(f, "SQLite"),
            DatabaseBackend::SQLiteMemory => write!(f, "SQLite (in-memory)"),
        }
    }
}

/// Unified database pool that can be either PostgreSQL or SQLite
pub enum UnifiedPool {
    #[cfg(feature = "postgres")]
    Postgres(PgPool),
    #[cfg(feature = "sqlite")]
    Sqlite(SqlitePool),
    /// Fallback when no database feature is enabled
    None,
}

impl UnifiedPool {
    /// Check if pool is available
    pub fn is_available(&self) -> bool {
        !matches!(self, UnifiedPool::None)
    }

    /// Get the backend type
    pub fn backend_type(&self) -> Option<DatabaseBackend> {
        match self {
            #[cfg(feature = "postgres")]
            UnifiedPool::Postgres(_) => Some(DatabaseBackend::PostgreSQL),
            #[cfg(feature = "sqlite")]
            UnifiedPool::Sqlite(_) => Some(DatabaseBackend::SQLite),
            UnifiedPool::None => None,
        }
    }

    /// Get pool size (approximate)
    pub fn size(&self) -> u32 {
        match self {
            #[cfg(feature = "postgres")]
            UnifiedPool::Postgres(pool) => pool.size(),
            #[cfg(feature = "sqlite")]
            UnifiedPool::Sqlite(pool) => pool.size(),
            UnifiedPool::None => 0,
        }
    }

    /// Get number of idle connections
    pub fn num_idle(&self) -> usize {
        match self {
            #[cfg(feature = "postgres")]
            UnifiedPool::Postgres(pool) => pool.num_idle(),
            #[cfg(feature = "sqlite")]
            UnifiedPool::Sqlite(pool) => pool.num_idle(),
            UnifiedPool::None => 0,
        }
    }

    /// Close the pool
    pub async fn close(&self) {
        match self {
            #[cfg(feature = "postgres")]
            UnifiedPool::Postgres(pool) => pool.close().await,
            #[cfg(feature = "sqlite")]
            UnifiedPool::Sqlite(pool) => pool.close().await,
            UnifiedPool::None => {}
        }
    }

    /// Get PostgreSQL pool (returns error if not postgres)
    #[cfg(feature = "postgres")]
    pub fn as_postgres(&self) -> Result<&PgPool, DatabaseError> {
        match self {
            UnifiedPool::Postgres(pool) => Ok(pool),
            _ => Err(DatabaseError::BackendMismatch("Expected PostgreSQL backend".to_string())),
        }
    }

    /// Get SQLite pool (returns error if not sqlite)
    #[cfg(feature = "sqlite")]
    pub fn as_sqlite(&self) -> Result<&SqlitePool, DatabaseError> {
        match self {
            UnifiedPool::Sqlite(pool) => Ok(pool),
            _ => Err(DatabaseError::BackendMismatch("Expected SQLite backend".to_string())),
        }
    }
}

/// Backend configuration
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Preferred backend (will fallback if not available)
    pub backend: DatabaseBackend,
    /// PostgreSQL connection URL
    pub postgres_url: Option<String>,
    /// SQLite database path (None = in-memory)
    pub sqlite_path: Option<PathBuf>,
    /// Maximum connections
    pub max_connections: u32,
    /// Minimum connections
    pub min_connections: u32,
    /// Connection timeout in seconds
    pub connect_timeout_secs: u64,
    /// Idle timeout in seconds
    pub idle_timeout_secs: u64,
    /// Auto-fallback to SQLite if PostgreSQL fails
    pub auto_fallback: bool,
}

impl Default for BackendConfig {
    fn default() -> Self {
        // Check environment for backend preference
        let backend = match std::env::var("NVM_DB_BACKEND").ok().as_deref() {
            Some("postgres" | "postgresql" | "pg") => DatabaseBackend::PostgreSQL,
            Some("sqlite" | "sqlite3") => DatabaseBackend::SQLite,
            Some("memory" | "sqlite:memory") => DatabaseBackend::SQLiteMemory,
            _ => {
                // Default: try postgres, fallback to sqlite
                if std::env::var("DATABASE_URL").is_ok() {
                    DatabaseBackend::PostgreSQL
                } else {
                    DatabaseBackend::SQLite
                }
            }
        };

        let postgres_url = std::env::var("DATABASE_URL").ok()
            .or_else(|| std::env::var("NVM_POSTGRES_URL").ok());

        let sqlite_path = std::env::var("NVM_SQLITE_PATH").ok()
            .map(PathBuf::from)
            .or_else(|| {
                // Default to data directory
                dirs::data_local_dir()
                    .map(|p| p.join("nvm").join("nvm.db"))
            });

        Self {
            backend,
            postgres_url,
            sqlite_path,
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 30,
            idle_timeout_secs: 300,
            auto_fallback: true,
        }
    }
}

impl BackendConfig {
    /// Create config for PostgreSQL
    pub fn postgres(url: &str) -> Self {
        Self {
            backend: DatabaseBackend::PostgreSQL,
            postgres_url: Some(url.to_string()),
            auto_fallback: false,
            ..Default::default()
        }
    }

    /// Create config for SQLite file
    pub fn sqlite(path: PathBuf) -> Self {
        Self {
            backend: DatabaseBackend::SQLite,
            sqlite_path: Some(path),
            auto_fallback: false,
            ..Default::default()
        }
    }

    /// Create config for in-memory SQLite (testing)
    pub fn sqlite_memory() -> Self {
        Self {
            backend: DatabaseBackend::SQLiteMemory,
            sqlite_path: None,
            auto_fallback: false,
            ..Default::default()
        }
    }

    /// Create config for development (auto-detect with fallback)
    pub fn development() -> Self {
        Self {
            auto_fallback: true,
            ..Default::default()
        }
    }
}

/// Connect to database using the backend configuration
pub async fn connect(config: &BackendConfig) -> Result<UnifiedPool, DatabaseError> {
    use std::time::Duration;

    // Try preferred backend first
    let result = match config.backend {
        DatabaseBackend::PostgreSQL => connect_postgres(config).await,
        DatabaseBackend::SQLite => connect_sqlite(config).await,
        DatabaseBackend::SQLiteMemory => connect_sqlite_memory(config).await,
    };

    // Handle fallback
    match result {
        Ok(pool) => Ok(pool),
        Err(e) if config.auto_fallback => {
            log::warn!(
                "Failed to connect to {} ({}), falling back to SQLite",
                config.backend, e
            );
            
            // Try SQLite as fallback
            match config.backend {
                DatabaseBackend::PostgreSQL => connect_sqlite(config).await,
                _ => Err(e), // No further fallback
            }
        }
        Err(e) => Err(e),
    }
}

/// Connect to PostgreSQL
#[cfg(feature = "postgres")]
async fn connect_postgres(config: &BackendConfig) -> Result<UnifiedPool, DatabaseError> {
    use std::time::Duration;

    let url = config.postgres_url.as_ref()
        .ok_or_else(|| DatabaseError::ConfigError("PostgreSQL URL not configured".to_string()))?;

    log::info!("Connecting to PostgreSQL database...");

    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.connect_timeout_secs))
        .idle_timeout(Duration::from_secs(config.idle_timeout_secs))
        .test_before_acquire(true)
        .connect(url)
        .await
        .map_err(|e| DatabaseError::ConnectionFailed(format!("PostgreSQL: {}", e)))?;

    log::info!("Connected to PostgreSQL successfully");
    Ok(UnifiedPool::Postgres(pool))
}

#[cfg(not(feature = "postgres"))]
async fn connect_postgres(_config: &BackendConfig) -> Result<UnifiedPool, DatabaseError> {
    Err(DatabaseError::BackendNotAvailable("PostgreSQL feature not enabled".to_string()))
}

/// Connect to SQLite file database
#[cfg(feature = "sqlite")]
async fn connect_sqlite(config: &BackendConfig) -> Result<UnifiedPool, DatabaseError> {
    use std::time::Duration;
    use std::str::FromStr;

    let path = config.sqlite_path.as_ref()
        .ok_or_else(|| DatabaseError::ConfigError("SQLite path not configured".to_string()))?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| DatabaseError::ConfigError(format!("Cannot create data directory: {}", e)))?;
    }

    log::info!("Connecting to SQLite database: {}", path.display());

    let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
        .map_err(|e| DatabaseError::ConfigError(format!("Invalid SQLite path: {}", e)))?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(config.connect_timeout_secs));

    let pool = SqlitePoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.connect_timeout_secs))
        .idle_timeout(Duration::from_secs(config.idle_timeout_secs))
        .connect_with(options)
        .await
        .map_err(|e| DatabaseError::ConnectionFailed(format!("SQLite: {}", e)))?;

    log::info!("Connected to SQLite successfully (WAL mode enabled)");
    Ok(UnifiedPool::Sqlite(pool))
}

#[cfg(not(feature = "sqlite"))]
async fn connect_sqlite(_config: &BackendConfig) -> Result<UnifiedPool, DatabaseError> {
    Err(DatabaseError::BackendNotAvailable("SQLite feature not enabled".to_string()))
}

/// Connect to in-memory SQLite (for testing)
#[cfg(feature = "sqlite")]
async fn connect_sqlite_memory(config: &BackendConfig) -> Result<UnifiedPool, DatabaseError> {
    use std::time::Duration;
    use std::str::FromStr;

    log::info!("Connecting to in-memory SQLite database (testing mode)");

    // Use shared cache for in-memory database
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .map_err(|e| DatabaseError::ConfigError(format!("Invalid SQLite options: {}", e)))?
        .shared_cache(true)
        .busy_timeout(Duration::from_secs(config.connect_timeout_secs));

    let pool = SqlitePoolOptions::new()
        .max_connections(1) // Single connection for in-memory
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(config.connect_timeout_secs))
        .connect_with(options)
        .await
        .map_err(|e| DatabaseError::ConnectionFailed(format!("SQLite (memory): {}", e)))?;

    log::info!("Connected to in-memory SQLite successfully");
    Ok(UnifiedPool::Sqlite(pool))
}

#[cfg(not(feature = "sqlite"))]
async fn connect_sqlite_memory(_config: &BackendConfig) -> Result<UnifiedPool, DatabaseError> {
    Err(DatabaseError::BackendNotAvailable("SQLite feature not enabled".to_string()))
}

/// Check database health
pub async fn health_check(pool: &UnifiedPool) -> Result<bool, DatabaseError> {
    match pool {
        #[cfg(feature = "postgres")]
        UnifiedPool::Postgres(pg) => {
            let result: Result<(i32,), _> = sqlx::query_as("SELECT 1")
                .fetch_one(pg)
                .await;
            Ok(result.is_ok())
        }
        #[cfg(feature = "sqlite")]
        UnifiedPool::Sqlite(sqlite) => {
            let result: Result<(i32,), _> = sqlx::query_as("SELECT 1")
                .fetch_one(sqlite)
                .await;
            Ok(result.is_ok())
        }
        UnifiedPool::None => Err(DatabaseError::NotConnected),
    }
}

/// Database health status for API responses
#[derive(Debug, serde::Serialize)]
pub struct DatabaseHealth {
    pub connected: bool,
    pub backend: String,
    pub pool_size: u32,
    pub active_connections: u32,
    pub idle_connections: u32,
    pub latency_ms: Option<u64>,
}

/// Get detailed health status
pub async fn get_health(pool: &UnifiedPool) -> DatabaseHealth {
    use std::time::Instant;

    let backend = pool.backend_type()
        .map(|b| b.to_string())
        .unwrap_or_else(|| "none".to_string());

    let start = Instant::now();
    let connected = health_check(pool).await.unwrap_or(false);
    let latency = if connected {
        Some(start.elapsed().as_millis() as u64)
    } else {
        None
    };

    let pool_size = pool.size();
    let idle = pool.num_idle() as u32;

    DatabaseHealth {
        connected,
        backend,
        pool_size,
        active_connections: pool_size.saturating_sub(idle),
        idle_connections: idle,
        latency_ms: latency,
    }
}
