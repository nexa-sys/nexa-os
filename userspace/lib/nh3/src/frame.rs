//! HTTP/3 Frame handling (RFC 9114)
//!
//! This module implements HTTP/3 frame serialization and deserialization.

use crate::constants::frame_type;
use crate::error::{ErrorCode, Result};

// ============================================================================
// Frame Type Enum
// ============================================================================

/// HTTP/3 frame types
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// DATA frame - carries request or response body
    Data = 0x00,
    /// HEADERS frame - carries HTTP header fields
    Headers = 0x01,
    /// CANCEL_PUSH frame - cancels a server push
    CancelPush = 0x03,
    /// SETTINGS frame - communicates configuration parameters
    Settings = 0x04,
    /// PUSH_PROMISE frame - initiates a server push
    PushPromise = 0x05,
    /// GOAWAY frame - initiates graceful shutdown
    Goaway = 0x07,
    /// MAX_PUSH_ID frame - controls maximum push ID
    MaxPushId = 0x0D,
    /// Unknown/reserved frame type
    Unknown(u64),
}

impl From<u64> for FrameType {
    fn from(value: u64) -> Self {
        match value {
            0x00 => FrameType::Data,
            0x01 => FrameType::Headers,
            0x03 => FrameType::CancelPush,
            0x04 => FrameType::Settings,
            0x05 => FrameType::PushPromise,
            0x07 => FrameType::Goaway,
            0x0D => FrameType::MaxPushId,
            other => FrameType::Unknown(other),
        }
    }
}

impl From<FrameType> for u64 {
    fn from(ft: FrameType) -> u64 {
        match ft {
            FrameType::Data => 0x00,
            FrameType::Headers => 0x01,
            FrameType::CancelPush => 0x03,
            FrameType::Settings => 0x04,
            FrameType::PushPromise => 0x05,
            FrameType::Goaway => 0x07,
            FrameType::MaxPushId => 0x0D,
            FrameType::Unknown(v) => v,
        }
    }
}

impl FrameType {
    /// Check if this is a reserved frame type that should be ignored
    pub fn is_reserved(&self) -> bool {
        match self {
            FrameType::Unknown(v) => {
                // Reserved frame types: 0x21 + 0x1f * N (for N >= 0)
                *v >= 0x21 && (*v - 0x21) % 0x1f == 0
            }
            _ => false,
        }
    }
}

// ============================================================================
// Frame Header
// ============================================================================

/// Frame header (type + length)
#[derive(Debug, Clone, Copy)]
pub struct FrameHeader {
    /// Frame type
    pub frame_type: FrameType,
    /// Payload length
    pub length: u64,
}

impl FrameHeader {
    /// Create a new frame header
    pub fn new(frame_type: FrameType, length: u64) -> Self {
        Self { frame_type, length }
    }
    
    /// Encode the frame header to a buffer
    pub fn encode(&self, buf: &mut Vec<u8>) {
        encode_varint(buf, self.frame_type.into());
        encode_varint(buf, self.length);
    }
    
    /// Decode a frame header from a buffer
    pub fn decode(data: &[u8]) -> Result<(Self, usize)> {
        let mut pos = 0;
        
        let (frame_type, consumed) = decode_varint(&data[pos..])?;
        pos += consumed;
        
        let (length, consumed) = decode_varint(&data[pos..])?;
        pos += consumed;
        
        Ok((
            Self {
                frame_type: FrameType::from(frame_type),
                length,
            },
            pos,
        ))
    }
}

// ============================================================================
// Frame Enum
// ============================================================================

/// HTTP/3 frame
#[derive(Debug, Clone)]
pub enum Frame {
    /// DATA frame
    Data(DataPayload),
    /// HEADERS frame
    Headers(HeadersPayload),
    /// CANCEL_PUSH frame
    CancelPush(CancelPushPayload),
    /// SETTINGS frame
    Settings(SettingsPayload),
    /// PUSH_PROMISE frame
    PushPromise(PushPromisePayload),
    /// GOAWAY frame
    Goaway(GoawayPayload),
    /// MAX_PUSH_ID frame
    MaxPushId(MaxPushIdPayload),
    /// Unknown frame (to be ignored)
    Unknown { frame_type: u64, payload: Vec<u8> },
}

impl Frame {
    /// Get the frame type
    pub fn frame_type(&self) -> FrameType {
        match self {
            Frame::Data(_) => FrameType::Data,
            Frame::Headers(_) => FrameType::Headers,
            Frame::CancelPush(_) => FrameType::CancelPush,
            Frame::Settings(_) => FrameType::Settings,
            Frame::PushPromise(_) => FrameType::PushPromise,
            Frame::Goaway(_) => FrameType::Goaway,
            Frame::MaxPushId(_) => FrameType::MaxPushId,
            Frame::Unknown { frame_type, .. } => FrameType::Unknown(*frame_type),
        }
    }
    
    /// Encode the frame to a buffer
    pub fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        match self {
            Frame::Data(payload) => {
                encode_varint(buf, frame_type::DATA);
                encode_varint(buf, payload.data.len() as u64);
                buf.extend_from_slice(&payload.data);
            }
            Frame::Headers(payload) => {
                encode_varint(buf, frame_type::HEADERS);
                encode_varint(buf, payload.header_block.len() as u64);
                buf.extend_from_slice(&payload.header_block);
            }
            Frame::CancelPush(payload) => {
                let mut inner = Vec::new();
                encode_varint(&mut inner, payload.push_id);
                encode_varint(buf, frame_type::CANCEL_PUSH);
                encode_varint(buf, inner.len() as u64);
                buf.extend_from_slice(&inner);
            }
            Frame::Settings(payload) => {
                let mut inner = Vec::new();
                for (id, value) in &payload.settings {
                    encode_varint(&mut inner, *id);
                    encode_varint(&mut inner, *value);
                }
                encode_varint(buf, frame_type::SETTINGS);
                encode_varint(buf, inner.len() as u64);
                buf.extend_from_slice(&inner);
            }
            Frame::PushPromise(payload) => {
                let mut inner = Vec::new();
                encode_varint(&mut inner, payload.push_id);
                inner.extend_from_slice(&payload.header_block);
                encode_varint(buf, frame_type::PUSH_PROMISE);
                encode_varint(buf, inner.len() as u64);
                buf.extend_from_slice(&inner);
            }
            Frame::Goaway(payload) => {
                let mut inner = Vec::new();
                encode_varint(&mut inner, payload.id);
                encode_varint(buf, frame_type::GOAWAY);
                encode_varint(buf, inner.len() as u64);
                buf.extend_from_slice(&inner);
            }
            Frame::MaxPushId(payload) => {
                let mut inner = Vec::new();
                encode_varint(&mut inner, payload.push_id);
                encode_varint(buf, frame_type::MAX_PUSH_ID);
                encode_varint(buf, inner.len() as u64);
                buf.extend_from_slice(&inner);
            }
            Frame::Unknown { frame_type, payload } => {
                encode_varint(buf, *frame_type);
                encode_varint(buf, payload.len() as u64);
                buf.extend_from_slice(payload);
            }
        }
        Ok(())
    }
    
    /// Decode a frame from a buffer
    pub fn decode(data: &[u8]) -> Result<(Self, usize)> {
        let (header, header_len) = FrameHeader::decode(data)?;
        
        let payload_start = header_len;
        let payload_end = header_len + header.length as usize;
        
        if payload_end > data.len() {
            return Err(ErrorCode::NoBuf.into());
        }
        
        let payload_data = &data[payload_start..payload_end];
        
        let frame = match header.frame_type {
            FrameType::Data => {
                Frame::Data(DataPayload {
                    data: payload_data.to_vec(),
                })
            }
            FrameType::Headers => {
                Frame::Headers(HeadersPayload {
                    header_block: payload_data.to_vec(),
                })
            }
            FrameType::CancelPush => {
                let (push_id, _) = decode_varint(payload_data)?;
                Frame::CancelPush(CancelPushPayload { push_id })
            }
            FrameType::Settings => {
                let mut settings = Vec::new();
                let mut pos = 0;
                while pos < payload_data.len() {
                    let (id, consumed) = decode_varint(&payload_data[pos..])?;
                    pos += consumed;
                    let (value, consumed) = decode_varint(&payload_data[pos..])?;
                    pos += consumed;
                    settings.push((id, value));
                }
                Frame::Settings(SettingsPayload { settings })
            }
            FrameType::PushPromise => {
                let (push_id, consumed) = decode_varint(payload_data)?;
                Frame::PushPromise(PushPromisePayload {
                    push_id,
                    header_block: payload_data[consumed..].to_vec(),
                })
            }
            FrameType::Goaway => {
                let (id, _) = decode_varint(payload_data)?;
                Frame::Goaway(GoawayPayload { id })
            }
            FrameType::MaxPushId => {
                let (push_id, _) = decode_varint(payload_data)?;
                Frame::MaxPushId(MaxPushIdPayload { push_id })
            }
            FrameType::Unknown(ft) => {
                Frame::Unknown {
                    frame_type: ft,
                    payload: payload_data.to_vec(),
                }
            }
        };
        
        Ok((frame, payload_end))
    }
}

// ============================================================================
// Frame Payloads
// ============================================================================

/// DATA frame payload
#[derive(Debug, Clone)]
pub struct DataPayload {
    /// Raw data
    pub data: Vec<u8>,
}

/// HEADERS frame payload
#[derive(Debug, Clone)]
pub struct HeadersPayload {
    /// QPACK-encoded header block
    pub header_block: Vec<u8>,
}

/// CANCEL_PUSH frame payload
#[derive(Debug, Clone, Copy)]
pub struct CancelPushPayload {
    /// Push ID to cancel
    pub push_id: u64,
}

/// SETTINGS frame payload
#[derive(Debug, Clone)]
pub struct SettingsPayload {
    /// List of (setting_id, value) pairs
    pub settings: Vec<(u64, u64)>,
}

impl SettingsPayload {
    /// Create default settings
    pub fn default_client() -> Self {
        use crate::constants::settings_id::*;
        Self {
            settings: vec![
                (MAX_FIELD_SECTION_SIZE, 16384),
                (QPACK_MAX_TABLE_CAPACITY, 4096),
                (QPACK_BLOCKED_STREAMS, 100),
            ],
        }
    }
    
    /// Create default server settings
    pub fn default_server() -> Self {
        use crate::constants::settings_id::*;
        Self {
            settings: vec![
                (MAX_FIELD_SECTION_SIZE, 16384),
                (QPACK_MAX_TABLE_CAPACITY, 4096),
                (QPACK_BLOCKED_STREAMS, 100),
            ],
        }
    }
}

/// PUSH_PROMISE frame payload
#[derive(Debug, Clone)]
pub struct PushPromisePayload {
    /// Push ID
    pub push_id: u64,
    /// QPACK-encoded header block
    pub header_block: Vec<u8>,
}

/// GOAWAY frame payload
#[derive(Debug, Clone, Copy)]
pub struct GoawayPayload {
    /// Stream ID or Push ID
    pub id: u64,
}

/// MAX_PUSH_ID frame payload
#[derive(Debug, Clone, Copy)]
pub struct MaxPushIdPayload {
    /// Maximum push ID
    pub push_id: u64,
}

// ============================================================================
// Variable-Length Integer Encoding (RFC 9000 Section 16)
// ============================================================================

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
        buf.push(0xC0 | ((value >> 56) as u8));
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
        return Err(ErrorCode::NoBuf.into());
    }
    
    let first = data[0];
    let length = 1 << (first >> 6);
    
    if data.len() < length {
        return Err(ErrorCode::NoBuf.into());
    }
    
    let value = match length {
        1 => first as u64,
        2 => {
            ((first as u64 & 0x3F) << 8) | (data[1] as u64)
        }
        4 => {
            ((first as u64 & 0x3F) << 24)
                | ((data[1] as u64) << 16)
                | ((data[2] as u64) << 8)
                | (data[3] as u64)
        }
        8 => {
            ((first as u64 & 0x3F) << 56)
                | ((data[1] as u64) << 48)
                | ((data[2] as u64) << 40)
                | ((data[3] as u64) << 32)
                | ((data[4] as u64) << 24)
                | ((data[5] as u64) << 16)
                | ((data[6] as u64) << 8)
                | (data[7] as u64)
        }
        _ => unreachable!(),
    };
    
    Ok((value, length))
}

/// Get the encoded length of a variable-length integer
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_varint_encoding() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 0);
        assert_eq!(buf, vec![0]);
        
        buf.clear();
        encode_varint(&mut buf, 63);
        assert_eq!(buf, vec![63]);
        
        buf.clear();
        encode_varint(&mut buf, 64);
        assert_eq!(buf, vec![0x40, 0x40]);
        
        buf.clear();
        encode_varint(&mut buf, 16383);
        assert_eq!(buf, vec![0x7F, 0xFF]);
    }
    
    #[test]
    fn test_varint_decoding() {
        let (val, len) = decode_varint(&[0]).unwrap();
        assert_eq!(val, 0);
        assert_eq!(len, 1);
        
        let (val, len) = decode_varint(&[63]).unwrap();
        assert_eq!(val, 63);
        assert_eq!(len, 1);
        
        let (val, len) = decode_varint(&[0x40, 0x40]).unwrap();
        assert_eq!(val, 64);
        assert_eq!(len, 2);
    }
    
    #[test]
    fn test_frame_roundtrip() {
        let settings = Frame::Settings(SettingsPayload::default_client());
        let mut buf = Vec::new();
        settings.encode(&mut buf).unwrap();
        
        let (decoded, _) = Frame::decode(&buf).unwrap();
        if let Frame::Settings(payload) = decoded {
            assert!(!payload.settings.is_empty());
        } else {
            panic!("Expected Settings frame");
        }
    }
}
