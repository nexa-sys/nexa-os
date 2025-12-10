//! HTTP/2 Frame handling
//!
//! This module implements HTTP/2 frame serialization and deserialization
//! according to RFC 7540 Section 4.

use crate::constants::{frame_flags, frame_type, FRAME_HEADER_LENGTH, MAX_MAX_FRAME_SIZE};
use crate::error::{Error, ErrorCode, Result};
use crate::types::{GoawayData, Nv, PrioritySpec, SettingsEntry, StreamId};

// ============================================================================
// Frame Type Enum
// ============================================================================

/// HTTP/2 frame types
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Data = 0x0,
    Headers = 0x1,
    Priority = 0x2,
    RstStream = 0x3,
    Settings = 0x4,
    PushPromise = 0x5,
    Ping = 0x6,
    Goaway = 0x7,
    WindowUpdate = 0x8,
    Continuation = 0x9,
    AltSvc = 0xa,
    Origin = 0xc,
    Unknown(u8),
}

impl From<u8> for FrameType {
    fn from(value: u8) -> Self {
        match value {
            0x0 => FrameType::Data,
            0x1 => FrameType::Headers,
            0x2 => FrameType::Priority,
            0x3 => FrameType::RstStream,
            0x4 => FrameType::Settings,
            0x5 => FrameType::PushPromise,
            0x6 => FrameType::Ping,
            0x7 => FrameType::Goaway,
            0x8 => FrameType::WindowUpdate,
            0x9 => FrameType::Continuation,
            0xa => FrameType::AltSvc,
            0xc => FrameType::Origin,
            other => FrameType::Unknown(other),
        }
    }
}

impl From<FrameType> for u8 {
    fn from(ft: FrameType) -> u8 {
        match ft {
            FrameType::Data => 0x0,
            FrameType::Headers => 0x1,
            FrameType::Priority => 0x2,
            FrameType::RstStream => 0x3,
            FrameType::Settings => 0x4,
            FrameType::PushPromise => 0x5,
            FrameType::Ping => 0x6,
            FrameType::Goaway => 0x7,
            FrameType::WindowUpdate => 0x8,
            FrameType::Continuation => 0x9,
            FrameType::AltSvc => 0xa,
            FrameType::Origin => 0xc,
            FrameType::Unknown(v) => v,
        }
    }
}

// ============================================================================
// Frame Flags
// ============================================================================

/// Frame flags wrapper
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameFlags(pub u8);

impl FrameFlags {
    pub const NONE: Self = Self(0);
    pub const END_STREAM: Self = Self(0x01);
    pub const ACK: Self = Self(0x01);
    pub const END_HEADERS: Self = Self(0x04);
    pub const PADDED: Self = Self(0x08);
    pub const PRIORITY: Self = Self(0x20);

    pub fn new(flags: u8) -> Self {
        Self(flags)
    }

    pub fn has(&self, flag: FrameFlags) -> bool {
        (self.0 & flag.0) != 0
    }

    pub fn set(&mut self, flag: FrameFlags) {
        self.0 |= flag.0;
    }

    pub fn clear(&mut self, flag: FrameFlags) {
        self.0 &= !flag.0;
    }

    pub fn end_stream(&self) -> bool {
        self.has(Self::END_STREAM)
    }

    pub fn ack(&self) -> bool {
        self.has(Self::ACK)
    }

    pub fn end_headers(&self) -> bool {
        self.has(Self::END_HEADERS)
    }

    pub fn padded(&self) -> bool {
        self.has(Self::PADDED)
    }

    pub fn priority(&self) -> bool {
        self.has(Self::PRIORITY)
    }
}

// ============================================================================
// Frame Header
// ============================================================================

/// HTTP/2 frame header (9 bytes)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FrameHeader {
    /// Length of the frame payload (24 bits)
    pub length: u32,
    /// Frame type
    pub frame_type: FrameType,
    /// Frame flags
    pub flags: FrameFlags,
    /// Stream identifier (31 bits)
    pub stream_id: StreamId,
}

impl FrameHeader {
    /// Create a new frame header
    pub fn new(frame_type: FrameType, flags: FrameFlags, stream_id: StreamId, length: u32) -> Self {
        Self {
            length,
            frame_type,
            flags,
            stream_id,
        }
    }

    /// Parse a frame header from bytes
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < FRAME_HEADER_LENGTH {
            return Err(Error::BufferTooSmall);
        }

        let length = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | (data[2] as u32);
        let frame_type = FrameType::from(data[3]);
        let flags = FrameFlags::new(data[4]);
        let stream_id = ((data[5] as i32 & 0x7F) << 24)
            | ((data[6] as i32) << 16)
            | ((data[7] as i32) << 8)
            | (data[8] as i32);

        // Validate frame size
        if length > MAX_MAX_FRAME_SIZE {
            return Err(Error::Protocol(ErrorCode::FrameSizeError));
        }

        Ok(Self {
            length,
            frame_type,
            flags,
            stream_id,
        })
    }

    /// Serialize frame header to bytes
    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.len() < FRAME_HEADER_LENGTH {
            return Err(Error::BufferTooSmall);
        }

        buf[0] = ((self.length >> 16) & 0xFF) as u8;
        buf[1] = ((self.length >> 8) & 0xFF) as u8;
        buf[2] = (self.length & 0xFF) as u8;
        buf[3] = self.frame_type.into();
        buf[4] = self.flags.0;
        buf[5] = ((self.stream_id >> 24) & 0x7F) as u8;
        buf[6] = ((self.stream_id >> 16) & 0xFF) as u8;
        buf[7] = ((self.stream_id >> 8) & 0xFF) as u8;
        buf[8] = (self.stream_id & 0xFF) as u8;

        Ok(FRAME_HEADER_LENGTH)
    }
}

// ============================================================================
// Frame Payload Types
// ============================================================================

/// DATA frame payload
#[derive(Debug)]
pub struct DataFrame {
    pub header: FrameHeader,
    pub pad_length: Option<u8>,
    pub data: Vec<u8>,
}

/// HEADERS frame payload
#[derive(Debug)]
pub struct HeadersFrame {
    pub header: FrameHeader,
    pub pad_length: Option<u8>,
    pub priority: Option<PrioritySpec>,
    pub header_block: Vec<u8>,
}

/// PRIORITY frame payload
#[derive(Debug, Clone, Copy)]
pub struct PriorityFrame {
    pub header: FrameHeader,
    pub priority: PrioritySpec,
}

/// RST_STREAM frame payload
#[derive(Debug, Clone, Copy)]
pub struct RstStreamFrame {
    pub header: FrameHeader,
    pub error_code: ErrorCode,
}

/// SETTINGS frame payload
#[derive(Debug)]
pub struct SettingsFrame {
    pub header: FrameHeader,
    pub entries: Vec<SettingsEntry>,
}

/// PUSH_PROMISE frame payload
#[derive(Debug)]
pub struct PushPromiseFrame {
    pub header: FrameHeader,
    pub pad_length: Option<u8>,
    pub promised_stream_id: StreamId,
    pub header_block: Vec<u8>,
}

/// PING frame payload
#[derive(Debug, Clone, Copy)]
pub struct PingFrame {
    pub header: FrameHeader,
    pub opaque_data: [u8; 8],
}

/// GOAWAY frame payload
#[derive(Debug)]
pub struct GoawayFrame {
    pub header: FrameHeader,
    pub last_stream_id: StreamId,
    pub error_code: ErrorCode,
    pub debug_data: Vec<u8>,
}

/// WINDOW_UPDATE frame payload
#[derive(Debug, Clone, Copy)]
pub struct WindowUpdateFrame {
    pub header: FrameHeader,
    pub window_size_increment: u32,
}

/// CONTINUATION frame payload
#[derive(Debug)]
pub struct ContinuationFrame {
    pub header: FrameHeader,
    pub header_block: Vec<u8>,
}

// ============================================================================
// Frame Enum
// ============================================================================

/// Generic frame type containing all possible frames
#[derive(Debug)]
pub enum Frame {
    Data(DataFrame),
    Headers(HeadersFrame),
    Priority(PriorityFrame),
    RstStream(RstStreamFrame),
    Settings(SettingsFrame),
    PushPromise(PushPromiseFrame),
    Ping(PingFrame),
    Goaway(GoawayFrame),
    WindowUpdate(WindowUpdateFrame),
    Continuation(ContinuationFrame),
    Unknown(FrameHeader, Vec<u8>),
}

impl Frame {
    /// Get the frame header
    pub fn header(&self) -> &FrameHeader {
        match self {
            Frame::Data(f) => &f.header,
            Frame::Headers(f) => &f.header,
            Frame::Priority(f) => &f.header,
            Frame::RstStream(f) => &f.header,
            Frame::Settings(f) => &f.header,
            Frame::PushPromise(f) => &f.header,
            Frame::Ping(f) => &f.header,
            Frame::Goaway(f) => &f.header,
            Frame::WindowUpdate(f) => &f.header,
            Frame::Continuation(f) => &f.header,
            Frame::Unknown(h, _) => h,
        }
    }

    /// Get the stream ID
    pub fn stream_id(&self) -> StreamId {
        self.header().stream_id
    }

    /// Get the frame type
    pub fn frame_type(&self) -> FrameType {
        self.header().frame_type
    }
}

// ============================================================================
// Frame Builder
// ============================================================================

/// Builder for creating frames
pub struct FrameBuilder {
    max_frame_size: u32,
}

impl FrameBuilder {
    /// Create a new frame builder
    pub fn new(max_frame_size: u32) -> Self {
        Self { max_frame_size }
    }

    /// Create a DATA frame
    pub fn data(&self, stream_id: StreamId, data: &[u8], end_stream: bool) -> Result<DataFrame> {
        if data.len() > self.max_frame_size as usize {
            return Err(Error::Protocol(ErrorCode::FrameSizeError));
        }

        let mut flags = FrameFlags::NONE;
        if end_stream {
            flags.set(FrameFlags::END_STREAM);
        }

        Ok(DataFrame {
            header: FrameHeader::new(FrameType::Data, flags, stream_id, data.len() as u32),
            pad_length: None,
            data: data.to_vec(),
        })
    }

    /// Create a HEADERS frame
    pub fn headers(
        &self,
        stream_id: StreamId,
        header_block: &[u8],
        end_stream: bool,
        end_headers: bool,
        priority: Option<PrioritySpec>,
    ) -> Result<HeadersFrame> {
        let mut flags = FrameFlags::NONE;
        if end_stream {
            flags.set(FrameFlags::END_STREAM);
        }
        if end_headers {
            flags.set(FrameFlags::END_HEADERS);
        }
        if priority.is_some() {
            flags.set(FrameFlags::PRIORITY);
        }

        let priority_len = if priority.is_some() { 5 } else { 0 };
        let total_len = header_block.len() + priority_len;

        Ok(HeadersFrame {
            header: FrameHeader::new(FrameType::Headers, flags, stream_id, total_len as u32),
            pad_length: None,
            priority,
            header_block: header_block.to_vec(),
        })
    }

    /// Create a SETTINGS frame
    pub fn settings(entries: &[SettingsEntry], ack: bool) -> SettingsFrame {
        let mut flags = FrameFlags::NONE;
        if ack {
            flags.set(FrameFlags::ACK);
        }

        let length = if ack { 0 } else { entries.len() * 6 };

        SettingsFrame {
            header: FrameHeader::new(FrameType::Settings, flags, 0, length as u32),
            entries: entries.to_vec(),
        }
    }

    /// Create a PING frame
    pub fn ping(opaque_data: [u8; 8], ack: bool) -> PingFrame {
        let mut flags = FrameFlags::NONE;
        if ack {
            flags.set(FrameFlags::ACK);
        }

        PingFrame {
            header: FrameHeader::new(FrameType::Ping, flags, 0, 8),
            opaque_data,
        }
    }

    /// Create a GOAWAY frame
    pub fn goaway(
        last_stream_id: StreamId,
        error_code: ErrorCode,
        debug_data: &[u8],
    ) -> GoawayFrame {
        GoawayFrame {
            header: FrameHeader::new(
                FrameType::Goaway,
                FrameFlags::NONE,
                0,
                (8 + debug_data.len()) as u32,
            ),
            last_stream_id,
            error_code,
            debug_data: debug_data.to_vec(),
        }
    }

    /// Create a WINDOW_UPDATE frame
    pub fn window_update(stream_id: StreamId, increment: u32) -> Result<WindowUpdateFrame> {
        if increment == 0 || increment > 0x7FFFFFFF {
            return Err(Error::Protocol(ErrorCode::ProtocolError));
        }

        Ok(WindowUpdateFrame {
            header: FrameHeader::new(FrameType::WindowUpdate, FrameFlags::NONE, stream_id, 4),
            window_size_increment: increment,
        })
    }

    /// Create a RST_STREAM frame
    pub fn rst_stream(stream_id: StreamId, error_code: ErrorCode) -> RstStreamFrame {
        RstStreamFrame {
            header: FrameHeader::new(FrameType::RstStream, FrameFlags::NONE, stream_id, 4),
            error_code,
        }
    }
}

// ============================================================================
// Frame Serializer
// ============================================================================

/// Serializes frames to bytes
pub struct FrameSerializer;

impl FrameSerializer {
    /// Serialize a frame to bytes
    pub fn serialize(frame: &Frame, buf: &mut Vec<u8>) -> Result<usize> {
        let start_len = buf.len();

        // Reserve space for header
        buf.resize(start_len + FRAME_HEADER_LENGTH, 0);

        // Serialize payload
        match frame {
            Frame::Data(f) => {
                if let Some(pad_len) = f.pad_length {
                    buf.push(pad_len);
                }
                buf.extend_from_slice(&f.data);
                if let Some(pad_len) = f.pad_length {
                    buf.resize(buf.len() + pad_len as usize, 0);
                }
            }
            Frame::Headers(f) => {
                if let Some(pad_len) = f.pad_length {
                    buf.push(pad_len);
                }
                if let Some(ref pri) = f.priority {
                    let dep = if pri.exclusive != 0 {
                        pri.stream_id | 0x80000000u32 as i32
                    } else {
                        pri.stream_id
                    };
                    buf.extend_from_slice(&dep.to_be_bytes());
                    buf.push((pri.weight - 1) as u8);
                }
                buf.extend_from_slice(&f.header_block);
                if let Some(pad_len) = f.pad_length {
                    buf.resize(buf.len() + pad_len as usize, 0);
                }
            }
            Frame::Priority(f) => {
                let dep = if f.priority.exclusive != 0 {
                    f.priority.stream_id | 0x80000000u32 as i32
                } else {
                    f.priority.stream_id
                };
                buf.extend_from_slice(&dep.to_be_bytes());
                buf.push((f.priority.weight - 1) as u8);
            }
            Frame::RstStream(f) => {
                buf.extend_from_slice(&(f.error_code as u32).to_be_bytes());
            }
            Frame::Settings(f) => {
                for entry in &f.entries {
                    buf.extend_from_slice(&(entry.settings_id as u16).to_be_bytes());
                    buf.extend_from_slice(&entry.value.to_be_bytes());
                }
            }
            Frame::PushPromise(f) => {
                if let Some(pad_len) = f.pad_length {
                    buf.push(pad_len);
                }
                buf.extend_from_slice(&(f.promised_stream_id & 0x7FFFFFFF).to_be_bytes());
                buf.extend_from_slice(&f.header_block);
                if let Some(pad_len) = f.pad_length {
                    buf.resize(buf.len() + pad_len as usize, 0);
                }
            }
            Frame::Ping(f) => {
                buf.extend_from_slice(&f.opaque_data);
            }
            Frame::Goaway(f) => {
                buf.extend_from_slice(&(f.last_stream_id & 0x7FFFFFFF).to_be_bytes());
                buf.extend_from_slice(&(f.error_code as u32).to_be_bytes());
                buf.extend_from_slice(&f.debug_data);
            }
            Frame::WindowUpdate(f) => {
                buf.extend_from_slice(&(f.window_size_increment & 0x7FFFFFFF).to_be_bytes());
            }
            Frame::Continuation(f) => {
                buf.extend_from_slice(&f.header_block);
            }
            Frame::Unknown(_, data) => {
                buf.extend_from_slice(data);
            }
        }

        // Update header with actual length
        let payload_len = buf.len() - start_len - FRAME_HEADER_LENGTH;
        let header = frame.header();
        let mut header_with_len = *header;
        header_with_len.length = payload_len as u32;
        header_with_len.serialize(&mut buf[start_len..start_len + FRAME_HEADER_LENGTH])?;

        Ok(buf.len() - start_len)
    }
}

// ============================================================================
// Frame Parser
// ============================================================================

/// Parses frames from bytes
pub struct FrameParser {
    max_frame_size: u32,
}

impl FrameParser {
    /// Create a new frame parser
    pub fn new(max_frame_size: u32) -> Self {
        Self { max_frame_size }
    }

    /// Parse a complete frame from bytes
    /// Returns the frame and number of bytes consumed
    pub fn parse(&self, data: &[u8]) -> Result<(Frame, usize)> {
        if data.len() < FRAME_HEADER_LENGTH {
            return Err(Error::BufferTooSmall);
        }

        let header = FrameHeader::parse(data)?;
        let total_len = FRAME_HEADER_LENGTH + header.length as usize;

        if data.len() < total_len {
            return Err(Error::BufferTooSmall);
        }

        if header.length > self.max_frame_size {
            return Err(Error::Protocol(ErrorCode::FrameSizeError));
        }

        let payload = &data[FRAME_HEADER_LENGTH..total_len];
        let frame = self.parse_payload(header, payload)?;

        Ok((frame, total_len))
    }

    fn parse_payload(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        match header.frame_type {
            FrameType::Data => self.parse_data(header, payload),
            FrameType::Headers => self.parse_headers(header, payload),
            FrameType::Priority => self.parse_priority(header, payload),
            FrameType::RstStream => self.parse_rst_stream(header, payload),
            FrameType::Settings => self.parse_settings(header, payload),
            FrameType::PushPromise => self.parse_push_promise(header, payload),
            FrameType::Ping => self.parse_ping(header, payload),
            FrameType::Goaway => self.parse_goaway(header, payload),
            FrameType::WindowUpdate => self.parse_window_update(header, payload),
            FrameType::Continuation => self.parse_continuation(header, payload),
            _ => Ok(Frame::Unknown(header, payload.to_vec())),
        }
    }

    fn parse_data(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        let (pad_length, data_start, data_end) = if header.flags.padded() {
            if payload.is_empty() {
                return Err(Error::Protocol(ErrorCode::ProtocolError));
            }
            let pad_len = payload[0] as usize;
            if pad_len >= payload.len() {
                return Err(Error::Protocol(ErrorCode::ProtocolError));
            }
            (Some(payload[0]), 1, payload.len() - pad_len)
        } else {
            (None, 0, payload.len())
        };

        Ok(Frame::Data(DataFrame {
            header,
            pad_length,
            data: payload[data_start..data_end].to_vec(),
        }))
    }

    fn parse_headers(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        let mut offset = 0;

        let pad_length = if header.flags.padded() {
            if payload.is_empty() {
                return Err(Error::Protocol(ErrorCode::ProtocolError));
            }
            let pl = payload[0];
            offset = 1;
            Some(pl)
        } else {
            None
        };

        let priority = if header.flags.priority() {
            if payload.len() < offset + 5 {
                return Err(Error::Protocol(ErrorCode::ProtocolError));
            }
            let dep = i32::from_be_bytes([
                payload[offset],
                payload[offset + 1],
                payload[offset + 2],
                payload[offset + 3],
            ]);
            let exclusive = (dep & 0x80000000u32 as i32) != 0;
            let stream_id = dep & 0x7FFFFFFF;
            let weight = payload[offset + 4] as i32 + 1;
            offset += 5;
            Some(PrioritySpec {
                stream_id,
                weight,
                exclusive: if exclusive { 1 } else { 0 },
            })
        } else {
            None
        };

        let data_end = if let Some(pl) = pad_length {
            payload.len().saturating_sub(pl as usize)
        } else {
            payload.len()
        };

        Ok(Frame::Headers(HeadersFrame {
            header,
            pad_length,
            priority,
            header_block: payload[offset..data_end].to_vec(),
        }))
    }

    fn parse_priority(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        if payload.len() != 5 {
            return Err(Error::Protocol(ErrorCode::FrameSizeError));
        }

        let dep = i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let exclusive = (dep & 0x80000000u32 as i32) != 0;
        let stream_id = dep & 0x7FFFFFFF;
        let weight = payload[4] as i32 + 1;

        Ok(Frame::Priority(PriorityFrame {
            header,
            priority: PrioritySpec {
                stream_id,
                weight,
                exclusive: if exclusive { 1 } else { 0 },
            },
        }))
    }

    fn parse_rst_stream(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        if payload.len() != 4 {
            return Err(Error::Protocol(ErrorCode::FrameSizeError));
        }

        let error_code = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);

        Ok(Frame::RstStream(RstStreamFrame {
            header,
            error_code: ErrorCode::from_u32(error_code),
        }))
    }

    fn parse_settings(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        if header.flags.ack() {
            if !payload.is_empty() {
                return Err(Error::Protocol(ErrorCode::FrameSizeError));
            }
            return Ok(Frame::Settings(SettingsFrame {
                header,
                entries: Vec::new(),
            }));
        }

        if payload.len() % 6 != 0 {
            return Err(Error::Protocol(ErrorCode::FrameSizeError));
        }

        let mut entries = Vec::new();
        for chunk in payload.chunks(6) {
            let id = u16::from_be_bytes([chunk[0], chunk[1]]);
            let value = u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
            entries.push(SettingsEntry {
                settings_id: id as i32,
                value,
            });
        }

        Ok(Frame::Settings(SettingsFrame { header, entries }))
    }

    fn parse_push_promise(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        let mut offset = 0;

        let pad_length = if header.flags.padded() {
            if payload.is_empty() {
                return Err(Error::Protocol(ErrorCode::ProtocolError));
            }
            let pl = payload[0];
            offset = 1;
            Some(pl)
        } else {
            None
        };

        if payload.len() < offset + 4 {
            return Err(Error::Protocol(ErrorCode::ProtocolError));
        }

        let promised = i32::from_be_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]) & 0x7FFFFFFF;
        offset += 4;

        let data_end = if let Some(pl) = pad_length {
            payload.len().saturating_sub(pl as usize)
        } else {
            payload.len()
        };

        Ok(Frame::PushPromise(PushPromiseFrame {
            header,
            pad_length,
            promised_stream_id: promised,
            header_block: payload[offset..data_end].to_vec(),
        }))
    }

    fn parse_ping(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        if payload.len() != 8 {
            return Err(Error::Protocol(ErrorCode::FrameSizeError));
        }

        let mut opaque_data = [0u8; 8];
        opaque_data.copy_from_slice(payload);

        Ok(Frame::Ping(PingFrame {
            header,
            opaque_data,
        }))
    }

    fn parse_goaway(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        if payload.len() < 8 {
            return Err(Error::Protocol(ErrorCode::FrameSizeError));
        }

        let last_stream_id =
            i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) & 0x7FFFFFFF;
        let error_code = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);

        Ok(Frame::Goaway(GoawayFrame {
            header,
            last_stream_id,
            error_code: ErrorCode::from_u32(error_code),
            debug_data: payload[8..].to_vec(),
        }))
    }

    fn parse_window_update(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        if payload.len() != 4 {
            return Err(Error::Protocol(ErrorCode::FrameSizeError));
        }

        let increment =
            u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) & 0x7FFFFFFF;

        if increment == 0 {
            return Err(Error::Protocol(ErrorCode::ProtocolError));
        }

        Ok(Frame::WindowUpdate(WindowUpdateFrame {
            header,
            window_size_increment: increment,
        }))
    }

    fn parse_continuation(&self, header: FrameHeader, payload: &[u8]) -> Result<Frame> {
        Ok(Frame::Continuation(ContinuationFrame {
            header,
            header_block: payload.to_vec(),
        }))
    }
}

// ============================================================================
// C API for Frame Functions
// ============================================================================

/// nghttp2_frame struct (C API compatible)
#[repr(C)]
pub struct NgHttp2Frame {
    pub hd: NgHttp2FrameHd,
}

/// nghttp2_frame_hd struct (C API compatible)
#[repr(C)]
pub struct NgHttp2FrameHd {
    pub length: usize,
    pub stream_id: i32,
    pub frame_type: u8,
    pub flags: u8,
    pub reserved: u8,
}
