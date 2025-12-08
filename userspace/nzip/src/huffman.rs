//! Huffman Coding for DEFLATE
//!
//! Implements Huffman tree construction and decoding for DEFLATE compression.

use std::vec::Vec;

/// Maximum code length for literal/length codes
pub const MAX_BITS: usize = 15;

/// Maximum number of literal/length symbols
pub const LITLEN_CODES: usize = 288;

/// Maximum number of distance symbols
pub const DIST_CODES: usize = 32;

/// Maximum number of code length symbols
pub const CODELEN_CODES: usize = 19;

/// Code length alphabet order (as per RFC 1951)
pub const CODELEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15
];

/// Fixed literal/length code lengths (RFC 1951)
pub const FIXED_LITLEN_LENGTHS: [u8; LITLEN_CODES] = {
    let mut lengths = [0u8; LITLEN_CODES];
    let mut i = 0;
    // 0-143: 8 bits
    while i <= 143 {
        lengths[i] = 8;
        i += 1;
    }
    // 144-255: 9 bits
    while i <= 255 {
        lengths[i] = 9;
        i += 1;
    }
    // 256-279: 7 bits
    while i <= 279 {
        lengths[i] = 7;
        i += 1;
    }
    // 280-287: 8 bits
    while i <= 287 {
        lengths[i] = 8;
        i += 1;
    }
    lengths
};

/// Fixed distance code lengths (RFC 1951)
pub const FIXED_DIST_LENGTHS: [u8; DIST_CODES] = [5; DIST_CODES];

/// Length base values (RFC 1951)
pub const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13,
    15, 17, 19, 23, 27, 31, 35, 43, 51, 59,
    67, 83, 99, 115, 131, 163, 195, 227, 258
];

/// Length extra bits (RFC 1951)
pub const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1,
    1, 1, 2, 2, 2, 2, 3, 3, 3, 3,
    4, 4, 4, 4, 5, 5, 5, 5, 0
];

/// Distance base values (RFC 1951)
pub const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25,
    33, 49, 65, 97, 129, 193, 257, 385, 513, 769,
    1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577
];

/// Distance extra bits (RFC 1951)
pub const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3,
    4, 4, 5, 5, 6, 6, 7, 7, 8, 8,
    9, 9, 10, 10, 11, 11, 12, 12, 13, 13
];

/// Huffman table entry
#[derive(Clone, Copy, Default)]
pub struct HuffmanEntry {
    /// Symbol value
    pub symbol: u16,
    /// Code bit length
    pub bits: u8,
}

/// Huffman decoder table
pub struct HuffmanTable {
    /// Lookup table for fast decoding (indexed by first MAX_BITS bits)
    pub table: Vec<HuffmanEntry>,
    /// Maximum bits used in table
    pub max_bits: u8,
}

impl HuffmanTable {
    /// Create a new empty Huffman table
    pub fn new() -> Self {
        Self {
            table: vec![HuffmanEntry::default(); 1 << MAX_BITS],
            max_bits: 0,
        }
    }

    /// Build Huffman table from code lengths
    /// Returns Ok if successful, Err if invalid code lengths
    pub fn build(&mut self, lengths: &[u8], max_sym: usize) -> Result<(), ()> {
        // Count codes of each length
        let mut bl_count = [0u32; MAX_BITS + 1];
        for &len in lengths.iter().take(max_sym) {
            if len as usize > MAX_BITS {
                return Err(());
            }
            bl_count[len as usize] += 1;
        }

        // Find the maximum code length actually used
        let mut max_len = MAX_BITS;
        while max_len > 0 && bl_count[max_len] == 0 {
            max_len -= 1;
        }
        self.max_bits = max_len as u8;

        if max_len == 0 {
            // No codes at all
            return Ok(());
        }

        // Check for over-subscribed or incomplete code
        let mut code = 0i32;
        for bits in 1..=max_len {
            code = (code + bl_count[bits - 1] as i32) << 1;
            if code > (1 << bits) {
                return Err(()); // Over-subscribed
            }
        }

        // Generate codes
        let mut next_code = [0u32; MAX_BITS + 1];
        code = 0;
        for bits in 1..=MAX_BITS {
            code = (code + bl_count[bits - 1] as i32) << 1;
            next_code[bits] = code as u32;
        }

        // Clear the table
        for entry in self.table.iter_mut() {
            *entry = HuffmanEntry::default();
        }

        // Fill in the table
        for sym in 0..max_sym {
            let len = lengths[sym];
            if len == 0 {
                continue;
            }

            let code = next_code[len as usize];
            next_code[len as usize] += 1;

            // Reverse the code bits for table lookup
            let mut rev_code = 0u32;
            for i in 0..len {
                if code & (1 << i) != 0 {
                    rev_code |= 1 << (len - 1 - i);
                }
            }

            // Fill all table entries that start with this code
            let entry = HuffmanEntry {
                symbol: sym as u16,
                bits: len,
            };

            let step = 1 << len;
            let mut idx = rev_code as usize;
            while idx < (1 << MAX_BITS) {
                self.table[idx] = entry;
                idx += step;
            }
        }

        Ok(())
    }

    /// Decode a symbol from a bit stream
    /// Returns (symbol, bits_consumed)
    #[inline]
    pub fn decode(&self, bits: u32) -> (u16, u8) {
        let entry = &self.table[(bits & ((1 << MAX_BITS) - 1)) as usize];
        (entry.symbol, entry.bits)
    }
}

impl Default for HuffmanTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Huffman encoder for compression
pub struct HuffmanEncoder {
    /// Code for each symbol
    pub codes: Vec<u32>,
    /// Bit length for each symbol
    pub lengths: Vec<u8>,
    /// Number of symbols
    pub num_symbols: usize,
}

impl HuffmanEncoder {
    /// Create encoder from code lengths
    pub fn from_lengths(lengths: &[u8]) -> Self {
        let num_symbols = lengths.len();
        let mut codes = vec![0u32; num_symbols];

        // Count codes of each length
        let mut bl_count = [0u32; MAX_BITS + 1];
        for &len in lengths {
            if len > 0 {
                bl_count[len as usize] += 1;
            }
        }

        // Calculate starting code for each length
        let mut next_code = [0u32; MAX_BITS + 1];
        let mut code = 0u32;
        for bits in 1..=MAX_BITS {
            code = (code + bl_count[bits - 1]) << 1;
            next_code[bits] = code;
        }

        // Assign codes to symbols
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 {
                codes[sym] = next_code[len as usize];
                next_code[len as usize] += 1;
            }
        }

        Self {
            codes,
            lengths: lengths.to_vec(),
            num_symbols,
        }
    }

    /// Get code and length for a symbol
    #[inline]
    pub fn get(&self, symbol: usize) -> (u32, u8) {
        (self.codes[symbol], self.lengths[symbol])
    }
}

/// Build optimal Huffman code lengths from symbol frequencies
/// Uses a simplified algorithm based on package-merge
pub fn build_code_lengths(freqs: &[u32], max_len: u8) -> Vec<u8> {
    let n = freqs.len();
    let mut lengths = vec![0u8; n];

    // Count non-zero frequencies
    let active: Vec<usize> = freqs.iter()
        .enumerate()
        .filter(|(_, &f)| f > 0)
        .map(|(i, _)| i)
        .collect();

    if active.is_empty() {
        return lengths;
    }

    if active.len() == 1 {
        // Only one symbol - give it a 1-bit code
        lengths[active[0]] = 1;
        return lengths;
    }

    if active.len() == 2 {
        // Two symbols - each gets a 1-bit code
        lengths[active[0]] = 1;
        lengths[active[1]] = 1;
        return lengths;
    }

    // Build Huffman tree using a heap-based algorithm
    // Create leaf nodes
    let mut nodes: Vec<(u64, Vec<usize>)> = active.iter()
        .map(|&i| (freqs[i] as u64, vec![i]))
        .collect();

    // Simple sorting-based Huffman (not the most efficient but correct)
    while nodes.len() > 1 {
        // Sort by frequency (ascending)
        nodes.sort_by_key(|(f, _)| *f);

        // Combine two lowest frequency nodes
        let (f1, s1) = nodes.remove(0);
        let (f2, s2) = nodes.remove(0);

        let mut combined = s1;
        combined.extend(s2);
        nodes.push((f1 + f2, combined));

        // Increment length for combined symbols
        for &sym in nodes.last().unwrap().1.iter() {
            lengths[sym] += 1;
        }
    }

    // Limit code lengths to max_len
    limit_code_lengths(&mut lengths, max_len);

    lengths
}

/// Limit code lengths to a maximum value
fn limit_code_lengths(lengths: &mut [u8], max_len: u8) {
    // Count symbols at each length
    let mut counts = [0u32; MAX_BITS + 1];
    for &len in lengths.iter() {
        if len > 0 {
            counts[len.min(max_len) as usize] += 1;
        }
    }

    // Check if we need to limit
    let mut overflow = 0i32;
    for bits in (max_len as usize + 1)..=MAX_BITS {
        overflow += counts[bits] as i32;
        counts[bits] = 0;
    }

    if overflow == 0 {
        return;
    }

    // Adjust counts to remove overflow
    counts[max_len as usize] += overflow as u32;

    // Recalculate from counts
    while overflow > 0 {
        // Find the highest bit level we can borrow from
        let mut bits = max_len as usize - 1;
        while bits > 0 && counts[bits] == 0 {
            bits -= 1;
        }

        if bits == 0 {
            break;
        }

        // Move one symbol down, creating two at the next level
        counts[bits] -= 1;
        counts[bits + 1] += 2;
        counts[max_len as usize] -= 1;
        overflow -= 1;
    }

    // Assign lengths based on counts
    let mut idx = 0;
    for bits in 1..=max_len as usize {
        for _ in 0..counts[bits] {
            while idx < lengths.len() && lengths[idx] == 0 {
                idx += 1;
            }
            if idx < lengths.len() {
                lengths[idx] = bits as u8;
                idx += 1;
            }
        }
    }
}
