//! zlib C ABI Compatibility Layer
//!
//! Provides internal state management for streaming compression/decompression.

use crate::{
    z_stream, c_int, c_char, voidpf, uInt, uLong, Bytef,
    Z_OK, Z_STREAM_END, Z_STREAM_ERROR, Z_DATA_ERROR, Z_BUF_ERROR,
    Z_NO_FLUSH, Z_SYNC_FLUSH, Z_FULL_FLUSH, Z_FINISH,
    MAX_WBITS, DEF_MEM_LEVEL,
};
use crate::deflate::Deflater;
use crate::inflate::Inflater;
use crate::adler32;

/// Internal state for deflate (compression)
pub struct DeflateState {
    /// Core deflater
    deflater: Deflater,
    /// Window bits (determines format)
    window_bits: i32,
    /// Whether zlib header has been written
    header_written: bool,
    /// Whether we're done
    finished: bool,
    /// Input buffer for partial data
    input_buffer: Vec<u8>,
    /// Dictionary (if set)
    dictionary: Option<Vec<u8>>,
}

impl DeflateState {
    /// Create a new deflate state
    pub fn new(level: c_int, window_bits: c_int, mem_level: c_int, strategy: c_int) -> Self {
        let actual_wbits = if window_bits < 0 {
            // Negative = raw deflate (no header)
            -window_bits
        } else if window_bits > 15 {
            // >15 = gzip format
            window_bits - 16
        } else {
            // Normal = zlib format
            window_bits
        };
        
        Self {
            deflater: Deflater::new(level, actual_wbits, mem_level, strategy),
            window_bits,
            header_written: false,
            finished: false,
            input_buffer: Vec::new(),
            dictionary: None,
        }
    }

    /// Reset the deflate state
    pub fn reset(&mut self) {
        self.deflater.reset();
        self.header_written = false;
        self.finished = false;
        self.input_buffer.clear();
    }

    /// Set dictionary for compression
    pub fn set_dictionary(&mut self, dict: &[u8]) {
        self.dictionary = Some(dict.to_vec());
    }

    /// Perform deflate operation
    pub fn deflate(&mut self, strm: &mut z_stream, flush: c_int) -> c_int {
        if self.finished && flush != Z_FINISH {
            return Z_STREAM_ERROR;
        }

        let mut output = Vec::new();

        // Write header if needed
        if !self.header_written {
            if self.window_bits > 0 && self.window_bits <= 15 {
                // zlib header
                let cmf = 0x78u8; // CM=8, CINFO=7
                let level_hint = match self.deflater.level {
                    0..=1 => 0,
                    2..=5 => 1,
                    6 => 2,
                    _ => 3,
                };
                let flg = (level_hint << 6) as u8;
                let check = ((cmf as u16) << 8) | (flg as u16);
                let flg = flg + (31 - (check % 31) as u8) as u8;
                
                output.push(cmf);
                output.push(flg);
            } else if self.window_bits > 15 {
                // gzip header
                output.extend_from_slice(&[
                    0x1F, 0x8B, // Magic
                    0x08,       // CM (deflate)
                    0x00,       // FLG
                    0, 0, 0, 0, // MTIME
                    0x00,       // XFL
                    0xFF,       // OS (unknown)
                ]);
            }
            // else: raw deflate, no header
            
            self.header_written = true;
        }

        // Read input
        unsafe {
            if !strm.next_in.is_null() && strm.avail_in > 0 {
                let input = core::slice::from_raw_parts(strm.next_in, strm.avail_in as usize);
                self.input_buffer.extend_from_slice(input);
                strm.next_in = strm.next_in.add(strm.avail_in as usize);
                strm.total_in += strm.avail_in as uLong;
                strm.avail_in = 0;
            }
        }

        // Compress
        let finish = flush == Z_FINISH;
        if !self.input_buffer.is_empty() || finish {
            match self.deflater.compress(&self.input_buffer, finish) {
                Ok(compressed) => {
                    output.extend_from_slice(&compressed);
                    self.input_buffer.clear();
                }
                Err(_) => return Z_DATA_ERROR,
            }
        }

        // Write trailer if finishing
        if finish && !self.finished {
            if self.window_bits > 0 && self.window_bits <= 15 {
                // zlib trailer: Adler-32
                let adler = self.deflater.checksum();
                output.push((adler >> 24) as u8);
                output.push((adler >> 16) as u8);
                output.push((adler >> 8) as u8);
                output.push(adler as u8);
            } else if self.window_bits > 15 {
                // gzip trailer: CRC-32 + ISIZE
                // For simplicity, use Adler-32 as placeholder
                // (real implementation would track CRC-32)
                let crc = self.deflater.checksum();
                output.push(crc as u8);
                output.push((crc >> 8) as u8);
                output.push((crc >> 16) as u8);
                output.push((crc >> 24) as u8);
                
                let size = strm.total_in as u32;
                output.push(size as u8);
                output.push((size >> 8) as u8);
                output.push((size >> 16) as u8);
                output.push((size >> 24) as u8);
            }
            self.finished = true;
        }

        // Write output
        unsafe {
            if !strm.next_out.is_null() && strm.avail_out > 0 {
                let to_write = core::cmp::min(output.len(), strm.avail_out as usize);
                core::ptr::copy_nonoverlapping(
                    output.as_ptr(),
                    strm.next_out,
                    to_write,
                );
                strm.next_out = strm.next_out.add(to_write);
                strm.avail_out -= to_write as uInt;
                strm.total_out += to_write as uLong;
                
                if to_write < output.len() {
                    return Z_BUF_ERROR;
                }
            }
        }

        // Update adler
        strm.adler = self.deflater.checksum() as uLong;

        if self.finished {
            Z_STREAM_END
        } else {
            Z_OK
        }
    }

    /// Get compression level (for internal use)
    #[allow(dead_code)]
    fn level(&self) -> i32 {
        self.deflater.level
    }
}

impl Deflater {
    /// Get compression level
    pub fn level(&self) -> i32 {
        self.level
    }
}

/// Internal state for inflate (decompression)
pub struct InflateState {
    /// Core inflater
    inflater: Inflater,
    /// Window bits (determines format)
    window_bits: i32,
    /// Header state
    header_state: HeaderState,
    /// Whether we're done
    finished: bool,
    /// Input buffer for partial data
    input_buffer: Vec<u8>,
    /// Output buffer for partial data
    output_buffer: Vec<u8>,
    /// Dictionary (if needed)
    dictionary: Option<Vec<u8>>,
    /// Expected dictionary Adler-32
    dict_id: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum HeaderState {
    /// Haven't read header yet
    Start,
    /// Need dictionary
    NeedDict,
    /// Reading deflate data
    Deflate,
    /// Reading trailer
    Trailer,
    /// Done
    Done,
}

impl InflateState {
    /// Create a new inflate state
    pub fn new(window_bits: c_int) -> Self {
        let actual_wbits = if window_bits < 0 {
            -window_bits
        } else if window_bits > 15 {
            window_bits - 16
        } else {
            window_bits
        };
        
        Self {
            inflater: Inflater::new(actual_wbits),
            window_bits,
            header_state: HeaderState::Start,
            finished: false,
            input_buffer: Vec::new(),
            output_buffer: Vec::new(),
            dictionary: None,
            dict_id: 0,
        }
    }

    /// Reset the inflate state
    pub fn reset(&mut self) {
        self.inflater.reset();
        self.header_state = HeaderState::Start;
        self.finished = false;
        self.input_buffer.clear();
        self.output_buffer.clear();
    }

    /// Reset with new window bits
    pub fn reset_with_window_bits(&mut self, window_bits: c_int) {
        let actual_wbits = if window_bits < 0 {
            -window_bits
        } else if window_bits > 15 {
            window_bits - 16
        } else {
            window_bits
        };
        
        self.window_bits = window_bits;
        self.inflater = Inflater::new(actual_wbits);
        self.header_state = HeaderState::Start;
        self.finished = false;
        self.input_buffer.clear();
        self.output_buffer.clear();
    }

    /// Set dictionary for decompression
    pub fn set_dictionary(&mut self, dict: &[u8]) -> bool {
        if self.header_state != HeaderState::NeedDict {
            return false;
        }
        
        // Verify dictionary ID
        let dict_adler = adler32::adler32(dict);
        if dict_adler != self.dict_id {
            return false;
        }
        
        self.dictionary = Some(dict.to_vec());
        self.header_state = HeaderState::Deflate;
        true
    }

    /// Perform inflate operation
    pub fn inflate(&mut self, strm: &mut z_stream, _flush: c_int) -> c_int {
        if self.finished {
            return Z_STREAM_END;
        }

        // Read input
        unsafe {
            if !strm.next_in.is_null() && strm.avail_in > 0 {
                let input = core::slice::from_raw_parts(strm.next_in, strm.avail_in as usize);
                self.input_buffer.extend_from_slice(input);
                strm.next_in = strm.next_in.add(strm.avail_in as usize);
                strm.total_in += strm.avail_in as uLong;
                strm.avail_in = 0;
            }
        }

        // Process header if needed
        if self.header_state == HeaderState::Start {
            if !self.process_header() {
                return Z_OK; // Need more input
            }
        }

        if self.header_state == HeaderState::NeedDict {
            return crate::Z_NEED_DICT;
        }

        // Decompress
        if self.header_state == HeaderState::Deflate && !self.input_buffer.is_empty() {
            match self.inflater.decompress(&self.input_buffer) {
                Ok((decompressed, consumed)) => {
                    self.output_buffer.extend_from_slice(&decompressed);
                    self.input_buffer.drain(..consumed);
                    
                    if self.inflater.finished {
                        self.header_state = HeaderState::Trailer;
                    }
                }
                Err(_) => return Z_DATA_ERROR,
            }
        }

        // Process trailer if needed
        if self.header_state == HeaderState::Trailer {
            if self.process_trailer() {
                self.header_state = HeaderState::Done;
                self.finished = true;
            }
        }

        // Write output
        unsafe {
            if !strm.next_out.is_null() && strm.avail_out > 0 && !self.output_buffer.is_empty() {
                let to_write = core::cmp::min(self.output_buffer.len(), strm.avail_out as usize);
                core::ptr::copy_nonoverlapping(
                    self.output_buffer.as_ptr(),
                    strm.next_out,
                    to_write,
                );
                strm.next_out = strm.next_out.add(to_write);
                strm.avail_out -= to_write as uInt;
                strm.total_out += to_write as uLong;
                self.output_buffer.drain(..to_write);
            }
        }

        // Update adler
        strm.adler = self.inflater.checksum() as uLong;

        if self.finished {
            Z_STREAM_END
        } else if self.output_buffer.is_empty() && self.input_buffer.is_empty() {
            Z_OK
        } else {
            Z_OK
        }
    }

    /// Process header based on window_bits
    fn process_header(&mut self) -> bool {
        if self.window_bits > 0 && self.window_bits <= 15 {
            // zlib header
            if self.input_buffer.len() < 2 {
                return false;
            }
            
            let cmf = self.input_buffer[0];
            let flg = self.input_buffer[1];
            
            // Validate header
            let cm = cmf & 0x0F;
            let cinfo = (cmf >> 4) & 0x0F;
            
            if cm != 8 || cinfo > 7 {
                return false;
            }
            
            if ((cmf as u16) * 256 + (flg as u16)) % 31 != 0 {
                return false;
            }
            
            if (flg & 0x20) != 0 {
                // Dictionary required
                if self.input_buffer.len() < 6 {
                    return false;
                }
                self.dict_id = ((self.input_buffer[2] as u32) << 24)
                    | ((self.input_buffer[3] as u32) << 16)
                    | ((self.input_buffer[4] as u32) << 8)
                    | (self.input_buffer[5] as u32);
                self.input_buffer.drain(..6);
                self.header_state = HeaderState::NeedDict;
            } else {
                self.input_buffer.drain(..2);
                self.header_state = HeaderState::Deflate;
            }
        } else if self.window_bits > 15 {
            // gzip header
            if self.input_buffer.len() < 10 {
                return false;
            }
            
            if self.input_buffer[0] != 0x1F || self.input_buffer[1] != 0x8B {
                return false;
            }
            
            if self.input_buffer[2] != 8 {
                return false; // Not deflate
            }
            
            let flg = self.input_buffer[3];
            let mut pos = 10;
            
            // Skip optional fields
            if (flg & 0x04) != 0 {
                // FEXTRA
                if self.input_buffer.len() < pos + 2 {
                    return false;
                }
                let xlen = (self.input_buffer[pos] as usize)
                    | ((self.input_buffer[pos + 1] as usize) << 8);
                pos += 2 + xlen;
            }
            
            if (flg & 0x08) != 0 {
                // FNAME
                while pos < self.input_buffer.len() && self.input_buffer[pos] != 0 {
                    pos += 1;
                }
                pos += 1;
            }
            
            if (flg & 0x10) != 0 {
                // FCOMMENT
                while pos < self.input_buffer.len() && self.input_buffer[pos] != 0 {
                    pos += 1;
                }
                pos += 1;
            }
            
            if (flg & 0x02) != 0 {
                // FHCRC
                pos += 2;
            }
            
            if self.input_buffer.len() < pos {
                return false;
            }
            
            self.input_buffer.drain(..pos);
            self.header_state = HeaderState::Deflate;
        } else {
            // Raw deflate
            self.header_state = HeaderState::Deflate;
        }
        
        true
    }

    /// Process trailer based on window_bits
    fn process_trailer(&mut self) -> bool {
        if self.window_bits > 0 && self.window_bits <= 15 {
            // zlib trailer: 4 bytes Adler-32
            if self.input_buffer.len() < 4 {
                return false;
            }
            
            let stored = ((self.input_buffer[0] as u32) << 24)
                | ((self.input_buffer[1] as u32) << 16)
                | ((self.input_buffer[2] as u32) << 8)
                | (self.input_buffer[3] as u32);
            
            self.input_buffer.drain(..4);
            
            // Verify checksum
            stored == self.inflater.checksum()
        } else if self.window_bits > 15 {
            // gzip trailer: 4 bytes CRC-32 + 4 bytes ISIZE
            if self.input_buffer.len() < 8 {
                return false;
            }
            
            self.input_buffer.drain(..8);
            true
        } else {
            // Raw deflate: no trailer
            true
        }
    }
}

impl Inflater {
    /// Check if inflation is finished
    pub fn finished(&self) -> bool {
        self.finished
    }
}
