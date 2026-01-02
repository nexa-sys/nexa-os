//! WebGUI Middleware

use std::future::Future;
use std::pin::Pin;

/// Request logging middleware
pub struct RequestLogger;

/// Rate limiting middleware
pub struct RateLimiter {
    pub requests_per_minute: u32,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self { requests_per_minute: 100 }
    }
}

/// CORS configuration
#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub max_age: u32,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".into()],
            allowed_methods: vec!["GET".into(), "POST".into(), "PUT".into(), "DELETE".into()],
            allowed_headers: vec!["Content-Type".into(), "Authorization".into()],
            max_age: 86400,
        }
    }
}
