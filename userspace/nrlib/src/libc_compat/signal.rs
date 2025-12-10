//! Signal handling implementation
//!
//! Provides signal-related functions for NexaOS.

use super::types::{sigaction, sighandler_t};
use crate::{c_int, c_void, pid_t, refresh_errno_from_kernel, set_errno, syscall2, EINVAL};

/// System call number for kill
const SYS_KILL: u64 = 62;

// ============================================================================
// Signal Constants (POSIX)
// ============================================================================

/// Hangup detected on controlling terminal
pub const SIGHUP: c_int = 1;
/// Interrupt from keyboard
pub const SIGINT: c_int = 2;
/// Quit from keyboard
pub const SIGQUIT: c_int = 3;
/// Illegal Instruction
pub const SIGILL: c_int = 4;
/// Trace/breakpoint trap
pub const SIGTRAP: c_int = 5;
/// Abort signal from abort(3)
pub const SIGABRT: c_int = 6;
/// Bus error
pub const SIGBUS: c_int = 7;
/// Floating point exception
pub const SIGFPE: c_int = 8;
/// Kill signal (cannot be caught or ignored)
pub const SIGKILL: c_int = 9;
/// User-defined signal 1
pub const SIGUSR1: c_int = 10;
/// Invalid memory reference
pub const SIGSEGV: c_int = 11;
/// User-defined signal 2
pub const SIGUSR2: c_int = 12;
/// Broken pipe
pub const SIGPIPE: c_int = 13;
/// Timer signal from alarm(2)
pub const SIGALRM: c_int = 14;
/// Termination signal
pub const SIGTERM: c_int = 15;
/// Child stopped or terminated
pub const SIGCHLD: c_int = 17;
/// Continue if stopped
pub const SIGCONT: c_int = 18;
/// Stop process (cannot be caught or ignored)
pub const SIGSTOP: c_int = 19;
/// Stop typed at terminal
pub const SIGTSTP: c_int = 20;
/// Terminal input for background process
pub const SIGTTIN: c_int = 21;
/// Terminal output for background process
pub const SIGTTOU: c_int = 22;

// ============================================================================
// Signal Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn signal(_signum: c_int, _handler: sighandler_t) -> sighandler_t {
    None // Return NULL (signal not supported)
}

#[no_mangle]
pub unsafe extern "C" fn sigaction(
    _signum: c_int,
    _act: *const sigaction,
    _oldact: *mut sigaction,
) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn sigaltstack(_ss: *const c_void, _old_ss: *mut c_void) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn sigemptyset(_set: *mut c_void) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn sigaddset(_set: *mut c_void, _signum: c_int) -> c_int {
    0
}

// ============================================================================
// Kill Function
// ============================================================================

/// Send a signal to a process
///
/// # Arguments
/// * `pid` - Process ID to send signal to:
///   - pid > 0: Send to process with that PID
///   - pid == 0: Send to all processes in caller's process group
///   - pid == -1: Send to all processes (except init)
///   - pid < -1: Send to all processes in process group -pid
/// * `sig` - Signal number to send (0 to check if process exists)
///
/// # Returns
/// * 0 on success
/// * -1 on error (errno is set)
#[no_mangle]
pub extern "C" fn kill(pid: pid_t, sig: c_int) -> c_int {
    // Validate signal number
    if sig < 0 || sig >= 32 {
        set_errno(EINVAL);
        return -1;
    }

    let ret = syscall2(SYS_KILL, pid as u64, sig as u64);

    if ret == u64::MAX {
        refresh_errno_from_kernel();
        -1
    } else {
        set_errno(0);
        0
    }
}
