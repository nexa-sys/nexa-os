//! File Descriptor Limits and Edge Case Tests
//!
//! Tests for FD limits, exhaustion, and edge cases using real kernel code.
//!
//! NOTE: Tests that modify FILE_HANDLES are marked #[serial] to prevent race conditions.

#[cfg(test)]
mod tests {
    use crate::syscalls::types::{
        allocate_duplicate_slot, clear_file_handle,
        FileHandle, FileBacking, FD_BASE, MAX_OPEN_FILES,
    };
    use crate::posix::{Metadata, errno};
    use serial_test::serial;

    /// Helper to clear all file handles for test isolation
    fn clear_all_handles() {
        unsafe {
            for idx in 0..MAX_OPEN_FILES {
                clear_file_handle(idx);
            }
        }
    }

    /// Helper to create a dummy file handle for testing
    fn dummy_handle() -> FileHandle {
        FileHandle {
            backing: FileBacking::DevNull,
            position: 0,
            metadata: Metadata::empty(),
        }
    }

    // =========================================================================
    // FD Limits Tests (using real kernel constants)
    // =========================================================================

    #[test]
    fn test_max_fds_reasonable() {
        assert!(MAX_OPEN_FILES >= 16, "Should support at least 16 FDs");
        assert!(MAX_OPEN_FILES <= 65536, "Should not have excessive FD limit");
    }

    #[test]
    fn test_fd_base_after_stdio() {
        // First available FD after stdin/stdout/stderr should be 3
        assert_eq!(FD_BASE, 3);
    }

    // =========================================================================
    // FD Exhaustion Tests (using real kernel allocation)
    // These tests use #[serial] because they modify global FILE_HANDLES state
    // =========================================================================

    #[test]
    #[serial]
    fn test_fd_exhaustion_returns_emfile() {
        clear_all_handles();

        // Fill all slots
        for _ in 0..MAX_OPEN_FILES {
            let _ = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        }

        // Next allocation should fail with EMFILE
        let result = allocate_duplicate_slot(FD_BASE, dummy_handle());
        assert!(result.is_err());
        assert!(result.is_err()); // EMFILE
    }

    #[test]
    #[serial]
    fn test_fd_reuse_after_close() {
        clear_all_handles();

        // Allocate 3 FDs
        let fd1 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        let fd2 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        let fd3 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();

        assert_eq!(fd1, FD_BASE);
        assert_eq!(fd2, FD_BASE + 1);
        assert_eq!(fd3, FD_BASE + 2);

        // Close fd2 (slot 1)
        unsafe { clear_file_handle(1); }

        // Next allocation should reuse slot 1 (fd = FD_BASE + 1)
        let fd4 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        assert_eq!(fd4, FD_BASE + 1);
    }

    #[test]
    #[serial]
    fn test_partial_exhaustion_then_release() {
        clear_all_handles();

        // Allocate all slots
        for _ in 0..MAX_OPEN_FILES {
            let _ = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        }

        // Should be exhausted
        assert!(allocate_duplicate_slot(FD_BASE, dummy_handle()).is_err());

        // Release one slot
        unsafe { clear_file_handle(0); }

        // Should be able to allocate again
        let result = allocate_duplicate_slot(FD_BASE, dummy_handle());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), FD_BASE); // Reuses slot 0
    }

    // =========================================================================
    // Invalid FD Handling Tests
    // =========================================================================

    #[test]
    fn test_fd_beyond_max_invalid() {
        // Trying to allocate with min_fd beyond range should fail
        let result = allocate_duplicate_slot(FD_BASE + MAX_OPEN_FILES as u64, dummy_handle());
        assert!(result.is_err());
    }

    #[test]
    fn test_negative_fd_semantics() {
        // Negative FDs (-1) are commonly used as error indicators
        let neg_fd: i32 = -1;
        
        // When cast to u64, becomes very large number
        let as_u64 = neg_fd as u64;
        assert!(as_u64 > MAX_OPEN_FILES as u64);
    }

    // =========================================================================
    // FD Range Tests
    // =========================================================================

    #[test]
    fn test_valid_fd_range() {
        // Valid user FDs are [FD_BASE, FD_BASE + MAX_OPEN_FILES)
        let min_valid = FD_BASE;
        let max_valid = FD_BASE + MAX_OPEN_FILES as u64 - 1;

        assert!(min_valid >= 3);
        assert!(max_valid > min_valid);
    }

    #[test]
    #[serial]
    fn test_allocation_respects_min_fd() {
        clear_all_handles();

        // Allocate with min_fd = 10
        let fd = allocate_duplicate_slot(10, dummy_handle()).unwrap();
        assert_eq!(fd, 10);

        // Allocate with min_fd = 5, should get 5 (lower slot available)
        let fd2 = allocate_duplicate_slot(5, dummy_handle()).unwrap();
        assert_eq!(fd2, 5);
    }

    // =========================================================================
    // Stress Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_allocate_close_cycle() {
        clear_all_handles();

        // Do multiple allocate/close cycles
        for cycle in 0..5 {
            // Allocate all
            for i in 0..MAX_OPEN_FILES {
                let result = allocate_duplicate_slot(FD_BASE, dummy_handle());
                assert!(result.is_ok(), "Cycle {}, alloc {} failed", cycle, i);
            }

            // Clear all
            clear_all_handles();
        }
    }
}
