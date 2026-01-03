//! Tests for syscalls/numbers.rs - System call numbers
//!
//! Verifies syscall numbers are Linux-compatible and properly organized.

#[cfg(test)]
mod tests {
    use crate::syscalls::numbers::*;

    // =========================================================================
    // Basic I/O Syscalls (0-20)
    // =========================================================================

    #[test]
    fn test_basic_io_syscalls() {
        // These must match Linux x86_64 ABI
        assert_eq!(SYS_READ, 0);
        assert_eq!(SYS_WRITE, 1);
        assert_eq!(SYS_OPEN, 2);
        assert_eq!(SYS_CLOSE, 3);
        assert_eq!(SYS_STAT, 4);
        assert_eq!(SYS_FSTAT, 5);
        assert_eq!(SYS_LSEEK, 8);
        assert_eq!(SYS_IOCTL, 16);
    }

    #[test]
    fn test_vectored_io_syscalls() {
        assert_eq!(SYS_PREAD64, 17);
        assert_eq!(SYS_PWRITE64, 18);
        assert_eq!(SYS_READV, 19);
        assert_eq!(SYS_WRITEV, 20);
    }

    // =========================================================================
    // Memory Management Syscalls
    // =========================================================================

    #[test]
    fn test_memory_syscalls() {
        assert_eq!(SYS_MMAP, 9);
        assert_eq!(SYS_MPROTECT, 10);
        assert_eq!(SYS_MUNMAP, 11);
        assert_eq!(SYS_BRK, 12);
        assert_eq!(SYS_MREMAP, 25);
        assert_eq!(SYS_MSYNC, 26);
        assert_eq!(SYS_MINCORE, 27);
        assert_eq!(SYS_MADVISE, 28);
    }

    #[test]
    fn test_memory_locking_syscalls() {
        assert_eq!(SYS_MLOCK, 149);
        assert_eq!(SYS_MUNLOCK, 150);
        assert_eq!(SYS_MLOCKALL, 151);
        assert_eq!(SYS_MUNLOCKALL, 152);
    }

    // =========================================================================
    // Process Management Syscalls
    // =========================================================================

    #[test]
    fn test_process_syscalls() {
        assert_eq!(SYS_GETPID, 39);
        assert_eq!(SYS_CLONE, 56);
        assert_eq!(SYS_FORK, 57);
        assert_eq!(SYS_EXECVE, 59);
        assert_eq!(SYS_EXIT, 60);
        assert_eq!(SYS_WAIT4, 61);
        assert_eq!(SYS_KILL, 62);
        assert_eq!(SYS_GETPPID, 110);
    }

    #[test]
    fn test_thread_syscalls() {
        assert_eq!(SYS_GETTID, 186);
        assert_eq!(SYS_FUTEX, 98);
        assert_eq!(SYS_SET_TID_ADDRESS, 218);
        assert_eq!(SYS_SET_ROBUST_LIST, 273);
        assert_eq!(SYS_GET_ROBUST_LIST, 274);
    }

    // =========================================================================
    // Signal Syscalls
    // =========================================================================

    #[test]
    fn test_signal_syscalls() {
        assert_eq!(SYS_SIGACTION, 13);
        assert_eq!(SYS_SIGPROCMASK, 14);
    }

    // =========================================================================
    // File Descriptor Syscalls
    // =========================================================================

    #[test]
    fn test_fd_syscalls() {
        assert_eq!(SYS_PIPE, 22);
        assert_eq!(SYS_DUP, 32);
        assert_eq!(SYS_DUP2, 33);
        assert_eq!(SYS_FCNTL, 72);
    }

    // =========================================================================
    // Network Syscalls
    // =========================================================================

    #[test]
    fn test_socket_syscalls() {
        assert_eq!(SYS_SOCKET, 41);
        assert_eq!(SYS_CONNECT, 42);
        assert_eq!(SYS_ACCEPT, 43);
        assert_eq!(SYS_SENDTO, 44);
        assert_eq!(SYS_RECVFROM, 45);
        assert_eq!(SYS_BIND, 49);
        assert_eq!(SYS_LISTEN, 50);
        assert_eq!(SYS_GETSOCKNAME, 51);
        assert_eq!(SYS_GETPEERNAME, 52);
        assert_eq!(SYS_SOCKETPAIR, 53);
        assert_eq!(SYS_SETSOCKOPT, 54);
    }

    // =========================================================================
    // Filesystem Syscalls
    // =========================================================================

    #[test]
    fn test_filesystem_syscalls() {
        assert_eq!(SYS_PIVOT_ROOT, 155);
        assert_eq!(SYS_CHROOT, 161);
        assert_eq!(SYS_MOUNT, 165);
        assert_eq!(SYS_UMOUNT, 166);
        assert_eq!(SYS_REBOOT, 169);
    }

    // =========================================================================
    // Time Syscalls
    // =========================================================================

    #[test]
    fn test_time_syscalls() {
        assert_eq!(SYS_NANOSLEEP, 35);
        assert_eq!(SYS_CLOCK_GETTIME, 228);
        assert_eq!(SYS_CLOCK_SETTIME, 227);
        assert_eq!(SYS_SCHED_YIELD, 24);
    }

    // =========================================================================
    // Architecture-specific Syscalls
    // =========================================================================

    #[test]
    fn test_arch_syscalls() {
        assert_eq!(SYS_ARCH_PRCTL, 158);
        assert_eq!(SYS_READLINKAT, 267);
    }

    // =========================================================================
    // Resource Limit Syscalls
    // =========================================================================

    #[test]
    fn test_resource_limit_syscalls() {
        assert_eq!(SYS_GETRLIMIT, 97);
        assert_eq!(SYS_SETRLIMIT, 160);
        assert_eq!(SYS_PRLIMIT64, 302);
    }

    // =========================================================================
    // NexaOS Custom Syscalls (200+)
    // =========================================================================

    #[test]
    fn test_custom_syscalls_range() {
        // Custom syscalls should be >= 200 to avoid Linux conflicts
        assert!(SYS_LIST_FILES >= 200);
        assert!(SYS_GETERRNO >= 200);
    }

    #[test]
    fn test_custom_syscalls_values() {
        assert_eq!(SYS_LIST_FILES, 200);
        assert_eq!(SYS_GETERRNO, 201);
    }

    // =========================================================================
    // Syscall Number Uniqueness
    // =========================================================================

    #[test]
    fn test_syscall_numbers_unique() {
        // Collect all syscall numbers that should be unique
        let syscalls = [
            SYS_READ, SYS_WRITE, SYS_OPEN, SYS_CLOSE, SYS_STAT, SYS_FSTAT,
            SYS_MMAP, SYS_MPROTECT, SYS_MUNMAP, SYS_BRK, SYS_IOCTL,
            SYS_FORK, SYS_EXECVE, SYS_EXIT, SYS_WAIT4, SYS_GETPID,
            SYS_SOCKET, SYS_CONNECT, SYS_BIND, SYS_LISTEN, SYS_ACCEPT,
        ];
        
        // Check for duplicates
        for i in 0..syscalls.len() {
            for j in (i + 1)..syscalls.len() {
                assert_ne!(syscalls[i], syscalls[j], 
                    "Duplicate syscall numbers at index {} and {}", i, j);
            }
        }
    }
}
