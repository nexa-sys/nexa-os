//! User management syscalls
//!
//! Implements: user_add, user_login, user_info, user_list, user_logout

use super::types::*;
use crate::posix;
use crate::kinfo;
use core::{fmt::Write, ptr, slice, str};

/// User add system call
pub fn user_add(request_ptr: *const UserRequest) -> u64 {
    if request_ptr.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if !crate::auth::require_admin() {
        posix::set_errno(posix::errno::EACCES);
        return u64::MAX;
    }

    let request = unsafe { &*request_ptr };
    if request.username_ptr == 0 || request.password_ptr == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let username_bytes = unsafe {
        slice::from_raw_parts(
            request.username_ptr as *const u8,
            request.username_len as usize,
        )
    };
    let password_bytes = unsafe {
        slice::from_raw_parts(
            request.password_ptr as *const u8,
            request.password_len as usize,
        )
    };

    let username = match str::from_utf8(username_bytes) {
        Ok(name) => name,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let password = match str::from_utf8(password_bytes) {
        Ok(pass) => pass,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    match crate::auth::create_user(username, password, (request.flags & USER_FLAG_ADMIN) != 0) {
        Ok(uid) => {
            posix::set_errno(0);
            uid as u64
        }
        Err(err) => {
            posix::set_errno(map_auth_error(err));
            u64::MAX
        }
    }
}

/// User login system call
pub fn user_login(request_ptr: *const UserRequest) -> u64 {
    if request_ptr.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let request = unsafe { &*request_ptr };
    if request.username_ptr == 0 || request.password_ptr == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    kinfo!(
        "user_login: request username_ptr={:#x} len={} password_ptr={:#x} len={}",
        request.username_ptr,
        request.username_len,
        request.password_ptr,
        request.password_len
    );

    let username_bytes = unsafe {
        slice::from_raw_parts(
            request.username_ptr as *const u8,
            request.username_len as usize,
        )
    };
    let password_bytes = unsafe {
        slice::from_raw_parts(
            request.password_ptr as *const u8,
            request.password_len as usize,
        )
    };

    kinfo!(
        "user_login: username bytes={:02x?} password bytes={:02x?}",
        username_bytes,
        password_bytes
    );

    let username = match str::from_utf8(username_bytes) {
        Ok(name) => name,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let password = match str::from_utf8(password_bytes) {
        Ok(pass) => pass,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    kinfo!(
        "user_login: parsed username='{}' password_len={} password_has_null?={}",
        username,
        password.len(),
        password_bytes.iter().any(|&b| b == 0)
    );

    match crate::auth::authenticate(username, password) {
        Ok(creds) => {
            posix::set_errno(0);
            creds.uid as u64
        }
        Err(err) => {
            posix::set_errno(map_auth_error(err));
            u64::MAX
        }
    }
}

/// User info system call
pub fn user_info(info_ptr: *mut UserInfoReply) -> u64 {
    if info_ptr.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let info = crate::auth::current_user();
    let mut reply = UserInfoReply {
        username: [0; 32],
        username_len: 0,
        uid: info.credentials.uid,
        gid: info.credentials.gid,
        is_admin: if info.credentials.is_admin { 1 } else { 0 },
    };
    let copy_len = core::cmp::min(info.username_len, reply.username.len());
    reply.username[..copy_len].copy_from_slice(&info.username[..copy_len]);
    reply.username_len = copy_len as u64;

    unsafe {
        ptr::write(info_ptr, reply);
    }
    posix::set_errno(0);
    0
}

/// User list system call
pub fn user_list(buf_ptr: *mut u8, count: usize) -> u64 {
    if buf_ptr.is_null() || count == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let buffer = unsafe { slice::from_raw_parts_mut(buf_ptr, count) };
    let mut writer = BufferWriter::new(buffer);

    crate::auth::enumerate_users(|summary| {
        if writer.overflowed() {
            return;
        }

        let username = summary.username_str();
        let admin_flag = if summary.is_admin { 1 } else { 0 };
        let _ = write!(
            writer,
            "{} uid={} gid={} admin={}\n",
            username, summary.uid, summary.gid, admin_flag
        );
    });

    if writer.overflowed() {
        posix::set_errno(posix::errno::EAGAIN);
    } else {
        posix::set_errno(0);
    }

    writer.written() as u64
}

/// User logout system call
pub fn user_logout() -> u64 {
    match crate::auth::logout() {
        Ok(_) => {
            posix::set_errno(0);
            0
        }
        Err(err) => {
            posix::set_errno(map_auth_error(err));
            u64::MAX
        }
    }
}
