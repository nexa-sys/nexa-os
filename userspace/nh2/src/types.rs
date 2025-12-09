//! Core type definitions for nhttp2
//!
//! This module defines the fundamental types used throughout the library,
//! maintaining compatibility with nghttp2's C API types.

use crate::{c_int, c_void, size_t};

// ============================================================================
// Stream ID Type
// ============================================================================

/// Stream identifier type
/// 
/// Stream IDs are 31-bit unsigned integers.
/// - Stream 0 is reserved for connection-level frames
/// - Client-initiated streams use odd IDs (1, 3, 5, ...)
/// - Server-initiated streams use even IDs (2, 4, 6, ...)
pub type StreamId = i32;

/// Maximum valid stream ID
pub const MAX_STREAM_ID: StreamId = 0x7FFFFFFF;

/// Connection-level stream (stream 0)
pub const CONNECTION_STREAM_ID: StreamId = 0;

// ============================================================================
// Settings
// ============================================================================

/// HTTP/2 settings identifiers
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsId {
    /// SETTINGS_HEADER_TABLE_SIZE
    HeaderTableSize = 0x01,
    /// SETTINGS_ENABLE_PUSH
    EnablePush = 0x02,
    /// SETTINGS_MAX_CONCURRENT_STREAMS
    MaxConcurrentStreams = 0x03,
    /// SETTINGS_INITIAL_WINDOW_SIZE
    InitialWindowSize = 0x04,
    /// SETTINGS_MAX_FRAME_SIZE
    MaxFrameSize = 0x05,
    /// SETTINGS_MAX_HEADER_LIST_SIZE
    MaxHeaderListSize = 0x06,
    /// SETTINGS_ENABLE_CONNECT_PROTOCOL (RFC 8441)
    EnableConnectProtocol = 0x08,
}

/// A single settings entry
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SettingsEntry {
    pub settings_id: i32,
    pub value: u32,
}

/// Default settings values
pub mod default_settings {
    pub const HEADER_TABLE_SIZE: u32 = 4096;
    pub const ENABLE_PUSH: u32 = 1;
    pub const MAX_CONCURRENT_STREAMS: u32 = 100;
    pub const INITIAL_WINDOW_SIZE: u32 = 65535;
    pub const MAX_FRAME_SIZE: u32 = 16384;
    pub const MAX_HEADER_LIST_SIZE: u32 = u32::MAX;
}

// ============================================================================
// Name-Value Pair (Headers)
// ============================================================================

/// Name-value pair for headers (nghttp2 compatible)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Nv {
    /// Header name
    pub name: *const u8,
    /// Header value
    pub value: *const u8,
    /// Length of name
    pub namelen: size_t,
    /// Length of value
    pub valuelen: size_t,
    /// Flags (NGHTTP2_NV_FLAG_*)
    pub flags: u8,
}

impl Nv {
    /// Create a new name-value pair
    pub fn new(name: &[u8], value: &[u8], flags: u8) -> Self {
        Self {
            name: name.as_ptr(),
            value: value.as_ptr(),
            namelen: name.len(),
            valuelen: value.len(),
            flags,
        }
    }

    /// Get name as slice
    /// 
    /// # Safety
    /// The name pointer must be valid and point to at least `namelen` bytes.
    pub unsafe fn name_slice(&self) -> &[u8] {
        core::slice::from_raw_parts(self.name, self.namelen)
    }

    /// Get value as slice
    /// 
    /// # Safety
    /// The value pointer must be valid and point to at least `valuelen` bytes.
    pub unsafe fn value_slice(&self) -> &[u8] {
        core::slice::from_raw_parts(self.value, self.valuelen)
    }
}

/// Name-value flags
pub mod nv_flags {
    /// No flags
    pub const NGHTTP2_NV_FLAG_NONE: u8 = 0;
    /// Do not index this header (HPACK)
    pub const NGHTTP2_NV_FLAG_NO_INDEX: u8 = 0x01;
    /// This header should never be indexed (HPACK)
    pub const NGHTTP2_NV_FLAG_NEVER_INDEX: u8 = 0x02;
    /// Name is statically allocated (no copy needed)
    pub const NGHTTP2_NV_FLAG_NO_COPY_NAME: u8 = 0x04;
    /// Value is statically allocated (no copy needed)
    pub const NGHTTP2_NV_FLAG_NO_COPY_VALUE: u8 = 0x08;
}

// ============================================================================
// Priority Specification
// ============================================================================

/// Priority specification for streams
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PrioritySpec {
    /// Stream ID of the parent stream
    pub stream_id: StreamId,
    /// Weight (1-256)
    pub weight: i32,
    /// Whether this is an exclusive dependency
    pub exclusive: u8,
}

impl Default for PrioritySpec {
    fn default() -> Self {
        Self {
            stream_id: 0,
            weight: 16,
            exclusive: 0,
        }
    }
}

// ============================================================================
// Data Provider
// ============================================================================

/// Data source union for data frames
#[repr(C)]
pub union DataSource {
    /// File descriptor
    pub fd: c_int,
    /// Arbitrary pointer
    pub ptr: *mut c_void,
}

/// Data provider for sending data frames
pub type DataSourceReadCallback = extern "C" fn(
    session: *mut c_void,
    stream_id: StreamId,
    buf: *mut u8,
    length: size_t,
    data_flags: *mut u32,
    source: *mut DataSource,
    user_data: *mut c_void,
) -> isize;

/// Data provider structure
#[repr(C)]
pub struct DataProvider {
    /// Data source
    pub source: DataSource,
    /// Read callback
    pub read_callback: Option<DataSourceReadCallback>,
}

/// Data flags for data provider callback
pub mod data_flags {
    /// No flags
    pub const NGHTTP2_DATA_FLAG_NONE: u32 = 0;
    /// End of file - no more data to send
    pub const NGHTTP2_DATA_FLAG_EOF: u32 = 0x01;
    /// Do not send DATA frame with this call
    pub const NGHTTP2_DATA_FLAG_NO_END_STREAM: u32 = 0x02;
    /// Do not copy data to internal buffer
    pub const NGHTTP2_DATA_FLAG_NO_COPY: u32 = 0x04;
}

// ============================================================================
// GOAWAY Frame Data
// ============================================================================

/// GOAWAY frame data
#[repr(C)]
#[derive(Debug, Clone)]
pub struct GoawayData {
    /// Last stream ID
    pub last_stream_id: StreamId,
    /// Error code
    pub error_code: u32,
    /// Opaque data
    pub opaque_data: *const u8,
    /// Length of opaque data
    pub opaque_data_len: size_t,
}

// ============================================================================
// RST_STREAM Frame Data
// ============================================================================

/// RST_STREAM frame data
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RstStreamData {
    /// Error code
    pub error_code: u32,
}

// ============================================================================
// PUSH_PROMISE Frame Data
// ============================================================================

/// PUSH_PROMISE frame data
#[repr(C)]
#[derive(Debug)]
pub struct PushPromiseData {
    /// Promised stream ID
    pub promised_stream_id: StreamId,
    /// Headers
    pub nva: *const Nv,
    /// Number of headers
    pub nvlen: size_t,
}

// ============================================================================
// Extension Frame Data
// ============================================================================

/// Extension frame data
#[repr(C)]
pub struct ExtensionData {
    /// Payload
    pub payload: *mut c_void,
}

// ============================================================================
// Memory Allocator
// ============================================================================

/// Memory allocator function type
pub type MemAllocFunc = extern "C" fn(size: size_t, mem_user_data: *mut c_void) -> *mut c_void;

/// Memory free function type
pub type MemFreeFunc = extern "C" fn(ptr: *mut c_void, mem_user_data: *mut c_void);

/// Memory realloc function type
pub type MemReallocFunc = extern "C" fn(
    ptr: *mut c_void,
    size: size_t,
    mem_user_data: *mut c_void,
) -> *mut c_void;

/// Custom memory allocator
#[repr(C)]
pub struct MemAllocator {
    /// User data pointer
    pub mem_user_data: *mut c_void,
    /// Malloc function
    pub malloc: Option<MemAllocFunc>,
    /// Free function
    pub free: Option<MemFreeFunc>,
    /// Calloc function
    pub calloc: Option<MemAllocFunc>,
    /// Realloc function
    pub realloc: Option<MemReallocFunc>,
}

// ============================================================================
// Session Option
// ============================================================================

/// Session options
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SessionOption {
    /// Disable automatic WINDOW_UPDATE
    pub no_auto_window_update: u8,
    /// Receive client connection preface automatically
    pub no_recv_client_magic: u8,
    /// Do not send HTTP messaging validation
    pub no_http_messaging: u8,
    /// Maximum deflate dynamic table size
    pub max_deflate_dynamic_table_size: size_t,
    /// Send SETTINGS automatically
    pub no_auto_ping_ack: u8,
    /// Maximum outbound header list size
    pub max_send_header_block_length: size_t,
    /// Maximum settings entries per SETTINGS frame
    pub max_settings: size_t,
}

impl Default for SessionOption {
    fn default() -> Self {
        Self {
            no_auto_window_update: 0,
            no_recv_client_magic: 0,
            no_http_messaging: 0,
            max_deflate_dynamic_table_size: 4096,
            no_auto_ping_ack: 0,
            max_send_header_block_length: 65536,
            max_settings: 32,
        }
    }
}
