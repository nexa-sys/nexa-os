//! PID Tree and Allocator Tests
//!
//! Tests for the REAL radix tree-based PID allocation and management system.
//! Uses the actual kernel pid_tree module, not simulated logic.

#[cfg(test)]
mod tests {
    use crate::process::pid_tree::{
        allocate_pid, free_pid, is_pid_allocated, lookup_pid, register_pid_mapping,
        unregister_pid_mapping, allocated_pid_count, get_pid_stats,
        MAX_PID, MIN_PID,
    };
    use serial_test::serial;

    /// Helper to reset PID allocator state between tests.
    /// NOTE: In production, PIDs are never fully reset - this is test-only.
    fn reset_pid_state() {
        // Free a range of test PIDs to ensure clean state
        for pid in 1000..1100 {
            let _ = free_pid(pid);
        }
    }

    // =========================================================================
    // PID Constants Tests (no state, safe to run in parallel)
    // =========================================================================

    #[test]
    fn test_pid_constants() {
        // MIN_PID should be 1 (PID 0 is reserved for kernel/idle)
        assert_eq!(MIN_PID, 1);
        // MAX_PID should be 2^18 - 1 = 262143
        assert_eq!(MAX_PID, (1 << 18) - 1);
        assert_eq!(MAX_PID, 262143);
    }

    #[test]
    fn test_pid_range_is_reasonable() {
        // Should support at least 32768 PIDs (like traditional Unix)
        assert!(MAX_PID >= 32767);
        // But not more than a million for sanity
        assert!(MAX_PID < 1_000_000);
    }

    // =========================================================================
    // PID Allocation Tests (using REAL kernel code)
    // =========================================================================

    #[test]
    #[serial]
    fn test_allocate_pid() {
        reset_pid_state();
        
        // Allocate a PID
        let pid = allocate_pid();
        
        // Should be a valid PID
        assert!(pid >= MIN_PID, "Allocated PID {} should be >= MIN_PID", pid);
        assert!(pid <= MAX_PID, "Allocated PID {} should be <= MAX_PID", pid);
        
        // Should be marked as allocated
        assert!(is_pid_allocated(pid), "Newly allocated PID should be marked allocated");
        
        // Free it for cleanup
        free_pid(pid);
    }

    #[test]
    #[serial]
    fn test_allocate_multiple_pids() {
        reset_pid_state();
        
        let mut pids = Vec::new();
        
        // Allocate several PIDs
        for _ in 0..10 {
            let pid = allocate_pid();
            assert!(!pids.contains(&pid), "Should not allocate duplicate PIDs");
            pids.push(pid);
        }
        
        // All should be allocated
        for &pid in &pids {
            assert!(is_pid_allocated(pid));
        }
        
        // Cleanup
        for pid in pids {
            free_pid(pid);
        }
    }

    #[test]
    #[serial]
    fn test_free_and_reuse_pid() {
        reset_pid_state();
        
        // Allocate a PID
        let pid1 = allocate_pid();
        assert!(is_pid_allocated(pid1));
        
        // Free it
        free_pid(pid1);
        assert!(!is_pid_allocated(pid1), "Freed PID should not be allocated");
        
        // Allocate again - might get the same PID back (implementation dependent)
        // but should get a valid PID
        let pid2 = allocate_pid();
        assert!(pid2 >= MIN_PID);
        assert!(is_pid_allocated(pid2));
        
        // Cleanup
        free_pid(pid2);
    }

    #[test]
    #[serial]
    fn test_pid_zero_always_allocated() {
        // PID 0 is reserved for kernel/idle and should always be allocated
        assert!(is_pid_allocated(0), "PID 0 should always be marked allocated");
    }

    #[test]
    #[serial]
    fn test_free_pid_zero_fails() {
        // Attempting to free PID 0 should not change its state
        free_pid(0);
        assert!(is_pid_allocated(0), "PID 0 should remain allocated after free attempt");
    }

    // =========================================================================
    // PID Lookup (Radix Tree) Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_register_and_lookup_pid() {
        reset_pid_state();
        
        let pid = allocate_pid();
        let table_idx: u16 = 42;
        
        // Register mapping
        let registered = register_pid_mapping(pid, table_idx);
        assert!(registered, "Should be able to register PID mapping");
        
        // Lookup should return the index
        let found_idx = lookup_pid(pid);
        assert_eq!(found_idx, Some(table_idx), "Lookup should return registered index");
        
        // Cleanup
        unregister_pid_mapping(pid);
        free_pid(pid);
    }

    #[test]
    #[serial]
    fn test_unregister_pid_mapping() {
        reset_pid_state();
        
        let pid = allocate_pid();
        let table_idx: u16 = 99;
        
        register_pid_mapping(pid, table_idx);
        assert_eq!(lookup_pid(pid), Some(table_idx));
        
        // Unregister
        unregister_pid_mapping(pid);
        
        // Lookup should now return None
        assert_eq!(lookup_pid(pid), None, "Unregistered PID should not be found");
        
        free_pid(pid);
    }

    #[test]
    #[serial]
    fn test_lookup_unallocated_pid() {
        // Looking up a PID that was never allocated should return None
        let result = lookup_pid(99999);
        assert_eq!(result, None);
    }

    // =========================================================================
    // Statistics Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_allocated_count() {
        reset_pid_state();
        
        let initial_count = allocated_pid_count();
        
        // Allocate some PIDs
        let pid1 = allocate_pid();
        let pid2 = allocate_pid();
        let pid3 = allocate_pid();
        
        let after_alloc = allocated_pid_count();
        assert!(after_alloc >= initial_count + 3, 
            "Count should increase by at least 3");
        
        // Free them
        free_pid(pid1);
        free_pid(pid2);
        free_pid(pid3);
        
        let after_free = allocated_pid_count();
        assert!(after_free <= after_alloc, "Count should decrease after freeing");
    }

    #[test]
    #[serial]
    fn test_get_pid_stats() {
        reset_pid_state();
        
        let (allocated, total_capacity) = get_pid_stats();
        
        // Should have some allocated (at least PID 0)
        assert!(allocated >= 1, "At least PID 0 should be allocated");
        
        // Total capacity should be reasonable
        assert!(total_capacity > 0);
        assert!(total_capacity as u64 <= MAX_PID + 1);
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_double_free_is_safe() {
        reset_pid_state();
        
        let pid = allocate_pid();
        free_pid(pid);
        
        // Double free should not panic or cause issues
        free_pid(pid);
        
        // PID should still be free
        assert!(!is_pid_allocated(pid));
    }

    #[test]
    #[serial]
    fn test_free_never_allocated_pid() {
        reset_pid_state();
        
        // Freeing a PID that was never allocated should be safe
        free_pid(50000);
        // Should not panic
    }

    #[test]
    fn test_is_pid_allocated_out_of_range() {
        // PIDs beyond MAX_PID should return false
        assert!(!is_pid_allocated(MAX_PID + 1));
        assert!(!is_pid_allocated(MAX_PID + 1000));
        assert!(!is_pid_allocated(u64::MAX));
    }

    #[test]
    #[serial]
    fn test_allocate_free_cycle() {
        reset_pid_state();
        
        // Stress test: allocate and free in cycles
        for _ in 0..100 {
            let pid = allocate_pid();
            assert!(is_pid_allocated(pid));
            free_pid(pid);
            assert!(!is_pid_allocated(pid));
        }
    }

    #[test]
    #[serial]
    fn test_bulk_allocation() {
        reset_pid_state();
        
        let mut pids = Vec::new();
        
        // Allocate many PIDs
        for _ in 0..50 {
            let pid = allocate_pid();
            pids.push(pid);
        }
        
        // All should be unique
        let mut sorted = pids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), pids.len(), "All PIDs should be unique");
        
        // Cleanup
        for pid in pids {
            free_pid(pid);
        }
    }
}
