//! System call numbers (POSIX-compliant where possible)
//!
//! This module defines all the system call numbers used by NexaOS.
//! Numbers are chosen to be compatible with Linux where possible.

// Basic I/O
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_STAT: u64 = 4;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;

// Memory management (Linux-compatible)
pub const SYS_MMAP: u64 = 9;
pub const SYS_MPROTECT: u64 = 10;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_BRK: u64 = 12;

// Signal handling
pub const SYS_SIGACTION: u64 = 13;
pub const SYS_SIGPROCMASK: u64 = 14;

// File descriptor manipulation
pub const SYS_PIPE: u64 = 22;
pub const SYS_SCHED_YIELD: u64 = 24;
pub const SYS_DUP: u64 = 32;
pub const SYS_DUP2: u64 = 33;
pub const SYS_NANOSLEEP: u64 = 35;

// Process management
pub const SYS_GETPID: u64 = 39;
pub const SYS_CLONE: u64 = 56;
pub const SYS_FORK: u64 = 57;
pub const SYS_EXECVE: u64 = 59;
pub const SYS_EXIT: u64 = 60;
pub const SYS_WAIT4: u64 = 61;
pub const SYS_KILL: u64 = 62;
pub const SYS_FCNTL: u64 = 72;
pub const SYS_GETPPID: u64 = 110;

// Thread management (Linux-compatible)
pub const SYS_GETTID: u64 = 186;
pub const SYS_FUTEX: u64 = 98; // Linux x86_64 uses 202, but we avoid conflict with NexaOS custom syscalls
pub const SYS_SET_TID_ADDRESS: u64 = 218;
pub const SYS_SET_ROBUST_LIST: u64 = 273;
pub const SYS_GET_ROBUST_LIST: u64 = 274;

// Architecture-specific (TLS support)
pub const SYS_ARCH_PRCTL: u64 = 158;

// Network socket calls (POSIX-compatible)
pub const SYS_SOCKET: u64 = 41;
pub const SYS_CONNECT: u64 = 42;
pub const SYS_ACCEPT: u64 = 43;
pub const SYS_SENDTO: u64 = 44;
pub const SYS_RECVFROM: u64 = 45;
pub const SYS_BIND: u64 = 49;
pub const SYS_LISTEN: u64 = 50;
pub const SYS_GETSOCKNAME: u64 = 51;
pub const SYS_GETPEERNAME: u64 = 52;
pub const SYS_SETSOCKOPT: u64 = 54;
pub const SYS_SOCKETPAIR: u64 = 53;

// Filesystem management
pub const SYS_PIVOT_ROOT: u64 = 155;
pub const SYS_CHROOT: u64 = 161;
pub const SYS_MOUNT: u64 = 165;
pub const SYS_UMOUNT: u64 = 166;
pub const SYS_REBOOT: u64 = 169;
pub const SYS_CLOCK_GETTIME: u64 = 228;

// Custom NexaOS syscalls (200+)
pub const SYS_LIST_FILES: u64 = 200;
pub const SYS_GETERRNO: u64 = 201;

// IPC syscalls
pub const SYS_IPC_CREATE: u64 = 210;
pub const SYS_IPC_SEND: u64 = 211;
pub const SYS_IPC_RECV: u64 = 212;

// User management syscalls
pub const SYS_USER_ADD: u64 = 220;
pub const SYS_USER_LOGIN: u64 = 221;
pub const SYS_USER_INFO: u64 = 222;
pub const SYS_USER_LIST: u64 = 223;
pub const SYS_USER_LOGOUT: u64 = 224;

// Init system calls
pub const SYS_SHUTDOWN: u64 = 230;
pub const SYS_RUNLEVEL: u64 = 231;

// UEFI compatibility bridge syscalls
pub const SYS_UEFI_GET_COUNTS: u64 = 240;
pub const SYS_UEFI_GET_FB_INFO: u64 = 241;
pub const SYS_UEFI_GET_NET_INFO: u64 = 242;
pub const SYS_UEFI_GET_BLOCK_INFO: u64 = 243;
pub const SYS_UEFI_MAP_NET_MMIO: u64 = 244;
pub const SYS_UEFI_GET_USB_INFO: u64 = 245;
pub const SYS_UEFI_GET_HID_INFO: u64 = 246;
pub const SYS_UEFI_MAP_USB_MMIO: u64 = 247;

// Kernel log syscalls
pub const SYS_SYSLOG: u64 = 250;

// Network configuration helpers
pub const SYS_NET_SET_DNS: u64 = 260;
pub const SYS_NET_GET_DNS: u64 = 261;
