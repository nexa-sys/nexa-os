# nzip - NexaOS Compression Library

A modern, zlib/libz.so ABI-compatible compression library for NexaOS.

## Features

- **Full zlib ABI compatibility** - Drop-in replacement for `libz.so`
- **DEFLATE compression** - RFC 1951 compliant
- **ZLIB format** - RFC 1950 compliant
- **GZIP format** - RFC 1952 compliant
- **Raw DEFLATE** - No header/trailer mode
- **Streaming API** - z_stream based compression/decompression
- **One-shot API** - Simple compress/uncompress functions
- **CRC32 & Adler-32** - Checksum functions

## Compression Levels

| Level | Description |
|-------|-------------|
| 0 | No compression (store only) |
| 1 | Best speed |
| 2-8 | Progressive trade-offs |
| 9 | Best compression |
| -1 | Default (level 6) |

## C API

### Basic Functions

```c
// Compress data
int compress(Bytef *dest, uLongf *destLen, const Bytef *source, uLong sourceLen);
int compress2(Bytef *dest, uLongf *destLen, const Bytef *source, uLong sourceLen, int level);

// Decompress data
int uncompress(Bytef *dest, uLongf *destLen, const Bytef *source, uLong sourceLen);

// Calculate compressed size bound
uLong compressBound(uLong sourceLen);
```

### Streaming Compression

```c
// Initialize
int deflateInit(z_streamp strm, int level);
int deflateInit2(z_streamp strm, int level, int method, int windowBits, int memLevel, int strategy);

// Compress
int deflate(z_streamp strm, int flush);

// Cleanup
int deflateEnd(z_streamp strm);
```

### Streaming Decompression

```c
// Initialize
int inflateInit(z_streamp strm);
int inflateInit2(z_streamp strm, int windowBits);

// Decompress
int inflate(z_streamp strm, int flush);

// Cleanup
int inflateEnd(z_streamp strm);
```

### Checksum Functions

```c
uLong adler32(uLong adler, const Bytef *buf, uInt len);
uLong crc32(uLong crc, const Bytef *buf, uInt len);
uLong adler32_combine(uLong adler1, uLong adler2, z_off_t len2);
uLong crc32_combine(uLong crc1, uLong crc2, z_off_t len2);
```

## Window Bits

The `windowBits` parameter controls the format:

| Value | Format |
|-------|--------|
| 8-15 | zlib format (default) |
| -8 to -15 | Raw DEFLATE (no header) |
| 24-31 (16+8 to 16+15) | gzip format |

## Rust API

```rust
use nzip::{Deflater, Inflater, compress_bound};
use nzip::zlib_format::{ZlibCompressor, ZlibDecompressor};
use nzip::gzip::{GzipCompressor, GzipDecompressor};

// One-shot compression
let input = b"Hello, World!";
let bound = compress_bound(input.len());
let mut output = vec![0u8; bound];
let compressed_len = nzip::compress_to_zlib(input, &mut output, 6)?;

// Streaming compression
let mut compressor = ZlibCompressor::new(6);
let compressed = compressor.compress(input, true)?;

// Streaming decompression
let mut decompressor = ZlibDecompressor::new();
let (decompressed, _) = decompressor.decompress(&compressed)?;
```

## Building

```bash
# Build as part of NexaOS userspace
./scripts/build.sh userspace

# Or build standalone
cd userspace/nzip
cargo build --release
```

## Library Files

- `libz.so` - Dynamic library
- `libz.a` - Static library

## Compatibility

This library is designed to be ABI-compatible with zlib 1.3.x. It can be used as a drop-in replacement for applications that link against `-lz`.

## License

MIT License - See LICENSE file
