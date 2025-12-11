//! QUIC Frame handling
//!
//! This module implements QUIC frame serialization and deserialization
//! according to RFC 9000 Section 19.

use crate::constants::frame_type;
use crate::error::{Error, NgError, Result, TransportError};
use crate::packet::{decode_varint, encode_varint, varint_len};
use crate::types::StreamId;

// ============================================================================
// Frame Type Enum
// ============================================================================

/// QUIC frame types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// PADDING frame
    Padding,
    /// PING frame
    Ping,
    /// ACK frame
    Ack { ecn: bool },
    /// RESET_STREAM frame
    ResetStream,
    /// STOP_SENDING frame
    StopSending,
    /// CRYPTO frame
    Crypto,
    /// NEW_TOKEN frame
    NewToken,
    /// STREAM frame
    Stream { fin: bool, len: bool, off: bool },
    /// MAX_DATA frame
    MaxData,
    /// MAX_STREAM_DATA frame
    MaxStreamData,
    /// MAX_STREAMS frame
    MaxStreams { bidi: bool },
    /// DATA_BLOCKED frame
    DataBlocked,
    /// STREAM_DATA_BLOCKED frame
    StreamDataBlocked,
    /// STREAMS_BLOCKED frame
    StreamsBlocked { bidi: bool },
    /// NEW_CONNECTION_ID frame
    NewConnectionId,
    /// RETIRE_CONNECTION_ID frame
    RetireConnectionId,
    /// PATH_CHALLENGE frame
    PathChallenge,
    /// PATH_RESPONSE frame
    PathResponse,
    /// CONNECTION_CLOSE frame
    ConnectionClose { app: bool },
    /// HANDSHAKE_DONE frame
    HandshakeDone,
    /// DATAGRAM frame (RFC 9221)
    Datagram { len: bool },
    /// Unknown frame type
    Unknown(u64),
}

impl FrameType {
    /// Parse frame type from varint
    pub fn from_varint(val: u64) -> Self {
        match val {
            frame_type::PADDING => FrameType::Padding,
            frame_type::PING => FrameType::Ping,
            frame_type::ACK => FrameType::Ack { ecn: false },
            frame_type::ACK_ECN => FrameType::Ack { ecn: true },
            frame_type::RESET_STREAM => FrameType::ResetStream,
            frame_type::STOP_SENDING => FrameType::StopSending,
            frame_type::CRYPTO => FrameType::Crypto,
            frame_type::NEW_TOKEN => FrameType::NewToken,
            0x08..=0x0f => {
                let fin = (val & 0x01) != 0;
                let len = (val & 0x02) != 0;
                let off = (val & 0x04) != 0;
                FrameType::Stream { fin, len, off }
            }
            frame_type::MAX_DATA => FrameType::MaxData,
            frame_type::MAX_STREAM_DATA => FrameType::MaxStreamData,
            frame_type::MAX_STREAMS_BIDI => FrameType::MaxStreams { bidi: true },
            frame_type::MAX_STREAMS_UNI => FrameType::MaxStreams { bidi: false },
            frame_type::DATA_BLOCKED => FrameType::DataBlocked,
            frame_type::STREAM_DATA_BLOCKED => FrameType::StreamDataBlocked,
            frame_type::STREAMS_BLOCKED_BIDI => FrameType::StreamsBlocked { bidi: true },
            frame_type::STREAMS_BLOCKED_UNI => FrameType::StreamsBlocked { bidi: false },
            frame_type::NEW_CONNECTION_ID => FrameType::NewConnectionId,
            frame_type::RETIRE_CONNECTION_ID => FrameType::RetireConnectionId,
            frame_type::PATH_CHALLENGE => FrameType::PathChallenge,
            frame_type::PATH_RESPONSE => FrameType::PathResponse,
            frame_type::CONNECTION_CLOSE => FrameType::ConnectionClose { app: false },
            frame_type::CONNECTION_CLOSE_APP => FrameType::ConnectionClose { app: true },
            frame_type::HANDSHAKE_DONE => FrameType::HandshakeDone,
            frame_type::DATAGRAM => FrameType::Datagram { len: false },
            frame_type::DATAGRAM_LEN => FrameType::Datagram { len: true },
            v => FrameType::Unknown(v),
        }
    }

    /// Convert to frame type value
    pub fn to_varint(&self) -> u64 {
        match self {
            FrameType::Padding => frame_type::PADDING,
            FrameType::Ping => frame_type::PING,
            FrameType::Ack { ecn: false } => frame_type::ACK,
            FrameType::Ack { ecn: true } => frame_type::ACK_ECN,
            FrameType::ResetStream => frame_type::RESET_STREAM,
            FrameType::StopSending => frame_type::STOP_SENDING,
            FrameType::Crypto => frame_type::CRYPTO,
            FrameType::NewToken => frame_type::NEW_TOKEN,
            FrameType::Stream { fin, len, off } => {
                let mut v = frame_type::STREAM;
                if *fin {
                    v |= 0x01;
                }
                if *len {
                    v |= 0x02;
                }
                if *off {
                    v |= 0x04;
                }
                v
            }
            FrameType::MaxData => frame_type::MAX_DATA,
            FrameType::MaxStreamData => frame_type::MAX_STREAM_DATA,
            FrameType::MaxStreams { bidi: true } => frame_type::MAX_STREAMS_BIDI,
            FrameType::MaxStreams { bidi: false } => frame_type::MAX_STREAMS_UNI,
            FrameType::DataBlocked => frame_type::DATA_BLOCKED,
            FrameType::StreamDataBlocked => frame_type::STREAM_DATA_BLOCKED,
            FrameType::StreamsBlocked { bidi: true } => frame_type::STREAMS_BLOCKED_BIDI,
            FrameType::StreamsBlocked { bidi: false } => frame_type::STREAMS_BLOCKED_UNI,
            FrameType::NewConnectionId => frame_type::NEW_CONNECTION_ID,
            FrameType::RetireConnectionId => frame_type::RETIRE_CONNECTION_ID,
            FrameType::PathChallenge => frame_type::PATH_CHALLENGE,
            FrameType::PathResponse => frame_type::PATH_RESPONSE,
            FrameType::ConnectionClose { app: false } => frame_type::CONNECTION_CLOSE,
            FrameType::ConnectionClose { app: true } => frame_type::CONNECTION_CLOSE_APP,
            FrameType::HandshakeDone => frame_type::HANDSHAKE_DONE,
            FrameType::Datagram { len: false } => frame_type::DATAGRAM,
            FrameType::Datagram { len: true } => frame_type::DATAGRAM_LEN,
            FrameType::Unknown(v) => *v,
        }
    }
}

// ============================================================================
// Frame Structures
// ============================================================================

/// ACK frame data
#[derive(Debug, Clone, Default)]
pub struct AckFrame {
    /// Largest acknowledged packet number
    pub largest_acked: u64,
    /// ACK delay (microseconds * 2^ack_delay_exponent)
    pub ack_delay: u64,
    /// ACK range count
    pub ack_range_count: u64,
    /// First ACK range
    pub first_ack_range: u64,
    /// Additional ACK ranges (gap, ack_range)
    pub ack_ranges: Vec<(u64, u64)>,
    /// ECN counts (ECT0, ECT1, CE)
    pub ecn_counts: Option<(u64, u64, u64)>,
}

/// RESET_STREAM frame data
#[derive(Debug, Clone)]
pub struct ResetStreamFrame {
    /// Stream ID
    pub stream_id: StreamId,
    /// Application error code
    pub error_code: u64,
    /// Final size of the stream
    pub final_size: u64,
}

/// STOP_SENDING frame data
#[derive(Debug, Clone)]
pub struct StopSendingFrame {
    /// Stream ID
    pub stream_id: StreamId,
    /// Application error code
    pub error_code: u64,
}

/// CRYPTO frame data
#[derive(Debug, Clone)]
pub struct CryptoFrame {
    /// Offset in crypto stream
    pub offset: u64,
    /// Crypto data
    pub data: Vec<u8>,
}

/// NEW_TOKEN frame data
#[derive(Debug, Clone)]
pub struct NewTokenFrame {
    /// Token
    pub token: Vec<u8>,
}

/// STREAM frame data
#[derive(Debug, Clone)]
pub struct StreamFrame {
    /// Stream ID
    pub stream_id: StreamId,
    /// Offset in stream (if OFF bit set)
    pub offset: u64,
    /// Stream data
    pub data: Vec<u8>,
    /// FIN flag
    pub fin: bool,
}

/// MAX_DATA frame data
#[derive(Debug, Clone)]
pub struct MaxDataFrame {
    /// Maximum data
    pub max_data: u64,
}

/// MAX_STREAM_DATA frame data
#[derive(Debug, Clone)]
pub struct MaxStreamDataFrame {
    /// Stream ID
    pub stream_id: StreamId,
    /// Maximum stream data
    pub max_stream_data: u64,
}

/// MAX_STREAMS frame data
#[derive(Debug, Clone)]
pub struct MaxStreamsFrame {
    /// Maximum streams
    pub max_streams: u64,
    /// Bidirectional streams
    pub bidi: bool,
}

/// DATA_BLOCKED frame data
#[derive(Debug, Clone)]
pub struct DataBlockedFrame {
    /// Maximum data limit
    pub max_data: u64,
}

/// STREAM_DATA_BLOCKED frame data
#[derive(Debug, Clone)]
pub struct StreamDataBlockedFrame {
    /// Stream ID
    pub stream_id: StreamId,
    /// Maximum stream data limit
    pub max_stream_data: u64,
}

/// STREAMS_BLOCKED frame data
#[derive(Debug, Clone)]
pub struct StreamsBlockedFrame {
    /// Maximum streams limit
    pub max_streams: u64,
    /// Bidirectional streams
    pub bidi: bool,
}

/// NEW_CONNECTION_ID frame data
#[derive(Debug, Clone)]
pub struct NewConnectionIdFrame {
    /// Sequence number
    pub sequence: u64,
    /// Retire prior to
    pub retire_prior_to: u64,
    /// Connection ID
    pub connection_id: Vec<u8>,
    /// Stateless reset token
    pub stateless_reset_token: [u8; 16],
}

/// RETIRE_CONNECTION_ID frame data
#[derive(Debug, Clone)]
pub struct RetireConnectionIdFrame {
    /// Sequence number
    pub sequence: u64,
}

/// PATH_CHALLENGE frame data
#[derive(Debug, Clone)]
pub struct PathChallengeFrame {
    /// 8-byte challenge data
    pub data: [u8; 8],
}

/// PATH_RESPONSE frame data
#[derive(Debug, Clone)]
pub struct PathResponseFrame {
    /// 8-byte response data
    pub data: [u8; 8],
}

/// CONNECTION_CLOSE frame data
#[derive(Debug, Clone)]
pub struct ConnectionCloseFrame {
    /// Error code
    pub error_code: u64,
    /// Frame type (if not application error)
    pub frame_type: Option<u64>,
    /// Reason phrase
    pub reason: Vec<u8>,
    /// Application-level close
    pub app: bool,
}

/// DATAGRAM frame data (RFC 9221)
#[derive(Debug, Clone)]
pub struct DatagramFrame {
    /// Datagram data
    pub data: Vec<u8>,
}

// ============================================================================
// Frame Enum
// ============================================================================

/// Parsed QUIC frame
#[derive(Debug, Clone)]
pub enum Frame {
    Padding,
    Ping,
    Ack(AckFrame),
    ResetStream(ResetStreamFrame),
    StopSending(StopSendingFrame),
    Crypto(CryptoFrame),
    NewToken(NewTokenFrame),
    Stream(StreamFrame),
    MaxData(MaxDataFrame),
    MaxStreamData(MaxStreamDataFrame),
    MaxStreams(MaxStreamsFrame),
    DataBlocked(DataBlockedFrame),
    StreamDataBlocked(StreamDataBlockedFrame),
    StreamsBlocked(StreamsBlockedFrame),
    NewConnectionId(NewConnectionIdFrame),
    RetireConnectionId(RetireConnectionIdFrame),
    PathChallenge(PathChallengeFrame),
    PathResponse(PathResponseFrame),
    ConnectionClose(ConnectionCloseFrame),
    HandshakeDone,
    Datagram(DatagramFrame),
}

impl Frame {
    /// Get the frame type
    pub fn frame_type(&self) -> FrameType {
        match self {
            Frame::Padding => FrameType::Padding,
            Frame::Ping => FrameType::Ping,
            Frame::Ack(f) => FrameType::Ack {
                ecn: f.ecn_counts.is_some(),
            },
            Frame::ResetStream(_) => FrameType::ResetStream,
            Frame::StopSending(_) => FrameType::StopSending,
            Frame::Crypto(_) => FrameType::Crypto,
            Frame::NewToken(_) => FrameType::NewToken,
            Frame::Stream(f) => FrameType::Stream {
                fin: f.fin,
                len: true,
                off: f.offset > 0,
            },
            Frame::MaxData(_) => FrameType::MaxData,
            Frame::MaxStreamData(_) => FrameType::MaxStreamData,
            Frame::MaxStreams(f) => FrameType::MaxStreams { bidi: f.bidi },
            Frame::DataBlocked(_) => FrameType::DataBlocked,
            Frame::StreamDataBlocked(_) => FrameType::StreamDataBlocked,
            Frame::StreamsBlocked(f) => FrameType::StreamsBlocked { bidi: f.bidi },
            Frame::NewConnectionId(_) => FrameType::NewConnectionId,
            Frame::RetireConnectionId(_) => FrameType::RetireConnectionId,
            Frame::PathChallenge(_) => FrameType::PathChallenge,
            Frame::PathResponse(_) => FrameType::PathResponse,
            Frame::ConnectionClose(f) => FrameType::ConnectionClose { app: f.app },
            Frame::HandshakeDone => FrameType::HandshakeDone,
            Frame::Datagram(_) => FrameType::Datagram { len: true },
        }
    }

    /// Check if this frame is ACK-eliciting
    pub fn is_ack_eliciting(&self) -> bool {
        !matches!(self, Frame::Padding | Frame::Ack(_))
    }
}

// ============================================================================
// Frame Parsing
// ============================================================================

/// Parse a single frame from data
pub fn parse_frame(data: &[u8]) -> Result<(Frame, usize)> {
    if data.is_empty() {
        return Err(Error::BufferTooSmall);
    }

    let (frame_type_val, mut offset) = decode_varint(data)?;
    let frame_type = FrameType::from_varint(frame_type_val);

    let frame = match frame_type {
        FrameType::Padding => {
            // Count consecutive padding bytes
            let mut count = 1;
            while offset + count < data.len() && data[offset + count] == 0 {
                count += 1;
            }
            offset += count - 1;
            Frame::Padding
        }

        FrameType::Ping => Frame::Ping,

        FrameType::Ack { ecn } => {
            let (largest_acked, n) = decode_varint(&data[offset..])?;
            offset += n;

            let (ack_delay, n) = decode_varint(&data[offset..])?;
            offset += n;

            let (ack_range_count, n) = decode_varint(&data[offset..])?;
            offset += n;

            let (first_ack_range, n) = decode_varint(&data[offset..])?;
            offset += n;

            let mut ack_ranges = Vec::new();
            for _ in 0..ack_range_count {
                let (gap, n) = decode_varint(&data[offset..])?;
                offset += n;
                let (range, n) = decode_varint(&data[offset..])?;
                offset += n;
                ack_ranges.push((gap, range));
            }

            let ecn_counts = if ecn {
                let (ect0, n) = decode_varint(&data[offset..])?;
                offset += n;
                let (ect1, n) = decode_varint(&data[offset..])?;
                offset += n;
                let (ce, n) = decode_varint(&data[offset..])?;
                offset += n;
                Some((ect0, ect1, ce))
            } else {
                None
            };

            Frame::Ack(AckFrame {
                largest_acked,
                ack_delay,
                ack_range_count,
                first_ack_range,
                ack_ranges,
                ecn_counts,
            })
        }

        FrameType::ResetStream => {
            let (stream_id, n) = decode_varint(&data[offset..])?;
            offset += n;
            let (error_code, n) = decode_varint(&data[offset..])?;
            offset += n;
            let (final_size, n) = decode_varint(&data[offset..])?;
            offset += n;

            Frame::ResetStream(ResetStreamFrame {
                stream_id: stream_id as StreamId,
                error_code,
                final_size,
            })
        }

        FrameType::StopSending => {
            let (stream_id, n) = decode_varint(&data[offset..])?;
            offset += n;
            let (error_code, n) = decode_varint(&data[offset..])?;
            offset += n;

            Frame::StopSending(StopSendingFrame {
                stream_id: stream_id as StreamId,
                error_code,
            })
        }

        FrameType::Crypto => {
            let (crypto_offset, n) = decode_varint(&data[offset..])?;
            offset += n;
            let (length, n) = decode_varint(&data[offset..])?;
            offset += n;

            if offset + length as usize > data.len() {
                return Err(Error::BufferTooSmall);
            }

            let crypto_data = data[offset..offset + length as usize].to_vec();
            offset += length as usize;

            Frame::Crypto(CryptoFrame {
                offset: crypto_offset,
                data: crypto_data,
            })
        }

        FrameType::NewToken => {
            let (length, n) = decode_varint(&data[offset..])?;
            offset += n;

            if offset + length as usize > data.len() {
                return Err(Error::BufferTooSmall);
            }

            let token = data[offset..offset + length as usize].to_vec();
            offset += length as usize;

            Frame::NewToken(NewTokenFrame { token })
        }

        FrameType::Stream { fin, len, off } => {
            let (stream_id, n) = decode_varint(&data[offset..])?;
            offset += n;

            let stream_offset = if off {
                let (o, n) = decode_varint(&data[offset..])?;
                offset += n;
                o
            } else {
                0
            };

            let length = if len {
                let (l, n) = decode_varint(&data[offset..])?;
                offset += n;
                l as usize
            } else {
                data.len() - offset
            };

            if offset + length > data.len() {
                return Err(Error::BufferTooSmall);
            }

            let stream_data = data[offset..offset + length].to_vec();
            offset += length;

            Frame::Stream(StreamFrame {
                stream_id: stream_id as StreamId,
                offset: stream_offset,
                data: stream_data,
                fin,
            })
        }

        FrameType::MaxData => {
            let (max_data, n) = decode_varint(&data[offset..])?;
            offset += n;
            Frame::MaxData(MaxDataFrame { max_data })
        }

        FrameType::MaxStreamData => {
            let (stream_id, n) = decode_varint(&data[offset..])?;
            offset += n;
            let (max_stream_data, n) = decode_varint(&data[offset..])?;
            offset += n;
            Frame::MaxStreamData(MaxStreamDataFrame {
                stream_id: stream_id as StreamId,
                max_stream_data,
            })
        }

        FrameType::MaxStreams { bidi } => {
            let (max_streams, n) = decode_varint(&data[offset..])?;
            offset += n;
            Frame::MaxStreams(MaxStreamsFrame { max_streams, bidi })
        }

        FrameType::DataBlocked => {
            let (max_data, n) = decode_varint(&data[offset..])?;
            offset += n;
            Frame::DataBlocked(DataBlockedFrame { max_data })
        }

        FrameType::StreamDataBlocked => {
            let (stream_id, n) = decode_varint(&data[offset..])?;
            offset += n;
            let (max_stream_data, n) = decode_varint(&data[offset..])?;
            offset += n;
            Frame::StreamDataBlocked(StreamDataBlockedFrame {
                stream_id: stream_id as StreamId,
                max_stream_data,
            })
        }

        FrameType::StreamsBlocked { bidi } => {
            let (max_streams, n) = decode_varint(&data[offset..])?;
            offset += n;
            Frame::StreamsBlocked(StreamsBlockedFrame { max_streams, bidi })
        }

        FrameType::NewConnectionId => {
            let (sequence, n) = decode_varint(&data[offset..])?;
            offset += n;
            let (retire_prior_to, n) = decode_varint(&data[offset..])?;
            offset += n;

            if offset >= data.len() {
                return Err(Error::BufferTooSmall);
            }
            let cid_len = data[offset] as usize;
            offset += 1;

            if cid_len > 20 || offset + cid_len + 16 > data.len() {
                return Err(Error::Ng(NgError::Proto));
            }

            let connection_id = data[offset..offset + cid_len].to_vec();
            offset += cid_len;

            let mut stateless_reset_token = [0u8; 16];
            stateless_reset_token.copy_from_slice(&data[offset..offset + 16]);
            offset += 16;

            Frame::NewConnectionId(NewConnectionIdFrame {
                sequence,
                retire_prior_to,
                connection_id,
                stateless_reset_token,
            })
        }

        FrameType::RetireConnectionId => {
            let (sequence, n) = decode_varint(&data[offset..])?;
            offset += n;
            Frame::RetireConnectionId(RetireConnectionIdFrame { sequence })
        }

        FrameType::PathChallenge => {
            if offset + 8 > data.len() {
                return Err(Error::BufferTooSmall);
            }
            let mut challenge_data = [0u8; 8];
            challenge_data.copy_from_slice(&data[offset..offset + 8]);
            offset += 8;
            Frame::PathChallenge(PathChallengeFrame {
                data: challenge_data,
            })
        }

        FrameType::PathResponse => {
            if offset + 8 > data.len() {
                return Err(Error::BufferTooSmall);
            }
            let mut response_data = [0u8; 8];
            response_data.copy_from_slice(&data[offset..offset + 8]);
            offset += 8;
            Frame::PathResponse(PathResponseFrame {
                data: response_data,
            })
        }

        FrameType::ConnectionClose { app } => {
            let (error_code, n) = decode_varint(&data[offset..])?;
            offset += n;

            let frame_type = if !app {
                let (ft, n) = decode_varint(&data[offset..])?;
                offset += n;
                Some(ft)
            } else {
                None
            };

            let (reason_len, n) = decode_varint(&data[offset..])?;
            offset += n;

            if offset + reason_len as usize > data.len() {
                return Err(Error::BufferTooSmall);
            }

            let reason = data[offset..offset + reason_len as usize].to_vec();
            offset += reason_len as usize;

            Frame::ConnectionClose(ConnectionCloseFrame {
                error_code,
                frame_type,
                reason,
                app,
            })
        }

        FrameType::HandshakeDone => Frame::HandshakeDone,

        FrameType::Datagram { len } => {
            let length = if len {
                let (l, n) = decode_varint(&data[offset..])?;
                offset += n;
                l as usize
            } else {
                data.len() - offset
            };

            if offset + length > data.len() {
                return Err(Error::BufferTooSmall);
            }

            let datagram_data = data[offset..offset + length].to_vec();
            offset += length;

            Frame::Datagram(DatagramFrame {
                data: datagram_data,
            })
        }

        FrameType::Unknown(ft) => {
            return Err(Error::Transport(TransportError::FrameEncodingError));
        }
    };

    Ok((frame, offset))
}

// ============================================================================
// Frame Building
// ============================================================================

/// Build a PADDING frame
pub fn build_padding(buf: &mut Vec<u8>, count: usize) {
    buf.extend(std::iter::repeat(0u8).take(count));
}

/// Build a PING frame
pub fn build_ping(buf: &mut Vec<u8>) {
    encode_varint(buf, frame_type::PING);
}

/// Build an ACK frame
pub fn build_ack(buf: &mut Vec<u8>, frame: &AckFrame) {
    let frame_type = if frame.ecn_counts.is_some() {
        frame_type::ACK_ECN
    } else {
        frame_type::ACK
    };

    encode_varint(buf, frame_type);
    encode_varint(buf, frame.largest_acked);
    encode_varint(buf, frame.ack_delay);
    encode_varint(buf, frame.ack_range_count);
    encode_varint(buf, frame.first_ack_range);

    for (gap, range) in &frame.ack_ranges {
        encode_varint(buf, *gap);
        encode_varint(buf, *range);
    }

    if let Some((ect0, ect1, ce)) = frame.ecn_counts {
        encode_varint(buf, ect0);
        encode_varint(buf, ect1);
        encode_varint(buf, ce);
    }
}

/// Build a CRYPTO frame
pub fn build_crypto(buf: &mut Vec<u8>, offset: u64, data: &[u8]) {
    encode_varint(buf, frame_type::CRYPTO);
    encode_varint(buf, offset);
    encode_varint(buf, data.len() as u64);
    buf.extend_from_slice(data);
}

/// Build a STREAM frame
pub fn build_stream(buf: &mut Vec<u8>, stream_id: StreamId, offset: u64, data: &[u8], fin: bool) {
    let mut frame_type = frame_type::STREAM | 0x02; // Always include length
    if fin {
        frame_type |= 0x01;
    }
    if offset > 0 {
        frame_type |= 0x04;
    }

    encode_varint(buf, frame_type);
    encode_varint(buf, stream_id as u64);
    if offset > 0 {
        encode_varint(buf, offset);
    }
    encode_varint(buf, data.len() as u64);
    buf.extend_from_slice(data);
}

/// Build a MAX_DATA frame
pub fn build_max_data(buf: &mut Vec<u8>, max_data: u64) {
    encode_varint(buf, frame_type::MAX_DATA);
    encode_varint(buf, max_data);
}

/// Build a MAX_STREAM_DATA frame
pub fn build_max_stream_data(buf: &mut Vec<u8>, stream_id: StreamId, max_stream_data: u64) {
    encode_varint(buf, frame_type::MAX_STREAM_DATA);
    encode_varint(buf, stream_id as u64);
    encode_varint(buf, max_stream_data);
}

/// Build a CONNECTION_CLOSE frame
pub fn build_connection_close(
    buf: &mut Vec<u8>,
    error_code: u64,
    frame_type_val: Option<u64>,
    reason: &[u8],
    app: bool,
) {
    let ft = if app {
        frame_type::CONNECTION_CLOSE_APP
    } else {
        frame_type::CONNECTION_CLOSE
    };

    encode_varint(buf, ft);
    encode_varint(buf, error_code);

    if !app {
        encode_varint(buf, frame_type_val.unwrap_or(0));
    }

    encode_varint(buf, reason.len() as u64);
    buf.extend_from_slice(reason);
}

/// Build a HANDSHAKE_DONE frame
pub fn build_handshake_done(buf: &mut Vec<u8>) {
    encode_varint(buf, frame_type::HANDSHAKE_DONE);
}

/// Build a PATH_CHALLENGE frame
pub fn build_path_challenge(buf: &mut Vec<u8>, data: &[u8; 8]) {
    encode_varint(buf, frame_type::PATH_CHALLENGE);
    buf.extend_from_slice(data);
}

/// Build a PATH_RESPONSE frame
pub fn build_path_response(buf: &mut Vec<u8>, data: &[u8; 8]) {
    encode_varint(buf, frame_type::PATH_RESPONSE);
    buf.extend_from_slice(data);
}
