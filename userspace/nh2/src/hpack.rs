//! HPACK Header Compression (RFC 7541)
//!
//! This module implements the HPACK header compression algorithm used in HTTP/2.

use crate::constants::hpack::*;
use crate::error::{Error, HpackError, Result};
use std::collections::VecDeque;

// ============================================================================
// Static Table (RFC 7541 Appendix A)
// ============================================================================

/// Static table entries (index 1-61)
const STATIC_TABLE: [(&[u8], &[u8]); 61] = [
    (b":authority", b""),
    (b":method", b"GET"),
    (b":method", b"POST"),
    (b":path", b"/"),
    (b":path", b"/index.html"),
    (b":scheme", b"http"),
    (b":scheme", b"https"),
    (b":status", b"200"),
    (b":status", b"204"),
    (b":status", b"206"),
    (b":status", b"304"),
    (b":status", b"400"),
    (b":status", b"404"),
    (b":status", b"500"),
    (b"accept-charset", b""),
    (b"accept-encoding", b"gzip, deflate"),
    (b"accept-language", b""),
    (b"accept-ranges", b""),
    (b"accept", b""),
    (b"access-control-allow-origin", b""),
    (b"age", b""),
    (b"allow", b""),
    (b"authorization", b""),
    (b"cache-control", b""),
    (b"content-disposition", b""),
    (b"content-encoding", b""),
    (b"content-language", b""),
    (b"content-length", b""),
    (b"content-location", b""),
    (b"content-range", b""),
    (b"content-type", b""),
    (b"cookie", b""),
    (b"date", b""),
    (b"etag", b""),
    (b"expect", b""),
    (b"expires", b""),
    (b"from", b""),
    (b"host", b""),
    (b"if-match", b""),
    (b"if-modified-since", b""),
    (b"if-none-match", b""),
    (b"if-range", b""),
    (b"if-unmodified-since", b""),
    (b"last-modified", b""),
    (b"link", b""),
    (b"location", b""),
    (b"max-forwards", b""),
    (b"proxy-authenticate", b""),
    (b"proxy-authorization", b""),
    (b"range", b""),
    (b"referer", b""),
    (b"refresh", b""),
    (b"retry-after", b""),
    (b"server", b""),
    (b"set-cookie", b""),
    (b"strict-transport-security", b""),
    (b"transfer-encoding", b""),
    (b"user-agent", b""),
    (b"vary", b""),
    (b"via", b""),
    (b"www-authenticate", b""),
];

// ============================================================================
// Header Field
// ============================================================================

/// A single header field (name-value pair)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderField {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
    pub sensitive: bool,
}

impl HeaderField {
    /// Create a new header field
    pub fn new(name: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            sensitive: false,
        }
    }

    /// Create a sensitive header field (never indexed)
    pub fn sensitive(name: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            sensitive: true,
        }
    }

    /// Get the size of this header field as per RFC 7541
    pub fn size(&self) -> usize {
        self.name.len() + self.value.len() + ENTRY_OVERHEAD
    }
}

// ============================================================================
// Dynamic Table
// ============================================================================

/// HPACK dynamic table
#[derive(Debug)]
pub struct DynamicTable {
    entries: VecDeque<HeaderField>,
    size: usize,
    max_size: usize,
}

impl DynamicTable {
    /// Create a new dynamic table
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            size: 0,
            max_size,
        }
    }

    /// Get the current size
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get the maximum size
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Set the maximum size (may evict entries)
    pub fn set_max_size(&mut self, max_size: usize) {
        self.max_size = max_size;
        self.evict();
    }

    /// Get an entry by index (0-based within dynamic table)
    pub fn get(&self, index: usize) -> Option<&HeaderField> {
        self.entries.get(index)
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the table is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert a new entry at the front
    pub fn insert(&mut self, field: HeaderField) {
        let field_size = field.size();

        // If the entry is larger than the table, clear everything
        if field_size > self.max_size {
            self.entries.clear();
            self.size = 0;
            return;
        }

        // Evict entries until there's room
        while self.size + field_size > self.max_size {
            if let Some(removed) = self.entries.pop_back() {
                self.size -= removed.size();
            } else {
                break;
            }
        }

        self.entries.push_front(field);
        self.size += field_size;
    }

    /// Evict entries to fit within max_size
    fn evict(&mut self) {
        while self.size > self.max_size {
            if let Some(removed) = self.entries.pop_back() {
                self.size -= removed.size();
            } else {
                break;
            }
        }
    }

    /// Find an entry by name and value
    pub fn find(&self, name: &[u8], value: &[u8]) -> Option<usize> {
        self.entries
            .iter()
            .position(|f| f.name == name && f.value == value)
    }

    /// Find an entry by name only
    pub fn find_name(&self, name: &[u8]) -> Option<usize> {
        self.entries.iter().position(|f| f.name == name)
    }
}

// ============================================================================
// HPACK Encoder
// ============================================================================

/// HPACK encoder
pub struct HpackEncoder {
    dynamic_table: DynamicTable,
    huffman: bool,
}

impl HpackEncoder {
    /// Create a new encoder
    pub fn new(max_table_size: usize) -> Self {
        Self {
            dynamic_table: DynamicTable::new(max_table_size),
            huffman: true,
        }
    }

    /// Set whether to use Huffman encoding
    pub fn set_huffman(&mut self, enabled: bool) {
        self.huffman = enabled;
    }

    /// Set the maximum dynamic table size
    pub fn set_max_table_size(&mut self, size: usize) {
        self.dynamic_table.set_max_size(size);
    }

    /// Encode a list of headers
    pub fn encode(&mut self, headers: &[HeaderField], buf: &mut Vec<u8>) -> Result<()> {
        for field in headers {
            self.encode_field(field, buf)?;
        }
        Ok(())
    }

    /// Encode a single header field
    fn encode_field(&mut self, field: &HeaderField, buf: &mut Vec<u8>) -> Result<()> {
        // Try to find in static table first
        if let Some((index, name_match)) = self.find_in_static(&field.name, &field.value) {
            if name_match {
                // Full match in static table - indexed header field
                self.encode_indexed(index, buf);
                return Ok(());
            }
        }

        // Try dynamic table
        if let Some(index) = self.dynamic_table.find(&field.name, &field.value) {
            // Full match in dynamic table
            let full_index = STATIC_TABLE_SIZE + 1 + index;
            self.encode_indexed(full_index, buf);
            return Ok(());
        }

        // Try name match in static table
        if let Some((index, _)) = self.find_in_static(&field.name, &[]) {
            if field.sensitive {
                // Never indexed
                self.encode_literal_never_indexed_with_index(index, &field.value, buf)?;
            } else {
                // Incremental indexing
                self.encode_literal_with_index(index, &field.value, buf)?;
                self.dynamic_table.insert(field.clone());
            }
            return Ok(());
        }

        // Try name match in dynamic table
        if let Some(index) = self.dynamic_table.find_name(&field.name) {
            let full_index = STATIC_TABLE_SIZE + 1 + index;
            if field.sensitive {
                self.encode_literal_never_indexed_with_index(full_index, &field.value, buf)?;
            } else {
                self.encode_literal_with_index(full_index, &field.value, buf)?;
                self.dynamic_table.insert(field.clone());
            }
            return Ok(());
        }

        // No match - encode with new name
        if field.sensitive {
            self.encode_literal_never_indexed_new_name(&field.name, &field.value, buf)?;
        } else {
            self.encode_literal_new_name(&field.name, &field.value, buf)?;
            self.dynamic_table.insert(field.clone());
        }

        Ok(())
    }

    fn find_in_static(&self, name: &[u8], value: &[u8]) -> Option<(usize, bool)> {
        let mut name_index = None;

        for (i, (n, v)) in STATIC_TABLE.iter().enumerate() {
            if *n == name {
                if *v == value && !value.is_empty() {
                    return Some((i + 1, true)); // Full match
                }
                if name_index.is_none() {
                    name_index = Some(i + 1);
                }
            }
        }

        name_index.map(|i| (i, false))
    }

    /// Encode an indexed header field
    fn encode_indexed(&self, index: usize, buf: &mut Vec<u8>) {
        // 1xxxxxxx
        self.encode_integer(index, 7, 0x80, buf);
    }

    /// Encode literal with incremental indexing (existing name)
    fn encode_literal_with_index(
        &self,
        index: usize,
        value: &[u8],
        buf: &mut Vec<u8>,
    ) -> Result<()> {
        // 01xxxxxx
        self.encode_integer(index, 6, 0x40, buf);
        self.encode_string(value, buf)?;
        Ok(())
    }

    /// Encode literal with incremental indexing (new name)
    fn encode_literal_new_name(&self, name: &[u8], value: &[u8], buf: &mut Vec<u8>) -> Result<()> {
        // 01000000
        buf.push(0x40);
        self.encode_string(name, buf)?;
        self.encode_string(value, buf)?;
        Ok(())
    }

    /// Encode literal without indexing (existing name)
    fn encode_literal_without_index(
        &self,
        index: usize,
        value: &[u8],
        buf: &mut Vec<u8>,
    ) -> Result<()> {
        // 0000xxxx
        self.encode_integer(index, 4, 0x00, buf);
        self.encode_string(value, buf)?;
        Ok(())
    }

    /// Encode literal never indexed (existing name)
    fn encode_literal_never_indexed_with_index(
        &self,
        index: usize,
        value: &[u8],
        buf: &mut Vec<u8>,
    ) -> Result<()> {
        // 0001xxxx
        self.encode_integer(index, 4, 0x10, buf);
        self.encode_string(value, buf)?;
        Ok(())
    }

    /// Encode literal never indexed (new name)
    fn encode_literal_never_indexed_new_name(
        &self,
        name: &[u8],
        value: &[u8],
        buf: &mut Vec<u8>,
    ) -> Result<()> {
        // 00010000
        buf.push(0x10);
        self.encode_string(name, buf)?;
        self.encode_string(value, buf)?;
        Ok(())
    }

    /// Encode an integer with prefix
    fn encode_integer(&self, value: usize, prefix_bits: u8, prefix: u8, buf: &mut Vec<u8>) {
        let max_prefix = (1 << prefix_bits) - 1;

        if value < max_prefix {
            buf.push(prefix | (value as u8));
        } else {
            buf.push(prefix | max_prefix as u8);
            let mut remaining = value - max_prefix;
            while remaining >= 128 {
                buf.push((remaining % 128 + 128) as u8);
                remaining /= 128;
            }
            buf.push(remaining as u8);
        }
    }

    /// Encode a string (with optional Huffman encoding)
    fn encode_string(&self, s: &[u8], buf: &mut Vec<u8>) -> Result<()> {
        if self.huffman {
            let encoded = huffman_encode(s);
            if encoded.len() < s.len() {
                // Use Huffman
                self.encode_integer(encoded.len(), 7, 0x80, buf);
                buf.extend_from_slice(&encoded);
                return Ok(());
            }
        }

        // Use literal
        self.encode_integer(s.len(), 7, 0x00, buf);
        buf.extend_from_slice(s);
        Ok(())
    }
}

// ============================================================================
// HPACK Decoder
// ============================================================================

/// HPACK decoder
pub struct HpackDecoder {
    dynamic_table: DynamicTable,
    max_table_size: usize,
}

impl HpackDecoder {
    /// Create a new decoder
    pub fn new(max_table_size: usize) -> Self {
        Self {
            dynamic_table: DynamicTable::new(max_table_size),
            max_table_size,
        }
    }

    /// Set the maximum dynamic table size
    pub fn set_max_table_size(&mut self, size: usize) {
        self.max_table_size = size;
        self.dynamic_table.set_max_size(size);
    }

    /// Decode a header block
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<HeaderField>> {
        let mut headers = Vec::new();
        let mut pos = 0;

        while pos < data.len() {
            let b = data[pos];

            if b & 0x80 != 0 {
                // Indexed header field
                let (index, consumed) = self.decode_integer(&data[pos..], 7)?;
                pos += consumed;
                let field = self.get_indexed(index)?;
                headers.push(field);
            } else if b & 0x40 != 0 {
                // Literal with incremental indexing
                let (index, consumed) = self.decode_integer(&data[pos..], 6)?;
                pos += consumed;

                let field = if index == 0 {
                    let (name, nc) = self.decode_string(&data[pos..])?;
                    pos += nc;
                    let (value, vc) = self.decode_string(&data[pos..])?;
                    pos += vc;
                    HeaderField::new(name, value)
                } else {
                    let (name, _) = self.get_indexed_name(index)?;
                    let (value, vc) = self.decode_string(&data[pos..])?;
                    pos += vc;
                    HeaderField::new(name, value)
                };

                self.dynamic_table.insert(field.clone());
                headers.push(field);
            } else if b & 0x20 != 0 {
                // Dynamic table size update
                let (size, consumed) = self.decode_integer(&data[pos..], 5)?;
                pos += consumed;
                if size > self.max_table_size {
                    return Err(Error::Hpack(HpackError::TableSizeExceeded));
                }
                self.dynamic_table.set_max_size(size);
            } else {
                // Literal without indexing or never indexed
                let never_index = (b & 0x10) != 0;
                let (index, consumed) = self.decode_integer(&data[pos..], 4)?;
                pos += consumed;

                let field = if index == 0 {
                    let (name, nc) = self.decode_string(&data[pos..])?;
                    pos += nc;
                    let (value, vc) = self.decode_string(&data[pos..])?;
                    pos += vc;
                    if never_index {
                        HeaderField::sensitive(name, value)
                    } else {
                        HeaderField::new(name, value)
                    }
                } else {
                    let (name, _) = self.get_indexed_name(index)?;
                    let (value, vc) = self.decode_string(&data[pos..])?;
                    pos += vc;
                    if never_index {
                        HeaderField::sensitive(name, value)
                    } else {
                        HeaderField::new(name, value)
                    }
                };

                headers.push(field);
            }
        }

        Ok(headers)
    }

    /// Get an indexed header field
    fn get_indexed(&self, index: usize) -> Result<HeaderField> {
        if index == 0 {
            return Err(Error::Hpack(HpackError::InvalidIndex));
        }

        if index <= STATIC_TABLE_SIZE {
            let (name, value) = STATIC_TABLE[index - 1];
            return Ok(HeaderField::new(name.to_vec(), value.to_vec()));
        }

        let dyn_index = index - STATIC_TABLE_SIZE - 1;
        self.dynamic_table
            .get(dyn_index)
            .cloned()
            .ok_or(Error::Hpack(HpackError::InvalidIndex))
    }

    /// Get an indexed header name
    fn get_indexed_name(&self, index: usize) -> Result<(Vec<u8>, Vec<u8>)> {
        if index == 0 {
            return Err(Error::Hpack(HpackError::InvalidIndex));
        }

        if index <= STATIC_TABLE_SIZE {
            let (name, value) = STATIC_TABLE[index - 1];
            return Ok((name.to_vec(), value.to_vec()));
        }

        let dyn_index = index - STATIC_TABLE_SIZE - 1;
        self.dynamic_table
            .get(dyn_index)
            .map(|f| (f.name.clone(), f.value.clone()))
            .ok_or(Error::Hpack(HpackError::InvalidIndex))
    }

    /// Decode an integer
    fn decode_integer(&self, data: &[u8], prefix_bits: u8) -> Result<(usize, usize)> {
        if data.is_empty() {
            return Err(Error::Hpack(HpackError::InvalidEncoding));
        }

        let max_prefix = (1 << prefix_bits) - 1;
        let mut value = (data[0] & max_prefix) as usize;

        if value < max_prefix as usize {
            return Ok((value, 1));
        }

        let mut m = 0;
        let mut i = 1;

        loop {
            if i >= data.len() {
                return Err(Error::Hpack(HpackError::InvalidEncoding));
            }

            let b = data[i];
            value += ((b & 127) as usize) << m;
            m += 7;
            i += 1;

            if m > 63 {
                return Err(Error::Hpack(HpackError::IntegerOverflow));
            }

            if (b & 128) == 0 {
                break;
            }
        }

        Ok((value, i))
    }

    /// Decode a string
    fn decode_string(&self, data: &[u8]) -> Result<(Vec<u8>, usize)> {
        if data.is_empty() {
            return Err(Error::Hpack(HpackError::InvalidEncoding));
        }

        let huffman = (data[0] & 0x80) != 0;
        let (length, consumed) = self.decode_integer(data, 7)?;

        if data.len() < consumed + length {
            return Err(Error::Hpack(HpackError::InvalidEncoding));
        }

        let string_data = &data[consumed..consumed + length];
        let result = if huffman {
            huffman_decode(string_data)?
        } else {
            string_data.to_vec()
        };

        Ok((result, consumed + length))
    }
}

// ============================================================================
// Huffman Coding (RFC 7541 Appendix B)
// ============================================================================

/// Huffman code table entry
struct HuffmanCode {
    code: u32,
    bits: u8,
}

/// Huffman encoding table (RFC 7541 Appendix B)
static HUFFMAN_TABLE: [HuffmanCode; 257] = [
    HuffmanCode {
        code: 0x1ff8,
        bits: 13,
    }, // 0
    HuffmanCode {
        code: 0x7fffd8,
        bits: 23,
    }, // 1
    HuffmanCode {
        code: 0xfffffe2,
        bits: 28,
    }, // 2
    HuffmanCode {
        code: 0xfffffe3,
        bits: 28,
    }, // 3
    HuffmanCode {
        code: 0xfffffe4,
        bits: 28,
    }, // 4
    HuffmanCode {
        code: 0xfffffe5,
        bits: 28,
    }, // 5
    HuffmanCode {
        code: 0xfffffe6,
        bits: 28,
    }, // 6
    HuffmanCode {
        code: 0xfffffe7,
        bits: 28,
    }, // 7
    HuffmanCode {
        code: 0xfffffe8,
        bits: 28,
    }, // 8
    HuffmanCode {
        code: 0xffffea,
        bits: 24,
    }, // 9
    HuffmanCode {
        code: 0x3ffffffc,
        bits: 30,
    }, // 10
    HuffmanCode {
        code: 0xfffffe9,
        bits: 28,
    }, // 11
    HuffmanCode {
        code: 0xfffffea,
        bits: 28,
    }, // 12
    HuffmanCode {
        code: 0x3ffffffd,
        bits: 30,
    }, // 13
    HuffmanCode {
        code: 0xfffffeb,
        bits: 28,
    }, // 14
    HuffmanCode {
        code: 0xfffffec,
        bits: 28,
    }, // 15
    HuffmanCode {
        code: 0xfffffed,
        bits: 28,
    }, // 16
    HuffmanCode {
        code: 0xfffffee,
        bits: 28,
    }, // 17
    HuffmanCode {
        code: 0xfffffef,
        bits: 28,
    }, // 18
    HuffmanCode {
        code: 0xffffff0,
        bits: 28,
    }, // 19
    HuffmanCode {
        code: 0xffffff1,
        bits: 28,
    }, // 20
    HuffmanCode {
        code: 0xffffff2,
        bits: 28,
    }, // 21
    HuffmanCode {
        code: 0x3ffffffe,
        bits: 30,
    }, // 22
    HuffmanCode {
        code: 0xffffff3,
        bits: 28,
    }, // 23
    HuffmanCode {
        code: 0xffffff4,
        bits: 28,
    }, // 24
    HuffmanCode {
        code: 0xffffff5,
        bits: 28,
    }, // 25
    HuffmanCode {
        code: 0xffffff6,
        bits: 28,
    }, // 26
    HuffmanCode {
        code: 0xffffff7,
        bits: 28,
    }, // 27
    HuffmanCode {
        code: 0xffffff8,
        bits: 28,
    }, // 28
    HuffmanCode {
        code: 0xffffff9,
        bits: 28,
    }, // 29
    HuffmanCode {
        code: 0xffffffa,
        bits: 28,
    }, // 30
    HuffmanCode {
        code: 0xffffffb,
        bits: 28,
    }, // 31
    HuffmanCode {
        code: 0x14,
        bits: 6,
    }, // ' ' (32)
    HuffmanCode {
        code: 0x3f8,
        bits: 10,
    }, // '!' (33)
    HuffmanCode {
        code: 0x3f9,
        bits: 10,
    }, // '"' (34)
    HuffmanCode {
        code: 0xffa,
        bits: 12,
    }, // '#' (35)
    HuffmanCode {
        code: 0x1ff9,
        bits: 13,
    }, // '$' (36)
    HuffmanCode {
        code: 0x15,
        bits: 6,
    }, // '%' (37)
    HuffmanCode {
        code: 0xf8,
        bits: 8,
    }, // '&' (38)
    HuffmanCode {
        code: 0x7fa,
        bits: 11,
    }, // '\'' (39)
    HuffmanCode {
        code: 0x3fa,
        bits: 10,
    }, // '(' (40)
    HuffmanCode {
        code: 0x3fb,
        bits: 10,
    }, // ')' (41)
    HuffmanCode {
        code: 0xf9,
        bits: 8,
    }, // '*' (42)
    HuffmanCode {
        code: 0x7fb,
        bits: 11,
    }, // '+' (43)
    HuffmanCode {
        code: 0xfa,
        bits: 8,
    }, // ',' (44)
    HuffmanCode {
        code: 0x16,
        bits: 6,
    }, // '-' (45)
    HuffmanCode {
        code: 0x17,
        bits: 6,
    }, // '.' (46)
    HuffmanCode {
        code: 0x18,
        bits: 6,
    }, // '/' (47)
    HuffmanCode { code: 0x0, bits: 5 }, // '0' (48)
    HuffmanCode { code: 0x1, bits: 5 }, // '1' (49)
    HuffmanCode { code: 0x2, bits: 5 }, // '2' (50)
    HuffmanCode {
        code: 0x19,
        bits: 6,
    }, // '3' (51)
    HuffmanCode {
        code: 0x1a,
        bits: 6,
    }, // '4' (52)
    HuffmanCode {
        code: 0x1b,
        bits: 6,
    }, // '5' (53)
    HuffmanCode {
        code: 0x1c,
        bits: 6,
    }, // '6' (54)
    HuffmanCode {
        code: 0x1d,
        bits: 6,
    }, // '7' (55)
    HuffmanCode {
        code: 0x1e,
        bits: 6,
    }, // '8' (56)
    HuffmanCode {
        code: 0x1f,
        bits: 6,
    }, // '9' (57)
    HuffmanCode {
        code: 0x5c,
        bits: 7,
    }, // ':' (58)
    HuffmanCode {
        code: 0xfb,
        bits: 8,
    }, // ';' (59)
    HuffmanCode {
        code: 0x7ffc,
        bits: 15,
    }, // '<' (60)
    HuffmanCode {
        code: 0x20,
        bits: 6,
    }, // '=' (61)
    HuffmanCode {
        code: 0xffb,
        bits: 12,
    }, // '>' (62)
    HuffmanCode {
        code: 0x3fc,
        bits: 10,
    }, // '?' (63)
    HuffmanCode {
        code: 0x1ffa,
        bits: 13,
    }, // '@' (64)
    HuffmanCode {
        code: 0x21,
        bits: 6,
    }, // 'A' (65)
    HuffmanCode {
        code: 0x5d,
        bits: 7,
    }, // 'B' (66)
    HuffmanCode {
        code: 0x5e,
        bits: 7,
    }, // 'C' (67)
    HuffmanCode {
        code: 0x5f,
        bits: 7,
    }, // 'D' (68)
    HuffmanCode {
        code: 0x60,
        bits: 7,
    }, // 'E' (69)
    HuffmanCode {
        code: 0x61,
        bits: 7,
    }, // 'F' (70)
    HuffmanCode {
        code: 0x62,
        bits: 7,
    }, // 'G' (71)
    HuffmanCode {
        code: 0x63,
        bits: 7,
    }, // 'H' (72)
    HuffmanCode {
        code: 0x64,
        bits: 7,
    }, // 'I' (73)
    HuffmanCode {
        code: 0x65,
        bits: 7,
    }, // 'J' (74)
    HuffmanCode {
        code: 0x66,
        bits: 7,
    }, // 'K' (75)
    HuffmanCode {
        code: 0x67,
        bits: 7,
    }, // 'L' (76)
    HuffmanCode {
        code: 0x68,
        bits: 7,
    }, // 'M' (77)
    HuffmanCode {
        code: 0x69,
        bits: 7,
    }, // 'N' (78)
    HuffmanCode {
        code: 0x6a,
        bits: 7,
    }, // 'O' (79)
    HuffmanCode {
        code: 0x6b,
        bits: 7,
    }, // 'P' (80)
    HuffmanCode {
        code: 0x6c,
        bits: 7,
    }, // 'Q' (81)
    HuffmanCode {
        code: 0x6d,
        bits: 7,
    }, // 'R' (82)
    HuffmanCode {
        code: 0x6e,
        bits: 7,
    }, // 'S' (83)
    HuffmanCode {
        code: 0x6f,
        bits: 7,
    }, // 'T' (84)
    HuffmanCode {
        code: 0x70,
        bits: 7,
    }, // 'U' (85)
    HuffmanCode {
        code: 0x71,
        bits: 7,
    }, // 'V' (86)
    HuffmanCode {
        code: 0x72,
        bits: 7,
    }, // 'W' (87)
    HuffmanCode {
        code: 0xfc,
        bits: 8,
    }, // 'X' (88)
    HuffmanCode {
        code: 0x73,
        bits: 7,
    }, // 'Y' (89)
    HuffmanCode {
        code: 0xfd,
        bits: 8,
    }, // 'Z' (90)
    HuffmanCode {
        code: 0x1ffb,
        bits: 13,
    }, // '[' (91)
    HuffmanCode {
        code: 0x7fff0,
        bits: 19,
    }, // '\\' (92)
    HuffmanCode {
        code: 0x1ffc,
        bits: 13,
    }, // ']' (93)
    HuffmanCode {
        code: 0x3ffc,
        bits: 14,
    }, // '^' (94)
    HuffmanCode {
        code: 0x22,
        bits: 6,
    }, // '_' (95)
    HuffmanCode {
        code: 0x7ffd,
        bits: 15,
    }, // '`' (96)
    HuffmanCode { code: 0x3, bits: 5 }, // 'a' (97)
    HuffmanCode {
        code: 0x23,
        bits: 6,
    }, // 'b' (98)
    HuffmanCode { code: 0x4, bits: 5 }, // 'c' (99)
    HuffmanCode {
        code: 0x24,
        bits: 6,
    }, // 'd' (100)
    HuffmanCode { code: 0x5, bits: 5 }, // 'e' (101)
    HuffmanCode {
        code: 0x25,
        bits: 6,
    }, // 'f' (102)
    HuffmanCode {
        code: 0x26,
        bits: 6,
    }, // 'g' (103)
    HuffmanCode {
        code: 0x27,
        bits: 6,
    }, // 'h' (104)
    HuffmanCode { code: 0x6, bits: 5 }, // 'i' (105)
    HuffmanCode {
        code: 0x74,
        bits: 7,
    }, // 'j' (106)
    HuffmanCode {
        code: 0x75,
        bits: 7,
    }, // 'k' (107)
    HuffmanCode {
        code: 0x28,
        bits: 6,
    }, // 'l' (108)
    HuffmanCode {
        code: 0x29,
        bits: 6,
    }, // 'm' (109)
    HuffmanCode {
        code: 0x2a,
        bits: 6,
    }, // 'n' (110)
    HuffmanCode { code: 0x7, bits: 5 }, // 'o' (111)
    HuffmanCode {
        code: 0x2b,
        bits: 6,
    }, // 'p' (112)
    HuffmanCode {
        code: 0x76,
        bits: 7,
    }, // 'q' (113)
    HuffmanCode {
        code: 0x2c,
        bits: 6,
    }, // 'r' (114)
    HuffmanCode { code: 0x8, bits: 5 }, // 's' (115)
    HuffmanCode { code: 0x9, bits: 5 }, // 't' (116)
    HuffmanCode {
        code: 0x2d,
        bits: 6,
    }, // 'u' (117)
    HuffmanCode {
        code: 0x77,
        bits: 7,
    }, // 'v' (118)
    HuffmanCode {
        code: 0x78,
        bits: 7,
    }, // 'w' (119)
    HuffmanCode {
        code: 0x79,
        bits: 7,
    }, // 'x' (120)
    HuffmanCode {
        code: 0x7a,
        bits: 7,
    }, // 'y' (121)
    HuffmanCode {
        code: 0x7b,
        bits: 7,
    }, // 'z' (122)
    HuffmanCode {
        code: 0x7ffe,
        bits: 15,
    }, // '{' (123)
    HuffmanCode {
        code: 0x7fc,
        bits: 11,
    }, // '|' (124)
    HuffmanCode {
        code: 0x3ffd,
        bits: 14,
    }, // '}' (125)
    HuffmanCode {
        code: 0x1ffd,
        bits: 13,
    }, // '~' (126)
    HuffmanCode {
        code: 0xffffffc,
        bits: 28,
    }, // 127
    HuffmanCode {
        code: 0xfffe6,
        bits: 20,
    }, // 128
    HuffmanCode {
        code: 0x3fffd2,
        bits: 22,
    }, // 129
    HuffmanCode {
        code: 0xfffe7,
        bits: 20,
    }, // 130
    HuffmanCode {
        code: 0xfffe8,
        bits: 20,
    }, // 131
    HuffmanCode {
        code: 0x3fffd3,
        bits: 22,
    }, // 132
    HuffmanCode {
        code: 0x3fffd4,
        bits: 22,
    }, // 133
    HuffmanCode {
        code: 0x3fffd5,
        bits: 22,
    }, // 134
    HuffmanCode {
        code: 0x7fffd9,
        bits: 23,
    }, // 135
    HuffmanCode {
        code: 0x3fffd6,
        bits: 22,
    }, // 136
    HuffmanCode {
        code: 0x7fffda,
        bits: 23,
    }, // 137
    HuffmanCode {
        code: 0x7fffdb,
        bits: 23,
    }, // 138
    HuffmanCode {
        code: 0x7fffdc,
        bits: 23,
    }, // 139
    HuffmanCode {
        code: 0x7fffdd,
        bits: 23,
    }, // 140
    HuffmanCode {
        code: 0x7fffde,
        bits: 23,
    }, // 141
    HuffmanCode {
        code: 0xffffeb,
        bits: 24,
    }, // 142
    HuffmanCode {
        code: 0x7fffdf,
        bits: 23,
    }, // 143
    HuffmanCode {
        code: 0xffffec,
        bits: 24,
    }, // 144
    HuffmanCode {
        code: 0xffffed,
        bits: 24,
    }, // 145
    HuffmanCode {
        code: 0x3fffd7,
        bits: 22,
    }, // 146
    HuffmanCode {
        code: 0x7fffe0,
        bits: 23,
    }, // 147
    HuffmanCode {
        code: 0xffffee,
        bits: 24,
    }, // 148
    HuffmanCode {
        code: 0x7fffe1,
        bits: 23,
    }, // 149
    HuffmanCode {
        code: 0x7fffe2,
        bits: 23,
    }, // 150
    HuffmanCode {
        code: 0x7fffe3,
        bits: 23,
    }, // 151
    HuffmanCode {
        code: 0x7fffe4,
        bits: 23,
    }, // 152
    HuffmanCode {
        code: 0x1fffdc,
        bits: 21,
    }, // 153
    HuffmanCode {
        code: 0x3fffd8,
        bits: 22,
    }, // 154
    HuffmanCode {
        code: 0x7fffe5,
        bits: 23,
    }, // 155
    HuffmanCode {
        code: 0x3fffd9,
        bits: 22,
    }, // 156
    HuffmanCode {
        code: 0x7fffe6,
        bits: 23,
    }, // 157
    HuffmanCode {
        code: 0x7fffe7,
        bits: 23,
    }, // 158
    HuffmanCode {
        code: 0xffffef,
        bits: 24,
    }, // 159
    HuffmanCode {
        code: 0x3fffda,
        bits: 22,
    }, // 160
    HuffmanCode {
        code: 0x1fffdd,
        bits: 21,
    }, // 161
    HuffmanCode {
        code: 0xfffe9,
        bits: 20,
    }, // 162
    HuffmanCode {
        code: 0x3fffdb,
        bits: 22,
    }, // 163
    HuffmanCode {
        code: 0x3fffdc,
        bits: 22,
    }, // 164
    HuffmanCode {
        code: 0x7fffe8,
        bits: 23,
    }, // 165
    HuffmanCode {
        code: 0x7fffe9,
        bits: 23,
    }, // 166
    HuffmanCode {
        code: 0x1fffde,
        bits: 21,
    }, // 167
    HuffmanCode {
        code: 0x7fffea,
        bits: 23,
    }, // 168
    HuffmanCode {
        code: 0x3fffdd,
        bits: 22,
    }, // 169
    HuffmanCode {
        code: 0x3fffde,
        bits: 22,
    }, // 170
    HuffmanCode {
        code: 0xfffff0,
        bits: 24,
    }, // 171
    HuffmanCode {
        code: 0x1fffdf,
        bits: 21,
    }, // 172
    HuffmanCode {
        code: 0x3fffdf,
        bits: 22,
    }, // 173
    HuffmanCode {
        code: 0x7fffeb,
        bits: 23,
    }, // 174
    HuffmanCode {
        code: 0x7fffec,
        bits: 23,
    }, // 175
    HuffmanCode {
        code: 0x1fffe0,
        bits: 21,
    }, // 176
    HuffmanCode {
        code: 0x1fffe1,
        bits: 21,
    }, // 177
    HuffmanCode {
        code: 0x3fffe0,
        bits: 22,
    }, // 178
    HuffmanCode {
        code: 0x1fffe2,
        bits: 21,
    }, // 179
    HuffmanCode {
        code: 0x7fffed,
        bits: 23,
    }, // 180
    HuffmanCode {
        code: 0x3fffe1,
        bits: 22,
    }, // 181
    HuffmanCode {
        code: 0x7fffee,
        bits: 23,
    }, // 182
    HuffmanCode {
        code: 0x7fffef,
        bits: 23,
    }, // 183
    HuffmanCode {
        code: 0xfffea,
        bits: 20,
    }, // 184
    HuffmanCode {
        code: 0x3fffe2,
        bits: 22,
    }, // 185
    HuffmanCode {
        code: 0x3fffe3,
        bits: 22,
    }, // 186
    HuffmanCode {
        code: 0x3fffe4,
        bits: 22,
    }, // 187
    HuffmanCode {
        code: 0x7ffff0,
        bits: 23,
    }, // 188
    HuffmanCode {
        code: 0x3fffe5,
        bits: 22,
    }, // 189
    HuffmanCode {
        code: 0x3fffe6,
        bits: 22,
    }, // 190
    HuffmanCode {
        code: 0x7ffff1,
        bits: 23,
    }, // 191
    HuffmanCode {
        code: 0x3ffffe0,
        bits: 26,
    }, // 192
    HuffmanCode {
        code: 0x3ffffe1,
        bits: 26,
    }, // 193
    HuffmanCode {
        code: 0xfffeb,
        bits: 20,
    }, // 194
    HuffmanCode {
        code: 0x7fff1,
        bits: 19,
    }, // 195
    HuffmanCode {
        code: 0x3fffe7,
        bits: 22,
    }, // 196
    HuffmanCode {
        code: 0x7ffff2,
        bits: 23,
    }, // 197
    HuffmanCode {
        code: 0x3fffe8,
        bits: 22,
    }, // 198
    HuffmanCode {
        code: 0x1ffffec,
        bits: 25,
    }, // 199
    HuffmanCode {
        code: 0x3ffffe2,
        bits: 26,
    }, // 200
    HuffmanCode {
        code: 0x3ffffe3,
        bits: 26,
    }, // 201
    HuffmanCode {
        code: 0x3ffffe4,
        bits: 26,
    }, // 202
    HuffmanCode {
        code: 0x7ffffde,
        bits: 27,
    }, // 203
    HuffmanCode {
        code: 0x7ffffdf,
        bits: 27,
    }, // 204
    HuffmanCode {
        code: 0x3ffffe5,
        bits: 26,
    }, // 205
    HuffmanCode {
        code: 0xfffff1,
        bits: 24,
    }, // 206
    HuffmanCode {
        code: 0x1ffffed,
        bits: 25,
    }, // 207
    HuffmanCode {
        code: 0x7fff2,
        bits: 19,
    }, // 208
    HuffmanCode {
        code: 0x1fffe3,
        bits: 21,
    }, // 209
    HuffmanCode {
        code: 0x3ffffe6,
        bits: 26,
    }, // 210
    HuffmanCode {
        code: 0x7ffffe0,
        bits: 27,
    }, // 211
    HuffmanCode {
        code: 0x7ffffe1,
        bits: 27,
    }, // 212
    HuffmanCode {
        code: 0x3ffffe7,
        bits: 26,
    }, // 213
    HuffmanCode {
        code: 0x7ffffe2,
        bits: 27,
    }, // 214
    HuffmanCode {
        code: 0xfffff2,
        bits: 24,
    }, // 215
    HuffmanCode {
        code: 0x1fffe4,
        bits: 21,
    }, // 216
    HuffmanCode {
        code: 0x1fffe5,
        bits: 21,
    }, // 217
    HuffmanCode {
        code: 0x3ffffe8,
        bits: 26,
    }, // 218
    HuffmanCode {
        code: 0x3ffffe9,
        bits: 26,
    }, // 219
    HuffmanCode {
        code: 0xffffffd,
        bits: 28,
    }, // 220
    HuffmanCode {
        code: 0x7ffffe3,
        bits: 27,
    }, // 221
    HuffmanCode {
        code: 0x7ffffe4,
        bits: 27,
    }, // 222
    HuffmanCode {
        code: 0x7ffffe5,
        bits: 27,
    }, // 223
    HuffmanCode {
        code: 0xfffec,
        bits: 20,
    }, // 224
    HuffmanCode {
        code: 0xfffff3,
        bits: 24,
    }, // 225
    HuffmanCode {
        code: 0xfffed,
        bits: 20,
    }, // 226
    HuffmanCode {
        code: 0x1fffe6,
        bits: 21,
    }, // 227
    HuffmanCode {
        code: 0x3fffe9,
        bits: 22,
    }, // 228
    HuffmanCode {
        code: 0x1fffe7,
        bits: 21,
    }, // 229
    HuffmanCode {
        code: 0x1fffe8,
        bits: 21,
    }, // 230
    HuffmanCode {
        code: 0x7ffff3,
        bits: 23,
    }, // 231
    HuffmanCode {
        code: 0x3fffea,
        bits: 22,
    }, // 232
    HuffmanCode {
        code: 0x3fffeb,
        bits: 22,
    }, // 233
    HuffmanCode {
        code: 0x1ffffee,
        bits: 25,
    }, // 234
    HuffmanCode {
        code: 0x1ffffef,
        bits: 25,
    }, // 235
    HuffmanCode {
        code: 0xfffff4,
        bits: 24,
    }, // 236
    HuffmanCode {
        code: 0xfffff5,
        bits: 24,
    }, // 237
    HuffmanCode {
        code: 0x3ffffea,
        bits: 26,
    }, // 238
    HuffmanCode {
        code: 0x7ffff4,
        bits: 23,
    }, // 239
    HuffmanCode {
        code: 0x3ffffeb,
        bits: 26,
    }, // 240
    HuffmanCode {
        code: 0x7ffffe6,
        bits: 27,
    }, // 241
    HuffmanCode {
        code: 0x3ffffec,
        bits: 26,
    }, // 242
    HuffmanCode {
        code: 0x3ffffed,
        bits: 26,
    }, // 243
    HuffmanCode {
        code: 0x7ffffe7,
        bits: 27,
    }, // 244
    HuffmanCode {
        code: 0x7ffffe8,
        bits: 27,
    }, // 245
    HuffmanCode {
        code: 0x7ffffe9,
        bits: 27,
    }, // 246
    HuffmanCode {
        code: 0x7ffffea,
        bits: 27,
    }, // 247
    HuffmanCode {
        code: 0x7ffffeb,
        bits: 27,
    }, // 248
    HuffmanCode {
        code: 0xffffffe,
        bits: 28,
    }, // 249
    HuffmanCode {
        code: 0x7ffffec,
        bits: 27,
    }, // 250
    HuffmanCode {
        code: 0x7ffffed,
        bits: 27,
    }, // 251
    HuffmanCode {
        code: 0x7ffffee,
        bits: 27,
    }, // 252
    HuffmanCode {
        code: 0x7ffffef,
        bits: 27,
    }, // 253
    HuffmanCode {
        code: 0x7fffff0,
        bits: 27,
    }, // 254
    HuffmanCode {
        code: 0x3ffffee,
        bits: 26,
    }, // 255
    HuffmanCode {
        code: 0x3fffffff,
        bits: 30,
    }, // EOS (256)
];

/// Encode data using Huffman coding
fn huffman_encode(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut current: u64 = 0;
    let mut bits: u8 = 0;

    for &byte in data {
        let entry = &HUFFMAN_TABLE[byte as usize];
        current = (current << entry.bits) | (entry.code as u64);
        bits += entry.bits;

        while bits >= 8 {
            bits -= 8;
            output.push((current >> bits) as u8);
        }
    }

    // Pad with EOS prefix (all 1s)
    if bits > 0 {
        current = (current << (8 - bits)) | ((1u64 << (8 - bits)) - 1);
        output.push(current as u8);
    }

    output
}

/// Decode Huffman-encoded data
fn huffman_decode(data: &[u8]) -> Result<Vec<u8>> {
    // Build decoding tree lazily (simplified linear search for now)
    let mut output = Vec::new();
    let mut current: u32 = 0;
    let mut bits: u8 = 0;

    for &byte in data {
        current = (current << 8) | (byte as u32);
        bits += 8;

        while bits >= 5 {
            // Try to find a matching symbol
            let mut found = false;
            for (sym, entry) in HUFFMAN_TABLE.iter().enumerate() {
                if entry.bits <= bits {
                    let shift = bits - entry.bits;
                    let candidate = current >> shift;
                    if candidate == entry.code {
                        if sym == 256 {
                            // EOS - should only appear in padding
                            return Ok(output);
                        }
                        output.push(sym as u8);
                        current &= (1 << shift) - 1;
                        bits = shift;
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                break;
            }
        }
    }

    // Remaining bits should be EOS padding (all 1s)
    if bits > 0 && bits <= 7 {
        let mask = (1u32 << bits) - 1;
        if current != mask {
            return Err(Error::Hpack(HpackError::HuffmanDecode));
        }
    }

    Ok(output)
}

// ============================================================================
// HPACK Main Interface
// ============================================================================

/// Main HPACK codec
pub struct Hpack {
    encoder: HpackEncoder,
    decoder: HpackDecoder,
}

impl Hpack {
    /// Create a new HPACK codec
    pub fn new(max_table_size: usize) -> Self {
        Self {
            encoder: HpackEncoder::new(max_table_size),
            decoder: HpackDecoder::new(max_table_size),
        }
    }

    /// Get the encoder
    pub fn encoder(&mut self) -> &mut HpackEncoder {
        &mut self.encoder
    }

    /// Get the decoder
    pub fn decoder(&mut self) -> &mut HpackDecoder {
        &mut self.decoder
    }

    /// Encode headers
    pub fn encode(&mut self, headers: &[HeaderField], buf: &mut Vec<u8>) -> Result<()> {
        self.encoder.encode(headers, buf)
    }

    /// Decode headers
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<HeaderField>> {
        self.decoder.decode(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_table_lookup() {
        let encoder = HpackEncoder::new(4096);

        // :method GET is at index 2
        let result = encoder.find_in_static(b":method", b"GET");
        assert_eq!(result, Some((2, true)));

        // :path / is at index 4
        let result = encoder.find_in_static(b":path", b"/");
        assert_eq!(result, Some((4, true)));
    }

    #[test]
    fn test_encode_decode_simple() {
        let mut hpack = Hpack::new(4096);

        let headers = vec![
            HeaderField::new(b":method".to_vec(), b"GET".to_vec()),
            HeaderField::new(b":path".to_vec(), b"/".to_vec()),
            HeaderField::new(b":scheme".to_vec(), b"https".to_vec()),
        ];

        let mut buf = Vec::new();
        hpack.encode(&headers, &mut buf).unwrap();

        let decoded = hpack.decode(&buf).unwrap();
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].name, b":method");
        assert_eq!(decoded[0].value, b"GET");
    }

    #[test]
    fn test_dynamic_table() {
        let mut table = DynamicTable::new(4096);

        let field = HeaderField::new(b"custom-header".to_vec(), b"custom-value".to_vec());
        let size = field.size();

        table.insert(field.clone());

        assert_eq!(table.len(), 1);
        assert_eq!(table.size(), size);
        assert_eq!(table.get(0).unwrap().name, b"custom-header");
    }
}
