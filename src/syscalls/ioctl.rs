//! ioctl syscall implementation (minimal)
//!
//! Currently used to support PTY (`/dev/ptmx` + `/dev/pts/<n>`) configuration.

use super::types::*;
use crate::posix;
use core::mem;
use core::ptr;

// Linux-compatible ioctl request numbers (x86_64)
const TCGETS: u64 = 0x5401;
const TCSETS: u64 = 0x5402;
const TCSETSW: u64 = 0x5403;
const TCSETSF: u64 = 0x5404;
const TIOCGWINSZ: u64 = 0x5413;
const TIOCSWINSZ: u64 = 0x5414;
const FIONREAD: u64 = 0x541B;

// PTY ioctls
const TIOCGPTN: u64 = 0x8004_5430;
const TIOCSPTLCK: u64 = 0x4004_5431;

pub fn ioctl(fd: u64, request: u64, arg: u64) -> u64 {
    // Only real fds; stdio is handled by nrlib compatibility shims today.
    if fd < FD_BASE {
        posix::set_errno(posix::errno::ENOTTY);
        return u64::MAX;
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = get_file_handle(idx) else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        match handle.backing {
            FileBacking::PtyMaster(id) => ioctl_pty(id as usize, true, request, arg),
            FileBacking::PtySlave(id) => ioctl_pty(id as usize, false, request, arg),
            FileBacking::DevLoop(index) => {
                match crate::drivers::r#loop::loop_device_ioctl(index as usize, request, arg) {
                    Ok(result) => {
                        posix::set_errno(0);
                        result as u64
                    }
                    Err(e) => {
                        posix::set_errno(e);
                        u64::MAX
                    }
                }
            }
            FileBacking::DevLoopControl => {
                match crate::drivers::r#loop::loop_control_ioctl(request, arg) {
                    Ok(result) => {
                        posix::set_errno(0);
                        result as u64
                    }
                    Err(e) => {
                        posix::set_errno(e);
                        u64::MAX
                    }
                }
            }
            FileBacking::DevInputEvent(index) => ioctl_input_event(index as usize, request, arg),
            FileBacking::DevInputMice => {
                // /dev/input/mice doesn't support most ioctls
                posix::set_errno(posix::errno::ENOTTY);
                u64::MAX
            }
            FileBacking::DevWatchdog => {
                match crate::drivers::watchdog::watchdog_ioctl(request, arg) {
                    Ok(result) => {
                        posix::set_errno(0);
                        result
                    }
                    Err(e) => {
                        posix::set_errno(e);
                        u64::MAX
                    }
                }
            }
            _ => {
                posix::set_errno(posix::errno::ENOTTY);
                u64::MAX
            }
        }
    }
}

// ============================================================================
// Input event ioctl handler
// ============================================================================

// Linux input ioctl commands
const EVIOCGVERSION: u64 = 0x80044501; // Get driver version
const EVIOCGID: u64 = 0x80084502; // Get device ID

fn ioctl_input_event(index: usize, request: u64, arg: u64) -> u64 {
    match request {
        EVIOCGVERSION => {
            // Return driver version (Linux uses 0x010001 = 1.0.1)
            if arg == 0 || !user_buffer_in_range(arg, mem::size_of::<i32>() as u64) {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }
            unsafe { ptr::write(arg as *mut i32, 0x010001) };
            posix::set_errno(0);
            0
        }
        EVIOCGID => {
            // Return device ID
            if arg == 0
                || !user_buffer_in_range(
                    arg,
                    mem::size_of::<crate::drivers::input::event::InputId>() as u64,
                )
            {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }
            if let Some(id) = crate::drivers::input::get_device_id(index) {
                unsafe { ptr::write(arg as *mut crate::drivers::input::event::InputId, id) };
                posix::set_errno(0);
                0
            } else {
                posix::set_errno(posix::errno::ENODEV);
                u64::MAX
            }
        }
        _ => {
            // Many input ioctls are optional
            posix::set_errno(posix::errno::ENOTTY);
            u64::MAX
        }
    }
}

fn ioctl_pty(id: usize, is_master: bool, request: u64, arg: u64) -> u64 {
    match request {
        TIOCGPTN => {
            if !is_master {
                posix::set_errno(posix::errno::ENOTTY);
                return u64::MAX;
            }
            if arg == 0 || !user_buffer_in_range(arg, mem::size_of::<u32>() as u64) {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }
            let Some(ptn) = crate::tty::pty::get_ptn(id) else {
                posix::set_errno(posix::errno::ENODEV);
                return u64::MAX;
            };
            unsafe {
                ptr::write(arg as *mut u32, ptn);
            }
            posix::set_errno(0);
            0
        }
        TIOCSPTLCK => {
            if !is_master {
                posix::set_errno(posix::errno::ENOTTY);
                return u64::MAX;
            }
            if arg == 0 || !user_buffer_in_range(arg, mem::size_of::<i32>() as u64) {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }
            let lock_val = unsafe { ptr::read(arg as *const i32) };
            let locked = lock_val != 0;
            if !crate::tty::pty::set_locked(id, locked) {
                posix::set_errno(posix::errno::ENODEV);
                return u64::MAX;
            }
            posix::set_errno(0);
            0
        }
        TCGETS => {
            if arg == 0
                || !user_buffer_in_range(arg, mem::size_of::<crate::tty::pty::Termios>() as u64)
            {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }
            let Some(t) = crate::tty::pty::get_termios(id) else {
                posix::set_errno(posix::errno::ENODEV);
                return u64::MAX;
            };
            unsafe { ptr::write(arg as *mut crate::tty::pty::Termios, t) };
            posix::set_errno(0);
            0
        }
        TCSETS | TCSETSW | TCSETSF => {
            if arg == 0
                || !user_buffer_in_range(arg, mem::size_of::<crate::tty::pty::Termios>() as u64)
            {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }
            let t = unsafe { ptr::read(arg as *const crate::tty::pty::Termios) };
            if !crate::tty::pty::set_termios(id, t) {
                posix::set_errno(posix::errno::ENODEV);
                return u64::MAX;
            }
            posix::set_errno(0);
            0
        }
        TIOCGWINSZ => {
            if arg == 0
                || !user_buffer_in_range(arg, mem::size_of::<crate::tty::pty::WinSize>() as u64)
            {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }
            let Some(w) = crate::tty::pty::get_winsize(id) else {
                posix::set_errno(posix::errno::ENODEV);
                return u64::MAX;
            };
            unsafe { ptr::write(arg as *mut crate::tty::pty::WinSize, w) };
            posix::set_errno(0);
            0
        }
        TIOCSWINSZ => {
            if arg == 0
                || !user_buffer_in_range(arg, mem::size_of::<crate::tty::pty::WinSize>() as u64)
            {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }
            let w = unsafe { ptr::read(arg as *const crate::tty::pty::WinSize) };
            if !crate::tty::pty::set_winsize(id, w) {
                posix::set_errno(posix::errno::ENODEV);
                return u64::MAX;
            }
            posix::set_errno(0);
            0
        }
        FIONREAD => {
            if arg == 0 || !user_buffer_in_range(arg, mem::size_of::<i32>() as u64) {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }
            let dir = if is_master {
                crate::tty::pty::PtyDirection::MasterReads
            } else {
                crate::tty::pty::PtyDirection::SlaveReads
            };
            let available = crate::tty::pty::bytes_available_for_read(id, dir);
            unsafe { ptr::write(arg as *mut i32, available as i32) };
            posix::set_errno(0);
            0
        }
        _ => {
            posix::set_errno(posix::errno::ENOTTY);
            u64::MAX
        }
    }
}
