/// Ethernet frame handling
///
/// This module provides structures and utilities for parsing and constructing
/// Ethernet II frames.

use core::mem;

/// Ethernet MAC address (6 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    pub const BROADCAST: MacAddress = MacAddress([0xFF; 6]);
    pub const ZERO: MacAddress = MacAddress([0x00; 6]);

    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }

    pub fn is_broadcast(&self) -> bool {
        *self == Self::BROADCAST
    }

    pub fn is_multicast(&self) -> bool {
        self.0[0] & 0x01 != 0
    }

    pub fn is_unicast(&self) -> bool {
        !self.is_multicast()
    }
}

impl core::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl From<[u8; 6]> for MacAddress {
    fn from(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }
}

impl From<&[u8]> for MacAddress {
    fn from(bytes: &[u8]) -> Self {
        let mut arr = [0u8; 6];
        arr.copy_from_slice(&bytes[..6]);
        Self(arr)
    }
}

/// EtherType values
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtherType {
    IPv4 = 0x0800,
    ARP = 0x0806,
    IPv6 = 0x86DD,
    Unknown = 0xFFFF,
}

impl From<u16> for EtherType {
    fn from(value: u16) -> Self {
        match value {
            0x0800 => EtherType::IPv4,
            0x0806 => EtherType::ARP,
            0x86DD => EtherType::IPv6,
            _ => EtherType::Unknown,
        }
    }
}

impl From<EtherType> for u16 {
    fn from(value: EtherType) -> Self {
        value as u16
    }
}

/// Ethernet II frame header (14 bytes)
#[repr(C, packed)]
pub struct EthernetHeader {
    pub dst_mac: MacAddress,
    pub src_mac: MacAddress,
    pub ether_type: u16, // Network byte order (big-endian)
}

impl EthernetHeader {
    pub const SIZE: usize = 14;

    pub fn ether_type(&self) -> EtherType {
        EtherType::from(u16::from_be(self.ether_type))
    }

    pub fn set_ether_type(&mut self, eth_type: EtherType) {
        self.ether_type = (eth_type as u16).to_be();
    }
}

/// Ethernet frame
pub struct EthernetFrame<'a> {
    buffer: &'a [u8],
}

impl<'a> EthernetFrame<'a> {
    /// Minimum Ethernet frame size (without FCS)
    pub const MIN_SIZE: usize = 60;
    /// Maximum Ethernet frame size (without FCS)
    pub const MAX_SIZE: usize = 1514;
    /// Maximum payload size
    pub const MAX_PAYLOAD: usize = Self::MAX_SIZE - EthernetHeader::SIZE;

    pub fn new(buffer: &'a [u8]) -> Option<Self> {
        if buffer.len() < EthernetHeader::SIZE {
            return None;
        }
        Some(Self { buffer })
    }

    pub fn header(&self) -> &EthernetHeader {
        unsafe { &*(self.buffer.as_ptr() as *const EthernetHeader) }
    }

    pub fn payload(&self) -> &[u8] {
        &self.buffer[EthernetHeader::SIZE..]
    }

    pub fn dst_mac(&self) -> MacAddress {
        self.header().dst_mac
    }

    pub fn src_mac(&self) -> MacAddress {
        self.header().src_mac
    }

    pub fn ether_type(&self) -> EtherType {
        self.header().ether_type()
    }
}

/// Mutable Ethernet frame for construction
pub struct EthernetFrameMut<'a> {
    buffer: &'a mut [u8],
}

impl<'a> EthernetFrameMut<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Option<Self> {
        if buffer.len() < EthernetHeader::SIZE {
            return None;
        }
        Some(Self { buffer })
    }

    pub fn header_mut(&mut self) -> &mut EthernetHeader {
        unsafe { &mut *(self.buffer.as_mut_ptr() as *mut EthernetHeader) }
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.buffer[EthernetHeader::SIZE..]
    }

    pub fn set_header(&mut self, dst: MacAddress, src: MacAddress, eth_type: EtherType) {
        let header = self.header_mut();
        header.dst_mac = dst;
        header.src_mac = src;
        header.set_ether_type(eth_type);
    }

    pub fn finalize(self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_address() {
        let mac = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert!(!mac.is_broadcast());
        assert!(mac.is_unicast());

        let broadcast = MacAddress::BROADCAST;
        assert!(broadcast.is_broadcast());
    }

    #[test]
    fn test_ethernet_frame_parse() {
        let mut buffer = [0u8; 64];
        buffer[0..6].copy_from_slice(&[0xFF; 6]); // dst
        buffer[6..12].copy_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]); // src
        buffer[12..14].copy_from_slice(&0x0800u16.to_be_bytes()); // IPv4

        let frame = EthernetFrame::new(&buffer).unwrap();
        assert_eq!(frame.dst_mac(), MacAddress::BROADCAST);
        assert_eq!(frame.ether_type(), EtherType::IPv4);
    }
}
