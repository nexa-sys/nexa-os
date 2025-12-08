//! DEFLATE Compression (RFC 1951)
//!
//! Implements DEFLATE compression algorithm with LZ77 and Huffman coding.

use std::vec::Vec;
use crate::huffman::{
    HuffmanEncoder, FIXED_LITLEN_LENGTHS, FIXED_DIST_LENGTHS,
    LENGTH_BASE, LENGTH_EXTRA, DIST_BASE, DIST_EXTRA,
    LITLEN_CODES, DIST_CODES, MAX_BITS,
    build_code_lengths,
};
use crate::adler32::Adler32;
use crate::error::{ZlibError, ZlibResult};
use crate::c_int;

/// Maximum window size (32KB)
pub const MAX_WINDOW_SIZE: usize = 32768;

/// Maximum match length
pub const MAX_MATCH: usize = 258;

/// Minimum match length
pub const MIN_MATCH: usize = 3;

/// Hash table size (power of 2)
const HASH_SIZE: usize = 32768;

/// Hash mask
const HASH_MASK: usize = HASH_SIZE - 1;

/// Calculate upper bound on compressed size
pub fn compress_bound(source_len: usize) -> usize {
    // zlib header (2) + deflate overhead + adler32 (4)
    source_len + (source_len >> 12) + (source_len >> 14) + (source_len >> 25) + 13
}

/// DEFLATE compressor state
pub struct Deflater {
    /// Compression level (0-9)
    level: i32,
    /// Window bits (8-15)
    window_bits: i32,
    /// Memory level (1-9)
    mem_level: i32,
    /// Compression strategy
    strategy: i32,
    
    /// Sliding window buffer
    window: Vec<u8>,
    /// Current position in window
    window_pos: usize,
    /// Bytes in window
    window_len: usize,
    
    /// Hash table for LZ77
    hash_table: Vec<u16>,
    /// Hash chain links
    hash_chain: Vec<u16>,
    
    /// Output buffer
    output: Vec<u8>,
    /// Bit buffer for output
    bit_buffer: u32,
    /// Bits in bit buffer
    bit_count: u8,
    
    /// Pending literal/length/distance tokens
    pending: Vec<Token>,
    
    /// Whether we've written the header
    header_written: bool,
    /// Whether compression is finished
    finished: bool,
    
    /// Checksum calculator
    checksum: Adler32,
}

/// LZ77 token (literal or length/distance pair)
#[derive(Clone, Copy)]
enum Token {
    /// Literal byte
    Literal(u8),
    /// Match (length, distance)
    Match(u16, u16),
}

impl Deflater {
    /// Create a new DEFLATE compressor
    pub fn new(level: i32, window_bits: i32, mem_level: i32, strategy: i32) -> Self {
        let actual_level = if level == -1 { 6 } else { level.clamp(0, 9) };
        let actual_wbits = window_bits.clamp(8, 15);
        let window_size = 1 << actual_wbits;
        
        Self {
            level: actual_level,
            window_bits: actual_wbits,
            mem_level: mem_level.clamp(1, 9),
            strategy,
            window: vec![0; window_size * 2],
            window_pos: 0,
            window_len: 0,
            hash_table: vec![0; HASH_SIZE],
            hash_chain: vec![0; window_size],
            output: Vec::with_capacity(4096),
            bit_buffer: 0,
            bit_count: 0,
            pending: Vec::with_capacity(4096),
            header_written: false,
            finished: false,
            checksum: Adler32::new(),
        }
    }

    /// Reset compressor state
    pub fn reset(&mut self) {
        self.window_pos = 0;
        self.window_len = 0;
        for h in self.hash_table.iter_mut() {
            *h = 0;
        }
        for c in self.hash_chain.iter_mut() {
            *c = 0;
        }
        self.output.clear();
        self.bit_buffer = 0;
        self.bit_count = 0;
        self.pending.clear();
        self.header_written = false;
        self.finished = false;
        self.checksum.reset();
    }

    /// Compress input data
    pub fn compress(&mut self, input: &[u8], finish: bool) -> ZlibResult<Vec<u8>> {
        // Update checksum
        self.checksum.update(input);
        
        // Process input through LZ77
        self.lz77_compress(input);
        
        // If finishing, flush pending tokens and write end marker
        if finish {
            self.flush_block(true)?;
            self.finished = true;
        }
        
        Ok(core::mem::take(&mut self.output))
    }

    /// LZ77 compression pass
    fn lz77_compress(&mut self, input: &[u8]) {
        if self.level == 0 {
            // No compression - just store as literals
            for &byte in input {
                self.pending.push(Token::Literal(byte));
            }
            return;
        }

        let mut pos = 0;
        
        while pos < input.len() {
            // Try to find a match
            if pos + MIN_MATCH <= input.len() {
                if let Some((length, distance)) = self.find_match(input, pos) {
                    self.pending.push(Token::Match(length as u16, distance as u16));
                    
                    // Update hash for matched bytes
                    for i in 0..length {
                        if pos + i + MIN_MATCH <= input.len() {
                            self.update_hash(input, pos + i);
                        }
                    }
                    pos += length;
                    continue;
                }
            }
            
            // No match - emit literal
            self.pending.push(Token::Literal(input[pos]));
            self.update_hash(input, pos);
            pos += 1;
            
            // Flush block if pending is too large
            if self.pending.len() >= 16384 {
                let _ = self.flush_block(false);
            }
        }
    }

    /// Calculate hash for 3-byte sequence
    fn hash(&self, data: &[u8], pos: usize) -> usize {
        if pos + 2 >= data.len() {
            return 0;
        }
        let h = (data[pos] as usize) << 10
            ^ (data[pos + 1] as usize) << 5
            ^ (data[pos + 2] as usize);
        h & HASH_MASK
    }

    /// Update hash table
    fn update_hash(&mut self, data: &[u8], pos: usize) {
        if pos + MIN_MATCH > data.len() {
            return;
        }
        
        let h = self.hash(data, pos);
        let window_size = 1 << self.window_bits;
        let win_pos = self.window_pos & (window_size - 1);
        
        // Store current position's previous match
        self.hash_chain[win_pos] = self.hash_table[h];
        self.hash_table[h] = win_pos as u16;
        
        // Copy byte to window
        if pos < data.len() {
            self.window[self.window_pos] = data[pos];
            self.window_pos = (self.window_pos + 1) & (window_size * 2 - 1);
            if self.window_len < window_size {
                self.window_len += 1;
            }
        }
    }

    /// Find longest match
    fn find_match(&self, data: &[u8], pos: usize) -> Option<(usize, usize)> {
        if pos + MIN_MATCH > data.len() {
            return None;
        }
        
        let h = self.hash(data, pos);
        let window_size = 1 << self.window_bits;
        
        let mut match_pos = self.hash_table[h] as usize;
        let mut best_len = MIN_MATCH - 1;
        let mut best_dist = 0;
        
        let max_chain = match self.level {
            1 => 4,
            2 => 8,
            3 => 16,
            4 => 32,
            5 => 64,
            6 => 128,
            7 => 256,
            8 => 512,
            9 => 4096,
            _ => 32,
        };
        
        let mut chain_len = 0;
        
        while match_pos != 0 && chain_len < max_chain {
            let dist = if self.window_pos > match_pos {
                self.window_pos - match_pos
            } else {
                window_size * 2 - match_pos + self.window_pos
            };
            
            if dist > window_size || dist == 0 {
                break;
            }
            
            // Check for match
            let mut len = 0;
            let max_len = core::cmp::min(MAX_MATCH, data.len() - pos);
            
            while len < max_len {
                let win_idx = (match_pos + len) & (window_size * 2 - 1);
                if self.window[win_idx] != data[pos + len] {
                    break;
                }
                len += 1;
            }
            
            if len > best_len {
                best_len = len;
                best_dist = dist;
                
                if len >= MAX_MATCH {
                    break;
                }
            }
            
            // Follow chain
            match_pos = self.hash_chain[match_pos & (window_size - 1)] as usize;
            chain_len += 1;
        }
        
        if best_len >= MIN_MATCH {
            Some((best_len, best_dist))
        } else {
            None
        }
    }

    /// Flush pending tokens to a DEFLATE block
    fn flush_block(&mut self, final_block: bool) -> ZlibResult<()> {
        if self.pending.is_empty() && !final_block {
            return Ok(());
        }

        // Write block header
        // BFINAL (1 bit) + BTYPE (2 bits)
        let bfinal = if final_block { 1u32 } else { 0u32 };
        
        if self.level == 0 {
            // Stored block (BTYPE = 00)
            self.write_bits(bfinal | (0 << 1), 3);
            self.flush_bits_to_byte();
            
            // Write stored block
            self.write_stored_block()?;
        } else if self.should_use_fixed() {
            // Fixed Huffman (BTYPE = 01)
            self.write_bits(bfinal | (1 << 1), 3);
            self.write_fixed_block()?;
        } else {
            // Dynamic Huffman (BTYPE = 10)
            self.write_bits(bfinal | (2 << 1), 3);
            self.write_dynamic_block()?;
        }

        self.pending.clear();
        
        Ok(())
    }

    /// Decide whether to use fixed Huffman codes
    fn should_use_fixed(&self) -> bool {
        // For simplicity, use fixed codes for small blocks
        self.pending.len() < 1024
    }

    /// Write bits to output
    fn write_bits(&mut self, value: u32, count: u8) {
        self.bit_buffer |= value << self.bit_count;
        self.bit_count += count;
        
        while self.bit_count >= 8 {
            self.output.push(self.bit_buffer as u8);
            self.bit_buffer >>= 8;
            self.bit_count -= 8;
        }
    }

    /// Flush remaining bits to byte boundary
    fn flush_bits_to_byte(&mut self) {
        if self.bit_count > 0 {
            self.output.push(self.bit_buffer as u8);
            self.bit_buffer = 0;
            self.bit_count = 0;
        }
    }

    /// Write a stored (uncompressed) block
    fn write_stored_block(&mut self) -> ZlibResult<()> {
        // Collect literals
        let literals: Vec<u8> = self.pending.iter()
            .filter_map(|t| match t {
                Token::Literal(b) => Some(*b),
                Token::Match(_, _) => None,
            })
            .collect();

        let len = literals.len() as u16;
        let nlen = !len;
        
        self.output.push(len as u8);
        self.output.push((len >> 8) as u8);
        self.output.push(nlen as u8);
        self.output.push((nlen >> 8) as u8);
        self.output.extend_from_slice(&literals);
        
        Ok(())
    }

    /// Write block with fixed Huffman codes
    fn write_fixed_block(&mut self) -> ZlibResult<()> {
        let litlen_enc = HuffmanEncoder::from_lengths(&FIXED_LITLEN_LENGTHS);
        let dist_enc = HuffmanEncoder::from_lengths(&FIXED_DIST_LENGTHS);
        
        for token in self.pending.iter() {
            match *token {
                Token::Literal(byte) => {
                    let (code, bits) = litlen_enc.get(byte as usize);
                    self.write_bits_reversed(code, bits);
                }
                Token::Match(length, distance) => {
                    // Encode length
                    let (len_sym, len_extra, len_extra_bits) = encode_length(length);
                    let (code, bits) = litlen_enc.get(len_sym as usize);
                    self.write_bits_reversed(code, bits);
                    if len_extra_bits > 0 {
                        self.write_bits(len_extra as u32, len_extra_bits);
                    }
                    
                    // Encode distance
                    let (dist_sym, dist_extra, dist_extra_bits) = encode_distance(distance);
                    let (code, bits) = dist_enc.get(dist_sym as usize);
                    self.write_bits_reversed(code, bits);
                    if dist_extra_bits > 0 {
                        self.write_bits(dist_extra as u32, dist_extra_bits);
                    }
                }
            }
        }
        
        // End of block marker (symbol 256)
        let (code, bits) = litlen_enc.get(256);
        self.write_bits_reversed(code, bits);
        
        Ok(())
    }

    /// Write block with dynamic Huffman codes
    fn write_dynamic_block(&mut self) -> ZlibResult<()> {
        // Count symbol frequencies
        let mut litlen_freqs = [0u32; LITLEN_CODES];
        let mut dist_freqs = [0u32; DIST_CODES];
        
        for token in self.pending.iter() {
            match *token {
                Token::Literal(byte) => {
                    litlen_freqs[byte as usize] += 1;
                }
                Token::Match(length, distance) => {
                    let (len_sym, _, _) = encode_length(length);
                    litlen_freqs[len_sym as usize] += 1;
                    
                    let (dist_sym, _, _) = encode_distance(distance);
                    dist_freqs[dist_sym as usize] += 1;
                }
            }
        }
        
        // End of block marker
        litlen_freqs[256] = 1;
        
        // Build Huffman codes
        let litlen_lengths = build_code_lengths(&litlen_freqs, MAX_BITS as u8);
        let dist_lengths = build_code_lengths(&dist_freqs, MAX_BITS as u8);
        
        let litlen_enc = HuffmanEncoder::from_lengths(&litlen_lengths);
        let dist_enc = HuffmanEncoder::from_lengths(&dist_lengths);
        
        // Find HLIT and HDIST
        let hlit = litlen_lengths.iter().rposition(|&l| l > 0).unwrap_or(256).max(256) - 256;
        let hdist = dist_lengths.iter().rposition(|&l| l > 0).unwrap_or(0).max(0);
        
        // Encode the code lengths using run-length encoding
        let combined: Vec<u8> = litlen_lengths[..257 + hlit].iter()
            .chain(dist_lengths[..hdist + 1].iter())
            .copied()
            .collect();
        
        let (codelen_symbols, codelen_freqs) = rle_encode_lengths(&combined);
        let codelen_lengths = build_code_lengths(&codelen_freqs, 7);
        let codelen_enc = HuffmanEncoder::from_lengths(&codelen_lengths);
        
        // Find HCLEN
        let hclen = crate::huffman::CODELEN_ORDER.iter()
            .rposition(|&i| i < codelen_lengths.len() && codelen_lengths[i] > 0)
            .unwrap_or(3)
            .max(3) - 3;
        
        // Write HLIT, HDIST, HCLEN
        self.write_bits(hlit as u32, 5);
        self.write_bits(hdist as u32, 5);
        self.write_bits(hclen as u32, 4);
        
        // Write code length code lengths
        for &i in crate::huffman::CODELEN_ORDER.iter().take(hclen + 4) {
            let len = if i < codelen_lengths.len() { codelen_lengths[i] } else { 0 };
            self.write_bits(len as u32, 3);
        }
        
        // Write encoded code lengths
        for &sym in codelen_symbols.iter() {
            let (code, bits) = codelen_enc.get((sym & 0xFF) as usize);
            self.write_bits_reversed(code, bits);
            
            // Extra bits
            match sym & 0xFF {
                16 => self.write_bits(((sym >> 8) - 3) as u32, 2),
                17 => self.write_bits(((sym >> 8) - 3) as u32, 3),
                18 => self.write_bits(((sym >> 8) - 11) as u32, 7),
                _ => {}
            }
        }
        
        // Write compressed data
        for token in self.pending.iter() {
            match *token {
                Token::Literal(byte) => {
                    let (code, bits) = litlen_enc.get(byte as usize);
                    self.write_bits_reversed(code, bits);
                }
                Token::Match(length, distance) => {
                    let (len_sym, len_extra, len_extra_bits) = encode_length(length);
                    let (code, bits) = litlen_enc.get(len_sym as usize);
                    self.write_bits_reversed(code, bits);
                    if len_extra_bits > 0 {
                        self.write_bits(len_extra as u32, len_extra_bits);
                    }
                    
                    let (dist_sym, dist_extra, dist_extra_bits) = encode_distance(distance);
                    let (code, bits) = dist_enc.get(dist_sym as usize);
                    self.write_bits_reversed(code, bits);
                    if dist_extra_bits > 0 {
                        self.write_bits(dist_extra as u32, dist_extra_bits);
                    }
                }
            }
        }
        
        // End of block marker
        let (code, bits) = litlen_enc.get(256);
        self.write_bits_reversed(code, bits);
        
        Ok(())
    }

    /// Write bits in reversed order (MSB first, as per DEFLATE)
    fn write_bits_reversed(&mut self, code: u32, bits: u8) {
        let mut rev = 0u32;
        for i in 0..bits {
            if code & (1 << i) != 0 {
                rev |= 1 << (bits - 1 - i);
            }
        }
        self.write_bits(rev, bits);
    }

    /// Get current checksum
    pub fn checksum(&self) -> u32 {
        self.checksum.finalize()
    }
}

/// Encode length to symbol and extra bits
fn encode_length(length: u16) -> (u16, u16, u8) {
    let len = length as usize;
    for (i, &base) in LENGTH_BASE.iter().enumerate() {
        let next_base = if i + 1 < LENGTH_BASE.len() {
            LENGTH_BASE[i + 1] as usize
        } else {
            259
        };
        
        if len >= base as usize && len < next_base {
            let extra_bits = LENGTH_EXTRA[i];
            let extra = (len - base as usize) as u16;
            return (257 + i as u16, extra, extra_bits);
        }
    }
    // Shouldn't reach here for valid lengths
    (285, 0, 0)
}

/// Encode distance to symbol and extra bits
fn encode_distance(distance: u16) -> (u16, u16, u8) {
    let dist = distance as usize;
    for (i, &base) in DIST_BASE.iter().enumerate() {
        let next_base = if i + 1 < DIST_BASE.len() {
            DIST_BASE[i + 1] as usize
        } else {
            32769
        };
        
        if dist >= base as usize && dist < next_base {
            let extra_bits = DIST_EXTRA[i];
            let extra = (dist - base as usize) as u16;
            return (i as u16, extra, extra_bits);
        }
    }
    // Shouldn't reach here for valid distances
    (29, 0, 0)
}

/// Run-length encode code lengths for dynamic Huffman header
fn rle_encode_lengths(lengths: &[u8]) -> (Vec<u16>, [u32; 19]) {
    let mut symbols = Vec::new();
    let mut freqs = [0u32; 19];
    
    let mut i = 0;
    while i < lengths.len() {
        let len = lengths[i];
        
        if len == 0 {
            // Count consecutive zeros
            let mut count = 1;
            while i + count < lengths.len() && lengths[i + count] == 0 && count < 138 {
                count += 1;
            }
            
            if count >= 11 {
                // Symbol 18: 11-138 zeros
                symbols.push(18 | ((count as u16) << 8));
                freqs[18] += 1;
            } else if count >= 3 {
                // Symbol 17: 3-10 zeros
                symbols.push(17 | ((count as u16) << 8));
                freqs[17] += 1;
            } else {
                // Individual zeros
                for _ in 0..count {
                    symbols.push(0);
                    freqs[0] += 1;
                }
            }
            i += count;
        } else {
            // Non-zero length
            symbols.push(len as u16);
            freqs[len as usize] += 1;
            i += 1;
            
            // Check for repetition
            let mut count = 0;
            while i + count < lengths.len() && lengths[i + count] == len && count < 6 {
                count += 1;
            }
            
            if count >= 3 {
                // Symbol 16: repeat previous 3-6 times
                symbols.push(16 | ((count as u16 + 3) << 8));
                freqs[16] += 1;
                i += count;
            }
        }
    }
    
    (symbols, freqs)
}

/// Compress data to zlib format in one call
pub fn compress_to_zlib(input: &[u8], output: &mut [u8], level: c_int) -> ZlibResult<usize> {
    let actual_level = if level == -1 { 6 } else { level.clamp(0, 9) };
    
    if output.len() < compress_bound(input.len()) {
        return Err(ZlibError::BufferError);
    }
    
    let mut pos = 0;
    
    // Write zlib header
    let cmf = 0x78; // CM=8 (deflate), CINFO=7 (32K window)
    let flg = match actual_level {
        0..=1 => 0x01,  // FLEVEL=0 (fastest)
        2..=5 => 0x5E,  // FLEVEL=1 (fast)
        6 => 0x9C,      // FLEVEL=2 (default)
        _ => 0xDA,      // FLEVEL=3 (max compression)
    };
    // Adjust FLG so (CMF*256 + FLG) % 31 == 0
    let check = ((cmf as u16) << 8) | (flg as u16);
    let flg = flg + (31 - (check % 31)) as u8;
    
    output[pos] = cmf;
    output[pos + 1] = flg;
    pos += 2;
    
    // Compress data
    let mut deflater = Deflater::new(actual_level, 15, 8, 0);
    let compressed = deflater.compress(input, true)?;
    
    if pos + compressed.len() + 4 > output.len() {
        return Err(ZlibError::BufferError);
    }
    
    output[pos..pos + compressed.len()].copy_from_slice(&compressed);
    pos += compressed.len();
    
    // Write Adler-32 checksum (big-endian)
    let checksum = deflater.checksum();
    output[pos] = (checksum >> 24) as u8;
    output[pos + 1] = (checksum >> 16) as u8;
    output[pos + 2] = (checksum >> 8) as u8;
    output[pos + 3] = checksum as u8;
    pos += 4;
    
    Ok(pos)
}
