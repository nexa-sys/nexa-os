//! Async I/O backend for nh3
//!
//! This module provides async/await support for HTTP/3 connections
//! using the tokio runtime.

use crate::connection::{Connection, ConnectionCallbacks, ConnectionState};
use crate::error::{Error, ErrorCode, Result};
use crate::types::{DataProvider, HeaderField, Settings, StreamId};

// ============================================================================
// Async Connection Wrapper
// ============================================================================

/// Async HTTP/3 connection
pub struct AsyncConnection {
    /// Inner connection
    inner: Connection,
}

impl AsyncConnection {
    /// Create a new async client connection
    pub fn client(settings: &Settings) -> Self {
        let callbacks = ConnectionCallbacks::default();
        Self {
            inner: Connection::client(settings, &callbacks, core::ptr::null_mut()),
        }
    }
    
    /// Create a new async server connection
    pub fn server(settings: &Settings) -> Self {
        let callbacks = ConnectionCallbacks::default();
        Self {
            inner: Connection::server(settings, &callbacks, core::ptr::null_mut()),
        }
    }
    
    /// Get connection state
    pub fn state(&self) -> ConnectionState {
        self.inner.state()
    }
    
    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.inner.state() == ConnectionState::Connected
    }
    
    /// Bind control stream
    pub fn bind_control_stream(&mut self, stream_id: StreamId) -> Result<()> {
        self.inner.bind_control_stream(stream_id)
    }
    
    /// Bind QPACK streams
    pub fn bind_qpack_streams(
        &mut self,
        encoder_stream_id: StreamId,
        decoder_stream_id: StreamId,
    ) -> Result<()> {
        self.inner.bind_qpack_streams(encoder_stream_id, decoder_stream_id)
    }
    
    /// Submit a request (async)
    pub async fn submit_request(
        &mut self,
        headers: &[HeaderField],
    ) -> Result<StreamId> {
        self.inner.submit_request(headers, None)
    }
    
    /// Submit a request with body
    pub async fn submit_request_with_body(
        &mut self,
        headers: &[HeaderField],
        body: &[u8],
    ) -> Result<StreamId> {
        let stream_id = self.inner.submit_request(headers, Some(&DataProvider::empty()))?;
        self.inner.submit_data(stream_id, body, true)?;
        Ok(stream_id)
    }
    
    /// Read response data
    pub async fn read_response(&mut self, stream_id: StreamId, buf: &mut [u8]) -> Result<usize> {
        self.inner.read_stream(stream_id, buf)
    }
    
    /// Process incoming data
    pub async fn process_data(
        &mut self,
        stream_id: StreamId,
        data: &[u8],
        fin: bool,
    ) -> Result<()> {
        self.inner.recv_stream_data(stream_id, data, fin)
    }
    
    /// Get next outgoing frame
    pub fn poll_frame(&mut self) -> Option<(StreamId, Vec<u8>)> {
        self.inner.poll_frame()
    }
    
    /// Check if there's pending data to send
    pub fn has_pending_data(&self) -> bool {
        self.inner.has_pending_data()
    }
    
    /// Shutdown the connection
    pub async fn shutdown(&mut self) -> Result<()> {
        self.inner.shutdown()
    }
}

// ============================================================================
// Request Builder
// ============================================================================

/// HTTP/3 request builder
pub struct RequestBuilder {
    method: String,
    scheme: String,
    authority: String,
    path: String,
    headers: Vec<HeaderField>,
    body: Option<Vec<u8>>,
}

impl RequestBuilder {
    /// Create a new request builder for GET
    pub fn get(url: &str) -> Self {
        let (scheme, authority, path) = parse_url(url);
        Self {
            method: "GET".to_string(),
            scheme,
            authority,
            path,
            headers: Vec::new(),
            body: None,
        }
    }
    
    /// Create a new request builder for POST
    pub fn post(url: &str) -> Self {
        let (scheme, authority, path) = parse_url(url);
        Self {
            method: "POST".to_string(),
            scheme,
            authority,
            path,
            headers: Vec::new(),
            body: None,
        }
    }
    
    /// Create a new request builder for PUT
    pub fn put(url: &str) -> Self {
        let (scheme, authority, path) = parse_url(url);
        Self {
            method: "PUT".to_string(),
            scheme,
            authority,
            path,
            headers: Vec::new(),
            body: None,
        }
    }
    
    /// Create a new request builder for DELETE
    pub fn delete(url: &str) -> Self {
        let (scheme, authority, path) = parse_url(url);
        Self {
            method: "DELETE".to_string(),
            scheme,
            authority,
            path,
            headers: Vec::new(),
            body: None,
        }
    }
    
    /// Add a header
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push(HeaderField::new(
            name.to_lowercase().into_bytes(),
            value.as_bytes().to_vec(),
        ));
        self
    }
    
    /// Set the request body
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }
    
    /// Build the headers list
    pub fn build_headers(&self) -> Vec<HeaderField> {
        let mut headers = vec![
            HeaderField::new(b":method".to_vec(), self.method.as_bytes().to_vec()),
            HeaderField::new(b":scheme".to_vec(), self.scheme.as_bytes().to_vec()),
            HeaderField::new(b":authority".to_vec(), self.authority.as_bytes().to_vec()),
            HeaderField::new(b":path".to_vec(), self.path.as_bytes().to_vec()),
        ];
        headers.extend(self.headers.clone());
        headers
    }
    
    /// Get the body
    pub fn get_body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }
}

/// Parse a URL into (scheme, authority, path)
fn parse_url(url: &str) -> (String, String, String) {
    let url = url.trim();
    
    // Default values
    let mut scheme = "https".to_string();
    let mut authority = String::new();
    let mut path = "/".to_string();
    
    let remaining = if let Some(rest) = url.strip_prefix("https://") {
        scheme = "https".to_string();
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        scheme = "http".to_string();
        rest
    } else {
        url
    };
    
    // Split authority and path
    if let Some(slash_pos) = remaining.find('/') {
        authority = remaining[..slash_pos].to_string();
        path = remaining[slash_pos..].to_string();
    } else {
        authority = remaining.to_string();
    }
    
    (scheme, authority, path)
}

// ============================================================================
// Response
// ============================================================================

/// HTTP/3 response
#[derive(Debug, Clone)]
pub struct Response {
    /// Status code
    pub status: u16,
    /// Response headers
    pub headers: Vec<HeaderField>,
    /// Response body
    pub body: Vec<u8>,
}

impl Response {
    /// Create a new response
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
    
    /// Get a header value
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        let name_lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|h| {
                std::str::from_utf8(&h.name)
                    .map(|n| n.to_lowercase() == name_lower)
                    .unwrap_or(false)
            })
            .map(|h| h.value.as_slice())
    }
    
    /// Get body as string
    pub fn text(&self) -> Result<&str> {
        std::str::from_utf8(&self.body)
            .map_err(|_| Error::NgError(ErrorCode::InvalidArgument))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_url() {
        let (scheme, authority, path) = parse_url("https://example.com/path");
        assert_eq!(scheme, "https");
        assert_eq!(authority, "example.com");
        assert_eq!(path, "/path");
        
        let (scheme, authority, path) = parse_url("http://example.com");
        assert_eq!(scheme, "http");
        assert_eq!(authority, "example.com");
        assert_eq!(path, "/");
    }
    
    #[test]
    fn test_request_builder() {
        let req = RequestBuilder::get("https://example.com/api")
            .header("accept", "application/json")
            .header("user-agent", "nh3/1.0");
        
        let headers = req.build_headers();
        assert_eq!(headers.len(), 6); // 4 pseudo-headers + 2 regular headers
        
        // Check pseudo-headers
        assert_eq!(headers[0].name, b":method");
        assert_eq!(headers[0].value, b"GET");
        assert_eq!(headers[1].name, b":scheme");
        assert_eq!(headers[1].value, b"https");
    }
    
    #[test]
    fn test_response() {
        let mut resp = Response::new(200);
        resp.headers.push(HeaderField::new(b"content-type".to_vec(), b"text/plain".to_vec()));
        resp.body = b"Hello, World!".to_vec();
        
        assert_eq!(resp.status, 200);
        assert_eq!(resp.header("content-type"), Some(b"text/plain".as_slice()));
        assert_eq!(resp.text().unwrap(), "Hello, World!");
    }
}
