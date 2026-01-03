//! Syscall Parameter Validation Comprehensive Tests
//!
//! Tests for syscall parameter validation including:
//! - Invalid pointer handling
//! - Buffer overflow prevention
//! - Size limits validation
//! - Alignment requirements
//!
//! All tests run in SEPARATE PROCESSES via rusty_fork for complete isolation.

use rusty_fork::rusty_fork_test;

/// Base address where kernel's mmap allocates from
const MMAP_REGION_START: u64 = 0x1D00000;
const MMAP_REGION_PAGES: u64 = 1024;
const PAGE_SIZE_U64: u64 = 4096;
const REGION_SIZE: usize = (MMAP_REGION_PAGES * PAGE_SIZE_U64) as usize;

// Linux mmap constants for setup
const LIBC_PROT_READ: i32 = 0x1;
const LIBC_PROT_WRITE: i32 = 0x2;
const LIBC_MAP_PRIVATE: i32 = 0x02;
const LIBC_MAP_ANONYMOUS: i32 = 0x20;
const LIBC_MAP_FIXED: i32 = 0x10;

extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
}

/// Initialize memory region for kernel mmap to use.
/// Uses real mmap MAP_FIXED to map memory at the exact address the kernel expects.
fn setup_vm() {
    use crate::mock::vm::VirtualMachine;
    
    // Map real memory at MMAP_REGION_START using system mmap
    let ptr = unsafe {
        mmap(
            MMAP_REGION_START as *mut u8,
            REGION_SIZE,
            LIBC_PROT_READ | LIBC_PROT_WRITE,
            LIBC_MAP_PRIVATE | LIBC_MAP_ANONYMOUS | LIBC_MAP_FIXED,
            -1,
            0
        )
    };
    
    if ptr as usize == usize::MAX || ptr as u64 != MMAP_REGION_START {
        panic!("Failed to mmap region at {:#x}, got {:?}", MMAP_REGION_START, ptr);
    }
    
    let vm = VirtualMachine::new();
    vm.install();
    
    std::mem::forget(vm);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syscalls::memory::{
        mmap, munmap, mprotect,
        PROT_NONE, PROT_READ, PROT_WRITE, PROT_EXEC,
        MAP_SHARED, MAP_PRIVATE, MAP_FIXED, MAP_ANONYMOUS,
        MAP_FAILED, PAGE_SIZE,
    };

    // =========================================================================
    // Invalid Address Tests
    // =========================================================================

    // Kernel should reject mmap requests with kernel-space addresses
    // and return MAP_FAILED instead of crashing
    rusty_fork_test! {
        #[test]
        fn test_mmap_kernel_address_rejected() {
            setup_vm();
            // Attempt to mmap at a kernel-space address (>= 0xFFFF_8000_0000_0000)
            let kernel_addr: u64 = 0xFFFF_8000_0000_0000;
            let result = mmap(kernel_addr, PAGE_SIZE, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
            
            // Kernel should reject this and return MAP_FAILED
            assert_eq!(result, MAP_FAILED, 
                "mmap should reject kernel-space addresses and return MAP_FAILED");
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_munmap_invalid_address() {
            setup_vm();
            let _ = munmap(0xDEAD_BEEF_0000u64, PAGE_SIZE);
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_munmap_unaligned_address() {
            setup_vm();
            let _ = munmap(0x1001u64, PAGE_SIZE);
        }
    }

    // =========================================================================
    // Size Validation Tests
    // =========================================================================

    rusty_fork_test! {
        #[test]
        fn test_mmap_huge_size() {
            setup_vm();
            let result = mmap(0, 0x1000_0000_0000u64, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            assert!(result == MAP_FAILED || result > 0);
        }
    }

    // Kernel should handle length overflow gracefully and return MAP_FAILED
    // instead of panicking
    rusty_fork_test! {
        #[test]
        fn test_mmap_overflow_size() {
            setup_vm();
            // Use a length that would overflow when adding PAGE_SIZE - 1
            let overflow_length: u64 = u64::MAX - PAGE_SIZE + 2;
            let result = mmap(0, overflow_length, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            
            // Kernel should detect overflow and return MAP_FAILED
            assert_eq!(result, MAP_FAILED,
                "mmap should handle overflow gracefully and return MAP_FAILED");
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_munmap_zero_size() {
            setup_vm();
            let _ = munmap(0x1000, 0);
        }
    }

    // =========================================================================
    // Protection Flags Tests
    // =========================================================================

    rusty_fork_test! {
        #[test]
        fn test_mmap_invalid_prot_combination() {
            setup_vm();
            let result = mmap(0, PAGE_SIZE, PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            if result != MAP_FAILED { munmap(result, PAGE_SIZE); }
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_mprotect_on_unmapped() {
            setup_vm();
            let _ = mprotect(0xCAFE_0000u64, PAGE_SIZE, PROT_READ);
        }
    }

    // =========================================================================
    // Flag Combination Tests
    // =========================================================================

    rusty_fork_test! {
        #[test]
        fn test_mmap_conflicting_flags() {
            setup_vm();
            let result = mmap(0, PAGE_SIZE, PROT_READ | PROT_WRITE,
                MAP_SHARED | MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            if result != MAP_FAILED { munmap(result, PAGE_SIZE); }
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_mmap_anonymous_with_fd() {
            setup_vm();
            let result = mmap(0, PAGE_SIZE, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS, 3, 0);
            if result != MAP_FAILED {
                assert_ne!(result, 0);
                munmap(result, PAGE_SIZE);
            }
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_mmap_file_backed_no_fd() {
            setup_vm();
            let result = mmap(0, PAGE_SIZE, PROT_READ, MAP_PRIVATE, -1, 0);
            if result != MAP_FAILED { munmap(result, PAGE_SIZE); }
        }
    }

    // =========================================================================
    // Offset Tests
    // =========================================================================

    rusty_fork_test! {
        #[test]
        fn test_mmap_unaligned_offset() {
            setup_vm();
            let result = mmap(0, PAGE_SIZE, PROT_READ, MAP_PRIVATE, 0, 1);
            if result != MAP_FAILED { munmap(result, PAGE_SIZE); }
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_mmap_huge_offset() {
            setup_vm();
            let result = mmap(0, PAGE_SIZE, PROT_READ,
                MAP_PRIVATE | MAP_ANONYMOUS, -1, u64::MAX - PAGE_SIZE);
            if result != MAP_FAILED { munmap(result, PAGE_SIZE); }
        }
    }

    // =========================================================================
    // Alignment Tests
    // =========================================================================

    rusty_fork_test! {
        #[test]
        fn test_mmap_returned_alignment() {
            setup_vm();
            for _ in 0..10 {
                let result = mmap(0, PAGE_SIZE, PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
                if result != MAP_FAILED {
                    assert_eq!(result % PAGE_SIZE, 0);
                    munmap(result, PAGE_SIZE);
                }
            }
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_mmap_hint_alignment() {
            setup_vm();
            let result = mmap(0x1234_5678u64, PAGE_SIZE, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            if result != MAP_FAILED {
                assert_eq!(result % PAGE_SIZE, 0);
                munmap(result, PAGE_SIZE);
            }
        }
    }

    // =========================================================================
    // Boundary Tests
    // =========================================================================

    rusty_fork_test! {
        #[test]
        fn test_mmap_at_boundary() {
            setup_vm();
            let result = mmap(0x1000_0000u64, PAGE_SIZE, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            if result != MAP_FAILED { munmap(result, PAGE_SIZE); }
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_munmap_partial() {
            setup_vm();
            let size = PAGE_SIZE * 4;
            let result = mmap(0, size, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            if result != MAP_FAILED {
                munmap(result + PAGE_SIZE, PAGE_SIZE * 2);
                munmap(result, PAGE_SIZE);
                munmap(result + PAGE_SIZE * 3, PAGE_SIZE);
            }
        }
    }

    // =========================================================================
    // Stress Tests
    // =========================================================================

    rusty_fork_test! {
        #[test]
        fn test_mmap_many_small() {
            setup_vm();
            let mut addrs = Vec::new();
            for _ in 0..100 {
                let r = mmap(0, PAGE_SIZE, PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
                if r != MAP_FAILED { addrs.push(r); } else { break; }
            }
            for a in addrs { munmap(a, PAGE_SIZE); }
        }
    }

    rusty_fork_test! {
        #[test]
        fn test_mmap_munmap_cycle() {
            setup_vm();
            for _ in 0..100 {
                let r = mmap(0, PAGE_SIZE * 10, PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
                if r != MAP_FAILED { munmap(r, PAGE_SIZE * 10); }
            }
            let r = mmap(0, PAGE_SIZE, PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            if r != MAP_FAILED { munmap(r, PAGE_SIZE); }
        }
    }
}
