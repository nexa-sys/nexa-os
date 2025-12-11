//! nzip FFI bindings for dynamic linking
//!
//! This module provides FFI declarations for dynamically linking against
//! libnzip.so at runtime. Instead of statically linking nzip, nurl can
//! load the library dynamically.
//!
//! # Usage
//! ```rust
//! use nurl::nzip_ffi::*;
//!
//! // Decompress gzip data
//! let mut dest = vec![0u8; 65536];
//! let mut dest_len = dest.len() as u64;
//! let result = unsafe { gzip_uncompress(dest.as_mut_ptr(), &mut dest_len, src.as_ptr(), src.len() as u64) };
//! ```

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_ulong};

// ============================================================================
// Type Definitions (zlib compatible)
// ============================================================================

pub type uInt = u32;
pub type uLong = u64;
pub type Bytef = u8;

// ============================================================================
// zlib Constants
// ============================================================================

/// No error
pub const Z_OK: c_int = 0;
/// Stream end
pub const Z_STREAM_END: c_int = 1;
/// Need dictionary
pub const Z_NEED_DICT: c_int = 2;
/// I/O error
pub const Z_ERRNO: c_int = -1;
/// Stream error
pub const Z_STREAM_ERROR: c_int = -2;
/// Data error
pub const Z_DATA_ERROR: c_int = -3;
/// Memory error
pub const Z_MEM_ERROR: c_int = -4;
/// Buffer error
pub const Z_BUF_ERROR: c_int = -5;
/// Version error
pub const Z_VERSION_ERROR: c_int = -6;

/// Default compression level
pub const Z_DEFAULT_COMPRESSION: c_int = -1;
/// No compression
pub const Z_NO_COMPRESSION: c_int = 0;
/// Best speed
pub const Z_BEST_SPEED: c_int = 1;
/// Best compression
pub const Z_BEST_COMPRESSION: c_int = 9;

// ============================================================================
// External Functions (linked dynamically from libz.so / libnzip.so)
// ============================================================================

// NexaOS nzip is built as libz.so for zlib API compatibility
// In sysroot-pic, it's accessible via libnzip.so symlink
#[link(name = "nzip")]
extern "C" {
    // ========================================================================
    // Library Information
    // ========================================================================

    /// Get zlib version string
    pub fn zlibVersion() -> *const c_char;

    /// Get nzip library version string
    pub fn nzip_version() -> *const c_char;

    /// Check if nzip is available (always returns 1)
    pub fn nzip_available() -> c_int;

    /// Get zlib compile flags
    pub fn zlibCompileFlags() -> uLong;

    // ========================================================================
    // Basic Compression/Decompression (zlib format)
    // ========================================================================

    /// Calculate upper bound of compressed size
    pub fn compressBound(source_len: uLong) -> uLong;

    /// Compress data in one call (zlib format)
    pub fn compress(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: uLong,
    ) -> c_int;

    /// Compress data with specified level (zlib format)
    pub fn compress2(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: uLong,
        level: c_int,
    ) -> c_int;

    /// Decompress data in one call (zlib format)
    pub fn uncompress(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: uLong,
    ) -> c_int;

    /// Decompress data with source length update (zlib format)
    pub fn uncompress2(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: *mut uLong,
    ) -> c_int;

    // ========================================================================
    // GZIP Format Functions (NexaOS Extensions)
    // ========================================================================

    /// Decompress gzip data in one call
    pub fn gzip_uncompress(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: uLong,
    ) -> c_int;

    /// Compress to gzip format in one call
    pub fn gzip_compress(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: uLong,
        level: c_int,
    ) -> c_int;

    /// Calculate upper bound for gzip compressed size
    pub fn gzip_compress_bound(source_len: uLong) -> uLong;

    // ========================================================================
    // Raw DEFLATE Functions (NexaOS Extensions)
    // ========================================================================

    /// Decompress raw deflate data (no zlib/gzip header)
    pub fn inflate_raw(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: uLong,
    ) -> c_int;

    /// Compress to raw deflate (no zlib/gzip header)
    pub fn deflate_raw(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: uLong,
        level: c_int,
    ) -> c_int;

    // ========================================================================
    // ZLIB Format Functions (Explicit naming, NexaOS Extensions)
    // ========================================================================

    /// Decompress zlib-format data
    pub fn zlib_uncompress(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: uLong,
    ) -> c_int;

    /// Compress to zlib format
    pub fn zlib_compress(
        dest: *mut Bytef,
        dest_len: *mut uLong,
        source: *const Bytef,
        source_len: uLong,
        level: c_int,
    ) -> c_int;

    // ========================================================================
    // Checksum Functions
    // ========================================================================

    /// Calculate Adler-32 checksum
    pub fn adler32_z(adler: uLong, buf: *const Bytef, len: usize) -> uLong;

    /// Calculate CRC-32 checksum
    pub fn crc32_z(crc: uLong, buf: *const Bytef, len: usize) -> uLong;

    /// Calculate Adler-32 checksum (compat)
    pub fn adler32_func(adler: uLong, buf: *const Bytef, len: uInt) -> uLong;

    /// Calculate CRC-32 checksum (compat)
    pub fn crc32_func(crc: uLong, buf: *const Bytef, len: uInt) -> uLong;
}

// ============================================================================
// Safe Rust Wrappers
// ============================================================================

/// Decompress gzip data
pub fn decompress_gzip(input: &[u8]) -> Result<Vec<u8>, DecompressError> {
    // Start with an estimated output size (4x input is a reasonable guess)
    let mut output_size = input.len().saturating_mul(4).max(1024);

    loop {
        let mut output = vec![0u8; output_size];
        let mut dest_len = output_size as u64;

        let result = unsafe {
            gzip_uncompress(
                output.as_mut_ptr(),
                &mut dest_len,
                input.as_ptr(),
                input.len() as u64,
            )
        };

        match result {
            Z_OK => {
                output.truncate(dest_len as usize);
                return Ok(output);
            }
            Z_BUF_ERROR => {
                // Buffer too small, double it
                output_size = output_size.saturating_mul(2);
                if output_size > 256 * 1024 * 1024 {
                    // 256MB limit
                    return Err(DecompressError::BufferTooLarge);
                }
            }
            Z_DATA_ERROR => return Err(DecompressError::InvalidData),
            Z_MEM_ERROR => return Err(DecompressError::OutOfMemory),
            Z_STREAM_ERROR => return Err(DecompressError::StreamError),
            _ => return Err(DecompressError::Unknown(result)),
        }
    }
}

/// Decompress zlib/deflate data
pub fn decompress_zlib(input: &[u8]) -> Result<Vec<u8>, DecompressError> {
    // Start with an estimated output size
    let mut output_size = input.len().saturating_mul(4).max(1024);

    loop {
        let mut output = vec![0u8; output_size];
        let mut dest_len = output_size as u64;

        let result = unsafe {
            uncompress(
                output.as_mut_ptr(),
                &mut dest_len,
                input.as_ptr(),
                input.len() as u64,
            )
        };

        match result {
            Z_OK => {
                output.truncate(dest_len as usize);
                return Ok(output);
            }
            Z_BUF_ERROR => {
                output_size = output_size.saturating_mul(2);
                if output_size > 256 * 1024 * 1024 {
                    return Err(DecompressError::BufferTooLarge);
                }
            }
            Z_DATA_ERROR => return Err(DecompressError::InvalidData),
            Z_MEM_ERROR => return Err(DecompressError::OutOfMemory),
            Z_STREAM_ERROR => return Err(DecompressError::StreamError),
            _ => return Err(DecompressError::Unknown(result)),
        }
    }
}

/// Decompress raw deflate data (no header)
pub fn decompress_raw(input: &[u8]) -> Result<Vec<u8>, DecompressError> {
    // Start with an estimated output size
    let mut output_size = input.len().saturating_mul(4).max(1024);

    loop {
        let mut output = vec![0u8; output_size];
        let mut dest_len = output_size as u64;

        let result = unsafe {
            inflate_raw(
                output.as_mut_ptr(),
                &mut dest_len,
                input.as_ptr(),
                input.len() as u64,
            )
        };

        match result {
            Z_OK => {
                output.truncate(dest_len as usize);
                return Ok(output);
            }
            Z_BUF_ERROR => {
                output_size = output_size.saturating_mul(2);
                if output_size > 256 * 1024 * 1024 {
                    return Err(DecompressError::BufferTooLarge);
                }
            }
            Z_DATA_ERROR => return Err(DecompressError::InvalidData),
            Z_MEM_ERROR => return Err(DecompressError::OutOfMemory),
            Z_STREAM_ERROR => return Err(DecompressError::StreamError),
            _ => return Err(DecompressError::Unknown(result)),
        }
    }
}

/// Decompression error types
#[derive(Debug, Clone)]
pub enum DecompressError {
    /// Invalid compressed data
    InvalidData,
    /// Output buffer too small (even after growing)
    BufferTooLarge,
    /// Out of memory
    OutOfMemory,
    /// Stream error
    StreamError,
    /// Unknown error
    Unknown(c_int),
}

impl std::fmt::Display for DecompressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecompressError::InvalidData => write!(f, "Invalid compressed data"),
            DecompressError::BufferTooLarge => write!(f, "Decompressed data exceeds size limit"),
            DecompressError::OutOfMemory => write!(f, "Out of memory"),
            DecompressError::StreamError => write!(f, "Stream error"),
            DecompressError::Unknown(code) => write!(f, "Unknown error (code {})", code),
        }
    }
}

impl std::error::Error for DecompressError {}
