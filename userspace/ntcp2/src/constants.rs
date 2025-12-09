//! QUIC protocol constants
//!
//! This module defines constants used throughout the QUIC implementation.

// ============================================================================
// Protocol Version Constants
// ============================================================================

/// QUIC version 1 (RFC 9000)
pub const QUIC_VERSION_1: u32 = 0x00000001;

/// QUIC version 2 (RFC 9369)
pub const QUIC_VERSION_2: u32 = 0x6b3343cf;

/// Version for version negotiation
pub const QUIC_VERSION_NEGOTIATION: u32 = 0x00000000;

// ============================================================================
// Packet Type Constants
// ============================================================================

pub mod packet_type {
    /// Initial packet (long header)
    pub const INITIAL: u8 = 0x00;
    /// 0-RTT packet (long header)
    pub const ZERO_RTT: u8 = 0x01;
    /// Handshake packet (long header)
    pub const HANDSHAKE: u8 = 0x02;
    /// Retry packet (long header)
    pub const RETRY: u8 = 0x03;
    /// 1-RTT packet (short header)
    pub const ONE_RTT: u8 = 0x40;
    /// Version negotiation packet
    pub const VERSION_NEGOTIATION: u8 = 0x80;
}

// ============================================================================
// Header Constants
// ============================================================================

/// Long header form bit
pub const HEADER_FORM_LONG: u8 = 0x80;

/// Fixed bit (must be 1)
pub const HEADER_FIXED_BIT: u8 = 0x40;

/// Long header type mask
pub const LONG_HEADER_TYPE_MASK: u8 = 0x30;

/// Short header spin bit
pub const SHORT_HEADER_SPIN_BIT: u8 = 0x20;

/// Short header key phase bit
pub const SHORT_HEADER_KEY_PHASE: u8 = 0x04;

/// Packet number length mask (short header)
pub const PN_LENGTH_MASK: u8 = 0x03;

// ============================================================================
// Frame Type Constants
// ============================================================================

pub mod frame_type {
    /// PADDING frame
    pub const PADDING: u64 = 0x00;
    /// PING frame
    pub const PING: u64 = 0x01;
    /// ACK frame (without ECN)
    pub const ACK: u64 = 0x02;
    /// ACK frame (with ECN)
    pub const ACK_ECN: u64 = 0x03;
    /// RESET_STREAM frame
    pub const RESET_STREAM: u64 = 0x04;
    /// STOP_SENDING frame
    pub const STOP_SENDING: u64 = 0x05;
    /// CRYPTO frame
    pub const CRYPTO: u64 = 0x06;
    /// NEW_TOKEN frame
    pub const NEW_TOKEN: u64 = 0x07;
    /// STREAM frame (base type, bits indicate FIN, LEN, OFF)
    pub const STREAM: u64 = 0x08;
    /// MAX_DATA frame
    pub const MAX_DATA: u64 = 0x10;
    /// MAX_STREAM_DATA frame
    pub const MAX_STREAM_DATA: u64 = 0x11;
    /// MAX_STREAMS (bidirectional) frame
    pub const MAX_STREAMS_BIDI: u64 = 0x12;
    /// MAX_STREAMS (unidirectional) frame
    pub const MAX_STREAMS_UNI: u64 = 0x13;
    /// DATA_BLOCKED frame
    pub const DATA_BLOCKED: u64 = 0x14;
    /// STREAM_DATA_BLOCKED frame
    pub const STREAM_DATA_BLOCKED: u64 = 0x15;
    /// STREAMS_BLOCKED (bidirectional) frame
    pub const STREAMS_BLOCKED_BIDI: u64 = 0x16;
    /// STREAMS_BLOCKED (unidirectional) frame
    pub const STREAMS_BLOCKED_UNI: u64 = 0x17;
    /// NEW_CONNECTION_ID frame
    pub const NEW_CONNECTION_ID: u64 = 0x18;
    /// RETIRE_CONNECTION_ID frame
    pub const RETIRE_CONNECTION_ID: u64 = 0x19;
    /// PATH_CHALLENGE frame
    pub const PATH_CHALLENGE: u64 = 0x1a;
    /// PATH_RESPONSE frame
    pub const PATH_RESPONSE: u64 = 0x1b;
    /// CONNECTION_CLOSE (transport error) frame
    pub const CONNECTION_CLOSE: u64 = 0x1c;
    /// CONNECTION_CLOSE (application error) frame
    pub const CONNECTION_CLOSE_APP: u64 = 0x1d;
    /// HANDSHAKE_DONE frame
    pub const HANDSHAKE_DONE: u64 = 0x1e;
    /// DATAGRAM frame (without length) - RFC 9221
    pub const DATAGRAM: u64 = 0x30;
    /// DATAGRAM frame (with length) - RFC 9221
    pub const DATAGRAM_LEN: u64 = 0x31;
}

/// Stream frame FIN bit
pub const STREAM_FIN_BIT: u64 = 0x01;
/// Stream frame LEN bit
pub const STREAM_LEN_BIT: u64 = 0x02;
/// Stream frame OFF bit
pub const STREAM_OFF_BIT: u64 = 0x04;

// ============================================================================
// Transport Parameter IDs
// ============================================================================

pub mod transport_param {
    /// Original destination connection ID
    pub const ORIGINAL_DCID: u64 = 0x00;
    /// Maximum idle timeout
    pub const MAX_IDLE_TIMEOUT: u64 = 0x01;
    /// Stateless reset token
    pub const STATELESS_RESET_TOKEN: u64 = 0x02;
    /// Maximum UDP payload size
    pub const MAX_UDP_PAYLOAD_SIZE: u64 = 0x03;
    /// Initial max data
    pub const INITIAL_MAX_DATA: u64 = 0x04;
    /// Initial max stream data (bidi local)
    pub const INITIAL_MAX_STREAM_DATA_BIDI_LOCAL: u64 = 0x05;
    /// Initial max stream data (bidi remote)
    pub const INITIAL_MAX_STREAM_DATA_BIDI_REMOTE: u64 = 0x06;
    /// Initial max stream data (uni)
    pub const INITIAL_MAX_STREAM_DATA_UNI: u64 = 0x07;
    /// Initial max streams (bidi)
    pub const INITIAL_MAX_STREAMS_BIDI: u64 = 0x08;
    /// Initial max streams (uni)
    pub const INITIAL_MAX_STREAMS_UNI: u64 = 0x09;
    /// ACK delay exponent
    pub const ACK_DELAY_EXPONENT: u64 = 0x0a;
    /// Max ACK delay
    pub const MAX_ACK_DELAY: u64 = 0x0b;
    /// Disable active migration
    pub const DISABLE_ACTIVE_MIGRATION: u64 = 0x0c;
    /// Preferred address
    pub const PREFERRED_ADDRESS: u64 = 0x0d;
    /// Active connection ID limit
    pub const ACTIVE_CONNECTION_ID_LIMIT: u64 = 0x0e;
    /// Initial source connection ID
    pub const INITIAL_SCID: u64 = 0x0f;
    /// Retry source connection ID
    pub const RETRY_SCID: u64 = 0x10;
    /// Max datagram frame size (RFC 9221)
    pub const MAX_DATAGRAM_FRAME_SIZE: u64 = 0x20;
    /// Grease QUIC bit (RFC 9287)
    pub const GREASE_QUIC_BIT: u64 = 0x2ab2;
    /// Version information
    pub const VERSION_INFORMATION: u64 = 0x11;
}

// ============================================================================
// Default Values
// ============================================================================

/// Default max UDP payload size
pub const DEFAULT_MAX_UDP_PAYLOAD_SIZE: u64 = 65527;

/// Default initial max data
pub const DEFAULT_INITIAL_MAX_DATA: u64 = 0;

/// Default initial max stream data
pub const DEFAULT_INITIAL_MAX_STREAM_DATA: u64 = 0;

/// Default initial max streams
pub const DEFAULT_INITIAL_MAX_STREAMS: u64 = 0;

/// Default max idle timeout (30 seconds in ms)
pub const DEFAULT_MAX_IDLE_TIMEOUT: u64 = 30_000;

/// Default ACK delay exponent
pub const DEFAULT_ACK_DELAY_EXPONENT: u64 = 3;

/// Default max ACK delay (25ms)
pub const DEFAULT_MAX_ACK_DELAY: u64 = 25;

/// Default active connection ID limit
pub const DEFAULT_ACTIVE_CONNECTION_ID_LIMIT: u64 = 2;

/// Minimum active connection ID limit
pub const MIN_ACTIVE_CONNECTION_ID_LIMIT: u64 = 2;

// ============================================================================
// Limits
// ============================================================================

/// Maximum connection ID length
pub const MAX_CID_LEN: usize = 20;

/// Minimum connection ID length
pub const MIN_CID_LEN: usize = 0;

/// Stateless reset token length
pub const STATELESS_RESET_TOKEN_LEN: usize = 16;

/// Maximum frame overhead
pub const MAX_FRAME_OVERHEAD: usize = 25;

/// Initial packet number space ID
pub const PKTNS_ID_INITIAL: usize = 0;

/// Handshake packet number space ID
pub const PKTNS_ID_HANDSHAKE: usize = 1;

/// Application (1-RTT) packet number space ID
pub const PKTNS_ID_APPLICATION: usize = 2;

/// Number of packet number spaces
pub const NUM_PKTNS: usize = 3;

// ============================================================================
// Loss Detection Constants (RFC 9002)
// ============================================================================

pub mod loss_detection {
    /// Packet threshold for loss detection (kPacketThreshold)
    pub const PACKET_THRESHOLD: u64 = 3;
    
    /// Time threshold multiplier (kTimeThreshold = 9/8)
    pub const TIME_THRESHOLD_NUM: u64 = 9;
    pub const TIME_THRESHOLD_DEN: u64 = 8;
    
    /// Initial RTT estimate (333ms in nanoseconds)
    pub const INITIAL_RTT: u64 = 333_000_000;
    
    /// kGranularity (timer granularity, 1ms in nanoseconds)
    pub const GRANULARITY: u64 = 1_000_000;
    
    /// Maximum number of PTO exponents
    pub const MAX_PTO_COUNT: usize = 6;
}

// ============================================================================
// Congestion Control Constants (RFC 9002)
// ============================================================================

pub mod congestion {
    /// Initial window (10 * MSS or 14720 bytes, whichever is smaller)
    pub const INITIAL_WINDOW_PACKETS: u64 = 10;
    
    /// Minimum window (2 * MSS)
    pub const MINIMUM_WINDOW_PACKETS: u64 = 2;
    
    /// Loss reduction factor (0.5 for New Reno)
    pub const LOSS_REDUCTION_FACTOR_NUM: u64 = 1;
    pub const LOSS_REDUCTION_FACTOR_DEN: u64 = 2;
    
    /// Persistent congestion threshold (kPersistentCongestionThreshold = 3)
    pub const PERSISTENT_CONGESTION_THRESHOLD: u64 = 3;
    
    /// Default MSS (Maximum Segment Size) for QUIC
    pub const DEFAULT_MSS: usize = 1200;
}

// ============================================================================
// Crypto Constants
// ============================================================================

pub mod crypto {
    /// Initial salt for QUIC v1 (RFC 9001)
    pub const INITIAL_SALT_V1: [u8; 20] = [
        0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3,
        0x4d, 0x17, 0x9a, 0xe6, 0xa4, 0xc8, 0x0c, 0xad,
        0xcc, 0xbb, 0x7f, 0x0a,
    ];
    
    /// Initial salt for QUIC v2 (RFC 9369)
    pub const INITIAL_SALT_V2: [u8; 20] = [
        0x0d, 0xed, 0xe3, 0xde, 0xf7, 0x00, 0xa6, 0xdb,
        0x81, 0x93, 0x81, 0xbe, 0x6e, 0x26, 0x9d, 0xcb,
        0xf9, 0xbd, 0x2e, 0xd9,
    ];
    
    /// Client initial label
    pub const CLIENT_INITIAL_LABEL: &[u8] = b"client in";
    
    /// Server initial label
    pub const SERVER_INITIAL_LABEL: &[u8] = b"server in";
    
    /// Key label
    pub const KEY_LABEL: &[u8] = b"quic key";
    
    /// IV label
    pub const IV_LABEL: &[u8] = b"quic iv";
    
    /// HP (header protection) label
    pub const HP_LABEL: &[u8] = b"quic hp";
    
    /// Key update label
    pub const KEY_UPDATE_LABEL: &[u8] = b"quic ku";
    
    /// Retry key for QUIC v1 (RFC 9001)
    pub const RETRY_KEY_V1: [u8; 16] = [
        0xbe, 0x0c, 0x69, 0x0b, 0x9f, 0x66, 0x57, 0x5a,
        0x1d, 0x76, 0x6b, 0x54, 0xe3, 0x68, 0xc8, 0x4e,
    ];
    
    /// Retry nonce for QUIC v1 (RFC 9001)
    pub const RETRY_NONCE_V1: [u8; 12] = [
        0x46, 0x15, 0x99, 0xd3, 0x5d, 0x63, 0x2b, 0xf2,
        0x23, 0x98, 0x25, 0xbb,
    ];
    
    /// Retry key for QUIC v2 (RFC 9369)
    pub const RETRY_KEY_V2: [u8; 16] = [
        0x8f, 0xb4, 0xb0, 0x1b, 0x56, 0xac, 0x48, 0xe2,
        0x60, 0xfb, 0xcb, 0xce, 0xad, 0x7c, 0xcc, 0x92,
    ];
    
    /// Retry nonce for QUIC v2 (RFC 9369)
    pub const RETRY_NONCE_V2: [u8; 12] = [
        0xd8, 0x69, 0x69, 0xbc, 0x2d, 0x7c, 0x6d, 0x99,
        0x90, 0xef, 0xb0, 0x4a,
    ];
    
    /// AES-128-GCM key length
    pub const AES_128_KEY_LEN: usize = 16;
    
    /// AES-256-GCM key length
    pub const AES_256_KEY_LEN: usize = 32;
    
    /// ChaCha20-Poly1305 key length
    pub const CHACHA20_KEY_LEN: usize = 32;
    
    /// AEAD tag length
    pub const AEAD_TAG_LEN: usize = 16;
    
    /// AEAD nonce/IV length
    pub const AEAD_NONCE_LEN: usize = 12;
    
    /// Header protection sample length
    pub const HP_SAMPLE_LEN: usize = 16;
    
    /// Maximum packet number length in bytes
    pub const MAX_PKT_NUM_LEN: usize = 4;
}

// ============================================================================
// QPACK Constants (RFC 9204)
// ============================================================================

#[cfg(feature = "qpack")]
pub mod qpack {
    /// Default QPACK max table capacity
    pub const DEFAULT_MAX_TABLE_CAPACITY: usize = 4096;
    
    /// Default QPACK max blocked streams
    pub const DEFAULT_MAX_BLOCKED_STREAMS: usize = 100;
    
    /// Entry overhead in dynamic table
    pub const ENTRY_OVERHEAD: usize = 32;
}

// ============================================================================
// Connection State
// ============================================================================

/// Connection states
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Initial state (client sending Initial, server receiving)
    Initial = 0,
    /// Handshake in progress
    Handshake = 1,
    /// Handshake complete, 0-RTT available
    EarlyData = 2,
    /// Fully established connection
    Established = 3,
    /// Connection closing (sending CONNECTION_CLOSE)
    Closing = 4,
    /// Connection draining (waiting for peer's packets)
    Draining = 5,
    /// Connection closed
    Closed = 6,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::Initial
    }
}
