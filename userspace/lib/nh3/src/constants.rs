//! HTTP/3 Constants
//!
//! This module defines constants used throughout the HTTP/3 implementation,
//! following RFC 9114 and RFC 9204 specifications.

// ============================================================================
// HTTP/3 Frame Types (RFC 9114 Section 7.2)
// ============================================================================

/// Frame type constants
pub mod frame_type {
    /// DATA frame - carries request or response body
    pub const DATA: u64 = 0x00;
    /// HEADERS frame - carries HTTP header fields
    pub const HEADERS: u64 = 0x01;
    /// CANCEL_PUSH frame - cancels a server push
    pub const CANCEL_PUSH: u64 = 0x03;
    /// SETTINGS frame - communicates configuration parameters
    pub const SETTINGS: u64 = 0x04;
    /// PUSH_PROMISE frame - initiates a server push
    pub const PUSH_PROMISE: u64 = 0x05;
    /// GOAWAY frame - initiates graceful connection shutdown
    pub const GOAWAY: u64 = 0x07;
    /// MAX_PUSH_ID frame - controls maximum push ID
    pub const MAX_PUSH_ID: u64 = 0x0D;
    
    // Reserved frame types that MUST be ignored
    pub const RESERVED_MIN: u64 = 0x21;
}

// ============================================================================
// HTTP/3 Error Codes (RFC 9114 Section 8.1)
// ============================================================================

/// HTTP/3 error codes
pub mod h3_error {
    /// No error
    pub const H3_NO_ERROR: u64 = 0x100;
    /// General protocol error
    pub const H3_GENERAL_PROTOCOL_ERROR: u64 = 0x101;
    /// Internal error
    pub const H3_INTERNAL_ERROR: u64 = 0x102;
    /// Stream creation error
    pub const H3_STREAM_CREATION_ERROR: u64 = 0x103;
    /// Critical stream closed unexpectedly
    pub const H3_CLOSED_CRITICAL_STREAM: u64 = 0x104;
    /// Frame not allowed in current context
    pub const H3_FRAME_UNEXPECTED: u64 = 0x105;
    /// Frame encoding error
    pub const H3_FRAME_ERROR: u64 = 0x106;
    /// Excessive load
    pub const H3_EXCESSIVE_LOAD: u64 = 0x107;
    /// Push ID error
    pub const H3_ID_ERROR: u64 = 0x108;
    /// Settings error
    pub const H3_SETTINGS_ERROR: u64 = 0x109;
    /// Missing settings
    pub const H3_MISSING_SETTINGS: u64 = 0x10A;
    /// Request rejected
    pub const H3_REQUEST_REJECTED: u64 = 0x10B;
    /// Request cancelled
    pub const H3_REQUEST_CANCELLED: u64 = 0x10C;
    /// Request incomplete
    pub const H3_REQUEST_INCOMPLETE: u64 = 0x10D;
    /// Message error
    pub const H3_MESSAGE_ERROR: u64 = 0x10E;
    /// Connect error
    pub const H3_CONNECT_ERROR: u64 = 0x10F;
    /// Version fallback
    pub const H3_VERSION_FALLBACK: u64 = 0x110;
}

// ============================================================================
// HTTP/3 Settings (RFC 9114 Section 7.2.4.1)
// ============================================================================

/// Settings identifier constants
pub mod settings_id {
    /// QPACK maximum table capacity
    pub const QPACK_MAX_TABLE_CAPACITY: u64 = 0x01;
    /// Maximum header list size (previously MAX_HEADER_LIST_SIZE)
    pub const MAX_FIELD_SECTION_SIZE: u64 = 0x06;
    /// QPACK blocked streams
    pub const QPACK_BLOCKED_STREAMS: u64 = 0x07;
    /// Enable CONNECT protocol (RFC 9220)
    pub const ENABLE_CONNECT_PROTOCOL: u64 = 0x08;
    /// Enable WebTransport (draft)
    pub const H3_DATAGRAM: u64 = 0x33;
    /// Enable WebTransport
    pub const ENABLE_WEBTRANSPORT: u64 = 0x2b603742;
    /// WebTransport max sessions
    pub const WEBTRANSPORT_MAX_SESSIONS: u64 = 0x2b603743;
}

// ============================================================================
// HTTP/3 Stream Types (RFC 9114 Section 6.2)
// ============================================================================

/// Unidirectional stream type constants
pub mod stream_type {
    /// Control stream
    pub const CONTROL: u64 = 0x00;
    /// Push stream
    pub const PUSH: u64 = 0x01;
    /// QPACK encoder stream
    pub const QPACK_ENCODER: u64 = 0x02;
    /// QPACK decoder stream
    pub const QPACK_DECODER: u64 = 0x03;
    /// WebTransport stream (draft)
    pub const WEBTRANSPORT: u64 = 0x41;
}

// ============================================================================
// QPACK Constants (RFC 9204)
// ============================================================================

/// QPACK related constants
pub mod qpack {
    /// Default maximum table capacity
    pub const DEFAULT_MAX_TABLE_CAPACITY: usize = 4096;
    /// Default maximum blocked streams
    pub const DEFAULT_MAX_BLOCKED_STREAMS: usize = 100;
    /// Static table size
    pub const STATIC_TABLE_SIZE: usize = 99;
    /// Default dynamic table capacity
    pub const DEFAULT_DYNAMIC_TABLE_CAPACITY: usize = 4096;
    
    /// QPACK instruction types
    pub mod instruction {
        /// Set dynamic table capacity
        pub const SET_DYNAMIC_TABLE_CAPACITY: u8 = 0x20;
        /// Insert with name reference
        pub const INSERT_WITH_NAME_REF: u8 = 0x80;
        /// Insert with literal name
        pub const INSERT_LITERAL_NAME: u8 = 0x40;
        /// Duplicate
        pub const DUPLICATE: u8 = 0x00;
        /// Section acknowledgment
        pub const SECTION_ACK: u8 = 0x80;
        /// Stream cancellation
        pub const STREAM_CANCEL: u8 = 0x40;
        /// Insert count increment
        pub const INSERT_COUNT_INCREMENT: u8 = 0x00;
    }
    
    /// QPACK header field representation
    pub mod header_field {
        /// Indexed field line
        pub const INDEXED: u8 = 0x80;
        /// Indexed field line with post-base index
        pub const INDEXED_POST_BASE: u8 = 0x10;
        /// Literal field line with name reference
        pub const LITERAL_NAME_REF: u8 = 0x40;
        /// Literal field line with post-base name reference
        pub const LITERAL_POST_BASE_NAME_REF: u8 = 0x00;
        /// Literal field line with literal name
        pub const LITERAL_NAME: u8 = 0x20;
    }
}

// ============================================================================
// Default Values
// ============================================================================

/// Default settings values
pub mod defaults {
    /// Default maximum header list size (16KB)
    pub const MAX_FIELD_SECTION_SIZE: u64 = 16384;
    /// Default QPACK max table capacity
    pub const QPACK_MAX_TABLE_CAPACITY: u64 = 4096;
    /// Default QPACK blocked streams
    pub const QPACK_BLOCKED_STREAMS: u64 = 100;
    /// Maximum frame size (we accept)
    pub const MAX_FRAME_SIZE: usize = 16384;
    /// Maximum push ID (default disabled)
    pub const MAX_PUSH_ID: u64 = 0;
}

// ============================================================================
// Limits
// ============================================================================

/// Protocol limits
pub mod limits {
    /// Maximum variable-length integer value (2^62 - 1)
    pub const MAX_VARINT: u64 = (1u64 << 62) - 1;
    /// Maximum frame payload size
    pub const MAX_FRAME_PAYLOAD: usize = 16777215; // 16MB - 1
    /// Maximum number of concurrent streams (recommended)
    pub const MAX_CONCURRENT_STREAMS: u64 = 100;
    /// Maximum header name length
    pub const MAX_HEADER_NAME_LEN: usize = 65536;
    /// Maximum header value length
    pub const MAX_HEADER_VALUE_LEN: usize = 65536;
}
