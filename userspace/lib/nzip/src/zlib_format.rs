//! ZLIB Format Wrapper (RFC 1950)
//!
//! Provides zlib format encoding/decoding.

use crate::adler32::{adler32, Adler32};
use crate::deflate::{compress_bound, Deflater};
use crate::error::{ZlibError, ZlibResult};
use crate::inflate::Inflater;

/// ZLIB format header
#[derive(Clone, Copy, Debug)]
pub struct ZlibHeader {
    /// Compression method (8 = deflate)
    pub cm: u8,
    /// Compression info (log2(window_size) - 8)
    pub cinfo: u8,
    /// Check bits
    pub fcheck: u8,
    /// Dictionary flag
    pub fdict: bool,
    /// Compression level
    pub flevel: u8,
    /// Dictionary ID (if fdict is true)
    pub dict_id: Option<u32>,
}

impl Default for ZlibHeader {
    fn default() -> Self {
        Self {
            cm: 8,     // DEFLATE
            cinfo: 7,  // 32K window
            fcheck: 0, // Calculated on encode
            fdict: false,
            flevel: 2, // Default compression
            dict_id: None,
        }
    }
}

impl ZlibHeader {
    /// Create header with specific compression level
    pub fn with_level(level: i32) -> Self {
        let flevel = match level {
            0..=1 => 0, // Fastest
            2..=5 => 1, // Fast
            6 => 2,     // Default
            _ => 3,     // Maximum
        };

        Self {
            flevel,
            ..Default::default()
        }
    }

    /// Encode header to bytes
    pub fn encode(&self) -> Vec<u8> {
        let cmf = (self.cinfo << 4) | self.cm;
        let mut flg = (self.flevel << 6) | if self.fdict { 0x20 } else { 0 };

        // Calculate fcheck so (cmf * 256 + flg) % 31 == 0
        let check = ((cmf as u16) << 8) | (flg as u16);
        let fcheck = (31 - (check % 31)) as u8;
        flg |= fcheck;

        let mut result = vec![cmf, flg];

        if let Some(dict_id) = self.dict_id {
            result.push((dict_id >> 24) as u8);
            result.push((dict_id >> 16) as u8);
            result.push((dict_id >> 8) as u8);
            result.push(dict_id as u8);
        }

        result
    }

    /// Decode header from bytes
    pub fn decode(data: &[u8]) -> ZlibResult<(Self, usize)> {
        if data.len() < 2 {
            return Err(ZlibError::UnexpectedEof);
        }

        let cmf = data[0];
        let flg = data[1];

        let cm = cmf & 0x0F;
        let cinfo = (cmf >> 4) & 0x0F;

        if cm != 8 {
            return Err(ZlibError::HeaderError);
        }

        if cinfo > 7 {
            return Err(ZlibError::HeaderError);
        }

        // Verify checksum
        if ((cmf as u16) * 256 + (flg as u16)) % 31 != 0 {
            return Err(ZlibError::HeaderError);
        }

        let fdict = (flg & 0x20) != 0;
        let flevel = (flg >> 6) & 0x03;
        let fcheck = flg & 0x1F;

        let (dict_id, consumed) = if fdict {
            if data.len() < 6 {
                return Err(ZlibError::UnexpectedEof);
            }
            let id = ((data[2] as u32) << 24)
                | ((data[3] as u32) << 16)
                | ((data[4] as u32) << 8)
                | (data[5] as u32);
            (Some(id), 6)
        } else {
            (None, 2)
        };

        Ok((
            Self {
                cm,
                cinfo,
                fcheck,
                fdict,
                flevel,
                dict_id,
            },
            consumed,
        ))
    }
}

/// ZLIB format compressor
pub struct ZlibCompressor {
    /// Header
    header: ZlibHeader,
    /// Underlying deflater
    deflater: Deflater,
    /// Adler-32 checksum
    checksum: Adler32,
    /// Whether header has been written
    header_written: bool,
}

impl ZlibCompressor {
    /// Create a new zlib compressor
    pub fn new(level: i32) -> Self {
        Self {
            header: ZlibHeader::with_level(level),
            deflater: Deflater::new(level, 15, 8, 0),
            checksum: Adler32::new(),
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

        // Update checksum
        self.checksum.update(input);

        // Compress
        let compressed = self.deflater.compress(input, finish)?;
        output.extend(compressed);

        // Write trailer if finishing
        if finish {
            let adler = self.checksum.finalize();
            output.push((adler >> 24) as u8);
            output.push((adler >> 16) as u8);
            output.push((adler >> 8) as u8);
            output.push(adler as u8);
        }

        Ok(output)
    }
}

/// ZLIB format decompressor
pub struct ZlibDecompressor {
    /// Header (once parsed)
    header: Option<ZlibHeader>,
    /// Underlying inflater
    inflater: Inflater,
    /// Adler-32 checksum
    checksum: Adler32,
    /// Buffer for partial header
    header_buffer: Vec<u8>,
}

impl ZlibDecompressor {
    /// Create a new zlib decompressor
    pub fn new() -> Self {
        Self {
            header: None,
            inflater: Inflater::new(15),
            checksum: Adler32::new(),
            header_buffer: Vec::new(),
        }
    }

    /// Decompress data
    pub fn decompress(&mut self, input: &[u8]) -> ZlibResult<(Vec<u8>, usize)> {
        let mut pos = 0;

        // Parse header if not yet done
        if self.header.is_none() {
            self.header_buffer.extend_from_slice(input);

            match ZlibHeader::decode(&self.header_buffer) {
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
            let combined: Vec<u8> = self
                .header_buffer
                .iter()
                .chain(input[pos..].iter())
                .copied()
                .collect();
            let result = self.inflater.decompress(&combined)?;
            self.header_buffer.clear();
            (result.0, result.1.saturating_sub(self.header_buffer.len()))
        } else {
            self.inflater.decompress(&input[pos..])?
        };

        // Update checksum
        self.checksum.update(&decompressed);

        Ok((decompressed, pos + consumed))
    }

    /// Verify checksum (call after all data is decompressed)
    pub fn verify(&self, stored_adler: u32) -> bool {
        self.checksum.finalize() == stored_adler
    }
}

impl Default for ZlibDecompressor {
    fn default() -> Self {
        Self::new()
    }
}
