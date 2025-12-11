/// HTTP/3 client implementation using dynamic linking to libnghttp3.so
///
/// This module is a placeholder for future HTTP/3 support.
/// HTTP/3 requires QUIC transport which is currently not implemented.
///
/// When implemented, this module will:
/// - Use the nghttp3 C ABI via dynamic linking
/// - Use QUIC (UDP-based) transport via ntcp2/nquic
/// - Provide HTTP/3 client functionality

#![allow(dead_code)]

use super::{HttpError, HttpResult};
use crate::args::Args;
use crate::url::ParsedUrl;

/// HTTP/3 is not yet implemented
pub const HTTP3_NOT_IMPLEMENTED: &str = "HTTP/3 support is not yet implemented. \
    HTTP/3 requires QUIC transport which is still in development.";

/// HTTP/3 client using dynamic linking to libnghttp3.so
/// 
/// Currently a placeholder - HTTP/3 requires QUIC transport.
pub struct Http3Client {
    _verbose: bool,
    _insecure: bool,
}

impl Http3Client {
    /// Create a new HTTP/3 client
    /// 
    /// Currently always returns NotSupported as HTTP/3 is not implemented.
    pub fn new(_verbose: bool, _insecure: bool) -> HttpResult<Self> {
        Err(HttpError::NotSupported(HTTP3_NOT_IMPLEMENTED.to_string()))
    }

    /// Execute an HTTP/3 request
    /// 
    /// Currently always returns NotSupported as HTTP/3 is not implemented.
    pub fn execute(&self, _args: &Args, _url: &ParsedUrl) -> HttpResult<super::HttpResponse> {
        Err(HttpError::NotSupported(HTTP3_NOT_IMPLEMENTED.to_string()))
    }
}

/// Check if HTTP/3 support is available
/// 
/// Currently always returns false.
pub fn is_available() -> bool {
    false
}

/// Get HTTP/3 library version string
/// 
/// Currently returns None as HTTP/3 is not implemented.
pub fn get_version() -> Option<&'static str> {
    None
}
