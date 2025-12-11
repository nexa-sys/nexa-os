//! QPACK Header Compression
//!
//! This module implements QPACK (RFC 9204) for HTTP/3 header compression.
//!
//! ## Overview
//!
//! QPACK is a header compression format for HTTP/3, based on HPACK (HTTP/2)
//! but designed to avoid head-of-line blocking in QUIC.
//!
//! ## Components
//!
//! - **Static Table**: Pre-defined header fields (99 entries)
//! - **Dynamic Table**: Connection-specific header fields
//! - **Encoder Stream**: Sends table updates from encoder to decoder
//! - **Decoder Stream**: Sends acknowledgments from decoder to encoder

use crate::error::{Error, NgError, Result};

use std::collections::{HashMap, VecDeque};

// ============================================================================
// Constants
// ============================================================================

/// Maximum dynamic table size (default)
pub const DEFAULT_MAX_TABLE_CAPACITY: usize = 4096;

/// Maximum blocked streams (default)
pub const DEFAULT_MAX_BLOCKED_STREAMS: usize = 100;

// ============================================================================
// Static Table (RFC 9204 Appendix A)
// ============================================================================

/// Static table entry
#[derive(Debug, Clone)]
pub struct StaticEntry {
    /// Index in table
    pub index: u8,
    /// Header name
    pub name: &'static [u8],
    /// Header value (may be empty)
    pub value: &'static [u8],
}

/// Static table entries
pub static STATIC_TABLE: &[StaticEntry] = &[
    StaticEntry { index: 0, name: b":authority", value: b"" },
    StaticEntry { index: 1, name: b":path", value: b"/" },
    StaticEntry { index: 2, name: b"age", value: b"0" },
    StaticEntry { index: 3, name: b"content-disposition", value: b"" },
    StaticEntry { index: 4, name: b"content-length", value: b"0" },
    StaticEntry { index: 5, name: b"cookie", value: b"" },
    StaticEntry { index: 6, name: b"date", value: b"" },
    StaticEntry { index: 7, name: b"etag", value: b"" },
    StaticEntry { index: 8, name: b"if-modified-since", value: b"" },
    StaticEntry { index: 9, name: b"if-none-match", value: b"" },
    StaticEntry { index: 10, name: b"last-modified", value: b"" },
    StaticEntry { index: 11, name: b"link", value: b"" },
    StaticEntry { index: 12, name: b"location", value: b"" },
    StaticEntry { index: 13, name: b"referer", value: b"" },
    StaticEntry { index: 14, name: b"set-cookie", value: b"" },
    StaticEntry { index: 15, name: b":method", value: b"CONNECT" },
    StaticEntry { index: 16, name: b":method", value: b"DELETE" },
    StaticEntry { index: 17, name: b":method", value: b"GET" },
    StaticEntry { index: 18, name: b":method", value: b"HEAD" },
    StaticEntry { index: 19, name: b":method", value: b"OPTIONS" },
    StaticEntry { index: 20, name: b":method", value: b"POST" },
    StaticEntry { index: 21, name: b":method", value: b"PUT" },
    StaticEntry { index: 22, name: b":scheme", value: b"http" },
    StaticEntry { index: 23, name: b":scheme", value: b"https" },
    StaticEntry { index: 24, name: b":status", value: b"103" },
    StaticEntry { index: 25, name: b":status", value: b"200" },
    StaticEntry { index: 26, name: b":status", value: b"304" },
    StaticEntry { index: 27, name: b":status", value: b"404" },
    StaticEntry { index: 28, name: b":status", value: b"503" },
    StaticEntry { index: 29, name: b"accept", value: b"*/*" },
    StaticEntry { index: 30, name: b"accept", value: b"application/dns-message" },
    StaticEntry { index: 31, name: b"accept-encoding", value: b"gzip, deflate, br" },
    StaticEntry { index: 32, name: b"accept-ranges", value: b"bytes" },
    StaticEntry { index: 33, name: b"access-control-allow-headers", value: b"cache-control" },
    StaticEntry { index: 34, name: b"access-control-allow-headers", value: b"content-type" },
    StaticEntry { index: 35, name: b"access-control-allow-origin", value: b"*" },
    StaticEntry { index: 36, name: b"cache-control", value: b"max-age=0" },
    StaticEntry { index: 37, name: b"cache-control", value: b"max-age=2592000" },
    StaticEntry { index: 38, name: b"cache-control", value: b"max-age=604800" },
    StaticEntry { index: 39, name: b"cache-control", value: b"no-cache" },
    StaticEntry { index: 40, name: b"cache-control", value: b"no-store" },
    StaticEntry { index: 41, name: b"cache-control", value: b"public, max-age=31536000" },
    StaticEntry { index: 42, name: b"content-encoding", value: b"br" },
    StaticEntry { index: 43, name: b"content-encoding", value: b"gzip" },
    StaticEntry { index: 44, name: b"content-type", value: b"application/dns-message" },
    StaticEntry { index: 45, name: b"content-type", value: b"application/javascript" },
    StaticEntry { index: 46, name: b"content-type", value: b"application/json" },
    StaticEntry { index: 47, name: b"content-type", value: b"application/x-www-form-urlencoded" },
    StaticEntry { index: 48, name: b"content-type", value: b"image/gif" },
    StaticEntry { index: 49, name: b"content-type", value: b"image/jpeg" },
    StaticEntry { index: 50, name: b"content-type", value: b"image/png" },
    StaticEntry { index: 51, name: b"content-type", value: b"text/css" },
    StaticEntry { index: 52, name: b"content-type", value: b"text/html; charset=utf-8" },
    StaticEntry { index: 53, name: b"content-type", value: b"text/plain" },
    StaticEntry { index: 54, name: b"content-type", value: b"text/plain;charset=utf-8" },
    StaticEntry { index: 55, name: b"range", value: b"bytes=0-" },
    StaticEntry { index: 56, name: b"strict-transport-security", value: b"max-age=31536000" },
    StaticEntry { index: 57, name: b"strict-transport-security", value: b"max-age=31536000; includesubdomains" },
    StaticEntry { index: 58, name: b"strict-transport-security", value: b"max-age=31536000; includesubdomains; preload" },
    StaticEntry { index: 59, name: b"vary", value: b"accept-encoding" },
    StaticEntry { index: 60, name: b"vary", value: b"origin" },
    StaticEntry { index: 61, name: b"x-content-type-options", value: b"nosniff" },
    StaticEntry { index: 62, name: b"x-xss-protection", value: b"1; mode=block" },
    StaticEntry { index: 63, name: b":status", value: b"100" },
    StaticEntry { index: 64, name: b":status", value: b"204" },
    StaticEntry { index: 65, name: b":status", value: b"206" },
    StaticEntry { index: 66, name: b":status", value: b"302" },
    StaticEntry { index: 67, name: b":status", value: b"400" },
    StaticEntry { index: 68, name: b":status", value: b"403" },
    StaticEntry { index: 69, name: b":status", value: b"421" },
    StaticEntry { index: 70, name: b":status", value: b"425" },
    StaticEntry { index: 71, name: b":status", value: b"500" },
    StaticEntry { index: 72, name: b"accept-language", value: b"" },
    StaticEntry { index: 73, name: b"access-control-allow-credentials", value: b"FALSE" },
    StaticEntry { index: 74, name: b"access-control-allow-credentials", value: b"TRUE" },
    StaticEntry { index: 75, name: b"access-control-allow-headers", value: b"*" },
    StaticEntry { index: 76, name: b"access-control-allow-methods", value: b"get" },
    StaticEntry { index: 77, name: b"access-control-allow-methods", value: b"get, post, options" },
    StaticEntry { index: 78, name: b"access-control-allow-methods", value: b"options" },
    StaticEntry { index: 79, name: b"access-control-expose-headers", value: b"content-length" },
    StaticEntry { index: 80, name: b"access-control-request-headers", value: b"content-type" },
    StaticEntry { index: 81, name: b"access-control-request-method", value: b"get" },
    StaticEntry { index: 82, name: b"access-control-request-method", value: b"post" },
    StaticEntry { index: 83, name: b"alt-svc", value: b"clear" },
    StaticEntry { index: 84, name: b"authorization", value: b"" },
    StaticEntry { index: 85, name: b"content-security-policy", value: b"script-src 'none'; object-src 'none'; base-uri 'none'" },
    StaticEntry { index: 86, name: b"early-data", value: b"1" },
    StaticEntry { index: 87, name: b"expect-ct", value: b"" },
    StaticEntry { index: 88, name: b"forwarded", value: b"" },
    StaticEntry { index: 89, name: b"if-range", value: b"" },
    StaticEntry { index: 90, name: b"origin", value: b"" },
    StaticEntry { index: 91, name: b"purpose", value: b"prefetch" },
    StaticEntry { index: 92, name: b"server", value: b"" },
    StaticEntry { index: 93, name: b"timing-allow-origin", value: b"*" },
    StaticEntry { index: 94, name: b"upgrade-insecure-requests", value: b"1" },
    StaticEntry { index: 95, name: b"user-agent", value: b"" },
    StaticEntry { index: 96, name: b"x-forwarded-for", value: b"" },
    StaticEntry { index: 97, name: b"x-frame-options", value: b"deny" },
    StaticEntry { index: 98, name: b"x-frame-options", value: b"sameorigin" },
];

// ============================================================================
// Dynamic Table
// ============================================================================

/// Dynamic table entry
#[derive(Debug, Clone)]
pub struct DynamicEntry {
    /// Header name
    pub name: Vec<u8>,
    /// Header value
    pub value: Vec<u8>,
    /// Entry size (name length + value length + 32)
    pub size: usize,
    /// Absolute index
    pub absolute_index: u64,
}

impl DynamicEntry {
    /// Create a new dynamic entry
    pub fn new(name: Vec<u8>, value: Vec<u8>, absolute_index: u64) -> Self {
        let size = name.len() + value.len() + 32;
        Self {
            name,
            value,
            size,
            absolute_index,
        }
    }
}

/// Dynamic table for QPACK
pub struct DynamicTable {
    /// Entries (most recent first)
    entries: VecDeque<DynamicEntry>,
    /// Current size
    size: usize,
    /// Maximum capacity
    max_capacity: usize,
    /// Inserted count (for absolute indexing)
    inserted_count: u64,
    /// Known received count (from decoder)
    known_received_count: u64,
}

impl DynamicTable {
    /// Create a new dynamic table
    pub fn new(max_capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            size: 0,
            max_capacity,
            inserted_count: 0,
            known_received_count: 0,
        }
    }

    /// Set maximum capacity
    pub fn set_max_capacity(&mut self, capacity: usize) {
        self.max_capacity = capacity;
        self.evict();
    }

    /// Insert a new entry
    pub fn insert(&mut self, name: Vec<u8>, value: Vec<u8>) -> u64 {
        let entry = DynamicEntry::new(name, value, self.inserted_count);
        let size = entry.size;
        let index = entry.absolute_index;

        // Evict if needed
        while self.size + size > self.max_capacity && !self.entries.is_empty() {
            if let Some(evicted) = self.entries.pop_back() {
                self.size -= evicted.size;
            }
        }

        // Insert if fits
        if self.size + size <= self.max_capacity {
            self.size += size;
            self.entries.push_front(entry);
            self.inserted_count += 1;
        }

        index
    }

    /// Duplicate an entry
    pub fn duplicate(&mut self, index: u64) -> Result<u64> {
        let entry = self.get_by_absolute(index)?.clone();
        Ok(self.insert(entry.name, entry.value))
    }

    /// Get entry by absolute index
    pub fn get_by_absolute(&self, absolute_index: u64) -> Result<&DynamicEntry> {
        for entry in &self.entries {
            if entry.absolute_index == absolute_index {
                return Ok(entry);
            }
        }
        Err(Error::Ng(NgError::Proto))
    }

    /// Get entry by relative index
    pub fn get_by_relative(&self, relative_index: usize) -> Result<&DynamicEntry> {
        self.entries
            .get(relative_index)
            .ok_or(Error::Ng(NgError::Proto))
    }

    /// Convert relative index to absolute
    pub fn relative_to_absolute(&self, relative_index: usize) -> Option<u64> {
        self.entries.get(relative_index).map(|e| e.absolute_index)
    }

    /// Find entry by name and value
    pub fn find(&self, name: &[u8], value: &[u8]) -> Option<(u64, bool)> {
        let mut name_match = None;

        for entry in &self.entries {
            if entry.name == name {
                if entry.value == value {
                    return Some((entry.absolute_index, true));
                }
                if name_match.is_none() {
                    name_match = Some(entry.absolute_index);
                }
            }
        }

        name_match.map(|idx| (idx, false))
    }

    /// Evict entries to fit within capacity
    fn evict(&mut self) {
        while self.size > self.max_capacity && !self.entries.is_empty() {
            if let Some(evicted) = self.entries.pop_back() {
                self.size -= evicted.size;
            }
        }
    }

    /// Get inserted count
    pub fn inserted_count(&self) -> u64 {
        self.inserted_count
    }

    /// Get known received count
    pub fn known_received_count(&self) -> u64 {
        self.known_received_count
    }

    /// Update known received count
    pub fn set_known_received_count(&mut self, count: u64) {
        self.known_received_count = count;
    }

    /// Get current size
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get entry count
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if table is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ============================================================================
// Encoder
// ============================================================================

/// QPACK encoder
pub struct Encoder {
    /// Dynamic table
    table: DynamicTable,
    /// Maximum blocked streams
    max_blocked_streams: usize,
    /// Current blocked streams count
    blocked_streams: usize,
    /// Use dynamic table
    use_dynamic_table: bool,
    /// Use Huffman encoding
    use_huffman: bool,
}

impl Encoder {
    /// Create a new encoder
    pub fn new(max_table_capacity: usize, max_blocked_streams: usize) -> Self {
        Self {
            table: DynamicTable::new(max_table_capacity),
            max_blocked_streams,
            blocked_streams: 0,
            use_dynamic_table: true,
            use_huffman: true,
        }
    }

    /// Set maximum table capacity (from decoder's settings)
    pub fn set_max_table_capacity(&mut self, capacity: usize) {
        self.table.set_max_capacity(capacity);
    }

    /// Set maximum blocked streams
    pub fn set_max_blocked_streams(&mut self, max: usize) {
        self.max_blocked_streams = max;
    }

    /// Configure Huffman encoding
    pub fn set_huffman(&mut self, enabled: bool) {
        self.use_huffman = enabled;
    }

    /// Configure dynamic table usage
    pub fn set_dynamic_table(&mut self, enabled: bool) {
        self.use_dynamic_table = enabled;
    }

    /// Encode a header field
    ///
    /// Returns (encoded_field, encoder_stream_data)
    pub fn encode_field(
        &mut self,
        name: &[u8],
        value: &[u8],
    ) -> (Vec<u8>, Vec<u8>) {
        let mut field = Vec::new();
        let mut stream = Vec::new();

        // Try static table first
        if let Some((index, full_match)) = self.find_static(name, value) {
            if full_match {
                // Indexed field line (static)
                self.encode_indexed_static(&mut field, index);
                return (field, stream);
            }
        }

        // Try dynamic table
        if self.use_dynamic_table {
            if let Some((abs_idx, full_match)) = self.table.find(name, value) {
                if full_match && abs_idx < self.table.known_received_count() {
                    // Indexed field line (dynamic, non-blocking)
                    self.encode_indexed_dynamic(&mut field, abs_idx);
                    return (field, stream);
                }
            }
        }

        // Check static table for name match
        if let Some((index, _)) = self.find_static(name, value) {
            // Literal with name reference (static)
            self.encode_literal_static_name(&mut field, index, value);
            return (field, stream);
        }

        // Literal without name reference
        self.encode_literal(&mut field, name, value);
        (field, stream)
    }

    /// Encode a header block
    ///
    /// Returns (header_block, encoder_stream_data)
    pub fn encode(
        &mut self,
        headers: &[(&[u8], &[u8])],
    ) -> (Vec<u8>, Vec<u8>) {
        let mut block = Vec::new();
        let mut stream = Vec::new();

        // Required insert count (0 for now - no dynamic table references)
        let ric = 0u64;
        // Base (0 for now)
        let base = 0i64;

        // Encode prefix
        self.encode_prefix(&mut block, ric, base);

        // Encode each header
        for (name, value) in headers {
            let (field_data, stream_data) = self.encode_field(name, value);
            block.extend(field_data);
            stream.extend(stream_data);
        }

        (block, stream)
    }

    /// Find in static table
    fn find_static(&self, name: &[u8], value: &[u8]) -> Option<(u8, bool)> {
        let mut name_match = None;

        for entry in STATIC_TABLE {
            if entry.name == name {
                if entry.value == value {
                    return Some((entry.index, true));
                }
                if name_match.is_none() {
                    name_match = Some(entry.index);
                }
            }
        }

        name_match.map(|idx| (idx, false))
    }

    /// Encode prefix (Required Insert Count and Base)
    fn encode_prefix(&self, dest: &mut Vec<u8>, ric: u64, base: i64) {
        // Required Insert Count (encoded)
        self.encode_integer(dest, ric, 0, 8);

        // Base (sign bit + delta)
        if base >= 0 {
            self.encode_integer(dest, base as u64, 0, 7);
        } else {
            self.encode_integer(dest, (-base - 1) as u64, 0x80, 7);
        }
    }

    /// Encode indexed field (static)
    fn encode_indexed_static(&self, dest: &mut Vec<u8>, index: u8) {
        // 1 1 T=1 Index
        self.encode_integer(dest, index as u64, 0xc0, 6);
    }

    /// Encode indexed field (dynamic)
    fn encode_indexed_dynamic(&self, dest: &mut Vec<u8>, abs_index: u64) {
        // 1 0 Index
        let rel_index = self.table.inserted_count() - abs_index - 1;
        self.encode_integer(dest, rel_index, 0x80, 6);
    }

    /// Encode literal with static name reference
    fn encode_literal_static_name(&self, dest: &mut Vec<u8>, name_index: u8, value: &[u8]) {
        // 0 1 N T=1 Index
        self.encode_integer(dest, name_index as u64, 0x50, 4);

        // Value
        if self.use_huffman {
            self.encode_string_huffman(dest, value);
        } else {
            self.encode_string_literal(dest, value);
        }
    }

    /// Encode literal without name reference
    fn encode_literal(&self, dest: &mut Vec<u8>, name: &[u8], value: &[u8]) {
        // 0 0 1 N Name
        dest.push(0x20);

        // Name
        if self.use_huffman {
            self.encode_string_huffman(dest, name);
        } else {
            self.encode_string_literal(dest, name);
        }

        // Value
        if self.use_huffman {
            self.encode_string_huffman(dest, value);
        } else {
            self.encode_string_literal(dest, value);
        }
    }

    /// Encode integer with prefix
    fn encode_integer(&self, dest: &mut Vec<u8>, value: u64, prefix: u8, n: usize) {
        let max_first = (1 << n) - 1;

        if value < max_first as u64 {
            dest.push(prefix | value as u8);
        } else {
            dest.push(prefix | max_first);
            let mut v = value - max_first as u64;

            while v >= 128 {
                dest.push((v as u8 & 0x7f) | 0x80);
                v >>= 7;
            }
            dest.push(v as u8);
        }
    }

    /// Encode string literal (no Huffman)
    fn encode_string_literal(&self, dest: &mut Vec<u8>, s: &[u8]) {
        // H=0, length
        self.encode_integer(dest, s.len() as u64, 0, 7);
        dest.extend_from_slice(s);
    }

    /// Encode string with Huffman
    fn encode_string_huffman(&self, dest: &mut Vec<u8>, s: &[u8]) {
        // For simplicity, fall back to literal encoding
        // Full implementation would use Huffman coding table
        self.encode_string_literal(dest, s);
    }

    /// Get dynamic table reference
    pub fn table(&self) -> &DynamicTable {
        &self.table
    }

    /// Acknowledge an insert
    pub fn on_insert_count_increment(&mut self, increment: u64) {
        let new_count = self.table.known_received_count() + increment;
        self.table.set_known_received_count(new_count);
    }

    /// Process section acknowledgment
    pub fn on_section_acknowledgment(&mut self, stream_id: u64) {
        // Mark sections as acknowledged
        if self.blocked_streams > 0 {
            self.blocked_streams -= 1;
        }
    }
}

// ============================================================================
// Decoder
// ============================================================================

/// QPACK decoder
pub struct Decoder {
    /// Dynamic table
    table: DynamicTable,
    /// Maximum blocked streams
    max_blocked_streams: usize,
    /// Blocked streams waiting for table updates
    blocked: HashMap<u64, Vec<u8>>,
}

impl Decoder {
    /// Create a new decoder
    pub fn new(max_table_capacity: usize, max_blocked_streams: usize) -> Self {
        Self {
            table: DynamicTable::new(max_table_capacity),
            max_blocked_streams,
            blocked: HashMap::new(),
        }
    }

    /// Set maximum table capacity
    pub fn set_max_table_capacity(&mut self, capacity: usize) {
        self.table.set_max_capacity(capacity);
    }

    /// Process encoder stream data
    pub fn process_encoder_stream(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let mut offset = 0;
        let mut decoder_stream = Vec::new();

        while offset < data.len() {
            let first = data[offset];

            if first & 0x80 != 0 {
                // Insert with name reference
                let static_ref = (first & 0x40) != 0;
                let (name_index, consumed) = self.decode_integer(&data[offset..], 6)?;
                offset += consumed;

                // Get name
                let name = if static_ref {
                    STATIC_TABLE
                        .get(name_index as usize)
                        .ok_or(Error::Ng(NgError::Proto))?
                        .name
                        .to_vec()
                } else {
                    self.table.get_by_relative(name_index as usize)?.name.clone()
                };

                // Decode value
                let (value, consumed) = self.decode_string(&data[offset..])?;
                offset += consumed;

                self.table.insert(name, value);
            } else if first & 0x40 != 0 {
                // Insert without name reference
                let (name, consumed) = self.decode_string(&data[offset..])?;
                offset += consumed;

                let (value, consumed) = self.decode_string(&data[offset..])?;
                offset += consumed;

                self.table.insert(name, value);
            } else if first & 0x20 != 0 {
                // Set dynamic table capacity
                let (capacity, consumed) = self.decode_integer(&data[offset..], 5)?;
                offset += consumed;
                self.table.set_max_capacity(capacity as usize);
            } else {
                // Duplicate
                let (index, consumed) = self.decode_integer(&data[offset..], 5)?;
                offset += consumed;
                self.table.duplicate(index)?;
            }
        }

        // Send insert count increment
        let ici = self.table.inserted_count();
        if ici > 0 {
            self.encode_insert_count_increment(&mut decoder_stream, ici);
        }

        Ok(decoder_stream)
    }

    /// Decode a header block
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        let mut offset = 0;

        // Decode prefix
        let (ric, consumed) = self.decode_integer(&data[offset..], 8)?;
        offset += consumed;

        let (base_delta, consumed) = self.decode_integer(&data[offset..], 7)?;
        offset += consumed;

        let sign = (data[offset - consumed] & 0x80) != 0;
        let _base = if sign {
            -(base_delta as i64 + 1)
        } else {
            base_delta as i64
        };

        // Check if we need to wait for table updates
        if ric > self.table.inserted_count() {
            return Err(Error::Ng(NgError::Proto)); // Should block
        }

        // Decode fields
        let mut headers = Vec::new();

        while offset < data.len() {
            let first = data[offset];

            if first & 0x80 != 0 {
                // Indexed field
                if first & 0x40 != 0 {
                    // Static
                    let (index, consumed) = self.decode_integer(&data[offset..], 6)?;
                    offset += consumed;

                    let entry = STATIC_TABLE
                        .get(index as usize)
                        .ok_or(Error::Ng(NgError::Proto))?;
                    headers.push((entry.name.to_vec(), entry.value.to_vec()));
                } else {
                    // Dynamic (relative)
                    let (index, consumed) = self.decode_integer(&data[offset..], 6)?;
                    offset += consumed;

                    let entry = self.table.get_by_relative(index as usize)?;
                    headers.push((entry.name.clone(), entry.value.clone()));
                }
            } else if first & 0x40 != 0 {
                // Literal with name reference
                let static_ref = (first & 0x10) != 0;
                let (name_index, consumed) = self.decode_integer(&data[offset..], 4)?;
                offset += consumed;

                let name = if static_ref {
                    STATIC_TABLE
                        .get(name_index as usize)
                        .ok_or(Error::Ng(NgError::Proto))?
                        .name
                        .to_vec()
                } else {
                    self.table.get_by_relative(name_index as usize)?.name.clone()
                };

                let (value, consumed) = self.decode_string(&data[offset..])?;
                offset += consumed;

                headers.push((name, value));
            } else if first & 0x20 != 0 {
                // Literal without name reference
                let (name, consumed) = self.decode_string(&data[offset..])?;
                offset += consumed;

                let (value, consumed) = self.decode_string(&data[offset..])?;
                offset += consumed;

                headers.push((name, value));
            } else if first & 0x10 != 0 {
                // Indexed field with post-base index
                let (index, consumed) = self.decode_integer(&data[offset..], 4)?;
                offset += consumed;

                let entry = self.table.get_by_relative(index as usize)?;
                headers.push((entry.name.clone(), entry.value.clone()));
            } else {
                // Literal with post-base name reference
                let (name_index, consumed) = self.decode_integer(&data[offset..], 3)?;
                offset += consumed;

                let entry = self.table.get_by_relative(name_index as usize)?;
                let name = entry.name.clone();

                let (value, consumed) = self.decode_string(&data[offset..])?;
                offset += consumed;

                headers.push((name, value));
            }
        }

        Ok(headers)
    }

    /// Decode an integer
    fn decode_integer(&self, data: &[u8], n: usize) -> Result<(u64, usize)> {
        if data.is_empty() {
            return Err(Error::Ng(NgError::Proto));
        }

        let max_first = (1 << n) - 1;
        let mut value = (data[0] & max_first) as u64;
        let mut offset = 1;

        if value == max_first as u64 {
            let mut m = 0u64;
            loop {
                if offset >= data.len() {
                    return Err(Error::Ng(NgError::Proto));
                }
                let b = data[offset] as u64;
                offset += 1;
                value += (b & 0x7f) << m;
                m += 7;
                if b & 0x80 == 0 {
                    break;
                }
                if m > 63 {
                    return Err(Error::Ng(NgError::Proto));
                }
            }
        }

        Ok((value, offset))
    }

    /// Decode a string
    fn decode_string(&self, data: &[u8]) -> Result<(Vec<u8>, usize)> {
        if data.is_empty() {
            return Err(Error::Ng(NgError::Proto));
        }

        let huffman = (data[0] & 0x80) != 0;
        let (length, consumed) = self.decode_integer(data, 7)?;
        let length = length as usize;

        if consumed + length > data.len() {
            return Err(Error::Ng(NgError::Proto));
        }

        let string_data = &data[consumed..consumed + length];
        let value = if huffman {
            // Huffman decoding (simplified - use literal for now)
            string_data.to_vec()
        } else {
            string_data.to_vec()
        };

        Ok((value, consumed + length))
    }

    /// Encode insert count increment
    fn encode_insert_count_increment(&self, dest: &mut Vec<u8>, increment: u64) {
        // 0 0 Increment
        if increment < 64 {
            dest.push(increment as u8);
        } else {
            dest.push(0x3f);
            let mut v = increment - 63;
            while v >= 128 {
                dest.push((v as u8 & 0x7f) | 0x80);
                v >>= 7;
            }
            dest.push(v as u8);
        }
    }

    /// Get dynamic table reference
    pub fn table(&self) -> &DynamicTable {
        &self.table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_table() {
        assert_eq!(STATIC_TABLE.len(), 99);
        assert_eq!(STATIC_TABLE[0].name, b":authority");
        assert_eq!(STATIC_TABLE[17].value, b"GET");
    }

    #[test]
    fn test_dynamic_table() {
        let mut table = DynamicTable::new(1024);

        let idx = table.insert(b"custom".to_vec(), b"value".to_vec());
        assert_eq!(idx, 0);
        assert_eq!(table.len(), 1);

        let entry = table.get_by_absolute(0).unwrap();
        assert_eq!(entry.name, b"custom");
        assert_eq!(entry.value, b"value");
    }

    #[test]
    fn test_encoder_basic() {
        let mut encoder = Encoder::new(4096, 100);

        let (block, _stream) = encoder.encode(&[
            (b":method", b"GET"),
            (b":path", b"/"),
        ]);

        assert!(!block.is_empty());
    }

    #[test]
    fn test_decoder_basic() {
        let mut encoder = Encoder::new(4096, 100);
        let mut decoder = Decoder::new(4096, 100);

        let (block, stream) = encoder.encode(&[
            (b":method", b"GET"),
            (b":path", b"/"),
        ]);

        let headers = decoder.decode(&block).unwrap();
        assert_eq!(headers.len(), 2);
    }
}
