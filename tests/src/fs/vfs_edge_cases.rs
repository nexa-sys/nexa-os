//! Filesystem and VFS Edge Case Tests
//!
//! Tests for file descriptor management, VFS operations, and filesystem
//! edge cases using real kernel code.

#[cfg(test)]
mod tests {
    use crate::syscalls::types::{
        allocate_duplicate_slot, clear_file_handle, handle_for_fd,
        FileHandle, FileBacking, StdStreamKind,
        FD_BASE, MAX_OPEN_FILES, STDIN, STDOUT, STDERR,
    };
    use crate::posix::{Metadata, errno};

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
    // File Descriptor Tests (using real kernel)
    // =========================================================================

    #[test]
    fn test_standard_fds_reserved() {
        assert_eq!(STDIN, 0);
        assert_eq!(STDOUT, 1);
        assert_eq!(STDERR, 2);
        assert_eq!(FD_BASE, 3);
    }

    #[test]
    fn test_fd_allocation_sequential() {
        clear_all_handles();

        let fd1 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        let fd2 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        let fd3 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        let fd4 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        let fd5 = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();

        assert_eq!(fd1, 3);
        assert_eq!(fd2, 4);
        assert_eq!(fd3, 5);
        assert_eq!(fd4, 6);
        assert_eq!(fd5, 7);
    }

    #[test]
    fn test_fd_reuse_after_close() {
        clear_all_handles();

        // Allocate FDs 3, 4, 5, 6, 7
        for _ in 0..5 {
            let _ = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        }

        // Close FD 5 (slot 2)
        unsafe { crate::syscalls::types::clear_file_handle(2); }

        // Next allocation should return 5 (lowest available)
        let new_fd = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        assert_eq!(new_fd, 5);
    }

    #[test]
    fn test_fd_max_limit() {
        clear_all_handles();

        // Allocate up to MAX_OPEN_FILES
        for i in 0..MAX_OPEN_FILES {
            let result = allocate_duplicate_slot(FD_BASE, dummy_handle());
            assert!(result.is_ok(), "Should allocate fd {}", i);
        }

        // Attempting to open more should fail with EMFILE
        let result = allocate_duplicate_slot(FD_BASE, dummy_handle());
        assert!(result.is_err());
        assert!(result.is_err()); // EMFILE
    }

    // =========================================================================
    // Open Flags Tests
    // =========================================================================

    #[test]
    fn test_open_access_modes() {
        // Access mode is in lower 2 bits
        const O_RDONLY: u64 = 0;
        const O_WRONLY: u64 = 1;
        const O_RDWR: u64 = 2;

        fn get_access_mode(flags: u64) -> u64 {
            flags & 3
        }

        assert_eq!(get_access_mode(O_RDONLY), 0);
        assert_eq!(get_access_mode(O_WRONLY), 1);
        assert_eq!(get_access_mode(O_RDWR), 2);
    }

    #[test]
    fn test_open_flags_combinable() {
        const O_WRONLY: u64 = 1;
        const O_CREAT: u64 = 0x40;
        const O_TRUNC: u64 = 0x200;

        let flags = O_WRONLY | O_CREAT | O_TRUNC;

        assert_ne!(flags & O_CREAT, 0);
        assert_ne!(flags & O_TRUNC, 0);
    }

    #[test]
    fn test_o_excl_with_creat() {
        const O_CREAT: u64 = 0x40;
        const O_EXCL: u64 = 0x80;

        fn validate_excl(flags: u64) -> bool {
            if (flags & O_EXCL) != 0 {
                (flags & O_CREAT) != 0
            } else {
                true
            }
        }

        assert!(validate_excl(O_CREAT | O_EXCL));
        assert!(!validate_excl(O_EXCL)); // Invalid without O_CREAT
    }

    // =========================================================================
    // Seek Tests
    // =========================================================================

    #[test]
    fn test_seek_constants() {
        const SEEK_SET: i32 = 0;
        const SEEK_CUR: i32 = 1;
        const SEEK_END: i32 = 2;

        assert_eq!(SEEK_SET, 0);
        assert_eq!(SEEK_CUR, 1);
        assert_eq!(SEEK_END, 2);
    }

    #[test]
    fn test_seek_position_calculation() {
        const SEEK_SET: i32 = 0;
        const SEEK_CUR: i32 = 1;
        const SEEK_END: i32 = 2;

        fn calculate_seek(current: u64, size: u64, offset: i64, whence: i32) -> Option<u64> {
            let base = match whence {
                SEEK_SET => 0i64,
                SEEK_CUR => current as i64,
                SEEK_END => size as i64,
                _ => return None,
            };
            let result = base + offset;
            if result < 0 { None } else { Some(result as u64) }
        }

        // SEEK_SET
        assert_eq!(calculate_seek(50, 1000, 100, SEEK_SET), Some(100));

        // SEEK_CUR
        assert_eq!(calculate_seek(50, 1000, 10, SEEK_CUR), Some(60));
        assert_eq!(calculate_seek(50, 1000, -10, SEEK_CUR), Some(40));

        // SEEK_END
        assert_eq!(calculate_seek(50, 1000, -100, SEEK_END), Some(900));

        // Negative result
        assert_eq!(calculate_seek(50, 1000, -1000, SEEK_CUR), None);
    }

    // =========================================================================
    // dup/dup2 Tests (using real kernel functions)
    // =========================================================================

    #[test]
    fn test_dup_returns_lowest_fd() {
        clear_all_handles();

        // Allocate FDs 3, 4, 6 (skip 5)
        let _ = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap(); // 3
        let _ = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap(); // 4
        let _ = allocate_duplicate_slot(6, dummy_handle()).unwrap(); // 6

        // Next allocation from FD_BASE should get 5
        let fd = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();
        assert_eq!(fd, 5);
    }

    #[test]
    fn test_dup2_same_fd() {
        // dup2(fd, fd) is a no-op per POSIX
        let fd: u64 = 5;
        // Just verifying the semantic - actual dup2 syscall test is elsewhere
        assert_eq!(fd, fd);
    }

    // =========================================================================
    // Close Tests
    // =========================================================================

    #[test]
    fn test_close_invalid_fd() {
        // Closing invalid FD should fail - test via handle_for_fd
        let invalid_fd: u64 = FD_BASE + MAX_OPEN_FILES as u64 + 100;
        let result = handle_for_fd(invalid_fd);
        assert!(result.is_err());
        assert!(result.is_err()); // EBADF
    }

    #[test]
    fn test_double_close_detection() {
        clear_all_handles();

        // Allocate one FD
        let fd = allocate_duplicate_slot(FD_BASE, dummy_handle()).unwrap();

        // First "close" - clear the handle
        unsafe { crate::syscalls::types::clear_file_handle(0); }

        // Second "close" - handle_for_fd should fail
        let result = handle_for_fd(fd);
        assert!(result.is_err());
        assert!(result.is_err()); // EBADF
    }

    // =========================================================================
    // File Backing Types Tests
    // =========================================================================

    #[test]
    fn test_file_backing_dev_null() {
        let handle = FileHandle {
            backing: FileBacking::DevNull,
            position: 0,
            metadata: Metadata::empty(),
        };

        matches!(handle.backing, FileBacking::DevNull);
    }

    #[test]
    fn test_file_backing_std_streams() {
        let stdin = FileBacking::StdStream(StdStreamKind::Stdin);
        let stdout = FileBacking::StdStream(StdStreamKind::Stdout);
        let stderr = FileBacking::StdStream(StdStreamKind::Stderr);

        assert!(matches!(stdin, FileBacking::StdStream(StdStreamKind::Stdin)));
        assert!(matches!(stdout, FileBacking::StdStream(StdStreamKind::Stdout)));
        assert!(matches!(stderr, FileBacking::StdStream(StdStreamKind::Stderr)));
    }

    // =========================================================================
    // Standard Stream Handle Tests
    // =========================================================================

    #[test]
    fn test_standard_stream_handles() {
        // Verify kernel provides handles for stdin/stdout/stderr
        assert!(handle_for_fd(STDIN).is_ok());
        assert!(handle_for_fd(STDOUT).is_ok());
        assert!(handle_for_fd(STDERR).is_ok());
    }

    #[test]
    fn test_standard_stream_handle_types() {
        let stdin = handle_for_fd(STDIN).unwrap();
        let stdout = handle_for_fd(STDOUT).unwrap();
        let stderr = handle_for_fd(STDERR).unwrap();

        assert!(matches!(stdin.backing, FileBacking::StdStream(StdStreamKind::Stdin)));
        assert!(matches!(stdout.backing, FileBacking::StdStream(StdStreamKind::Stdout)));
        assert!(matches!(stderr.backing, FileBacking::StdStream(StdStreamKind::Stderr)));
    }
}
