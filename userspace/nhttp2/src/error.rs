//! Error types for nhttp2
//!
//! This module provides error handling compatible with nghttp2's error codes.

use core::fmt;

/// Result type alias using nhttp2 Error
pub type Result<T> = core::result::Result<T, Error>;

/// HTTP/2 error codes (RFC 7540 Section 7)
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// No error
    NoError = 0x0,
    /// Protocol error detected
    ProtocolError = 0x1,
    /// Internal error
    InternalError = 0x2,
    /// Flow control error
    FlowControlError = 0x3,
    /// Settings timeout
    SettingsTimeout = 0x4,
    /// Stream closed
    StreamClosed = 0x5,
    /// Frame size error
    FrameSizeError = 0x6,
    /// Refused stream
    RefusedStream = 0x7,
    /// Cancel
    Cancel = 0x8,
    /// Compression error
    CompressionError = 0x9,
    /// Connect error
    ConnectError = 0xa,
    /// Enhance your calm
    EnhanceYourCalm = 0xb,
    /// Inadequate security
    InadequateSecurity = 0xc,
    /// HTTP/1.1 required
    Http11Required = 0xd,
}

impl ErrorCode {
    /// Convert from u32
    pub fn from_u32(code: u32) -> Self {
        match code {
            0x0 => ErrorCode::NoError,
            0x1 => ErrorCode::ProtocolError,
            0x2 => ErrorCode::InternalError,
            0x3 => ErrorCode::FlowControlError,
            0x4 => ErrorCode::SettingsTimeout,
            0x5 => ErrorCode::StreamClosed,
            0x6 => ErrorCode::FrameSizeError,
            0x7 => ErrorCode::RefusedStream,
            0x8 => ErrorCode::Cancel,
            0x9 => ErrorCode::CompressionError,
            0xa => ErrorCode::ConnectError,
            0xb => ErrorCode::EnhanceYourCalm,
            0xc => ErrorCode::InadequateSecurity,
            0xd => ErrorCode::Http11Required,
            _ => ErrorCode::InternalError,
        }
    }
}

/// nghttp2-compatible error codes (negative values)
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NgError {
    /// Invalid argument
    InvalidArgument = -501,
    /// Buffer overflow
    BufferError = -502,
    /// Unsupported version
    UnsupportedVersion = -503,
    /// Would block (non-blocking I/O)
    WouldBlock = -504,
    /// Protocol error
    Proto = -505,
    /// Invalid frame
    InvalidFrame = -506,
    /// End of file
    Eof = -507,
    /// Deferred
    Deferred = -508,
    /// Stream ID not available
    StreamIdNotAvailable = -509,
    /// Stream closed
    StreamClosed = -510,
    /// Stream closing
    StreamClosing = -511,
    /// Stream shut write
    StreamShutWr = -512,
    /// Invalid stream ID
    InvalidStreamId = -513,
    /// Invalid stream state
    InvalidStreamState = -514,
    /// Data exists
    DataExist = -515,
    /// Push disabled
    PushDisabled = -516,
    /// Too many inflight settings
    TooManyInflightSettings = -517,
    /// Invalid header block
    InvalidHeaderBlock = -518,
    /// Flow control
    FlowControl = -519,
    /// Header compression failure
    HeaderComp = -520,
    /// Settings expected
    SettingsExpected = -521,
    /// Internal error
    Internal = -522,
    /// Cancel
    Cancel = -523,
    /// No memory
    NoMem = -524,
    /// Callback failure
    CallbackFailure = -525,
    /// Bad client magic
    BadClientMagic = -526,
    /// Flooded
    Flooded = -527,
    /// HTTP header
    HttpHeader = -528,
    /// HTTP messaging
    HttpMessaging = -529,
    /// Refused stream
    RefusedStream = -530,
    /// Fatal error
    Fatal = -900,
}

impl NgError {
    /// Convert to i32
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// Library error type
#[derive(Debug)]
pub enum Error {
    /// HTTP/2 protocol error
    Protocol(ErrorCode),
    /// nghttp2 library error
    Library(NgError),
    /// HPACK error
    Hpack(HpackError),
    /// I/O error
    Io(IoError),
    /// TLS error
    Tls(TlsError),
    /// Invalid state
    InvalidState(&'static str),
    /// Buffer too small
    BufferTooSmall,
    /// Connection closed
    ConnectionClosed,
    /// Stream not found
    StreamNotFound(i32),
    /// Internal error
    Internal(&'static str),
}

impl Error {
    /// Convert to nghttp2-compatible error code
    pub fn to_error_code(&self) -> i32 {
        match self {
            Error::Protocol(ec) => *ec as i32,
            Error::Library(ne) => *ne as i32,
            Error::Hpack(_) => NgError::HeaderComp as i32,
            Error::Io(_) => NgError::CallbackFailure as i32,
            Error::Tls(_) => NgError::Proto as i32,
            Error::InvalidState(_) => NgError::InvalidStreamState as i32,
            Error::BufferTooSmall => NgError::BufferError as i32,
            Error::ConnectionClosed => NgError::Eof as i32,
            Error::StreamNotFound(_) => NgError::InvalidStreamId as i32,
            Error::Internal(_) => NgError::Internal as i32,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Protocol(ec) => write!(f, "HTTP/2 protocol error: {:?}", ec),
            Error::Library(ne) => write!(f, "Library error: {:?}", ne),
            Error::Hpack(he) => write!(f, "HPACK error: {:?}", he),
            Error::Io(ie) => write!(f, "I/O error: {:?}", ie),
            Error::Tls(te) => write!(f, "TLS error: {:?}", te),
            Error::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
            Error::BufferTooSmall => write!(f, "Buffer too small"),
            Error::ConnectionClosed => write!(f, "Connection closed"),
            Error::StreamNotFound(id) => write!(f, "Stream {} not found", id),
            Error::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

/// HPACK error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HpackError {
    /// Invalid index
    InvalidIndex,
    /// Header too large
    HeaderTooLarge,
    /// Invalid encoding
    InvalidEncoding,
    /// Huffman decode error
    HuffmanDecode,
    /// Integer overflow
    IntegerOverflow,
    /// Table size exceeded
    TableSizeExceeded,
}

/// I/O error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoError {
    /// Would block
    WouldBlock,
    /// Connection reset
    ConnectionReset,
    /// Connection refused
    ConnectionRefused,
    /// Broken pipe
    BrokenPipe,
    /// Timeout
    Timeout,
    /// Other
    Other,
}

/// TLS error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsError {
    /// Handshake failed
    HandshakeFailed,
    /// Certificate error
    CertificateError,
    /// Protocol error
    ProtocolError,
    /// ALPN negotiation failed
    AlpnFailed,
    /// Other
    Other,
}

// ============================================================================
// C API Error String Functions
// ============================================================================

/// Get error string for nghttp2 error code
#[no_mangle]
pub extern "C" fn nghttp2_strerror(error_code: i32) -> *const i8 {
    let msg: &'static [u8] = match error_code {
        0 => b"Success\0",
        -501 => b"Invalid argument\0",
        -502 => b"Buffer error\0",
        -503 => b"Unsupported version\0",
        -504 => b"Would block\0",
        -505 => b"Protocol error\0",
        -506 => b"Invalid frame\0",
        -507 => b"EOF\0",
        -508 => b"Deferred\0",
        -509 => b"Stream ID not available\0",
        -510 => b"Stream closed\0",
        -511 => b"Stream closing\0",
        -512 => b"Stream shut write\0",
        -513 => b"Invalid stream ID\0",
        -514 => b"Invalid stream state\0",
        -515 => b"Data exist\0",
        -516 => b"Push disabled\0",
        -517 => b"Too many inflight settings\0",
        -518 => b"Invalid header block\0",
        -519 => b"Flow control\0",
        -520 => b"Header compression failure\0",
        -521 => b"Settings expected\0",
        -522 => b"Internal error\0",
        -523 => b"Cancel\0",
        -524 => b"No memory\0",
        -525 => b"Callback failure\0",
        -526 => b"Bad client magic\0",
        -527 => b"Flooded\0",
        -528 => b"HTTP header error\0",
        -529 => b"HTTP messaging error\0",
        -530 => b"Refused stream\0",
        -900 => b"Fatal error\0",
        _ => b"Unknown error\0",
    };
    msg.as_ptr() as *const i8
}

/// Get HTTP/2 error code string
#[no_mangle]
pub extern "C" fn nghttp2_http2_strerror(error_code: u32) -> *const i8 {
    let msg: &'static [u8] = match error_code {
        0x0 => b"NO_ERROR\0",
        0x1 => b"PROTOCOL_ERROR\0",
        0x2 => b"INTERNAL_ERROR\0",
        0x3 => b"FLOW_CONTROL_ERROR\0",
        0x4 => b"SETTINGS_TIMEOUT\0",
        0x5 => b"STREAM_CLOSED\0",
        0x6 => b"FRAME_SIZE_ERROR\0",
        0x7 => b"REFUSED_STREAM\0",
        0x8 => b"CANCEL\0",
        0x9 => b"COMPRESSION_ERROR\0",
        0xa => b"CONNECT_ERROR\0",
        0xb => b"ENHANCE_YOUR_CALM\0",
        0xc => b"INADEQUATE_SECURITY\0",
        0xd => b"HTTP_1_1_REQUIRED\0",
        _ => b"UNKNOWN\0",
    };
    msg.as_ptr() as *const i8
}
