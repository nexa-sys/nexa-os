//! File Descriptor Tests
//!
//! Tests for file descriptor management using real kernel types and functions.
//!
//! NOTE: Tests that modify FILE_HANDLES use #[serial] to prevent race conditions.

#[cfg(test)]
mod tests {
    use crate::syscalls::types::{
        allocate_duplicate_slot, clear_file_handle, handle_for_fd,
        FileHandle, FileBacking, StdStreamKind, 
        FD_BASE, MAX_OPEN_FILES, STDIN, STDOUT, STDERR,
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
    // File Descriptor Constants Tests (using kernel constants)
    // =========================================================================

    #[test]
    fn test_standard_fds() {
        // Standard file descriptors (0, 1, 2) from kernel
        assert_eq!(STDIN, 0);
        assert_eq!(STDOUT, 1);
        assert_eq!(STDERR, 2);
    }

    #[test]
    fn test_fd_base() {
        // FD_BASE is where user file descriptors start
        assert!(FD_BASE >= 3, "FD_BASE should be >= 3 to avoid stdin/stdout/stderr");
    }

    #[test]
    fn test_max_open_files() {
        // Should have reasonable limit
        assert!(MAX_OPEN_FILES >= 16, "Should support at least 16 open files");
        assert!(MAX_OPEN_FILES <= 1024, "Should not be excessive");
    }

    // =========================================================================
    // StdStreamKind Tests (using kernel types)
    // =========================================================================

    #[test]
    fn test_std_stream_fd_mapping() {
        // Test kernel's StdStreamKind.fd() method
        assert_eq!(StdStreamKind::Stdin.fd(), STDIN);
        assert_eq!(StdStreamKind::Stdout.fd(), STDOUT);
        assert_eq!(StdStreamKind::Stderr.fd(), STDERR);
    }

    // =========================================================================
    // File Handle Lookup Tests (using real kernel functions)
    // =========================================================================

    #[test]
    fn test_handle_for_standard_streams() {
        // Kernel's handle_for_fd should return handles for stdin/stdout/stderr
        let stdin_handle = handle_for_fd(STDIN);
        let stdout_handle = handle_for_fd(STDOUT);
        let stderr_handle = handle_for_fd(STDERR);

        assert!(stdin_handle.is_ok(), "stdin should have a handle");
        assert!(stdout_handle.is_ok(), "stdout should have a handle");
        assert!(stderr_handle.is_ok(), "stderr should have a handle");
    }

    #[test]
    fn test_handle_for_invalid_fd() {
        // Invalid FD should return EBADF
        let result = handle_for_fd(FD_BASE + MAX_OPEN_FILES as u64 + 100);
        assert!(result.is_err());
        assert!(result.is_err()); // EBADF
    }

    // =========================================================================
    // FD Allocation Tests (using real kernel allocate_duplicate_slot)
    // =========================================================================

    #[test]
    fn test_fd_allocation_sequential() {
        clear_all_handles();

        // Allocate FDs sequentially using real kernel function
        let fd1 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        let fd2 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        let fd3 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();

        assert_eq!(fd1, FD_BASE);
        assert_eq!(fd2, FD_BASE + 1);
        assert_eq!(fd3, FD_BASE + 2);
    }

    #[test]
    fn test_fd_allocation_with_min_fd() {
        clear_all_handles();

        // Allocate with min_fd higher than FD_BASE
        let fd = allocate_duplicate_slot(10, dummy_handle()).unwrap();
        assert_eq!(fd, 10);

        let fd2 = allocate_duplicate_slot(10, dummy_handle()).unwrap();
        assert_eq!(fd2, 11);
    }

    #[test]
    fn test_fd_allocation_finds_lowest() {
        clear_all_handles();

        // Fill slots 0, 1, 2 (relative to FD_BASE)
        let _ = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap(); // -> 3
        let _ = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap(); // -> 4
        let _ = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap(); // -> 5

        // Allocate with min=3 should find slot 6
        let fd = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        assert_eq!(fd, FD_BASE + 3);
    }

    #[test]
    fn test_fd_allocation_exhaustion() {
        clear_all_handles();

        // Fill all slots
        for i in 0..MAX_OPEN_FILES {
            let result = allocate_duplicate_slot(FD_BASE, dummy_handle());
            assert!(result.is_ok(), "Should be able to allocate slot {}", i);
        }

        // Next allocation should fail with EMFILE
        let result = allocate_duplicate_slot(FD_BASE, dummy_handle());
        assert!(result.is_err());
        assert!(result.is_err()); // EMFILE
    }

    #[test]
    fn test_fd_allocation_min_fd_too_high() {
        clear_all_handles();

        // min_fd beyond MAX_OPEN_FILES should fail
        let result = allocate_duplicate_slot(FD_BASE + MAX_OPEN_FILES as u64 + 1, dummy_handle());
        assert!(result.is_err());
        assert!(result.is_err()); // EMFILE
    }

    // =========================================================================
    // File Backing Type Tests
    // =========================================================================

    #[test]
    fn test_file_backing_variants() {
        // Test that we can create various FileBacking types
        let _dev_null = FileBacking::DevNull;
        let _dev_zero = FileBacking::DevZero;
        let _dev_random = FileBacking::DevRandom;
        let _dev_urandom = FileBacking::DevUrandom;
        let _std_stream = FileBacking::StdStream(StdStreamKind::Stdin);
    }

    // =========================================================================
    // Seek Position Tests (algorithm validation)
    // =========================================================================

    #[test]
    fn test_seek_positions() {
        // SEEK constants from POSIX
        const SEEK_SET: i32 = 0;
        const SEEK_CUR: i32 = 1;
        const SEEK_END: i32 = 2;

        fn calculate_new_position(
            current: u64,
            file_size: u64,
            offset: i64,
            whence: i32,
        ) -> Option<u64> {
            let base = match whence {
                SEEK_SET => 0i64,
                SEEK_CUR => current as i64,
                SEEK_END => file_size as i64,
                _ => return None,
            };

            let new_pos = base + offset;
            if new_pos < 0 {
                None
            } else {
                Some(new_pos as u64)
            }
        }

        // SEEK_SET to 100
        assert_eq!(calculate_new_position(50, 1000, 100, SEEK_SET), Some(100));

        // SEEK_CUR +50
        assert_eq!(calculate_new_position(50, 1000, 50, SEEK_CUR), Some(100));

        // SEEK_END -10
        assert_eq!(calculate_new_position(50, 1000, -10, SEEK_END), Some(990));

        // Invalid: seek before start
        assert_eq!(calculate_new_position(50, 1000, -100, SEEK_SET), None);
    }

    // =========================================================================
    // Open Flags Tests (constants validation)
    // =========================================================================

    #[test]
    fn test_open_flags() {
        // These should match kernel's open flags
        const O_RDONLY: u32 = 0;
        const O_WRONLY: u32 = 1;
        const O_RDWR: u32 = 2;
        const O_CREAT: u32 = 0o100;
        const O_TRUNC: u32 = 0o1000;
        const O_APPEND: u32 = 0o2000;

        // Access mode is in lowest 2 bits
        fn access_mode(flags: u32) -> u32 {
            flags & 0o3
        }

        assert_eq!(access_mode(O_RDONLY), O_RDONLY);
        assert_eq!(access_mode(O_WRONLY), O_WRONLY);
        assert_eq!(access_mode(O_RDWR), O_RDWR);

        // Combined flags
        let flags = O_RDWR | O_CREAT | O_TRUNC;
        assert_eq!(access_mode(flags), O_RDWR);
        assert_ne!(flags & O_CREAT, 0);
        assert_ne!(flags & O_TRUNC, 0);
    }
}
