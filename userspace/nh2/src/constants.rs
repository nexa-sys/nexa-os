//! HTTP/2 protocol constants
//!
//! This module defines constants used throughout the HTTP/2 protocol implementation.

// ============================================================================
// Frame Constants
// ============================================================================

/// Maximum frame payload size (default)
pub const DEFAULT_MAX_FRAME_SIZE: u32 = 16384;

/// Minimum allowed max frame size
pub const MIN_MAX_FRAME_SIZE: u32 = 16384;

/// Maximum allowed max frame size
pub const MAX_MAX_FRAME_SIZE: u32 = 16777215;

/// Frame header length (9 bytes)
pub const FRAME_HEADER_LENGTH: usize = 9;

/// Default header table size
pub const DEFAULT_HEADER_TABLE_SIZE: u32 = 4096;

/// Default initial window size
pub const DEFAULT_INITIAL_WINDOW_SIZE: u32 = 65535;

/// Default max concurrent streams
pub const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 100;

/// Maximum window size
pub const MAX_WINDOW_SIZE: u32 = 0x7FFFFFFF;

/// Default window update threshold
pub const DEFAULT_WINDOW_UPDATE_THRESHOLD: u32 = 32768;

// ============================================================================
// Frame Types
// ============================================================================

/// Frame type constants
pub mod frame_type {
    pub const DATA: u8 = 0x0;
    pub const HEADERS: u8 = 0x1;
    pub const PRIORITY: u8 = 0x2;
    pub const RST_STREAM: u8 = 0x3;
    pub const SETTINGS: u8 = 0x4;
    pub const PUSH_PROMISE: u8 = 0x5;
    pub const PING: u8 = 0x6;
    pub const GOAWAY: u8 = 0x7;
    pub const WINDOW_UPDATE: u8 = 0x8;
    pub const CONTINUATION: u8 = 0x9;
    pub const ALTSVC: u8 = 0xa;
    pub const ORIGIN: u8 = 0xc;
}

// ============================================================================
// Frame Flags
// ============================================================================

/// Frame flag constants
pub mod frame_flags {
    /// No flags
    pub const NONE: u8 = 0x0;
    /// END_STREAM flag (DATA, HEADERS)
    pub const END_STREAM: u8 = 0x1;
    /// ACK flag (SETTINGS, PING)
    pub const ACK: u8 = 0x1;
    /// END_HEADERS flag (HEADERS, PUSH_PROMISE, CONTINUATION)
    pub const END_HEADERS: u8 = 0x4;
    /// PADDED flag (DATA, HEADERS, PUSH_PROMISE)
    pub const PADDED: u8 = 0x8;
    /// PRIORITY flag (HEADERS)
    pub const PRIORITY: u8 = 0x20;
}

// ============================================================================
// Settings IDs
// ============================================================================

/// Settings identifier constants
pub mod settings_id {
    pub const HEADER_TABLE_SIZE: u16 = 0x1;
    pub const ENABLE_PUSH: u16 = 0x2;
    pub const MAX_CONCURRENT_STREAMS: u16 = 0x3;
    pub const INITIAL_WINDOW_SIZE: u16 = 0x4;
    pub const MAX_FRAME_SIZE: u16 = 0x5;
    pub const MAX_HEADER_LIST_SIZE: u16 = 0x6;
    pub const ENABLE_CONNECT_PROTOCOL: u16 = 0x8;
}

// ============================================================================
// Stream States
// ============================================================================

/// Stream state constants for nghttp2 compatibility
pub mod stream_state {
    /// Idle state
    pub const IDLE: i32 = 1;
    /// Open state
    pub const OPEN: i32 = 2;
    /// Reserved (local) state
    pub const RESERVED_LOCAL: i32 = 3;
    /// Reserved (remote) state
    pub const RESERVED_REMOTE: i32 = 4;
    /// Half-closed (local) state
    pub const HALF_CLOSED_LOCAL: i32 = 5;
    /// Half-closed (remote) state
    pub const HALF_CLOSED_REMOTE: i32 = 6;
    /// Closed state
    pub const CLOSED: i32 = 7;
}

// ============================================================================
// HPACK Constants
// ============================================================================

/// HPACK-related constants
pub mod hpack {
    /// Static table size
    pub const STATIC_TABLE_SIZE: usize = 61;
    /// Default dynamic table size
    pub const DEFAULT_DYNAMIC_TABLE_SIZE: usize = 4096;
    /// Maximum dynamic table size
    pub const MAX_DYNAMIC_TABLE_SIZE: usize = 65536;
    /// Entry overhead (per RFC 7541)
    pub const ENTRY_OVERHEAD: usize = 32;
}

// ============================================================================
// Priority Constants
// ============================================================================

/// Priority-related constants
pub mod priority {
    /// Default weight
    pub const DEFAULT_WEIGHT: i32 = 16;
    /// Minimum weight
    pub const MIN_WEIGHT: i32 = 1;
    /// Maximum weight
    pub const MAX_WEIGHT: i32 = 256;
}

// ============================================================================
// Connection Constants
// ============================================================================

/// Connection-related constants
pub mod connection {
    /// Connection preface length
    pub const PREFACE_LENGTH: usize = 24;
    /// Maximum stream ID
    pub const MAX_STREAM_ID: i32 = 0x7FFFFFFF;
    /// Default ping timeout (seconds)
    pub const DEFAULT_PING_TIMEOUT: u64 = 30;
    /// Default keepalive timeout (seconds)
    pub const DEFAULT_KEEPALIVE_TIMEOUT: u64 = 60;
}

// ============================================================================
// Buffer Sizes
// ============================================================================

/// Buffer size constants
pub mod buffer {
    /// Default input buffer size
    pub const DEFAULT_INPUT_BUFFER_SIZE: usize = 65536;
    /// Default output buffer size
    pub const DEFAULT_OUTPUT_BUFFER_SIZE: usize = 65536;
    /// Maximum header list size
    pub const DEFAULT_MAX_HEADER_LIST_SIZE: usize = 65536;
}
