//! Signal related syscalls
//!
//! Implements: sigaction, sigprocmask

use crate::kinfo;
use crate::posix;

/// POSIX sigaction() system call - examine and change signal action
pub fn sigaction(signum: u64, _act: *const u8, _oldact: *mut u8) -> u64 {
    if signum >= crate::signal::NSIG as u64 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // TODO: Implement full sigaction with user-space handlers
    kinfo!("sigaction(sig={}) called", signum);
    posix::set_errno(0);
    0
}

/// POSIX sigprocmask() system call - examine and change blocked signals
pub fn sigprocmask(_how: i32, _set: *const u64, _oldset: *mut u64) -> u64 {
    // TODO: Implement signal masking
    kinfo!("sigprocmask() called");
    posix::set_errno(0);
    0
}
