//! Full kernel tests - moved from src/
//!
//! All #[cfg(test)] modules from kernel source have been moved here.
//! Tests the complete kernel including:
//! - Memory allocator (BuddyAllocator, SlabAllocator)
//! - Network stack (Ethernet, IPv4, ARP, UDP)
//! - Kernel modules (kmod, crypto, pkcs7)
//! - Filesystem (fstab parsing)
//! - Scheduler (CpuMask, SchedPolicy)
//! - IPC (signals)
//! - Process management

// =============================================================================
// Network: Ethernet Tests (from src/net/ethernet.rs)
// =============================================================================

mod ethernet_tests {
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
}

// =============================================================================
// Network: IPv4 Tests (from src/net/ipv4.rs)
// =============================================================================

mod ipv4_tests {
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
}

// =============================================================================
// Network: ARP Tests (from src/net/arp.rs)
// =============================================================================

mod arp_tests {
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
}

// =============================================================================
// Network: UDP Tests (from src/net/udp.rs)
// =============================================================================

mod udp_tests {
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
}

// =============================================================================
// Network: UDP Helper Tests (from src/net/udp_helper.rs)
// =============================================================================

mod udp_helper_tests {
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
}

// =============================================================================
// Kmod: Crypto Tests (from src/kmod/crypto.rs)
// =============================================================================

mod kmod_crypto_tests {
    use crate::kmod::crypto::{sha256, Sha256};

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        let expected: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha256_hello() {
        let hash = sha256(b"hello");
        let expected: [u8; 32] = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9,
            0xe2, 0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62,
            0x93, 0x8b, 0x98, 0x24,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha256_incremental() {
        let mut hasher1 = Sha256::new();
        hasher1.update(b"hello world");
        let digest1 = hasher1.finalize();

        let mut hasher2 = Sha256::new();
        hasher2.update(b"hello ");
        hasher2.update(b"world");
        let digest2 = hasher2.finalize();

        assert_eq!(digest1, digest2);
    }

    #[test]
    fn test_sha256_reset() {
        let mut hasher = Sha256::new();
        hasher.update(b"garbage");
        hasher.reset();
        let digest = hasher.finalize();

        // Should be same as empty
        let expected = sha256(b"");
        assert_eq!(digest, expected);
    }
}

// =============================================================================
// Kmod: Module Tests (from src/kmod/mod.rs)
// =============================================================================

mod kmod_tests {
    use crate::kmod::{generate_nkm, NkmHeader, ModuleType};

    #[test]
    fn test_generate_and_parse_nkm() {
        let data = generate_nkm(
            "ext2",
            ModuleType::Filesystem,
            "1.0.0",
            "ext2 filesystem driver",
        );
        let header = NkmHeader::parse(&data).expect("parse failed");
        assert_eq!(header.name_str(), "ext2");
        assert_eq!(header.module_type(), ModuleType::Filesystem);
    }
}

// =============================================================================
// Kmod: PKCS7 Tests (from src/kmod/pkcs7.rs)
// =============================================================================

mod pkcs7_tests {
    use crate::kmod::pkcs7::{ModuleSigInfo, verify_module_signature, SignatureVerifyResult};
    use crate::kmod::crypto::HashAlgorithm;

    #[test]
    fn test_sig_info_parse() {
        let data = [
            0x00, // algo
            0x04, // hash (SHA256)
            0x01, // key_type (RSA)
            0x01, // signer_id_type
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x01, 0x00, // sig_len (256)
        ];

        let info = ModuleSigInfo::from_bytes(&data).unwrap();
        assert_eq!(info.signature_len(), 256);
        assert_eq!(info.hash_algo(), Some(HashAlgorithm::Sha256));
    }

    #[test]
    fn test_unsigned_module() {
        let module_data = b"fake module data without signature";
        let result = verify_module_signature(module_data);
        assert_eq!(result, SignatureVerifyResult::Unsigned);
    }
}

// =============================================================================
// Filesystem: fstab Tests (from src/fs/fstab.rs)
// =============================================================================

mod fstab_tests {
    use crate::fs::fstab::parse_fstab;

    #[test]
    fn test_parse_fstab() {
        let content = r#"
# Comment line
/dev/vda1   /       ext2    defaults    0   1
tmpfs       /tmp    tmpfs   size=64M    0   0
"#;
        let entries = parse_fstab(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].device, "/dev/vda1");
        assert_eq!(entries[0].mount_point, "/");
        assert_eq!(entries[1].fs_type, "tmpfs");
    }
}

// =============================================================================
// Scheduler Tests
// =============================================================================

mod scheduler_tests {
    use crate::scheduler::{CpuMask, SchedPolicy};

    #[test]
    fn test_cpu_mask_empty() {
        let mask = CpuMask::empty();
        assert!(mask.is_empty());
        assert_eq!(mask.count(), 0);
    }

    #[test]
    fn test_cpu_mask_operations() {
        let mut mask = CpuMask::empty();

        mask.set(5);
        assert!(mask.is_set(5));
        assert!(!mask.is_set(4));
        assert_eq!(mask.count(), 1);

        mask.set(10);
        assert_eq!(mask.count(), 2);

        mask.clear(5);
        assert!(!mask.is_set(5));
        assert_eq!(mask.count(), 1);
    }

    #[test]
    fn test_cpu_mask_all() {
        let mask = CpuMask::all();
        assert!(!mask.is_empty());
        for i in 0..64 {
            assert!(mask.is_set(i));
        }
    }

    #[test]
    fn test_sched_policy_equality() {
        assert_ne!(SchedPolicy::Normal, SchedPolicy::Batch);
        assert_ne!(SchedPolicy::Batch, SchedPolicy::Idle);
        assert_eq!(SchedPolicy::Normal, SchedPolicy::Normal);
    }
}

// =============================================================================
// IPC Signal Tests
// =============================================================================

mod signal_tests {
    use crate::ipc::signal::{SignalState, SignalAction, SIGKILL, SIGTERM, SIGUSR1, SIGSTOP};

    #[test]
    fn test_signal_state_new() {
        let state = SignalState::new();
        assert!(state.has_pending_signal().is_none());
    }

    #[test]
    fn test_signal_send_and_check() {
        let mut state = SignalState::new();
        state.send_signal(SIGUSR1).unwrap();
        let pending = state.has_pending_signal();
        assert_eq!(pending, Some(SIGUSR1));
    }

    #[test]
    fn test_signal_clear() {
        let mut state = SignalState::new();
        state.send_signal(SIGUSR1).unwrap();
        assert!(state.has_pending_signal().is_some());
        state.clear_signal(SIGUSR1);
        assert!(state.has_pending_signal().is_none());
    }

    #[test]
    fn test_signal_blocking() {
        let mut state = SignalState::new();
        state.block_signal(SIGUSR1);
        state.send_signal(SIGUSR1).unwrap();
        // Signal is pending but blocked
        assert!(state.has_pending_signal().is_none());
        state.unblock_signal(SIGUSR1);
        assert_eq!(state.has_pending_signal(), Some(SIGUSR1));
    }

    #[test]
    fn test_signal_action_cannot_change_sigkill() {
        let mut state = SignalState::new();
        let result = state.set_action(SIGKILL, SignalAction::Ignore);
        assert!(result.is_err());
        let result = state.set_action(SIGSTOP, SignalAction::Ignore);
        assert!(result.is_err());
    }

    #[test]
    fn test_signal_action_change() {
        let mut state = SignalState::new();
        let old = state.set_action(SIGTERM, SignalAction::Ignore).unwrap();
        assert_eq!(old, SignalAction::Default);
        let current = state.get_action(SIGTERM).unwrap();
        assert_eq!(current, SignalAction::Ignore);
    }
}

// =============================================================================
// Process Tests
// =============================================================================

mod process_tests {
    use crate::process::{ProcessState, Context};

    #[test]
    fn test_process_state_comparison() {
        assert_ne!(ProcessState::Ready, ProcessState::Running);
        assert_ne!(ProcessState::Running, ProcessState::Sleeping);
        assert_ne!(ProcessState::Sleeping, ProcessState::Zombie);
    }

    #[test]
    fn test_context_zero() {
        let ctx = Context::zero();
        assert_eq!(ctx.rax, 0);
        assert_eq!(ctx.rbx, 0);
        assert_eq!(ctx.rcx, 0);
        assert_eq!(ctx.rdx, 0);
        assert_eq!(ctx.rip, 0);
        // IF flag should be set (0x200)
        assert_eq!(ctx.rflags & 0x200, 0x200);
    }
}

// =============================================================================
// Safety Module Tests
// =============================================================================

mod safety_tests {
    use crate::safety::{layout_of, layout_array};

    #[test]
    fn test_layout_of() {
        let layout = layout_of::<u64>();
        assert_eq!(layout.size(), 8);
        assert_eq!(layout.align(), 8);
    }

    #[test]
    fn test_layout_array() {
        let layout = layout_array::<u32>(10).unwrap();
        assert_eq!(layout.size(), 40);
        assert_eq!(layout.align(), 4);
    }

    #[test]
    fn test_layout_array_zero() {
        let layout = layout_array::<u64>(0).unwrap();
        assert_eq!(layout.size(), 0);
    }
}

// =============================================================================
// Memory Allocator Tests
// =============================================================================

mod allocator_tests {
    use crate::mm::allocator::BuddyStats;

    #[test]
    fn test_buddy_stats_fields() {
        let stats = BuddyStats {
            pages_allocated: 10,
            pages_free: 90,
            allocations: 5,
            frees: 3,
            splits: 2,
            merges: 1,
        };
        assert_eq!(stats.pages_allocated, 10);
        assert_eq!(stats.pages_free, 90);
        assert_eq!(stats.allocations, 5);
    }
}
