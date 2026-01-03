//! Database Module - Enterprise PostgreSQL Backend
//!
//! Provides database connectivity, models, and migrations for NVM.
//! Uses SQLx with PostgreSQL for async database operations.

mod models;
mod repository;
mod migrations;
mod pool;

pub use models::*;
pub use repository::*;
pub use migrations::*;
pub use pool::*;

use std::sync::Arc;
use parking_lot::RwLock;

#[cfg(feature = "database")]
use sqlx::postgres::PgPool;

/// Database configuration
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL
    pub url: String,
    /// Maximum connections in pool
    pub max_connections: u32,
    /// Minimum connections in pool  
    pub min_connections: u32,
    /// Connection timeout in seconds
    pub connect_timeout: u64,
    /// Idle timeout in seconds
    pub idle_timeout: u64,
    /// Enable SSL
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
            url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://nvm:nvm@localhost:5432/nvm".to_string()),
            max_connections: 10,
            min_connections: 2,
            connect_timeout: 30,
            idle_timeout: 300,
            ssl_mode: SslMode::Prefer,
        }
    }
}

/// Database state manager
pub struct Database {
    #[cfg(feature = "database")]
    pool: Option<PgPool>,
    config: DatabaseConfig,
    initialized: bool,
}

impl Database {
    pub fn new(config: DatabaseConfig) -> Self {
        Self {
            #[cfg(feature = "database")]
            pool: None,
            config,
            initialized: false,
        }
    }

    /// Initialize database connection pool
    #[cfg(feature = "database")]
    pub async fn connect(&mut self) -> Result<(), DatabaseError> {
        use sqlx::postgres::PgPoolOptions;
        use std::time::Duration;

        let pool = PgPoolOptions::new()
            .max_connections(self.config.max_connections)
            .min_connections(self.config.min_connections)
            .acquire_timeout(Duration::from_secs(self.config.connect_timeout))
            .idle_timeout(Duration::from_secs(self.config.idle_timeout))
            .connect(&self.config.url)
            .await
            .map_err(|e| DatabaseError::ConnectionFailed(e.to_string()))?;

        self.pool = Some(pool);
        self.initialized = true;
        
        Ok(())
    }

    /// Run database migrations
    #[cfg(feature = "database")]
    pub async fn migrate(&self) -> Result<(), DatabaseError> {
        let pool = self.pool.as_ref()
            .ok_or(DatabaseError::NotConnected)?;
        
        run_migrations(pool).await
    }

    /// Get pool reference
    #[cfg(feature = "database")]
    pub fn pool(&self) -> Result<&PgPool, DatabaseError> {
        self.pool.as_ref().ok_or(DatabaseError::NotConnected)
    }

    /// Check if database is connected
    pub fn is_connected(&self) -> bool {
        self.initialized
    }

    /// Create default admin user if not exists
    #[cfg(feature = "database")]
    pub async fn ensure_default_admin(&self) -> Result<(), DatabaseError> {
        let pool = self.pool()?;
        let repo = UserRepository::new(pool.clone());
        
        // Check if admin exists
        if repo.find_by_username("admin").await?.is_none() {
            log::info!("Creating default admin user...");
            
            let admin = CreateUser {
                username: "admin".to_string(),
                email: "admin@localhost".to_string(),
                password: "admin123".to_string(),  // Will be hashed
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
}

#[cfg(feature = "database")]
impl From<sqlx::Error> for DatabaseError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => DatabaseError::NotFound,
            sqlx::Error::Database(db_err) => {
                if db_err.code().map(|c| c == "23505").unwrap_or(false) {
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

/// Initialize global database
#[cfg(feature = "database")]
pub async fn init_database(config: DatabaseConfig) -> Result<(), DatabaseError> {
    let mut db = Database::new(config);
    db.connect().await?;
    db.migrate().await?;
    db.ensure_default_admin().await?;
    
    *DB.write() = Some(db);
    
    Ok(())
}
