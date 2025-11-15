/// UDP (User Datagram Protocol) implementation
///
/// This module provides structures and utilities for parsing, constructing,
/// and handling UDP datagrams.

use core::mem;

use super::ipv4::{Ipv4Address, Ipv4Header};

/// UDP port number
pub type Port = u16;

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

    /// Get mutable reference to the UDP header
    pub fn header_mut(&mut self) -> &mut UdpHeader {
        unsafe { &mut *(self.buffer.as_mut_ptr() as *mut UdpHeader) }
    }

    /// Get mutable reference to the payload
    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.buffer[UdpHeader::SIZE..UdpHeader::SIZE + self.data_len]
    }

    /// Finalize the datagram by calculating checksum
    pub fn finalize(mut self, src_ip: &Ipv4Address, dst_ip: &Ipv4Address) -> &'a [u8] {
        let payload = &self.buffer[UdpHeader::SIZE..UdpHeader::SIZE + self.data_len];
        self.header_mut().calculate_checksum(src_ip, dst_ip, payload);
        &self.buffer[..UdpHeader::SIZE + self.data_len]
    }

    /// Get the total length (header + payload)
    pub fn total_len(&self) -> usize {
        UdpHeader::SIZE + self.data_len
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
}
