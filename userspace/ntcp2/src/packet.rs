//! QUIC Packet handling
//!
//! This module implements QUIC packet serialization and deserialization
//! according to RFC 9000 Section 12-17.

use crate::constants::{packet_type, crypto, HEADER_FORM_LONG, HEADER_FIXED_BIT, 
    LONG_HEADER_TYPE_MASK, SHORT_HEADER_KEY_PHASE, PN_LENGTH_MASK};
use crate::error::{Error, Result, NgError};
use crate::{ConnectionId, NGTCP2_MAX_CIDLEN};

// ============================================================================
// Packet Type Enum
// ============================================================================

/// QUIC packet types
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    /// Initial packet (long header)
    Initial,
    /// 0-RTT packet (long header)
    ZeroRtt,
    /// Handshake packet (long header)
    Handshake,
    /// Retry packet (long header)
    Retry,
    /// 1-RTT packet (short header)
    Short,
    /// Version negotiation packet
    VersionNegotiation,
}

impl PacketType {
    /// Check if this packet type uses a long header
    pub fn is_long_header(&self) -> bool {
        !matches!(self, PacketType::Short)
    }
    
    /// Get the encryption level for this packet type
    pub fn encryption_level(&self) -> crate::types::EncryptionLevel {
        match self {
            PacketType::Initial => crate::types::EncryptionLevel::Initial,
            PacketType::ZeroRtt => crate::types::EncryptionLevel::ZeroRtt,
            PacketType::Handshake => crate::types::EncryptionLevel::Handshake,
            PacketType::Short => crate::types::EncryptionLevel::OneRtt,
            PacketType::Retry | PacketType::VersionNegotiation => {
                crate::types::EncryptionLevel::Initial
            }
        }
    }
}

// ============================================================================
// Packet Header
// ============================================================================

/// QUIC packet header
#[derive(Debug, Clone)]
pub struct PacketHeader {
    /// Packet type
    pub pkt_type: PacketType,
    /// QUIC version (long header only)
    pub version: u32,
    /// Destination connection ID
    pub dcid: ConnectionId,
    /// Source connection ID (long header only)
    pub scid: ConnectionId,
    /// Packet number (decoded)
    pub pkt_num: u64,
    /// Packet number length in bytes
    pub pkt_num_len: usize,
    /// Key phase (1-RTT only)
    pub key_phase: bool,
    /// Spin bit (1-RTT only)
    pub spin: bool,
    /// Token (Initial only)
    pub token: Vec<u8>,
    /// Payload length (long header only)
    pub length: usize,
}

impl PacketHeader {
    /// Create a new Initial packet header
    pub fn initial(version: u32, dcid: ConnectionId, scid: ConnectionId) -> Self {
        Self {
            pkt_type: PacketType::Initial,
            version,
            dcid,
            scid,
            pkt_num: 0,
            pkt_num_len: 1,
            key_phase: false,
            spin: false,
            token: Vec::new(),
            length: 0,
        }
    }
    
    /// Create a new Handshake packet header
    pub fn handshake(version: u32, dcid: ConnectionId, scid: ConnectionId) -> Self {
        Self {
            pkt_type: PacketType::Handshake,
            version,
            dcid,
            scid,
            pkt_num: 0,
            pkt_num_len: 1,
            key_phase: false,
            spin: false,
            token: Vec::new(),
            length: 0,
        }
    }
    
    /// Create a new 1-RTT (short header) packet header
    pub fn short(dcid: ConnectionId) -> Self {
        Self {
            pkt_type: PacketType::Short,
            version: 0,
            dcid,
            scid: ConnectionId::empty(),
            pkt_num: 0,
            pkt_num_len: 1,
            key_phase: false,
            spin: false,
            token: Vec::new(),
            length: 0,
        }
    }
    
    /// Get the header length in bytes (without packet number)
    pub fn header_len(&self) -> usize {
        match self.pkt_type {
            PacketType::Initial => {
                // 1 (flags) + 4 (version) + 1 (DCID len) + dcid + 1 (SCID len) + scid
                // + varint(token len) + token + varint(length)
                1 + 4 + 1 + self.dcid.datalen + 1 + self.scid.datalen
                    + varint_len(self.token.len() as u64) + self.token.len()
                    + varint_len(self.length as u64)
            }
            PacketType::Handshake | PacketType::ZeroRtt => {
                // 1 (flags) + 4 (version) + 1 (DCID len) + dcid + 1 (SCID len) + scid
                // + varint(length)
                1 + 4 + 1 + self.dcid.datalen + 1 + self.scid.datalen
                    + varint_len(self.length as u64)
            }
            PacketType::Retry => {
                // 1 (flags) + 4 (version) + 1 (DCID len) + dcid + 1 (SCID len) + scid
                1 + 4 + 1 + self.dcid.datalen + 1 + self.scid.datalen
            }
            PacketType::Short => {
                // 1 (flags) + dcid
                1 + self.dcid.datalen
            }
            PacketType::VersionNegotiation => {
                // 1 (flags) + 4 (version=0) + 1 (DCID len) + dcid + 1 (SCID len) + scid
                1 + 4 + 1 + self.dcid.datalen + 1 + self.scid.datalen
            }
        }
    }
}

// ============================================================================
// Packet
// ============================================================================

/// Complete QUIC packet
#[derive(Debug, Clone)]
pub struct Packet {
    /// Packet header
    pub header: PacketHeader,
    /// Packet payload (decrypted frames)
    pub payload: Vec<u8>,
}

impl Packet {
    /// Create a new packet
    pub fn new(header: PacketHeader, payload: Vec<u8>) -> Self {
        Self { header, payload }
    }
}

// ============================================================================
// Packet Parsing
// ============================================================================

/// Parse a QUIC packet header
pub fn parse_header(data: &[u8], dcid_len: usize) -> Result<(PacketHeader, usize)> {
    if data.is_empty() {
        return Err(Error::BufferTooSmall);
    }
    
    let first_byte = data[0];
    
    // Check header form
    if (first_byte & HEADER_FORM_LONG) != 0 {
        parse_long_header(data)
    } else {
        parse_short_header(data, dcid_len)
    }
}

/// Parse a long header packet
fn parse_long_header(data: &[u8]) -> Result<(PacketHeader, usize)> {
    if data.len() < 6 {
        return Err(Error::BufferTooSmall);
    }
    
    let first_byte = data[0];
    let mut offset = 1;
    
    // Version
    let version = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
    offset += 4;
    
    // Check for version negotiation
    if version == 0 {
        return parse_version_negotiation(data, first_byte, offset);
    }
    
    // Determine packet type
    let pkt_type = match (first_byte & LONG_HEADER_TYPE_MASK) >> 4 {
        0x00 => PacketType::Initial,
        0x01 => PacketType::ZeroRtt,
        0x02 => PacketType::Handshake,
        0x03 => PacketType::Retry,
        _ => return Err(Error::Ng(NgError::Proto)),
    };
    
    // DCID length and DCID
    if offset >= data.len() {
        return Err(Error::BufferTooSmall);
    }
    let dcid_len = data[offset] as usize;
    offset += 1;
    
    if dcid_len > NGTCP2_MAX_CIDLEN || offset + dcid_len > data.len() {
        return Err(Error::Ng(NgError::Proto));
    }
    let dcid = ConnectionId::new(&data[offset..offset + dcid_len]);
    offset += dcid_len;
    
    // SCID length and SCID
    if offset >= data.len() {
        return Err(Error::BufferTooSmall);
    }
    let scid_len = data[offset] as usize;
    offset += 1;
    
    if scid_len > NGTCP2_MAX_CIDLEN || offset + scid_len > data.len() {
        return Err(Error::Ng(NgError::Proto));
    }
    let scid = ConnectionId::new(&data[offset..offset + scid_len]);
    offset += scid_len;
    
    // Token (Initial only)
    let token = if pkt_type == PacketType::Initial {
        let (token_len, varint_size) = decode_varint(&data[offset..])?;
        offset += varint_size;
        
        if offset + token_len as usize > data.len() {
            return Err(Error::BufferTooSmall);
        }
        let token = data[offset..offset + token_len as usize].to_vec();
        offset += token_len as usize;
        token
    } else {
        Vec::new()
    };
    
    // Payload length (not for Retry)
    let length = if pkt_type != PacketType::Retry {
        let (len, varint_size) = decode_varint(&data[offset..])?;
        offset += varint_size;
        len as usize
    } else {
        0
    };
    
    // Packet number length (in first byte, after decryption)
    let pkt_num_len = ((first_byte & PN_LENGTH_MASK) + 1) as usize;
    
    let header = PacketHeader {
        pkt_type,
        version,
        dcid,
        scid,
        pkt_num: 0, // Will be decoded after header protection removal
        pkt_num_len,
        key_phase: false,
        spin: false,
        token,
        length,
    };
    
    Ok((header, offset))
}

/// Parse a short header packet
fn parse_short_header(data: &[u8], dcid_len: usize) -> Result<(PacketHeader, usize)> {
    if data.len() < 1 + dcid_len {
        return Err(Error::BufferTooSmall);
    }
    
    let first_byte = data[0];
    
    // Check fixed bit
    if (first_byte & HEADER_FIXED_BIT) == 0 {
        return Err(Error::Ng(NgError::Proto));
    }
    
    let spin = (first_byte & 0x20) != 0;
    let key_phase = (first_byte & SHORT_HEADER_KEY_PHASE) != 0;
    let pkt_num_len = ((first_byte & PN_LENGTH_MASK) + 1) as usize;
    
    let dcid = ConnectionId::new(&data[1..1 + dcid_len]);
    
    let header = PacketHeader {
        pkt_type: PacketType::Short,
        version: 0,
        dcid,
        scid: ConnectionId::empty(),
        pkt_num: 0,
        pkt_num_len,
        key_phase,
        spin,
        token: Vec::new(),
        length: 0,
    };
    
    Ok((header, 1 + dcid_len))
}

/// Parse version negotiation packet
fn parse_version_negotiation(data: &[u8], first_byte: u8, mut offset: usize) 
    -> Result<(PacketHeader, usize)> 
{
    // DCID
    if offset >= data.len() {
        return Err(Error::BufferTooSmall);
    }
    let dcid_len = data[offset] as usize;
    offset += 1;
    
    if dcid_len > NGTCP2_MAX_CIDLEN || offset + dcid_len > data.len() {
        return Err(Error::Ng(NgError::Proto));
    }
    let dcid = ConnectionId::new(&data[offset..offset + dcid_len]);
    offset += dcid_len;
    
    // SCID
    if offset >= data.len() {
        return Err(Error::BufferTooSmall);
    }
    let scid_len = data[offset] as usize;
    offset += 1;
    
    if scid_len > NGTCP2_MAX_CIDLEN || offset + scid_len > data.len() {
        return Err(Error::Ng(NgError::Proto));
    }
    let scid = ConnectionId::new(&data[offset..offset + scid_len]);
    offset += scid_len;
    
    let header = PacketHeader {
        pkt_type: PacketType::VersionNegotiation,
        version: 0,
        dcid,
        scid,
        pkt_num: 0,
        pkt_num_len: 0,
        key_phase: false,
        spin: false,
        token: Vec::new(),
        length: data.len() - offset,
    };
    
    Ok((header, offset))
}

// ============================================================================
// Packet Building
// ============================================================================

/// Packet builder for constructing QUIC packets
pub struct PacketBuilder {
    /// Output buffer
    buffer: Vec<u8>,
    /// Packet header
    header: PacketHeader,
    /// Header offset in buffer
    header_offset: usize,
    /// Payload start offset
    payload_offset: usize,
}

impl PacketBuilder {
    /// Create a new packet builder
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            header: PacketHeader::short(ConnectionId::empty()),
            header_offset: 0,
            payload_offset: 0,
        }
    }
    
    /// Start building an Initial packet
    pub fn start_initial(
        &mut self,
        version: u32,
        dcid: &ConnectionId,
        scid: &ConnectionId,
        token: &[u8],
    ) -> Result<()> {
        self.buffer.clear();
        self.header_offset = 0;
        
        // First byte (will be updated with packet number length later)
        let first_byte = HEADER_FORM_LONG | HEADER_FIXED_BIT | (packet_type::INITIAL << 4);
        self.buffer.push(first_byte);
        
        // Version
        self.buffer.extend_from_slice(&version.to_be_bytes());
        
        // DCID
        self.buffer.push(dcid.datalen as u8);
        self.buffer.extend_from_slice(dcid.as_slice());
        
        // SCID
        self.buffer.push(scid.datalen as u8);
        self.buffer.extend_from_slice(scid.as_slice());
        
        // Token
        encode_varint(&mut self.buffer, token.len() as u64);
        self.buffer.extend_from_slice(token);
        
        // Length placeholder (2 bytes for most packets)
        let length_offset = self.buffer.len();
        self.buffer.extend_from_slice(&[0, 0]);
        
        self.payload_offset = self.buffer.len();
        
        self.header = PacketHeader {
            pkt_type: PacketType::Initial,
            version,
            dcid: *dcid,
            scid: *scid,
            pkt_num: 0,
            pkt_num_len: 1,
            key_phase: false,
            spin: false,
            token: token.to_vec(),
            length: 0,
        };
        
        Ok(())
    }
    
    /// Start building a Handshake packet
    pub fn start_handshake(
        &mut self,
        version: u32,
        dcid: &ConnectionId,
        scid: &ConnectionId,
    ) -> Result<()> {
        self.buffer.clear();
        self.header_offset = 0;
        
        let first_byte = HEADER_FORM_LONG | HEADER_FIXED_BIT | (packet_type::HANDSHAKE << 4);
        self.buffer.push(first_byte);
        
        self.buffer.extend_from_slice(&version.to_be_bytes());
        
        self.buffer.push(dcid.datalen as u8);
        self.buffer.extend_from_slice(dcid.as_slice());
        
        self.buffer.push(scid.datalen as u8);
        self.buffer.extend_from_slice(scid.as_slice());
        
        // Length placeholder
        self.buffer.extend_from_slice(&[0, 0]);
        
        self.payload_offset = self.buffer.len();
        
        self.header = PacketHeader {
            pkt_type: PacketType::Handshake,
            version,
            dcid: *dcid,
            scid: *scid,
            pkt_num: 0,
            pkt_num_len: 1,
            key_phase: false,
            spin: false,
            token: Vec::new(),
            length: 0,
        };
        
        Ok(())
    }
    
    /// Start building a 1-RTT (short header) packet
    pub fn start_short(&mut self, dcid: &ConnectionId, key_phase: bool) -> Result<()> {
        self.buffer.clear();
        self.header_offset = 0;
        
        let mut first_byte = HEADER_FIXED_BIT;
        if key_phase {
            first_byte |= SHORT_HEADER_KEY_PHASE;
        }
        self.buffer.push(first_byte);
        
        self.buffer.extend_from_slice(dcid.as_slice());
        
        self.payload_offset = self.buffer.len();
        
        self.header = PacketHeader {
            pkt_type: PacketType::Short,
            version: 0,
            dcid: *dcid,
            scid: ConnectionId::empty(),
            pkt_num: 0,
            pkt_num_len: 1,
            key_phase,
            spin: false,
            token: Vec::new(),
            length: 0,
        };
        
        Ok(())
    }
    
    /// Get the current buffer for adding payload
    pub fn payload_buffer(&mut self) -> &mut Vec<u8> {
        &mut self.buffer
    }
    
    /// Finish building the packet
    pub fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

// ============================================================================
// Variable-Length Integer Encoding (RFC 9000 Section 16)
// ============================================================================

/// Get the length of a varint encoding
pub fn varint_len(value: u64) -> usize {
    if value < 64 {
        1
    } else if value < 16384 {
        2
    } else if value < 1073741824 {
        4
    } else {
        8
    }
}

/// Encode a variable-length integer
pub fn encode_varint(buf: &mut Vec<u8>, value: u64) {
    if value < 64 {
        buf.push(value as u8);
    } else if value < 16384 {
        buf.push(0x40 | ((value >> 8) as u8));
        buf.push(value as u8);
    } else if value < 1073741824 {
        buf.push(0x80 | ((value >> 24) as u8));
        buf.push((value >> 16) as u8);
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    } else {
        buf.push(0xc0 | ((value >> 56) as u8));
        buf.push((value >> 48) as u8);
        buf.push((value >> 40) as u8);
        buf.push((value >> 32) as u8);
        buf.push((value >> 24) as u8);
        buf.push((value >> 16) as u8);
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    }
}

/// Decode a variable-length integer
pub fn decode_varint(data: &[u8]) -> Result<(u64, usize)> {
    if data.is_empty() {
        return Err(Error::BufferTooSmall);
    }
    
    let first = data[0];
    let len = 1 << (first >> 6);
    
    if data.len() < len {
        return Err(Error::BufferTooSmall);
    }
    
    let value = match len {
        1 => first as u64,
        2 => {
            let v = ((first & 0x3f) as u64) << 8 | data[1] as u64;
            v
        }
        4 => {
            let v = ((first & 0x3f) as u64) << 24
                | (data[1] as u64) << 16
                | (data[2] as u64) << 8
                | data[3] as u64;
            v
        }
        8 => {
            let v = ((first & 0x3f) as u64) << 56
                | (data[1] as u64) << 48
                | (data[2] as u64) << 40
                | (data[3] as u64) << 32
                | (data[4] as u64) << 24
                | (data[5] as u64) << 16
                | (data[6] as u64) << 8
                | data[7] as u64;
            v
        }
        _ => unreachable!(),
    };
    
    Ok((value, len))
}

// ============================================================================
// Packet Number Encoding/Decoding
// ============================================================================

/// Encode a packet number with the smallest encoding
pub fn encode_pkt_num(pkt_num: u64, largest_acked: u64) -> (u64, usize) {
    let diff = pkt_num.saturating_sub(largest_acked);
    
    if diff < 128 {
        (pkt_num & 0xff, 1)
    } else if diff < 32768 {
        (pkt_num & 0xffff, 2)
    } else if diff < 8388608 {
        (pkt_num & 0xffffff, 3)
    } else {
        (pkt_num & 0xffffffff, 4)
    }
}

/// Decode a truncated packet number
pub fn decode_pkt_num(truncated: u64, pkt_num_len: usize, largest_pn: u64) -> u64 {
    let expected = largest_pn + 1;
    let win = 1u64 << (pkt_num_len * 8);
    let half_win = win / 2;
    
    let candidate = (expected & !(win - 1)) | truncated;
    
    if candidate <= expected.saturating_sub(half_win) && candidate < (1u64 << 62) - win {
        candidate + win
    } else if candidate > expected + half_win && candidate >= win {
        candidate - win
    } else {
        candidate
    }
}
