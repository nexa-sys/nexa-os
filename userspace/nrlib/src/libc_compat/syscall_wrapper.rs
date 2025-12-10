//! Syscall wrapper for variadic syscall function
//!
//! Provides the syscall() function that routes to appropriate syscall handlers.

use crate::{c_int, c_void};

use super::clone::{futex, gettid, set_tid_address};
use super::time_compat::nanosleep;
use super::types::timespec;

const SYS_SCHED_YIELD: i64 = 24;
const SYS_NANOSLEEP: i64 = 35;
const SYS_GETPID: i64 = 39;
const SYS_GETTID_NR: i64 = 186;
const SYS_FUTEX_NR: i64 = 98; // NexaOS uses 98, not Linux's 202
const SYS_SET_TID_ADDRESS_NR: i64 = 218;
const SYS_GETRANDOM: i64 = 318;

#[no_mangle]
pub unsafe extern "C" fn syscall(number: i64, mut args: ...) -> i64 {
    match number {
        SYS_GETPID => {
            crate::set_errno(0);
            crate::getpid() as i64
        }
        SYS_GETTID_NR => {
            crate::set_errno(0);
            gettid() as i64
        }
        SYS_SCHED_YIELD => {
            // Single-threaded for now – nothing to schedule.
            crate::set_errno(0);
            0
        }
        SYS_NANOSLEEP => {
            let req: *const timespec = args.arg();
            let rem: *mut timespec = args.arg();
            return nanosleep(req, rem) as i64;
        }
        SYS_GETRANDOM => {
            let buf: *mut c_void = args.arg();
            let len: usize = args.arg();
            let flags: u32 = args.arg();
            let res = crate::getrandom(buf, len, flags);
            if res < 0 {
                res as i64
            } else {
                crate::set_errno(0);
                res as i64
            }
        }
        SYS_SET_TID_ADDRESS_NR => {
            let tidptr: *mut c_int = args.arg();
            set_tid_address(tidptr) as i64
        }
        SYS_FUTEX_NR => {
            let uaddr: *mut i32 = args.arg();
            let op: i32 = args.arg();
            let val: i32 = args.arg();
            let timeout: *const timespec = args.arg();
            let uaddr2: *mut i32 = args.arg();
            let val3: i32 = args.arg();

            // Call our futex implementation which routes to kernel
            futex(
                uaddr as *mut c_int,
                op,
                val,
                timeout,
                uaddr2 as *mut c_int,
                val3,
            ) as i64
        }
        _ => {
            crate::set_errno(crate::ENOSYS);
            -1
        }
    }
}
