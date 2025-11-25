//! Time related syscalls
//!
//! Implements: clock_gettime, nanosleep, sched_yield

use super::types::*;
use crate::posix;
use crate::process::{USER_REGION_SIZE, USER_VIRT_BASE};

/// SYS_SCHED_YIELD - Yield CPU to scheduler
pub fn syscall_sched_yield() -> u64 {
    crate::kinfo!("sched_yield() - yielding CPU to scheduler");

    // Perform context switch to next ready process
    crate::scheduler::do_schedule();

    posix::set_errno(0);
    0
}

/// SYS_CLOCK_GETTIME - Get current time from specified clock
pub fn syscall_clock_gettime(_clk_id: i32, tp: *mut TimeSpec) -> u64 {
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

    // Get boot time in microseconds from TSC
    let time_us = crate::logger::boot_time_us();

    // Convert to seconds and nanoseconds
    let tv_sec = (time_us / 1_000_000) as i64;
    let tv_nsec = ((time_us % 1_000_000) * 1000) as i64;

    let timespec = TimeSpec { tv_sec, tv_nsec };

    unsafe {
        *tp = timespec;
    }

    posix::set_errno(0);
    0
}

/// SYS_NANOSLEEP - Sleep for specified time
pub fn syscall_nanosleep(req: *const TimeSpec, rem: *mut TimeSpec) -> u64 {
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
