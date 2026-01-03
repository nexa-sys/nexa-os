//! Database Module - Enterprise Multi-Backend Database Layer
//!
//! Provides database connectivity, models, and migrations for NVM.
//! Supports PostgreSQL (production) and SQLite (development/testing).
//!
//! # Backend Selection
//!
//! The backend is selected based on environment variables:
//! - `NVM_DB_BACKEND`: Explicit backend choice (`postgres`, `sqlite`, `memory`)
//! - `DATABASE_URL`: If set, defaults to PostgreSQL
//! - Otherwise: Defaults to SQLite with auto-fallback
//!
//! # Usage Examples
//!
//! ```rust,ignore
//! // Production (PostgreSQL)
//! let config = BackendConfig::postgres("postgres://user:pass@localhost/nvm");
//! let pool = connect(&config).await?;
//!
//! // Development (SQLite with auto-fallback)
//! let config = BackendConfig::development();
//! let pool = connect(&config).await?;
//!
//! // Testing (in-memory SQLite)
//! let config = BackendConfig::sqlite_memory();
//! let pool = connect(&config).await?;
//! ```

mod models;
mod repository;
mod migrations;
mod pool;
mod backend;

pub use models::*;
pub use repository::*;
pub use migrations::*;
pub use pool::*;
pub use backend::*;

use std::sync::Arc;
use parking_lot::RwLock;

#[cfg(feature = "postgres")]
use sqlx::postgres::PgPool;

#[cfg(feature = "sqlite")]
use sqlx::sqlite::SqlitePool;

/// Database configuration
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// Backend configuration
    pub backend: BackendConfig,
    /// Enable SSL (PostgreSQL only)
    pub ssl_mode: SslMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SslMode {
    Disable,
    Prefer,
    Require,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            backend: BackendConfig::default(),
            ssl_mode: SslMode::Prefer,
        }
    }
}

impl DatabaseConfig {
    /// Create config for PostgreSQL
    pub fn postgres(url: &str) -> Self {
        Self {
            backend: BackendConfig::postgres(url),
            ssl_mode: SslMode::Prefer,
        }
    }

    /// Create config for SQLite
    pub fn sqlite(path: std::path::PathBuf) -> Self {
        Self {
            backend: BackendConfig::sqlite(path),
            ssl_mode: SslMode::Disable,
        }
    }

    /// Create config for in-memory SQLite (testing)
    pub fn sqlite_memory() -> Self {
        Self {
            backend: BackendConfig::sqlite_memory(),
            ssl_mode: SslMode::Disable,
        }
    }

    /// Create config for development (auto-detect with fallback)
    pub fn development() -> Self {
        Self {
            backend: BackendConfig::development(),
            ssl_mode: SslMode::Disable,
        }
    }
}

/// Database state manager
pub struct Database {
    /// Unified connection pool
    pool: Option<UnifiedPool>,
    config: DatabaseConfig,
    initialized: bool,
}

impl Database {
    pub fn new(config: DatabaseConfig) -> Self {
        Self {
            pool: None,
            config,
            initialized: false,
        }
    }

    /// Initialize database connection pool
    pub async fn connect(&mut self) -> Result<(), DatabaseError> {
        let pool = backend::connect(&self.config.backend).await?;
        
        log::info!(
            "Database connected: {} backend",
            pool.backend_type().map(|b| b.to_string()).unwrap_or_else(|| "none".to_string())
        );
        
        self.pool = Some(pool);
        self.initialized = true;
        
        Ok(())
    }

    /// Run database migrations
    pub async fn migrate(&self) -> Result<(), DatabaseError> {
        let pool = self.pool.as_ref()
            .ok_or(DatabaseError::NotConnected)?;
        
        run_migrations(pool).await
    }

    /// Get unified pool reference
    pub fn pool(&self) -> Result<&UnifiedPool, DatabaseError> {
        self.pool.as_ref().ok_or(DatabaseError::NotConnected)
    }

    /// Get PostgreSQL pool (if using postgres backend)
    #[cfg(feature = "postgres")]
    pub fn pg_pool(&self) -> Result<&PgPool, DatabaseError> {
        self.pool()?.as_postgres()
    }

    /// Get SQLite pool (if using sqlite backend)
    #[cfg(feature = "sqlite")]
    pub fn sqlite_pool(&self) -> Result<&SqlitePool, DatabaseError> {
        self.pool()?.as_sqlite()
    }

    /// Get the active backend type
    pub fn backend_type(&self) -> Option<DatabaseBackend> {
        self.pool.as_ref().and_then(|p| p.backend_type())
    }

    /// Check if database is connected
    pub fn is_connected(&self) -> bool {
        self.initialized && self.pool.as_ref().map(|p| p.is_available()).unwrap_or(false)
    }

    /// Create default admin user if not exists
    pub async fn ensure_default_admin(&self) -> Result<(), DatabaseError> {
        let pool = self.pool()?;
        
        match pool.backend_type() {
            #[cfg(feature = "postgres")]
            Some(DatabaseBackend::PostgreSQL) => {
                let pg = pool.as_postgres()?;
                let repo = UserRepository::new_pg(pg.clone());
                self.create_admin_if_needed(&repo).await
            }
            #[cfg(feature = "sqlite")]
            Some(DatabaseBackend::SQLite | DatabaseBackend::SQLiteMemory) => {
                let sqlite = pool.as_sqlite()?;
                let repo = SqliteUserRepository::new_sqlite(sqlite.clone());
                self.create_admin_if_needed(&repo).await
            }
            _ => Err(DatabaseError::NotConnected),
        }
    }

    async fn create_admin_if_needed<R: UserRepositoryTrait>(&self, repo: &R) -> Result<(), DatabaseError> {
        if repo.find_by_username("admin").await?.is_none() {
            log::info!("Creating default admin user...");
            
            let admin = CreateUser {
                username: "admin".to_string(),
                email: "admin@localhost".to_string(),
                password: "admin123".to_string(),
                display_name: Some("Administrator".to_string()),
                role: UserRole::Admin,
            };
            
            repo.create(&admin).await?;
            log::info!("Default admin user created (username: admin, password: admin123)");
        }
        
        Ok(())
    }
}

/// Database errors
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Database connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Database not connected")]
    NotConnected,
    
    #[error("Migration failed: {0}")]
    MigrationFailed(String),
    
    #[error("Query failed: {0}")]
    QueryFailed(String),
    
    #[error("Record not found")]
    NotFound,
    
    #[error("Duplicate entry: {0}")]
    DuplicateEntry(String),
    
    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Backend not available: {0}")]
    BackendNotAvailable(String),

    #[error("Backend mismatch: {0}")]
    BackendMismatch(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

#[cfg(any(feature = "postgres", feature = "sqlite"))]
impl From<sqlx::Error> for DatabaseError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => DatabaseError::NotFound,
            sqlx::Error::Database(db_err) => {
                // PostgreSQL duplicate key: 23505
                // SQLite constraint violation: 1555 (UNIQUE), 2067 (UNIQUE)
                let code = db_err.code().map(|c| c.to_string());
                if code.as_deref() == Some("23505") || 
                   code.as_deref() == Some("1555") || 
                   code.as_deref() == Some("2067") {
                    DatabaseError::DuplicateEntry(db_err.message().to_string())
                } else {
                    DatabaseError::QueryFailed(db_err.message().to_string())
                }
            }
            _ => DatabaseError::QueryFailed(e.to_string()),
        }
    }
}

/// Global database instance
lazy_static::lazy_static! {
    pub static ref DB: Arc<RwLock<Option<Database>>> = Arc::new(RwLock::new(None));
}

/// Initialize global database with auto-detection
pub async fn init_database(config: DatabaseConfig) -> Result<(), DatabaseError> {
    let mut db = Database::new(config);
    db.connect().await?;
    db.migrate().await?;
    db.ensure_default_admin().await?;
    
    let backend_name = db.backend_type()
        .map(|b| b.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    
    log::info!("Database initialized with {} backend", backend_name);
    
    *DB.write() = Some(db);
    
    Ok(())
}

/// Initialize database for development (auto-detect, SQLite fallback)
pub async fn init_database_dev() -> Result<(), DatabaseError> {
    init_database(DatabaseConfig::development()).await
}
