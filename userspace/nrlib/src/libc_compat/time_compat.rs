//! Time-related compatibility functions
//!
//! Provides clock_gettime, gettimeofday, nanosleep, and related functions.

use crate::{c_int, c_long, time};
use super::types::{timespec, timeval, timezone, CLOCK_REALTIME, CLOCK_MONOTONIC, NSEC_PER_SEC};

// ============================================================================
// Helper Functions
// ============================================================================

pub fn validate_timespec(ts: &timespec) -> Result<u64, ()> {
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return Err(());
    }

    let total = (ts.tv_sec as u128)
        .saturating_mul(NSEC_PER_SEC)
        .saturating_add(ts.tv_nsec as u128);

    Ok(total.min(u64::MAX as u128) as u64)
}

// ============================================================================
// Clock Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn clock_gettime(clock_id: c_int, tp: *mut timespec) -> c_int {
    if tp.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let (sec, nsec) = match clock_id {
        CLOCK_REALTIME => time::realtime_timespec(),
        CLOCK_MONOTONIC => time::monotonic_timespec(),
        _ => {
            crate::set_errno(crate::EINVAL);
            return -1;
        }
    };

    (*tp).tv_sec = sec;
    (*tp).tv_nsec = nsec;
    crate::set_errno(0);
    0
}

#[no_mangle]
pub unsafe extern "C" fn clock_getres(clock_id: c_int, res: *mut timespec) -> c_int {
    if res.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    match clock_id {
        CLOCK_REALTIME | CLOCK_MONOTONIC => {
            let nanos = time::resolution_ns();
            (*res).tv_sec = 0;
            (*res).tv_nsec = nanos.max(1);
            crate::set_errno(0);
            0
        }
        _ => {
            crate::set_errno(crate::EINVAL);
            -1
        }
    }
}

/// clock_settime - Set the time of a specified clock
/// 
/// Only CLOCK_REALTIME can be set. Setting the time requires appropriate
/// privileges (typically root).
/// 
/// # Arguments
/// * `clock_id` - Clock to set (only CLOCK_REALTIME supported)
/// * `tp` - Pointer to timespec with new time
/// 
/// # Returns
/// * 0 on success
/// * -1 on error (errno set)
#[no_mangle]
pub unsafe extern "C" fn clock_settime(clock_id: c_int, tp: *const timespec) -> c_int {
    const SYS_CLOCK_SETTIME: u64 = 227;
    
    if tp.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }
    
    // Only CLOCK_REALTIME can be set
    if clock_id != CLOCK_REALTIME {
        crate::set_errno(crate::EINVAL);
        return -1;
    }
    
    // Validate timespec
    let ts = &*tp;
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        crate::set_errno(crate::EINVAL);
        return -1;
    }
    
    // Call kernel syscall
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") SYS_CLOCK_SETTIME,
        in("rdi") clock_id,
        in("rsi") tp,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    
    if ret == 0 {
        crate::set_errno(0);
        0
    } else {
        crate::set_errno(crate::EPERM);
        -1
    }
}

#[no_mangle]
pub unsafe extern "C" fn gettimeofday(tv: *mut timeval, tz: *mut timezone) -> c_int {
    if tv.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let (sec, nsec) = time::realtime_timespec();
    (*tv).tv_sec = sec;
    (*tv).tv_usec = (nsec / 1_000) as i64;

    if !tz.is_null() {
        (*tz).tz_minuteswest = 0;
        (*tz).tz_dsttime = 0;
    }

    crate::set_errno(0);
    0
}

// ============================================================================
// Sleep Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn nanosleep(req: *const timespec, rem: *mut timespec) -> c_int {
    if req.is_null() {
        crate::set_errno(crate::EINVAL);
        return -1;
    }

    let requested = match validate_timespec(&*req) {
        Ok(ns) => ns,
        Err(_) => {
            crate::set_errno(crate::EINVAL);
            return -1;
        }
    };

    time::sleep_ns(requested);

    if !rem.is_null() {
        (*rem).tv_sec = 0;
        (*rem).tv_nsec = 0;
    }

    crate::set_errno(0);
    0
}

#[no_mangle]
pub unsafe extern "C" fn pause() -> c_int {
    -1
}

// ============================================================================
// System Configuration
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn sysconf(_name: c_int) -> c_long {
    -1 // Not supported
}
