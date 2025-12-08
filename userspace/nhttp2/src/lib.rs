//! NexaOS HTTP/2 Library (nhttp2)
//!
//! A modern, nghttp2 ABI-compatible HTTP/2 library for NexaOS with tokio async backend.
//!
//! ## Features
//! - **Full HTTP/2 protocol support** (RFC 7540, RFC 9113)
//! - **nghttp2 C ABI compatibility** for drop-in replacement
//! - **Tokio async backend** for high-performance I/O
//! - **HPACK header compression** (RFC 7541)
//! - **Server push support**
//! - **Flow control management**
//! - **Stream priority and dependency**
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Application Layer                        │
//! │  (nghttp2-compatible C API or Native Rust API)             │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    Session Layer                            │
//! │  - Stream management                                        │
//! │  - Flow control                                             │
//! │  - Priority handling                                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    Frame Layer                              │
//! │  - Frame serialization/deserialization                     │
//! │  - HPACK encoding/decoding                                 │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    Transport Layer                          │
//! │  - Tokio async I/O                                         │
//! │  - TLS integration (via nssl)                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage (Rust API)
//!
//! ```rust,ignore
//! use nhttp2::{Session, SessionBuilder, StreamId};
//!
//! // Create a client session
//! let session = SessionBuilder::new()
//!     .client()
//!     .build()?;
//!
//! // Submit a request
//! let stream_id = session.submit_request(
//!     &[
//!         (":method", "GET"),
//!         (":scheme", "https"),
//!         (":path", "/"),
//!         (":authority", "example.com"),
//!     ],
//!     None,
//! )?;
//!
//! // Process with tokio
//! session.send().await?;
//! session.recv().await?;
//! ```
//!
//! ## Usage (C API - nghttp2 compatible)
//!
//! ```c
//! #include <nghttp2/nghttp2.h>
//!
//! nghttp2_session *session;
//! nghttp2_session_callbacks *callbacks;
//!
//! nghttp2_session_callbacks_new(&callbacks);
//! // Set callbacks...
//! nghttp2_session_client_new(&session, callbacks, user_data);
//!
//! // Submit request
//! nghttp2_submit_request(session, NULL, nva, nvlen, NULL, NULL);
//!
//! // Send/receive data
//! nghttp2_session_send(session);
//! nghttp2_session_recv(session);
//! ```

#![feature(linkage)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

// ============================================================================
// Module Declarations
// ============================================================================

// Core types and constants
pub mod types;
pub mod error;
pub mod constants;

// Frame layer
pub mod frame;
pub mod hpack;

// Session layer
pub mod session;
pub mod stream;
pub mod flow_control;
pub mod priority;

// Async I/O backend (tokio)
#[cfg(feature = "async-tokio")]
pub mod async_io;

// nghttp2 C ABI compatibility layer
pub mod compat;

// ============================================================================
// Re-exports for convenience
// ============================================================================

pub use types::*;
pub use error::{Error, Result};
pub use constants::*;
pub use session::{Session, SessionBuilder, SessionCallbacks};
pub use stream::{Stream, StreamState, StreamMap};
pub use frame::{Frame, FrameType, FrameFlags, FrameHeader};
pub use hpack::{Hpack, HpackEncoder, HpackDecoder, HeaderField};

#[cfg(feature = "async-tokio")]
pub use async_io::{AsyncSession, Connection};

// ============================================================================
// C Type Definitions (nghttp2 compatible)
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

/// Library version string
pub const NHTTP2_VERSION: &str = "1.58.0";

/// Library version number (0xMMmmpp format)
pub const NHTTP2_VERSION_NUM: u32 = 0x013A00; // 1.58.0

/// Protocol version ID
pub const NGHTTP2_PROTO_VERSION_ID: &[u8] = b"h2";
pub const NGHTTP2_PROTO_VERSION_ID_LEN: usize = 2;

/// ALPN protocol ID for HTTP/2 over TLS
pub const NGHTTP2_PROTO_ALPN: &[u8] = b"\x02h2";
pub const NGHTTP2_PROTO_ALPN_LEN: usize = 3;

/// HTTP/2 connection preface
pub const NGHTTP2_CLIENT_MAGIC: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
pub const NGHTTP2_CLIENT_MAGIC_LEN: usize = 24;

// ============================================================================
// Library Initialization
// ============================================================================

/// Initialize the library (nghttp2 compatible, but no-op for us)
/// 
/// # Safety
/// This function is safe to call from C code.
#[no_mangle]
pub extern "C" fn nghttp2_is_fatal(error_code: c_int) -> c_int {
    if error_code < 0 {
        1
    } else {
        0
    }
}

/// Get library version info
#[no_mangle]
pub extern "C" fn nghttp2_version(least_version: c_int) -> *const NgHttp2Info {
    static INFO: NgHttp2Info = NgHttp2Info {
        age: 1,
        version_num: NHTTP2_VERSION_NUM as c_int,
        version_str: NHTTP2_VERSION.as_ptr() as *const c_char,
        proto_str: NGHTTP2_PROTO_VERSION_ID.as_ptr() as *const c_char,
    };
    
    if least_version as u32 > NHTTP2_VERSION_NUM {
        core::ptr::null()
    } else {
        &INFO
    }
}

/// Version info structure (nghttp2 compatible)
#[repr(C)]
pub struct NgHttp2Info {
    /// Age of this struct (always 1)
    pub age: c_int,
    /// Library version number
    pub version_num: c_int,
    /// Library version string
    pub version_str: *const c_char,
    /// HTTP/2 protocol version string
    pub proto_str: *const c_char,
}

// SAFETY: NgHttp2Info only contains constant pointers to static strings
unsafe impl Sync for NgHttp2Info {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let info = unsafe { &*nghttp2_version(0) };
        assert_eq!(info.version_num as u32, NHTTP2_VERSION_NUM);
    }

    #[test]
    fn test_client_magic() {
        assert_eq!(NGHTTP2_CLIENT_MAGIC_LEN, 24);
        assert!(NGHTTP2_CLIENT_MAGIC.starts_with(b"PRI"));
    }
}
