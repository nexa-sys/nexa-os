//! Time related syscalls
//!
//! Implements: clock_gettime, clock_settime, nanosleep, sched_yield

use super::types::*;
use crate::posix;
use crate::process::{Pid, USER_REGION_SIZE, USER_VIRT_BASE};
use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// System time offset from boot time in microseconds
/// When set, realtime = boot_time + TIME_OFFSET_US
static TIME_OFFSET_US: AtomicI64 = AtomicI64::new(0);

/// Maximum number of sleeping processes that can be tracked
const MAX_SLEEPERS: usize = 64;

/// Sleep entry: (wake_time_us, pid)
/// wake_time_us == 0 means the slot is empty
struct SleepEntry {
    wake_time_us: AtomicU64,
    pid: AtomicU64,
}

impl SleepEntry {
    const fn new() -> Self {
        Self {
            wake_time_us: AtomicU64::new(0),
            pid: AtomicU64::new(0),
        }
    }
}

/// Global sleep queue for nanosleep
static SLEEP_QUEUE: [SleepEntry; MAX_SLEEPERS] = {
    const EMPTY: SleepEntry = SleepEntry::new();
    [EMPTY; MAX_SLEEPERS]
};

/// Add a process to the sleep queue
fn add_sleeper(pid: Pid, wake_time_us: u64) -> bool {
    for entry in SLEEP_QUEUE.iter() {
        // Try to claim an empty slot (wake_time_us == 0)
        if entry
            .wake_time_us
            .compare_exchange(0, wake_time_us, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            entry.pid.store(pid as u64, Ordering::SeqCst);
            return true;
        }
    }
    false // Queue full
}

/// Remove a process from the sleep queue
fn remove_sleeper(pid: Pid) {
    for entry in SLEEP_QUEUE.iter() {
        if entry.pid.load(Ordering::SeqCst) == pid as u64 {
            entry.wake_time_us.store(0, Ordering::SeqCst);
            entry.pid.store(0, Ordering::SeqCst);
            break;
        }
    }
}

/// Check and wake up sleeping processes (called from timer tick)
/// IMPORTANT: This is called from interrupt context, so we use try_wake_process()
/// which won't deadlock if PROCESS_TABLE lock is already held.
pub fn check_sleepers() {
    let now_us = crate::logger::boot_time_us();

    for entry in SLEEP_QUEUE.iter() {
        let wake_time = entry.wake_time_us.load(Ordering::SeqCst);
        if wake_time == 0 {
            continue; // Empty slot
        }

        if now_us >= wake_time {
            let pid = entry.pid.load(Ordering::SeqCst) as Pid;
            // Clear the entry first
            entry.wake_time_us.store(0, Ordering::SeqCst);
            entry.pid.store(0, Ordering::SeqCst);
            // Wake up the process - use try_wake to avoid deadlock in interrupt context
            crate::scheduler::try_wake_process(pid);
            crate::kdebug!(
                "check_sleepers: woke PID {} (target={}, now={})",
                pid,
                wake_time,
                now_us
            );
        }
    }
}

/// Set the system time offset (called by clock_settime)
/// `realtime_us` is the Unix timestamp in microseconds
pub fn set_system_time_offset(realtime_us: i64) {
    let boot_us = crate::logger::boot_time_us() as i64;
    let offset = realtime_us - boot_us;
    TIME_OFFSET_US.store(offset, Ordering::SeqCst);
    crate::kinfo!(
        "System time offset set: {} us (realtime: {} us)",
        offset,
        realtime_us
    );
}

/// Get the current system time offset
pub fn get_system_time_offset() -> i64 {
    TIME_OFFSET_US.load(Ordering::SeqCst)
}

/// Get current realtime in microseconds (Unix timestamp)
pub fn get_realtime_us() -> i64 {
    let boot_us = crate::logger::boot_time_us() as i64;
    let offset = TIME_OFFSET_US.load(Ordering::SeqCst);
    boot_us + offset
}

/// SYS_SCHED_YIELD - Yield CPU to scheduler
pub fn sched_yield() -> u64 {
    crate::kinfo!("sched_yield() - yielding CPU to scheduler");

    // Perform context switch to next ready process
    crate::scheduler::do_schedule();

    posix::set_errno(0);
    0
}

/// SYS_CLOCK_GETTIME - Get current time from specified clock
pub fn clock_gettime(clk_id: i32, tp: *mut TimeSpec) -> u64 {
    if tp.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Validate user pointer
    let ptr_addr = tp as usize;
    if ptr_addr < USER_VIRT_BASE as usize
        || ptr_addr + core::mem::size_of::<TimeSpec>()
            > (USER_VIRT_BASE + USER_REGION_SIZE) as usize
    {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let (tv_sec, tv_nsec) = match clk_id {
        CLOCK_REALTIME => {
            // Return real time (Unix timestamp)
            let realtime_us = get_realtime_us();
            let sec = realtime_us / 1_000_000;
            let nsec = (realtime_us % 1_000_000) * 1000;
            (sec, nsec)
        }
        CLOCK_MONOTONIC | CLOCK_BOOTTIME => {
            // Return time since boot
            let time_us = crate::logger::boot_time_us() as i64;
            let sec = time_us / 1_000_000;
            let nsec = (time_us % 1_000_000) * 1000;
            (sec, nsec)
        }
        _ => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let timespec = TimeSpec { tv_sec, tv_nsec };

    unsafe {
        *tp = timespec;
    }

    posix::set_errno(0);
    0
}

/// SYS_CLOCK_SETTIME - Set current time for specified clock
pub fn clock_settime(clk_id: i32, tp: *const TimeSpec) -> u64 {
    if tp.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Validate user pointer
    let ptr_addr = tp as usize;
    if ptr_addr < USER_VIRT_BASE as usize
        || ptr_addr + core::mem::size_of::<TimeSpec>()
            > (USER_VIRT_BASE + USER_REGION_SIZE) as usize
    {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Only CLOCK_REALTIME can be set
    if clk_id != CLOCK_REALTIME {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let request = unsafe { *tp };

    // Validate timespec values
    if request.tv_sec < 0 || request.tv_nsec < 0 || request.tv_nsec >= 1_000_000_000 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Convert to microseconds and set the system time offset
    let realtime_us = (request.tv_sec * 1_000_000) + (request.tv_nsec / 1000);
    set_system_time_offset(realtime_us);

    crate::kinfo!(
        "clock_settime: system time set to {} (Unix timestamp)",
        request.tv_sec
    );

    posix::set_errno(0);
    0
}

/// tms structure for times() syscall
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Tms {
    pub tms_utime: i64,  // User CPU time
    pub tms_stime: i64,  // System CPU time
    pub tms_cutime: i64, // User CPU time of children
    pub tms_cstime: i64, // System CPU time of children
}

/// SYS_TIMES - Get process times
/// Returns clock ticks since system boot, fills tms structure
pub fn sys_times(buf: *mut Tms) -> u64 {
    // Get current boot time in microseconds
    let boot_us = crate::logger::boot_time_us();

    // Convert to clock ticks (assume 100 Hz = 10ms per tick, standard Linux)
    // clock ticks = microseconds / 10000
    let ticks = (boot_us / 10000) as i64;

    if !buf.is_null() {
        // Validate user pointer
        let ptr_addr = buf as usize;
        if ptr_addr >= USER_VIRT_BASE as usize
            && ptr_addr + core::mem::size_of::<Tms>()
                <= (USER_VIRT_BASE + USER_REGION_SIZE) as usize
        {
            // For now, return simplified times based on boot time
            // TODO: Track actual user/system time per process
            unsafe {
                (*buf).tms_utime = ticks / 2; // Approximate user time
                (*buf).tms_stime = ticks / 4; // Approximate system time
                (*buf).tms_cutime = 0; // No child accounting yet
                (*buf).tms_cstime = 0;
            }
        } else {
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }
    }

    posix::set_errno(0);
    ticks as u64
}

/// SYS_NANOSLEEP - Sleep for specified time
pub fn nanosleep(req: *const TimeSpec, rem: *mut TimeSpec) -> u64 {
    if req.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Validate user pointer
    let req_addr = req as usize;
    if req_addr < USER_VIRT_BASE as usize
        || req_addr + core::mem::size_of::<TimeSpec>()
            > (USER_VIRT_BASE + USER_REGION_SIZE) as usize
    {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let request = unsafe { *req };

    // Validate timespec values
    if request.tv_sec < 0 || request.tv_nsec < 0 || request.tv_nsec >= 1_000_000_000 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Convert to microseconds
    let sleep_us = (request.tv_sec as u64 * 1_000_000) + (request.tv_nsec as u64 / 1000);

    // Get current PID
    let Some(pid) = crate::scheduler::get_current_pid() else {
        posix::set_errno(posix::errno::ESRCH);
        return u64::MAX;
    };

    // Get current time and calculate wake time
    let start_us = crate::logger::boot_time_us();
    let target_us = start_us + sleep_us;

    crate::kdebug!(
        "nanosleep: PID {} sleeping for {} us (until {})",
        pid,
        sleep_us,
        target_us
    );

    // Add to sleep queue
    if !add_sleeper(pid, target_us) {
        // Queue full, fall back to minimal busy-wait with longer intervals
        crate::kwarn!(
            "nanosleep: sleep queue full, using busy-wait for PID {}",
            pid
        );
        loop {
            let now_us = crate::logger::boot_time_us();
            if now_us >= target_us {
                break;
            }
            // Yield to scheduler
            crate::scheduler::do_schedule();
        }
    } else {
        // Put process to sleep and let timer wake it
        crate::scheduler::sleep_current_process();
        crate::scheduler::do_schedule();

        // When we return here, we've been woken up
        // Remove ourselves from sleep queue if still there (e.g., spurious wake)
        remove_sleeper(pid);
    }

    // If rem is provided and sleep was interrupted (not implemented yet), fill it
    if !rem.is_null() {
        let rem_addr = rem as usize;
        if rem_addr >= USER_VIRT_BASE as usize
            && rem_addr + core::mem::size_of::<TimeSpec>()
                <= (USER_VIRT_BASE + USER_REGION_SIZE) as usize
        {
            unsafe {
                (*rem).tv_sec = 0;
                (*rem).tv_nsec = 0;
            }
        }
    }

    posix::set_errno(0);
    0
}
