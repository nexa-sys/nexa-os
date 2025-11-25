//! File descriptor operations
//!
//! Implements: dup, dup2, pipe

use super::types::*;
use crate::posix;

/// POSIX dup() system call - duplicate file descriptor
pub fn syscall_dup(oldfd: u64) -> u64 {
    let handle = match handle_for_fd(oldfd) {
        Ok(handle) => handle,
        Err(errno) => {
            posix::set_errno(errno);
            return u64::MAX;
        }
    };

    match allocate_duplicate_slot(FD_BASE, handle) {
        Ok(fd) => {
            posix::set_errno(0);
            fd
        }
        Err(errno) => {
            posix::set_errno(errno);
            u64::MAX
        }
    }
}

/// POSIX dup2() system call - duplicate file descriptor to specific FD
pub fn syscall_dup2(oldfd: u64, newfd: u64) -> u64 {
    if oldfd == newfd {
        posix::set_errno(0);
        return newfd;
    }

    let handle = match handle_for_fd(oldfd) {
        Ok(handle) => handle,
        Err(errno) => {
            posix::set_errno(errno);
            return u64::MAX;
        }
    };

    if newfd < FD_BASE {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    let idx = (newfd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        FILE_HANDLES[idx] = Some(handle);
    }

    posix::set_errno(0);
    newfd
}

/// POSIX pipe() system call - creates a pipe
pub fn syscall_pipe(pipefd: *mut [i32; 2]) -> u64 {
    if pipefd.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    match crate::pipe::create_pipe() {
        Ok((read_fd, write_fd)) => {
            unsafe {
                (*pipefd)[0] = read_fd as i32;
                (*pipefd)[1] = write_fd as i32;
            }
            posix::set_errno(0);
            0
        }
        Err(_) => {
            posix::set_errno(posix::errno::EMFILE);
            u64::MAX
        }
    }
}
