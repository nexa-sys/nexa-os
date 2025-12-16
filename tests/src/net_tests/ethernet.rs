//! Ethernet tests (from src/net/ethernet.rs)

use crate::net::ethernet::{MacAddress, EtherType, EthernetFrame};

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
