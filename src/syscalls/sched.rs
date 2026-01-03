//! Scheduler-related syscalls
//!
//! Provides sched_getaffinity, sched_setaffinity for CPU affinity management.

use crate::posix;
use crate::{kinfo, kwarn};

// ============================================================================
// CPU Affinity
// ============================================================================

/// CPU set size in bits
const CPU_SETSIZE: usize = 1024;

/// Get CPU affinity mask
///
/// For now, returns a mask with all available CPUs set.
pub fn sched_getaffinity(pid: i64, cpusetsize: usize, mask: *mut u64) -> u64 {
    kinfo!(
        "[SYS_SCHED_GETAFFINITY] pid={} cpusetsize={}",
        pid,
        cpusetsize
    );

    if mask.is_null() || cpusetsize == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Validate pid (0 = current process)
    if pid < 0 {
        posix::set_errno(posix::errno::ESRCH);
        return u64::MAX;
    }

    // Get number of CPUs from SMP module
    let num_cpus = crate::smp::cpu_count();

    // Clear the mask first
    let words = cpusetsize / 8;
    unsafe {
        for i in 0..words {
            *mask.add(i) = 0;
        }
    }

    // Set bits for available CPUs
    let bits_to_set = num_cpus.min(cpusetsize * 8);
    unsafe {
        for cpu in 0..bits_to_set {
            let word_idx = cpu / 64;
            let bit_idx = cpu % 64;
            if word_idx < words {
                *mask.add(word_idx) |= 1u64 << bit_idx;
            }
        }
    }

    kinfo!(
        "[SYS_SCHED_GETAFFINITY] Returning {} CPUs in affinity mask",
        bits_to_set
    );
    posix::set_errno(0);
    // Linux returns the actual size used, but we return 0 for success
    cpusetsize.min((num_cpus + 63) / 64 * 8) as u64
}

/// Set CPU affinity mask
///
/// Currently a no-op (accepts but ignores the mask).
pub fn sched_setaffinity(pid: i64, cpusetsize: usize, mask: *const u64) -> u64 {
    kinfo!(
        "[SYS_SCHED_SETAFFINITY] pid={} cpusetsize={}",
        pid,
        cpusetsize
    );

    if mask.is_null() || cpusetsize == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if pid < 0 {
        posix::set_errno(posix::errno::ESRCH);
        return u64::MAX;
    }

    // For now, just accept but don't actually change affinity
    // TODO: Implement actual CPU affinity in scheduler
    kinfo!("[SYS_SCHED_SETAFFINITY] Accepted (affinity not enforced)");
    posix::set_errno(0);
    0
}
