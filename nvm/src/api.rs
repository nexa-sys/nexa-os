//! REST API Interface
//!
//! Enterprise API layer for VM management including:
//! - RESTful endpoints
//! - Authentication/Authorization
//! - Rate limiting
//! - API versioning

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// API server configuration
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub bind_address: String,
    pub port: u16,
    pub tls_enabled: bool,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
    pub rate_limit_per_minute: u32,
    pub auth_required: bool,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 8080,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            rate_limit_per_minute: 1000,
            auth_required: true,
        }
    }
}

/// API request
#[derive(Debug, Clone)]
pub struct ApiRequest {
    pub method: HttpMethod,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub query_params: HashMap<String, String>,
}

/// HTTP method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

/// API response
#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl ApiResponse {
    pub fn ok(body: Vec<u8>) -> Self {
        Self { status: 200, headers: HashMap::new(), body }
    }
    
    pub fn created(body: Vec<u8>) -> Self {
        Self { status: 201, headers: HashMap::new(), body }
    }
    
    pub fn no_content() -> Self {
        Self { status: 204, headers: HashMap::new(), body: Vec::new() }
    }
    
    pub fn bad_request(msg: &str) -> Self {
        Self { status: 400, headers: HashMap::new(), body: msg.as_bytes().to_vec() }
    }
    
    pub fn unauthorized() -> Self {
        Self { status: 401, headers: HashMap::new(), body: b"Unauthorized".to_vec() }
    }
    
    pub fn forbidden() -> Self {
        Self { status: 403, headers: HashMap::new(), body: b"Forbidden".to_vec() }
    }
    
    pub fn not_found() -> Self {
        Self { status: 404, headers: HashMap::new(), body: b"Not Found".to_vec() }
    }
    
    pub fn internal_error(msg: &str) -> Self {
        Self { status: 500, headers: HashMap::new(), body: msg.as_bytes().to_vec() }
    }
}

/// API endpoint handler
pub type EndpointHandler = Box<dyn Fn(&ApiRequest) -> ApiResponse + Send + Sync>;

/// API router
pub struct ApiRouter {
    routes: RwLock<HashMap<(HttpMethod, String), Arc<EndpointHandler>>>,
}

impl ApiRouter {
    pub fn new() -> Self {
        Self { routes: RwLock::new(HashMap::new()) }
    }
    
    pub fn add_route(&self, method: HttpMethod, path: &str, handler: EndpointHandler) {
        self.routes.write().unwrap()
            .insert((method, path.to_string()), Arc::new(handler));
    }
    
    pub fn handle(&self, request: &ApiRequest) -> ApiResponse {
        let routes = self.routes.read().unwrap();
        if let Some(handler) = routes.get(&(request.method, request.path.clone())) {
            handler(request)
        } else {
            ApiResponse::not_found()
        }
    }
}

impl Default for ApiRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// API token
#[derive(Debug, Clone)]
pub struct ApiToken {
    pub token: String,
    pub user_id: String,
    pub permissions: Vec<String>,
    pub expires_at: u64,
}

/// Rate limiter
pub struct RateLimiter {
    limits: RwLock<HashMap<String, (u32, u64)>>,
    max_per_minute: u32,
}

impl RateLimiter {
    pub fn new(max_per_minute: u32) -> Self {
        Self {
            limits: RwLock::new(HashMap::new()),
            max_per_minute,
        }
    }
    
    pub fn check(&self, client_id: &str) -> bool {
        let mut limits = self.limits.write().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let (count, window_start) = limits.entry(client_id.to_string())
            .or_insert((0, now));
        
        if now - *window_start >= 60 {
            *count = 1;
            *window_start = now;
            true
        } else if *count < self.max_per_minute {
            *count += 1;
            true
        } else {
            false
        }
    }
}
