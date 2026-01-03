//! Watchdog timer syscalls
//!
//! This module provides syscall interfaces for the hardware watchdog timer.
//! The watchdog is typically accessed via /dev/watchdog, but these syscalls
//! provide a direct API for programs that need watchdog functionality.
//!
//! Syscalls:
//! - SYS_WATCHDOG_ENABLE: Enable the watchdog timer
//! - SYS_WATCHDOG_DISABLE: Disable the watchdog timer
//! - SYS_WATCHDOG_FEED: Feed (pet) the watchdog to prevent reset
//! - SYS_WATCHDOG_SET_TIMEOUT: Set the watchdog timeout
//! - SYS_WATCHDOG_GET_INFO: Get watchdog information

use super::types::user_buffer_in_range;
use crate::drivers::watchdog;
use crate::posix;
use core::mem::size_of;

/// SYS_WATCHDOG_CTL - Watchdog control syscall
///
/// # Arguments
/// * `cmd` - Command (see WatchdogCmd)
/// * `arg` - Command-specific argument
///
/// # Commands
/// - 0: ENABLE - Enable the watchdog
/// - 1: DISABLE - Disable the watchdog (requires root)
/// - 2: FEED - Feed/pet the watchdog
/// - 3: SET_TIMEOUT - Set timeout (arg = seconds)
/// - 4: GET_TIMEOUT - Get current timeout
/// - 5: GET_INFO - Get watchdog info (arg = pointer to WatchdogInfo)
/// - 6: GET_STATUS - Get enabled status
///
/// # Returns
/// Command-dependent value, or u64::MAX on error
pub fn watchdog_ctl(cmd: u64, arg: u64) -> u64 {
    const CMD_ENABLE: u64 = 0;
    const CMD_DISABLE: u64 = 1;
    const CMD_FEED: u64 = 2;
    const CMD_SET_TIMEOUT: u64 = 3;
    const CMD_GET_TIMEOUT: u64 = 4;
    const CMD_GET_INFO: u64 = 5;
    const CMD_GET_STATUS: u64 = 6;

    match cmd {
        CMD_ENABLE => match watchdog::enable() {
            Ok(()) => {
                posix::set_errno(0);
                0
            }
            Err(e) => {
                posix::set_errno(e);
                u64::MAX
            }
        },
        CMD_DISABLE => {
            // Disabling watchdog requires root
            if !crate::auth::is_superuser() {
                crate::kwarn!("[watchdog] disable: permission denied");
                posix::set_errno(posix::errno::EPERM);
                return u64::MAX;
            }

            match watchdog::disable() {
                Ok(()) => {
                    posix::set_errno(0);
                    0
                }
                Err(e) => {
                    posix::set_errno(e);
                    u64::MAX
                }
            }
        }
        CMD_FEED => match watchdog::feed() {
            Ok(()) => {
                posix::set_errno(0);
                0
            }
            Err(e) => {
                posix::set_errno(e);
                u64::MAX
            }
        },
        CMD_SET_TIMEOUT => {
            // Setting timeout requires root
            if !crate::auth::is_superuser() {
                crate::kwarn!("[watchdog] set_timeout: permission denied");
                posix::set_errno(posix::errno::EPERM);
                return u64::MAX;
            }

            match watchdog::set_timeout(arg as u32) {
                Ok(actual) => {
                    posix::set_errno(0);
                    actual as u64
                }
                Err(e) => {
                    posix::set_errno(e);
                    u64::MAX
                }
            }
        }
        CMD_GET_TIMEOUT => {
            posix::set_errno(0);
            watchdog::get_timeout() as u64
        }
        CMD_GET_INFO => {
            if arg == 0 || !user_buffer_in_range(arg, size_of::<watchdog::WatchdogInfo>() as u64) {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }

            let info = watchdog::get_info();
            unsafe {
                core::ptr::write(arg as *mut watchdog::WatchdogInfo, info);
            }

            posix::set_errno(0);
            0
        }
        CMD_GET_STATUS => {
            posix::set_errno(0);
            if watchdog::is_enabled() {
                1
            } else {
                0
            }
        }
        _ => {
            posix::set_errno(posix::errno::EINVAL);
            u64::MAX
        }
    }
}
