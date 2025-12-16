//! Full kernel tests
//!
//! Tests the complete kernel including memory allocator, kmod, scheduler, etc.

// =============================================================================
// Memory Allocator Tests
// =============================================================================

mod allocator_tests {
    use crate::mm::allocator::BuddyStats;

    // Note: BuddyAllocator::new() is private, so we can only test through
    // the global allocator interface or the public init functions
    
    #[test]
    fn test_buddy_stats_default() {
        let stats = BuddyStats {
            pages_allocated: 0,
            pages_free: 0,
            allocations: 0,
            frees: 0,
            splits: 0,
            merges: 0,
        };
        assert_eq!(stats.pages_allocated, 0);
        assert_eq!(stats.pages_free, 0);
    }
}

// =============================================================================
// Kmod Crypto Tests  
// =============================================================================

mod kmod_crypto_tests {
    use crate::kmod::crypto::Sha256;

    #[test]
    fn test_sha256_empty() {
        let mut hasher = Sha256::new();
        let digest = hasher.finalize();
        
        // SHA256 of empty string
        let expected: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
            0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
            0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
            0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
        ];
        assert_eq!(digest, expected);
    }

    #[test]
    fn test_sha256_hello() {
        let mut hasher = Sha256::new();
        hasher.update(b"hello");
        let digest = hasher.finalize();
        
        // SHA256 of "hello"
        let expected: [u8; 32] = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e,
            0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9, 0xe2, 0x9e,
            0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e,
            0x73, 0x04, 0x33, 0x62, 0x93, 0x8b, 0x98, 0x24,
        ];
        assert_eq!(digest, expected);
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
        let mut fresh = Sha256::new();
        let expected = fresh.finalize();
        
        assert_eq!(digest, expected);
    }
}

// =============================================================================
// Network Tests
// =============================================================================

mod net_tests {
    use crate::net::ethernet::MacAddress;
    use crate::net::ipv4::Ipv4Address;
    use crate::net::arp::ArpCache;

    #[test]
    fn test_mac_address() {
        let mac = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert!(!mac.is_broadcast());
        assert!(mac.is_unicast());

        let broadcast = MacAddress::BROADCAST;
        assert!(broadcast.is_broadcast());
    }

    #[test]
    fn test_mac_address_equality() {
        let mac1 = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let mac2 = MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let mac3 = MacAddress::new([0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        
        assert_eq!(mac1, mac2);
        assert_ne!(mac1, mac3);
    }

    #[test]
    fn test_ipv4_address() {
        let addr = Ipv4Address::new(192, 168, 1, 1);
        assert!(addr.is_private());
        assert!(!addr.is_loopback());
        
        let loopback = Ipv4Address::LOOPBACK;
        assert!(loopback.is_loopback());
    }

    #[test]
    fn test_ipv4_address_classes() {
        let private_10 = Ipv4Address::new(10, 0, 0, 1);
        let private_172 = Ipv4Address::new(172, 16, 0, 1);
        let private_192 = Ipv4Address::new(192, 168, 0, 1);
        let public = Ipv4Address::new(8, 8, 8, 8);
        
        assert!(private_10.is_private());
        assert!(private_172.is_private());
        assert!(private_192.is_private());
        assert!(!public.is_private());
    }

    #[test]
    fn test_arp_cache() {
        let mut cache = ArpCache::new();
        let ip = Ipv4Address::new(192, 168, 1, 1);
        let mac = MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);

        cache.insert(ip, mac, 1000);
        
        // Should find the entry (not expired)
        assert_eq!(cache.lookup(&ip, 1000), Some(mac));
        
        // Entry should expire after 60 seconds (60000ms)
        assert_eq!(cache.lookup(&ip, 1000 + 60_001), None);
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
        // Should have bits set for all CPUs
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
        // No pending signals initially
        assert!(state.has_pending_signal().is_none());
    }

    #[test]
    fn test_signal_send_and_check() {
        let mut state = SignalState::new();
        
        state.send_signal(SIGUSR1).unwrap();
        
        // Should have pending signal
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
        
        // Signal is pending but blocked, so has_pending_signal should return None
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
        assert_eq!(layout.size(), 40); // 10 * 4 bytes
        assert_eq!(layout.align(), 4);
    }

    #[test]
    fn test_layout_array_zero() {
        let layout = layout_array::<u64>(0).unwrap();
        assert_eq!(layout.size(), 0);
    }
}
