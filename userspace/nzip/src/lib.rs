//! NexaOS Compression Library (nzip)
//!
//! A modern, zlib/libz.so ABI-compatible compression library for NexaOS.
//!
//! This library provides DEFLATE-based compression/decompression:
//!
//! ## Compression Formats
//! - **Raw DEFLATE**: RFC 1951 - No header/trailer
//! - **ZLIB**: RFC 1950 - 2-byte header + adler32 trailer
//! - **GZIP**: RFC 1952 - 10+ byte header + crc32 trailer
//!
//! ## Features
//! - Streaming compression/decompression via z_stream
//! - One-shot compress/uncompress functions
//! - Memory-efficient implementation
//! - Full zlib ABI compatibility (drop-in replacement)
//!
//! ## Compression Levels
//! - Level 0: No compression (store only)
//! - Level 1: Best speed
//! - Levels 2-8: Progressive trade-offs
//! - Level 9: Best compression
//!
//! # Design Philosophy
//! - Pure Rust implementation
//! - No external dependencies
//! - zlib/libz.so ABI compatibility for drop-in replacement
//! - Clean Rust API alongside C ABI exports

#![feature(linkage)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

// ============================================================================
// Module Declarations
// ============================================================================

// CRC32 checksum (for GZIP format)
pub mod crc32;

// Adler-32 checksum (for ZLIB format)
pub mod adler32;

// Huffman coding
pub mod huffman;

// DEFLATE compression algorithm
pub mod deflate;

// DEFLATE decompression (inflate)
pub mod inflate;

// zlib format wrapper
pub mod zlib_format;

// gzip format wrapper
pub mod gzip;

// zlib C ABI compatibility layer
pub mod compat;

// Error handling
pub mod error;

// ============================================================================
// C Type Definitions (zlib compatible)
// ============================================================================

pub type c_int = i32;
pub type c_uint = u32;
pub type c_long = i64;
pub type c_ulong = u64;
pub type c_char = i8;
pub type c_uchar = u8;
pub type uInt = u32;
pub type uLong = u64;
pub type Bytef = u8;
pub type charf = i8;
pub type voidpf = *mut core::ffi::c_void;
pub type voidp = *mut core::ffi::c_void;

// ============================================================================
// zlib Constants
// ============================================================================

/// zlib library version
pub const ZLIB_VERSION: &[u8] = b"1.3.1\0";
pub const ZLIB_VERNUM: u32 = 0x1310;

/// Flush values
pub const Z_NO_FLUSH: c_int = 0;
pub const Z_PARTIAL_FLUSH: c_int = 1;
pub const Z_SYNC_FLUSH: c_int = 2;
pub const Z_FULL_FLUSH: c_int = 3;
pub const Z_FINISH: c_int = 4;
pub const Z_BLOCK: c_int = 5;
pub const Z_TREES: c_int = 6;

/// Return codes
pub const Z_OK: c_int = 0;
pub const Z_STREAM_END: c_int = 1;
pub const Z_NEED_DICT: c_int = 2;
pub const Z_ERRNO: c_int = -1;
pub const Z_STREAM_ERROR: c_int = -2;
pub const Z_DATA_ERROR: c_int = -3;
pub const Z_MEM_ERROR: c_int = -4;
pub const Z_BUF_ERROR: c_int = -5;
pub const Z_VERSION_ERROR: c_int = -6;

/// Compression levels
pub const Z_NO_COMPRESSION: c_int = 0;
pub const Z_BEST_SPEED: c_int = 1;
pub const Z_BEST_COMPRESSION: c_int = 9;
pub const Z_DEFAULT_COMPRESSION: c_int = -1;

/// Compression strategies
pub const Z_FILTERED: c_int = 1;
pub const Z_HUFFMAN_ONLY: c_int = 2;
pub const Z_RLE: c_int = 3;
pub const Z_FIXED: c_int = 4;
pub const Z_DEFAULT_STRATEGY: c_int = 0;

/// Data types
pub const Z_BINARY: c_int = 0;
pub const Z_TEXT: c_int = 1;
pub const Z_ASCII: c_int = Z_TEXT;
pub const Z_UNKNOWN: c_int = 2;

/// Compression method
pub const Z_DEFLATED: c_int = 8;

/// Memory level
pub const MAX_MEM_LEVEL: c_int = 9;
pub const DEF_MEM_LEVEL: c_int = 8;

/// Window bits
pub const MAX_WBITS: c_int = 15;
pub const DEF_WBITS: c_int = MAX_WBITS;

// ============================================================================
// z_stream Structure (zlib ABI compatible)
// ============================================================================

/// Allocation function type
pub type alloc_func = Option<extern "C" fn(opaque: voidpf, items: uInt, size: uInt) -> voidpf>;
/// Free function type
pub type free_func = Option<extern "C" fn(opaque: voidpf, address: voidpf)>;

/// z_stream structure - zlib ABI compatible
#[repr(C)]
pub struct z_stream {
    /// Next input byte
    pub next_in: *const Bytef,
    /// Number of bytes available at next_in
    pub avail_in: uInt,
    /// Total number of input bytes read so far
    pub total_in: uLong,

    /// Next output byte will go here
    pub next_out: *mut Bytef,
    /// Remaining free space at next_out
    pub avail_out: uInt,
    /// Total number of bytes output so far
    pub total_out: uLong,

    /// Last error message, NULL if no error
    pub msg: *const c_char,
    /// Internal state (opaque)
    pub state: voidpf,

    /// Used to allocate the internal state
    pub zalloc: alloc_func,
    /// Used to free the internal state
    pub zfree: free_func,
    /// Private data object passed to zalloc and zfree
    pub opaque: voidpf,

    /// Best guess about the data type: binary or text
    pub data_type: c_int,
    /// Adler-32 or CRC-32 value of uncompressed data
    pub adler: uLong,
    /// Reserved for future use
    pub reserved: uLong,
}

pub type z_streamp = *mut z_stream;

/// gz_header structure for gzip header information
#[repr(C)]
pub struct gz_header {
    /// True if compressed data believed to be text
    pub text: c_int,
    /// Modification time
    pub time: uLong,
    /// Extra flags (not used when writing)
    pub xflags: c_int,
    /// Operating system
    pub os: c_int,
    /// Pointer to extra field or NULL if none
    pub extra: *mut Bytef,
    /// Extra field length (valid if extra != NULL)
    pub extra_len: uInt,
    /// Space at extra (only when reading header)
    pub extra_max: uInt,
    /// Pointer to zero-terminated file name or NULL
    pub name: *mut Bytef,
    /// Space at name (only when reading header)
    pub name_max: uInt,
    /// Pointer to zero-terminated comment or NULL
    pub comment: *mut Bytef,
    /// Space at comment (only when reading header)
    pub comm_max: uInt,
    /// True if there was or will be a header crc
    pub hcrc: c_int,
    /// True when done reading gzip header (not used when writing)
    pub done: c_int,
}

pub type gz_headerp = *mut gz_header;

// ============================================================================
// Library Version Information
// ============================================================================

/// Get library version string
#[no_mangle]
pub extern "C" fn zlibVersion() -> *const c_char {
    ZLIB_VERSION.as_ptr() as *const c_char
}

/// Get library compile flags
#[no_mangle]
pub extern "C" fn zlibCompileFlags() -> uLong {
    // Return flags indicating our capabilities
    // Bits 0-1: size of uInt (0=16, 1=32, 2=64)
    // Bits 2-3: size of uLong
    // Bits 4-5: size of voidpf
    // Bits 6-7: size of z_off_t
    let mut flags: uLong = 0;

    // uInt is 32-bit
    flags |= 1;
    // uLong is 64-bit
    flags |= 2 << 2;
    // voidpf is 64-bit (pointer)
    flags |= 2 << 4;
    // z_off_t is 64-bit
    flags |= 2 << 6;

    flags
}

// ============================================================================
// Re-exports
// ============================================================================

// CRC32
pub use crc32::{crc32, crc32_combine, Crc32};

// Adler-32
pub use adler32::{adler32, adler32_combine, Adler32};

// Error types
pub use error::{ZlibError, ZlibResult};

// Deflate compression
pub use deflate::{compress_bound, Deflater};

// Inflate decompression
pub use inflate::Inflater;

// ============================================================================
// C ABI Exports - Basic Functions
// ============================================================================

/// Calculate upper bound of compressed size
#[no_mangle]
pub extern "C" fn compressBound(source_len: uLong) -> uLong {
    compress_bound(source_len as usize) as uLong
}

/// Compress data in one call
#[no_mangle]
pub extern "C" fn compress(
    dest: *mut Bytef,
    dest_len: *mut uLong,
    source: *const Bytef,
    source_len: uLong,
) -> c_int {
    compress2(dest, dest_len, source, source_len, Z_DEFAULT_COMPRESSION)
}

/// Compress data with specified level
#[no_mangle]
pub extern "C" fn compress2(
    dest: *mut Bytef,
    dest_len: *mut uLong,
    source: *const Bytef,
    source_len: uLong,
    level: c_int,
) -> c_int {
    if dest.is_null() || dest_len.is_null() || source.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        let src = core::slice::from_raw_parts(source, source_len as usize);
        let dst_cap = *dest_len as usize;
        let dst = core::slice::from_raw_parts_mut(dest, dst_cap);

        match deflate::compress_to_zlib(src, dst, level) {
            Ok(written) => {
                *dest_len = written as uLong;
                Z_OK
            }
            Err(_) => Z_BUF_ERROR,
        }
    }
}

/// Decompress data in one call
#[no_mangle]
pub extern "C" fn uncompress(
    dest: *mut Bytef,
    dest_len: *mut uLong,
    source: *const Bytef,
    source_len: uLong,
) -> c_int {
    uncompress2(dest, dest_len, source, &mut (source_len as uLong))
}

/// Decompress data with source length update
#[no_mangle]
pub extern "C" fn uncompress2(
    dest: *mut Bytef,
    dest_len: *mut uLong,
    source: *const Bytef,
    source_len: *mut uLong,
) -> c_int {
    if dest.is_null() || dest_len.is_null() || source.is_null() || source_len.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        let src = core::slice::from_raw_parts(source, *source_len as usize);
        let dst_cap = *dest_len as usize;
        let dst = core::slice::from_raw_parts_mut(dest, dst_cap);

        match inflate::decompress_zlib(src, dst) {
            Ok((written, consumed)) => {
                *dest_len = written as uLong;
                *source_len = consumed as uLong;
                Z_OK
            }
            Err(e) => e.to_zlib_error(),
        }
    }
}

// ============================================================================
// C ABI Exports - Deflate Functions
// ============================================================================

/// Initialize deflate stream
#[no_mangle]
pub extern "C" fn deflateInit_(
    strm: z_streamp,
    level: c_int,
    version: *const c_char,
    stream_size: c_int,
) -> c_int {
    deflateInit2_(
        strm,
        level,
        Z_DEFLATED,
        MAX_WBITS,
        DEF_MEM_LEVEL,
        Z_DEFAULT_STRATEGY,
        version,
        stream_size,
    )
}

/// Initialize deflate stream with more options
#[no_mangle]
pub extern "C" fn deflateInit2_(
    strm: z_streamp,
    level: c_int,
    method: c_int,
    window_bits: c_int,
    mem_level: c_int,
    strategy: c_int,
    _version: *const c_char,
    stream_size: c_int,
) -> c_int {
    if strm.is_null() {
        return Z_STREAM_ERROR;
    }

    if stream_size != core::mem::size_of::<z_stream>() as c_int {
        return Z_VERSION_ERROR;
    }

    if method != Z_DEFLATED {
        return Z_STREAM_ERROR;
    }

    unsafe {
        // Initialize stream fields
        (*strm).total_in = 0;
        (*strm).total_out = 0;
        (*strm).msg = core::ptr::null();
        (*strm).data_type = Z_UNKNOWN;
        (*strm).adler = 1; // Initial adler32 value

        // Allocate internal state
        let state = Box::new(compat::DeflateState::new(
            level,
            window_bits,
            mem_level,
            strategy,
        ));
        (*strm).state = Box::into_raw(state) as voidpf;

        Z_OK
    }
}

/// Deflate (compress) data
#[no_mangle]
pub extern "C" fn deflate(strm: z_streamp, flush: c_int) -> c_int {
    if strm.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        if (*strm).state.is_null() {
            return Z_STREAM_ERROR;
        }

        let state = &mut *((*strm).state as *mut compat::DeflateState);
        state.deflate(&mut *strm, flush)
    }
}

/// End deflate stream
#[no_mangle]
pub extern "C" fn deflateEnd(strm: z_streamp) -> c_int {
    if strm.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        if (*strm).state.is_null() {
            return Z_STREAM_ERROR;
        }

        // Free the internal state
        let _ = Box::from_raw((*strm).state as *mut compat::DeflateState);
        (*strm).state = core::ptr::null_mut();

        Z_OK
    }
}

/// Reset deflate stream
#[no_mangle]
pub extern "C" fn deflateReset(strm: z_streamp) -> c_int {
    if strm.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        if (*strm).state.is_null() {
            return Z_STREAM_ERROR;
        }

        let state = &mut *((*strm).state as *mut compat::DeflateState);
        state.reset();

        (*strm).total_in = 0;
        (*strm).total_out = 0;
        (*strm).adler = 1;
        (*strm).msg = core::ptr::null();

        Z_OK
    }
}

/// Set deflate dictionary
#[no_mangle]
pub extern "C" fn deflateSetDictionary(
    strm: z_streamp,
    dictionary: *const Bytef,
    dict_length: uInt,
) -> c_int {
    if strm.is_null() || dictionary.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        if (*strm).state.is_null() {
            return Z_STREAM_ERROR;
        }

        let state = &mut *((*strm).state as *mut compat::DeflateState);
        let dict = core::slice::from_raw_parts(dictionary, dict_length as usize);
        state.set_dictionary(dict);

        // Update adler32 with dictionary
        (*strm).adler = adler32::adler32_slice(1, dict) as uLong;

        Z_OK
    }
}

/// Get deflate bound
#[no_mangle]
pub extern "C" fn deflateBound(_strm: z_streamp, source_len: uLong) -> uLong {
    compress_bound(source_len as usize) as uLong
}

// ============================================================================
// C ABI Exports - Inflate Functions
// ============================================================================

/// Initialize inflate stream
#[no_mangle]
pub extern "C" fn inflateInit_(
    strm: z_streamp,
    version: *const c_char,
    stream_size: c_int,
) -> c_int {
    inflateInit2_(strm, DEF_WBITS, version, stream_size)
}

/// Initialize inflate stream with window bits
#[no_mangle]
pub extern "C" fn inflateInit2_(
    strm: z_streamp,
    window_bits: c_int,
    _version: *const c_char,
    stream_size: c_int,
) -> c_int {
    if strm.is_null() {
        return Z_STREAM_ERROR;
    }

    if stream_size != core::mem::size_of::<z_stream>() as c_int {
        return Z_VERSION_ERROR;
    }

    unsafe {
        // Initialize stream fields
        (*strm).total_in = 0;
        (*strm).total_out = 0;
        (*strm).msg = core::ptr::null();
        (*strm).data_type = Z_UNKNOWN;
        (*strm).adler = 1; // Initial adler32/crc32 value

        // Allocate internal state
        let state = Box::new(compat::InflateState::new(window_bits));
        (*strm).state = Box::into_raw(state) as voidpf;

        Z_OK
    }
}

/// Inflate (decompress) data
#[no_mangle]
pub extern "C" fn inflate(strm: z_streamp, flush: c_int) -> c_int {
    if strm.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        if (*strm).state.is_null() {
            return Z_STREAM_ERROR;
        }

        let state = &mut *((*strm).state as *mut compat::InflateState);
        state.inflate(&mut *strm, flush)
    }
}

/// End inflate stream
#[no_mangle]
pub extern "C" fn inflateEnd(strm: z_streamp) -> c_int {
    if strm.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        if (*strm).state.is_null() {
            return Z_STREAM_ERROR;
        }

        // Free the internal state
        let _ = Box::from_raw((*strm).state as *mut compat::InflateState);
        (*strm).state = core::ptr::null_mut();

        Z_OK
    }
}

/// Reset inflate stream
#[no_mangle]
pub extern "C" fn inflateReset(strm: z_streamp) -> c_int {
    if strm.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        if (*strm).state.is_null() {
            return Z_STREAM_ERROR;
        }

        let state = &mut *((*strm).state as *mut compat::InflateState);
        state.reset();

        (*strm).total_in = 0;
        (*strm).total_out = 0;
        (*strm).adler = 1;
        (*strm).msg = core::ptr::null();

        Z_OK
    }
}

/// Reset inflate stream with new window bits
#[no_mangle]
pub extern "C" fn inflateReset2(strm: z_streamp, window_bits: c_int) -> c_int {
    if strm.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        if (*strm).state.is_null() {
            return Z_STREAM_ERROR;
        }

        let state = &mut *((*strm).state as *mut compat::InflateState);
        state.reset_with_window_bits(window_bits);

        (*strm).total_in = 0;
        (*strm).total_out = 0;
        (*strm).adler = 1;
        (*strm).msg = core::ptr::null();

        Z_OK
    }
}

/// Set inflate dictionary
#[no_mangle]
pub extern "C" fn inflateSetDictionary(
    strm: z_streamp,
    dictionary: *const Bytef,
    dict_length: uInt,
) -> c_int {
    if strm.is_null() || dictionary.is_null() {
        return Z_STREAM_ERROR;
    }

    unsafe {
        if (*strm).state.is_null() {
            return Z_STREAM_ERROR;
        }

        let state = &mut *((*strm).state as *mut compat::InflateState);
        let dict = core::slice::from_raw_parts(dictionary, dict_length as usize);

        if state.set_dictionary(dict) {
            Z_OK
        } else {
            Z_DATA_ERROR
        }
    }
}

// ============================================================================
// C ABI Exports - Checksum Functions
// ============================================================================

/// Calculate Adler-32 checksum
#[no_mangle]
pub extern "C" fn adler32_z(adler: uLong, buf: *const Bytef, len: usize) -> uLong {
    if buf.is_null() {
        return 1;
    }

    unsafe {
        let data = core::slice::from_raw_parts(buf, len);
        adler32::adler32_slice(adler as u32, data) as uLong
    }
}

/// Calculate CRC-32 checksum
#[no_mangle]
pub extern "C" fn crc32_z(crc: uLong, buf: *const Bytef, len: usize) -> uLong {
    if buf.is_null() {
        return 0;
    }

    unsafe {
        let data = core::slice::from_raw_parts(buf, len);
        crc32::crc32_slice(crc as u32, data) as uLong
    }
}

// Aliases for compatibility
#[no_mangle]
pub extern "C" fn adler32_func(adler: uLong, buf: *const Bytef, len: uInt) -> uLong {
    adler32_z(adler, buf, len as usize)
}

#[no_mangle]
pub extern "C" fn crc32_func(crc: uLong, buf: *const Bytef, len: uInt) -> uLong {
    crc32_z(crc, buf, len as usize)
}

/// Combine two Adler-32 checksums
#[no_mangle]
pub extern "C" fn adler32_combine_func(adler1: uLong, adler2: uLong, len2: i64) -> uLong {
    adler32::adler32_combine_impl(adler1 as u32, adler2 as u32, len2 as usize) as uLong
}

/// Combine two CRC-32 checksums
#[no_mangle]
pub extern "C" fn crc32_combine_func(crc1: uLong, crc2: uLong, len2: i64) -> uLong {
    crc32::crc32_combine_impl(crc1 as u32, crc2 as u32, len2 as usize) as uLong
}
