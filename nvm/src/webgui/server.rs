//! WebGUI Server Core
//!
//! Axum-based HTTP/HTTPS server with WebSocket support

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "webgui")]
use axum::Router;

use super::websocket::WebSocketManager;
use super::auth::SessionManager;

/// WebGUI server state
pub struct WebGuiState {
    /// Server configuration
    pub config: RwLock<WebGuiConfig>,
    /// WebSocket manager
    pub ws_manager: Arc<WebSocketManager>,
    /// Session manager
    pub sessions: Arc<SessionManager>,
    /// Active tasks
    pub tasks: RwLock<HashMap<Uuid, TaskInfo>>,
    /// Server statistics
    pub stats: RwLock<ServerStats>,
}

impl WebGuiState {
    pub fn new(config: WebGuiConfig) -> Self {
        Self {
            config: RwLock::new(config),
            ws_manager: Arc::new(WebSocketManager::new()),
            sessions: Arc::new(SessionManager::new(Duration::from_secs(3600 * 8))),
            tasks: RwLock::new(HashMap::new()),
            stats: RwLock::new(ServerStats::default()),
        }
    }
}

/// WebGUI server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebGuiConfig {
    /// Bind address
    pub bind_address: String,
    /// HTTP port
    pub http_port: u16,
    /// HTTPS port
    pub https_port: u16,
    /// Enable HTTPS
    pub tls_enabled: bool,
    /// TLS certificate path
    pub tls_cert_path: Option<PathBuf>,
    /// TLS key path
    pub tls_key_path: Option<PathBuf>,
    /// Static assets directory
    pub assets_dir: PathBuf,
    /// Enable compression
    pub compression: bool,
    /// Session timeout (seconds)
    pub session_timeout: u64,
    /// Maximum upload size (bytes)
    pub max_upload_size: usize,
    /// Enable API documentation
    pub enable_docs: bool,
    /// Trusted proxy headers
    pub trusted_proxies: Vec<String>,
    /// CORS origins
    pub cors_origins: Vec<String>,
    /// Rate limit (requests per minute)
    pub rate_limit: u32,
    /// Enable audit logging
    pub audit_logging: bool,
    /// Custom branding
    pub branding: BrandingConfig,
}

impl Default for WebGuiConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            http_port: 8006,
            https_port: 8007,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            assets_dir: PathBuf::from("/usr/share/nvm/webgui"),
            compression: true,
            session_timeout: 28800, // 8 hours
            max_upload_size: 10 * 1024 * 1024 * 1024, // 10GB for ISO uploads
            enable_docs: true,
            trusted_proxies: vec![],
            cors_origins: vec![],
            rate_limit: 1000,
            audit_logging: true,
            branding: BrandingConfig::default(),
        }
    }
}

/// Custom branding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandingConfig {
    /// Product name
    pub product_name: String,
    /// Company name
    pub company_name: String,
    /// Logo URL
    pub logo_url: Option<String>,
    /// Favicon URL
    pub favicon_url: Option<String>,
    /// Primary color
    pub primary_color: String,
    /// Theme (light/dark/auto)
    pub theme: String,
    /// Custom CSS
    pub custom_css: Option<String>,
    /// Support URL
    pub support_url: Option<String>,
    /// Documentation URL
    pub docs_url: Option<String>,
}

impl Default for BrandingConfig {
    fn default() -> Self {
        Self {
            product_name: "NexaOS Virtual Machine Manager".to_string(),
            company_name: "NexaOS Project".to_string(),
            logo_url: None,
            favicon_url: None,
            primary_color: "#1976d2".to_string(),
            theme: "auto".to_string(),
            custom_css: None,
            support_url: Some("https://github.com/nexa-sys/nexa-os/issues".to_string()),
            docs_url: Some("https://docs.nexaos.dev/nvm".to_string()),
        }
    }
}

/// Server statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerStats {
    pub start_time: u64,
    pub total_requests: u64,
    pub active_connections: u32,
    pub active_websockets: u32,
    pub active_sessions: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Background task information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: Uuid,
    pub task_type: TaskType,
    pub description: String,
    pub status: TaskStatus,
    pub progress: f64,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub user: String,
    pub target: String,
    pub error: Option<String>,
}

/// Task type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    VmCreate,
    VmClone,
    VmMigrate,
    VmBackup,
    VmRestore,
    SnapshotCreate,
    SnapshotRevert,
    TemplateCreate,
    IsoUpload,
    StorageSync,
    ClusterJoin,
}

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// WebGUI Server
pub struct WebGuiServer {
    state: Arc<WebGuiState>,
}

impl WebGuiServer {
    /// Create new WebGUI server
    pub fn new(config: WebGuiConfig) -> Self {
        Self {
            state: Arc::new(WebGuiState::new(config)),
        }
    }

    /// Get server state
    pub fn state(&self) -> Arc<WebGuiState> {
        self.state.clone()
    }

    /// Start the server (returns immediately, runs in background)
    #[cfg(feature = "webgui")]
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use axum::{
            routing::{get, post, put, delete},
            Router,
            middleware as axum_mw,
        };
        use tower_http::{
            services::ServeDir,
            compression::CompressionLayer,
            cors::CorsLayer,
        };

        let config = self.state.config.read().clone();
        let state = self.state.clone();

        // Initialize database if enabled
        #[cfg(any(feature = "postgres", feature = "sqlite"))]
        {
            use crate::db::{DatabaseConfig, init_database};
            
            // Use development config (auto-detect backend, SQLite fallback)
            let db_config = DatabaseConfig::development();
            log::info!("Connecting to database (auto-detect mode)...");
            
            match init_database(db_config).await {
                Ok(_) => log::info!("Database initialized successfully"),
                Err(e) => log::warn!("Database init failed (using fallback auth): {}", e),
            }
        }

        // Check if embedded frontend is available
        let has_frontend = super::frontend::has_frontend();
        if has_frontend {
            log::info!("Embedded Vue.js frontend available ({} files)", 
                      super::frontend::list_assets().len());
        } else {
            log::warn!("Frontend not embedded. Build with: cd webui && npm run build");
        }

        // Build router with all routes
        let app = Router::new()
            // API v2 routes
            .nest("/api/v2", Self::api_routes())
            // WebSocket endpoint
            .route("/ws", get(super::websocket::ws_handler))
            // noVNC console
            .route("/novnc/:vmid", get(super::console::novnc_handler))
            // Health check endpoint (no auth required)
            .route("/health", get(Self::health_check))
            // Serve embedded frontend or fallback to static directory
            .fallback(super::frontend::serve_frontend)
            // Apply middleware
            .layer(CompressionLayer::new())
            .with_state(state);

        let addr: SocketAddr = format!("{}:{}", config.bind_address, config.http_port)
            .parse()?;

        log::info!("Starting NVM WebGUI server on http://{}", addr);
        log::info!("Default login: admin / admin123");

        // Start server
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    /// Health check endpoint
    #[cfg(feature = "webgui")]
    async fn health_check() -> impl axum::response::IntoResponse {
        use axum::Json;
        
        #[derive(serde::Serialize)]
        struct Health {
            status: &'static str,
            version: &'static str,
            frontend: bool,
            database: bool,
        }

        let mut database_ok = false;
        
        #[cfg(any(feature = "postgres", feature = "sqlite"))]
        {
            use crate::db::DB;
            if let Some(db) = DB.read().as_ref() {
                database_ok = db.is_connected();
            }
        }

        Json(Health {
            status: "ok",
            version: env!("CARGO_PKG_VERSION"),
            frontend: super::frontend::has_frontend(),
            database: database_ok,
        })
    }

    #[cfg(feature = "webgui")]
    fn api_routes() -> Router<Arc<WebGuiState>> {
        use axum::routing::{get, post, put, delete};

        Router::new()
            // Authentication
            .route("/auth/login", post(super::handlers::auth::login))
            .route("/auth/logout", post(super::handlers::auth::logout))
            .route("/auth/refresh", post(super::handlers::auth::refresh_token))
            .route("/auth/me", get(super::handlers::auth::me))
            // Dashboard
            .route("/dashboard", get(super::handlers::dashboard::overview))
            .route("/dashboard/stats", get(super::handlers::dashboard::stats))
            // VMs
            .route("/vms", get(super::handlers::vm::list))
            .route("/vms", post(super::handlers::vm::create))
            .route("/vms/:id", get(super::handlers::vm::get))
            .route("/vms/:id", put(super::handlers::vm::update))
            .route("/vms/:id", delete(super::handlers::vm::delete))
            .route("/vms/:id/start", post(super::handlers::vm::start))
            .route("/vms/:id/stop", post(super::handlers::vm::stop))
            .route("/vms/:id/restart", post(super::handlers::vm::restart))
            .route("/vms/:id/pause", post(super::handlers::vm::pause))
            .route("/vms/:id/resume", post(super::handlers::vm::resume))
            .route("/vms/:id/snapshot", post(super::handlers::vm::snapshot))
            .route("/vms/:id/clone", post(super::handlers::vm::clone))
            .route("/vms/:id/migrate", post(super::handlers::vm::migrate))
            .route("/vms/:id/console", get(super::handlers::vm::console_ticket))
            .route("/vms/:id/metrics", get(super::handlers::vm::metrics))
            // Templates
            .route("/templates", get(super::handlers::template::list))
            .route("/templates", post(super::handlers::template::create))
            .route("/templates/:id", get(super::handlers::template::get))
            .route("/templates/:id", delete(super::handlers::template::delete))
            .route("/templates/:id/deploy", post(super::handlers::template::deploy))
            // Storage
            .route("/storage/pools", get(super::handlers::storage::list_pools))
            .route("/storage/pools", post(super::handlers::storage::create_pool))
            .route("/storage/volumes", get(super::handlers::storage::list_volumes))
            .route("/storage/volumes", post(super::handlers::storage::create_volume))
            .route("/storage/isos", get(super::handlers::storage::list_isos))
            .route("/storage/upload", post(super::handlers::storage::upload))
            // Network
            .route("/networks", get(super::handlers::network::list))
            .route("/networks", post(super::handlers::network::create))
            .route("/networks/:id", get(super::handlers::network::get))
            .route("/networks/:id", delete(super::handlers::network::delete))
            // Nodes/Cluster
            .route("/nodes", get(super::handlers::cluster::list_nodes))
            .route("/nodes/:id", get(super::handlers::cluster::get_node))
            .route("/nodes/:id/metrics", get(super::handlers::cluster::node_metrics))
            .route("/cluster/status", get(super::handlers::cluster::status))
            .route("/cluster/join", post(super::handlers::cluster::join))
            .route("/cluster/leave", post(super::handlers::cluster::leave))
            // Users & Permissions
            .route("/users", get(super::handlers::users::list))
            .route("/users", post(super::handlers::users::create))
            .route("/users/:id", get(super::handlers::users::get))
            .route("/users/:id", put(super::handlers::users::update))
            .route("/users/:id", delete(super::handlers::users::delete))
            .route("/roles", get(super::handlers::users::list_roles))
            // Tasks
            .route("/tasks", get(super::handlers::tasks::list))
            .route("/tasks/:id", get(super::handlers::tasks::get))
            .route("/tasks/:id/cancel", post(super::handlers::tasks::cancel))
            // Events/Audit
            .route("/events", get(super::handlers::events::list))
            .route("/audit", get(super::handlers::events::audit_log))
            // Backup
            .route("/backup/jobs", get(super::handlers::backup::list_jobs))
            .route("/backup/jobs", post(super::handlers::backup::create_job))
            .route("/backup/schedules", get(super::handlers::backup::list_schedules))
            // System
            .route("/system/info", get(super::handlers::system::info))
            .route("/system/config", get(super::handlers::system::config))
            .route("/system/config", put(super::handlers::system::update_config))
            .route("/system/license", get(super::handlers::system::license))
            .route("/system/license", post(super::handlers::system::activate_license))
            .route("/system/update", post(super::handlers::system::check_updates))
    }

    /// Start without async (for non-tokio contexts)
    pub fn start_blocking(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "webgui")]
        {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(self.start())?;
        }
        Ok(())
    }
}
