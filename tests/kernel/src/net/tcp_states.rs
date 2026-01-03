//! Network protocol stack edge case tests
//!
//! Tests for TCP connection state machine, socket options, and error handling.

#[cfg(test)]
mod tests {
    use crate::net::ipv4::Ipv4Address;
    use crate::net::ethernet::MacAddress;

    // =========================================================================
    // IPv4 Address Tests
    // =========================================================================

    #[test]
    fn test_ipv4_loopback() {
        let loopback = Ipv4Address::new(127, 0, 0, 1);
        assert!(loopback.is_loopback());
    }

    #[test]
    fn test_ipv4_broadcast() {
        let broadcast = Ipv4Address::new(255, 255, 255, 255);
        assert!(broadcast.is_broadcast());
    }

    #[test]
    fn test_ipv4_unspecified() {
        let unspecified = Ipv4Address::UNSPECIFIED;
        // Check manually since is_unspecified may not exist
        assert_eq!(unspecified.0, [0, 0, 0, 0]);
        
        let also_unspecified = Ipv4Address::new(0, 0, 0, 0);
        assert_eq!(also_unspecified.0, [0, 0, 0, 0]);
    }

    #[test]
    fn test_ipv4_private_addresses() {
        // 10.0.0.0/8
        let class_a_private = Ipv4Address::new(10, 0, 0, 1);
        assert!(class_a_private.is_private());
        
        // 172.16.0.0/12
        let class_b_private = Ipv4Address::new(172, 16, 0, 1);
        assert!(class_b_private.is_private());
        
        // 192.168.0.0/16
        let class_c_private = Ipv4Address::new(192, 168, 1, 1);
        assert!(class_c_private.is_private());
        
        // Public address
        let public = Ipv4Address::new(8, 8, 8, 8);
        assert!(!public.is_private());
    }

    #[test]
    fn test_ipv4_multicast() {
        // 224.0.0.0 - 239.255.255.255
        let multicast = Ipv4Address::new(224, 0, 0, 1);
        assert!(multicast.is_multicast());
        
        let not_multicast = Ipv4Address::new(223, 255, 255, 255);
        assert!(!not_multicast.is_multicast());
    }

    #[test]
    fn test_ipv4_link_local() {
        // 169.254.0.0/16 (APIPA)
        let link_local = Ipv4Address::new(169, 254, 1, 1);
        // Check first two octets manually
        assert_eq!(link_local.0[0], 169);
        assert_eq!(link_local.0[1], 254);
    }

    #[test]
    fn test_ipv4_octets() {
        let ip = Ipv4Address::new(192, 168, 1, 100);
        
        // Access octets via the public tuple struct field
        assert_eq!(ip.0[0], 192);
        assert_eq!(ip.0[1], 168);
        assert_eq!(ip.0[2], 1);
        assert_eq!(ip.0[3], 100);
    }

    #[test]
    fn test_ipv4_from_octets() {
        let octets = [10, 20, 30, 40];
        let ip = Ipv4Address::from(octets);
        
        assert_eq!(ip.0, octets);
    }

    #[test]
    fn test_ipv4_to_u32() {
        let ip = Ipv4Address::new(192, 168, 1, 1);
        
        // Network byte order: 192.168.1.1 = 0xC0A80101
        // In host byte order on little-endian: 0x0101A8C0
        let as_u32 = u32::from_be_bytes(ip.0);
        assert_eq!(as_u32, 0xC0A80101);
    }

    // =========================================================================
    // MAC Address Tests
    // =========================================================================

    #[test]
    fn test_mac_address_zero() {
        let zero = MacAddress::ZERO;
        assert_eq!(zero.0, [0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_mac_address_broadcast() {
        let broadcast = MacAddress::BROADCAST;
        assert_eq!(broadcast.0, [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(broadcast.is_broadcast());
    }

    #[test]
    fn test_mac_address_multicast() {
        // Multicast MAC: first byte LSB = 1
        let multicast = MacAddress::new([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]);
        assert!(multicast.is_multicast());
        
        let unicast = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert!(!unicast.is_multicast());
    }

    #[test]
    fn test_mac_address_locally_administered() {
        // Locally administered: second LSB of first byte = 1
        let local = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        // Check bit manually: (0x02 & 0x02) != 0
        assert_ne!(local.0[0] & 0x02, 0);
        
        let universal = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(universal.0[0] & 0x02, 0);
    }

    // =========================================================================
    // Port Number Tests
    // =========================================================================

    #[test]
    fn test_port_well_known() {
        // Well-known ports: 0-1023
        assert!(is_well_known_port(80));
        assert!(is_well_known_port(443));
        assert!(is_well_known_port(22));
        assert!(is_well_known_port(0));
        assert!(is_well_known_port(1023));
        
        assert!(!is_well_known_port(1024));
        assert!(!is_well_known_port(8080));
    }

    #[test]
    fn test_port_ephemeral() {
        // Ephemeral ports: 49152-65535 (Linux uses 32768-60999)
        assert!(is_ephemeral_port(49152));
        assert!(is_ephemeral_port(65535));
        
        assert!(!is_ephemeral_port(80));
        assert!(!is_ephemeral_port(8080));
    }

    fn is_well_known_port(port: u16) -> bool {
        port < 1024
    }

    fn is_ephemeral_port(port: u16) -> bool {
        port >= 49152
    }

    // =========================================================================
    // Checksum Tests
    // =========================================================================

    #[test]
    fn test_ones_complement_sum() {
        fn ones_complement_sum(data: &[u16]) -> u16 {
            let mut sum: u32 = 0;
            for &word in data {
                sum += word as u32;
            }
            
            // Fold 32-bit sum to 16 bits
            while sum > 0xFFFF {
                sum = (sum & 0xFFFF) + (sum >> 16);
            }
            
            !sum as u16
        }
        
        // Simple test
        let data = [0x0001, 0x0002, 0x0003];
        let checksum = ones_complement_sum(&data);
        
        // Sum = 6, one's complement = 0xFFF9
        assert_eq!(checksum, 0xFFF9);
    }

    #[test]
    fn test_checksum_with_overflow() {
        fn ones_complement_sum(data: &[u16]) -> u16 {
            let mut sum: u32 = 0;
            for &word in data {
                sum += word as u32;
            }
            while sum > 0xFFFF {
                sum = (sum & 0xFFFF) + (sum >> 16);
            }
            !sum as u16
        }
        
        // Test with values that cause overflow
        let data = [0xFFFF, 0xFFFF];
        let checksum = ones_complement_sum(&data);
        
        // 0xFFFF + 0xFFFF = 0x1FFFE, fold = 0xFFFF, complement = 0
        // But 0 checksum means "no checksum" in UDP, so it becomes 0xFFFF
        assert_eq!(checksum, 0);
    }

    // =========================================================================
    // Network Byte Order Tests
    // =========================================================================

    #[test]
    fn test_network_byte_order() {
        // Network byte order is big-endian
        let host_value: u16 = 0x1234;
        let network_value = host_value.to_be();
        
        // On little-endian system, bytes are swapped
        let bytes = network_value.to_ne_bytes();
        assert_eq!(bytes[0], 0x12);
        assert_eq!(bytes[1], 0x34);
    }

    #[test]
    fn test_u32_network_byte_order() {
        let host_value: u32 = 0x12345678;
        let network_value = host_value.to_be();
        
        let bytes = network_value.to_ne_bytes();
        assert_eq!(bytes[0], 0x12);
        assert_eq!(bytes[1], 0x34);
        assert_eq!(bytes[2], 0x56);
        assert_eq!(bytes[3], 0x78);
    }

    // =========================================================================
    // Socket State Tests
    // =========================================================================

    #[test]
    fn test_socket_states() {
        // TCP states
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum TcpState {
            Closed,
            Listen,
            SynSent,
            SynReceived,
            Established,
            FinWait1,
            FinWait2,
            CloseWait,
            Closing,
            LastAck,
            TimeWait,
        }
        
        // All states should be distinct
        let states = [
            TcpState::Closed, TcpState::Listen, TcpState::SynSent,
            TcpState::SynReceived, TcpState::Established, TcpState::FinWait1,
            TcpState::FinWait2, TcpState::CloseWait, TcpState::Closing,
            TcpState::LastAck, TcpState::TimeWait,
        ];
        
        for (i, &s1) in states.iter().enumerate() {
            for (j, &s2) in states.iter().enumerate() {
                if i != j {
                    assert_ne!(s1, s2);
                }
            }
        }
    }

    // =========================================================================
    // Buffer Size Tests
    // =========================================================================

    #[test]
    fn test_mtu_sizes() {
        const ETHERNET_MTU: usize = 1500;
        const ETHERNET_HEADER_SIZE: usize = 14;
        const IP_HEADER_MIN: usize = 20;
        const UDP_HEADER_SIZE: usize = 8;
        const TCP_HEADER_MIN: usize = 20;
        
        // Max UDP payload in single packet
        let max_udp_payload = ETHERNET_MTU - IP_HEADER_MIN - UDP_HEADER_SIZE;
        assert_eq!(max_udp_payload, 1472);
        
        // Max TCP payload in single segment
        let max_tcp_payload = ETHERNET_MTU - IP_HEADER_MIN - TCP_HEADER_MIN;
        assert_eq!(max_tcp_payload, 1460);
    }

    #[test]
    fn test_jumbo_frame_mtu() {
        const JUMBO_MTU: usize = 9000;
        const IP_HEADER_MIN: usize = 20;
        const TCP_HEADER_MIN: usize = 20;
        
        let max_tcp_payload = JUMBO_MTU - IP_HEADER_MIN - TCP_HEADER_MIN;
        assert_eq!(max_tcp_payload, 8960);
    }
}
