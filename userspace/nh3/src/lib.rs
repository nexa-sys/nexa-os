//! NexaOS HTTP/3 Library (nh3)
//!
//! A modern, nghttp3 ABI-compatible HTTP/3 library for NexaOS with QUIC backend.
//!
//! ## Features
//! - **Full HTTP/3 protocol support** (RFC 9114)
//! - **nghttp3 C ABI compatibility** for drop-in replacement
//! - **QUIC transport via ntcp2** (dynamic linking)
//! - **QPACK header compression** (RFC 9204)
//! - **Server push support**
//! - **Priority handling** (RFC 9218)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Application Layer                        │
//! │  (nghttp3-compatible C API or Native Rust API)             │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    HTTP/3 Layer (nh3)                       │
//! │  - Stream management                                        │
//! │  - Request/Response handling                                │
//! │  - QPACK encoding/decoding                                 │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    QUIC Layer (ntcp2)                       │
//! │  - Connection management                                    │
//! │  - Flow control & congestion control                       │
//! │  - Loss detection & recovery                               │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage (Rust API)
//!
//! ```rust,ignore
//! use nh3::{Connection, Config, StreamId};
//!
//! // Create configuration
//! let config = Config::default();
//!
//! // Create a client connection
//! let conn = Connection::client(&config)?;
//!
//! // Submit a request
//! let stream_id = conn.submit_request(
//!     &[
//!         (":method", "GET"),
//!         (":scheme", "https"),
//!         (":path", "/"),
//!         (":authority", "example.com"),
//!     ],
//!     None,
//! )?;
//! ```
//!
//! ## Usage (C API - nghttp3 compatible)
//!
//! ```c
//! #include <nghttp3/nghttp3.h>
//!
//! nghttp3_conn *conn;
//! nghttp3_callbacks callbacks;
//! nghttp3_settings settings;
//!
//! nghttp3_settings_default(&settings);
//! nghttp3_conn_client_new(&conn, &callbacks, &settings, NULL, user_data);
//! ```

#![feature(linkage)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

// ============================================================================
// Module Declarations
// ============================================================================

// Core types and constants
pub mod constants;
pub mod error;
pub mod types;

// FFI bindings to ntcp2 (libngtcp2.so)
// Provides ngtcp2-compatible C ABI for QUIC operations
pub mod quic_ffi;

// QPACK header compression (RFC 9204)
pub mod qpack;

// Frame layer
pub mod frame;

// Connection layer
pub mod connection;
pub mod stream;

// Async I/O backend (tokio)
#[cfg(feature = "async-tokio")]
pub mod async_io;

// nghttp3 C ABI compatibility layer
pub mod compat;

// ============================================================================
// Re-exports for convenience
// ============================================================================

pub use constants::*;
pub use error::{Error, ErrorCode, NgError, Result};
pub use frame::{Frame, FrameType};
pub use qpack::{QpackDecoder, QpackEncoder};
pub use connection::{Connection, ConnectionCallbacks, ConnectionState};
pub use stream::{Stream, StreamMap, StreamState, StreamType};
pub use types::*;

// ============================================================================
// C Type Definitions (nghttp3 compatible)
// ============================================================================

pub type c_int = i32;
pub type c_uint = u32;
pub type c_long = i64;
pub type c_ulong = u64;
pub type c_char = i8;
pub type c_uchar = u8;
pub type c_void = core::ffi::c_void;
pub type size_t = usize;
pub type ssize_t = isize;

// ============================================================================
// Version Constants
// ============================================================================

/// Library version string (NUL-terminated for C ABI compatibility)
pub const NH3_VERSION: &str = "1.0.0";
pub const NH3_VERSION_CSTR: &[u8] = b"1.0.0\0";

/// Library version number (0xMMmmpp format)
pub const NH3_VERSION_NUM: u32 = 0x010000; // 1.0.0

/// Protocol version ID
pub const NGHTTP3_ALPN_H3: &[u8] = b"\x02h3";

/// HTTP/3 ALPN protocol string
pub const HTTP3_ALPN: &[u8] = b"h3";

// ============================================================================
// Library Initialization
// ============================================================================

/// Check if an error code is fatal
#[no_mangle]
pub extern "C" fn nghttp3_err_is_fatal(error_code: c_int) -> c_int {
    if error_code < -500 {
        1
    } else {
        0
    }
}

/// Version info structure (nghttp3 compatible)
#[repr(C)]
pub struct Nghttp3Info {
    /// Age of this struct
    pub age: c_int,
    /// Version number
    pub version_num: c_int,
    /// Version string
    pub version_str: *const c_char,
}

// SAFETY: Nghttp3Info only contains constant pointers to static strings
unsafe impl Send for Nghttp3Info {}
unsafe impl Sync for Nghttp3Info {}

/// Get library version info
#[no_mangle]
pub extern "C" fn nghttp3_version(least_version: c_int) -> *const Nghttp3Info {
    static INFO: Nghttp3Info = Nghttp3Info {
        age: 1,
        version_num: NH3_VERSION_NUM as c_int,
        version_str: NH3_VERSION_CSTR.as_ptr() as *const c_char,
    };

    if least_version as u32 > NH3_VERSION_NUM {
        core::ptr::null()
    } else {
        &INFO
    }
}

// ============================================================================
// Error Code Helper
// ============================================================================

/// Convert nghttp3 error code to string
#[no_mangle]
pub extern "C" fn nghttp3_strerror(error_code: c_int) -> *const c_char {
    let msg: &[u8] = match error_code {
        0 => b"NO_ERROR\0",
        -101 => b"ERR_INVALID_ARGUMENT\0",
        -102 => b"ERR_NOBUF\0",
        -103 => b"ERR_INVALID_STATE\0",
        -104 => b"ERR_WOULDBLOCK\0",
        -105 => b"ERR_STREAM_IN_USE\0",
        -106 => b"ERR_PUSH_ID_BLOCKED\0",
        -107 => b"ERR_MALFORMED_HTTP_HEADER\0",
        -108 => b"ERR_REMOVE_HTTP_HEADER\0",
        -109 => b"ERR_MALFORMED_HTTP_MESSAGING\0",
        -110 => b"ERR_QPACK_FATAL\0",
        -111 => b"ERR_QPACK_HEADER_TOO_LARGE\0",
        -112 => b"ERR_IGNORE_STREAM\0",
        -113 => b"ERR_H3_FRAME_UNEXPECTED\0",
        -114 => b"ERR_H3_FRAME_ERROR\0",
        -115 => b"ERR_H3_MISSING_SETTINGS\0",
        -116 => b"ERR_H3_INTERNAL_ERROR\0",
        -117 => b"ERR_H3_CLOSED_CRITICAL_STREAM\0",
        -118 => b"ERR_H3_GENERAL_PROTOCOL_ERROR\0",
        -119 => b"ERR_H3_ID_ERROR\0",
        -120 => b"ERR_H3_SETTINGS_ERROR\0",
        -121 => b"ERR_H3_STREAM_CREATION_ERROR\0",
        -501 => b"ERR_FATAL\0",
        -502 => b"ERR_NOMEM\0",
        -503 => b"ERR_CALLBACK_FAILURE\0",
        _ => b"UNKNOWN_ERROR\0",
    };
    msg.as_ptr() as *const c_char
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let info = unsafe { &*nghttp3_version(0) };
        assert_eq!(info.version_num as u32, NH3_VERSION_NUM);
    }

    #[test]
    fn test_err_is_fatal() {
        assert_eq!(nghttp3_err_is_fatal(0), 0);
        assert_eq!(nghttp3_err_is_fatal(-100), 0);
        assert_eq!(nghttp3_err_is_fatal(-501), 1);
        assert_eq!(nghttp3_err_is_fatal(-600), 1);
    }
}
