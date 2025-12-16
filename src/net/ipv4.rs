/// IPv4 packet handling
///
/// This module provides structures and utilities for parsing and constructing
/// IPv4 packets.
use core::mem;

/// IPv4 address (4 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ipv4Address(pub [u8; 4]);

impl Ipv4Address {
    pub const UNSPECIFIED: Ipv4Address = Ipv4Address([0, 0, 0, 0]);
    pub const BROADCAST: Ipv4Address = Ipv4Address([255, 255, 255, 255]);
    pub const LOOPBACK: Ipv4Address = Ipv4Address([127, 0, 0, 1]);

    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }

    pub fn from_bytes(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    pub fn is_broadcast(&self) -> bool {
        *self == Self::BROADCAST
    }

    pub fn is_multicast(&self) -> bool {
        self.0[0] >= 224 && self.0[0] <= 239
    }

    pub fn is_loopback(&self) -> bool {
        self.0[0] == 127
    }

    pub fn is_private(&self) -> bool {
        match self.0[0] {
            10 => true,
            172 => self.0[1] >= 16 && self.0[1] <= 31,
            192 => self.0[1] == 168,
            _ => false,
        }
    }
}

impl core::fmt::Display for Ipv4Address {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

impl From<[u8; 4]> for Ipv4Address {
    fn from(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }
}

impl From<&[u8]> for Ipv4Address {
    fn from(bytes: &[u8]) -> Self {
        let mut arr = [0u8; 4];
        arr.copy_from_slice(&bytes[..4]);
        Self(arr)
    }
}

/// IP protocol numbers
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpProtocol {
    ICMP = 1,
    TCP = 6,
    UDP = 17,
    Unknown = 0xFF,
}

impl From<u8> for IpProtocol {
    fn from(value: u8) -> Self {
        match value {
            1 => IpProtocol::ICMP,
            6 => IpProtocol::TCP,
            17 => IpProtocol::UDP,
            _ => IpProtocol::Unknown,
        }
    }
}

impl From<IpProtocol> for u8 {
    fn from(value: IpProtocol) -> Self {
        value as u8
    }
}

/// IPv4 header (minimum 20 bytes)
#[repr(C, packed)]
pub struct Ipv4Header {
    pub version_ihl: u8,       // Version (4 bits) + IHL (4 bits)
    pub dscp_ecn: u8,          // DSCP (6 bits) + ECN (2 bits)
    pub total_length: u16,     // Total length (header + data) in bytes
    pub identification: u16,   // Identification
    pub flags_fragment: u16,   // Flags (3 bits) + Fragment offset (13 bits)
    pub ttl: u8,               // Time to live
    pub protocol: u8,          // Protocol
    pub header_checksum: u16,  // Header checksum
    pub src_addr: Ipv4Address, // Source address
    pub dst_addr: Ipv4Address, // Destination address
}

impl Ipv4Header {
    pub const MIN_SIZE: usize = 20;

    pub fn version(&self) -> u8 {
        self.version_ihl >> 4
    }

    pub fn ihl(&self) -> u8 {
        self.version_ihl & 0x0F
    }

    pub fn header_len(&self) -> usize {
        (self.ihl() as usize) * 4
    }

    pub fn total_length(&self) -> u16 {
        u16::from_be(self.total_length)
    }

    pub fn protocol(&self) -> IpProtocol {
        IpProtocol::from(self.protocol)
    }

    pub fn set_version_ihl(&mut self, version: u8, ihl: u8) {
        self.version_ihl = (version << 4) | (ihl & 0x0F);
    }

    pub fn set_total_length(&mut self, length: u16) {
        self.total_length = length.to_be();
    }

    pub fn set_protocol(&mut self, protocol: IpProtocol) {
        self.protocol = protocol as u8;
    }

    /// Calculate and set IPv4 header checksum
    pub fn calculate_checksum(&mut self) {
        self.header_checksum = 0;
        let header_bytes = unsafe {
            core::slice::from_raw_parts(self as *const Self as *const u8, self.header_len())
        };
        self.header_checksum = calculate_checksum(header_bytes).to_be();
    }

    /// Verify IPv4 header checksum
    pub fn verify_checksum(&self) -> bool {
        let header_bytes = unsafe {
            core::slice::from_raw_parts(self as *const Self as *const u8, self.header_len())
        };
        calculate_checksum(header_bytes) == 0
    }
}

/// IPv4 packet
pub struct Ipv4Packet<'a> {
    buffer: &'a [u8],
}

impl<'a> Ipv4Packet<'a> {
    pub fn new(buffer: &'a [u8]) -> Option<Self> {
        if buffer.len() < Ipv4Header::MIN_SIZE {
            return None;
        }

        let packet = Self { buffer };
        let header = packet.header();

        // Validate version
        if header.version() != 4 {
            return None;
        }

        // Validate IHL
        if header.ihl() < 5 {
            return None;
        }

        // Validate total length
        let total_len = header.total_length() as usize;
        if total_len > buffer.len() {
            return None;
        }

        Some(packet)
    }

    pub fn header(&self) -> &Ipv4Header {
        unsafe { &*(self.buffer.as_ptr() as *const Ipv4Header) }
    }

    pub fn payload(&self) -> &[u8] {
        let header_len = self.header().header_len();
        &self.buffer[header_len..]
    }

    pub fn src_addr(&self) -> Ipv4Address {
        self.header().src_addr
    }

    pub fn dst_addr(&self) -> Ipv4Address {
        self.header().dst_addr
    }

    pub fn protocol(&self) -> IpProtocol {
        self.header().protocol()
    }
}

/// Mutable IPv4 packet for construction
pub struct Ipv4PacketMut<'a> {
    buffer: &'a mut [u8],
}

impl<'a> Ipv4PacketMut<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Option<Self> {
        if buffer.len() < Ipv4Header::MIN_SIZE {
            return None;
        }
        Some(Self { buffer })
    }

    pub fn header_mut(&mut self) -> &mut Ipv4Header {
        unsafe { &mut *(self.buffer.as_mut_ptr() as *mut Ipv4Header) }
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        let header_len = Ipv4Header::MIN_SIZE;
        &mut self.buffer[header_len..]
    }

    pub fn set_header(
        &mut self,
        src: Ipv4Address,
        dst: Ipv4Address,
        protocol: IpProtocol,
        ttl: u8,
        payload_len: usize,
    ) {
        let header = self.header_mut();
        header.set_version_ihl(4, 5); // IPv4, no options
        header.dscp_ecn = 0;
        header.set_total_length((Ipv4Header::MIN_SIZE + payload_len) as u16);
        header.identification = 0;
        header.flags_fragment = 0;
        header.ttl = ttl;
        header.set_protocol(protocol);
        header.src_addr = src;
        header.dst_addr = dst;
        header.calculate_checksum();
    }

    pub fn finalize(self) -> usize {
        let header = unsafe { &*(self.buffer.as_ptr() as *const Ipv4Header) };
        header.total_length() as usize
    }
}

/// Calculate Internet checksum (RFC 1071)
pub fn calculate_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Sum 16-bit words
    let mut i = 0;
    while i < data.len() - 1 {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum += word as u32;
        i += 2;
    }

    // Add remaining byte if odd length
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }

    // Fold 32-bit sum to 16 bits
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    // One's complement
    !sum as u16
}
