//! Time related syscalls
//!
//! Implements: clock_gettime, clock_settime, nanosleep, sched_yield

use super::types::*;
use crate::posix;
use crate::process::{USER_REGION_SIZE, USER_VIRT_BASE};
use core::sync::atomic::{AtomicI64, Ordering};

/// System time offset from boot time in microseconds
/// When set, realtime = boot_time + TIME_OFFSET_US
static TIME_OFFSET_US: AtomicI64 = AtomicI64::new(0);

/// Set the system time offset (called by clock_settime)
/// `realtime_us` is the Unix timestamp in microseconds
pub fn set_system_time_offset(realtime_us: i64) {
    let boot_us = crate::logger::boot_time_us() as i64;
    let offset = realtime_us - boot_us;
    TIME_OFFSET_US.store(offset, Ordering::SeqCst);
    crate::kinfo!("System time offset set: {} us (realtime: {} us)", offset, realtime_us);
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

    // Get current time
    let start_us = crate::logger::boot_time_us();
    let target_us = start_us + sleep_us;

    crate::kdebug!(
        "nanosleep: sleeping for {} us (until {})",
        sleep_us,
        target_us
    );

    // Busy-wait sleep for now
    // TODO: Implement proper scheduler-based sleep with wait queues
    loop {
        let now_us = crate::logger::boot_time_us();
        if now_us >= target_us {
            break;
        }

        // Yield to scheduler to avoid monopolizing CPU
        crate::scheduler::do_schedule();
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
