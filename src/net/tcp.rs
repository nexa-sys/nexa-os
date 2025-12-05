use super::drivers::NetError;
use super::ethernet::MacAddress;
use super::ipv4::Ipv4Address;
use super::stack::{TxBatch, MAX_FRAME_SIZE};
use crate::logger;
use crate::process::Pid;
/// TCP protocol implementation
///
/// This module provides a complete TCP stack including connection management,
/// reliable data transfer, flow control, and retransmission.
use crate::{kdebug, kerror, ktrace};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::cmp;

/// TCP header structure
#[repr(C, packed)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset_flags: u16, // 4 bits offset, 6 bits reserved, 6 bits flags
    pub window_size: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

impl TcpHeader {
    pub fn new(src_port: u16, dst_port: u16) -> Self {
        Self {
            src_port: src_port.to_be(),
            dst_port: dst_port.to_be(),
            seq_num: 0,
            ack_num: 0,
            data_offset_flags: (5u16 << 12).to_be(), // 5 words (20 bytes), no flags
            window_size: 8192u16.to_be(),
            checksum: 0,
            urgent_ptr: 0,
        }
    }

    pub fn data_offset(&self) -> usize {
        ((u16::from_be(self.data_offset_flags) >> 12) * 4) as usize
    }

    pub fn flags(&self) -> u8 {
        (u16::from_be(self.data_offset_flags) & 0x3F) as u8
    }

    pub fn set_flags(&mut self, flags: u8) {
        let offset = u16::from_be(self.data_offset_flags) & 0xFFC0;
        self.data_offset_flags = (offset | (flags as u16)).to_be();
    }
}

// TCP flags
pub const TCP_FIN: u8 = 0x01;
pub const TCP_SYN: u8 = 0x02;
pub const TCP_RST: u8 = 0x04;
pub const TCP_PSH: u8 = 0x08;
pub const TCP_ACK: u8 = 0x10;
pub const TCP_URG: u8 = 0x20;

// TCP option kinds
pub const TCP_OPT_END: u8 = 0;
pub const TCP_OPT_NOP: u8 = 1;
pub const TCP_OPT_MSS: u8 = 2;
pub const TCP_OPT_WINDOW_SCALE: u8 = 3;
pub const TCP_OPT_SACK_PERMITTED: u8 = 4;
pub const TCP_OPT_SACK: u8 = 5;
pub const TCP_OPT_TIMESTAMP: u8 = 8;

/// TCP options structure
#[derive(Debug, Clone, Copy)]
pub struct TcpOptions {
    pub mss: Option<u16>,
    pub window_scale: Option<u8>,
    pub sack_permitted: bool,
    pub timestamp: Option<(u32, u32)>, // (TSval, TSecr)
}

impl TcpOptions {
    pub const fn new() -> Self {
        Self {
            mss: None,
            window_scale: None,
            sack_permitted: false,
            timestamp: None,
        }
    }

    /// Parse TCP options from header
    pub fn parse(data: &[u8]) -> Self {
        let mut opts = Self::new();
        let mut i = 0;

        while i < data.len() {
            let kind = data[i];

            match kind {
                TCP_OPT_END => break,
                TCP_OPT_NOP => {
                    i += 1;
                }
                TCP_OPT_MSS => {
                    if i + 3 < data.len() && data[i + 1] == 4 {
                        opts.mss = Some(u16::from_be_bytes([data[i + 2], data[i + 3]]));
                        i += 4;
                    } else {
                        break;
                    }
                }
                TCP_OPT_WINDOW_SCALE => {
                    if i + 2 < data.len() && data[i + 1] == 3 {
                        opts.window_scale = Some(data[i + 2]);
                        i += 3;
                    } else {
                        break;
                    }
                }
                TCP_OPT_SACK_PERMITTED => {
                    if i + 1 < data.len() && data[i + 1] == 2 {
                        opts.sack_permitted = true;
                        i += 2;
                    } else {
                        break;
                    }
                }
                TCP_OPT_TIMESTAMP => {
                    if i + 9 < data.len() && data[i + 1] == 10 {
                        let tsval = u32::from_be_bytes([
                            data[i + 2],
                            data[i + 3],
                            data[i + 4],
                            data[i + 5],
                        ]);
                        let tsecr = u32::from_be_bytes([
                            data[i + 6],
                            data[i + 7],
                            data[i + 8],
                            data[i + 9],
                        ]);
                        opts.timestamp = Some((tsval, tsecr));
                        i += 10;
                    } else {
                        break;
                    }
                }
                _ => {
                    // Unknown option, skip it
                    if i + 1 < data.len() {
                        let len = data[i + 1] as usize;
                        if len >= 2 {
                            i += len;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        opts
    }

    /// Generate options bytes for TCP header
    pub fn generate(&self, buffer: &mut [u8]) -> usize {
        let mut offset = 0;

        // MSS option (4 bytes)
        if let Some(mss) = self.mss {
            if offset + 4 <= buffer.len() {
                buffer[offset] = TCP_OPT_MSS;
                buffer[offset + 1] = 4;
                buffer[offset + 2..offset + 4].copy_from_slice(&mss.to_be_bytes());
                offset += 4;
            }
        }

        // SACK permitted option (2 bytes)
        if self.sack_permitted {
            if offset + 2 <= buffer.len() {
                buffer[offset] = TCP_OPT_SACK_PERMITTED;
                buffer[offset + 1] = 2;
                offset += 2;
            }
        }

        // Window scale option (3 bytes + 1 NOP = 4 bytes)
        if let Some(scale) = self.window_scale {
            if offset + 4 <= buffer.len() {
                buffer[offset] = TCP_OPT_WINDOW_SCALE;
                buffer[offset + 1] = 3;
                buffer[offset + 2] = scale;
                offset += 3;
                // Add NOP for alignment
                buffer[offset] = TCP_OPT_NOP;
                offset += 1;
            }
        }

        // Timestamp option (10 bytes + NOPs for 4-byte alignment)
        if let Some((tsval, tsecr)) = self.timestamp {
            // Pad to 4-byte boundary before timestamp
            while offset % 4 != 0 && offset < buffer.len() {
                buffer[offset] = TCP_OPT_NOP;
                offset += 1;
            }
            if offset + 10 <= buffer.len() {
                buffer[offset] = TCP_OPT_TIMESTAMP;
                buffer[offset + 1] = 10;
                buffer[offset + 2..offset + 6].copy_from_slice(&tsval.to_be_bytes());
                buffer[offset + 6..offset + 10].copy_from_slice(&tsecr.to_be_bytes());
                offset += 10;
            }
        }

        // Final padding to make total length multiple of 4 bytes
        while offset % 4 != 0 && offset < buffer.len() {
            buffer[offset] = TCP_OPT_END;
            offset += 1;
        }

        offset
    }

    /// Calculate the size needed for these options
    pub fn size(&self) -> usize {
        let mut size = 0;

        // MSS: 4 bytes
        if self.mss.is_some() {
            size += 4;
        }

        // SACK permitted: 2 bytes
        if self.sack_permitted {
            size += 2;
        }

        // Window scale: 3 bytes + 1 NOP = 4 bytes
        if self.window_scale.is_some() {
            size += 4;
        }

        // Timestamp: align to 4 bytes + 10 bytes
        if self.timestamp.is_some() {
            // Pad to 4-byte boundary
            size = (size + 3) & !3;
            size += 10;
        }

        // Final padding to 4-byte boundary
        (size + 3) & !3
    }
}

/// TCP connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

/// TCP segment for retransmission queue
#[derive(Clone)]
struct TcpSegment {
    seq: u32,
    data: Vec<u8>,
    flags: u8,
    timestamp: u64,
    retransmit_count: u8,
    rtt_measured: bool, // Whether RTT was measured for this segment (Karn's algorithm)
}

/// Maximum segment size
const MSS: usize = 1460;
/// Receive window size
const RECV_WINDOW: usize = 65535;
/// Send buffer size
const SEND_BUFFER_SIZE: usize = 65535;
/// Receive buffer size
const RECV_BUFFER_SIZE: usize = 65535;
/// Initial retransmission timeout (ms)
const INITIAL_RTO: u64 = 1000;
/// Maximum retransmission timeout (ms)
const MAX_RTO: u64 = 60000;
/// Minimum retransmission timeout (ms)
const MIN_RTO: u64 = 200;
/// Maximum retransmit attempts
const MAX_RETRANSMIT: u8 = 12;

// RTT estimation constants (RFC 6298)
const ALPHA: i64 = 125; // 1/8 = 0.125 in fixed point (125/1000)
const BETA: i64 = 250; // 1/4 = 0.25 in fixed point (250/1000)
const K: i64 = 4; // RTT variance multiplier
const G: i64 = 10; // Clock granularity (10ms)

/// TCP socket structure
pub struct TcpSocket {
    pub state: TcpState,
    pub local_ip: Ipv4Address,
    pub local_port: u16,
    pub remote_ip: Ipv4Address,
    pub remote_port: u16,
    pub local_mac: MacAddress,
    pub remote_mac: MacAddress,
    pub device_idx: Option<usize>,

    // Sequence numbers
    snd_una: u32, // Send unacknowledged
    snd_nxt: u32, // Send next
    snd_wnd: u16, // Send window
    rcv_nxt: u32, // Receive next
    rcv_wnd: u16, // Receive window
    iss: u32,     // Initial send sequence number
    irs: u32,     // Initial receive sequence number

    // MSS and options
    mss: u16,              // Maximum segment size (negotiated)
    peer_mss: u16,         // Peer's MSS
    window_scale: u8,      // Our window scale factor
    peer_window_scale: u8, // Peer's window scale factor
    sack_permitted: bool,  // Whether SACK is permitted
    use_timestamps: bool,  // Whether to use timestamps
    ts_recent: u32,        // Most recent timestamp from peer
    ts_last_ack_sent: u32, // Last ACK we sent (for timestamp echo)

    // Congestion control
    cwnd: u32,           // Congestion window (in bytes)
    ssthresh: u32,       // Slow start threshold (in bytes)
    dup_acks: u8,        // Count of duplicate ACKs
    last_ack: u32,       // Last ACK number received
    in_recovery: bool,   // Whether in fast recovery
    recovery_point: u32, // Sequence number to exit recovery

    // Buffers
    send_buffer: VecDeque<u8>,
    recv_buffer: VecDeque<u8>,
    retransmit_queue: Vec<TcpSegment>,

    // Timers and RTT estimation
    rto: u64,             // Retransmission timeout
    srtt: i64,            // Smoothed RTT (microseconds)
    rttvar: i64,          // RTT variance (microseconds)
    last_activity: u64,   // Last activity timestamp
    rtt_seq: Option<u32>, // Sequence number for RTT measurement
    rtt_time: u64,        // Timestamp when RTT measurement started

    // Wait queue for blocking reads
    pub wait_queue: Vec<Pid>,

    // Flags
    pub in_use: bool,
}

impl TcpSocket {
    pub const fn new() -> Self {
        Self {
            state: TcpState::Closed,
            local_ip: Ipv4Address::UNSPECIFIED,
            local_port: 0,
            remote_ip: Ipv4Address::UNSPECIFIED,
            remote_port: 0,
            local_mac: MacAddress([0; 6]),
            remote_mac: MacAddress([0; 6]),
            device_idx: None,
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: 0,
            rcv_nxt: 0,
            rcv_wnd: RECV_WINDOW as u16,
            iss: 0,
            irs: 0,
            mss: MSS as u16,
            peer_mss: 536, // Default minimum MSS
            window_scale: 0,
            peer_window_scale: 0,
            sack_permitted: false,
            use_timestamps: true,
            ts_recent: 0,
            ts_last_ack_sent: 0,
            cwnd: 10 * MSS as u32, // Initial cwnd: 10 segments (RFC 6928)
            ssthresh: 65535,       // Initial ssthresh: 64KB
            dup_acks: 0,
            last_ack: 0,
            in_recovery: false,
            recovery_point: 0,
            send_buffer: VecDeque::new(),
            recv_buffer: VecDeque::new(),
            retransmit_queue: Vec::new(),
            rto: INITIAL_RTO,
            srtt: 0,
            rttvar: 0,
            last_activity: 0,
            rtt_seq: None,
            rtt_time: 0,
            wait_queue: Vec::new(),
            in_use: false,
        }
    }

    /// Initialize socket for active connection (client)
    pub fn connect(
        &mut self,
        local_ip: Ipv4Address,
        local_port: u16,
        remote_ip: Ipv4Address,
        remote_port: u16,
        local_mac: MacAddress,
        device_idx: usize,
    ) -> Result<(), NetError> {
        if self.state != TcpState::Closed {
            return Err(NetError::InvalidState);
        }

        self.local_ip = local_ip;
        self.local_port = local_port;
        self.remote_ip = remote_ip;
        self.remote_port = remote_port;
        self.local_mac = local_mac;
        self.device_idx = Some(device_idx);
        self.iss = self.generate_isn();
        self.snd_una = self.iss;
        self.snd_nxt = self.iss;
        self.state = TcpState::SynSent;
        self.in_use = true;
        // Set last_activity to 0 to trigger immediate SYN send in poll()
        self.last_activity = 0;

        Ok(())
    }

    /// Initialize socket for passive connection (server)
    pub fn listen(&mut self, local_ip: Ipv4Address, local_port: u16) -> Result<(), NetError> {
        if self.state != TcpState::Closed {
            return Err(NetError::InvalidState);
        }

        self.local_ip = local_ip;
        self.local_port = local_port;
        self.state = TcpState::Listen;
        self.in_use = true;
        self.last_activity = self.current_time();

        Ok(())
    }

    /// Send data (non-blocking)
    pub fn send(&mut self, data: &[u8]) -> Result<usize, NetError> {
        if self.state != TcpState::Established && self.state != TcpState::CloseWait {
            return Err(NetError::InvalidState);
        }

        let available = SEND_BUFFER_SIZE - self.send_buffer.len();
        if available == 0 {
            return Err(NetError::TxBusy);
        }

        let to_send = cmp::min(data.len(), available);
        for &byte in &data[..to_send] {
            self.send_buffer.push_back(byte);
        }

        Ok(to_send)
    }

    /// Receive data (non-blocking)
    pub fn recv(&mut self, buffer: &mut [u8]) -> Result<usize, NetError> {
        if self.recv_buffer.is_empty() {
            // If connection is closed or in CloseWait (FIN received), return EOF
            if self.state == TcpState::Closed || self.state == TcpState::CloseWait {
                ktrace!("[TCP recv] Connection closed or FIN received, returning 0 (EOF)");
                return Ok(0); // EOF
            }
            return Err(NetError::WouldBlock);
        }

        let to_recv = cmp::min(buffer.len(), self.recv_buffer.len());
        for i in 0..to_recv {
            buffer[i] = self.recv_buffer.pop_front().unwrap();
        }

        ktrace!(
            "[TCP recv] Received {} bytes, remaining in buffer={}",
            to_recv,
            self.recv_buffer.len()
        );

        // Update receive window
        self.rcv_wnd = (RECV_BUFFER_SIZE - self.recv_buffer.len()) as u16;

        Ok(to_recv)
    }

    /// Add a process to the wait queue
    pub fn add_waiter(&mut self, pid: Pid) {
        // Avoid duplicates
        if !self.wait_queue.contains(&pid) {
            self.wait_queue.push(pid);
            ktrace!(
                "[TCP add_waiter] Added PID {} to wait queue, queue_len now={}",
                pid,
                self.wait_queue.len()
            );
        }
    }

    /// Close connection
    pub fn close(&mut self) -> Result<(), NetError> {
        match self.state {
            TcpState::Closed => Ok(()),
            TcpState::Listen | TcpState::SynSent => {
                self.reset();
                Ok(())
            }
            TcpState::SynReceived | TcpState::Established => {
                self.state = TcpState::FinWait1;
                Ok(())
            }
            TcpState::CloseWait => {
                self.state = TcpState::LastAck;
                Ok(())
            }
            _ => Err(NetError::InvalidState),
        }
    }

    /// Process incoming TCP segment
    pub fn process_segment(
        &mut self,
        src_ip: Ipv4Address,
        src_mac: MacAddress,
        tcp_data: &[u8],
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        if tcp_data.len() < 20 {
            return Ok(());
        }

        // Parse TCP header
        let src_port = u16::from_be_bytes([tcp_data[0], tcp_data[1]]);
        let dst_port = u16::from_be_bytes([tcp_data[2], tcp_data[3]]);
        let seq = u32::from_be_bytes([tcp_data[4], tcp_data[5], tcp_data[6], tcp_data[7]]);
        let ack = u32::from_be_bytes([tcp_data[8], tcp_data[9], tcp_data[10], tcp_data[11]]);
        let data_offset = ((tcp_data[12] >> 4) * 4) as usize;
        let flags = tcp_data[13];
        let window = u16::from_be_bytes([tcp_data[14], tcp_data[15]]);

        ktrace!(
            "[TCP process_segment] ENTRY: state={:?}, flags={:02x}, {}:{} -> {}:{}, seq={}, ack={}",
            self.state,
            flags,
            src_ip,
            src_port,
            self.local_ip,
            dst_port,
            seq,
            ack
        );

        // Verify this segment is for us
        if dst_port != self.local_port {
            ktrace!(
                "[TCP process_segment] Port mismatch: dst_port={}, local_port={}",
                dst_port,
                self.local_port
            );
            return Ok(());
        }

        if self.state != TcpState::Listen && src_port != self.remote_port {
            ktrace!(
                "[TCP process_segment] Remote port mismatch: src_port={}, remote_port={}",
                src_port,
                self.remote_port
            );
            return Ok(());
        }

        if self.state != TcpState::Listen && src_ip != self.remote_ip {
            ktrace!(
                "[TCP process_segment] Remote IP mismatch: src_ip={}, remote_ip={}",
                src_ip,
                self.remote_ip
            );
            return Ok(());
        }

        self.last_activity = self.current_time();

        ktrace!(
            "[TCP process_segment] TCP data: total_len={}, data_offset={}, header_size={}",
            tcp_data.len(),
            data_offset,
            data_offset
        );

        // Parse TCP options if present
        let options = if data_offset > 20 && data_offset <= tcp_data.len() {
            TcpOptions::parse(&tcp_data[20..data_offset])
        } else {
            TcpOptions::new()
        };

        // Extract payload
        let payload = if data_offset < tcp_data.len() {
            ktrace!("[TCP process_segment] Extracting payload: data_offset={}, tcp_data.len()={}, payload_len={}", data_offset, tcp_data.len(), tcp_data.len() - data_offset);
            &tcp_data[data_offset..]
        } else {
            ktrace!(
                "[TCP process_segment] No payload: data_offset={} >= tcp_data.len()={}",
                data_offset,
                tcp_data.len()
            );
            &[]
        };

        // Handle RST
        if flags & TCP_RST != 0 {
            self.reset();
            return Ok(());
        }

        // State machine
        match self.state {
            TcpState::Listen => {
                if flags & TCP_SYN != 0 {
                    self.remote_ip = src_ip;
                    self.remote_port = src_port;
                    self.remote_mac = src_mac;
                    self.irs = seq;
                    self.rcv_nxt = seq.wrapping_add(1);
                    self.iss = self.generate_isn();
                    self.snd_una = self.iss;
                    self.snd_nxt = self.iss;
                    self.snd_wnd = window;

                    // Process SYN options
                    if let Some(peer_mss) = options.mss {
                        self.peer_mss = peer_mss;
                        ktrace!("[TCP] Peer MSS: {}", peer_mss);
                    }
                    if options.sack_permitted {
                        self.sack_permitted = true;
                    }
                    if let Some(scale) = options.window_scale {
                        self.peer_window_scale = scale;
                    }
                    if options.timestamp.is_some() {
                        self.use_timestamps = true;
                    }

                    self.state = TcpState::SynReceived;
                    kdebug!("[TCP process_segment] Transition Listen->SynReceived for {}:{} (iss={}, irs={})", self.remote_ip, self.remote_port, self.iss, self.irs);
                    self.send_segment(&[], TCP_SYN | TCP_ACK, tx)?;
                }
            }
            TcpState::SynSent => {
                ktrace!("[TCP process_segment] SynSent: flags={:02x}, seq={}, ack={}, snd_nxt={}, iss={}", flags, seq, ack, self.snd_nxt, self.iss);

                if flags & TCP_ACK != 0 {
                    // Check if ACK is valid (should acknowledge our SYN: iss + 1)
                    let expected_ack = self.iss.wrapping_add(1);
                    ktrace!(
                        "[TCP process_segment] ACK received: ack={}, expected_ack={}, match={}",
                        ack,
                        expected_ack,
                        ack == expected_ack
                    );

                    if ack == expected_ack {
                        self.snd_una = ack;
                        if flags & TCP_SYN != 0 {
                            self.remote_mac = src_mac;
                            self.irs = seq;
                            self.rcv_nxt = seq.wrapping_add(1);
                            self.snd_wnd = window;

                            // Process SYN-ACK options
                            if let Some(peer_mss) = options.mss {
                                self.peer_mss = peer_mss;
                                ktrace!("[TCP] Peer MSS: {}", peer_mss);
                            }
                            if options.sack_permitted && self.sack_permitted {
                                self.sack_permitted = true;
                            } else {
                                self.sack_permitted = false;
                            }
                            if let Some(scale) = options.window_scale {
                                self.peer_window_scale = scale;
                            }
                            if let Some((tsval, _)) = options.timestamp {
                                self.ts_recent = tsval;
                                self.use_timestamps = true;
                            }

                            self.state = TcpState::Established;
                            kdebug!(
                                "[TCP process_segment] Transition SynSent->Established for {}:{}",
                                self.remote_ip,
                                self.remote_port
                            );
                            self.send_segment(&[], TCP_ACK, tx)?;
                        }
                    } else {
                        ktrace!(
                            "[TCP process_segment] Invalid ACK in SynSent, expected {}, got {}",
                            expected_ack,
                            ack
                        );
                    }
                } else if flags & TCP_SYN != 0 {
                    // Simultaneous open
                    ktrace!("[TCP process_segment] Simultaneous open detected");
                    self.remote_mac = src_mac;
                    self.irs = seq;
                    self.rcv_nxt = seq.wrapping_add(1);
                    self.snd_wnd = window;

                    // Process SYN options for simultaneous open
                    if let Some(peer_mss) = options.mss {
                        self.peer_mss = peer_mss;
                    }
                    if options.sack_permitted {
                        self.sack_permitted = true;
                    }
                    if let Some(scale) = options.window_scale {
                        self.peer_window_scale = scale;
                    }

                    self.state = TcpState::SynReceived;
                    kdebug!(
                        "[TCP process_segment] Simultaneous open - SynReceived for {}:{}",
                        self.remote_ip,
                        self.remote_port
                    );
                    self.send_segment(&[], TCP_SYN | TCP_ACK, tx)?;
                }
            }
            TcpState::SynReceived => {
                // Complete the 3-way handshake on the server side when ACK is received
                if flags & TCP_ACK != 0 {
                    // Verify ACK number is correct (should ACK our SYN)
                    if ack == self.snd_nxt || ack == self.iss.wrapping_add(1) {
                        self.snd_una = ack;
                        // snd_nxt already advanced when we sent SYN|ACK, so do not increment again
                        self.snd_wnd = window;
                        self.state = TcpState::Established;
                        kdebug!(
                            "[TCP process_segment] Transition SynReceived->Established for {}:{}",
                            self.remote_ip,
                            self.remote_port
                        );
                    } else {
                        ktrace!(
                            "[TCP process_segment] SynReceived: Invalid ACK {}, expected {} or {}",
                            ack,
                            self.snd_nxt,
                            self.iss.wrapping_add(1)
                        );
                    }
                }
            }
            TcpState::Established | TcpState::FinWait1 | TcpState::FinWait2 => {
                // Update send window
                if flags & TCP_ACK != 0 {
                    self.process_ack(ack, window)?;
                }

                // Process payload
                if !payload.is_empty() {
                    crate::kinfo!("[TCP] Payload: len={}, seq={}, rcv_nxt={}, match={}", payload.len(), seq, self.rcv_nxt, seq == self.rcv_nxt);
                }

                if !payload.is_empty() && seq == self.rcv_nxt {
                    let space = RECV_BUFFER_SIZE - self.recv_buffer.len();
                    let to_recv = cmp::min(payload.len(), space);

                    // Compute simple checksum for debugging
                    let mut sum: u32 = 0;
                    for &b in &payload[..to_recv] {
                        sum = sum.wrapping_add(b as u32);
                    }
                    crate::kinfo!("[TCP] Recv: len={}, first4={:02x?}, sum={:#x}", 
                        to_recv, 
                        &payload[..4.min(to_recv)],
                        sum);

                    ktrace!(
                        "[TCP process_segment] Receiving {} bytes (space={}, payload_len={})",
                        to_recv,
                        space,
                        payload.len()
                    );

                    for &byte in &payload[..to_recv] {
                        self.recv_buffer.push_back(byte);
                    }

                    self.rcv_nxt = self.rcv_nxt.wrapping_add(to_recv as u32);
                    self.rcv_wnd = (RECV_BUFFER_SIZE - self.recv_buffer.len()) as u16;

                    ktrace!("[TCP process_segment] Received data, new rcv_nxt={}, recv_buffer_len={}, wait_queue_len={}", self.rcv_nxt, self.recv_buffer.len(), self.wait_queue.len());

                    self.send_segment(&[], TCP_ACK, tx)?;

                    // Wake up waiting processes
                    if !self.wait_queue.is_empty() {
                        ktrace!(
                            "[TCP process_segment] Waking {} waiting processes",
                            self.wait_queue.len()
                        );
                        for pid in self.wait_queue.drain(..) {
                            crate::scheduler::wake_process(pid);
                        }
                    }
                } else if !payload.is_empty() {
                    crate::kerror!(
                        "[TCP] Payload DROPPED: seq={} != rcv_nxt={}",
                        seq,
                        self.rcv_nxt
                    );
                }

                // Handle FIN
                if flags & TCP_FIN != 0 {
                    self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
                    self.send_segment(&[], TCP_ACK, tx)?;

                    match self.state {
                        TcpState::Established => {
                            self.state = TcpState::CloseWait;
                        }
                        TcpState::FinWait1 => {
                            if flags & TCP_ACK != 0 && ack == self.snd_nxt {
                                self.state = TcpState::TimeWait;
                            } else {
                                self.state = TcpState::Closing;
                            }
                        }
                        TcpState::FinWait2 => {
                            self.state = TcpState::TimeWait;
                        }
                        _ => {}
                    }
                }

                // Check FinWait1 -> FinWait2
                if self.state == TcpState::FinWait1 && flags & TCP_ACK != 0 && ack == self.snd_nxt {
                    self.state = TcpState::FinWait2;
                }
            }
            TcpState::CloseWait => {
                if flags & TCP_ACK != 0 {
                    self.process_ack(ack, window)?;
                }
            }
            TcpState::Closing => {
                if flags & TCP_ACK != 0 && ack == self.snd_nxt {
                    self.state = TcpState::TimeWait;
                }
            }
            TcpState::LastAck => {
                if flags & TCP_ACK != 0 && ack == self.snd_nxt {
                    self.reset();
                }
            }
            TcpState::TimeWait => {
                // Wait for 2*MSL before closing
                // For now, we'll just reset after a timeout
            }
            TcpState::Closed => {}
        }

        Ok(())
    }

    /// Poll for sending data and handling timeouts
    pub fn poll(&mut self, tx: &mut TxBatch) -> Result<(), NetError> {
        let now = self.current_time();

        // Handle state timeouts
        match self.state {
            TcpState::TimeWait => {
                if now - self.last_activity > 30000 {
                    // 30 seconds
                    self.reset();
                }
                return Ok(());
            }
            TcpState::SynSent | TcpState::SynReceived => {
                if self.last_activity != 0 && now - self.last_activity > 75000 {
                    // 75 seconds
                    ktrace!("[TCP poll] TIMEOUT reset state={:?}", self.state);
                    self.reset();
                    return Ok(());
                }
            }
            _ => {}
        }

        // Send SYN for initial connection
        if self.state == TcpState::SynSent {
            let elapsed = now.saturating_sub(self.last_activity);
            if elapsed >= self.rto || self.last_activity == 0 {
                ktrace!("[TCP] Sending SYN (elapsed={}ms)", elapsed);
                let result = self.send_segment(&[], TCP_SYN, tx);
                if let Err(e) = &result {
                    ktrace!("[TCP] SYN send failed: {:?}", e);
                }
                if result.is_ok() {
                    self.last_activity = now;
                }
            }
        }

        // Send pending data
        if (self.state == TcpState::Established || self.state == TcpState::CloseWait)
            && !self.send_buffer.is_empty()
        {
            self.send_pending_data(tx)?;
        }

        // Send FIN if needed
        if self.state == TcpState::FinWait1 || self.state == TcpState::LastAck {
            if self.send_buffer.is_empty() {
                self.send_segment(&[], TCP_FIN | TCP_ACK, tx)?;
            }
        }

        // Handle retransmissions
        self.check_retransmissions(tx)?;

        Ok(())
    }

    /// Send pending data from send buffer
    fn send_pending_data(&mut self, tx: &mut TxBatch) -> Result<(), NetError> {
        while !self.send_buffer.is_empty() {
            // Calculate available window (min of congestion window and receiver window)
            let flight_size = self.snd_nxt.wrapping_sub(self.snd_una);
            let cwnd_available = self.cwnd.saturating_sub(flight_size);
            let rwnd_available =
                (self.snd_una.wrapping_add(self.snd_wnd as u32)).wrapping_sub(self.snd_nxt);

            let window_available = cmp::min(cwnd_available, rwnd_available as u32) as usize;

            if window_available == 0 {
                ktrace!(
                    "[TCP] Send blocked - cwnd={}, flight={}, rwnd={}",
                    self.cwnd,
                    flight_size,
                    self.snd_wnd
                );
                break;
            }

            // Use negotiated peer MSS for segmentation
            let effective_mss = cmp::min(self.peer_mss as usize, MSS);
            let to_send = cmp::min(
                cmp::min(self.send_buffer.len(), effective_mss),
                window_available,
            );
            if to_send == 0 {
                break;
            }

            let mut data = Vec::with_capacity(to_send);
            for _ in 0..to_send {
                if let Some(byte) = self.send_buffer.pop_front() {
                    data.push(byte);
                }
            }

            self.send_segment(&data, TCP_ACK | TCP_PSH, tx)?;
        }

        Ok(())
    }

    /// Send a TCP segment
    fn send_segment_internal(
        &mut self,
        payload: &[u8],
        flags: u8,
        tx: &mut TxBatch,
        explicit_seq: Option<u32>,
        queue_for_retransmit: bool,
    ) -> Result<(), NetError> {
        if self.device_idx.is_none() {
            return Err(NetError::NoDevice);
        }

        // Check if we have remote MAC address
        if self.remote_mac.0 == [0, 0, 0, 0, 0, 0] {
            kerror!("[TCP send_segment] ERROR: remote_mac not set! Cannot send packet");
            return Err(NetError::NoDevice);
        }

        // Prepare TCP options for SYN packets
        let mut tcp_options = TcpOptions::new();
        let mut options_buffer = [0u8; 40]; // Max TCP options size
        let options_len = if flags & TCP_SYN != 0 {
            // Include MSS, SACK permitted, and window scale on SYN
            // NOTE: Temporarily disable timestamps and window scale for compatibility
            tcp_options.mss = Some(self.mss);
            tcp_options.sack_permitted = true;
            // tcp_options.window_scale = Some(0); // Disabled for now
            // if self.use_timestamps {
            //     tcp_options.timestamp = Some((self.current_time_ms(), 0));
            // }
            tcp_options.generate(&mut options_buffer)
        } else if self.use_timestamps && flags & TCP_ACK != 0 {
            // Include timestamp in ACK packets
            tcp_options.timestamp = Some((self.current_time_ms(), self.ts_recent));
            tcp_options.generate(&mut options_buffer)
        } else {
            0
        };

        let tcp_header_len = 20 + options_len;
        let ip_header_len = 20;
        let total_len = 14 + ip_header_len + tcp_header_len + payload.len();

        if total_len > MAX_FRAME_SIZE {
            return Err(NetError::BufferTooSmall);
        }

        let mut packet = Vec::with_capacity(MAX_FRAME_SIZE);
        packet.resize(MAX_FRAME_SIZE, 0);

        // Ethernet header
        packet[0..6].copy_from_slice(&self.remote_mac.0);
        packet[6..12].copy_from_slice(&self.local_mac.0);
        packet[12..14].copy_from_slice(&0x0800u16.to_be_bytes());

        // IP header
        packet[14] = 0x45; // Version 4, IHL 5
        packet[15] = 0; // DSCP/ECN
        let ip_total = (ip_header_len + tcp_header_len + payload.len()) as u16;
        packet[16..18].copy_from_slice(&ip_total.to_be_bytes());
        packet[18..20].copy_from_slice(&0u16.to_be_bytes()); // ID
        packet[20..22].copy_from_slice(&0x4000u16.to_be_bytes()); // Flags + Fragment
        packet[22] = 64; // TTL
        packet[23] = 6; // Protocol (TCP)
        packet[24..26].copy_from_slice(&[0, 0]); // Checksum (will calculate)
        packet[26..30].copy_from_slice(self.local_ip.as_bytes());
        packet[30..34].copy_from_slice(self.remote_ip.as_bytes());

        // Calculate IP checksum
        let ip_checksum = calculate_checksum(&packet[14..34]);
        packet[24..26].copy_from_slice(&ip_checksum.to_be_bytes());

        // TCP header
        let tcp_offset = 34;
        packet[tcp_offset..tcp_offset + 2].copy_from_slice(&self.local_port.to_be_bytes());
        packet[tcp_offset + 2..tcp_offset + 4].copy_from_slice(&self.remote_port.to_be_bytes());

        let seq = explicit_seq.unwrap_or_else(|| {
            if flags & TCP_SYN != 0 && self.state == TcpState::SynSent {
                self.iss
            } else {
                self.snd_nxt
            }
        });
        packet[tcp_offset + 4..tcp_offset + 8].copy_from_slice(&seq.to_be_bytes());
        packet[tcp_offset + 8..tcp_offset + 12].copy_from_slice(&self.rcv_nxt.to_be_bytes());

        packet[tcp_offset + 12] = ((tcp_header_len / 4) as u8) << 4;
        packet[tcp_offset + 13] = flags;
        packet[tcp_offset + 14..tcp_offset + 16].copy_from_slice(&self.rcv_wnd.to_be_bytes());
        packet[tcp_offset + 16..tcp_offset + 18].copy_from_slice(&[0, 0]); // Checksum (zero before calculation)
        packet[tcp_offset + 18..tcp_offset + 20].copy_from_slice(&[0, 0]); // Urgent pointer

        // Copy options
        if options_len > 0 {
            packet[tcp_offset + 20..tcp_offset + 20 + options_len]
                .copy_from_slice(&options_buffer[..options_len]);
        }

        // Payload
        packet[tcp_offset + tcp_header_len..tcp_offset + tcp_header_len + payload.len()]
            .copy_from_slice(payload);

        // TCP checksum
        let tcp_checksum = calculate_tcp_checksum(
            self.local_ip.as_bytes(),
            self.remote_ip.as_bytes(),
            &packet[tcp_offset..tcp_offset + tcp_header_len + payload.len()],
        );
        packet[tcp_offset + 16..tcp_offset + 18].copy_from_slice(&tcp_checksum.to_be_bytes());

        ktrace!("[TCP send_segment] Sending: flags={:02x}, seq={}, ack={}, len={}, queue_for_retransmit={}, {}:{} -> {}:{}", 
            flags, seq, self.rcv_nxt, total_len, queue_for_retransmit,
            self.local_ip, self.local_port, self.remote_ip, self.remote_port);
        ktrace!(
            "[TCP send_segment] remote_mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.remote_mac.0[0],
            self.remote_mac.0[1],
            self.remote_mac.0[2],
            self.remote_mac.0[3],
            self.remote_mac.0[4],
            self.remote_mac.0[5]
        );

        // Dump TCP packet for debugging (first 128 bytes or entire packet)
        let dump_len = core::cmp::min(total_len, 128);
        ktrace!("[TCP send_segment] Packet dump ({} bytes):", dump_len);
        for i in (0..dump_len).step_by(16) {
            let mut line = alloc::format!("  {:04x}: ", i);
            for j in 0..16 {
                if i + j < dump_len {
                    line.push_str(&alloc::format!("{:02x} ", packet[i + j]));
                } else {
                    line.push_str("   ");
                }
            }
            ktrace!("{}", line);
        }

        tx.push(&packet[..total_len])?;

        // Update sequence number
        let mut seq_advance = payload.len() as u32;
        if flags & (TCP_SYN | TCP_FIN) != 0 {
            seq_advance = seq_advance.wrapping_add(1);
        }

        if queue_for_retransmit
            && seq_advance > 0
            && (flags & TCP_SYN != 0 || !payload.is_empty() || flags & TCP_FIN != 0)
        {
            // Add to retransmit queue
            let segment = TcpSegment {
                seq,
                data: payload.to_vec(),
                flags,
                timestamp: self.current_time(),
                retransmit_count: 0,
                rtt_measured: false,
            };

            // Start RTT measurement if not already measuring
            if self.rtt_seq.is_none() && !payload.is_empty() {
                self.rtt_seq = Some(seq);
                self.rtt_time = self.current_time();
            }

            self.retransmit_queue.push(segment);
            // Advance snd_nxt only when queuing a new segment
            self.snd_nxt = self.snd_nxt.wrapping_add(seq_advance);
        }

        self.last_activity = self.current_time();

        Ok(())
    }

    fn send_segment(
        &mut self,
        payload: &[u8],
        flags: u8,
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        self.send_segment_internal(payload, flags, tx, None, true)
    }

    /// Process ACK and update send window
    fn process_ack(&mut self, ack: u32, window: u16) -> Result<(), NetError> {
        if ack.wrapping_sub(self.snd_una) <= self.snd_nxt.wrapping_sub(self.snd_una) {
            let newly_acked = ack.wrapping_sub(self.snd_una);

            // Detect duplicate ACK
            if ack == self.last_ack && newly_acked == 0 && !self.send_buffer.is_empty() {
                self.dup_acks += 1;
                ktrace!(
                    "[TCP Congestion] Duplicate ACK #{} for seq {}",
                    self.dup_acks,
                    ack
                );

                // Fast retransmit on 3 duplicate ACKs (TCP Reno)
                if self.dup_acks == 3 {
                    kdebug!("[TCP Congestion] Fast retransmit triggered");

                    // Enter fast recovery
                    if !self.in_recovery {
                        self.ssthresh = cmp::max(self.cwnd / 2, 2 * self.mss as u32);
                        self.cwnd = self.ssthresh + 3 * self.mss as u32;
                        self.in_recovery = true;
                        self.recovery_point = self.snd_nxt;

                        kdebug!(
                            "[TCP Congestion] Enter fast recovery - ssthresh={}, cwnd={}",
                            self.ssthresh,
                            self.cwnd
                        );

                        // NOTE: Actual retransmission will be handled in check_retransmissions()
                        // by the dedicated fast retransmit logic there
                    }
                } else if self.dup_acks > 3 && self.in_recovery {
                    // Inflate cwnd for each additional duplicate ACK
                    self.cwnd += self.mss as u32;
                    ktrace!(
                        "[TCP Congestion] Fast recovery - inflate cwnd to {}",
                        self.cwnd
                    );
                }
            } else if newly_acked > 0 {
                // New data acknowledged
                self.dup_acks = 0;
                self.last_ack = ack;

                // Valid ACK
                self.snd_una = ack;
                self.snd_wnd = window;

                // Update congestion window
                if self.in_recovery {
                    // Fast recovery: deflate cwnd
                    if ack.wrapping_sub(self.recovery_point) > 0 {
                        // Exit recovery
                        self.in_recovery = false;
                        self.cwnd = self.ssthresh;
                        kdebug!(
                            "[TCP Congestion] Exit recovery, cwnd={}, ssthresh={}",
                            self.cwnd,
                            self.ssthresh
                        );
                    }
                } else if self.cwnd < self.ssthresh {
                    // Slow start: exponential growth
                    self.cwnd += newly_acked;
                    ktrace!(
                        "[TCP Congestion] Slow start, cwnd={} (+{}), ssthresh={}",
                        self.cwnd,
                        newly_acked,
                        self.ssthresh
                    );
                } else {
                    // Congestion avoidance: linear growth
                    // Increase cwnd by MSS * (MSS / cwnd) for each ACK
                    let increment = (self.mss as u32 * newly_acked) / self.cwnd;
                    self.cwnd += cmp::max(increment, 1);
                    ktrace!(
                        "[TCP Congestion] Congestion avoidance, cwnd={} (+{}), ssthresh={}",
                        self.cwnd,
                        increment,
                        self.ssthresh
                    );
                }

                // RTT measurement (Karn's algorithm)
                if let Some(rtt_seq) = self.rtt_seq {
                    // Check if the ACK covers our RTT measurement sequence
                    if ack.wrapping_sub(rtt_seq) > 0 && ack.wrapping_sub(rtt_seq) <= newly_acked {
                        // Measure RTT
                        let now = self.current_time();
                        let rtt_sample = now.saturating_sub(self.rtt_time);

                        // Only update if this wasn't a retransmission
                        if let Some(seg) = self.retransmit_queue.iter().find(|s| s.seq == rtt_seq) {
                            if !seg.rtt_measured && seg.retransmit_count == 0 {
                                self.update_rtt(rtt_sample);
                            }
                        }

                        // Clear RTT measurement
                        self.rtt_seq = None;
                    }
                }

                // Remove acknowledged segments from retransmit queue
                self.retransmit_queue.retain(|seg| {
                    let seg_end = seg.seq.wrapping_add(seg.data.len() as u32);
                    let seg_end = if seg.flags & (TCP_SYN | TCP_FIN) != 0 {
                        seg_end.wrapping_add(1)
                    } else {
                        seg_end
                    };
                    // Keep segment if its end sequence is after the ACK
                    // Using wrapping arithmetic: if (ack - seg_end) wraps to a large number,
                    // seg_end is after ack, so we keep it
                    ack.wrapping_sub(seg_end) > (1u32 << 31)
                });
            }
        }

        Ok(())
    }

    /// Update RTT estimation using RFC 6298 algorithm
    fn update_rtt(&mut self, rtt_sample: u64) {
        let rtt_ms = rtt_sample as i64;

        if self.srtt == 0 {
            // First RTT measurement
            self.srtt = rtt_ms;
            self.rttvar = rtt_ms / 2;
            self.rto = cmp::max((self.srtt + cmp::max(G, K * self.rttvar)) as u64, MIN_RTO);
        } else {
            // Subsequent measurements
            // RTTVAR = (1 - beta) * RTTVAR + beta * |SRTT - R'|
            let abs_diff = (self.srtt - rtt_ms).abs();
            self.rttvar = ((1000 - BETA) * self.rttvar + BETA * abs_diff) / 1000;

            // SRTT = (1 - alpha) * SRTT + alpha * R'
            self.srtt = ((1000 - ALPHA) * self.srtt + ALPHA * rtt_ms) / 1000;

            // RTO = SRTT + max(G, K * RTTVAR)
            self.rto = cmp::max((self.srtt + cmp::max(G, K * self.rttvar)) as u64, MIN_RTO);
        }

        // Cap RTO at maximum
        self.rto = cmp::min(self.rto, MAX_RTO);

        ktrace!(
            "[TCP RTT] sample={}ms, SRTT={}ms, RTTVAR={}ms, RTO={}ms",
            rtt_ms,
            self.srtt,
            self.rttvar,
            self.rto
        );
    }

    /// Check and handle retransmissions
    fn check_retransmissions(&mut self, tx: &mut TxBatch) -> Result<(), NetError> {
        let now = self.current_time();
        let mut to_retransmit = Vec::new();
        let mut timeout_occurred = false;

        // Handle fast retransmit when just entering recovery
        if self.dup_acks >= 3 && self.in_recovery {
            // Fast retransmit: retransmit the first unacknowledged segment
            // Only do this once when dup_acks == 3 to avoid duplicate retransmissions
            if self.dup_acks == 3 {
                if let Some(segment) = self.retransmit_queue.first() {
                    if segment.seq == self.snd_una {
                        kdebug!("[TCP Fast Retransmit] Retransmitting seq={}", segment.seq);
                        let seg_clone = segment.clone();
                        self.send_segment_internal(
                            &seg_clone.data,
                            seg_clone.flags,
                            tx,
                            Some(seg_clone.seq),
                            false,
                        )?;
                    }
                }
            }
        }

        for segment in &mut self.retransmit_queue {
            if now - segment.timestamp > self.rto {
                if segment.retransmit_count >= MAX_RETRANSMIT {
                    // Give up, reset connection
                    kerror!("[TCP] Max retransmit attempts reached, resetting connection");
                    self.reset();
                    return Ok(());
                }

                to_retransmit.push(segment.clone());
                segment.timestamp = now;
                segment.retransmit_count += 1;
                segment.rtt_measured = true; // Mark as retransmitted (Karn's algorithm)

                if !timeout_occurred {
                    timeout_occurred = true;

                    // Timeout: Enter slow start
                    self.ssthresh = cmp::max(self.cwnd / 2, 2 * self.mss as u32);
                    self.cwnd = self.mss as u32; // Reset to 1 MSS
                    self.dup_acks = 0;
                    self.in_recovery = false;

                    kdebug!(
                        "[TCP Congestion] Timeout - ssthresh={}, cwnd={}",
                        self.ssthresh,
                        self.cwnd
                    );
                }

                // Exponential backoff
                self.rto = cmp::min(self.rto * 2, MAX_RTO);

                kdebug!(
                    "[TCP] Retransmitting segment seq={}, count={}, new RTO={}ms",
                    segment.seq,
                    segment.retransmit_count,
                    self.rto
                );
            }
        }

        for segment in to_retransmit {
            // Re-send without adding another retransmit queue entry and using original seq
            self.send_segment_internal(&segment.data, segment.flags, tx, Some(segment.seq), false)?;
        }

        Ok(())
    }

    /// Generate initial sequence number
    fn generate_isn(&self) -> u32 {
        (logger::boot_time_us() as u32) ^ 0x13579BDF
    }

    /// Get current time in milliseconds
    fn current_time(&self) -> u64 {
        logger::boot_time_us() / 1000
    }

    /// Get current time as milliseconds u32 for TCP timestamps
    fn current_time_ms(&self) -> u32 {
        (logger::boot_time_us() / 1000) as u32
    }

    /// Reset socket to closed state
    pub fn reset(&mut self) {
        self.state = TcpState::Closed;
        self.remote_ip = Ipv4Address::UNSPECIFIED;
        self.remote_port = 0;
        self.remote_mac = MacAddress([0; 6]);
        self.snd_una = 0;
        self.snd_nxt = 0;
        self.snd_wnd = 0;
        self.rcv_nxt = 0;
        self.rcv_wnd = RECV_WINDOW as u16;
        self.send_buffer.clear();
        self.recv_buffer.clear();
        self.retransmit_queue.clear();
        self.rto = INITIAL_RTO;
        self.in_use = false;
    }

    /// Check if socket can accept more data
    pub fn can_send(&self) -> bool {
        self.send_buffer.len() < SEND_BUFFER_SIZE
    }

    /// Check if socket has data to read
    pub fn has_data(&self) -> bool {
        !self.recv_buffer.is_empty()
    }

    /// Get number of bytes available to read
    pub fn available(&self) -> usize {
        self.recv_buffer.len()
    }
}

/// Calculate IP/TCP checksum
fn calculate_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;

    while i < data.len() {
        if i + 1 < data.len() {
            let word = u16::from_be_bytes([data[i], data[i + 1]]);
            sum = sum.wrapping_add(word as u32);
        } else {
            sum = sum.wrapping_add((data[i] as u32) << 8);
        }
        i += 2;
    }

    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    !sum as u16
}

/// Calculate TCP checksum with pseudo-header
fn calculate_tcp_checksum(src_ip: &[u8], dst_ip: &[u8], tcp_segment: &[u8]) -> u16 {
    let mut pseudo = [0u8; 12];
    pseudo[0..4].copy_from_slice(src_ip);
    pseudo[4..8].copy_from_slice(dst_ip);
    pseudo[8] = 0;
    pseudo[9] = 6; // TCP protocol
    pseudo[10..12].copy_from_slice(&(tcp_segment.len() as u16).to_be_bytes());

    let mut sum: u32 = 0;

    // Pseudo-header
    for i in (0..12).step_by(2) {
        let word = u16::from_be_bytes([pseudo[i], pseudo[i + 1]]);
        sum = sum.wrapping_add(word as u32);
    }

    // TCP segment
    let mut i = 0;
    while i < tcp_segment.len() {
        if i + 1 < tcp_segment.len() {
            let word = u16::from_be_bytes([tcp_segment[i], tcp_segment[i + 1]]);
            sum = sum.wrapping_add(word as u32);
        } else {
            sum = sum.wrapping_add((tcp_segment[i] as u32) << 8);
        }
        i += 2;
    }

    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    !sum as u16
}
