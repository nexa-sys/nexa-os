//! Integration tests for nzip library

use nzip::{
    compress, compress2, uncompress, compressBound,
    deflateInit_, deflate, deflateEnd, deflateReset,
    inflateInit_, inflate, inflateEnd, inflateReset,
    z_stream, Z_OK, Z_STREAM_END, Z_FINISH, Z_NO_FLUSH,
    Z_DEFAULT_COMPRESSION, Z_BEST_SPEED, Z_BEST_COMPRESSION,
    ZLIB_VERSION,
};
use nzip::crc32::{crc32, crc32_slice, Crc32};
use nzip::adler32::{adler32, adler32_slice, Adler32};

#[test]
fn test_crc32_basic() {
    // Test empty data
    assert_eq!(crc32(&[]), 0);
    
    // Test known value for "hello"
    let crc = crc32(b"hello");
    assert_eq!(crc, 0x3610a686);
}

#[test]
fn test_crc32_incremental() {
    let full = crc32(b"hello world");
    
    let mut hasher = Crc32::new();
    hasher.update(b"hello");
    hasher.update(b" ");
    hasher.update(b"world");
    
    assert_eq!(hasher.finalize(), full);
}

#[test]
fn test_adler32_basic() {
    // Adler-32 of empty data is 1
    assert_eq!(adler32(&[]), 1);
    
    // Test known value
    let adler = adler32(b"hello");
    assert_eq!(adler, 0x062c0215);
}

#[test]
fn test_adler32_incremental() {
    let full = adler32(b"hello world");
    
    let mut hasher = Adler32::new();
    hasher.update(b"hello");
    hasher.update(b" ");
    hasher.update(b"world");
    
    assert_eq!(hasher.finalize(), full);
}

#[test]
fn test_compress_uncompress_basic() {
    let input = b"Hello, World! This is a test of the nzip compression library.";
    let bound = compressBound(input.len() as u64) as usize;
    
    let mut compressed = vec![0u8; bound];
    let mut compressed_len = bound as u64;
    
    // Compress
    let ret = unsafe {
        compress(
            compressed.as_mut_ptr(),
            &mut compressed_len,
            input.as_ptr(),
            input.len() as u64,
        )
    };
    assert_eq!(ret, Z_OK);
    compressed.truncate(compressed_len as usize);
    
    // Uncompress
    let mut decompressed = vec![0u8; input.len()];
    let mut decompressed_len = input.len() as u64;
    
    let ret = unsafe {
        uncompress(
            decompressed.as_mut_ptr(),
            &mut decompressed_len,
            compressed.as_ptr(),
            compressed_len,
        )
    };
    assert_eq!(ret, Z_OK);
    assert_eq!(decompressed_len as usize, input.len());
    assert_eq!(&decompressed[..], input);
}

#[test]
fn test_compress_levels() {
    let input = b"The quick brown fox jumps over the lazy dog. ".repeat(100);
    let bound = compressBound(input.len() as u64) as usize;
    
    for level in [Z_BEST_SPEED, Z_DEFAULT_COMPRESSION, Z_BEST_COMPRESSION] {
        let mut compressed = vec![0u8; bound];
        let mut compressed_len = bound as u64;
        
        let ret = unsafe {
            compress2(
                compressed.as_mut_ptr(),
                &mut compressed_len,
                input.as_ptr(),
                input.len() as u64,
                level,
            )
        };
        assert_eq!(ret, Z_OK);
        
        // Verify we can decompress
        let mut decompressed = vec![0u8; input.len()];
        let mut decompressed_len = input.len() as u64;
        
        let ret = unsafe {
            uncompress(
                decompressed.as_mut_ptr(),
                &mut decompressed_len,
                compressed.as_ptr(),
                compressed_len,
            )
        };
        assert_eq!(ret, Z_OK);
        assert_eq!(decompressed, input);
    }
}

#[test]
fn test_streaming_deflate_inflate() {
    let input = b"Test data for streaming compression and decompression.".repeat(50);
    
    // Initialize deflate
    let mut strm: z_stream = unsafe { core::mem::zeroed() };
    let ret = unsafe {
        deflateInit_(
            &mut strm,
            Z_DEFAULT_COMPRESSION,
            ZLIB_VERSION.as_ptr() as *const i8,
            core::mem::size_of::<z_stream>() as i32,
        )
    };
    assert_eq!(ret, Z_OK);
    
    // Compress
    let mut compressed = vec![0u8; compressBound(input.len() as u64) as usize];
    strm.next_in = input.as_ptr();
    strm.avail_in = input.len() as u32;
    strm.next_out = compressed.as_mut_ptr();
    strm.avail_out = compressed.len() as u32;
    
    let ret = unsafe { deflate(&mut strm, Z_FINISH) };
    assert!(ret == Z_OK || ret == Z_STREAM_END);
    
    let compressed_len = strm.total_out as usize;
    compressed.truncate(compressed_len);
    
    unsafe { deflateEnd(&mut strm) };
    
    // Initialize inflate
    let mut strm: z_stream = unsafe { core::mem::zeroed() };
    let ret = unsafe {
        inflateInit_(
            &mut strm,
            ZLIB_VERSION.as_ptr() as *const i8,
            core::mem::size_of::<z_stream>() as i32,
        )
    };
    assert_eq!(ret, Z_OK);
    
    // Decompress
    let mut decompressed = vec![0u8; input.len()];
    strm.next_in = compressed.as_ptr();
    strm.avail_in = compressed.len() as u32;
    strm.next_out = decompressed.as_mut_ptr();
    strm.avail_out = decompressed.len() as u32;
    
    let ret = unsafe { inflate(&mut strm, Z_FINISH) };
    assert!(ret == Z_OK || ret == Z_STREAM_END);
    
    unsafe { inflateEnd(&mut strm) };
    
    assert_eq!(decompressed, input);
}

#[test]
fn test_deflate_reset() {
    let input1 = b"First data block";
    let input2 = b"Second data block";
    
    // Initialize
    let mut strm: z_stream = unsafe { core::mem::zeroed() };
    let ret = unsafe {
        deflateInit_(
            &mut strm,
            Z_DEFAULT_COMPRESSION,
            ZLIB_VERSION.as_ptr() as *const i8,
            core::mem::size_of::<z_stream>() as i32,
        )
    };
    assert_eq!(ret, Z_OK);
    
    // Compress first block
    let mut compressed1 = vec![0u8; 256];
    strm.next_in = input1.as_ptr();
    strm.avail_in = input1.len() as u32;
    strm.next_out = compressed1.as_mut_ptr();
    strm.avail_out = compressed1.len() as u32;
    
    let ret = unsafe { deflate(&mut strm, Z_FINISH) };
    assert!(ret == Z_OK || ret == Z_STREAM_END);
    
    // Reset
    let ret = unsafe { deflateReset(&mut strm) };
    assert_eq!(ret, Z_OK);
    
    // Compress second block
    let mut compressed2 = vec![0u8; 256];
    strm.next_in = input2.as_ptr();
    strm.avail_in = input2.len() as u32;
    strm.next_out = compressed2.as_mut_ptr();
    strm.avail_out = compressed2.len() as u32;
    
    let ret = unsafe { deflate(&mut strm, Z_FINISH) };
    assert!(ret == Z_OK || ret == Z_STREAM_END);
    
    unsafe { deflateEnd(&mut strm) };
}
