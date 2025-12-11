//! Error handling for nh3
//!
//! This module defines error types used throughout the HTTP/3 implementation,
//! providing both native Rust errors and nghttp3-compatible error codes.

use std::fmt;

// ============================================================================
// Error Codes (nghttp3 compatible)
// ============================================================================

/// nghttp3 compatible error codes
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// No error
    NoError = 0,
    /// Invalid argument passed
    InvalidArgument = -101,
    /// Buffer too small
    NoBuf = -102,
    /// Invalid state
    InvalidState = -103,
    /// Operation would block
    WouldBlock = -104,
    /// Stream already in use
    StreamInUse = -105,
    /// Push ID blocked
    PushIdBlocked = -106,
    /// Malformed HTTP header
    MalformedHttpHeader = -107,
    /// Required header removed
    RemoveHttpHeader = -108,
    /// Malformed HTTP messaging
    MalformedHttpMessaging = -109,
    /// Fatal QPACK error
    QpackFatal = -110,
    /// QPACK header too large
    QpackHeaderTooLarge = -111,
    /// Stream should be ignored
    IgnoreStream = -112,
    /// H3: Unexpected frame
    H3FrameUnexpected = -113,
    /// H3: Frame error
    H3FrameError = -114,
    /// H3: Missing settings
    H3MissingSettings = -115,
    /// H3: Internal error
    H3InternalError = -116,
    /// H3: Closed critical stream
    H3ClosedCriticalStream = -117,
    /// H3: General protocol error
    H3GeneralProtocolError = -118,
    /// H3: ID error
    H3IdError = -119,
    /// H3: Settings error
    H3SettingsError = -120,
    /// H3: Stream creation error
    H3StreamCreationError = -121,
    /// Fatal error
    Fatal = -501,
    /// Out of memory
    NoMem = -502,
    /// Callback failure
    CallbackFailure = -503,
}

impl ErrorCode {
    /// Check if this error is fatal
    pub fn is_fatal(self) -> bool {
        (self as i32) < -500
    }
    
    /// Convert to i32
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

impl From<i32> for ErrorCode {
    fn from(code: i32) -> Self {
        match code {
            0 => ErrorCode::NoError,
            -101 => ErrorCode::InvalidArgument,
            -102 => ErrorCode::NoBuf,
            -103 => ErrorCode::InvalidState,
            -104 => ErrorCode::WouldBlock,
            -105 => ErrorCode::StreamInUse,
            -106 => ErrorCode::PushIdBlocked,
            -107 => ErrorCode::MalformedHttpHeader,
            -108 => ErrorCode::RemoveHttpHeader,
            -109 => ErrorCode::MalformedHttpMessaging,
            -110 => ErrorCode::QpackFatal,
            -111 => ErrorCode::QpackHeaderTooLarge,
            -112 => ErrorCode::IgnoreStream,
            -113 => ErrorCode::H3FrameUnexpected,
            -114 => ErrorCode::H3FrameError,
            -115 => ErrorCode::H3MissingSettings,
            -116 => ErrorCode::H3InternalError,
            -117 => ErrorCode::H3ClosedCriticalStream,
            -118 => ErrorCode::H3GeneralProtocolError,
            -119 => ErrorCode::H3IdError,
            -120 => ErrorCode::H3SettingsError,
            -121 => ErrorCode::H3StreamCreationError,
            -501 => ErrorCode::Fatal,
            -502 => ErrorCode::NoMem,
            -503 => ErrorCode::CallbackFailure,
            _ => ErrorCode::Fatal,
        }
    }
}

// ============================================================================
// Native Error Type
// ============================================================================

/// Native Rust error type for nh3
#[derive(Debug)]
pub enum Error {
    /// nghttp3-compatible error
    NgError(ErrorCode),
    /// HTTP/3 protocol error
    H3Error(H3Error),
    /// QPACK error
    QpackError(QpackError),
    /// I/O error
    IoError(std::io::Error),
    /// QUIC layer error (from ntcp2)
    QuicError(i32),
}

impl Error {
    /// Create from error code
    pub fn from_code(code: ErrorCode) -> Self {
        Error::NgError(code)
    }
    
    /// Get the error code
    pub fn code(&self) -> i32 {
        match self {
            Error::NgError(e) => *e as i32,
            Error::H3Error(e) => e.code() as i32,
            Error::QpackError(e) => e.code() as i32,
            Error::IoError(_) => ErrorCode::Fatal as i32,
            Error::QuicError(e) => *e,
        }
    }
    
    /// Check if this error is fatal
    pub fn is_fatal(&self) -> bool {
        match self {
            Error::NgError(e) => e.is_fatal(),
            Error::H3Error(_) => true,
            Error::QpackError(e) => e.is_fatal(),
            Error::IoError(_) => true,
            Error::QuicError(_) => true,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NgError(e) => write!(f, "nghttp3 error: {:?}", e),
            Error::H3Error(e) => write!(f, "HTTP/3 error: {:?}", e),
            Error::QpackError(e) => write!(f, "QPACK error: {:?}", e),
            Error::IoError(e) => write!(f, "I/O error: {}", e),
            Error::QuicError(e) => write!(f, "QUIC error: {}", e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<ErrorCode> for Error {
    fn from(e: ErrorCode) -> Self {
        Error::NgError(e)
    }
}

// ============================================================================
// HTTP/3 Specific Errors
// ============================================================================

/// HTTP/3 protocol errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H3Error {
    /// No error
    NoError,
    /// General protocol error
    GeneralProtocolError,
    /// Internal error
    InternalError,
    /// Stream creation error
    StreamCreationError,
    /// Closed critical stream
    ClosedCriticalStream,
    /// Unexpected frame
    FrameUnexpected,
    /// Frame error
    FrameError,
    /// Excessive load
    ExcessiveLoad,
    /// ID error
    IdError,
    /// Settings error
    SettingsError,
    /// Missing settings
    MissingSettings,
    /// Request rejected
    RequestRejected,
    /// Request cancelled
    RequestCancelled,
    /// Request incomplete
    RequestIncomplete,
    /// Message error
    MessageError,
    /// Connect error
    ConnectError,
    /// Version fallback
    VersionFallback,
}

impl H3Error {
    /// Get the HTTP/3 error code value
    pub fn code(self) -> u64 {
        use crate::constants::h3_error::*;
        match self {
            H3Error::NoError => H3_NO_ERROR,
            H3Error::GeneralProtocolError => H3_GENERAL_PROTOCOL_ERROR,
            H3Error::InternalError => H3_INTERNAL_ERROR,
            H3Error::StreamCreationError => H3_STREAM_CREATION_ERROR,
            H3Error::ClosedCriticalStream => H3_CLOSED_CRITICAL_STREAM,
            H3Error::FrameUnexpected => H3_FRAME_UNEXPECTED,
            H3Error::FrameError => H3_FRAME_ERROR,
            H3Error::ExcessiveLoad => H3_EXCESSIVE_LOAD,
            H3Error::IdError => H3_ID_ERROR,
            H3Error::SettingsError => H3_SETTINGS_ERROR,
            H3Error::MissingSettings => H3_MISSING_SETTINGS,
            H3Error::RequestRejected => H3_REQUEST_REJECTED,
            H3Error::RequestCancelled => H3_REQUEST_CANCELLED,
            H3Error::RequestIncomplete => H3_REQUEST_INCOMPLETE,
            H3Error::MessageError => H3_MESSAGE_ERROR,
            H3Error::ConnectError => H3_CONNECT_ERROR,
            H3Error::VersionFallback => H3_VERSION_FALLBACK,
        }
    }
}

// ============================================================================
// QPACK Errors
// ============================================================================

/// QPACK errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QpackError {
    /// Decompression failed
    DecompressionFailed,
    /// Encoder stream error
    EncoderStreamError,
    /// Decoder stream error
    DecoderStreamError,
    /// Header too large
    HeaderTooLarge,
    /// Invalid static table index
    InvalidStaticIndex,
    /// Invalid dynamic table index
    InvalidDynamicIndex,
    /// Table capacity exceeded
    TableCapacityExceeded,
    /// Blocked stream limit exceeded
    BlockedStreamLimit,
}

impl QpackError {
    /// Get the error code
    pub fn code(self) -> i32 {
        match self {
            QpackError::DecompressionFailed => ErrorCode::QpackFatal as i32,
            QpackError::EncoderStreamError => ErrorCode::QpackFatal as i32,
            QpackError::DecoderStreamError => ErrorCode::QpackFatal as i32,
            QpackError::HeaderTooLarge => ErrorCode::QpackHeaderTooLarge as i32,
            QpackError::InvalidStaticIndex => ErrorCode::QpackFatal as i32,
            QpackError::InvalidDynamicIndex => ErrorCode::QpackFatal as i32,
            QpackError::TableCapacityExceeded => ErrorCode::QpackFatal as i32,
            QpackError::BlockedStreamLimit => ErrorCode::QpackFatal as i32,
        }
    }
    
    /// Check if this error is fatal
    pub fn is_fatal(self) -> bool {
        true // All QPACK errors are fatal in HTTP/3
    }
}

impl From<QpackError> for Error {
    fn from(e: QpackError) -> Self {
        Error::QpackError(e)
    }
}

impl From<H3Error> for Error {
    fn from(e: H3Error) -> Self {
        Error::H3Error(e)
    }
}

// ============================================================================
// nghttp3-compatible error alias
// ============================================================================

/// nghttp3 compatible error type alias
pub type NgError = ErrorCode;

/// Result type for nh3 operations
pub type Result<T> = std::result::Result<T, Error>;
