/// TCP protocol implementation
///
/// This module provides a complete TCP stack including connection management,
/// reliable data transfer, flow control, and retransmission.

use crate::logger;
use crate::serial;
use super::ethernet::MacAddress;
use super::ipv4::Ipv4Address;
use super::stack::{TxBatch, MAX_FRAME_SIZE};
use super::drivers::NetError;
use crate::process::Pid;
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
/// Maximum retransmit attempts
const MAX_RETRANSMIT: u8 = 12;

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
    snd_una: u32,  // Send unacknowledged
    snd_nxt: u32,  // Send next
    snd_wnd: u16,  // Send window
    rcv_nxt: u32,  // Receive next
    rcv_wnd: u16,  // Receive window
    iss: u32,      // Initial send sequence number
    irs: u32,      // Initial receive sequence number
    
    // Buffers
    send_buffer: VecDeque<u8>,
    recv_buffer: VecDeque<u8>,
    retransmit_queue: Vec<TcpSegment>,
    
    // Timers and RTT estimation
    rto: u64,          // Retransmission timeout
    #[allow(dead_code)]
    srtt: i64,         // Smoothed RTT
    #[allow(dead_code)]
    rttvar: i64,       // RTT variance
    last_activity: u64, // Last activity timestamp
    
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
            send_buffer: VecDeque::new(),
            recv_buffer: VecDeque::new(),
            retransmit_queue: Vec::new(),
            rto: INITIAL_RTO,
            srtt: 0,
            rttvar: 0,
            last_activity: 0,
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
            if self.state == TcpState::Closed {
                return Err(NetError::ConnectionClosed);
            }
            return Err(NetError::WouldBlock);
        }

        let to_recv = cmp::min(buffer.len(), self.recv_buffer.len());
        for i in 0..to_recv {
            buffer[i] = self.recv_buffer.pop_front().unwrap();
        }

        // Update receive window
        self.rcv_wnd = (RECV_BUFFER_SIZE - self.recv_buffer.len()) as u16;

        Ok(to_recv)
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

        // Verify this segment is for us
        if dst_port != self.local_port {
            return Ok(());
        }

        if self.state != TcpState::Listen && src_port != self.remote_port {
            return Ok(());
        }

        if self.state != TcpState::Listen && src_ip != self.remote_ip {
            return Ok(());
        }

        self.last_activity = self.current_time();
        
        // Extract payload
        let payload = if data_offset < tcp_data.len() {
            &tcp_data[data_offset..]
        } else {
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
                    self.state = TcpState::SynReceived;
                    self.send_segment(&[], TCP_SYN | TCP_ACK, tx)?;
                }
            }
            TcpState::SynSent => {
                if flags & TCP_ACK != 0 && ack == self.snd_nxt.wrapping_add(1) {
                    self.snd_una = ack;
                    if flags & TCP_SYN != 0 {
                        self.remote_mac = src_mac;
                        self.irs = seq;
                        self.rcv_nxt = seq.wrapping_add(1);
                        self.snd_wnd = window;
                        self.snd_nxt = self.snd_nxt.wrapping_add(1);
                        self.state = TcpState::Established;
                        self.send_segment(&[], TCP_ACK, tx)?;
                    }
                } else if flags & TCP_SYN != 0 {
                    // Simultaneous open
                    self.remote_mac = src_mac;
                    self.irs = seq;
                    self.rcv_nxt = seq.wrapping_add(1);
                    self.snd_wnd = window;
                    self.state = TcpState::SynReceived;
                    self.send_segment(&[], TCP_SYN | TCP_ACK, tx)?;
                }
            }
            TcpState::SynReceived => {
                if flags & TCP_ACK != 0 && ack == self.snd_nxt.wrapping_add(1) {
                    self.snd_una = ack;
                    self.snd_nxt = self.snd_nxt.wrapping_add(1);
                    self.snd_wnd = window;
                    self.state = TcpState::Established;
                }
            }
            TcpState::Established | TcpState::FinWait1 | TcpState::FinWait2 => {
                // Update send window
                if flags & TCP_ACK != 0 {
                    self.process_ack(ack, window);
                }

                // Process payload
                if !payload.is_empty() && seq == self.rcv_nxt {
                    let space = RECV_BUFFER_SIZE - self.recv_buffer.len();
                    let to_recv = cmp::min(payload.len(), space);
                    
                    for &byte in &payload[..to_recv] {
                        self.recv_buffer.push_back(byte);
                    }
                    
                    self.rcv_nxt = self.rcv_nxt.wrapping_add(to_recv as u32);
                    self.rcv_wnd = (RECV_BUFFER_SIZE - self.recv_buffer.len()) as u16;
                    self.send_segment(&[], TCP_ACK, tx)?;

                    // Wake up waiting processes
                    if !self.wait_queue.is_empty() {
                        for pid in self.wait_queue.drain(..) {
                            crate::scheduler::wake_process(pid);
                        }
                    }
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
                    self.process_ack(ack, window);
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

        if self.state == TcpState::SynSent {
             serial::_print(format_args!("[TCP poll] ENTRY SynSent last_activity={} now={}\n", self.last_activity, now));
        }

        // Handle state timeouts
        match self.state {
            TcpState::TimeWait => {
                if now - self.last_activity > 30000 { // 30 seconds
                    self.reset();
                }
                return Ok(());
            }
            TcpState::SynSent | TcpState::SynReceived => {
                if self.last_activity != 0 && now - self.last_activity > 75000 { // 75 seconds
                    serial::_print(format_args!("[TCP poll] TIMEOUT reset state={:?}\n", self.state));
                    self.reset();
                    return Ok(());
                }
            }
            _ => {}
        }

        // Send SYN for initial connection
        if self.state == TcpState::SynSent {
            let elapsed = now.saturating_sub(self.last_activity);
            serial::_print(format_args!(
                "[TCP poll] SynSent state: now={}, last_activity={}, elapsed={}, rto={}\n",
                now, self.last_activity, elapsed, self.rto
            ));
            if elapsed >= self.rto || self.last_activity == 0 {
                serial::_print(format_args!("[TCP poll] Sending SYN packet\n"));
                let result = self.send_segment(&[], TCP_SYN, tx);
                serial::_print(format_args!("[TCP poll] send_segment result: {:?}\n", result));
                if result.is_ok() {
                    self.last_activity = now;
                }
            }
        }

        // Send pending data
        if (self.state == TcpState::Established || self.state == TcpState::CloseWait) 
            && !self.send_buffer.is_empty() {
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
            // Check window
            let window_available = (self.snd_una.wrapping_add(self.snd_wnd as u32))
                .wrapping_sub(self.snd_nxt) as usize;
            
            if window_available == 0 {
                break;
            }

            let to_send = cmp::min(cmp::min(self.send_buffer.len(), MSS), window_available);
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
    fn send_segment(&mut self, payload: &[u8], flags: u8, tx: &mut TxBatch) -> Result<(), NetError> {
        if self.device_idx.is_none() {
            return Err(NetError::NoDevice);
        }

        // Check if we have remote MAC address
        if self.remote_mac.0 == [0, 0, 0, 0, 0, 0] {
            serial::_print(format_args!(
                "[TCP send_segment] ERROR: remote_mac not set! Cannot send packet\n"
            ));
            return Err(NetError::NoDevice);
        }

        let tcp_header_len = 20;
        let ip_header_len = 20;
        let total_len = 14 + ip_header_len + tcp_header_len + payload.len();

        if total_len > MAX_FRAME_SIZE {
            return Err(NetError::BufferTooSmall);
        }

        let mut packet = [0u8; MAX_FRAME_SIZE];

        // Ethernet header
        packet[0..6].copy_from_slice(&self.remote_mac.0);
        packet[6..12].copy_from_slice(&self.local_mac.0);
        packet[12..14].copy_from_slice(&0x0800u16.to_be_bytes());

        // IP header
        packet[14] = 0x45; // Version 4, IHL 5
        packet[15] = 0;    // DSCP/ECN
        let ip_total = (ip_header_len + tcp_header_len + payload.len()) as u16;
        packet[16..18].copy_from_slice(&ip_total.to_be_bytes());
        packet[18..20].copy_from_slice(&0u16.to_be_bytes()); // ID
        packet[20..22].copy_from_slice(&0x4000u16.to_be_bytes()); // Flags + Fragment
        packet[22] = 64;   // TTL
        packet[23] = 6;    // Protocol (TCP)
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
        
        let seq = if flags & TCP_SYN != 0 && self.state == TcpState::SynSent {
            self.iss
        } else {
            self.snd_nxt
        };
        packet[tcp_offset + 4..tcp_offset + 8].copy_from_slice(&seq.to_be_bytes());
        packet[tcp_offset + 8..tcp_offset + 12].copy_from_slice(&self.rcv_nxt.to_be_bytes());
        
        packet[tcp_offset + 12] = ((tcp_header_len / 4) as u8) << 4;
        packet[tcp_offset + 13] = flags;
        packet[tcp_offset + 14..tcp_offset + 16].copy_from_slice(&self.rcv_wnd.to_be_bytes());
        packet[tcp_offset + 16..tcp_offset + 18].copy_from_slice(&[0, 0]); // Checksum
        packet[tcp_offset + 18..tcp_offset + 20].copy_from_slice(&[0, 0]); // Urgent pointer

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

        serial::_print(format_args!(
            "[TCP send_segment] Sending: flags={:02x}, seq={}, ack={}, len={}, {}:{} -> {}:{}\n",
            flags, seq, self.rcv_nxt, total_len,
            self.local_ip, self.local_port, self.remote_ip, self.remote_port
        ));
        serial::_print(format_args!(
            "[TCP send_segment] remote_mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            self.remote_mac.0[0], self.remote_mac.0[1], self.remote_mac.0[2],
            self.remote_mac.0[3], self.remote_mac.0[4], self.remote_mac.0[5]
        ));

        tx.push(&packet[..total_len])?;

        // Update sequence number
        let mut seq_advance = payload.len() as u32;
        if flags & (TCP_SYN | TCP_FIN) != 0 {
            seq_advance = seq_advance.wrapping_add(1);
        }

        if seq_advance > 0 && (flags & TCP_SYN != 0 || !payload.is_empty() || flags & TCP_FIN != 0) {
            // Add to retransmit queue
            let segment = TcpSegment {
                seq,
                data: payload.to_vec(),
                flags,
                timestamp: self.current_time(),
                retransmit_count: 0,
            };
            self.retransmit_queue.push(segment);
            
            self.snd_nxt = self.snd_nxt.wrapping_add(seq_advance);
        }

        self.last_activity = self.current_time();

        Ok(())
    }

    /// Process ACK and update send window
    fn process_ack(&mut self, ack: u32, window: u16) {
        if ack.wrapping_sub(self.snd_una) <= self.snd_nxt.wrapping_sub(self.snd_una) {
            // Valid ACK
            self.snd_una = ack;
            self.snd_wnd = window;

            // Remove acknowledged segments from retransmit queue
            self.retransmit_queue.retain(|seg| {
                let seg_end = seg.seq.wrapping_add(seg.data.len() as u32);
                seg_end.wrapping_sub(ack) > 0
            });
        }
    }

    /// Check and handle retransmissions
    fn check_retransmissions(&mut self, tx: &mut TxBatch) -> Result<(), NetError> {
        let now = self.current_time();
        let mut to_retransmit = Vec::new();

        for segment in &mut self.retransmit_queue {
            if now - segment.timestamp > self.rto {
                if segment.retransmit_count >= MAX_RETRANSMIT {
                    // Give up, reset connection
                    self.reset();
                    return Ok(());
                }

                to_retransmit.push(segment.clone());
                segment.timestamp = now;
                segment.retransmit_count += 1;
                
                // Exponential backoff
                self.rto = cmp::min(self.rto * 2, MAX_RTO);
            }
        }

        for segment in to_retransmit {
            self.send_segment(&segment.data, segment.flags, tx)?;
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
