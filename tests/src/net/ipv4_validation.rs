//! IPv4 Address Validation Tests
//!
//! Tests for IPv4 address handling, classification, and edge cases.
//! These tests help catch bugs in network address handling.

#[cfg(test)]
mod tests {
    use crate::net::ipv4::{Ipv4Address, IpProtocol};

    // =========================================================================
    // IPv4 Address Construction Tests
    // =========================================================================

    #[test]
    fn test_ipv4_new() {
        let addr = Ipv4Address::new(192, 168, 1, 1);
        assert_eq!(addr.0, [192, 168, 1, 1]);
    }

    #[test]
    fn test_ipv4_from_bytes() {
        let bytes = [10, 0, 0, 1];
        let addr = Ipv4Address::from_bytes(bytes);
        assert_eq!(addr.0, bytes);
    }

    #[test]
    fn test_ipv4_as_bytes() {
        let addr = Ipv4Address::new(172, 16, 0, 1);
        let bytes = addr.as_bytes();
        assert_eq!(*bytes, [172, 16, 0, 1]);
    }

    // =========================================================================
    // Special Address Constants Tests
    // =========================================================================

    #[test]
    fn test_ipv4_unspecified() {
        assert_eq!(Ipv4Address::UNSPECIFIED.0, [0, 0, 0, 0]);
    }

    #[test]
    fn test_ipv4_broadcast() {
        assert_eq!(Ipv4Address::BROADCAST.0, [255, 255, 255, 255]);
    }

    #[test]
    fn test_ipv4_loopback() {
        assert_eq!(Ipv4Address::LOOPBACK.0, [127, 0, 0, 1]);
    }

    // =========================================================================
    // Address Classification Tests
    // =========================================================================

    #[test]
    fn test_is_broadcast() {
        assert!(Ipv4Address::BROADCAST.is_broadcast());
        assert!(!Ipv4Address::new(255, 255, 255, 0).is_broadcast());
        assert!(!Ipv4Address::new(192, 168, 1, 255).is_broadcast());
    }

    #[test]
    fn test_is_multicast() {
        // Multicast range: 224.0.0.0 - 239.255.255.255
        assert!(Ipv4Address::new(224, 0, 0, 1).is_multicast());
        assert!(Ipv4Address::new(239, 255, 255, 255).is_multicast());
        assert!(Ipv4Address::new(230, 1, 2, 3).is_multicast());
        
        assert!(!Ipv4Address::new(223, 255, 255, 255).is_multicast());
        assert!(!Ipv4Address::new(240, 0, 0, 0).is_multicast());
        assert!(!Ipv4Address::new(192, 168, 1, 1).is_multicast());
    }

    #[test]
    fn test_is_loopback() {
        // Loopback range: 127.0.0.0/8
        assert!(Ipv4Address::new(127, 0, 0, 1).is_loopback());
        assert!(Ipv4Address::new(127, 0, 0, 0).is_loopback());
        assert!(Ipv4Address::new(127, 255, 255, 255).is_loopback());
        assert!(Ipv4Address::new(127, 1, 2, 3).is_loopback());
        
        assert!(!Ipv4Address::new(126, 0, 0, 1).is_loopback());
        assert!(!Ipv4Address::new(128, 0, 0, 1).is_loopback());
    }

    #[test]
    fn test_is_private_class_a() {
        // Private Class A: 10.0.0.0/8
        assert!(Ipv4Address::new(10, 0, 0, 0).is_private());
        assert!(Ipv4Address::new(10, 255, 255, 255).is_private());
        assert!(Ipv4Address::new(10, 1, 2, 3).is_private());
        
        assert!(!Ipv4Address::new(11, 0, 0, 0).is_private());
        assert!(!Ipv4Address::new(9, 255, 255, 255).is_private());
    }

    #[test]
    fn test_is_private_class_b() {
        // Private Class B: 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
        assert!(Ipv4Address::new(172, 16, 0, 0).is_private());
        assert!(Ipv4Address::new(172, 31, 255, 255).is_private());
        assert!(Ipv4Address::new(172, 20, 1, 1).is_private());
        
        assert!(!Ipv4Address::new(172, 15, 255, 255).is_private());
        assert!(!Ipv4Address::new(172, 32, 0, 0).is_private());
    }

    #[test]
    fn test_is_private_class_c() {
        // Private Class C: 192.168.0.0/16
        assert!(Ipv4Address::new(192, 168, 0, 0).is_private());
        assert!(Ipv4Address::new(192, 168, 255, 255).is_private());
        assert!(Ipv4Address::new(192, 168, 1, 100).is_private());
        
        assert!(!Ipv4Address::new(192, 167, 0, 0).is_private());
        assert!(!Ipv4Address::new(192, 169, 0, 0).is_private());
    }

    #[test]
    fn test_public_addresses_not_private() {
        assert!(!Ipv4Address::new(8, 8, 8, 8).is_private());
        assert!(!Ipv4Address::new(1, 1, 1, 1).is_private());
        assert!(!Ipv4Address::new(208, 67, 222, 222).is_private());
    }

    // =========================================================================
    // Address Comparison Tests
    // =========================================================================

    #[test]
    fn test_ipv4_equality() {
        let a = Ipv4Address::new(192, 168, 1, 1);
        let b = Ipv4Address::new(192, 168, 1, 1);
        let c = Ipv4Address::new(192, 168, 1, 2);
        
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_ipv4_hash_consistency() {
        use std::collections::HashSet;
        
        let mut set = HashSet::new();
        set.insert(Ipv4Address::new(192, 168, 1, 1));
        set.insert(Ipv4Address::new(192, 168, 1, 1)); // Duplicate
        
        assert_eq!(set.len(), 1, "Duplicate addresses should hash the same");
        
        set.insert(Ipv4Address::new(192, 168, 1, 2));
        assert_eq!(set.len(), 2);
    }

    // =========================================================================
    // From/Into Conversion Tests
    // =========================================================================

    #[test]
    fn test_ipv4_from_array() {
        let bytes: [u8; 4] = [10, 20, 30, 40];
        let addr: Ipv4Address = bytes.into();
        assert_eq!(addr.0, bytes);
    }

    #[test]
    fn test_ipv4_from_slice() {
        let bytes: &[u8] = &[10, 20, 30, 40, 50, 60]; // Extra bytes
        let addr: Ipv4Address = bytes.into();
        assert_eq!(addr.0, [10, 20, 30, 40]); // Only first 4 bytes
    }

    // =========================================================================
    // IP Protocol Tests
    // =========================================================================

    #[test]
    fn test_ip_protocol_values() {
        assert_eq!(IpProtocol::ICMP as u8, 1);
        assert_eq!(IpProtocol::TCP as u8, 6);
        assert_eq!(IpProtocol::UDP as u8, 17);
    }

    #[test]
    fn test_ip_protocol_from_u8() {
        assert_eq!(IpProtocol::from(1), IpProtocol::ICMP);
        assert_eq!(IpProtocol::from(6), IpProtocol::TCP);
        assert_eq!(IpProtocol::from(17), IpProtocol::UDP);
        assert_eq!(IpProtocol::from(99), IpProtocol::Unknown);
        assert_eq!(IpProtocol::from(0), IpProtocol::Unknown);
    }

    #[test]
    fn test_ip_protocol_to_u8() {
        assert_eq!(u8::from(IpProtocol::ICMP), 1);
        assert_eq!(u8::from(IpProtocol::TCP), 6);
        assert_eq!(u8::from(IpProtocol::UDP), 17);
    }

    // =========================================================================
    // Display Format Tests
    // =========================================================================

    #[test]
    fn test_ipv4_display() {
        let addr = Ipv4Address::new(192, 168, 1, 1);
        assert_eq!(format!("{}", addr), "192.168.1.1");
        
        let addr2 = Ipv4Address::new(0, 0, 0, 0);
        assert_eq!(format!("{}", addr2), "0.0.0.0");
        
        let addr3 = Ipv4Address::new(255, 255, 255, 255);
        assert_eq!(format!("{}", addr3), "255.255.255.255");
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    fn test_ipv4_boundary_values() {
        // All zeros
        let zero = Ipv4Address::new(0, 0, 0, 0);
        assert!(!zero.is_broadcast());
        assert!(!zero.is_multicast());
        assert!(!zero.is_loopback());
        
        // All ones
        let ones = Ipv4Address::new(255, 255, 255, 255);
        assert!(ones.is_broadcast());
        assert!(!ones.is_multicast()); // 255 is not in multicast range
    }

    #[test]
    fn test_multicast_boundary() {
        // Just below multicast range
        let below = Ipv4Address::new(223, 255, 255, 255);
        assert!(!below.is_multicast());
        
        // Start of multicast range
        let start = Ipv4Address::new(224, 0, 0, 0);
        assert!(start.is_multicast());
        
        // End of multicast range
        let end = Ipv4Address::new(239, 255, 255, 255);
        assert!(end.is_multicast());
        
        // Just above multicast range
        let above = Ipv4Address::new(240, 0, 0, 0);
        assert!(!above.is_multicast());
    }

    #[test]
    fn test_private_class_b_boundary() {
        // 172.15.x.x is NOT private
        assert!(!Ipv4Address::new(172, 15, 255, 255).is_private());
        
        // 172.16.x.x IS private (start)
        assert!(Ipv4Address::new(172, 16, 0, 0).is_private());
        
        // 172.31.x.x IS private (end)
        assert!(Ipv4Address::new(172, 31, 255, 255).is_private());
        
        // 172.32.x.x is NOT private
        assert!(!Ipv4Address::new(172, 32, 0, 0).is_private());
    }
}
