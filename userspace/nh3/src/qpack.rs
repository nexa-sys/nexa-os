//! QPACK Header Compression (RFC 9204)
//!
//! This module implements the QPACK header compression algorithm used in HTTP/3.
//! QPACK is based on HPACK but designed for QUIC's out-of-order delivery.

use crate::constants::qpack::*;
use crate::error::{Error, ErrorCode, QpackError, Result};
use crate::types::HeaderField;
use std::collections::VecDeque;

// ============================================================================
// Static Table (RFC 9204 Appendix A)
// ============================================================================

/// Static table entries (indices 0-98)
const STATIC_TABLE: [(&[u8], &[u8]); 99] = [
    (b":authority", b""),
    (b":path", b"/"),
    (b"age", b"0"),
    (b"content-disposition", b""),
    (b"content-length", b"0"),
    (b"cookie", b""),
    (b"date", b""),
    (b"etag", b""),
    (b"if-modified-since", b""),
    (b"if-none-match", b""),
    (b"last-modified", b""),
    (b"link", b""),
    (b"location", b""),
    (b"referer", b""),
    (b"set-cookie", b""),
    (b":method", b"CONNECT"),
    (b":method", b"DELETE"),
    (b":method", b"GET"),
    (b":method", b"HEAD"),
    (b":method", b"OPTIONS"),
    (b":method", b"POST"),
    (b":method", b"PUT"),
    (b":scheme", b"http"),
    (b":scheme", b"https"),
    (b":status", b"103"),
    (b":status", b"200"),
    (b":status", b"304"),
    (b":status", b"404"),
    (b":status", b"503"),
    (b"accept", b"*/*"),
    (b"accept", b"application/dns-message"),
    (b"accept-encoding", b"gzip, deflate, br"),
    (b"accept-ranges", b"bytes"),
    (b"access-control-allow-headers", b"cache-control"),
    (b"access-control-allow-headers", b"content-type"),
    (b"access-control-allow-origin", b"*"),
    (b"cache-control", b"max-age=0"),
    (b"cache-control", b"max-age=2592000"),
    (b"cache-control", b"max-age=604800"),
    (b"cache-control", b"no-cache"),
    (b"cache-control", b"no-store"),
    (b"cache-control", b"public, max-age=31536000"),
    (b"content-encoding", b"br"),
    (b"content-encoding", b"gzip"),
    (b"content-type", b"application/dns-message"),
    (b"content-type", b"application/javascript"),
    (b"content-type", b"application/json"),
    (b"content-type", b"application/x-www-form-urlencoded"),
    (b"content-type", b"image/gif"),
    (b"content-type", b"image/jpeg"),
    (b"content-type", b"image/png"),
    (b"content-type", b"text/css"),
    (b"content-type", b"text/html; charset=utf-8"),
    (b"content-type", b"text/plain"),
    (b"content-type", b"text/plain;charset=utf-8"),
    (b"range", b"bytes=0-"),
    (b"strict-transport-security", b"max-age=31536000"),
    (b"strict-transport-security", b"max-age=31536000; includesubdomains"),
    (b"strict-transport-security", b"max-age=31536000; includesubdomains; preload"),
    (b"vary", b"accept-encoding"),
    (b"vary", b"origin"),
    (b"x-content-type-options", b"nosniff"),
    (b"x-xss-protection", b"1; mode=block"),
    (b":status", b"100"),
    (b":status", b"204"),
    (b":status", b"206"),
    (b":status", b"302"),
    (b":status", b"400"),
    (b":status", b"403"),
    (b":status", b"421"),
    (b":status", b"425"),
    (b":status", b"500"),
    (b"accept-language", b""),
    (b"access-control-allow-credentials", b"FALSE"),
    (b"access-control-allow-credentials", b"TRUE"),
    (b"access-control-allow-headers", b"*"),
    (b"access-control-allow-methods", b"get"),
    (b"access-control-allow-methods", b"get, post, options"),
    (b"access-control-allow-methods", b"options"),
    (b"access-control-expose-headers", b"content-length"),
    (b"access-control-request-headers", b"content-type"),
    (b"access-control-request-method", b"get"),
    (b"access-control-request-method", b"post"),
    (b"alt-svc", b"clear"),
    (b"authorization", b""),
    (b"content-security-policy", b"script-src 'none'; object-src 'none'; base-uri 'none'"),
    (b"early-data", b"1"),
    (b"expect-ct", b""),
    (b"forwarded", b""),
    (b"if-range", b""),
    (b"origin", b""),
    (b"purpose", b"prefetch"),
    (b"server", b""),
    (b"timing-allow-origin", b"*"),
    (b"upgrade-insecure-requests", b"1"),
    (b"user-agent", b""),
    (b"x-forwarded-for", b""),
    (b"x-frame-options", b"deny"),
    (b"x-frame-options", b"sameorigin"),
];

// ============================================================================
// Dynamic Table Entry
// ============================================================================

/// Dynamic table entry
#[derive(Debug, Clone)]
struct DynamicEntry {
    name: Vec<u8>,
    value: Vec<u8>,
}

impl DynamicEntry {
    fn new(name: Vec<u8>, value: Vec<u8>) -> Self {
        Self { name, value }
    }
    
    /// Entry size (name.len() + value.len() + 32 per RFC 9204)
    fn size(&self) -> usize {
        self.name.len() + self.value.len() + 32
    }
}

// ============================================================================
// Dynamic Table
// ============================================================================

/// QPACK dynamic table
#[derive(Debug)]
pub struct DynamicTable {
    /// Entries (newest first)
    entries: VecDeque<DynamicEntry>,
    /// Current size in bytes
    size: usize,
    /// Maximum capacity in bytes
    capacity: usize,
    /// Number of acknowledged inserts
    acked_count: usize,
    /// Total number of inserted entries (for absolute indexing)
    insert_count: usize,
}

impl DynamicTable {
    /// Create a new dynamic table with given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            size: 0,
            capacity,
            acked_count: 0,
            insert_count: 0,
        }
    }
    
    /// Get the current number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    
    /// Get current size
    pub fn size(&self) -> usize {
        self.size
    }
    
    /// Get capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    
    /// Get insert count
    pub fn insert_count(&self) -> usize {
        self.insert_count
    }
    
    /// Set capacity (may evict entries)
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        self.evict();
    }
    
    /// Insert a new entry
    pub fn insert(&mut self, name: Vec<u8>, value: Vec<u8>) -> Result<usize> {
        let entry = DynamicEntry::new(name, value);
        let entry_size = entry.size();
        
        // Entry too large for table
        if entry_size > self.capacity {
            return Err(Error::QpackError(QpackError::TableCapacityExceeded));
        }
        
        // Evict entries to make room
        while self.size + entry_size > self.capacity {
            if let Some(evicted) = self.entries.pop_back() {
                self.size -= evicted.size();
            } else {
                break;
            }
        }
        
        self.size += entry_size;
        self.entries.push_front(entry);
        self.insert_count += 1;
        
        Ok(self.insert_count - 1)
    }
    
    /// Get entry by absolute index
    pub fn get(&self, absolute_index: usize) -> Option<(&[u8], &[u8])> {
        if absolute_index >= self.insert_count {
            return None;
        }
        
        let relative_index = self.insert_count - 1 - absolute_index;
        self.entries.get(relative_index).map(|e| (e.name.as_slice(), e.value.as_slice()))
    }
    
    /// Acknowledge inserts up to the given count
    pub fn acknowledge(&mut self, insert_count: usize) {
        self.acked_count = self.acked_count.max(insert_count);
    }
    
    /// Evict entries to fit within capacity
    fn evict(&mut self) {
        while self.size > self.capacity {
            if let Some(evicted) = self.entries.pop_back() {
                self.size -= evicted.size();
            } else {
                break;
            }
        }
    }
}

// ============================================================================
// QPACK Encoder
// ============================================================================

/// QPACK encoder
#[derive(Debug)]
pub struct QpackEncoder {
    /// Dynamic table
    dynamic_table: DynamicTable,
    /// Maximum blocked streams
    max_blocked_streams: usize,
    /// Current blocked streams count
    blocked_streams: usize,
}

impl QpackEncoder {
    /// Create a new encoder
    pub fn new(max_table_capacity: usize, max_blocked_streams: usize) -> Self {
        Self {
            dynamic_table: DynamicTable::new(max_table_capacity),
            max_blocked_streams,
            blocked_streams: 0,
        }
    }
    
    /// Set dynamic table capacity
    pub fn set_capacity(&mut self, capacity: usize) {
        self.dynamic_table.set_capacity(capacity);
    }
    
    /// Get insert count
    pub fn insert_count(&self) -> usize {
        self.dynamic_table.insert_count()
    }
    
    /// Encode a header field
    pub fn encode_field(&self, field: &HeaderField, buf: &mut Vec<u8>) -> Result<()> {
        // Try to find in static table
        if let Some((index, has_value)) = self.find_in_static_table(&field.name, &field.value) {
            if has_value && !field.never_index {
                // Indexed field line (static)
                self.encode_indexed(buf, index, true);
                return Ok(());
            } else if !field.never_index {
                // Literal with name reference (static)
                self.encode_literal_with_name_ref(buf, index, true, &field.value, false);
                return Ok(());
            }
        }
        
        // Literal with literal name
        self.encode_literal(buf, &field.name, &field.value, field.never_index);
        Ok(())
    }
    
    /// Encode a list of headers
    pub fn encode(&self, headers: &[HeaderField], buf: &mut Vec<u8>) -> Result<()> {
        // Encode required insert count (0 for no dynamic table usage)
        buf.push(0);
        // Encode base (delta = 0)
        buf.push(0);
        
        // Encode each header field
        for field in headers {
            self.encode_field(field, buf)?;
        }
        
        Ok(())
    }
    
    /// Find header in static table
    fn find_in_static_table(&self, name: &[u8], value: &[u8]) -> Option<(usize, bool)> {
        let mut name_match = None;
        
        for (i, (n, v)) in STATIC_TABLE.iter().enumerate() {
            if n.eq_ignore_ascii_case(name) {
                if v == &value {
                    return Some((i, true)); // Full match
                }
                if name_match.is_none() {
                    name_match = Some(i);
                }
            }
        }
        
        name_match.map(|i| (i, false))
    }
    
    /// Encode indexed field line
    fn encode_indexed(&self, buf: &mut Vec<u8>, index: usize, static_table: bool) {
        if static_table {
            // Static table: 1xxxxxx
            self.encode_prefixed_integer(buf, index, 6, 0xC0);
        } else {
            // Dynamic table: 1xxxxxxx (post-base)
            self.encode_prefixed_integer(buf, index, 4, 0x10);
        }
    }
    
    /// Encode literal with name reference
    fn encode_literal_with_name_ref(
        &self,
        buf: &mut Vec<u8>,
        name_index: usize,
        static_table: bool,
        value: &[u8],
        never_index: bool,
    ) {
        let prefix = if never_index { 0x50 } else { 0x40 };
        let t_bit = if static_table { 0x10 } else { 0x00 };
        
        // Name reference
        self.encode_prefixed_integer(buf, name_index, 4, prefix | t_bit);
        
        // Value (without Huffman encoding for simplicity)
        self.encode_prefixed_integer(buf, value.len(), 7, 0x00);
        buf.extend_from_slice(value);
    }
    
    /// Encode literal field line
    fn encode_literal(&self, buf: &mut Vec<u8>, name: &[u8], value: &[u8], never_index: bool) {
        let prefix = if never_index { 0x30 } else { 0x20 };
        
        // Literal name
        buf.push(prefix);
        self.encode_prefixed_integer(buf, name.len(), 3, 0x00);
        buf.extend_from_slice(name);
        
        // Value
        self.encode_prefixed_integer(buf, value.len(), 7, 0x00);
        buf.extend_from_slice(value);
    }
    
    /// Encode a prefixed integer
    fn encode_prefixed_integer(&self, buf: &mut Vec<u8>, value: usize, prefix_bits: u8, mask: u8) {
        let max_prefix = (1usize << prefix_bits) - 1;
        
        if value < max_prefix {
            buf.push(mask | (value as u8));
        } else {
            buf.push(mask | (max_prefix as u8));
            let mut remaining = value - max_prefix;
            while remaining >= 128 {
                buf.push(((remaining % 128) as u8) | 0x80);
                remaining /= 128;
            }
            buf.push(remaining as u8);
        }
    }
    
    /// Process acknowledgment from decoder
    pub fn process_ack(&mut self, _stream_id: i64, insert_count: usize) {
        self.dynamic_table.acknowledge(insert_count);
        if self.blocked_streams > 0 {
            self.blocked_streams -= 1;
        }
    }
}

impl Default for QpackEncoder {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_TABLE_CAPACITY, DEFAULT_MAX_BLOCKED_STREAMS)
    }
}

// ============================================================================
// QPACK Decoder
// ============================================================================

/// QPACK decoder
#[derive(Debug)]
pub struct QpackDecoder {
    /// Dynamic table
    dynamic_table: DynamicTable,
    /// Maximum blocked streams
    max_blocked_streams: usize,
}

impl QpackDecoder {
    /// Create a new decoder
    pub fn new(max_table_capacity: usize, max_blocked_streams: usize) -> Self {
        Self {
            dynamic_table: DynamicTable::new(max_table_capacity),
            max_blocked_streams,
        }
    }
    
    /// Set dynamic table capacity
    pub fn set_capacity(&mut self, capacity: usize) {
        self.dynamic_table.set_capacity(capacity);
    }
    
    /// Decode a header block
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<HeaderField>> {
        let mut headers = Vec::new();
        let mut pos = 0;
        
        if data.is_empty() {
            return Ok(headers);
        }
        
        // Decode required insert count
        let (ric, consumed) = self.decode_prefixed_integer(&data[pos..], 8)?;
        pos += consumed;
        
        if pos >= data.len() {
            return Ok(headers);
        }
        
        // Decode base (delta)
        let sign = (data[pos] & 0x80) != 0;
        let (delta, consumed) = self.decode_prefixed_integer(&data[pos..], 7)?;
        pos += consumed;
        
        let _base = if sign {
            ric.saturating_sub(delta + 1)
        } else {
            ric + delta
        };
        
        // Decode header fields
        while pos < data.len() {
            let (field, consumed) = self.decode_field(&data[pos..])?;
            headers.push(field);
            pos += consumed;
        }
        
        Ok(headers)
    }
    
    /// Decode a single header field
    fn decode_field(&self, data: &[u8]) -> Result<(HeaderField, usize)> {
        if data.is_empty() {
            return Err(ErrorCode::QpackFatal.into());
        }
        
        let first = data[0];
        
        if (first & 0x80) != 0 {
            // Indexed field line
            if (first & 0x40) != 0 {
                // Static table
                self.decode_indexed_static(data)
            } else {
                // Dynamic table (not implemented for now)
                Err(ErrorCode::QpackFatal.into())
            }
        } else if (first & 0x40) != 0 {
            // Literal with name reference
            self.decode_literal_with_name_ref(data)
        } else if (first & 0x20) != 0 {
            // Literal with literal name
            self.decode_literal(data)
        } else {
            // Indexed field line with post-base index (dynamic)
            Err(ErrorCode::QpackFatal.into())
        }
    }
    
    /// Decode indexed field from static table
    fn decode_indexed_static(&self, data: &[u8]) -> Result<(HeaderField, usize)> {
        let (index, consumed) = self.decode_prefixed_integer(data, 6)?;
        
        if index >= STATIC_TABLE.len() {
            return Err(Error::QpackError(QpackError::InvalidStaticIndex));
        }
        
        let (name, value) = STATIC_TABLE[index];
        Ok((HeaderField::new(name.to_vec(), value.to_vec()), consumed))
    }
    
    /// Decode literal with name reference
    fn decode_literal_with_name_ref(&self, data: &[u8]) -> Result<(HeaderField, usize)> {
        let mut pos = 0;
        
        let never_index = (data[0] & 0x20) != 0;
        let static_table = (data[0] & 0x10) != 0;
        
        let (name_index, consumed) = self.decode_prefixed_integer(data, 4)?;
        pos += consumed;
        
        let name = if static_table {
            if name_index >= STATIC_TABLE.len() {
                return Err(Error::QpackError(QpackError::InvalidStaticIndex));
            }
            STATIC_TABLE[name_index].0.to_vec()
        } else {
            return Err(ErrorCode::QpackFatal.into()); // Dynamic not implemented
        };
        
        let (value, consumed) = self.decode_string(&data[pos..])?;
        pos += consumed;
        
        let mut field = HeaderField::new(name, value);
        field.never_index = never_index;
        
        Ok((field, pos))
    }
    
    /// Decode literal field
    fn decode_literal(&self, data: &[u8]) -> Result<(HeaderField, usize)> {
        let mut pos = 0;
        
        let never_index = (data[0] & 0x10) != 0;
        
        let (name, consumed) = self.decode_string_with_prefix(data, 3)?;
        pos += consumed;
        
        let (value, consumed) = self.decode_string(&data[pos..])?;
        pos += consumed;
        
        let mut field = HeaderField::new(name, value);
        field.never_index = never_index;
        
        Ok((field, pos))
    }
    
    /// Decode a prefixed integer
    fn decode_prefixed_integer(&self, data: &[u8], prefix_bits: u8) -> Result<(usize, usize)> {
        if data.is_empty() {
            return Err(ErrorCode::NoBuf.into());
        }
        
        let max_prefix = (1usize << prefix_bits) - 1;
        let mask = max_prefix as u8;
        
        let mut value = (data[0] & mask) as usize;
        let mut pos = 1;
        
        if value < max_prefix {
            return Ok((value, pos));
        }
        
        let mut shift = 0usize;
        loop {
            if pos >= data.len() {
                return Err(ErrorCode::NoBuf.into());
            }
            
            let b = data[pos] as usize;
            pos += 1;
            
            value += (b & 0x7F) << shift;
            shift += 7;
            
            if (b & 0x80) == 0 {
                break;
            }
            
            if shift > 63 {
                return Err(ErrorCode::QpackFatal.into());
            }
        }
        
        Ok((value, pos))
    }
    
    /// Decode a string with 7-bit prefix
    fn decode_string(&self, data: &[u8]) -> Result<(Vec<u8>, usize)> {
        self.decode_string_with_prefix(data, 7)
    }
    
    /// Decode a string with given prefix bits
    fn decode_string_with_prefix(&self, data: &[u8], prefix_bits: u8) -> Result<(Vec<u8>, usize)> {
        if data.is_empty() {
            return Err(ErrorCode::NoBuf.into());
        }
        
        let huffman = (data[0] & (1 << prefix_bits)) != 0;
        let (length, consumed) = self.decode_prefixed_integer(data, prefix_bits)?;
        
        if consumed + length > data.len() {
            return Err(ErrorCode::NoBuf.into());
        }
        
        let string_data = &data[consumed..consumed + length];
        
        let result = if huffman {
            // Huffman decoding not implemented - return as-is for now
            string_data.to_vec()
        } else {
            string_data.to_vec()
        };
        
        Ok((result, consumed + length))
    }
    
    /// Process encoder instruction
    pub fn process_encoder_instruction(&mut self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        
        let first = data[0];
        
        if (first & 0x20) != 0 {
            // Set dynamic table capacity
            let (capacity, _) = self.decode_prefixed_integer(data, 5)?;
            self.dynamic_table.set_capacity(capacity);
        }
        // Other instructions not implemented yet
        
        Ok(())
    }
    
    /// Generate section acknowledgment
    pub fn generate_section_ack(&self, stream_id: i64, buf: &mut Vec<u8>) {
        // Section Acknowledgment: 1xxxxxxx
        buf.push(0x80);
        self.encode_prefixed_integer(buf, stream_id as usize, 7, 0x00);
    }
    
    /// Encode a prefixed integer (for generating instructions)
    fn encode_prefixed_integer(&self, buf: &mut Vec<u8>, value: usize, prefix_bits: u8, mask: u8) {
        let max_prefix = (1usize << prefix_bits) - 1;
        
        if value < max_prefix {
            if buf.is_empty() {
                buf.push(mask | (value as u8));
            } else {
                let last = buf.len() - 1;
                buf[last] |= value as u8;
            }
        } else {
            if buf.is_empty() {
                buf.push(mask | (max_prefix as u8));
            } else {
                let last = buf.len() - 1;
                buf[last] |= max_prefix as u8;
            }
            let mut remaining = value - max_prefix;
            while remaining >= 128 {
                buf.push(((remaining % 128) as u8) | 0x80);
                remaining /= 128;
            }
            buf.push(remaining as u8);
        }
    }
}

impl Default for QpackDecoder {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_TABLE_CAPACITY, DEFAULT_MAX_BLOCKED_STREAMS)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_static_table() {
        assert_eq!(STATIC_TABLE[0], (b":authority".as_slice(), b"".as_slice()));
        assert_eq!(STATIC_TABLE[17], (b":method".as_slice(), b"GET".as_slice()));
        assert_eq!(STATIC_TABLE[25], (b":status".as_slice(), b"200".as_slice()));
    }
    
    #[test]
    fn test_encoder_basic() {
        let encoder = QpackEncoder::default();
        let mut buf = Vec::new();
        
        let headers = vec![
            HeaderField::new(b":method".to_vec(), b"GET".to_vec()),
            HeaderField::new(b":path".to_vec(), b"/".to_vec()),
            HeaderField::new(b":scheme".to_vec(), b"https".to_vec()),
        ];
        
        encoder.encode(&headers, &mut buf).unwrap();
        assert!(!buf.is_empty());
    }
    
    #[test]
    fn test_dynamic_table() {
        let mut table = DynamicTable::new(4096);
        
        // Insert entry
        let idx = table.insert(b"custom-header".to_vec(), b"value".to_vec()).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(table.len(), 1);
        
        // Get entry
        let (name, value) = table.get(0).unwrap();
        assert_eq!(name, b"custom-header");
        assert_eq!(value, b"value");
    }
}
