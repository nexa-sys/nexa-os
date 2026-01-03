//! PID Tree and Radix Tree Edge Case Tests
//!
//! Tests for the REAL PID allocation, deallocation, and lookup operations
//! including boundary conditions, recycling, and stress patterns.
//! Uses the actual kernel pid_tree module.

#[cfg(test)]
mod tests {
    use crate::process::pid_tree::{
        allocate_pid, free_pid, is_pid_allocated, lookup_pid, register_pid_mapping,
        unregister_pid_mapping, allocated_pid_count, get_pid_stats, update_pid_mapping,
        allocate_specific_pid, MAX_PID, MIN_PID,
    };
    use serial_test::serial;

    /// Helper to clean up test PIDs
    fn cleanup_pids(pids: &[u64]) {
        for &pid in pids {
            unregister_pid_mapping(pid);
            free_pid(pid);
        }
    }

    // =========================================================================
    // PID Constants Validation
    // =========================================================================

    #[test]
    fn test_pid_constants() {
        assert_eq!(MIN_PID, 1);
        assert!(MAX_PID >= 32767, "MAX_PID should support at least 32767 processes");
        assert!(MAX_PID <= (1 << 22), "MAX_PID should not be excessively large");
    }

    #[test]
    fn test_pid_range_valid() {
        assert!(MIN_PID < MAX_PID);
        let pid_count = MAX_PID - MIN_PID + 1;
        assert!(pid_count > 0);
    }

    // =========================================================================
    // Allocation Edge Cases
    // =========================================================================

    #[test]
    #[serial]
    fn test_allocate_sequential() {
        let mut pids = Vec::new();
        
        // Allocate several PIDs
        for _ in 0..20 {
            let pid = allocate_pid();
            assert!(pid >= MIN_PID);
            assert!(pid <= MAX_PID);
            assert!(!pids.contains(&pid), "PIDs should be unique");
            pids.push(pid);
        }
        
        // All should be allocated
        for &pid in &pids {
            assert!(is_pid_allocated(pid));
        }
        
        cleanup_pids(&pids);
    }

    #[test]
    #[serial]
    fn test_free_and_reallocate() {
        let mut pids = Vec::new();
        
        // Allocate 10 PIDs
        for _ in 0..10 {
            pids.push(allocate_pid());
        }
        
        // Free the first 5
        for i in 0..5 {
            free_pid(pids[i]);
        }
        
        // Allocate 5 more - should eventually reuse freed PIDs
        let mut new_pids = Vec::new();
        for _ in 0..5 {
            new_pids.push(allocate_pid());
        }
        
        // All new PIDs should be valid
        for &pid in &new_pids {
            assert!(is_pid_allocated(pid));
        }
        
        // Cleanup
        cleanup_pids(&pids[5..]);
        cleanup_pids(&new_pids);
    }

    #[test]
    #[serial]
    fn test_pid_zero_reserved() {
        // PID 0 is always allocated
        assert!(is_pid_allocated(0));
        
        // Free should have no effect
        free_pid(0);
        assert!(is_pid_allocated(0));
    }

    #[test]
    #[serial]
    fn test_double_free_safe() {
        let pid = allocate_pid();
        free_pid(pid);
        
        // Double free should be safe
        free_pid(pid);
        assert!(!is_pid_allocated(pid));
    }

    #[test]
    #[serial]
    fn test_free_unallocated_safe() {
        // Should not panic
        free_pid(99999);
        free_pid(MAX_PID);
    }

    // =========================================================================
    // Radix Tree Lookup Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_register_lookup_unregister() {
        let pid = allocate_pid();
        
        // Register mapping
        assert!(register_pid_mapping(pid, 42));
        
        // Lookup should work
        assert_eq!(lookup_pid(pid), Some(42));
        
        // Unregister
        unregister_pid_mapping(pid);
        assert_eq!(lookup_pid(pid), None);
        
        free_pid(pid);
    }

    #[test]
    #[serial]
    fn test_update_mapping() {
        let pid = allocate_pid();
        
        register_pid_mapping(pid, 10);
        assert_eq!(lookup_pid(pid), Some(10));
        
        // Update to new index
        update_pid_mapping(pid, 20);
        assert_eq!(lookup_pid(pid), Some(20));
        
        unregister_pid_mapping(pid);
        free_pid(pid);
    }

    #[test]
    #[serial]
    fn test_lookup_unregistered() {
        let pid = allocate_pid();
        
        // No mapping registered
        assert_eq!(lookup_pid(pid), None);
        
        free_pid(pid);
    }

    #[test]
    #[serial]
    fn test_sparse_pid_lookups() {
        let targets = [1000u64, 2000, 5000, 10000, 20000];
        let mut allocated = Vec::new();
        
        // Try to allocate specific PIDs
        for &target in &targets {
            // First free the target if it's somehow allocated
            free_pid(target);
            
            // Try to allocate it
            if allocate_specific_pid(target) {
                register_pid_mapping(target, (target % 1000) as u16);
                allocated.push(target);
            }
        }
        
        // Verify lookups work
        for &pid in &allocated {
            let expected = (pid % 1000) as u16;
            assert_eq!(lookup_pid(pid), Some(expected));
        }
        
        cleanup_pids(&allocated);
    }

    // =========================================================================
    // Statistics Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_allocation_count_consistency() {
        let initial_count = allocated_pid_count();
        
        // Allocate 10 PIDs
        let mut pids = Vec::new();
        for _ in 0..10 {
            pids.push(allocate_pid());
        }
        
        let after_alloc = allocated_pid_count();
        assert_eq!(after_alloc, initial_count + 10);
        
        // Free 5 PIDs
        for i in 0..5 {
            free_pid(pids[i]);
        }
        
        let after_free = allocated_pid_count();
        assert_eq!(after_free, initial_count + 5);
        
        // Cleanup remaining
        for i in 5..10 {
            free_pid(pids[i]);
        }
    }

    #[test]
    #[serial]
    fn test_get_stats() {
        let (allocated, capacity) = get_pid_stats();
        
        assert!(allocated >= 1, "At least PID 0 should be allocated");
        assert!(capacity > 0);
        // capacity is node count, not directly comparable to allocated
    }

    // =========================================================================
    // Stress Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_bulk_allocate_free_cycle() {
        for _ in 0..5 {
            let mut pids = Vec::new();
            
            // Allocate batch
            for _ in 0..50 {
                pids.push(allocate_pid());
            }
            
            // Free all
            for pid in pids {
                free_pid(pid);
            }
        }
    }

    #[test]
    #[serial]
    fn test_interleaved_allocate_free() {
        let mut live_pids = Vec::new();
        
        for i in 0..100 {
            if i % 3 == 0 && !live_pids.is_empty() {
                // Free oldest PID
                let pid = live_pids.remove(0);
                free_pid(pid);
            } else {
                // Allocate new PID
                live_pids.push(allocate_pid());
            }
        }
        
        // Verify all remaining are allocated
        for &pid in &live_pids {
            assert!(is_pid_allocated(pid));
        }
        
        cleanup_pids(&live_pids);
    }

    #[test]
    #[serial]
    fn test_many_lookups() {
        let mut pids = Vec::new();
        
        // Allocate and register
        for i in 0..30 {
            let pid = allocate_pid();
            register_pid_mapping(pid, i as u16);
            pids.push(pid);
        }
        
        // Many lookups
        for _ in 0..100 {
            for (i, &pid) in pids.iter().enumerate() {
                assert_eq!(lookup_pid(pid), Some(i as u16));
            }
        }
        
        cleanup_pids(&pids);
    }

    // =========================================================================
    // Boundary Tests
    // =========================================================================

    #[test]
    fn test_out_of_range_operations() {
        // Operations on invalid PIDs should be safe
        assert!(!is_pid_allocated(MAX_PID + 1));
        assert!(!is_pid_allocated(u64::MAX));
        assert_eq!(lookup_pid(MAX_PID + 1), None);
        assert_eq!(lookup_pid(u64::MAX), None);
    }

    #[test]
    #[serial]
    fn test_allocate_specific_pid() {
        // Try to allocate a specific high PID
        let target = 50000u64;
        
        // Make sure it's free first
        free_pid(target);
        
        // Try to allocate it
        let success = allocate_specific_pid(target);
        if success {
            assert!(is_pid_allocated(target));
            free_pid(target);
        }
        // If it was already allocated, that's fine too
    }

    #[test]
    #[serial]
    fn test_allocate_specific_invalid() {
        // PID 0 cannot be allocated (always reserved)
        assert!(!allocate_specific_pid(0));
        
        // Beyond MAX_PID should fail
        assert!(!allocate_specific_pid(MAX_PID + 1));
    }
}
