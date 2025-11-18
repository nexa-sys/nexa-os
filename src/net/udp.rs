/// UDP (User Datagram Protocol) implementation
///
/// This module provides structures and utilities for parsing, constructing,
/// and handling UDP datagrams, with support for:
/// - Standard UDP header parsing and construction
/// - Checksum calculation and verification
/// - UDP socket options (TTL, ToS, broadcast)
/// - Fragmentation handling

use core::mem;

use super::ipv4::{Ipv4Address, Ipv4Header};

/// UDP port number
pub type Port = u16;

/// UDP socket options
#[derive(Clone, Copy, Debug)]
pub struct UdpSocketOptions {
    /// Time To Live (TTL) - default 64
    pub ttl: u8,
    /// Type of Service (ToS) - QoS marking
    pub tos: u8,
    /// Allow broadcasting
    pub broadcast: bool,
    /// UDP buffer size in bytes
    pub buffer_size: u16,
}

impl UdpSocketOptions {
    /// Create socket options with default values
    pub fn default() -> Self {
        Self {
            ttl: 64,
            tos: 0,
            broadcast: false,
            buffer_size: 65535,
        }
    }

    /// Enable broadcast for this socket
    pub fn with_broadcast(mut self) -> Self {
        self.broadcast = true;
        self
    }

    /// Set custom TTL
    pub fn with_ttl(mut self, ttl: u8) -> Self {
        self.ttl = ttl;
        self
    }

    /// Set Type of Service
    pub fn with_tos(mut self, tos: u8) -> Self {
        self.tos = tos;
        self
    }
}

/// UDP header (8 bytes)
#[repr(C, packed)]
pub struct UdpHeader {
    pub src_port: u16,      // Source port (network byte order)
    pub dst_port: u16,      // Destination port (network byte order)
    pub length: u16,        // Length of UDP header + data (network byte order)
    pub checksum: u16,      // Checksum (network byte order, optional for IPv4)
}

impl UdpHeader {
    pub const SIZE: usize = 8;

    /// Create a new UDP header
    pub fn new(src_port: Port, dst_port: Port, data_len: usize) -> Self {
        Self {
            src_port: src_port.to_be(),
            dst_port: dst_port.to_be(),
            length: ((Self::SIZE + data_len) as u16).to_be(),
            checksum: 0, // Will be calculated later
        }
    }

    /// Get source port in host byte order
    pub fn src_port(&self) -> Port {
        u16::from_be(self.src_port)
    }

    /// Get destination port in host byte order
    pub fn dst_port(&self) -> Port {
        u16::from_be(self.dst_port)
    }

    /// Get length in host byte order
    pub fn length(&self) -> u16 {
        u16::from_be(self.length)
    }

    /// Get checksum in host byte order
    pub fn checksum(&self) -> u16 {
        u16::from_be(self.checksum)
    }

    /// Check if this is a valid UDP datagram length
    pub fn is_valid_length(&self) -> bool {
        self.length() >= Self::SIZE as u16
    }

    /// Get payload size from header length
    pub fn payload_size(&self) -> Option<usize> {
        let total = self.length() as usize;
        if total >= Self::SIZE {
            Some(total - Self::SIZE)
        } else {
            None
        }
    }

    /// Calculate and set UDP checksum (including IPv4 pseudo-header)
    pub fn calculate_checksum(
        &mut self,
        src_ip: &Ipv4Address,
        dst_ip: &Ipv4Address,
        payload: &[u8],
    ) {
        self.checksum = 0;

        let mut sum: u32 = 0;

        // IPv4 pseudo-header
        // Source IP
        for i in (0..4).step_by(2) {
            let word = ((src_ip.0[i] as u16) << 8) | (src_ip.0[i + 1] as u16);
            sum += word as u32;
        }
        // Destination IP
        for i in (0..4).step_by(2) {
            let word = ((dst_ip.0[i] as u16) << 8) | (dst_ip.0[i + 1] as u16);
            sum += word as u32;
        }
        // Protocol (UDP = 17)
        sum += 17u32;
        // UDP length
        sum += self.length() as u32;

        // UDP header
        let header_bytes = unsafe {
            core::slice::from_raw_parts(
                self as *const UdpHeader as *const u8,
                mem::size_of::<UdpHeader>(),
            )
        };
        for chunk in header_bytes.chunks(2) {
            let word = if chunk.len() == 2 {
                ((chunk[0] as u16) << 8) | (chunk[1] as u16)
            } else {
                (chunk[0] as u16) << 8
            };
            sum += word as u32;
        }

        // UDP payload
        for chunk in payload.chunks(2) {
            let word = if chunk.len() == 2 {
                ((chunk[0] as u16) << 8) | (chunk[1] as u16)
            } else {
                (chunk[0] as u16) << 8
            };
            sum += word as u32;
        }

        // Fold 32-bit sum to 16 bits
        while sum > 0xFFFF {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        // One's complement
        let checksum = !sum as u16;
        self.checksum = if checksum == 0 { 0xFFFF } else { checksum }.to_be();
    }

    /// Verify UDP checksum
    pub fn verify_checksum(
        &self,
        src_ip: &Ipv4Address,
        dst_ip: &Ipv4Address,
        payload: &[u8],
    ) -> bool {
        // Checksum is optional in IPv4 (0 means no checksum)
        if self.checksum == 0 {
            return true;
        }

        let mut sum: u32 = 0;

        // IPv4 pseudo-header
        for i in (0..4).step_by(2) {
            let word = ((src_ip.0[i] as u16) << 8) | (src_ip.0[i + 1] as u16);
            sum += word as u32;
        }
        for i in (0..4).step_by(2) {
            let word = ((dst_ip.0[i] as u16) << 8) | (dst_ip.0[i + 1] as u16);
            sum += word as u32;
        }
        sum += 17u32;
        sum += self.length() as u32;

        // UDP header (including checksum field)
        let header_bytes = unsafe {
            core::slice::from_raw_parts(
                self as *const UdpHeader as *const u8,
                mem::size_of::<UdpHeader>(),
            )
        };
        for chunk in header_bytes.chunks(2) {
            let word = if chunk.len() == 2 {
                ((chunk[0] as u16) << 8) | (chunk[1] as u16)
            } else {
                (chunk[0] as u16) << 8
            };
            sum += word as u32;
        }

        // UDP payload
        for chunk in payload.chunks(2) {
            let word = if chunk.len() == 2 {
                ((chunk[0] as u16) << 8) | (chunk[1] as u16)
            } else {
                (chunk[0] as u16) << 8
            };
            sum += word as u32;
        }

        while sum > 0xFFFF {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        sum == 0xFFFF
    }
}

/// UDP datagram
pub struct UdpDatagram<'a> {
    buffer: &'a [u8],
}

impl<'a> UdpDatagram<'a> {
    /// Parse a UDP datagram from a buffer
    pub fn parse(buffer: &'a [u8]) -> Option<Self> {
        if buffer.len() < UdpHeader::SIZE {
            return None;
        }
        Some(Self { buffer })
    }

    /// Get the UDP header
    pub fn header(&self) -> &UdpHeader {
        unsafe { &*(self.buffer.as_ptr() as *const UdpHeader) }
    }

    /// Get the UDP payload
    pub fn payload(&self) -> &[u8] {
        let data_len = self.header().length() as usize - UdpHeader::SIZE;
        &self.buffer[UdpHeader::SIZE..UdpHeader::SIZE + data_len]
    }

    /// Get source port
    pub fn src_port(&self) -> Port {
        self.header().src_port()
    }

    /// Get destination port
    pub fn dst_port(&self) -> Port {
        self.header().dst_port()
    }

    /// Check if payload matches expected size
    pub fn validate_length(&self) -> bool {
        let header_len = self.header().length() as usize;
        header_len >= UdpHeader::SIZE && self.buffer.len() >= header_len
    }

    /// Get total datagram size
    pub fn total_size(&self) -> usize {
        self.header().length() as usize
    }

    /// Check if datagram has valid checksum (if present)
    pub fn has_valid_checksum(&self, src_ip: &Ipv4Address, dst_ip: &Ipv4Address) -> bool {
        self.header().verify_checksum(src_ip, dst_ip, self.payload())
    }
}

/// Mutable UDP datagram for construction
pub struct UdpDatagramMut<'a> {
    buffer: &'a mut [u8],
    data_len: usize,
}

impl<'a> UdpDatagramMut<'a> {
    /// Create a new UDP datagram in the buffer
    pub fn new(buffer: &'a mut [u8], src_port: Port, dst_port: Port, data_len: usize) -> Option<Self> {
        let total_len = UdpHeader::SIZE + data_len;
        if buffer.len() < total_len {
            return None;
        }

        let header = UdpHeader::new(src_port, dst_port, data_len);
        unsafe {
            core::ptr::write(buffer.as_mut_ptr() as *mut UdpHeader, header);
        }

        Some(Self { buffer, data_len })
    }

    /// Create from an existing immutable datagram
    pub fn from_existing(buffer: &'a mut [u8]) -> Option<Self> {
        if buffer.len() < UdpHeader::SIZE {
            return None;
        }
        let data_len = unsafe {
            let header = &*(buffer.as_ptr() as *const UdpHeader);
            header.length() as usize - UdpHeader::SIZE
        };
        if buffer.len() < UdpHeader::SIZE + data_len {
            return None;
        }
        Some(Self { buffer, data_len })
    }

    /// Get mutable reference to the UDP header
    pub fn header_mut(&mut self) -> &mut UdpHeader {
        unsafe { &mut *(self.buffer.as_mut_ptr() as *mut UdpHeader) }
    }

    /// Get mutable reference to the payload
    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.buffer[UdpHeader::SIZE..UdpHeader::SIZE + self.data_len]
    }

    /// Get immutable reference to header
    pub fn header(&self) -> &UdpHeader {
        unsafe { &*(self.buffer.as_ptr() as *const UdpHeader) }
    }

    /// Get immutable reference to payload
    pub fn payload(&self) -> &[u8] {
        &self.buffer[UdpHeader::SIZE..UdpHeader::SIZE + self.data_len]
    }

    /// Finalize the datagram by calculating checksum
    pub fn finalize(mut self, src_ip: &Ipv4Address, dst_ip: &Ipv4Address) -> &'a [u8] {
        let (header_bytes, payload_bytes) = self.buffer.split_at_mut(UdpHeader::SIZE);
        let header = unsafe { &mut *(header_bytes.as_mut_ptr() as *mut UdpHeader) };
        let payload = &payload_bytes[..self.data_len];
        header.calculate_checksum(src_ip, dst_ip, payload);
        &self.buffer[..UdpHeader::SIZE + self.data_len]
    }

    /// Finalize without checksum (for IPv4 where checksum is optional)
    pub fn finalize_no_checksum(self) -> &'a [u8] {
        &self.buffer[..UdpHeader::SIZE + self.data_len]
    }

    /// Get the total length (header + payload)
    pub fn total_len(&self) -> usize {
        UdpHeader::SIZE + self.data_len
    }

    /// Set source port
    pub fn set_src_port(&mut self, port: Port) {
        self.header_mut().src_port = port.to_be();
    }

    /// Set destination port
    pub fn set_dst_port(&mut self, port: Port) {
        self.header_mut().dst_port = port.to_be();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_header_creation() {
        let header = UdpHeader::new(12345, 80, 100);
        assert_eq!(header.src_port(), 12345);
        assert_eq!(header.dst_port(), 80);
        assert_eq!(header.length(), 108); // 8 (header) + 100 (data)
        assert!(header.is_valid_length());
    }

    #[test]
    fn test_udp_socket_options() {
        let opts = UdpSocketOptions::default();
        assert_eq!(opts.ttl, 64);
        assert!(!opts.broadcast);

        let opts_broadcast = opts.with_broadcast().with_ttl(128);
        assert_eq!(opts_broadcast.ttl, 128);
        assert!(opts_broadcast.broadcast);
    }

    #[test]
    fn test_udp_checksum() {
        let src_ip = Ipv4Address::new(192, 168, 1, 100);
        let dst_ip = Ipv4Address::new(8, 8, 8, 8);
        let payload = b"Hello, UDP!";

        let mut buffer = [0u8; 256];
        let mut datagram = UdpDatagramMut::new(&mut buffer, 12345, 53, payload.len()).unwrap();
        datagram.payload_mut().copy_from_slice(payload);
        let finalized = datagram.finalize(&src_ip, &dst_ip);

        // Parse and verify
        let parsed = UdpDatagram::parse(finalized).unwrap();
        assert!(parsed.header().verify_checksum(&src_ip, &dst_ip, parsed.payload()));
    }

    #[test]
    fn test_udp_datagram_parse() {
        let mut buffer = [0u8; 256];
        let payload = b"Test";
        let mut dg = UdpDatagramMut::new(&mut buffer, 5000, 5001, payload.len()).unwrap();
        dg.payload_mut().copy_from_slice(payload);
        let finalized = dg.finalize_no_checksum();

        let parsed = UdpDatagram::parse(finalized).unwrap();
        assert_eq!(parsed.src_port(), 5000);
        assert_eq!(parsed.dst_port(), 5001);
        assert_eq!(parsed.payload(), payload);
        assert!(parsed.validate_length());
    }

    #[test]
    fn test_udp_header_payload_size() {
        let header = UdpHeader::new(1234, 5678, 256);
        assert_eq!(header.payload_size(), Some(256));
    }

    #[test]
    fn test_udp_datagram_mut_port_update() {
        let mut buffer = [0u8; 128];
        let mut dg = UdpDatagramMut::new(&mut buffer, 1000, 2000, 10).unwrap();
        dg.set_src_port(3000);
        dg.set_dst_port(4000);
        
        assert_eq!(dg.header().src_port(), 3000);
        assert_eq!(dg.header().dst_port(), 4000);
    }
}
