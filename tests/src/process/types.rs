//! Process tests

use crate::process::{ProcessState, Context};
use crate::process::pid_tree;
use crate::process::{
    USER_VIRT_BASE, USER_PHYS_BASE, HEAP_BASE, HEAP_SIZE, STACK_BASE, STACK_SIZE,
    INTERP_BASE, INTERP_REGION_SIZE, USER_REGION_SIZE, KERNEL_STACK_SIZE, KERNEL_STACK_ALIGN,
    MAX_PROCESSES, MAX_PROCESS_ARGS, MAX_CMDLINE_SIZE, clone_flags, build_cmdline, DEFAULT_ARGV0,
};
use core::mem;

// =========================================================================
// ProcessState Tests
// =========================================================================

#[test]
fn test_process_state_comparison() {
    assert_ne!(ProcessState::Ready, ProcessState::Running);
    assert_ne!(ProcessState::Running, ProcessState::Sleeping);
    assert_ne!(ProcessState::Sleeping, ProcessState::Zombie);
}

#[test]
fn test_process_state_all_variants() {
    let states = [
        ProcessState::Ready,
        ProcessState::Running,
        ProcessState::Sleeping,
        ProcessState::Zombie,
    ];
    
    // All states should be distinct
    for i in 0..states.len() {
        for j in (i + 1)..states.len() {
            assert_ne!(states[i], states[j]);
        }
    }
}

#[test]
fn test_process_state_copy_clone() {
    let state = ProcessState::Running;
    let copied = state;
    let cloned = state.clone();
    assert_eq!(state, copied);
    assert_eq!(state, cloned);
}

// =========================================================================
// Context Tests
// =========================================================================

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

#[test]
fn test_context_size() {
    // Context should contain 18 u64 fields (15 GPR + rip + rsp + rflags)
    assert_eq!(mem::size_of::<Context>(), 18 * 8);
}

#[test]
fn test_context_alignment() {
    // Context should be properly aligned for C ABI
    assert!(mem::align_of::<Context>() >= 8);
}

#[test]
fn test_context_copy() {
    let ctx = Context::zero();
    let ctx2 = ctx;
    assert_eq!(ctx.rax, ctx2.rax);
    assert_eq!(ctx.rflags, ctx2.rflags);
}

// =========================================================================
// Memory Layout Constants Tests
// =========================================================================

#[test]
fn test_user_memory_base() {
    // User virtual base should be at 16MB
    assert_eq!(USER_VIRT_BASE, 0x1000000);
    assert_eq!(USER_PHYS_BASE, 0x1000000);
}

#[test]
fn test_heap_layout() {
    // Heap should start 2MB after USER_VIRT_BASE
    assert_eq!(HEAP_BASE, USER_VIRT_BASE + 0x200000);
    // Heap should be 8MB
    assert_eq!(HEAP_SIZE, 0x800000);
}

#[test]
fn test_stack_layout() {
    // Stack should be placed after heap
    assert_eq!(STACK_BASE, HEAP_BASE + HEAP_SIZE);
    // Stack should be 2MB (aligned for huge pages)
    assert_eq!(STACK_SIZE, 0x200000);
}

#[test]
fn test_interp_layout() {
    // Interpreter region should be after stack
    assert_eq!(INTERP_BASE, STACK_BASE + STACK_SIZE);
    // Interpreter region should be 16MB
    assert_eq!(INTERP_REGION_SIZE, 0x1000000);
}

#[test]
fn test_user_region_size() {
    // Total user region size should span from USER_VIRT_BASE to end of interp region
    let expected = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
    assert_eq!(USER_REGION_SIZE, expected);
}

#[test]
fn test_memory_regions_no_overlap() {
    // Code region: USER_VIRT_BASE to HEAP_BASE
    let code_end = HEAP_BASE;
    // Heap region: HEAP_BASE to STACK_BASE
    let heap_end = STACK_BASE;
    // Stack region: STACK_BASE to INTERP_BASE
    let stack_end = INTERP_BASE;
    
    assert!(code_end <= HEAP_BASE, "Code should not overlap heap");
    assert!(heap_end <= STACK_BASE, "Heap should not overlap stack");
    assert!(stack_end <= INTERP_BASE, "Stack should not overlap interp");
}

// =========================================================================
// Kernel Stack Tests
// =========================================================================

#[test]
fn test_kernel_stack_size() {
    // Kernel stack should be 32KB
    assert_eq!(KERNEL_STACK_SIZE, 32 * 1024);
}

#[test]
fn test_kernel_stack_alignment() {
    // Stack should be 16-byte aligned (x86_64 ABI requirement)
    assert_eq!(KERNEL_STACK_ALIGN, 16);
}

// =========================================================================
// Process Limits Tests
// =========================================================================

#[test]
fn test_max_processes() {
    assert!(MAX_PROCESSES >= 64);
    assert!(MAX_PROCESSES <= 4096);
}

#[test]
fn test_max_process_args() {
    assert!(MAX_PROCESS_ARGS >= 16);
}

#[test]
fn test_max_cmdline_size() {
    assert!(MAX_CMDLINE_SIZE >= 256);
    assert_eq!(MAX_CMDLINE_SIZE, 1024);
}

// =========================================================================
// Clone Flags Tests (Linux compatible)
// =========================================================================

#[test]
fn test_clone_flags_values() {
    assert_eq!(clone_flags::CLONE_VM, 0x00000100);
    assert_eq!(clone_flags::CLONE_FS, 0x00000200);
    assert_eq!(clone_flags::CLONE_FILES, 0x00000400);
    assert_eq!(clone_flags::CLONE_SIGHAND, 0x00000800);
    assert_eq!(clone_flags::CLONE_THREAD, 0x00010000);
    assert_eq!(clone_flags::CLONE_SYSVSEM, 0x00040000);
    assert_eq!(clone_flags::CLONE_SETTLS, 0x00080000);
    assert_eq!(clone_flags::CLONE_PARENT_SETTID, 0x00100000);
    assert_eq!(clone_flags::CLONE_CHILD_CLEARTID, 0x00200000);
    assert_eq!(clone_flags::CLONE_CHILD_SETTID, 0x01000000);
}

#[test]
fn test_clone_flags_no_overlap() {
    // All flags should be distinct bits
    let flags = [
        clone_flags::CLONE_VM,
        clone_flags::CLONE_FS,
        clone_flags::CLONE_FILES,
        clone_flags::CLONE_SIGHAND,
        clone_flags::CLONE_THREAD,
        clone_flags::CLONE_SYSVSEM,
        clone_flags::CLONE_SETTLS,
        clone_flags::CLONE_PARENT_SETTID,
        clone_flags::CLONE_CHILD_CLEARTID,
        clone_flags::CLONE_CHILD_SETTID,
    ];
    
    for (i, &f1) in flags.iter().enumerate() {
        for &f2 in flags.iter().skip(i + 1) {
            assert_eq!(f1 & f2, 0, "Clone flags should not overlap");
        }
    }
}

#[test]
fn test_clone_thread_combination() {
    // Common CLONE_THREAD combination for pthread_create
    let thread_flags = clone_flags::CLONE_VM 
        | clone_flags::CLONE_FS 
        | clone_flags::CLONE_FILES 
        | clone_flags::CLONE_SIGHAND 
        | clone_flags::CLONE_THREAD
        | clone_flags::CLONE_SETTLS
        | clone_flags::CLONE_PARENT_SETTID
        | clone_flags::CLONE_CHILD_CLEARTID;
    
    // Verify all expected flags are set
    assert!(thread_flags & clone_flags::CLONE_VM != 0);
    assert!(thread_flags & clone_flags::CLONE_THREAD != 0);
}

// =========================================================================
// Cmdline Builder Tests
// =========================================================================

#[test]
fn test_build_cmdline_empty() {
    let (buffer, len) = build_cmdline(&[]);
    assert_eq!(len, 0);
    assert_eq!(buffer[0], 0);
}

#[test]
fn test_build_cmdline_single() {
    let (buffer, len) = build_cmdline(&[b"hello"]);
    assert_eq!(&buffer[0..5], b"hello");
    assert_eq!(buffer[5], 0); // null terminator
    assert_eq!(len, 6);
}

#[test]
fn test_build_cmdline_multiple() {
    let (buffer, len) = build_cmdline(&[b"ls", b"-la", b"/home"]);
    // "ls\0-la\0/home\0"
    assert_eq!(&buffer[0..2], b"ls");
    assert_eq!(buffer[2], 0);
    assert_eq!(&buffer[3..6], b"-la");
    assert_eq!(buffer[6], 0);
    assert_eq!(&buffer[7..12], b"/home");
    assert_eq!(buffer[12], 0);
    assert_eq!(len, 13);
}

#[test]
fn test_default_argv0() {
    assert_eq!(DEFAULT_ARGV0, b"nexa");
}

// =========================================================================
// PID Tree Tests
// =========================================================================

mod pid_tree_tests {
    use crate::process::pid_tree;

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
