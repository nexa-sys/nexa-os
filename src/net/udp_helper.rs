#[cfg(test)]
use super::drivers::NetError;
/// UDP helper utilities and higher-level abstractions
///
/// This module provides:
/// - UDP socket state management helpers
/// - Common UDP patterns (broadcast, multicast awareness)
/// - Buffer management utilities
/// - Protocol handler abstractions

#[cfg(test)]
use super::ipv4::Ipv4Address;

// Note: UdpMessage is not used in production code, only in tests
// If needed in production, use references to avoid large stack allocations
/// UDP message for higher-level access (NOT RECOMMENDED for production use)
/// Use references or slices instead to avoid 64KB stack allocation
#[cfg(test)]
#[derive(Clone)]
pub struct UdpMessage {
    pub src_ip: [u8; 4],
    pub src_port: u16,
    pub dst_ip: [u8; 4],
    pub dst_port: u16,
    pub payload: [u8; 65535],
    pub payload_len: usize,
}

#[cfg(test)]
impl UdpMessage {
    /// Create new UDP message
    pub fn new() -> Self {
        Self {
            src_ip: [0; 4],
            src_port: 0,
            dst_ip: [0; 4],
            dst_port: 0,
            payload: [0u8; 65535],
            payload_len: 0,
        }
    }

    /// Set source address
    pub fn set_src(&mut self, ip: [u8; 4], port: u16) {
        self.src_ip = ip;
        self.src_port = port;
    }

    /// Set destination address
    pub fn set_dst(&mut self, ip: [u8; 4], port: u16) {
        self.dst_ip = ip;
        self.dst_port = port;
    }

    /// Get payload slice
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len]
    }

    /// Get mutable payload slice
    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.payload[..self.payload_len]
    }

    /// Set payload from slice
    pub fn set_payload(&mut self, data: &[u8]) -> Result<(), NetError> {
        if data.len() > 65535 {
            return Err(NetError::BufferTooSmall);
        }
        self.payload_len = data.len();
        self.payload[..data.len()].copy_from_slice(data);
        Ok(())
    }

    /// Check if message is complete
    pub fn is_valid(&self) -> bool {
        self.payload_len > 0 && self.src_port > 0 && self.dst_port > 0
    }
}

/// UDP connection context for stateful operations
#[derive(Clone, Copy)]
pub struct UdpConnectionContext {
    pub local_ip: [u8; 4],
    pub local_port: u16,
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    pub ttl: u8,
    pub tos: u8,
}

impl UdpConnectionContext {
    /// Create new connection context
    pub fn new(local_ip: [u8; 4], local_port: u16, remote_ip: [u8; 4], remote_port: u16) -> Self {
        Self {
            local_ip,
            local_port,
            remote_ip,
            remote_port,
            ttl: 64,
            tos: 0,
        }
    }

    /// Set TTL
    pub fn with_ttl(mut self, ttl: u8) -> Self {
        self.ttl = ttl;
        self
    }

    /// Set Type of Service
    pub fn with_tos(mut self, tos: u8) -> Self {
        self.tos = tos;
        self
    }

    /// Check if addresses are valid
    pub fn validate(&self) -> bool {
        // Check that IPs are not all zeros
        !self.local_ip.iter().all(|&b| b == 0)
            && !self.remote_ip.iter().all(|&b| b == 0)
            && self.local_port > 0
            && self.remote_port > 0
    }
}

// UdpProtocolHandler trait is deprecated - don't use in production
// Use raw buffer processing instead to avoid large allocations
#[cfg(test)]
/// UDP protocol handler trait (test-only, uses large UdpMessage)
pub trait UdpProtocolHandler {
    /// Handle incoming UDP packet
    fn handle_packet(&mut self, message: &UdpMessage) -> Result<(), NetError>;

    /// Called periodically for protocol timeouts, retransmissions, etc.
    fn poll(&mut self) -> Result<(), NetError>;

    /// Check if this handler is interested in the given destination port
    fn handles_port(&self, port: u16) -> bool;
}

/// DNS-like protocol helper (port 53)
pub struct DnsHelper {
    pub port: u16,
}

impl DnsHelper {
    pub fn new() -> Self {
        Self { port: 53 }
    }

    /// Check if packet is DNS query
    pub fn is_dns_query(payload: &[u8]) -> bool {
        // Minimum DNS query: ID (2) + Flags (2) + Questions (2) + ...
        if payload.len() < 12 {
            return false;
        }
        // Check QR bit (first bit of flags) - should be 0 for query
        (payload[2] & 0x80) == 0
    }

    /// Extract DNS query name (simplified)
    pub fn extract_query_name(payload: &[u8]) -> Option<(usize, usize)> {
        if payload.len() < 12 {
            return None;
        }
        // Return question section start and end offsets
        Some((12, payload.len()))
    }
}

/// DHCP helper (port 67/68)
pub struct DhcpHelper {
    pub server_port: u16,
    pub client_port: u16,
}

impl DhcpHelper {
    pub fn new() -> Self {
        Self {
            server_port: 67,
            client_port: 68,
        }
    }

    /// Check if packet is DHCP
    pub fn is_dhcp_packet(payload: &[u8]) -> bool {
        // DHCP packets have minimum length of 240 bytes
        payload.len() >= 240
    }

    /// Check DHCP message type from options
    pub fn get_message_type(payload: &[u8]) -> Option<u8> {
        if payload.len() < 240 {
            return None;
        }
        // Message type is in options field (starting at byte 236)
        // This is simplified; real implementation would parse options correctly
        if payload.len() > 240 {
            Some(payload[240])
        } else {
            None
        }
    }
}

/// NTP helper (port 123)
pub struct NtpHelper {
    pub port: u16,
}

impl NtpHelper {
    pub fn new() -> Self {
        Self { port: 123 }
    }

    /// Check if packet is NTP
    pub fn is_ntp_packet(payload: &[u8]) -> bool {
        // NTP packets are exactly 48 bytes
        payload.len() == 48
    }

    /// Get NTP version from packet
    pub fn get_version(payload: &[u8]) -> Option<u8> {
        if payload.is_empty() {
            return None;
        }
        Some((payload[0] >> 3) & 0x07)
    }

    /// Get NTP mode from packet
    pub fn get_mode(payload: &[u8]) -> Option<u8> {
        if payload.is_empty() {
            return None;
        }
        Some(payload[0] & 0x07)
    }
}

/// UDP packet statistics
#[derive(Clone, Copy, Debug)]
pub struct UdpStats {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub checksum_errors: u64,
    pub length_errors: u64,
}

impl UdpStats {
    /// Create empty stats
    pub const fn new() -> Self {
        Self {
            packets_sent: 0,
            packets_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            checksum_errors: 0,
            length_errors: 0,
        }
    }

    /// Record sent packet
    pub fn record_sent(&mut self, bytes: usize) {
        self.packets_sent += 1;
        self.bytes_sent += bytes as u64;
    }

    /// Record received packet
    pub fn record_received(&mut self, bytes: usize) {
        self.packets_received += 1;
        self.bytes_received += bytes as u64;
    }

    /// Record checksum error
    pub fn record_checksum_error(&mut self) {
        self.checksum_errors += 1;
    }

    /// Record length error
    pub fn record_length_error(&mut self) {
        self.length_errors += 1;
    }

    /// Reset statistics
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Get average packet size sent
    pub fn avg_sent_size(&self) -> u64 {
        if self.packets_sent > 0 {
            self.bytes_sent / self.packets_sent
        } else {
            0
        }
    }

    /// Get average packet size received
    pub fn avg_recv_size(&self) -> u64 {
        if self.packets_received > 0 {
            self.bytes_received / self.packets_received
        } else {
            0
        }
    }
}
