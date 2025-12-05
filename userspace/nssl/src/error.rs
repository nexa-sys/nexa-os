//! Error Handling
//!
//! SSL/TLS error codes and error queue management.

use std::vec::Vec;
use std::sync::Mutex;
use std::collections::VecDeque;
use crate::{c_char, c_ulong, size_t};

/// SSL Error type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SslError {
    None,
    Ssl,
    WantRead,
    WantWrite,
    WantX509Lookup,
    Syscall,
    ZeroReturn,
    WantConnect,
    WantAccept,
    Internal,
    InvalidArgument,
    CertificateRequired,
    ProtocolVersion,
    HandshakeFailure,
    BadCertificate,
}

impl SslError {
    pub fn code(&self) -> i32 {
        match self {
            Self::None => 0,
            Self::Ssl => 1,
            Self::WantRead => 2,
            Self::WantWrite => 3,
            Self::WantX509Lookup => 4,
            Self::Syscall => 5,
            Self::ZeroReturn => 6,
            Self::WantConnect => 7,
            Self::WantAccept => 8,
            Self::Internal => 80,
            Self::InvalidArgument => 103,
            Self::CertificateRequired => 116,
            Self::ProtocolVersion => 70,
            Self::HandshakeFailure => 40,
            Self::BadCertificate => 42,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::None => "no error",
            Self::Ssl => "SSL error",
            Self::WantRead => "want read",
            Self::WantWrite => "want write",
            Self::WantX509Lookup => "want X509 lookup",
            Self::Syscall => "system call error",
            Self::ZeroReturn => "connection closed",
            Self::WantConnect => "want connect",
            Self::WantAccept => "want accept",
            Self::Internal => "internal error",
            Self::InvalidArgument => "invalid argument",
            Self::CertificateRequired => "certificate required",
            Self::ProtocolVersion => "unsupported protocol version",
            Self::HandshakeFailure => "handshake failure",
            Self::BadCertificate => "bad certificate",
        }
    }
}

/// Result type for SSL operations
pub type SslResult<T> = Result<T, SslError>;

/// Error entry in the error queue
#[derive(Clone)]
struct ErrorEntry {
    /// Error code
    code: u64,
    /// Library code (SSL = 0x14)
    lib: u8,
    /// Function code
    func: u16,
    /// Reason code
    reason: u16,
    /// File name
    file: &'static str,
    /// Line number
    line: u32,
}

impl ErrorEntry {
    fn new(reason: u16) -> Self {
        Self {
            code: pack_error(0x14, 0, reason),
            lib: 0x14, // SSL library
            func: 0,
            reason,
            file: "",
            line: 0,
        }
    }

    fn with_location(mut self, file: &'static str, line: u32) -> Self {
        self.file = file;
        self.line = line;
        self
    }
}

/// Pack error into OpenSSL-compatible format
fn pack_error(lib: u8, func: u16, reason: u16) -> u64 {
    ((lib as u64) << 24) | ((func as u64) << 12) | (reason as u64)
}

/// Unpack error code
fn unpack_error(code: u64) -> (u8, u16, u16) {
    let lib = ((code >> 24) & 0xFF) as u8;
    let func = ((code >> 12) & 0xFFF) as u16;
    let reason = (code & 0xFFF) as u16;
    (lib, func, reason)
}

/// Thread-local error queue
static ERROR_QUEUE: Mutex<VecDeque<ErrorEntry>> = Mutex::new(VecDeque::new());

/// Push error to queue
pub fn push_error(error: SslError) {
    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        queue.push_back(ErrorEntry::new(error.code() as u16));
    }
}

/// Push error with location
pub fn push_error_with_location(error: SslError, file: &'static str, line: u32) {
    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        queue.push_back(ErrorEntry::new(error.code() as u16).with_location(file, line));
    }
}

/// Get and remove first error from queue
pub fn get_error() -> c_ulong {
    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        queue.pop_front().map(|e| e.code).unwrap_or(0) as c_ulong
    } else {
        0
    }
}

/// Peek at first error without removing
pub fn peek_error() -> c_ulong {
    if let Ok(queue) = ERROR_QUEUE.lock() {
        queue.front().map(|e| e.code).unwrap_or(0) as c_ulong
    } else {
        0
    }
}

/// Clear error queue
pub fn clear_error() {
    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        queue.clear();
    }
}

/// Get error string
pub fn error_string(e: c_ulong, buf: *mut c_char) -> *mut c_char {
    let (lib, _func, reason) = unpack_error(e as u64);
    
    let msg = if lib == 0x14 {
        // SSL library error
        match reason {
            0 => "no error",
            1 => "SSL error",
            2 => "want read",
            3 => "want write",
            4 => "want X509 lookup",
            5 => "system call error",
            6 => "connection closed",
            7 => "want connect",
            8 => "want accept",
            40 => "handshake failure",
            42 => "bad certificate",
            70 => "unsupported protocol version",
            80 => "internal error",
            103 => "invalid argument",
            116 => "certificate required",
            _ => "unknown error",
        }
    } else {
        "unknown library error"
    };
    
    if buf.is_null() {
        // Return static string
        return msg.as_ptr() as *mut c_char;
    }
    
    // Copy to buffer
    let bytes = msg.as_bytes();
    unsafe {
        let len = bytes.len().min(255);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, len);
        *((buf as *mut u8).add(len)) = 0;
    }
    
    buf
}

/// Get error string with length limit
pub fn error_string_n(e: c_ulong, buf: *mut c_char, len: size_t) {
    if buf.is_null() || len == 0 {
        return;
    }
    
    let (lib, _func, reason) = unpack_error(e as u64);
    
    let msg = if lib == 0x14 {
        match reason {
            0 => "no error",
            1 => "SSL error",
            2 => "want read",
            3 => "want write",
            4 => "want X509 lookup",
            5 => "system call error",
            6 => "connection closed",
            7 => "want connect",
            8 => "want accept",
            40 => "handshake failure",
            42 => "bad certificate",
            70 => "unsupported protocol version",
            80 => "internal error",
            103 => "invalid argument",
            116 => "certificate required",
            _ => "unknown error",
        }
    } else {
        "unknown library error"
    };
    
    let bytes = msg.as_bytes();
    let copy_len = (len - 1).min(bytes.len());
    
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, copy_len);
        *((buf as *mut u8).add(copy_len)) = 0;
    }
}

/// Print all errors (to stderr)
pub fn print_errors() {
    if let Ok(mut queue) = ERROR_QUEUE.lock() {
        while let Some(entry) = queue.pop_front() {
            let (lib, func, reason) = unpack_error(entry.code);
            eprintln!("error:{}:{}:{}:{}", lib, func, reason, entry.line);
        }
    }
}

/// Error reason codes
pub mod reason {
    pub const SSL_R_NO_PROTOCOLS_AVAILABLE: u16 = 191;
    pub const SSL_R_NO_SHARED_CIPHER: u16 = 193;
    pub const SSL_R_CERTIFICATE_VERIFY_FAILED: u16 = 134;
    pub const SSL_R_WRONG_VERSION_NUMBER: u16 = 267;
    pub const SSL_R_UNEXPECTED_MESSAGE: u16 = 244;
    pub const SSL_R_BAD_RECORD_MAC: u16 = 20;
    pub const SSL_R_DECRYPTION_FAILED: u16 = 112;
    pub const SSL_R_UNKNOWN_PROTOCOL: u16 = 252;
    pub const SSL_R_PEER_DID_NOT_RETURN_A_CERTIFICATE: u16 = 199;
    pub const SSL_R_CERTIFICATE_REQUIRED: u16 = 116;
    pub const SSL_R_NO_CERTIFICATE_SET: u16 = 179;
    pub const SSL_R_BAD_CERTIFICATE: u16 = 42;
    pub const SSL_R_UNKNOWN_CERTIFICATE_TYPE: u16 = 247;
    pub const SSL_R_UNSUPPORTED_PROTOCOL: u16 = 258;
    pub const SSL_R_HANDSHAKE_FAILURE_ON_CLIENT_HELLO: u16 = 164;
    pub const SSL_R_TLSV1_ALERT_PROTOCOL_VERSION: u16 = 1070;
    pub const SSL_R_TLSV1_ALERT_HANDSHAKE_FAILURE: u16 = 1040;
}

/// Macro to push error with file/line info
#[macro_export]
macro_rules! ssl_error {
    ($err:expr) => {
        $crate::error::push_error_with_location($err, file!(), line!())
    };
}
