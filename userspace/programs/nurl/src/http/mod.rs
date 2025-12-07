/// HTTP protocol abstraction layer
/// 
/// This module provides a unified interface for different HTTP protocol versions:
/// - HTTP/1.1 (implemented)
/// - HTTP/2 (planned)
/// - HTTP/3 (planned)

pub mod http1;
pub mod request;
pub mod response;

use crate::args::{Args, HttpVersion};
use crate::url::ParsedUrl;

/// Result type for HTTP operations
pub type HttpResult<T> = Result<T, HttpError>;

/// HTTP error types
#[derive(Debug)]
pub enum HttpError {
    /// Connection failed
    ConnectionFailed(String),
    /// TLS/SSL error
    TlsError(String),
    /// Request send failed
    SendFailed(String),
    /// Response read failed
    ReceiveFailed(String),
    /// Invalid response
    InvalidResponse(String),
    /// Protocol error
    ProtocolError(String),
    /// Timeout
    Timeout,
    /// Feature not supported
    NotSupported(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            HttpError::TlsError(msg) => write!(f, "TLS error: {}", msg),
            HttpError::SendFailed(msg) => write!(f, "Send failed: {}", msg),
            HttpError::ReceiveFailed(msg) => write!(f, "Receive failed: {}", msg),
            HttpError::InvalidResponse(msg) => write!(f, "Invalid response: {}", msg),
            HttpError::ProtocolError(msg) => write!(f, "Protocol error: {}", msg),
            HttpError::Timeout => write!(f, "Request timeout"),
            HttpError::NotSupported(msg) => write!(f, "Not supported: {}", msg),
        }
    }
}

/// HTTP response from any protocol version
#[derive(Debug)]
pub struct HttpResponse {
    /// HTTP status code
    pub status_code: u16,
    /// HTTP status reason phrase
    pub reason: String,
    /// Response headers
    pub headers: Vec<(String, String)>,
    /// Response body
    pub body: Vec<u8>,
    /// Protocol version used
    pub version: String,
}

impl HttpResponse {
    /// Get a header value by name (case-insensitive)
    pub fn get_header(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
    }

    /// Get the status line
    pub fn status_line(&self) -> String {
        format!("{} {} {}", self.version, self.status_code, self.reason)
    }

    /// Check if the response indicates success (2xx)
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }
}

/// Trait for HTTP client implementations
pub trait HttpClient {
    /// Perform an HTTP request
    fn request(&mut self, args: &Args, url: &ParsedUrl) -> HttpResult<HttpResponse>;
}

/// Perform an HTTP request using the appropriate protocol version
pub fn perform_request(args: &Args, url: &ParsedUrl) -> HttpResult<HttpResponse> {
    match args.http_version {
        HttpVersion::Http1 => {
            let mut client = http1::Http1Client::new(args.verbose, args.insecure)?;
            client.request(args, url)
        }
        HttpVersion::Http2 => {
            // TODO: Implement HTTP/2 support
            // For now, fall back to HTTP/1.1
            if args.verbose {
                eprintln!("* HTTP/2 not yet implemented, falling back to HTTP/1.1");
            }
            let mut client = http1::Http1Client::new(args.verbose, args.insecure)?;
            client.request(args, url)
        }
        HttpVersion::Http3 => {
            // TODO: Implement HTTP/3 support
            // For now, fall back to HTTP/1.1
            if args.verbose {
                eprintln!("* HTTP/3 not yet implemented, falling back to HTTP/1.1");
            }
            let mut client = http1::Http1Client::new(args.verbose, args.insecure)?;
            client.request(args, url)
        }
    }
}
