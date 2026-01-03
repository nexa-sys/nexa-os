//! Scheduler-related functions
//!
//! Provides sched_getaffinity, sched_setaffinity, and related functions.

use crate::{c_int, c_void, pid_t, refresh_errno_from_kernel, set_errno, size_t, EINVAL, ESRCH};
use core::arch::asm;

// Syscall numbers
const SYS_SCHED_GETAFFINITY: u64 = 204;
const SYS_SCHED_SETAFFINITY: u64 = 203;

/// CPU set size (1024 bits = 128 bytes to support up to 1024 CPUs)
pub const CPU_SETSIZE: usize = 1024;

/// CPU set structure
#[repr(C)]
#[derive(Clone, Copy)]
pub struct cpu_set_t {
    pub __bits: [u64; CPU_SETSIZE / 64],
}

impl cpu_set_t {
    /// Create an empty CPU set
    pub const fn new() -> Self {
        Self {
            __bits: [0; CPU_SETSIZE / 64],
        }
    }

    /// Set a CPU in the set
    pub fn set(&mut self, cpu: usize) {
        if cpu < CPU_SETSIZE {
            self.__bits[cpu / 64] |= 1 << (cpu % 64);
        }
    }

    /// Clear a CPU from the set
    pub fn clr(&mut self, cpu: usize) {
        if cpu < CPU_SETSIZE {
            self.__bits[cpu / 64] &= !(1 << (cpu % 64));
        }
    }

    /// Check if a CPU is in the set
    pub fn isset(&self, cpu: usize) -> bool {
        if cpu < CPU_SETSIZE {
            (self.__bits[cpu / 64] & (1 << (cpu % 64))) != 0
        } else {
            false
        }
    }

    /// Clear all CPUs from the set
    pub fn zero(&mut self) {
        for i in 0..self.__bits.len() {
            self.__bits[i] = 0;
        }
    }

    /// Count the number of CPUs in the set
    pub fn count(&self) -> usize {
        self.__bits.iter().map(|x| x.count_ones() as usize).sum()
    }
}

impl Default for cpu_set_t {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// CPU Set Macros (as functions)
// ============================================================================

/// Clear all CPUs from the set
#[no_mangle]
pub unsafe extern "C" fn __sched_cpufree(set: *mut cpu_set_t) {
    // In this implementation, cpu_set_t is stack-allocated, so nothing to free
    let _ = set;
}

/// Allocate a CPU set
#[no_mangle]
pub unsafe extern "C" fn __sched_cpualloc(count: size_t) -> *mut cpu_set_t {
    // We don't support dynamic allocation, return NULL
    let _ = count;
    core::ptr::null_mut()
}

/// Get CPU set size
#[no_mangle]
pub extern "C" fn __sched_cpucount(setsize: size_t, set: *const cpu_set_t) -> c_int {
    if set.is_null() || setsize == 0 {
        return 0;
    }

    let words = setsize / 8;
    let mut count = 0;
    unsafe {
        for i in 0..words.min(CPU_SETSIZE / 64) {
            count += (*set).__bits[i].count_ones() as c_int;
        }
    }
    count
}

// ============================================================================
// Scheduler Functions
// ============================================================================

/// Get CPU affinity mask of a process
///
/// # Arguments
/// * `pid` - Process ID (0 for current process)
/// * `cpusetsize` - Size of the CPU set buffer
/// * `mask` - Buffer to receive CPU affinity mask
///
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub unsafe extern "C" fn sched_getaffinity(
    pid: pid_t,
    cpusetsize: size_t,
    mask: *mut cpu_set_t,
) -> c_int {
    if mask.is_null() || cpusetsize == 0 {
        set_errno(EINVAL);
        return -1;
    }

    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") SYS_SCHED_GETAFFINITY => ret,
        in("rdi") pid as u64,
        in("rsi") cpusetsize as u64,
        in("rdx") mask as u64,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );

    if ret < 0 {
        set_errno((-ret) as c_int);
        -1
    } else {
        set_errno(0);
        0
    }
}

/// Set CPU affinity mask of a process
///
/// # Arguments
/// * `pid` - Process ID (0 for current process)
/// * `cpusetsize` - Size of the CPU set
/// * `mask` - CPU affinity mask to set
///
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub unsafe extern "C" fn sched_setaffinity(
    pid: pid_t,
    cpusetsize: size_t,
    mask: *const cpu_set_t,
) -> c_int {
    if mask.is_null() || cpusetsize == 0 {
        set_errno(EINVAL);
        return -1;
    }

    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") SYS_SCHED_SETAFFINITY => ret,
        in("rdi") pid as u64,
        in("rsi") cpusetsize as u64,
        in("rdx") mask as u64,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );

    if ret < 0 {
        set_errno((-ret) as c_int);
        -1
    } else {
        set_errno(0);
        0
    }
}

/// Get number of processors currently online
#[no_mangle]
pub extern "C" fn get_nprocs() -> c_int {
    // Try to get CPU count from affinity mask
    let mut mask = cpu_set_t::new();
    unsafe {
        if sched_getaffinity(0, core::mem::size_of::<cpu_set_t>(), &mut mask) == 0 {
            let count = mask.count();
            if count > 0 {
                return count as c_int;
            }
        }
    }
    // Fallback to 1 CPU
    1
}

/// Get number of processors configured
#[no_mangle]
pub extern "C" fn get_nprocs_conf() -> c_int {
    get_nprocs()
}

/// Get number of processors (sysconf-compatible)
#[no_mangle]
pub extern "C" fn sysconf(name: c_int) -> i64 {
    const _SC_NPROCESSORS_ONLN: c_int = 84;
    const _SC_NPROCESSORS_CONF: c_int = 83;
    const _SC_PAGESIZE: c_int = 30;
    const _SC_PAGE_SIZE: c_int = 30;
    const _SC_CLK_TCK: c_int = 2;

    match name {
        _SC_NPROCESSORS_ONLN | _SC_NPROCESSORS_CONF => get_nprocs() as i64,
        _SC_PAGESIZE | _SC_PAGE_SIZE => 4096,
        _SC_CLK_TCK => 100, // Standard Linux value
        _ => -1,
    }
}
