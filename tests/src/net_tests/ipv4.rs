//! IPv4 tests (from src/net/ipv4.rs)

use crate::net::ipv4::{Ipv4Address, calculate_checksum};

#[test]
fn test_ipv4_address() {
    let addr = Ipv4Address::new(192, 168, 1, 1);
    assert!(addr.is_private());
    assert!(!addr.is_broadcast());

    let loopback = Ipv4Address::LOOPBACK;
    assert!(loopback.is_loopback());
}

#[test]
fn test_checksum() {
    let data = [0x45, 0x00, 0x00, 0x3c, 0x1c, 0x46, 0x40, 0x00, 0x40, 0x06];
    let checksum = calculate_checksum(&data);
    assert_ne!(checksum, 0);
}
