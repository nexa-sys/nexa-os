//! Process tests

use crate::process::{ProcessState, Context};
use crate::process::pid_tree;

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

// PID tree tests
mod pid_tree_tests {
    use super::*;
    
    #[test]
    fn test_pid_allocation() {
        // Allocate several PIDs
        let pid1 = pid_tree::allocate_pid();
        let pid2 = pid_tree::allocate_pid();
        let pid3 = pid_tree::allocate_pid();
        
        // PIDs should be unique
        assert_ne!(pid1, pid2);
        assert_ne!(pid2, pid3);
        assert_ne!(pid1, pid3);
        
        // PIDs should be valid (> 0)
        assert!(pid1 > 0);
        assert!(pid2 > 0);
        assert!(pid3 > 0);
        
        // Clean up
        pid_tree::free_pid(pid1);
        pid_tree::free_pid(pid2);
        pid_tree::free_pid(pid3);
    }
    
    #[test]
    fn test_pid_mapping() {
        let pid = pid_tree::allocate_pid();
        
        // Register mapping
        let result = pid_tree::register_pid_mapping(pid, 42);
        assert!(result, "Should successfully register PID mapping");
        
        // Look up mapping
        let idx = pid_tree::lookup_pid(pid);
        assert_eq!(idx, Some(42), "Should find correct process table index");
        
        // Clean up
        pid_tree::free_pid(pid);
    }
    
    #[test]
    fn test_pid_free_reuse() {
        // Allocate and free a PID
        let pid = pid_tree::allocate_pid();
        pid_tree::free_pid(pid);
        
        // PID should be available for reuse (though not guaranteed immediately)
        assert!(!pid_tree::is_pid_allocated(pid), 
            "Freed PID should not be allocated");
    }
    
    #[test]
    fn test_pid_lookup_nonexistent() {
        // Looking up unregistered PID should return None
        let result = pid_tree::lookup_pid(999999);
        assert_eq!(result, None, "Nonexistent PID should return None");
    }
    
    #[test]
    fn test_specific_pid_allocation() {
        // Allocate a specific PID
        let target_pid = 1000u64;
        
        // First ensure it's not allocated
        if pid_tree::is_pid_allocated(target_pid) {
            pid_tree::free_pid(target_pid);
        }
        
        let result = pid_tree::allocate_specific_pid(target_pid);
        assert!(result, "Should allocate specific PID");
        assert!(pid_tree::is_pid_allocated(target_pid));
        
        // Clean up
        pid_tree::free_pid(target_pid);
    }
    
    #[test]
    fn test_pid_count() {
        let initial_count = pid_tree::allocated_pid_count();
        
        let pid1 = pid_tree::allocate_pid();
        let pid2 = pid_tree::allocate_pid();
        
        assert_eq!(pid_tree::allocated_pid_count(), initial_count + 2,
            "Allocation count should increase by 2");
        
        pid_tree::free_pid(pid1);
        pid_tree::free_pid(pid2);
        
        assert_eq!(pid_tree::allocated_pid_count(), initial_count,
            "Allocation count should return to initial");
    }
    
    #[test]
    fn test_pid_update_mapping() {
        let pid = pid_tree::allocate_pid();
        
        // Register initial mapping
        pid_tree::register_pid_mapping(pid, 10);
        assert_eq!(pid_tree::lookup_pid(pid), Some(10));
        
        // Update mapping
        pid_tree::update_pid_mapping(pid, 20);
        assert_eq!(pid_tree::lookup_pid(pid), Some(20));
        
        // Clean up
        pid_tree::free_pid(pid);
    }
    
    #[test]
    fn test_pid_stats() {
        let (allocated, _nodes) = pid_tree::get_pid_stats();
        assert!(allocated >= 1, "Should have at least PID 0 allocated");
    }
}
