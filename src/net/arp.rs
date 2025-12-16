/// ARP (Address Resolution Protocol) implementation
///
/// This module provides structures and utilities for ARP requests/replies
/// and ARP cache management.
use crate::ktrace;
use core::mem;

use super::ethernet::MacAddress;
use super::ipv4::Ipv4Address;

/// ARP hardware types
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareType {
    Ethernet = 1,
    Unknown = 0xFFFF,
}

impl From<u16> for HardwareType {
    fn from(value: u16) -> Self {
        match value {
            1 => HardwareType::Ethernet,
            _ => HardwareType::Unknown,
        }
    }
}

/// ARP operations
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpOperation {
    Request = 1,
    Reply = 2,
    Unknown = 0xFFFF,
}

impl From<u16> for ArpOperation {
    fn from(value: u16) -> Self {
        match value {
            1 => ArpOperation::Request,
            2 => ArpOperation::Reply,
            _ => ArpOperation::Unknown,
        }
    }
}

/// ARP packet for Ethernet/IPv4 (28 bytes)
#[repr(C, packed)]
pub struct ArpPacket {
    pub hw_type: u16,                   // Hardware type (1 = Ethernet)
    pub proto_type: u16,                // Protocol type (0x0800 = IPv4)
    pub hw_addr_len: u8,                // Hardware address length (6 for MAC)
    pub proto_addr_len: u8,             // Protocol address length (4 for IPv4)
    pub operation: u16,                 // Operation (1 = request, 2 = reply)
    pub sender_hw_addr: MacAddress,     // Sender hardware address
    pub sender_proto_addr: Ipv4Address, // Sender protocol address
    pub target_hw_addr: MacAddress,     // Target hardware address
    pub target_proto_addr: Ipv4Address, // Target protocol address
}

impl ArpPacket {
    pub const SIZE: usize = 28;

    /// Create a new ARP request
    pub fn new_request(
        sender_mac: MacAddress,
        sender_ip: Ipv4Address,
        target_ip: Ipv4Address,
    ) -> Self {
        Self {
            hw_type: 1u16.to_be(),
            proto_type: 0x0800u16.to_be(),
            hw_addr_len: 6,
            proto_addr_len: 4,
            operation: 1u16.to_be(),
            sender_hw_addr: sender_mac,
            sender_proto_addr: sender_ip,
            target_hw_addr: MacAddress::ZERO,
            target_proto_addr: target_ip,
        }
    }

    /// Create a new ARP reply
    pub fn new_reply(
        sender_mac: MacAddress,
        sender_ip: Ipv4Address,
        target_mac: MacAddress,
        target_ip: Ipv4Address,
    ) -> Self {
        Self {
            hw_type: 1u16.to_be(),
            proto_type: 0x0800u16.to_be(),
            hw_addr_len: 6,
            proto_addr_len: 4,
            operation: 2u16.to_be(),
            sender_hw_addr: sender_mac,
            sender_proto_addr: sender_ip,
            target_hw_addr: target_mac,
            target_proto_addr: target_ip,
        }
    }

    /// Get hardware type
    pub fn hw_type(&self) -> HardwareType {
        HardwareType::from(u16::from_be(self.hw_type))
    }

    /// Get protocol type
    pub fn proto_type(&self) -> u16 {
        u16::from_be(self.proto_type)
    }

    /// Get operation
    pub fn operation(&self) -> ArpOperation {
        ArpOperation::from(u16::from_be(self.operation))
    }

    /// Check if this is a valid Ethernet/IPv4 ARP packet
    pub fn is_valid(&self) -> bool {
        self.hw_type() == HardwareType::Ethernet
            && self.proto_type() == 0x0800
            && self.hw_addr_len == 6
            && self.proto_addr_len == 4
    }
}

/// ARP cache entry
#[derive(Clone, Copy)]
pub struct ArpEntry {
    pub ip: Ipv4Address,
    pub mac: MacAddress,
    pub timestamp_ms: u64,
    pub valid: bool,
}

impl ArpEntry {
    pub const fn empty() -> Self {
        Self {
            ip: Ipv4Address::UNSPECIFIED,
            mac: MacAddress::ZERO,
            timestamp_ms: 0,
            valid: false,
        }
    }

    pub fn new(ip: Ipv4Address, mac: MacAddress, timestamp_ms: u64) -> Self {
        Self {
            ip,
            mac,
            timestamp_ms,
            valid: true,
        }
    }

    /// Check if entry is stale (older than 60 seconds)
    pub fn is_stale(&self, current_ms: u64) -> bool {
        if !self.valid {
            return true;
        }
        current_ms.saturating_sub(self.timestamp_ms) > 60_000
    }
}

/// ARP cache
pub const ARP_CACHE_SIZE: usize = 32;

pub struct ArpCache {
    entries: [ArpEntry; ARP_CACHE_SIZE],
}

impl ArpCache {
    pub const fn new() -> Self {
        Self {
            entries: [ArpEntry::empty(); ARP_CACHE_SIZE],
        }
    }

    /// Look up MAC address for an IP address
    pub fn lookup(&self, ip: &Ipv4Address, current_ms: u64) -> Option<MacAddress> {
        let valid_count = self.entries.iter().filter(|e| e.valid).count();

        ktrace!(
            "[ARP Cache @{:p}] LOOKUP for {} (current_ms={}, cache has {} valid entries)",
            self,
            ip,
            current_ms,
            valid_count
        );

        // Show all valid entries for debugging
        for entry in self.entries.iter() {
            if entry.valid {
                let age_ms = current_ms.saturating_sub(entry.timestamp_ms);
                let is_match = entry.ip == *ip;
                let is_stale = entry.is_stale(current_ms);
                ktrace!(
                    "  Entry: {} -> {} (age: {}ms, match={}, stale={})",
                    entry.ip,
                    entry.mac,
                    age_ms,
                    is_match,
                    is_stale
                );
            }
        }

        let result = self
            .entries
            .iter()
            .find(|e| e.valid && e.ip == *ip && !e.is_stale(current_ms))
            .map(|e| e.mac);

        if result.is_some() {
            ktrace!("[ARP Cache] HIT for {}", ip);
        } else {
            ktrace!("[ARP Cache] MISS for {}", ip);
        }

        result
    }

    /// Insert or update an ARP cache entry
    pub fn insert(&mut self, ip: Ipv4Address, mac: MacAddress, timestamp_ms: u64) {
        ktrace!("[ARP Cache @{:p}] INSERT {} -> {}", self, ip, mac);

        // Try to update existing entry
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.ip == ip {
                entry.mac = mac;
                entry.timestamp_ms = timestamp_ms;
                return;
            }
        }

        // Find empty slot or oldest entry
        let mut oldest_idx = 0;
        let mut oldest_time = u64::MAX;

        for (idx, entry) in self.entries.iter().enumerate() {
            if !entry.valid {
                oldest_idx = idx;
                break;
            }
            if entry.timestamp_ms < oldest_time {
                oldest_time = entry.timestamp_ms;
                oldest_idx = idx;
            }
        }

        self.entries[oldest_idx] = ArpEntry::new(ip, mac, timestamp_ms);

        // Verify insertion
        let valid_count_after = self.entries.iter().filter(|e| e.valid).count();
        ktrace!(
            "[ARP Cache] INSERT complete: valid_count={}, entry[{}].valid={}, entry[{}].ip={}",
            valid_count_after,
            oldest_idx,
            self.entries[oldest_idx].valid,
            oldest_idx,
            self.entries[oldest_idx].ip
        );
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        for entry in self.entries.iter_mut() {
            *entry = ArpEntry::empty();
        }
    }

    /// Remove stale entries
    pub fn cleanup(&mut self, current_ms: u64) {
        for entry in self.entries.iter_mut() {
            if entry.is_stale(current_ms) {
                entry.valid = false;
            }
        }
    }
}
