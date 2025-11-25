//! IPC related syscalls
//!
//! Implements: ipc_create, ipc_send, ipc_recv

use super::types::*;
use crate::posix;
use core::slice;

/// IPC create channel system call
pub fn syscall_ipc_create() -> u64 {
    match crate::ipc::create_channel() {
        Ok(id) => {
            posix::set_errno(0);
            id as u64
        }
        Err(err) => {
            posix::set_errno(map_ipc_error(err));
            u64::MAX
        }
    }
}

/// IPC send system call
pub fn syscall_ipc_send(request_ptr: *const IpcTransferRequest) -> u64 {
    if request_ptr.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }
    let request = unsafe { &*request_ptr };
    if request.buffer_ptr == 0 || request.buffer_len == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let data = unsafe {
        slice::from_raw_parts(request.buffer_ptr as *const u8, request.buffer_len as usize)
    };

    match crate::ipc::send(request.channel_id, data) {
        Ok(()) => {
            posix::set_errno(0);
            0
        }
        Err(err) => {
            posix::set_errno(map_ipc_error(err));
            u64::MAX
        }
    }
}

/// IPC receive system call
pub fn syscall_ipc_recv(request_ptr: *const IpcTransferRequest) -> u64 {
    if request_ptr.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }
    let request = unsafe { &*request_ptr };
    if request.buffer_ptr == 0 || request.buffer_len == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let buffer = unsafe {
        slice::from_raw_parts_mut(request.buffer_ptr as *mut u8, request.buffer_len as usize)
    };

    match crate::ipc::receive(request.channel_id, buffer) {
        Ok(bytes) => {
            posix::set_errno(0);
            bytes as u64
        }
        Err(err) => {
            posix::set_errno(map_ipc_error(err));
            u64::MAX
        }
    }
}
