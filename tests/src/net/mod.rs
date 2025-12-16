//! Tests for kernel network code
//!
//! These tests directly use the kernel's net modules included via #[path].

#[cfg(test)]
mod tests {
    // Use the kernel modules included in lib.rs via #[path]
    use crate::ethernet::{MacAddress, EtherType};
    use crate::ipv4::{Ipv4Address, IpProtocol};
    use crate::arp::{ArpPacket, ArpOperation};

    #[test]
    fn test_mac_address_broadcast() {
        let broadcast = MacAddress::BROADCAST;
        assert!(broadcast.is_broadcast());
        assert!(broadcast.is_multicast());
    }

    #[test]
    fn test_mac_address_unicast() {
        let mac = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert!(!mac.is_broadcast());
        assert!(mac.is_unicast());
        assert!(!mac.is_multicast());
    }

    #[test]
    fn test_mac_address_multicast() {
        // Multicast MAC has LSB of first byte set
        let mac = MacAddress::new([0x01, 0x00, 0x5e, 0x00, 0x00, 0x01]);
        assert!(mac.is_multicast());
        assert!(!mac.is_unicast());
    }

    #[test]
    fn test_ipv4_address_types() {
        let loopback = Ipv4Address::LOOPBACK;
        assert!(loopback.is_loopback());

        let broadcast = Ipv4Address::BROADCAST;
        assert!(broadcast.is_broadcast());

        let multicast = Ipv4Address::new(224, 0, 0, 1);
        assert!(multicast.is_multicast());

        let private_10 = Ipv4Address::new(10, 0, 0, 1);
        assert!(private_10.is_private());

        let private_172 = Ipv4Address::new(172, 16, 0, 1);
        assert!(private_172.is_private());

        let private_192 = Ipv4Address::new(192, 168, 1, 1);
        assert!(private_192.is_private());

        let public = Ipv4Address::new(8, 8, 8, 8);
        assert!(!public.is_private());
        assert!(!public.is_loopback());
        assert!(!public.is_multicast());
    }

    #[test]
    fn test_ip_protocol_conversion() {
        assert_eq!(IpProtocol::from(1), IpProtocol::ICMP);
        assert_eq!(IpProtocol::from(6), IpProtocol::TCP);
        assert_eq!(IpProtocol::from(17), IpProtocol::UDP);
        assert_eq!(IpProtocol::from(99), IpProtocol::Unknown);
    }

    #[test]
    fn test_ether_type_conversion() {
        assert_eq!(EtherType::from(0x0800), EtherType::IPv4);
        assert_eq!(EtherType::from(0x0806), EtherType::ARP);
        assert_eq!(EtherType::from(0x86DD), EtherType::IPv6);
        assert_eq!(EtherType::from(0x1234), EtherType::Unknown);
    }

    #[test]
    fn test_arp_request_creation() {
        let sender_mac = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let sender_ip = Ipv4Address::new(192, 168, 1, 100);
        let target_ip = Ipv4Address::new(192, 168, 1, 1);

        let arp = ArpPacket::new_request(sender_mac, sender_ip, target_ip);
        
        assert_eq!(arp.sender_hw_addr, sender_mac);
        assert_eq!(arp.sender_proto_addr, sender_ip);
        assert_eq!(arp.target_proto_addr, target_ip);
        assert_eq!(arp.target_hw_addr, MacAddress::ZERO);
    }

    #[test]
    fn test_arp_reply_creation() {
        let sender_mac = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let sender_ip = Ipv4Address::new(192, 168, 1, 1);
        let target_mac = MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let target_ip = Ipv4Address::new(192, 168, 1, 100);

        let arp = ArpPacket::new_reply(sender_mac, sender_ip, target_mac, target_ip);
        
        assert_eq!(arp.sender_hw_addr, sender_mac);
        assert_eq!(arp.sender_proto_addr, sender_ip);
        assert_eq!(arp.target_hw_addr, target_mac);
        assert_eq!(arp.target_proto_addr, target_ip);
    }

    #[test]
    fn test_arp_operation() {
        assert_eq!(ArpOperation::from(1), ArpOperation::Request);
        assert_eq!(ArpOperation::from(2), ArpOperation::Reply);
        assert_eq!(ArpOperation::from(99), ArpOperation::Unknown);
    }
}
