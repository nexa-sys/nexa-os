//! UDP tests (from src/net/udp.rs)

use crate::net::ipv4::Ipv4Address;
use crate::net::udp::{UdpHeader, UdpDatagram, UdpDatagramMut, UdpSocketOptions};

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
    assert!(parsed
        .header()
        .verify_checksum(&src_ip, &dst_ip, parsed.payload()));
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
