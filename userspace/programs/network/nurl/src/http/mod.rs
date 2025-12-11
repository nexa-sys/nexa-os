/// HTTP protocol abstraction layer
///
/// This module provides a unified interface for different HTTP protocol versions:
/// - HTTP/1.1 (implemented)
/// - HTTP/2 (implemented via nh2 - static or dynamic linking)
/// - HTTP/3 (planned)
pub mod http1;
pub mod request;
pub mod response;

// HTTP/2 module selection based on features:
// - http2: static linking with nh2 crate (Rust API)
// - http2-dynamic: dynamic linking with libnh2.so via FFI (C ABI)

#[cfg(feature = "http2")]
pub mod http2;

#[cfg(feature = "http2-dynamic")]
#[path = "http2_dynamic.rs"]
pub mod http2_dynamic;

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
    /// Decompression error
    DecompressionError(String),
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
            HttpError::DecompressionError(msg) => write!(f, "Decompression error: {}", msg),
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

    /// Decompress the response body if Content-Encoding indicates compression
    #[cfg(feature = "compression")]
    pub fn decompress_body(&mut self) -> HttpResult<()> {
        let encoding = match self.get_header("content-encoding") {
            Some(enc) => enc.to_lowercase(),
            None => return Ok(()), // No compression
        };

        let decompressed = match encoding.as_str() {
            "gzip" => nzip::gzip::gzip_decompress(&self.body)
                .map_err(|e| HttpError::DecompressionError(format!("gzip: {:?}", e)))?,
            "deflate" => {
                // Try zlib format first, fall back to raw deflate
                let mut decompressor = nzip::zlib_format::ZlibDecompressor::new();
                match decompressor.decompress(&self.body) {
                    Ok((data, _)) => data,
                    Err(_) => {
                        // Try raw deflate
                        let mut inflater = nzip::inflate::Inflater::new(15);
                        inflater
                            .decompress(&self.body)
                            .map(|(data, _)| data)
                            .map_err(|e| {
                                HttpError::DecompressionError(format!("deflate: {:?}", e))
                            })?
                    }
                }
            }
            "identity" | "" => return Ok(()), // No compression
            other => {
                return Err(HttpError::DecompressionError(format!(
                    "Unsupported encoding: {}",
                    other
                )));
            }
        };

        self.body = decompressed;

        // Remove Content-Encoding header since we've decompressed
        self.headers
            .retain(|(k, _)| !k.eq_ignore_ascii_case("content-encoding"));

        Ok(())
    }

    /// Decompress the response body (no-op if compression feature is disabled)
    #[cfg(not(feature = "compression"))]
    pub fn decompress_body(&mut self) -> HttpResult<()> {
        if self.get_header("content-encoding").is_some() {
            return Err(HttpError::NotSupported(
                "Compression support not compiled in (enable 'compression' feature)".to_string(),
            ));
        }
        Ok(())
    }
}

/// Trait for HTTP client implementations
pub trait HttpClient {
    /// Perform an HTTP request
    fn request(&mut self, args: &Args, url: &ParsedUrl) -> HttpResult<HttpResponse>;
}

/// Perform an HTTP request using the appropriate protocol version
pub fn perform_request(args: &Args, url: &ParsedUrl) -> HttpResult<HttpResponse> {
    let mut response = match args.http_version {
        HttpVersion::Http1 => {
            let mut client = http1::Http1Client::new(args.verbose, args.insecure)?;
            client.request(args, url)?
        }
        HttpVersion::Http2 => {
            // Prefer dynamic linking (http2-dynamic) over static linking (http2)
            #[cfg(feature = "http2-dynamic")]
            {
                if args.verbose {
                    eprintln!("* Using HTTP/2 via libnh2.so (dynamic linking)");
                }
                let mut client = http2_dynamic::Http2Client::new(args.verbose, args.insecure)?;
                match client.request(args, url) {
                    Ok(resp) => resp,
                    Err(e) => {
                        if args.verbose {
                            eprintln!("* HTTP/2 (dynamic) failed: {}, falling back to HTTP/1.1", e);
                        }
                        let mut client = http1::Http1Client::new(args.verbose, args.insecure)?;
                        client.request(args, url)?
                    }
                }
            }
            #[cfg(all(feature = "http2", not(feature = "http2-dynamic")))]
            {
                if args.verbose {
                    eprintln!("* Using HTTP/2 via nh2 library (static linking)");
                }
                let mut client = http2::Http2Client::new(args.verbose, args.insecure)?;
                match client.request(args, url) {
                    Ok(resp) => resp,
                    Err(e) => {
                        if args.verbose {
                            eprintln!("* HTTP/2 failed: {}, falling back to HTTP/1.1", e);
                        }
                        let mut client = http1::Http1Client::new(args.verbose, args.insecure)?;
                        client.request(args, url)?
                    }
                }
            }
            #[cfg(not(any(feature = "http2", feature = "http2-dynamic")))]
            {
                if args.verbose {
                    eprintln!("* HTTP/2 not compiled in, using HTTP/1.1");
                }
                let mut client = http1::Http1Client::new(args.verbose, args.insecure)?;
                client.request(args, url)?
            }
        }
        HttpVersion::Http3 => {
            // TODO: Implement HTTP/3 support
            // For now, fall back to HTTP/1.1
            if args.verbose {
                eprintln!("* HTTP/3 not yet implemented, falling back to HTTP/1.1");
            }
            let mut client = http1::Http1Client::new(args.verbose, args.insecure)?;
            client.request(args, url)?
        }
    };

    // Decompress response body if compressed
    if args.compressed {
        if let Some(encoding) = response.get_header("content-encoding") {
            if args.verbose {
                eprintln!("* Decompressing response (Content-Encoding: {})", encoding);
            }
        }
        response.decompress_body()?;
    }

    Ok(response)
}
