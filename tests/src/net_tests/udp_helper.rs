//! UDP helper tests (from src/net/udp_helper.rs)

use crate::net::udp_helper::{UdpMessage, UdpConnectionContext, UdpStats, DnsHelper, DhcpHelper, NtpHelper};

#[test]
fn test_udp_message_creation() {
    let mut msg = UdpMessage::new();
    msg.set_src([192, 168, 1, 1], 5000);
    msg.set_dst([192, 168, 1, 254], 5001);

    assert_eq!(msg.src_ip, [192, 168, 1, 1]);
    assert_eq!(msg.src_port, 5000);
    assert_eq!(msg.dst_ip, [192, 168, 1, 254]);
    assert_eq!(msg.dst_port, 5001);
}

#[test]
fn test_udp_connection_context() {
    let ctx = UdpConnectionContext::new([10, 0, 2, 15], 5000, [8, 8, 8, 8], 53)
        .with_ttl(128)
        .with_tos(0x10);

    assert!(ctx.validate());
    assert_eq!(ctx.ttl, 128);
}

#[test]
fn test_udp_stats() {
    let mut stats = UdpStats::new();
    stats.record_sent(100);
    stats.record_sent(200);
    stats.record_received(150);

    assert_eq!(stats.packets_sent, 2);
    assert_eq!(stats.bytes_sent, 300);
    assert_eq!(stats.packets_received, 1);
    assert_eq!(stats.avg_sent_size(), 150);
}

#[test]
fn test_dns_helper() {
    let dns = DnsHelper::new();
    assert_eq!(dns.port, 53);
}

#[test]
fn test_dhcp_helper() {
    let dhcp = DhcpHelper::new();
    assert_eq!(dhcp.server_port, 67);
    assert_eq!(dhcp.client_port, 68);
}

#[test]
fn test_ntp_helper() {
    let ntp = NtpHelper::new();
    assert_eq!(ntp.port, 123);
}
