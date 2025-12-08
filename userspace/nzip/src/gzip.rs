//! GZIP Format Wrapper (RFC 1952)
//!
//! Provides gzip format encoding/decoding.

use crate::crc32::{Crc32, crc32};
use crate::deflate::Deflater;
use crate::inflate::Inflater;
use crate::error::{ZlibError, ZlibResult};

/// GZIP magic bytes
pub const GZIP_MAGIC: [u8; 2] = [0x1F, 0x8B];

/// GZIP compression method
pub const GZIP_CM_DEFLATE: u8 = 8;

/// GZIP flags
pub mod flags {
    pub const FTEXT: u8 = 0x01;
    pub const FHCRC: u8 = 0x02;
    pub const FEXTRA: u8 = 0x04;
    pub const FNAME: u8 = 0x08;
    pub const FCOMMENT: u8 = 0x10;
}

/// GZIP OS identifiers
pub mod os {
    pub const FAT: u8 = 0;
    pub const AMIGA: u8 = 1;
    pub const VMS: u8 = 2;
    pub const UNIX: u8 = 3;
    pub const VM_CMS: u8 = 4;
    pub const ATARI: u8 = 5;
    pub const HPFS: u8 = 6;
    pub const MAC: u8 = 7;
    pub const ZSYSTEM: u8 = 8;
    pub const CPM: u8 = 9;
    pub const TOPS20: u8 = 10;
    pub const NTFS: u8 = 11;
    pub const QDOS: u8 = 12;
    pub const ACORN: u8 = 13;
    pub const UNKNOWN: u8 = 255;
}

/// GZIP header
#[derive(Clone, Debug)]
pub struct GzipHeader {
    /// Compression method (8 = deflate)
    pub cm: u8,
    /// Flags
    pub flg: u8,
    /// Modification time (Unix timestamp)
    pub mtime: u32,
    /// Extra flags (compression level hint)
    pub xfl: u8,
    /// Operating system
    pub os: u8,
    /// Extra field data
    pub extra: Option<Vec<u8>>,
    /// Original filename
    pub name: Option<String>,
    /// Comment
    pub comment: Option<String>,
    /// Header CRC16 (if FHCRC flag set)
    pub hcrc: Option<u16>,
}

impl Default for GzipHeader {
    fn default() -> Self {
        Self {
            cm: GZIP_CM_DEFLATE,
            flg: 0,
            mtime: 0,
            xfl: 0,
            os: os::UNIX,
            extra: None,
            name: None,
            comment: None,
            hcrc: None,
        }
    }
}

impl GzipHeader {
    /// Create header with filename
    pub fn with_name(name: &str) -> Self {
        Self {
            flg: flags::FNAME,
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    /// Encode header to bytes
    pub fn encode(&self) -> Vec<u8> {
        let mut output = Vec::new();
        
        // Magic number
        output.extend_from_slice(&GZIP_MAGIC);
        
        // CM
        output.push(self.cm);
        
        // Calculate flags
        let mut flg = 0u8;
        if self.extra.is_some() {
            flg |= flags::FEXTRA;
        }
        if self.name.is_some() {
            flg |= flags::FNAME;
        }
        if self.comment.is_some() {
            flg |= flags::FCOMMENT;
        }
        output.push(flg);
        
        // MTIME (little-endian)
        output.push(self.mtime as u8);
        output.push((self.mtime >> 8) as u8);
        output.push((self.mtime >> 16) as u8);
        output.push((self.mtime >> 24) as u8);
        
        // XFL
        output.push(self.xfl);
        
        // OS
        output.push(self.os);
        
        // Extra field
        if let Some(ref extra) = self.extra {
            let len = extra.len() as u16;
            output.push(len as u8);
            output.push((len >> 8) as u8);
            output.extend_from_slice(extra);
        }
        
        // Filename
        if let Some(ref name) = self.name {
            output.extend_from_slice(name.as_bytes());
            output.push(0);
        }
        
        // Comment
        if let Some(ref comment) = self.comment {
            output.extend_from_slice(comment.as_bytes());
            output.push(0);
        }
        
        output
    }

    /// Decode header from bytes
    pub fn decode(data: &[u8]) -> ZlibResult<(Self, usize)> {
        if data.len() < 10 {
            return Err(ZlibError::UnexpectedEof);
        }
        
        // Check magic
        if data[0] != GZIP_MAGIC[0] || data[1] != GZIP_MAGIC[1] {
            return Err(ZlibError::HeaderError);
        }
        
        let cm = data[2];
        if cm != GZIP_CM_DEFLATE {
            return Err(ZlibError::HeaderError);
        }
        
        let flg = data[3];
        let mtime = (data[4] as u32)
            | ((data[5] as u32) << 8)
            | ((data[6] as u32) << 16)
            | ((data[7] as u32) << 24);
        let xfl = data[8];
        let os = data[9];
        
        let mut pos = 10;
        
        // Extra field
        let extra = if (flg & flags::FEXTRA) != 0 {
            if pos + 2 > data.len() {
                return Err(ZlibError::UnexpectedEof);
            }
            let xlen = (data[pos] as usize) | ((data[pos + 1] as usize) << 8);
            pos += 2;
            
            if pos + xlen > data.len() {
                return Err(ZlibError::UnexpectedEof);
            }
            let extra_data = data[pos..pos + xlen].to_vec();
            pos += xlen;
            Some(extra_data)
        } else {
            None
        };
        
        // Filename
        let name = if (flg & flags::FNAME) != 0 {
            let start = pos;
            while pos < data.len() && data[pos] != 0 {
                pos += 1;
            }
            if pos >= data.len() {
                return Err(ZlibError::UnexpectedEof);
            }
            let name_bytes = &data[start..pos];
            pos += 1; // Skip null terminator
            String::from_utf8(name_bytes.to_vec()).ok()
        } else {
            None
        };
        
        // Comment
        let comment = if (flg & flags::FCOMMENT) != 0 {
            let start = pos;
            while pos < data.len() && data[pos] != 0 {
                pos += 1;
            }
            if pos >= data.len() {
                return Err(ZlibError::UnexpectedEof);
            }
            let comment_bytes = &data[start..pos];
            pos += 1; // Skip null terminator
            String::from_utf8(comment_bytes.to_vec()).ok()
        } else {
            None
        };
        
        // Header CRC
        let hcrc = if (flg & flags::FHCRC) != 0 {
            if pos + 2 > data.len() {
                return Err(ZlibError::UnexpectedEof);
            }
            let crc = (data[pos] as u16) | ((data[pos + 1] as u16) << 8);
            pos += 2;
            Some(crc)
        } else {
            None
        };
        
        Ok((Self {
            cm,
            flg,
            mtime,
            xfl,
            os,
            extra,
            name,
            comment,
            hcrc,
        }, pos))
    }
}

/// GZIP trailer
#[derive(Clone, Copy, Debug)]
pub struct GzipTrailer {
    /// CRC32 of uncompressed data
    pub crc32: u32,
    /// Size of uncompressed data (mod 2^32)
    pub isize: u32,
}

impl GzipTrailer {
    /// Encode trailer to bytes (little-endian)
    pub fn encode(&self) -> [u8; 8] {
        [
            self.crc32 as u8,
            (self.crc32 >> 8) as u8,
            (self.crc32 >> 16) as u8,
            (self.crc32 >> 24) as u8,
            self.isize as u8,
            (self.isize >> 8) as u8,
            (self.isize >> 16) as u8,
            (self.isize >> 24) as u8,
        ]
    }

    /// Decode trailer from bytes
    pub fn decode(data: &[u8]) -> ZlibResult<Self> {
        if data.len() < 8 {
            return Err(ZlibError::UnexpectedEof);
        }
        
        let crc32 = (data[0] as u32)
            | ((data[1] as u32) << 8)
            | ((data[2] as u32) << 16)
            | ((data[3] as u32) << 24);
        
        let isize = (data[4] as u32)
            | ((data[5] as u32) << 8)
            | ((data[6] as u32) << 16)
            | ((data[7] as u32) << 24);
        
        Ok(Self { crc32, isize })
    }
}

/// GZIP compressor
pub struct GzipCompressor {
    /// Header
    header: GzipHeader,
    /// Underlying deflater
    deflater: Deflater,
    /// CRC32 checksum
    crc: Crc32,
    /// Uncompressed size
    size: u32,
    /// Whether header has been written
    header_written: bool,
}

impl GzipCompressor {
    /// Create a new gzip compressor
    pub fn new(level: i32) -> Self {
        Self {
            header: GzipHeader::default(),
            deflater: Deflater::new(level, 15, 8, 0),
            crc: Crc32::new(),
            size: 0,
            header_written: false,
        }
    }

    /// Create with custom header
    pub fn with_header(header: GzipHeader, level: i32) -> Self {
        Self {
            header,
            deflater: Deflater::new(level, 15, 8, 0),
            crc: Crc32::new(),
            size: 0,
            header_written: false,
        }
    }

    /// Compress data
    pub fn compress(&mut self, input: &[u8], finish: bool) -> ZlibResult<Vec<u8>> {
        let mut output = Vec::new();
        
        // Write header if not yet written
        if !self.header_written {
            output.extend(self.header.encode());
            self.header_written = true;
        }
        
        // Update checksum and size
        self.crc.update(input);
        self.size = self.size.wrapping_add(input.len() as u32);
        
        // Compress
        let compressed = self.deflater.compress(input, finish)?;
        output.extend(compressed);
        
        // Write trailer if finishing
        if finish {
            let trailer = GzipTrailer {
                crc32: self.crc.finalize(),
                isize: self.size,
            };
            output.extend_from_slice(&trailer.encode());
        }
        
        Ok(output)
    }
}

/// GZIP decompressor
pub struct GzipDecompressor {
    /// Header (once parsed)
    header: Option<GzipHeader>,
    /// Underlying inflater
    inflater: Inflater,
    /// CRC32 checksum
    crc: Crc32,
    /// Uncompressed size
    size: u32,
    /// Buffer for partial header
    header_buffer: Vec<u8>,
}

impl GzipDecompressor {
    /// Create a new gzip decompressor
    pub fn new() -> Self {
        Self {
            header: None,
            inflater: Inflater::new(15),
            crc: Crc32::new(),
            size: 0,
            header_buffer: Vec::new(),
        }
    }

    /// Get the parsed header
    pub fn header(&self) -> Option<&GzipHeader> {
        self.header.as_ref()
    }

    /// Decompress data
    pub fn decompress(&mut self, input: &[u8]) -> ZlibResult<(Vec<u8>, usize)> {
        let mut pos = 0;
        
        // Parse header if not yet done
        if self.header.is_none() {
            self.header_buffer.extend_from_slice(input);
            
            match GzipHeader::decode(&self.header_buffer) {
                Ok((header, consumed)) => {
                    self.header = Some(header);
                    let remaining = self.header_buffer[consumed..].to_vec();
                    self.header_buffer = remaining;
                    pos = consumed.saturating_sub(input.len() - self.header_buffer.len());
                }
                Err(ZlibError::UnexpectedEof) => {
                    return Ok((Vec::new(), input.len()));
                }
                Err(e) => return Err(e),
            }
        }
        
        // Decompress
        let (decompressed, consumed) = if !self.header_buffer.is_empty() {
            let combined: Vec<u8> = self.header_buffer.iter()
                .chain(input[pos..].iter())
                .copied()
                .collect();
            let result = self.inflater.decompress(&combined)?;
            self.header_buffer.clear();
            (result.0, result.1.saturating_sub(self.header_buffer.len()))
        } else {
            self.inflater.decompress(&input[pos..])?
        };
        
        // Update checksum and size
        self.crc.update(&decompressed);
        self.size = self.size.wrapping_add(decompressed.len() as u32);
        
        Ok((decompressed, pos + consumed))
    }

    /// Verify trailer (call after all data is decompressed)
    pub fn verify(&self, trailer: &GzipTrailer) -> bool {
        self.crc.finalize() == trailer.crc32 && self.size == trailer.isize
    }
}

impl Default for GzipDecompressor {
    fn default() -> Self {
        Self::new()
    }
}

/// Compress data to gzip format in one call
pub fn gzip_compress(input: &[u8], level: i32) -> ZlibResult<Vec<u8>> {
    let mut compressor = GzipCompressor::new(level);
    compressor.compress(input, true)
}

/// Decompress gzip data in one call
pub fn gzip_decompress(input: &[u8]) -> ZlibResult<Vec<u8>> {
    let mut decompressor = GzipDecompressor::new();
    let (output, consumed) = decompressor.decompress(input)?;
    
    // Verify trailer
    if consumed + 8 > input.len() {
        return Err(ZlibError::UnexpectedEof);
    }
    
    let trailer = GzipTrailer::decode(&input[consumed..])?;
    if !decompressor.verify(&trailer) {
        return Err(ZlibError::ChecksumError);
    }
    
    Ok(output)
}
