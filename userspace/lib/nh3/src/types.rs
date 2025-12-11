//! Core type definitions for nh3
//!
//! This module defines the fundamental types used throughout the HTTP/3 library,
//! maintaining compatibility with nghttp3's C API types.

use crate::{c_int, c_void, size_t};

// ============================================================================
// Stream ID
// ============================================================================

/// Stream identifier type (62-bit, same as QUIC)
pub type StreamId = i64;

/// Maximum valid stream ID (2^62 - 1)
pub const MAX_STREAM_ID: StreamId = (1i64 << 62) - 1;

/// Push ID type
pub type PushId = u64;

/// Maximum push ID
pub const MAX_PUSH_ID: PushId = u64::MAX;

// ============================================================================
// Priority
// ============================================================================

/// HTTP/3 priority specification (RFC 9218)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Priority {
    /// Urgency (0-7, lower is more urgent)
    pub urgency: u8,
    /// Incremental flag
    pub inc: bool,
}

impl Priority {
    /// Default urgency value
    pub const DEFAULT_URGENCY: u8 = 3;
    /// Maximum urgency value
    pub const MAX_URGENCY: u8 = 7;
    
    /// Create a new priority with default values
    pub fn new() -> Self {
        Self {
            urgency: Self::DEFAULT_URGENCY,
            inc: false,
        }
    }
    
    /// Create a priority with specified values
    pub fn with_urgency(urgency: u8, incremental: bool) -> Self {
        Self {
            urgency: urgency.min(Self::MAX_URGENCY),
            inc: incremental,
        }
    }
}

/// nghttp3 compatible priority type
pub type nghttp3_pri = Priority;

// ============================================================================
// Name-Value Pair (Header)
// ============================================================================

/// Name-value pair for headers (nghttp3 compatible)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Nv {
    /// Header name
    pub name: *const u8,
    /// Header value
    pub value: *const u8,
    /// Name length
    pub namelen: size_t,
    /// Value length
    pub valuelen: size_t,
    /// Flags (never indexed, etc.)
    pub flags: u8,
}

impl Nv {
    /// Never index flag
    pub const FLAG_NEVER_INDEX: u8 = 0x01;
    /// No copy name flag
    pub const FLAG_NO_COPY_NAME: u8 = 0x02;
    /// No copy value flag
    pub const FLAG_NO_COPY_VALUE: u8 = 0x04;
    
    /// Create an empty NV (used as array terminator)
    pub fn empty() -> Self {
        Self {
            name: core::ptr::null(),
            value: core::ptr::null(),
            namelen: 0,
            valuelen: 0,
            flags: 0,
        }
    }
}

impl Default for Nv {
    fn default() -> Self {
        Self::empty()
    }
}

/// nghttp3 compatible type alias
pub type nghttp3_nv = Nv;

// ============================================================================
// Header Field (Rust-native)
// ============================================================================

/// A single header field (name-value pair) - Rust native version
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderField {
    /// Header name (lowercase)
    pub name: Vec<u8>,
    /// Header value
    pub value: Vec<u8>,
    /// Never index in QPACK
    pub never_index: bool,
}

impl HeaderField {
    /// Create a new header field
    pub fn new(name: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            never_index: false,
        }
    }
    
    /// Create a sensitive header field (never indexed)
    pub fn sensitive(name: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            never_index: true,
        }
    }
    
    /// Get the size of this header field (for table size calculation)
    /// Size = name.len() + value.len() + 32 (per RFC 9204)
    pub fn size(&self) -> usize {
        self.name.len() + self.value.len() + 32
    }
}

// ============================================================================
// Settings
// ============================================================================

/// HTTP/3 settings
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Settings {
    /// Maximum field section size
    pub max_field_section_size: u64,
    /// QPACK maximum table capacity
    pub qpack_max_dtable_capacity: u64,
    /// QPACK blocked streams
    pub qpack_blocked_streams: u64,
    /// Enable CONNECT protocol (RFC 9220)
    pub enable_connect_protocol: bool,
    /// Enable H3 datagrams
    pub h3_datagram: bool,
}

impl Default for Settings {
    fn default() -> Self {
        use crate::constants::defaults::*;
        Self {
            max_field_section_size: MAX_FIELD_SECTION_SIZE,
            qpack_max_dtable_capacity: QPACK_MAX_TABLE_CAPACITY,
            qpack_blocked_streams: QPACK_BLOCKED_STREAMS,
            enable_connect_protocol: false,
            h3_datagram: false,
        }
    }
}

/// nghttp3 compatible type alias
pub type nghttp3_settings = Settings;

/// Set default settings (nghttp3 compatible)
#[no_mangle]
pub extern "C" fn nghttp3_settings_default(settings: *mut nghttp3_settings) {
    if !settings.is_null() {
        unsafe {
            *settings = Settings::default();
        }
    }
}

// ============================================================================
// Data Provider
// ============================================================================

/// Read callback type for data provider
pub type ReadCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    buf: *mut u8,
    buflen: size_t,
    pflags: *mut u32,
    user_data: *mut c_void,
) -> isize;

/// Data provider for request/response body
#[repr(C)]
#[derive(Clone)]
pub struct DataProvider {
    /// Read callback
    pub read_data: Option<ReadCallback>,
}

impl DataProvider {
    /// Data provider flag: EOF
    pub const FLAG_EOF: u32 = 0x01;
    /// Data provider flag: no end stream
    pub const FLAG_NO_END_STREAM: u32 = 0x02;
    
    /// Create an empty data provider (no body)
    pub fn empty() -> Self {
        Self { read_data: None }
    }
    
    /// Create a data provider with a callback
    pub fn with_callback(callback: ReadCallback) -> Self {
        Self {
            read_data: Some(callback),
        }
    }
}

impl Default for DataProvider {
    fn default() -> Self {
        Self::empty()
    }
}

/// nghttp3 compatible type alias
pub type nghttp3_data_reader = DataProvider;

// ============================================================================
// Received Data
// ============================================================================

/// Received data information
#[repr(C)]
#[derive(Debug, Clone)]
pub struct RcBuf {
    /// Base pointer to data
    pub base: *const u8,
    /// Length of data
    pub len: size_t,
}

impl RcBuf {
    /// Create from slice
    pub fn from_slice(data: &[u8]) -> Self {
        Self {
            base: data.as_ptr(),
            len: data.len(),
        }
    }
    
    /// Get as slice (unsafe - caller must ensure validity)
    pub unsafe fn as_slice(&self) -> &[u8] {
        if self.base.is_null() {
            &[]
        } else {
            core::slice::from_raw_parts(self.base, self.len)
        }
    }
}

/// nghttp3 compatible type alias
pub type nghttp3_rcbuf = RcBuf;

// ============================================================================
// Vector (iovec-like)
// ============================================================================

/// I/O vector (similar to iovec)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Vec3 {
    /// Base pointer
    pub base: *mut u8,
    /// Length
    pub len: size_t,
}

impl Vec3 {
    /// Create an empty vector
    pub fn empty() -> Self {
        Self {
            base: core::ptr::null_mut(),
            len: 0,
        }
    }
    
    /// Create from mutable slice
    pub fn from_mut_slice(data: &mut [u8]) -> Self {
        Self {
            base: data.as_mut_ptr(),
            len: data.len(),
        }
    }
}

/// nghttp3 compatible type alias
pub type nghttp3_vec = Vec3;

// ============================================================================
// Callback Types
// ============================================================================

/// ACK stream data callback
pub type AckedStreamDataCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    datalen: size_t,
    user_data: *mut c_void,
) -> c_int;

/// Stream close callback
pub type StreamCloseCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    app_error_code: u64,
    user_data: *mut c_void,
) -> c_int;

/// Receive data callback
pub type RecvDataCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    data: *const u8,
    datalen: size_t,
    user_data: *mut c_void,
) -> c_int;

/// Deferred consume callback
pub type DeferredConsumeCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    consumed: size_t,
    user_data: *mut c_void,
) -> c_int;

/// Begin headers callback
pub type BeginHeadersCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    user_data: *mut c_void,
) -> c_int;

/// Receive header callback
pub type RecvHeaderCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    token: i32,
    name: *const nghttp3_rcbuf,
    value: *const nghttp3_rcbuf,
    flags: u8,
    user_data: *mut c_void,
) -> c_int;

/// End headers callback
pub type EndHeadersCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    fin: c_int,
    user_data: *mut c_void,
) -> c_int;

/// Begin trailers callback
pub type BeginTrailersCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    user_data: *mut c_void,
) -> c_int;

/// Receive trailer callback
pub type RecvTrailerCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    token: i32,
    name: *const nghttp3_rcbuf,
    value: *const nghttp3_rcbuf,
    flags: u8,
    user_data: *mut c_void,
) -> c_int;

/// End trailers callback
pub type EndTrailersCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    fin: c_int,
    user_data: *mut c_void,
) -> c_int;

/// Stop sending callback
pub type StopSendingCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    app_error_code: u64,
    user_data: *mut c_void,
) -> c_int;

/// End stream callback
pub type EndStreamCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    user_data: *mut c_void,
) -> c_int;

/// Reset stream callback
pub type ResetStreamCallback = extern "C" fn(
    conn: *mut c_void,
    stream_id: StreamId,
    app_error_code: u64,
    user_data: *mut c_void,
) -> c_int;

/// Shutdown callback
pub type ShutdownCallback = extern "C" fn(
    conn: *mut c_void,
    id: i64,
    user_data: *mut c_void,
) -> c_int;
