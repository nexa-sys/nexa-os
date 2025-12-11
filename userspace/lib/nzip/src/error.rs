//! Error types for nzip library

use crate::{c_int, Z_BUF_ERROR, Z_DATA_ERROR, Z_MEM_ERROR, Z_STREAM_ERROR};

/// Error type for zlib operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZlibError {
    /// Invalid or corrupt data
    DataError,
    /// Memory allocation failed
    MemoryError,
    /// Output buffer too small
    BufferError,
    /// Invalid stream state
    StreamError,
    /// Invalid header/trailer
    HeaderError,
    /// Checksum mismatch
    ChecksumError,
    /// Unexpected end of input
    UnexpectedEof,
}

impl ZlibError {
    /// Convert to zlib error code
    pub fn to_zlib_error(self) -> c_int {
        match self {
            ZlibError::DataError | ZlibError::HeaderError | ZlibError::ChecksumError => {
                Z_DATA_ERROR
            }
            ZlibError::MemoryError => Z_MEM_ERROR,
            ZlibError::BufferError => Z_BUF_ERROR,
            ZlibError::StreamError | ZlibError::UnexpectedEof => Z_STREAM_ERROR,
        }
    }
}

/// Result type for zlib operations
pub type ZlibResult<T> = Result<T, ZlibError>;
