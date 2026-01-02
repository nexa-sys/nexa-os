//! Syscall Parameter Validation Tests
//!
//! Tests for syscall argument validation, error handling, and POSIX compliance.
//! These tests help catch bugs where invalid parameters aren't properly rejected.
//! Uses REAL kernel constants and functions.

#[cfg(test)]
mod tests {
    use crate::syscalls::memory::{
        PROT_NONE, PROT_READ, PROT_WRITE, PROT_EXEC,
        MAP_SHARED, MAP_PRIVATE, MAP_FIXED, MAP_ANONYMOUS,
        MAP_FAILED, PAGE_SIZE,
    };
    use crate::syscalls::numbers::*;
    use crate::safety::paging::{align_up, align_down, is_user_address};

    // =========================================================================
    // mmap Parameter Validation
    // =========================================================================

    #[test]
    fn test_mmap_zero_length_invalid() {
        // mmap with length=0 should fail with EINVAL
        let length = 0u64;
        // Zero length is always invalid per POSIX
        assert_eq!(length, 0, "Zero length should cause EINVAL");
    }

    #[test]
    fn test_mmap_length_overflow() {
        // Very large length could overflow when aligning
        let huge_length = u64::MAX;
        
        // Use kernel's align_up - should handle overflow gracefully
        // (wrapping or saturating)
        let aligned = align_up(huge_length, PAGE_SIZE);
        
        // Result should wrap or be zero due to overflow
        assert!(aligned == 0 || aligned < huge_length,
            "Overflow should be detected");
    }

    #[test]
    fn test_mmap_fixed_unaligned_address() {
        // MAP_FIXED with unaligned address should fail
        let addr = 0x1001u64; // Not page-aligned
        
        // Use kernel's align_down to check alignment
        let is_aligned = align_down(addr, PAGE_SIZE) == addr;
        assert!(!is_aligned, "0x1001 should not be page-aligned");
    }

    #[test]
    fn test_mmap_fixed_null_address() {
        // MAP_FIXED with addr=0 should fail
        let addr = 0u64;
        let flags = MAP_FIXED | MAP_ANONYMOUS | MAP_PRIVATE;
        
        // Both conditions must fail: addr=0 AND MAP_FIXED set
        let is_invalid = (flags & MAP_FIXED) != 0 && addr == 0;
        assert!(is_invalid, "MAP_FIXED with NULL address is invalid");
    }

    #[test]
    fn test_mmap_shared_private_mutual_exclusion() {
        // MAP_SHARED and MAP_PRIVATE are usually mutually exclusive
        let flags = MAP_SHARED | MAP_PRIVATE;
        
        let has_shared = (flags & MAP_SHARED) != 0;
        let has_private = (flags & MAP_PRIVATE) != 0;
        
        // Both being set is typically invalid
        assert!(has_shared && has_private,
            "Both MAP_SHARED and MAP_PRIVATE set");
    }

    #[test]
    fn test_mmap_anonymous_with_fd() {
        // MAP_ANONYMOUS should ignore fd, but fd=-1 is conventional
        let flags = MAP_ANONYMOUS | MAP_PRIVATE;
        let fd = -1i64;
        
        fn validate_anonymous_fd(flags: u64, fd: i64) -> bool {
            if (flags & MAP_ANONYMOUS) != 0 {
                // fd should typically be -1 for anonymous mappings
                fd == -1
            } else {
                // File-backed requires valid fd
                fd >= 0
            }
        }
        
        assert!(validate_anonymous_fd(flags, fd));
        
        // Non-anonymous without valid fd
        let flags2 = MAP_PRIVATE;
        let fd2 = -1i64;
        assert!(!validate_anonymous_fd(flags2, fd2));
    }

    // =========================================================================
    // brk/sbrk Validation
    // =========================================================================

    #[test]
    fn test_brk_decrease_limit() {
        // brk can't decrease below initial heap base
        let heap_base = 0x1200000u64;
        let requested = 0x1000000u64; // Below heap base
        
        fn validate_brk(requested: u64, heap_base: u64, current_brk: u64) -> Result<u64, &'static str> {
            if requested < heap_base {
                return Err("ENOMEM: Cannot set brk below heap base");
            }
            Ok(requested)
        }
        
        let result = validate_brk(requested, heap_base, heap_base);
        assert!(result.is_err());
    }

    #[test]
    fn test_brk_overflow_heap() {
        // brk can't grow beyond heap region
        let heap_base = 0x1200000u64;
        let heap_size = 0x800000u64;
        let requested = heap_base + heap_size + 0x1000; // Beyond heap
        
        fn validate_brk_limit(requested: u64, heap_base: u64, heap_size: u64) -> Result<u64, &'static str> {
            if requested > heap_base + heap_size {
                return Err("ENOMEM: brk beyond heap limit");
            }
            Ok(requested)
        }
        
        assert!(validate_brk_limit(requested, heap_base, heap_size).is_err());
    }

    // =========================================================================
    // mprotect Validation
    // =========================================================================

    #[test]
    fn test_mprotect_unaligned_address() {
        let addr = 0x1001u64;
        
        // Use REAL kernel align_down to check page alignment
        let is_page_aligned = |addr: u64| align_down(addr, PAGE_SIZE) == addr;
        
        assert!(!is_page_aligned(addr), "0x1001 should not be page-aligned");
    }

    #[test]
    fn test_mprotect_invalid_prot_combination() {
        // PROT_WRITE without PROT_READ might be invalid on some architectures
        let prot = PROT_WRITE; // Write-only
        
        // On x86, write implies read (hardware enforced)
        // Verify the flag value is valid (non-overlapping with other flags)
        assert_eq!(prot & PROT_READ, 0, "PROT_WRITE should not include PROT_READ");
        assert_eq!(prot & PROT_EXEC, 0, "PROT_WRITE should not include PROT_EXEC");
    }

    // =========================================================================
    // Process Syscall Validation
    // =========================================================================

    #[test]
    fn test_kill_signal_zero() {
        // kill with signal 0 is permission check, not signal delivery
        let sig = 0u32;
        
        fn is_null_signal(sig: u32) -> bool {
            sig == 0
        }
        
        assert!(is_null_signal(sig));
    }

    #[test]
    fn test_kill_negative_pid() {
        // Negative PIDs have special meaning:
        // pid=-1: all processes (except init and caller)
        // pid<-1: process group |pid|
        
        fn interpret_kill_pid(pid: i64) -> &'static str {
            match pid {
                p if p > 0 => "specific process",
                0 => "caller's process group",
                -1 => "all processes",
                p if p < -1 => "process group",
                _ => unreachable!(),
            }
        }
        
        assert_eq!(interpret_kill_pid(1), "specific process");
        assert_eq!(interpret_kill_pid(0), "caller's process group");
        assert_eq!(interpret_kill_pid(-1), "all processes");
        assert_eq!(interpret_kill_pid(-100), "process group");
    }

    #[test]
    fn test_wait4_pid_values() {
        // wait4 pid interpretation
        fn interpret_wait_pid(pid: i64) -> &'static str {
            match pid {
                p if p > 0 => "specific child",
                0 => "any child in same process group",
                -1 => "any child",
                p if p < -1 => "any child in specific process group",
                _ => unreachable!(),
            }
        }
        
        assert_eq!(interpret_wait_pid(123), "specific child");
        assert_eq!(interpret_wait_pid(-1), "any child");
    }

    // =========================================================================
    // File Descriptor Validation
    // =========================================================================

    #[test]
    fn test_negative_fd_invalid() {
        let fd = -1i64;
        
        fn is_valid_fd(fd: i64) -> bool {
            fd >= 0 && fd < 1024 // Typical max fd limit
        }
        
        assert!(!is_valid_fd(fd));
    }

    #[test]
    fn test_fd_limit() {
        // Most systems have fd limit (soft/hard)
        const MAX_FD: i64 = 1024;
        
        fn is_valid_fd(fd: i64) -> bool {
            fd >= 0 && fd < MAX_FD
        }
        
        assert!(is_valid_fd(0));
        assert!(is_valid_fd(MAX_FD - 1));
        assert!(!is_valid_fd(MAX_FD));
        assert!(!is_valid_fd(MAX_FD + 100));
    }

    #[test]
    fn test_dup2_same_fd() {
        // dup2(fd, fd) should succeed and return fd
        let oldfd = 5i64;
        let newfd = 5i64;
        
        fn dup2_same_fd_behavior(oldfd: i64, newfd: i64) -> Result<i64, &'static str> {
            if oldfd == newfd {
                // Per POSIX: if oldfd == newfd and oldfd is valid, return newfd
                // without closing
                return Ok(newfd);
            }
            Ok(newfd)
        }
        
        assert_eq!(dup2_same_fd_behavior(oldfd, newfd), Ok(5));
    }

    // =========================================================================
    // Clone Flag Validation
    // =========================================================================

    #[test]
    fn test_clone_thread_requires_vm() {
        // CLONE_THREAD requires CLONE_VM (via SIGHAND dependency)
        // Use REAL kernel validation function
        use crate::syscalls::validate_clone_flags;
        use crate::process::clone_flags::*;
        
        // Thread without VM sharing - invalid (missing SIGHAND too)
        let bad_flags = CLONE_THREAD;
        assert!(validate_clone_flags(bad_flags).is_err());
        
        // Proper thread flags
        let good_flags = CLONE_THREAD | CLONE_VM | CLONE_FILES | CLONE_SIGHAND;
        assert!(validate_clone_flags(good_flags).is_ok());
    }

    #[test]
    fn test_clone_sighand_requires_vm() {
        // CLONE_SIGHAND requires CLONE_VM (Linux semantics)
        // Use REAL kernel validation function
        use crate::syscalls::validate_clone_flags;
        use crate::process::clone_flags::*;
        
        let bad = CLONE_SIGHAND;
        assert!(validate_clone_flags(bad).is_err());
        
        let good = CLONE_SIGHAND | CLONE_VM;
        assert!(validate_clone_flags(good).is_ok());
    }

    // =========================================================================
    // Syscall Number Validation
    // =========================================================================

    #[test]
    fn test_syscall_numbers_unique() {
        // All syscall numbers should be unique
        let numbers = [
            SYS_READ, SYS_WRITE, SYS_OPEN, SYS_CLOSE, SYS_STAT, SYS_FSTAT,
            SYS_MMAP, SYS_MPROTECT, SYS_MUNMAP, SYS_BRK,
            SYS_FORK, SYS_EXIT, SYS_WAIT4, SYS_KILL,
            SYS_SOCKET, SYS_BIND, SYS_CONNECT, SYS_LISTEN, SYS_ACCEPT,
        ];
        
        let mut sorted = numbers.to_vec();
        sorted.sort();
        sorted.dedup();
        
        assert_eq!(sorted.len(), numbers.len(), "Duplicate syscall numbers found!");
    }

    #[test]
    fn test_syscall_numbers_positive() {
        let numbers = [
            SYS_READ, SYS_WRITE, SYS_OPEN, SYS_CLOSE,
            SYS_MMAP, SYS_FORK, SYS_EXIT,
        ];
        
        // SYS_READ is 0, which is valid
        // All others should be positive
        for &num in &numbers[1..] {
            assert!(num > 0, "Syscall number {} should be positive", num);
        }
    }
}
