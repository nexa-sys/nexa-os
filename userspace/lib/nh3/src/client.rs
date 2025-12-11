//! High-Level HTTP/3 Client API
//!
//! This module provides an easy-to-use Rust API for making HTTP/3 requests.
//! It wraps the lower-level connection and QUIC transport layers.
//!
//! ## Example
//!
//! ```rust,ignore
//! use nh3::client::{Client, Request};
//!
//! let client = Client::new()?;
//! let response = client.get("https://example.com/")?.send()?;
//! println!("Status: {}", response.status());
//! println!("Body: {}", response.text()?);
//! ```

#![allow(dead_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use crate::error::{Error, ErrorCode, Result};
use crate::types::{HeaderField, Settings};
use crate::c_int;

// ============================================================================
// HTTP Method
// ============================================================================

/// HTTP methods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    GET,
    POST,
    PUT,
    DELETE,
    HEAD,
    OPTIONS,
    PATCH,
    CONNECT,
}

impl Method {
    /// Convert to string
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::GET => "GET",
            Method::POST => "POST",
            Method::PUT => "PUT",
            Method::DELETE => "DELETE",
            Method::HEAD => "HEAD",
            Method::OPTIONS => "OPTIONS",
            Method::PATCH => "PATCH",
            Method::CONNECT => "CONNECT",
        }
    }
}

impl Default for Method {
    fn default() -> Self {
        Method::GET
    }
}

// ============================================================================
// Request Builder
// ============================================================================

/// HTTP/3 Request builder
pub struct Request {
    /// HTTP method
    method: Method,
    /// Request URL
    url: String,
    /// Request headers
    headers: Vec<(String, String)>,
    /// Request body (optional)
    body: Option<Vec<u8>>,
}

impl Request {
    /// Create a new GET request
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::GET,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Create a new POST request
    pub fn post(url: impl Into<String>) -> Self {
        Self {
            method: Method::POST,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Create a new request with custom method
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Add a header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Set the request body
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set JSON body with raw bytes
    /// Note: JSON serialization must be done externally in no_std environment
    pub fn json_body(mut self, json_bytes: Vec<u8>) -> Self {
        self.headers.push(("content-type".into(), "application/json".into()));
        self.body = Some(json_bytes);
        self
    }

    /// Get the method
    pub fn method(&self) -> Method {
        self.method
    }

    /// Get the URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Get the headers
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Get the body
    pub fn get_body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }
}

// ============================================================================
// Response
// ============================================================================

/// HTTP/3 Response
pub struct Response {
    /// HTTP status code
    status: u16,
    /// Response headers
    headers: Vec<(String, String)>,
    /// Response body
    body: Vec<u8>,
}

impl Response {
    /// Create a new response
    pub fn new(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        Self { status, headers, body }
    }

    /// Get the status code
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Check if status is success (2xx)
    pub fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// Get a header value
    pub fn header(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
    }

    /// Get all headers
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Get the body as bytes
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }

    /// Get the body as text (UTF-8)
    pub fn text(&self) -> Result<String> {
        String::from_utf8(self.body.clone())
            .map_err(|_| Error::from_code(ErrorCode::MalformedHttpMessaging))
    }

    /// Get the body length
    pub fn content_length(&self) -> usize {
        self.body.len()
    }
}

// ============================================================================
// Client Configuration
// ============================================================================

/// Client configuration
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Maximum idle timeout (milliseconds)
    pub max_idle_timeout: u64,
    /// Enable certificate verification
    pub verify_certs: bool,
    /// Custom ALPN protocols
    pub alpn_protocols: Vec<Vec<u8>>,
    /// Maximum concurrent streams
    pub max_concurrent_streams: u64,
    /// HTTP/3 settings
    pub h3_settings: Settings,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            max_idle_timeout: 30_000, // 30 seconds
            verify_certs: true,
            alpn_protocols: vec![b"h3".to_vec()],
            max_concurrent_streams: 100,
            h3_settings: Settings::default(),
        }
    }
}

impl ClientConfig {
    /// Create a new configuration with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Set max idle timeout
    pub fn max_idle_timeout(mut self, ms: u64) -> Self {
        self.max_idle_timeout = ms;
        self
    }

    /// Disable certificate verification (insecure!)
    pub fn danger_disable_cert_verification(mut self) -> Self {
        self.verify_certs = false;
        self
    }
}

// ============================================================================
// Client
// ============================================================================

/// HTTP/3 Client
///
/// A high-level client for making HTTP/3 requests over QUIC.
/// Note: In no_std environment, connection pooling is not available.
/// Each request creates a new connection.
pub struct Client {
    /// Configuration
    config: ClientConfig,
}

impl Client {
    /// Create a new client with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(ClientConfig::default())
    }

    /// Create a new client with custom configuration
    pub fn with_config(config: ClientConfig) -> Result<Self> {
        Ok(Self {
            config,
        })
    }

    /// Create a GET request builder
    pub fn get(&self, url: impl Into<String>) -> Request {
        Request::get(url)
    }

    /// Create a POST request builder
    pub fn post(&self, url: impl Into<String>) -> Request {
        Request::post(url)
    }

    /// Send a request and get a response
    ///
    /// This is a simplified interface. For production use, you would
    /// implement proper async handling with tokio.
    pub fn send(&self, request: &Request) -> Result<Response> {
        // Parse URL to get host and path
        let (scheme, authority, path) = parse_url(request.url())?;
        
        if scheme != "https" {
            return Err(Error::from_code(ErrorCode::InvalidArgument));
        }

        // Build HTTP/3 headers
        let mut h3_headers = vec![
            HeaderField::new(b":method".to_vec(), request.method().as_str().as_bytes().to_vec()),
            HeaderField::new(b":scheme".to_vec(), scheme.as_bytes().to_vec()),
            HeaderField::new(b":authority".to_vec(), authority.as_bytes().to_vec()),
            HeaderField::new(b":path".to_vec(), path.as_bytes().to_vec()),
        ];

        // Add custom headers
        for (name, value) in request.headers() {
            h3_headers.push(HeaderField::new(
                name.to_lowercase().into_bytes(),
                value.clone().into_bytes(),
            ));
        }

        // Add content-length if body present
        if let Some(body) = request.get_body() {
            h3_headers.push(HeaderField::new(
                b"content-length".to_vec(),
                body.len().to_string().into_bytes(),
            ));
        }

        // This is a placeholder implementation
        // In production, this would:
        // 1. Get or create a QUIC connection to the host
        // 2. Create HTTP/3 control and QPACK streams
        // 3. Submit the request
        // 4. Wait for response
        // 5. Return the response

        // For now, return a placeholder error indicating this needs QUIC transport
        Err(Error::from_code(ErrorCode::InvalidState))
    }

    /// Check if HTTP/3 support is available
    pub fn is_available() -> bool {
        crate::quic_transport::Http3Client::is_available()
    }
}

// ============================================================================
// URL Parsing Helper
// ============================================================================

/// Parse a URL into (scheme, authority, path)
fn parse_url(url: &str) -> Result<(String, String, String)> {
    // Simple URL parsing
    let (scheme, rest) = url.split_once("://")
        .ok_or_else(|| Error::from_code(ErrorCode::InvalidArgument))?;
    
    let (authority, path) = if let Some(idx) = rest.find('/') {
        (&rest[..idx], &rest[idx..])
    } else {
        (rest, "/")
    };

    Ok((scheme.to_string(), authority.to_string(), path.to_string()))
}

// ============================================================================
// C ABI Functions for High-Level Client
// ============================================================================

/// Opaque client handle
#[repr(C)]
pub struct nghttp3_client {
    inner: *mut Client,
}

/// Create a new HTTP/3 client
#[no_mangle]
pub extern "C" fn nghttp3_client_new(pclient: *mut *mut nghttp3_client) -> c_int {
    if pclient.is_null() {
        return -101;
    }

    match Client::new() {
        Ok(client) => {
            let boxed = Box::new(client);
            let handle = Box::new(nghttp3_client {
                inner: Box::into_raw(boxed),
            });
            unsafe {
                *pclient = Box::into_raw(handle);
            }
            0
        }
        Err(_) => -501,
    }
}

/// Delete HTTP/3 client
#[no_mangle]
pub extern "C" fn nghttp3_client_del(client: *mut nghttp3_client) {
    if !client.is_null() {
        unsafe {
            let handle = Box::from_raw(client);
            if !handle.inner.is_null() {
                let _ = Box::from_raw(handle.inner);
            }
        }
    }
}

/// Check if HTTP/3 is available
#[no_mangle]
pub extern "C" fn nghttp3_is_available() -> c_int {
    if Client::is_available() { 1 } else { 0 }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_as_str() {
        assert_eq!(Method::GET.as_str(), "GET");
        assert_eq!(Method::POST.as_str(), "POST");
    }

    #[test]
    fn test_request_builder() {
        let req = Request::get("https://example.com/")
            .header("Accept", "application/json");
        
        assert_eq!(req.method(), Method::GET);
        assert_eq!(req.url(), "https://example.com/");
        assert_eq!(req.headers().len(), 1);
    }

    #[test]
    fn test_response() {
        let resp = Response::new(200, vec![], b"Hello".to_vec());
        assert_eq!(resp.status(), 200);
        assert!(resp.is_success());
        assert_eq!(resp.text().unwrap(), "Hello");
    }

    #[test]
    fn test_parse_url() {
        let (scheme, auth, path) = parse_url("https://example.com/path").unwrap();
        assert_eq!(scheme, "https");
        assert_eq!(auth, "example.com");
        assert_eq!(path, "/path");
    }

    #[test]
    fn test_client_config() {
        let config = ClientConfig::new()
            .max_idle_timeout(60_000)
            .danger_disable_cert_verification();
        
        assert_eq!(config.max_idle_timeout, 60_000);
        assert!(!config.verify_certs);
    }
}
