//! Common type definitions for libc compatibility layer
//!
//! This module contains type definitions shared across the libc_compat module.

use crate::{c_int, c_uint, c_ulong};
use core::sync::atomic::AtomicU32;

// ============================================================================
// Time Types
// ============================================================================

pub const CLOCK_REALTIME: c_int = 0;
pub const CLOCK_MONOTONIC: c_int = 1;

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct timeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct timezone {
    pub tz_minuteswest: i32,
    pub tz_dsttime: i32,
}

// ============================================================================
// Pthread Types
// ============================================================================

pub const PTHREAD_MUTEX_NORMAL: c_int = 0;
pub const PTHREAD_MUTEX_RECURSIVE: c_int = 1;
pub const PTHREAD_MUTEX_DEFAULT: c_int = PTHREAD_MUTEX_NORMAL;

pub const EPERM: c_int = 1;
pub const EBUSY: c_int = 16;
pub const EDEADLK: c_int = 35;

pub const MUTEX_UNLOCKED: u32 = 0;
pub const MUTEX_LOCKED: u32 = 1;

pub const PTHREAD_MUTEX_WORDS: usize = 5;
pub const MUTEX_MAGIC: usize = 0x4E584D5554585F4D; // "NXMUTX_M"
pub const GLIBC_KIND_WORD: usize = 2;

#[repr(C)]
pub struct pthread_mutex_t {
    pub data: [usize; PTHREAD_MUTEX_WORDS],
}

#[repr(C)]
pub struct pthread_mutexattr_t {
    pub data: [c_int; 7],
}

impl pthread_mutexattr_t {
    pub fn set_kind(&mut self, kind: c_int) {
        self.data[0] = kind;
    }

    pub fn kind(&self) -> c_int {
        self.data[0]
    }
}

pub struct MutexInner {
    pub state: AtomicU32,
    pub owner: c_ulong,
    pub recursion: c_uint,
    pub kind: c_int,
}

impl MutexInner {
    pub const fn new(kind: c_int) -> Self {
        Self {
            state: AtomicU32::new(MUTEX_UNLOCKED),
            owner: 0,
            recursion: 0,
            kind,
        }
    }
}

#[repr(C)]
pub struct pthread_attr_t {
    pub __size: [u64; 7],
}

#[allow(non_camel_case_types)]
pub type pthread_t = c_ulong;

// pthread_once support
#[repr(C)]
pub struct pthread_once_t {
    pub state: AtomicU32,
}

pub const PTHREAD_ONCE_INIT_VALUE: u32 = 0;
pub const PTHREAD_ONCE_IN_PROGRESS: u32 = 1;
pub const PTHREAD_ONCE_DONE: u32 = 2;

#[no_mangle]
pub static PTHREAD_ONCE_INIT: pthread_once_t = pthread_once_t {
    state: AtomicU32::new(PTHREAD_ONCE_INIT_VALUE),
};

// ============================================================================
// I/O Types
// ============================================================================

#[repr(C)]
pub struct iovec {
    pub iov_base: *mut crate::c_void,
    pub iov_len: crate::size_t,
}

// File access check constants
pub const F_OK: c_int = 0;
pub const R_OK: c_int = 4;
pub const W_OK: c_int = 2;
pub const X_OK: c_int = 1;

// fcntl commands
pub const F_DUPFD: c_int = 0;
pub const F_GETFL: c_int = 3;
pub const F_SETFL: c_int = 4;

// ============================================================================
// Unwind Types
// ============================================================================

#[repr(C)]
pub struct UnwindContext {
    pub _private: [u8; 0],
}

pub type UnwindReasonCode = c_int;

pub type UnwindTraceFn =
    unsafe extern "C" fn(context: *mut UnwindContext, arg: *mut crate::c_void) -> UnwindReasonCode;

// ============================================================================
// Signal Types
// ============================================================================

pub type sighandler_t = Option<unsafe extern "C" fn(c_int)>;

#[repr(C)]
pub struct sigaction {
    pub _private: [u8; 0],
}

// ============================================================================
// Clone/Thread Types
// ============================================================================

pub const CLONE_VM: c_int = 0x00000100;
pub const CLONE_FS: c_int = 0x00000200;
pub const CLONE_FILES: c_int = 0x00000400;
pub const CLONE_SIGHAND: c_int = 0x00000800;
pub const CLONE_THREAD: c_int = 0x00010000;
pub const CLONE_NEWNS: c_int = 0x00020000;
pub const CLONE_SYSVSEM: c_int = 0x00040000;
pub const CLONE_SETTLS: c_int = 0x00080000;
pub const CLONE_PARENT_SETTID: c_int = 0x00100000;
pub const CLONE_CHILD_CLEARTID: c_int = 0x00200000;
pub const CLONE_DETACHED: c_int = 0x00400000;
pub const CLONE_UNTRACED: c_int = 0x00800000;
pub const CLONE_CHILD_SETTID: c_int = 0x01000000;
pub const CLONE_VFORK: c_int = 0x00004000;

// Futex operations
pub const FUTEX_WAIT_OP: c_int = 0;
pub const FUTEX_WAKE_OP: c_int = 1;
pub const FUTEX_FD_OP: c_int = 2;
pub const FUTEX_REQUEUE_OP: c_int = 3;
pub const FUTEX_CMP_REQUEUE_OP: c_int = 4;
pub const FUTEX_WAKE_OP_OP: c_int = 5;
pub const FUTEX_LOCK_PI_OP: c_int = 6;
pub const FUTEX_UNLOCK_PI_OP: c_int = 7;
pub const FUTEX_TRYLOCK_PI_OP: c_int = 8;
pub const FUTEX_WAIT_BITSET_OP: c_int = 9;
pub const FUTEX_WAKE_BITSET_OP: c_int = 10;

pub const FUTEX_PRIVATE: c_int = 128;
pub const FUTEX_CLOCK_REALTIME_FLAG: c_int = 256;

// ============================================================================
// Memory Mapping Types
// ============================================================================

pub const PROT_NONE: c_int = 0x0;
pub const PROT_READ: c_int = 0x1;
pub const PROT_WRITE: c_int = 0x2;
pub const PROT_EXEC: c_int = 0x4;

pub const MAP_SHARED: c_int = 0x01;
pub const MAP_PRIVATE: c_int = 0x02;
pub const MAP_FIXED: c_int = 0x10;
pub const MAP_ANONYMOUS: c_int = 0x20;
pub const MAP_ANON: c_int = MAP_ANONYMOUS;
pub const MAP_NORESERVE: c_int = 0x4000;
pub const MAP_POPULATE: c_int = 0x8000;

pub const MAP_FAILED: *mut crate::c_void = (-1isize) as *mut crate::c_void;

// ============================================================================
// Wait/Process Types
// ============================================================================

pub const WNOHANG: c_int = 1;
pub const WUNTRACED: c_int = 2;
pub const WCONTINUED: c_int = 8;

// idtype_t for waitid
pub const P_PID: c_int = 1;
pub const P_PGID: c_int = 2;
pub const P_ALL: c_int = 0;

pub const WEXITED: c_int = 4;
pub const WSTOPPED: c_int = 2;
pub const WNOWAIT: c_int = 0x01000000;

/// siginfo_t structure (simplified)
#[repr(C)]
pub struct siginfo_t {
    pub si_signo: c_int,
    pub si_errno: c_int,
    pub si_code: c_int,
    pub _pad: [c_int; 29],
}

// ============================================================================
// posix_spawn Types
// ============================================================================

#[repr(C)]
pub struct posix_spawn_file_actions_t {
    pub _private: [u8; 80],
}

#[repr(C)]
pub struct posix_spawnattr_t {
    pub _private: [u8; 336],
}

// ============================================================================
// Helper Constants
// ============================================================================

pub const NSEC_PER_SEC: u128 = 1_000_000_000;
pub const MAX_PTHREAD_MUTEXES: usize = 128;
