/// HTTP request building utilities
use crate::args::{Args, HttpMethod};
use crate::url::ParsedUrl;

/// HTTP/1.1 request builder
pub struct Http1RequestBuilder {
    method: HttpMethod,
    path: String,
    host: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    compressed: bool,
}

impl Http1RequestBuilder {
    /// Create a new request builder
    pub fn new(method: HttpMethod, url: &ParsedUrl) -> Self {
        Self {
            method,
            path: url.path.clone(),
            host: url.authority(),
            headers: Vec::new(),
            body: None,
            compressed: false,
        }
    }

    /// Create a request builder from Args and ParsedUrl
    pub fn from_args(args: &Args, url: &ParsedUrl) -> Self {
        let mut builder = Self::new(args.method, url);

        // Enable compression if requested
        builder.compressed = args.compressed;

        // Add custom headers
        for (key, value) in &args.headers {
            builder = builder.header(key, value);
        }

        // Add body if present
        if let Some(ref data) = args.data {
            builder = builder.body(data.clone());
        }

        builder
    }

    /// Add a header
    pub fn header(mut self, key: &str, value: &str) -> Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    /// Set the request body
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    /// Build the HTTP/1.1 request string
    pub fn build(self) -> Vec<u8> {
        let mut request = String::new();

        // Request line
        request.push_str(self.method.as_str());
        request.push(' ');
        request.push_str(&self.path);
        request.push_str(" HTTP/1.1\r\n");

        // Host header (required for HTTP/1.1)
        request.push_str("Host: ");
        request.push_str(&self.host);
        request.push_str("\r\n");

        // Custom headers
        let mut has_user_agent = false;
        let mut has_accept = false;
        let mut has_content_type = false;
        let mut has_accept_encoding = false;

        for (key, value) in &self.headers {
            request.push_str(key);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");

            let key_lower = key.to_lowercase();
            if key_lower == "user-agent" {
                has_user_agent = true;
            } else if key_lower == "accept" {
                has_accept = true;
            } else if key_lower == "content-type" {
                has_content_type = true;
            } else if key_lower == "accept-encoding" {
                has_accept_encoding = true;
            }
        }

        // Add default headers if not provided
        if !has_user_agent {
            request.push_str("User-Agent: nurl/1.0\r\n");
        }
        if !has_accept {
            request.push_str("Accept: */*\r\n");
        }

        // Add Accept-Encoding for compression if requested
        if self.compressed && !has_accept_encoding {
            request.push_str("Accept-Encoding: gzip, deflate\r\n");
        }

        // Content-Length if we have a body
        if let Some(ref body) = self.body {
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
            if !has_content_type {
                request.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
            }
        }

        // Connection: close for simplicity
        request.push_str("Connection: close\r\n");

        // End of headers
        request.push_str("\r\n");

        // Combine headers and body
        let mut result = request.into_bytes();
        if let Some(body) = self.body {
            result.extend_from_slice(&body);
        }

        result
    }
}

/// HTTP/2 pseudo-headers and frame building (placeholder)
#[cfg(feature = "http2")]
pub mod http2 {
    use super::*;

    /// HTTP/2 request builder (placeholder for future implementation)
    pub struct Http2RequestBuilder {
        // TODO: Implement HTTP/2 frame building
        // - HEADERS frame with HPACK compression
        // - DATA frames for body
        // - Stream ID management
    }
}

/// HTTP/3 QPACK headers and frame building (placeholder)
#[cfg(feature = "http3")]
pub mod http3 {
    use super::*;

    /// HTTP/3 request builder (placeholder for future implementation)
    pub struct Http3RequestBuilder {
        // TODO: Implement HTTP/3 frame building
        // - QPACK header compression
        // - QUIC stream management
        // - Unidirectional/bidirectional streams
    }
}
