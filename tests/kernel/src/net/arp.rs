//! ARP tests (from src/net/arp.rs)

use crate::net::ethernet::MacAddress;
use crate::net::ipv4::Ipv4Address;
use crate::net::arp::{ArpPacket, ArpOperation, ArpCache};

#[test]
fn test_arp_request() {
    let sender_mac = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    let sender_ip = Ipv4Address::new(192, 168, 1, 100);
    let target_ip = Ipv4Address::new(192, 168, 1, 1);

    let request = ArpPacket::new_request(sender_mac, sender_ip, target_ip);

    assert!(request.is_valid());
    assert_eq!(request.operation(), ArpOperation::Request);
    assert_eq!(request.sender_hw_addr, sender_mac);
    assert_eq!(request.sender_proto_addr, sender_ip);
    assert_eq!(request.target_proto_addr, target_ip);
}

#[test]
fn test_arp_cache() {
    let mut cache = ArpCache::new();
    let ip = Ipv4Address::new(192, 168, 1, 1);
    let mac = MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);

    cache.insert(ip, mac, 1000);
    assert_eq!(cache.lookup(&ip, 1000), Some(mac));

    // Should be stale after 60 seconds (60000ms)
    assert_eq!(cache.lookup(&ip, 62000), None);
}
