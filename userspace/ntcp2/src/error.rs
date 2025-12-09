//! Error types for ntcp2
//!
//! This module provides error handling compatible with ngtcp2's error codes.

use core::fmt;

/// Result type alias using ntcp2 Error
pub type Result<T> = core::result::Result<T, Error>;

// ============================================================================
// QUIC Transport Error Codes (RFC 9000 Section 20)
// ============================================================================

/// QUIC transport error codes
/// 
/// Note: Values 0x00-0x10 are standard QUIC transport errors.
/// Values 0x100-0x1FF are TLS crypto errors (0x100 | TLS alert code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// No error
    NoError,
    /// Internal error
    InternalError,
    /// Connection refused
    ConnectionRefused,
    /// Flow control error
    FlowControlError,
    /// Stream limit error
    StreamLimitError,
    /// Stream state error
    StreamStateError,
    /// Final size error
    FinalSizeError,
    /// Frame encoding error
    FrameEncodingError,
    /// Transport parameter error
    TransportParameterError,
    /// Connection ID limit error
    ConnectionIdLimitError,
    /// Protocol violation
    ProtocolViolation,
    /// Invalid token
    InvalidToken,
    /// Application error
    ApplicationError,
    /// Crypto buffer exceeded
    CryptoBufferExceeded,
    /// Key update error
    KeyUpdateError,
    /// AEAD limit reached
    AeadLimitReached,
    /// No viable path
    NoViablePath,
    /// Crypto error (0x1XX range, where XX is TLS alert)
    CryptoError(u8),
}

impl TransportError {
    /// Convert from u64
    pub fn from_u64(code: u64) -> Self {
        if code >= 0x100 && code < 0x200 {
            return TransportError::CryptoError((code & 0xff) as u8);
        }
        
        match code {
            0x00 => TransportError::NoError,
            0x01 => TransportError::InternalError,
            0x02 => TransportError::ConnectionRefused,
            0x03 => TransportError::FlowControlError,
            0x04 => TransportError::StreamLimitError,
            0x05 => TransportError::StreamStateError,
            0x06 => TransportError::FinalSizeError,
            0x07 => TransportError::FrameEncodingError,
            0x08 => TransportError::TransportParameterError,
            0x09 => TransportError::ConnectionIdLimitError,
            0x0a => TransportError::ProtocolViolation,
            0x0b => TransportError::InvalidToken,
            0x0c => TransportError::ApplicationError,
            0x0d => TransportError::CryptoBufferExceeded,
            0x0e => TransportError::KeyUpdateError,
            0x0f => TransportError::AeadLimitReached,
            0x10 => TransportError::NoViablePath,
            _ => TransportError::InternalError,
        }
    }
    
    /// Convert to u64
    pub fn as_u64(self) -> u64 {
        match self {
            TransportError::NoError => 0x00,
            TransportError::InternalError => 0x01,
            TransportError::ConnectionRefused => 0x02,
            TransportError::FlowControlError => 0x03,
            TransportError::StreamLimitError => 0x04,
            TransportError::StreamStateError => 0x05,
            TransportError::FinalSizeError => 0x06,
            TransportError::FrameEncodingError => 0x07,
            TransportError::TransportParameterError => 0x08,
            TransportError::ConnectionIdLimitError => 0x09,
            TransportError::ProtocolViolation => 0x0a,
            TransportError::InvalidToken => 0x0b,
            TransportError::ApplicationError => 0x0c,
            TransportError::CryptoBufferExceeded => 0x0d,
            TransportError::KeyUpdateError => 0x0e,
            TransportError::AeadLimitReached => 0x0f,
            TransportError::NoViablePath => 0x10,
            TransportError::CryptoError(alert) => 0x100 | (alert as u64),
        }
    }
}

// ============================================================================
// ngtcp2-compatible Error Codes (negative values)
// ============================================================================

/// ngtcp2-compatible error codes
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NgError {
    /// Invalid argument
    InvalidArgument = -201,
    /// Buffer overflow / no buffer space
    NoBuf = -202,
    /// Protocol error
    Proto = -203,
    /// Invalid state
    InvalidState = -204,
    /// ACK frame error
    AckFrame = -205,
    /// Stream ID blocked
    StreamIdBlocked = -206,
    /// Stream in use
    StreamInUse = -207,
    /// Stream data blocked
    StreamDataBlocked = -208,
    /// Flow control error
    FlowControl = -209,
    /// Connection ID limit error
    ConnectionIdLimit = -210,
    /// Stream limit error
    StreamLimit = -211,
    /// Final size error
    FinalSize = -212,
    /// Crypto error
    Crypto = -213,
    /// Packet number exhausted
    PktNumExhausted = -214,
    /// Required transport parameter missing
    RequiredTransportParam = -215,
    /// Malformed transport parameter
    MalformedTransportParam = -216,
    /// Frame encoding error
    FrameEncoding = -217,
    /// Decrypt failure
    Decrypt = -218,
    /// Stream write shutdown
    StreamShutWr = -219,
    /// Stream not found
    StreamNotFound = -220,
    /// Stream state error
    StreamState = -221,
    /// No key
    NoKey = -222,
    /// Early data rejected
    EarlyDataRejected = -223,
    /// Retry required
    Retry = -224,
    /// Dropping packet
    DropConn = -225,
    /// Application close error
    ApplicationClose = -226,
    /// Version negotiation required
    VersionNegotiation = -227,
    /// Fatal error
    Fatal = -501,
    /// Memory allocation failure
    NoMem = -502,
    /// Callback failure
    CallbackFailure = -503,
}

impl NgError {
    /// Get error code as i32
    pub fn as_i32(self) -> i32 {
        self as i32
    }
    
    /// Check if error is fatal
    pub fn is_fatal(self) -> bool {
        (self as i32) < -500
    }
}

// ============================================================================
// Crypto Error
// ============================================================================

/// Cryptographic operation errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    /// Invalid key length
    InvalidKeyLength,
    /// Invalid nonce length
    InvalidNonceLength,
    /// Encryption failed
    EncryptionFailed,
    /// Decryption failed
    DecryptionFailed,
    /// Invalid tag
    InvalidTag,
    /// Buffer too small
    BufferTooSmall,
    /// Key derivation failed
    KeyDerivationFailed,
    /// Invalid header protection sample
    InvalidSample,
    /// TLS handshake error
    TlsError,
    /// Certificate error
    CertificateError,
    /// Header protection error
    HeaderProtection,
    /// Encryption error (alias for EncryptionFailed)
    Encryption,
    /// Decryption error (alias for DecryptionFailed)
    Decryption,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoError::InvalidKeyLength => write!(f, "invalid key length"),
            CryptoError::InvalidNonceLength => write!(f, "invalid nonce length"),
            CryptoError::EncryptionFailed => write!(f, "encryption failed"),
            CryptoError::DecryptionFailed => write!(f, "decryption failed"),
            CryptoError::InvalidTag => write!(f, "invalid authentication tag"),
            CryptoError::BufferTooSmall => write!(f, "buffer too small"),
            CryptoError::KeyDerivationFailed => write!(f, "key derivation failed"),
            CryptoError::InvalidSample => write!(f, "invalid header protection sample"),
            CryptoError::TlsError => write!(f, "TLS handshake error"),
            CryptoError::CertificateError => write!(f, "certificate error"),
            CryptoError::HeaderProtection => write!(f, "header protection error"),
            CryptoError::Encryption => write!(f, "encryption error"),
            CryptoError::Decryption => write!(f, "decryption error"),
        }
    }
}

// ============================================================================
// Main Error Type
// ============================================================================

/// Main error type for ntcp2
#[derive(Debug, Clone)]
pub enum Error {
    /// Transport error (QUIC protocol level)
    Transport(TransportError),
    /// ngtcp2 API error
    Ng(NgError),
    /// Cryptographic error
    Crypto(CryptoError),
    /// I/O error
    Io(String),
    /// Buffer too small
    BufferTooSmall,
    /// Invalid argument
    InvalidArgument,
    /// Connection closed
    ConnectionClosed,
    /// Stream closed
    StreamClosed,
    /// Timeout
    Timeout,
    /// Would block (non-blocking operation)
    WouldBlock,
    /// Custom error message
    Custom(String),
}

impl Error {
    /// Create a transport error
    pub fn transport(err: TransportError) -> Self {
        Error::Transport(err)
    }
    
    /// Create an ngtcp2 API error
    pub fn ng(err: NgError) -> Self {
        Error::Ng(err)
    }
    
    /// Create a crypto error
    pub fn crypto(err: CryptoError) -> Self {
        Error::Crypto(err)
    }
    
    /// Create an I/O error
    pub fn io(msg: impl Into<String>) -> Self {
        Error::Io(msg.into())
    }
    
    /// Create a custom error
    pub fn custom(msg: impl Into<String>) -> Self {
        Error::Custom(msg.into())
    }
    
    /// Check if error is fatal
    pub fn is_fatal(&self) -> bool {
        match self {
            Error::Ng(e) => e.is_fatal(),
            Error::ConnectionClosed => true,
            _ => false,
        }
    }
    
    /// Convert to ngtcp2 error code
    pub fn as_error_code(&self) -> i32 {
        match self {
            Error::Transport(_) => NgError::Proto.as_i32(),
            Error::Ng(e) => e.as_i32(),
            Error::Crypto(_) => NgError::Crypto.as_i32(),
            Error::Io(_) => NgError::CallbackFailure.as_i32(),
            Error::BufferTooSmall => NgError::NoBuf.as_i32(),
            Error::InvalidArgument => NgError::InvalidArgument.as_i32(),
            Error::ConnectionClosed => NgError::DropConn.as_i32(),
            Error::StreamClosed => NgError::StreamNotFound.as_i32(),
            Error::Timeout => NgError::InvalidState.as_i32(),
            Error::WouldBlock => NgError::InvalidState.as_i32(),
            Error::Custom(_) => NgError::CallbackFailure.as_i32(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Transport(e) => write!(f, "QUIC transport error: 0x{:x}", e.as_u64()),
            Error::Ng(e) => write!(f, "ngtcp2 error: {}", e.as_i32()),
            Error::Crypto(e) => write!(f, "crypto error: {}", e),
            Error::Io(msg) => write!(f, "I/O error: {}", msg),
            Error::BufferTooSmall => write!(f, "buffer too small"),
            Error::InvalidArgument => write!(f, "invalid argument"),
            Error::ConnectionClosed => write!(f, "connection closed"),
            Error::StreamClosed => write!(f, "stream closed"),
            Error::Timeout => write!(f, "operation timed out"),
            Error::WouldBlock => write!(f, "operation would block"),
            Error::Custom(msg) => write!(f, "{}", msg),
        }
    }
}

impl From<TransportError> for Error {
    fn from(err: TransportError) -> Self {
        Error::Transport(err)
    }
}

impl From<NgError> for Error {
    fn from(err: NgError) -> Self {
        Error::Ng(err)
    }
}

impl From<CryptoError> for Error {
    fn from(err: CryptoError) -> Self {
        Error::Crypto(err)
    }
}

// ============================================================================
// TLS Alert Codes (RFC 8446)
// ============================================================================

/// TLS alert codes used in CRYPTO_ERROR
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsAlert {
    /// Close notify
    CloseNotify = 0,
    /// Unexpected message
    UnexpectedMessage = 10,
    /// Bad record MAC
    BadRecordMac = 20,
    /// Record overflow
    RecordOverflow = 22,
    /// Handshake failure
    HandshakeFailure = 40,
    /// Bad certificate
    BadCertificate = 42,
    /// Unsupported certificate
    UnsupportedCertificate = 43,
    /// Certificate revoked
    CertificateRevoked = 44,
    /// Certificate expired
    CertificateExpired = 45,
    /// Certificate unknown
    CertificateUnknown = 46,
    /// Illegal parameter
    IllegalParameter = 47,
    /// Unknown CA
    UnknownCa = 48,
    /// Access denied
    AccessDenied = 49,
    /// Decode error
    DecodeError = 50,
    /// Decrypt error
    DecryptError = 51,
    /// Protocol version
    ProtocolVersion = 70,
    /// Insufficient security
    InsufficientSecurity = 71,
    /// Internal error
    InternalError = 80,
    /// Inappropriate fallback
    InappropriateFallback = 86,
    /// User canceled
    UserCanceled = 90,
    /// Missing extension
    MissingExtension = 109,
    /// Unsupported extension
    UnsupportedExtension = 110,
    /// Unrecognized name
    UnrecognizedName = 112,
    /// Bad certificate status response
    BadCertificateStatusResponse = 113,
    /// Unknown PSK identity
    UnknownPskIdentity = 115,
    /// Certificate required
    CertificateRequired = 116,
    /// No application protocol
    NoApplicationProtocol = 120,
}

impl TlsAlert {
    /// Convert to QUIC transport error code
    pub fn to_transport_error(self) -> TransportError {
        TransportError::CryptoError(self as u8)
    }
    
    /// Convert alert code to u64 error code
    pub fn as_error_code(self) -> u64 {
        0x100 | (self as u64)
    }
}
