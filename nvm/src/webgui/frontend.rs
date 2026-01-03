//! Frontend Assets - Embedded Vue.js Application
//!
//! Uses rust-embed to embed the built Vue.js frontend into the binary.
//! The frontend is served as static files with SPA fallback support.

#[cfg(feature = "webgui")]
use rust_embed::RustEmbed;

#[cfg(feature = "webgui")]
use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode, Uri},
    response::IntoResponse,
};

/// Embedded frontend assets from the Vue.js build
/// 
/// The assets are embedded at compile time from `webui/dist/`.
/// In development, you can also serve from filesystem by setting
/// `NVM_DEV_ASSETS_PATH` environment variable.
#[cfg(feature = "webgui")]
#[derive(RustEmbed)]
#[folder = "webui/dist"]
#[prefix = ""]
pub struct FrontendAssets;

/// Serve embedded frontend file
#[cfg(feature = "webgui")]
pub async fn serve_frontend(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    
    // Try to serve the exact path first
    if let Some(content) = FrontendAssets::get(path) {
        return serve_file(path, &content.data);
    }
    
    // For SPA: if path doesn't exist and doesn't look like a file, serve index.html
    if !path.contains('.') || path.is_empty() {
        if let Some(content) = FrontendAssets::get("index.html") {
            return serve_file("index.html", &content.data);
        }
    }
    
    // 404 for actual missing files
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

/// Serve a file with appropriate content type
#[cfg(feature = "webgui")]
fn serve_file(path: &str, data: &[u8]) -> Response<Body> {
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime);
    
    // Add cache headers based on file type
    if path.contains("/assets/") || path.ends_with(".js") || path.ends_with(".css") {
        // Long cache for hashed assets
        response = response.header(header::CACHE_CONTROL, "public, max-age=31536000, immutable");
    } else if path == "index.html" {
        // No cache for index.html (SPA entry point)
        response = response.header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate");
    } else {
        // Short cache for other files
        response = response.header(header::CACHE_CONTROL, "public, max-age=3600");
    }
    
    response.body(Body::from(data.to_vec())).unwrap()
}

/// Handler for the root path
#[cfg(feature = "webgui")]
pub async fn serve_index() -> impl IntoResponse {
    if let Some(content) = FrontendAssets::get("index.html") {
        serve_file("index.html", &content.data)
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Frontend not built. Run 'npm run build' in webui/"))
            .unwrap()
    }
}

/// Check if frontend assets are available
#[cfg(feature = "webgui")]
pub fn has_frontend() -> bool {
    FrontendAssets::get("index.html").is_some()
}

/// Get list of all embedded files (for debugging)
#[cfg(feature = "webgui")]
pub fn list_assets() -> Vec<String> {
    FrontendAssets::iter().map(|s| s.to_string()).collect()
}

/// Frontend build info
#[derive(Debug, serde::Serialize)]
pub struct FrontendInfo {
    pub available: bool,
    pub files_count: usize,
    pub index_size: usize,
}

#[cfg(feature = "webgui")]
pub fn get_frontend_info() -> FrontendInfo {
    let available = has_frontend();
    let files_count = FrontendAssets::iter().count();
    let index_size = FrontendAssets::get("index.html")
        .map(|f| f.data.len())
        .unwrap_or(0);
    
    FrontendInfo {
        available,
        files_count,
        index_size,
    }
}

#[cfg(not(feature = "webgui"))]
pub fn get_frontend_info() -> FrontendInfo {
    FrontendInfo {
        available: false,
        files_count: 0,
        index_size: 0,
    }
}

// ============================================================================
// Development Mode - Serve from filesystem
// ============================================================================

/// Serve frontend from filesystem (development mode)
#[cfg(feature = "webgui")]
pub async fn serve_dev_frontend(uri: Uri, dev_path: &std::path::Path) -> impl IntoResponse {
    use tokio::fs;
    
    let path = uri.path().trim_start_matches('/');
    let file_path = dev_path.join(if path.is_empty() { "index.html" } else { path });
    
    // Try exact path
    if file_path.exists() && file_path.is_file() {
        if let Ok(content) = fs::read(&file_path).await {
            return serve_file(path, &content);
        }
    }
    
    // SPA fallback
    if !path.contains('.') {
        let index_path = dev_path.join("index.html");
        if let Ok(content) = fs::read(&index_path).await {
            return serve_file("index.html", &content);
        }
    }
    
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}
