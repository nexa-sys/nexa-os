//! Process control functions
//!
//! Provides wait status macros, posix_spawn, exec, and process ID functions.

use crate::{c_char, c_int, c_void, size_t, ssize_t};
use core::ptr;

use super::types::{
    posix_spawn_file_actions_t, posix_spawnattr_t, siginfo_t,
    P_PID, P_PGID, P_ALL, WNOHANG,
};

// ============================================================================
// Wait Status Macros
// ============================================================================

/// POSIX wait status macros - extracts exit code from wait status
#[inline]
pub const fn wexitstatus(status: c_int) -> c_int {
    (status >> 8) & 0xff
}

/// POSIX wait status macros - checks if process exited normally
#[inline]
pub const fn wifexited(status: c_int) -> bool {
    (status & 0x7f) == 0
}

/// POSIX wait status macros - checks if process was terminated by signal
#[inline]
pub const fn wifsignaled(status: c_int) -> bool {
    ((status & 0x7f) + 1) as i8 >= 2
}

/// POSIX wait status macros - extracts signal number that terminated process
#[inline]
pub const fn wtermsig(status: c_int) -> c_int {
    status & 0x7f
}

/// POSIX wait status macros - checks if process was stopped
#[inline]
pub const fn wifstopped(status: c_int) -> bool {
    (status & 0xff) == 0x7f
}

/// POSIX wait status macros - extracts stop signal
#[inline]
pub const fn wstopsig(status: c_int) -> c_int {
    (status >> 8) & 0xff
}

// Export wait status macros as C-compatible functions
#[no_mangle]
pub extern "C" fn __WEXITSTATUS(status: c_int) -> c_int {
    wexitstatus(status)
}

#[no_mangle]
pub extern "C" fn __WIFEXITED(status: c_int) -> c_int {
    if wifexited(status) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn __WIFSIGNALED(status: c_int) -> c_int {
    if wifsignaled(status) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn __WTERMSIG(status: c_int) -> c_int {
    wtermsig(status)
}

#[no_mangle]
pub extern "C" fn __WIFSTOPPED(status: c_int) -> c_int {
    if wifstopped(status) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn __WSTOPSIG(status: c_int) -> c_int {
    wstopsig(status)
}

// ============================================================================
// Wait Functions
// ============================================================================

/// waitpid - wait for process to change state
#[no_mangle]
pub unsafe extern "C" fn waitpid(pid: crate::pid_t, status: *mut c_int, options: c_int) -> crate::pid_t {
    crate::wait4(pid, status, options, ptr::null_mut())
}

/// wait3 - wait for process to change state (BSD-style)
#[no_mangle]
pub unsafe extern "C" fn wait3(status: *mut c_int, options: c_int, _rusage: *mut c_void) -> crate::pid_t {
    crate::wait4(-1, status, options, ptr::null_mut())
}

/// waitid - wait for a process to change state (POSIX)
#[no_mangle]
pub unsafe extern "C" fn waitid(
    idtype: c_int,
    id: crate::pid_t,
    infop: *mut siginfo_t,
    options: c_int,
) -> c_int {
    let pid = match idtype {
        P_PID => id,
        P_PGID => -(id as i32),
        P_ALL => -1,
        _ => {
            crate::set_errno(crate::EINVAL);
            return -1;
        }
    };
    
    let wait_options = if (options & WNOHANG) != 0 { WNOHANG } else { 0 };
    
    let mut status: c_int = 0;
    let result = crate::wait4(pid, &mut status, wait_options, ptr::null_mut());
    
    if result < 0 {
        return -1;
    }
    
    if !infop.is_null() {
        (*infop).si_signo = 17; // SIGCHLD
        (*infop).si_errno = 0;
        
        if wifexited(status) {
            (*infop).si_code = 1; // CLD_EXITED
        } else if wifsignaled(status) {
            (*infop).si_code = 2; // CLD_KILLED
        } else if wifstopped(status) {
            (*infop).si_code = 5; // CLD_STOPPED
        } else {
            (*infop).si_code = 0;
        }
    }
    
    0
}

// ============================================================================
// Fork/Exec Functions
// ============================================================================

/// vfork - create a child process (implemented as fork)
#[no_mangle]
pub extern "C" fn vfork() -> crate::pid_t {
    crate::fork()
}

/// execvp - execute program (search PATH)
#[no_mangle]
pub extern "C" fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int {
    crate::execve(file as *const u8, argv as *const *const u8, ptr::null())
}

// ============================================================================
// posix_spawn Functions
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn posix_spawn_file_actions_init(
    file_actions: *mut posix_spawn_file_actions_t,
) -> c_int {
    if file_actions.is_null() {
        return crate::EINVAL;
    }
    ptr::write_bytes(file_actions as *mut u8, 0, core::mem::size_of::<posix_spawn_file_actions_t>());
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawn_file_actions_destroy(
    _file_actions: *mut posix_spawn_file_actions_t,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawn_file_actions_adddup2(
    _file_actions: *mut posix_spawn_file_actions_t,
    _oldfd: c_int,
    _newfd: c_int,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawn_file_actions_addclose(
    _file_actions: *mut posix_spawn_file_actions_t,
    _fd: c_int,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawn_file_actions_addopen(
    _file_actions: *mut posix_spawn_file_actions_t,
    _fd: c_int,
    _path: *const c_char,
    _oflag: c_int,
    _mode: crate::mode_t,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawn_file_actions_addchdir_np(
    _file_actions: *mut posix_spawn_file_actions_t,
    _path: *const c_char,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawnattr_init(attr: *mut posix_spawnattr_t) -> c_int {
    if attr.is_null() {
        return crate::EINVAL;
    }
    ptr::write_bytes(attr as *mut u8, 0, core::mem::size_of::<posix_spawnattr_t>());
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawnattr_destroy(_attr: *mut posix_spawnattr_t) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawnattr_setflags(
    _attr: *mut posix_spawnattr_t,
    _flags: i16,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawnattr_setsigmask(
    _attr: *mut posix_spawnattr_t,
    _sigmask: *const c_void,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawnattr_setsigdefault(
    _attr: *mut posix_spawnattr_t,
    _sigdefault: *const c_void,
) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn posix_spawnattr_setpgroup(
    _attr: *mut posix_spawnattr_t,
    _pgroup: crate::pid_t,
) -> c_int {
    0
}

/// posix_spawn - spawn a new process
#[no_mangle]
pub unsafe extern "C" fn posix_spawn(
    pid: *mut crate::pid_t,
    path: *const c_char,
    _file_actions: *const posix_spawn_file_actions_t,
    _attrp: *const posix_spawnattr_t,
    argv: *const *mut c_char,
    envp: *const *mut c_char,
) -> c_int {
    if path.is_null() || pid.is_null() {
        return crate::EINVAL;
    } 

    // DEBUG: Print path pointer before fork
    crate::debug_log_message(b"[posix_spawn] BEFORE fork, path_ptr=0x");
    crate::debug_log_hex(path as u64);
    crate::debug_log_message(b"\n");

    let child_pid = crate::fork();
    if child_pid < 0 {
        return crate::get_errno();
    }

    if child_pid == 0 {
        // Child process - exec the program
        // DEBUG: Print path pointer after fork in child
        crate::debug_log_message(b"[posix_spawn] CHILD after fork, path_ptr=0x");
        crate::debug_log_hex(path as u64);
        crate::debug_log_message(b"\n");
        
        let ret = crate::execve(
            path as *const u8,
            argv as *const *const u8,
            envp as *const *const u8,
        );
        // If execve returns, it failed
        crate::_exit(if ret < 0 { 127 } else { ret });
    }

    // Parent process
    *pid = child_pid;
    0
}

/// posix_spawnp - spawn a new process (search PATH)
#[no_mangle]
pub unsafe extern "C" fn posix_spawnp(
    pid: *mut crate::pid_t,
    file: *const c_char,
    file_actions: *const posix_spawn_file_actions_t,
    attrp: *const posix_spawnattr_t,
    argv: *const *mut c_char,
    envp: *const *mut c_char,
) -> c_int {
    // TODO: implement PATH search
    posix_spawn(pid, file, file_actions, attrp, argv, envp)
}

// ============================================================================
// Process ID Functions
// ============================================================================

/// getuid - get real user ID
#[no_mangle]
pub extern "C" fn getuid() -> crate::uid_t {
    0 // Always root in NexaOS for now
}

/// geteuid - get effective user ID
#[no_mangle]
pub extern "C" fn geteuid() -> crate::uid_t {
    0
}

/// getgid - get real group ID
#[no_mangle]
pub extern "C" fn getgid() -> crate::gid_t {
    0
}

/// getegid - get effective group ID
#[no_mangle]
pub extern "C" fn getegid() -> crate::gid_t {
    0
}

/// setuid - set user ID (stub)
#[no_mangle]
pub extern "C" fn setuid(_uid: crate::uid_t) -> c_int {
    0
}

/// setgid - set group ID (stub)
#[no_mangle]
pub extern "C" fn setgid(_gid: crate::gid_t) -> c_int {
    0
}

/// setsid - create a new session
#[no_mangle]
pub extern "C" fn setsid() -> crate::pid_t {
    crate::getpid()
}

/// setpgid - set process group ID
#[no_mangle]
pub extern "C" fn setpgid(_pid: crate::pid_t, _pgid: crate::pid_t) -> c_int {
    0
}

/// getpgid - get process group ID
#[no_mangle]
pub extern "C" fn getpgid(_pid: crate::pid_t) -> crate::pid_t {
    crate::getpid()
}

/// setgroups - set supplementary group IDs
#[no_mangle]
pub extern "C" fn setgroups(_size: size_t, _list: *const crate::gid_t) -> c_int {
    0
}

// ============================================================================
// Directory Change Functions (stubs)
// Note: chdir is now implemented in fs.rs
// ============================================================================

/// chroot - change root directory
#[no_mangle]
pub extern "C" fn chroot(_path: *const c_char) -> c_int {
    crate::set_errno(crate::ENOSYS);
    -1
}

// ============================================================================
// Pipe Functions
// ============================================================================

/// pipe2 - create pipe with flags
#[no_mangle]
pub extern "C" fn pipe2(pipefd: *mut c_int, _flags: c_int) -> c_int {
    crate::pipe(pipefd)
}

// ============================================================================
// Message Functions (stubs)
// ============================================================================

/// sendmsg - send a message on a socket
#[no_mangle]
pub extern "C" fn sendmsg(_sockfd: c_int, _msg: *const c_void, _flags: c_int) -> ssize_t {
    crate::set_errno(crate::ENOSYS);
    -1
}

/// recvmsg - receive a message from a socket
#[no_mangle]
pub extern "C" fn recvmsg(_sockfd: c_int, _msg: *mut c_void, _flags: c_int) -> ssize_t {
    crate::set_errno(crate::ENOSYS);
    -1
}
