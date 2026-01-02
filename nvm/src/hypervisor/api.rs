//! REST API for Hypervisor Management
//!
//! This module provides a REST API framework for enterprise hypervisor management.
//! In a real implementation, this would expose HTTP endpoints for VM management,
//! monitoring, and orchestration.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use super::core::{VmId, VmStatus, VmSpec, HypervisorError, HypervisorResult};

// ============================================================================
// API Server
// ============================================================================

/// API server for hypervisor management
pub struct ApiServer {
    /// Configuration
    config: RwLock<ApiConfig>,
    /// Registered endpoints
    endpoints: RwLock<Vec<Endpoint>>,
    /// API keys
    api_keys: RwLock<HashMap<String, ApiKey>>,
    /// Request handlers
    handlers: RwLock<HashMap<String, Box<dyn RequestHandler>>>,
    /// Statistics
    stats: RwLock<ApiStats>,
}

/// API configuration
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Listen address
    pub listen_addr: String,
    /// Listen port
    pub port: u16,
    /// Enable TLS
    pub tls_enabled: bool,
    /// TLS certificate path
    pub tls_cert: Option<String>,
    /// TLS key path
    pub tls_key: Option<String>,
    /// Enable authentication
    pub auth_enabled: bool,
    /// Rate limit (requests per second)
    pub rate_limit: u32,
    /// Request timeout (seconds)
    pub timeout: u32,
    /// Enable CORS
    pub cors_enabled: bool,
    /// Allowed origins
    pub cors_origins: Vec<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0".to_string(),
            port: 8443,
            tls_enabled: true,
            tls_cert: None,
            tls_key: None,
            auth_enabled: true,
            rate_limit: 100,
            timeout: 30,
            cors_enabled: false,
            cors_origins: Vec::new(),
        }
    }
}

/// API statistics
#[derive(Debug, Clone, Default)]
pub struct ApiStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub auth_failures: u64,
    pub rate_limited: u64,
}

impl ApiServer {
    pub fn new() -> Self {
        let server = Self {
            config: RwLock::new(ApiConfig::default()),
            endpoints: RwLock::new(Vec::new()),
            api_keys: RwLock::new(HashMap::new()),
            handlers: RwLock::new(HashMap::new()),
            stats: RwLock::new(ApiStats::default()),
        };
        
        // Register default endpoints
        server.register_default_endpoints();
        
        server
    }
    
    /// Configure API server
    pub fn configure(&self, config: ApiConfig) {
        *self.config.write().unwrap() = config;
    }
    
    /// Register endpoint
    pub fn register_endpoint(&self, endpoint: Endpoint) {
        self.endpoints.write().unwrap().push(endpoint);
    }
    
    /// Register API key
    pub fn register_api_key(&self, key: ApiKey) {
        self.api_keys.write().unwrap().insert(key.key.clone(), key);
    }
    
    /// Revoke API key
    pub fn revoke_api_key(&self, key: &str) {
        self.api_keys.write().unwrap().remove(key);
    }
    
    /// Handle request
    pub fn handle_request(&self, request: ApiRequest) -> ApiResponse {
        self.stats.write().unwrap().total_requests += 1;
        
        // Find endpoint first to check if it requires auth
        let endpoint = self.find_endpoint(&request.method, &request.path);
        
        if endpoint.is_none() {
            self.stats.write().unwrap().failed_requests += 1;
            return ApiResponse::not_found();
        }
        
        let endpoint = endpoint.unwrap();
        
        // Authenticate only if config requires it AND endpoint has permissions
        if self.config.read().unwrap().auth_enabled && !endpoint.required_permissions.is_empty() {
            if !self.authenticate(&request) {
                self.stats.write().unwrap().auth_failures += 1;
                return ApiResponse::unauthorized();
            }
        }
        
        // Check permissions
        if !self.check_permissions(&request, &endpoint) {
            self.stats.write().unwrap().failed_requests += 1;
            return ApiResponse::forbidden();
        }
        
        // Execute handler
        let response = self.execute_handler(&endpoint, &request);
        
        if response.status_code >= 200 && response.status_code < 300 {
            self.stats.write().unwrap().successful_requests += 1;
        } else {
            self.stats.write().unwrap().failed_requests += 1;
        }
        
        response
    }
    
    fn authenticate(&self, request: &ApiRequest) -> bool {
        if let Some(auth) = &request.authorization {
            if auth.starts_with("Bearer ") {
                let key = &auth[7..];
                return self.api_keys.read().unwrap().contains_key(key);
            }
        }
        false
    }
    
    fn check_permissions(&self, request: &ApiRequest, endpoint: &Endpoint) -> bool {
        if let Some(auth) = &request.authorization {
            if auth.starts_with("Bearer ") {
                let key = &auth[7..];
                if let Some(api_key) = self.api_keys.read().unwrap().get(key) {
                    for required_perm in &endpoint.required_permissions {
                        if !api_key.permissions.contains(required_perm) {
                            return false;
                        }
                    }
                    return true;
                }
            }
        }
        endpoint.required_permissions.is_empty()
    }
    
    fn find_endpoint(&self, method: &HttpMethod, path: &str) -> Option<Endpoint> {
        let endpoints = self.endpoints.read().unwrap();
        
        for endpoint in endpoints.iter() {
            if &endpoint.method == method && endpoint.matches_path(path) {
                return Some(endpoint.clone());
            }
        }
        
        None
    }
    
    fn execute_handler(&self, endpoint: &Endpoint, request: &ApiRequest) -> ApiResponse {
        // Simulated handler execution
        match endpoint.handler_name.as_str() {
            "list_vms" => self.handle_list_vms(request),
            "get_vm" => self.handle_get_vm(request),
            "create_vm" => self.handle_create_vm(request),
            "delete_vm" => self.handle_delete_vm(request),
            "start_vm" => self.handle_start_vm(request),
            "stop_vm" => self.handle_stop_vm(request),
            "get_stats" => self.handle_get_stats(request),
            "health" => self.handle_health(request),
            _ => ApiResponse::not_found(),
        }
    }
    
    fn register_default_endpoints(&self) {
        let endpoints = vec![
            // VM management
            Endpoint::new(HttpMethod::Get, "/api/v1/vms", "list_vms")
                .with_permission("vm:read"),
            Endpoint::new(HttpMethod::Get, "/api/v1/vms/{id}", "get_vm")
                .with_permission("vm:read"),
            Endpoint::new(HttpMethod::Post, "/api/v1/vms", "create_vm")
                .with_permission("vm:write"),
            Endpoint::new(HttpMethod::Delete, "/api/v1/vms/{id}", "delete_vm")
                .with_permission("vm:write"),
            Endpoint::new(HttpMethod::Post, "/api/v1/vms/{id}/start", "start_vm")
                .with_permission("vm:control"),
            Endpoint::new(HttpMethod::Post, "/api/v1/vms/{id}/stop", "stop_vm")
                .with_permission("vm:control"),
            
            // Statistics
            Endpoint::new(HttpMethod::Get, "/api/v1/stats", "get_stats")
                .with_permission("stats:read"),
            
            // Health check (no auth required)
            Endpoint::new(HttpMethod::Get, "/health", "health"),
        ];
        
        let mut eps = self.endpoints.write().unwrap();
        eps.extend(endpoints);
    }
    
    // Handler implementations
    
    fn handle_list_vms(&self, _request: &ApiRequest) -> ApiResponse {
        ApiResponse::ok(r#"{"vms": []}"#.to_string())
    }
    
    fn handle_get_vm(&self, request: &ApiRequest) -> ApiResponse {
        if let Some(id) = request.path_params.get("id") {
            ApiResponse::ok(format!(r#"{{"id": "{}", "status": "running"}}"#, id))
        } else {
            ApiResponse::bad_request("Missing VM ID".to_string())
        }
    }
    
    fn handle_create_vm(&self, request: &ApiRequest) -> ApiResponse {
        if request.body.is_some() {
            ApiResponse::created(r#"{"id": "vm-123", "status": "created"}"#.to_string())
        } else {
            ApiResponse::bad_request("Missing request body".to_string())
        }
    }
    
    fn handle_delete_vm(&self, request: &ApiRequest) -> ApiResponse {
        if request.path_params.contains_key("id") {
            ApiResponse::no_content()
        } else {
            ApiResponse::bad_request("Missing VM ID".to_string())
        }
    }
    
    fn handle_start_vm(&self, request: &ApiRequest) -> ApiResponse {
        if request.path_params.contains_key("id") {
            ApiResponse::ok(r#"{"status": "starting"}"#.to_string())
        } else {
            ApiResponse::bad_request("Missing VM ID".to_string())
        }
    }
    
    fn handle_stop_vm(&self, request: &ApiRequest) -> ApiResponse {
        if request.path_params.contains_key("id") {
            ApiResponse::ok(r#"{"status": "stopping"}"#.to_string())
        } else {
            ApiResponse::bad_request("Missing VM ID".to_string())
        }
    }
    
    fn handle_get_stats(&self, _request: &ApiRequest) -> ApiResponse {
        let stats = self.stats.read().unwrap();
        ApiResponse::ok(format!(
            r#"{{"total_requests": {}, "successful": {}, "failed": {}}}"#,
            stats.total_requests, stats.successful_requests, stats.failed_requests
        ))
    }
    
    fn handle_health(&self, _request: &ApiRequest) -> ApiResponse {
        ApiResponse::ok(r#"{"status": "healthy"}"#.to_string())
    }
    
    /// Get statistics
    pub fn stats(&self) -> ApiStats {
        self.stats.read().unwrap().clone()
    }
}

impl Default for ApiServer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// API Endpoint
// ============================================================================

/// API endpoint definition
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// HTTP method
    pub method: HttpMethod,
    /// Path pattern (supports {param} placeholders)
    pub path: String,
    /// Handler name
    pub handler_name: String,
    /// Required permissions
    pub required_permissions: Vec<String>,
    /// Description
    pub description: String,
}

impl Endpoint {
    pub fn new(method: HttpMethod, path: &str, handler: &str) -> Self {
        Self {
            method,
            path: path.to_string(),
            handler_name: handler.to_string(),
            required_permissions: Vec::new(),
            description: String::new(),
        }
    }
    
    pub fn with_permission(mut self, perm: &str) -> Self {
        self.required_permissions.push(perm.to_string());
        self
    }
    
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }
    
    pub fn matches_path(&self, path: &str) -> bool {
        let pattern_parts: Vec<&str> = self.path.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();
        
        if pattern_parts.len() != path_parts.len() {
            return false;
        }
        
        for (pattern, actual) in pattern_parts.iter().zip(path_parts.iter()) {
            if pattern.starts_with('{') && pattern.ends_with('}') {
                continue; // Parameter placeholder
            }
            if pattern != actual {
                return false;
            }
        }
        
        true
    }
    
    pub fn extract_params(&self, path: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();
        
        let pattern_parts: Vec<&str> = self.path.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();
        
        for (pattern, actual) in pattern_parts.iter().zip(path_parts.iter()) {
            if pattern.starts_with('{') && pattern.ends_with('}') {
                let name = &pattern[1..pattern.len()-1];
                params.insert(name.to_string(), actual.to_string());
            }
        }
        
        params
    }
}

// ============================================================================
// HTTP Types
// ============================================================================

/// HTTP method
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
            Self::Put => write!(f, "PUT"),
            Self::Patch => write!(f, "PATCH"),
            Self::Delete => write!(f, "DELETE"),
            Self::Options => write!(f, "OPTIONS"),
            Self::Head => write!(f, "HEAD"),
        }
    }
}

// ============================================================================
// API Request/Response
// ============================================================================

/// API request
#[derive(Debug, Clone)]
pub struct ApiRequest {
    /// HTTP method
    pub method: HttpMethod,
    /// Request path
    pub path: String,
    /// Query parameters
    pub query_params: HashMap<String, String>,
    /// Path parameters (extracted from URL)
    pub path_params: HashMap<String, String>,
    /// Headers
    pub headers: HashMap<String, String>,
    /// Authorization header
    pub authorization: Option<String>,
    /// Request body
    pub body: Option<String>,
    /// Content type
    pub content_type: Option<String>,
}

impl ApiRequest {
    pub fn new(method: HttpMethod, path: &str) -> Self {
        Self {
            method,
            path: path.to_string(),
            query_params: HashMap::new(),
            path_params: HashMap::new(),
            headers: HashMap::new(),
            authorization: None,
            body: None,
            content_type: None,
        }
    }
    
    pub fn with_auth(mut self, token: &str) -> Self {
        self.authorization = Some(format!("Bearer {}", token));
        self
    }
    
    pub fn with_body(mut self, body: &str) -> Self {
        self.body = Some(body.to_string());
        self.content_type = Some("application/json".to_string());
        self
    }
    
    pub fn with_param(mut self, key: &str, value: &str) -> Self {
        self.path_params.insert(key.to_string(), value.to_string());
        self
    }
}

/// API response
#[derive(Debug, Clone)]
pub struct ApiResponse {
    /// HTTP status code
    pub status_code: u16,
    /// Status message
    pub status_message: String,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body
    pub body: Option<String>,
    /// Content type
    pub content_type: String,
}

impl ApiResponse {
    pub fn new(status_code: u16, message: &str) -> Self {
        Self {
            status_code,
            status_message: message.to_string(),
            headers: HashMap::new(),
            body: None,
            content_type: "application/json".to_string(),
        }
    }
    
    pub fn ok(body: String) -> Self {
        Self {
            status_code: 200,
            status_message: "OK".to_string(),
            headers: HashMap::new(),
            body: Some(body),
            content_type: "application/json".to_string(),
        }
    }
    
    pub fn created(body: String) -> Self {
        Self {
            status_code: 201,
            status_message: "Created".to_string(),
            headers: HashMap::new(),
            body: Some(body),
            content_type: "application/json".to_string(),
        }
    }
    
    pub fn no_content() -> Self {
        Self {
            status_code: 204,
            status_message: "No Content".to_string(),
            headers: HashMap::new(),
            body: None,
            content_type: "application/json".to_string(),
        }
    }
    
    pub fn bad_request(message: String) -> Self {
        Self {
            status_code: 400,
            status_message: "Bad Request".to_string(),
            headers: HashMap::new(),
            body: Some(format!(r#"{{"error": "{}"}}"#, message)),
            content_type: "application/json".to_string(),
        }
    }
    
    pub fn unauthorized() -> Self {
        Self {
            status_code: 401,
            status_message: "Unauthorized".to_string(),
            headers: HashMap::new(),
            body: Some(r#"{"error": "Authentication required"}"#.to_string()),
            content_type: "application/json".to_string(),
        }
    }
    
    pub fn forbidden() -> Self {
        Self {
            status_code: 403,
            status_message: "Forbidden".to_string(),
            headers: HashMap::new(),
            body: Some(r#"{"error": "Permission denied"}"#.to_string()),
            content_type: "application/json".to_string(),
        }
    }
    
    pub fn not_found() -> Self {
        Self {
            status_code: 404,
            status_message: "Not Found".to_string(),
            headers: HashMap::new(),
            body: Some(r#"{"error": "Resource not found"}"#.to_string()),
            content_type: "application/json".to_string(),
        }
    }
    
    pub fn internal_error(message: &str) -> Self {
        Self {
            status_code: 500,
            status_message: "Internal Server Error".to_string(),
            headers: HashMap::new(),
            body: Some(format!(r#"{{"error": "{}"}}"#, message)),
            content_type: "application/json".to_string(),
        }
    }
    
    pub fn is_success(&self) -> bool {
        self.status_code >= 200 && self.status_code < 300
    }
}

// ============================================================================
// API Key
// ============================================================================

/// API key for authentication
#[derive(Debug, Clone)]
pub struct ApiKey {
    /// Key ID
    pub id: String,
    /// API key value
    pub key: String,
    /// Key name/description
    pub name: String,
    /// Permissions
    pub permissions: Vec<String>,
    /// Expiration time
    pub expires_at: Option<Instant>,
    /// Rate limit (requests per second, 0 = use default)
    pub rate_limit: u32,
    /// Enabled
    pub enabled: bool,
}

impl ApiKey {
    pub fn new(id: &str, key: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            key: key.to_string(),
            name: name.to_string(),
            permissions: Vec::new(),
            expires_at: None,
            rate_limit: 0,
            enabled: true,
        }
    }
    
    pub fn with_permission(mut self, perm: &str) -> Self {
        self.permissions.push(perm.to_string());
        self
    }
    
    pub fn with_permissions(mut self, perms: Vec<&str>) -> Self {
        for perm in perms {
            self.permissions.push(perm.to_string());
        }
        self
    }
    
    pub fn is_valid(&self) -> bool {
        if !self.enabled {
            return false;
        }
        
        if let Some(expires) = self.expires_at {
            if Instant::now() > expires {
                return false;
            }
        }
        
        true
    }
}

// ============================================================================
// Request Handler Trait
// ============================================================================

/// Request handler trait
pub trait RequestHandler: Send + Sync {
    fn handle(&self, request: &ApiRequest) -> ApiResponse;
}

// ============================================================================
// OpenAPI Schema Generator
// ============================================================================

/// OpenAPI schema generator
pub struct OpenApiGenerator {
    /// API title
    pub title: String,
    /// API version
    pub version: String,
    /// API description
    pub description: String,
}

impl OpenApiGenerator {
    pub fn new(title: &str, version: &str) -> Self {
        Self {
            title: title.to_string(),
            version: version.to_string(),
            description: String::new(),
        }
    }
    
    /// Generate OpenAPI 3.0 schema
    pub fn generate(&self, endpoints: &[Endpoint]) -> String {
        let mut schema = format!(
            r#"{{
  "openapi": "3.0.0",
  "info": {{
    "title": "{}",
    "version": "{}",
    "description": "{}"
  }},
  "paths": {{"#,
            self.title, self.version, self.description
        );
        
        let mut paths: HashMap<String, Vec<&Endpoint>> = HashMap::new();
        
        for endpoint in endpoints {
            paths.entry(endpoint.path.clone())
                .or_default()
                .push(endpoint);
        }
        
        let path_entries: Vec<String> = paths.iter().map(|(path, eps)| {
            let methods: Vec<String> = eps.iter().map(|ep| {
                let method = match ep.method {
                    HttpMethod::Get => "get",
                    HttpMethod::Post => "post",
                    HttpMethod::Put => "put",
                    HttpMethod::Patch => "patch",
                    HttpMethod::Delete => "delete",
                    HttpMethod::Options => "options",
                    HttpMethod::Head => "head",
                };
                
                format!(
                    r#"    "{}": {{
      "summary": "{}",
      "operationId": "{}",
      "responses": {{
        "200": {{
          "description": "Success"
        }}
      }}
    }}"#,
                    method, ep.description, ep.handler_name
                )
            }).collect();
            
            format!(r#"  "{}": {{
{}
  }}"#, path, methods.join(",\n"))
        }).collect();
        
        schema.push_str(&path_entries.join(",\n"));
        schema.push_str("\n  }\n}");
        
        schema
    }
}

impl Default for OpenApiGenerator {
    fn default() -> Self {
        Self::new("Hypervisor API", "1.0.0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_api_server() {
        let server = ApiServer::new();
        
        // Create API key
        let key = ApiKey::new("key-1", "test-api-key", "Test Key")
            .with_permissions(vec!["vm:read", "vm:write", "vm:control"]);
        
        server.register_api_key(key);
        
        // Test health endpoint (no auth)
        let request = ApiRequest::new(HttpMethod::Get, "/health");
        let response = server.handle_request(request);
        assert!(response.is_success());
    }
    
    #[test]
    fn test_api_authentication() {
        let server = ApiServer::new();
        
        // Create API key
        let key = ApiKey::new("key-1", "test-api-key", "Test Key")
            .with_permission("vm:read");
        
        server.register_api_key(key);
        
        // Test without auth
        let request = ApiRequest::new(HttpMethod::Get, "/api/v1/vms");
        let response = server.handle_request(request);
        assert_eq!(response.status_code, 401);
        
        // Test with auth
        let request = ApiRequest::new(HttpMethod::Get, "/api/v1/vms")
            .with_auth("test-api-key");
        let response = server.handle_request(request);
        assert!(response.is_success());
    }
    
    #[test]
    fn test_endpoint_matching() {
        let endpoint = Endpoint::new(HttpMethod::Get, "/api/v1/vms/{id}", "get_vm");
        
        assert!(endpoint.matches_path("/api/v1/vms/123"));
        assert!(!endpoint.matches_path("/api/v1/vms"));
        assert!(!endpoint.matches_path("/api/v1/vms/123/start"));
        
        let params = endpoint.extract_params("/api/v1/vms/123");
        assert_eq!(params.get("id"), Some(&"123".to_string()));
    }
    
    #[test]
    fn test_api_permissions() {
        let server = ApiServer::new();
        
        // Create API key with limited permissions
        let key = ApiKey::new("key-1", "limited-key", "Limited Key")
            .with_permission("vm:read");
        
        server.register_api_key(key);
        
        // Read should work
        let request = ApiRequest::new(HttpMethod::Get, "/api/v1/vms")
            .with_auth("limited-key");
        let response = server.handle_request(request);
        assert!(response.is_success());
        
        // Write should fail (no vm:write permission)
        let request = ApiRequest::new(HttpMethod::Post, "/api/v1/vms")
            .with_auth("limited-key")
            .with_body(r#"{"name": "test"}"#);
        let response = server.handle_request(request);
        assert_eq!(response.status_code, 403);
    }
    
    #[test]
    fn test_openapi_generator() {
        let generator = OpenApiGenerator::new("Test API", "1.0.0");
        
        let endpoints = vec![
            Endpoint::new(HttpMethod::Get, "/api/vms", "list_vms")
                .with_description("List all VMs"),
            Endpoint::new(HttpMethod::Post, "/api/vms", "create_vm")
                .with_description("Create a new VM"),
        ];
        
        let schema = generator.generate(&endpoints);
        
        assert!(schema.contains("openapi"));
        assert!(schema.contains("Test API"));
        assert!(schema.contains("/api/vms"));
    }
}
