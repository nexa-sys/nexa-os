//! DEFLATE Decompression (RFC 1951)
//!
//! Implements DEFLATE decompression (inflate) algorithm.

use std::vec::Vec;
use crate::huffman::{
    HuffmanTable, FIXED_LITLEN_LENGTHS, FIXED_DIST_LENGTHS,
    LENGTH_BASE, LENGTH_EXTRA, DIST_BASE, DIST_EXTRA,
    LITLEN_CODES, DIST_CODES, CODELEN_CODES, CODELEN_ORDER,
    MAX_BITS,
};
use crate::adler32::Adler32;
use crate::error::{ZlibError, ZlibResult};

/// Maximum window size (32KB)
pub const MAX_WINDOW_SIZE: usize = 32768;

/// DEFLATE decompressor state
pub struct Inflater {
    /// Window bits (8-15)
    window_bits: i32,
    
    /// Sliding window buffer
    window: Vec<u8>,
    /// Current position in window
    window_pos: usize,
    /// Bytes in window
    window_len: usize,
    
    /// Bit buffer for input
    bit_buffer: u32,
    /// Bits in bit buffer
    bit_count: u8,
    
    /// Whether we've read the final block
    final_block: bool,
    /// Whether decompression is complete
    finished: bool,
    
    /// Literal/length Huffman table
    litlen_table: HuffmanTable,
    /// Distance Huffman table
    dist_table: HuffmanTable,
    
    /// Checksum calculator
    checksum: Adler32,
}

impl Inflater {
    /// Create a new DEFLATE decompressor
    pub fn new(window_bits: i32) -> Self {
        let actual_wbits = window_bits.abs().clamp(8, 15);
        let window_size = 1 << actual_wbits;
        
        Self {
            window_bits: actual_wbits,
            window: vec![0; window_size],
            window_pos: 0,
            window_len: 0,
            bit_buffer: 0,
            bit_count: 0,
            final_block: false,
            finished: false,
            litlen_table: HuffmanTable::new(),
            dist_table: HuffmanTable::new(),
            checksum: Adler32::new(),
        }
    }

    /// Reset decompressor state
    pub fn reset(&mut self) {
        let window_size = 1 << self.window_bits;
        self.window = vec![0; window_size];
        self.window_pos = 0;
        self.window_len = 0;
        self.bit_buffer = 0;
        self.bit_count = 0;
        self.final_block = false;
        self.finished = false;
        self.checksum.reset();
    }

    /// Decompress input data
    pub fn decompress(&mut self, input: &[u8]) -> ZlibResult<(Vec<u8>, usize)> {
        let mut output = Vec::new();
        let mut input_pos = 0;
        
        while !self.finished && input_pos < input.len() {
            // Read block header if needed
            if !self.final_block {
                // Need at least 3 bits for block header
                while self.bit_count < 3 && input_pos < input.len() {
                    self.bit_buffer |= (input[input_pos] as u32) << self.bit_count;
                    self.bit_count += 8;
                    input_pos += 1;
                }
                
                if self.bit_count < 3 {
                    break; // Need more input
                }
                
                // Read BFINAL and BTYPE
                let header = self.bit_buffer & 0x7;
                self.bit_buffer >>= 3;
                self.bit_count -= 3;
                
                self.final_block = (header & 1) != 0;
                let btype = (header >> 1) & 3;
                
                match btype {
                    0 => {
                        // Stored block
                        self.inflate_stored(input, &mut input_pos, &mut output)?;
                    }
                    1 => {
                        // Fixed Huffman
                        self.setup_fixed_huffman()?;
                        self.inflate_huffman(input, &mut input_pos, &mut output)?;
                    }
                    2 => {
                        // Dynamic Huffman
                        self.read_dynamic_huffman(input, &mut input_pos)?;
                        self.inflate_huffman(input, &mut input_pos, &mut output)?;
                    }
                    3 => {
                        return Err(ZlibError::DataError); // Reserved
                    }
                    _ => unreachable!(),
                }
                
                if self.final_block {
                    self.finished = true;
                }
            }
        }
        
        // Update checksum
        self.checksum.update(&output);
        
        Ok((output, input_pos))
    }

    /// Inflate a stored (uncompressed) block
    fn inflate_stored(&mut self, input: &[u8], pos: &mut usize, output: &mut Vec<u8>) -> ZlibResult<()> {
        // Discard bits to byte boundary
        self.bit_buffer = 0;
        self.bit_count = 0;
        
        // Read LEN and NLEN
        if *pos + 4 > input.len() {
            return Err(ZlibError::UnexpectedEof);
        }
        
        let len = (input[*pos] as u16) | ((input[*pos + 1] as u16) << 8);
        let nlen = (input[*pos + 2] as u16) | ((input[*pos + 3] as u16) << 8);
        *pos += 4;
        
        // Verify LEN/NLEN
        if len != !nlen {
            return Err(ZlibError::DataError);
        }
        
        // Copy data
        if *pos + len as usize > input.len() {
            return Err(ZlibError::UnexpectedEof);
        }
        
        for i in 0..len as usize {
            let byte = input[*pos + i];
            output.push(byte);
            self.add_to_window(byte);
        }
        *pos += len as usize;
        
        Ok(())
    }

    /// Setup fixed Huffman tables
    fn setup_fixed_huffman(&mut self) -> ZlibResult<()> {
        self.litlen_table.build(&FIXED_LITLEN_LENGTHS, LITLEN_CODES)
            .map_err(|_| ZlibError::DataError)?;
        self.dist_table.build(&FIXED_DIST_LENGTHS, DIST_CODES)
            .map_err(|_| ZlibError::DataError)?;
        Ok(())
    }

    /// Read dynamic Huffman tables from input
    fn read_dynamic_huffman(&mut self, input: &[u8], pos: &mut usize) -> ZlibResult<()> {
        // Read HLIT, HDIST, HCLEN
        self.fill_bits(input, pos, 14)?;
        
        let hlit = (self.bit_buffer & 0x1F) as usize + 257;
        self.bit_buffer >>= 5;
        self.bit_count -= 5;
        
        let hdist = (self.bit_buffer & 0x1F) as usize + 1;
        self.bit_buffer >>= 5;
        self.bit_count -= 5;
        
        let hclen = (self.bit_buffer & 0xF) as usize + 4;
        self.bit_buffer >>= 4;
        self.bit_count -= 4;
        
        if hlit > LITLEN_CODES || hdist > DIST_CODES {
            return Err(ZlibError::DataError);
        }
        
        // Read code length code lengths
        let mut codelen_lengths = [0u8; CODELEN_CODES];
        for i in 0..hclen {
            self.fill_bits(input, pos, 3)?;
            codelen_lengths[CODELEN_ORDER[i]] = (self.bit_buffer & 0x7) as u8;
            self.bit_buffer >>= 3;
            self.bit_count -= 3;
        }
        
        // Build code length Huffman table
        let mut codelen_table = HuffmanTable::new();
        codelen_table.build(&codelen_lengths, CODELEN_CODES)
            .map_err(|_| ZlibError::DataError)?;
        
        // Read literal/length and distance code lengths
        let mut lengths = vec![0u8; hlit + hdist];
        let mut i = 0;
        
        while i < lengths.len() {
            self.fill_bits(input, pos, MAX_BITS as u8)?;
            
            let (sym, bits) = codelen_table.decode(self.bit_buffer);
            self.bit_buffer >>= bits;
            self.bit_count -= bits;
            
            match sym {
                0..=15 => {
                    // Literal length
                    lengths[i] = sym as u8;
                    i += 1;
                }
                16 => {
                    // Copy previous 3-6 times
                    if i == 0 {
                        return Err(ZlibError::DataError);
                    }
                    self.fill_bits(input, pos, 2)?;
                    let count = (self.bit_buffer & 0x3) as usize + 3;
                    self.bit_buffer >>= 2;
                    self.bit_count -= 2;
                    
                    let prev = lengths[i - 1];
                    for _ in 0..count {
                        if i >= lengths.len() {
                            return Err(ZlibError::DataError);
                        }
                        lengths[i] = prev;
                        i += 1;
                    }
                }
                17 => {
                    // Repeat zero 3-10 times
                    self.fill_bits(input, pos, 3)?;
                    let count = (self.bit_buffer & 0x7) as usize + 3;
                    self.bit_buffer >>= 3;
                    self.bit_count -= 3;
                    
                    for _ in 0..count {
                        if i >= lengths.len() {
                            return Err(ZlibError::DataError);
                        }
                        lengths[i] = 0;
                        i += 1;
                    }
                }
                18 => {
                    // Repeat zero 11-138 times
                    self.fill_bits(input, pos, 7)?;
                    let count = (self.bit_buffer & 0x7F) as usize + 11;
                    self.bit_buffer >>= 7;
                    self.bit_count -= 7;
                    
                    for _ in 0..count {
                        if i >= lengths.len() {
                            return Err(ZlibError::DataError);
                        }
                        lengths[i] = 0;
                        i += 1;
                    }
                }
                _ => return Err(ZlibError::DataError),
            }
        }
        
        // Build Huffman tables
        self.litlen_table.build(&lengths[..hlit], hlit)
            .map_err(|_| ZlibError::DataError)?;
        self.dist_table.build(&lengths[hlit..], hdist)
            .map_err(|_| ZlibError::DataError)?;
        
        Ok(())
    }

    /// Inflate data using Huffman codes
    fn inflate_huffman(&mut self, input: &[u8], pos: &mut usize, output: &mut Vec<u8>) -> ZlibResult<()> {
        loop {
            self.fill_bits(input, pos, MAX_BITS as u8)?;
            
            let (sym, bits) = self.litlen_table.decode(self.bit_buffer);
            if bits == 0 {
                return Err(ZlibError::DataError);
            }
            self.bit_buffer >>= bits;
            self.bit_count -= bits;
            
            if sym < 256 {
                // Literal byte
                output.push(sym as u8);
                self.add_to_window(sym as u8);
            } else if sym == 256 {
                // End of block
                break;
            } else {
                // Length/distance pair
                let len_idx = (sym - 257) as usize;
                if len_idx >= LENGTH_BASE.len() {
                    return Err(ZlibError::DataError);
                }
                
                let mut length = LENGTH_BASE[len_idx] as usize;
                let extra_bits = LENGTH_EXTRA[len_idx];
                
                if extra_bits > 0 {
                    self.fill_bits(input, pos, extra_bits)?;
                    length += (self.bit_buffer & ((1 << extra_bits) - 1)) as usize;
                    self.bit_buffer >>= extra_bits;
                    self.bit_count -= extra_bits;
                }
                
                // Read distance
                self.fill_bits(input, pos, MAX_BITS as u8)?;
                
                let (dist_sym, dist_bits) = self.dist_table.decode(self.bit_buffer);
                if dist_bits == 0 {
                    return Err(ZlibError::DataError);
                }
                self.bit_buffer >>= dist_bits;
                self.bit_count -= dist_bits;
                
                let dist_idx = dist_sym as usize;
                if dist_idx >= DIST_BASE.len() {
                    return Err(ZlibError::DataError);
                }
                
                let mut distance = DIST_BASE[dist_idx] as usize;
                let dist_extra = DIST_EXTRA[dist_idx];
                
                if dist_extra > 0 {
                    self.fill_bits(input, pos, dist_extra)?;
                    distance += (self.bit_buffer & ((1 << dist_extra) - 1)) as usize;
                    self.bit_buffer >>= dist_extra;
                    self.bit_count -= dist_extra;
                }
                
                // Copy from window
                self.copy_from_window(output, distance, length)?;
            }
        }
        
        Ok(())
    }

    /// Fill bit buffer with at least `needed` bits
    fn fill_bits(&mut self, input: &[u8], pos: &mut usize, needed: u8) -> ZlibResult<()> {
        while self.bit_count < needed && *pos < input.len() {
            self.bit_buffer |= (input[*pos] as u32) << self.bit_count;
            self.bit_count += 8;
            *pos += 1;
        }
        
        if self.bit_count < needed {
            return Err(ZlibError::UnexpectedEof);
        }
        
        Ok(())
    }

    /// Add byte to sliding window
    fn add_to_window(&mut self, byte: u8) {
        let window_size = self.window.len();
        self.window[self.window_pos] = byte;
        self.window_pos = (self.window_pos + 1) & (window_size - 1);
        if self.window_len < window_size {
            self.window_len += 1;
        }
    }

    /// Copy bytes from sliding window
    fn copy_from_window(&mut self, output: &mut Vec<u8>, distance: usize, length: usize) -> ZlibResult<()> {
        if distance > self.window_len || distance == 0 {
            return Err(ZlibError::DataError);
        }
        
        let window_size = self.window.len();
        let mut src_pos = (self.window_pos + window_size - distance) & (window_size - 1);
        
        for _ in 0..length {
            let byte = self.window[src_pos];
            output.push(byte);
            self.add_to_window(byte);
            src_pos = (src_pos + 1) & (window_size - 1);
        }
        
        Ok(())
    }

    /// Get current checksum
    pub fn checksum(&self) -> u32 {
        self.checksum.finalize()
    }
}

/// Decompress zlib format data in one call
pub fn decompress_zlib(input: &[u8], output: &mut [u8]) -> ZlibResult<(usize, usize)> {
    if input.len() < 6 {
        return Err(ZlibError::UnexpectedEof);
    }
    
    // Parse zlib header
    let cmf = input[0];
    let flg = input[1];
    
    // Check header
    let cm = cmf & 0x0F;
    let cinfo = (cmf >> 4) & 0x0F;
    
    if cm != 8 {
        return Err(ZlibError::HeaderError); // Not DEFLATE
    }
    
    if cinfo > 7 {
        return Err(ZlibError::HeaderError); // Invalid window size
    }
    
    // Check header checksum
    if ((cmf as u16) * 256 + (flg as u16)) % 31 != 0 {
        return Err(ZlibError::HeaderError);
    }
    
    // Check for preset dictionary (not supported)
    if (flg & 0x20) != 0 {
        return Err(ZlibError::HeaderError);
    }
    
    // Decompress
    let mut inflater = Inflater::new(8 + cinfo as i32);
    let (decompressed, consumed) = inflater.decompress(&input[2..])?;
    
    // Check we have room for output
    if decompressed.len() > output.len() {
        return Err(ZlibError::BufferError);
    }
    
    output[..decompressed.len()].copy_from_slice(&decompressed);
    
    // Verify Adler-32 checksum
    let trailer_pos = 2 + consumed;
    if trailer_pos + 4 > input.len() {
        return Err(ZlibError::UnexpectedEof);
    }
    
    let stored_adler = ((input[trailer_pos] as u32) << 24)
        | ((input[trailer_pos + 1] as u32) << 16)
        | ((input[trailer_pos + 2] as u32) << 8)
        | (input[trailer_pos + 3] as u32);
    
    if stored_adler != inflater.checksum() {
        return Err(ZlibError::ChecksumError);
    }
    
    Ok((decompressed.len(), trailer_pos + 4))
}
