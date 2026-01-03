//! WebGUI HTTP Route Handlers
//!
//! API endpoint implementations for the web interface

pub mod auth;
pub mod dashboard;
pub mod vm;
pub mod template;
pub mod storage;
pub mod network;
pub mod cluster;
pub mod users;
pub mod tasks;
pub mod events;
pub mod backup;
pub mod system;

use std::sync::Arc;
use serde::{Deserialize, Serialize};

/// Standard API response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ApiError>,
    pub meta: Option<ResponseMeta>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: None,
        }
    }

    pub fn error(code: u32, message: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(ApiError {
                code,
                message: message.to_string(),
                details: None,
            }),
            meta: None,
        }
    }

    /// Create error from ApiError
    pub fn from_error(err: ApiError) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(err),
            meta: None,
        }
    }

    pub fn with_meta(mut self, meta: ResponseMeta) -> Self {
        self.meta = Some(meta);
        self
    }
}

/// API error info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: u32,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    /// Create a new error
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    /// Create a 400 Bad Request error
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, message)
    }

    /// Create a 401 Unauthorized error
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(401, message)
    }

    /// Create a 403 Forbidden error
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(403, message)
    }

    /// Create a 404 Not Found error
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(404, message)
    }

    /// Create a 500 Internal Server Error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(500, message)
    }

    /// Create a 501 Not Implemented error
    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::new(501, message)
    }

    /// Add details to the error
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// Response metadata (pagination, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMeta {
    pub page: u32,
    pub per_page: u32,
    pub total: u64,
    pub total_pages: u32,
}

/// Pagination parameters
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

fn default_page() -> u32 { 1 }
fn default_per_page() -> u32 { 50 }

/// Default index HTML for WebGUI
const DEFAULT_INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NVM Enterprise - Virtual Machine Management</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; background: #1a1a2e; color: #eee; }
        .container { max-width: 1200px; margin: 0 auto; }
        h1 { color: #4fc3f7; }
        .status { padding: 20px; background: #16213e; border-radius: 8px; margin: 20px 0; }
        .api-link { color: #4fc3f7; text-decoration: none; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🖥️ NVM Enterprise Platform</h1>
        <div class="status">
            <h2>System Status: Online</h2>
            <p>NexaOS Virtual Machine Management Platform v2.0</p>
            <p>API Endpoint: <a class="api-link" href="/api/v2">/api/v2</a></p>
        </div>
    </div>
</body>
</html>"#;

/// Index handler - serve main SPA
#[cfg(feature = "webgui")]
pub async fn index_handler() -> impl axum::response::IntoResponse {
    axum::response::Html(DEFAULT_INDEX_HTML)
}

// Placeholder for non-webgui builds
#[cfg(not(feature = "webgui"))]
pub async fn index_handler() -> &'static str {
    "WebGUI feature not enabled"
}
