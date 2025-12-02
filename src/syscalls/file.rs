//! File system related syscalls
//!
//! Implements: read, write, open, close, stat, fstat, lseek, fcntl, list_files

use super::types::*;
use crate::posix::{self, FileType};
use crate::process::{Process, ProcessState, USER_REGION_SIZE, USER_VIRT_BASE};
use crate::scheduler;
use crate::vt;
use crate::{kdebug, kerror, kinfo, ktrace, kwarn};
use alloc::boxed::Box;
use core::{cmp, ptr, slice, str};

/// Write to standard stream (stdout/stderr)
pub fn write_to_std_stream(kind: StdStreamKind, buf: u64, count: u64) -> u64 {
    if !user_buffer_in_range(buf, count) {
        let (stack_base, stack_top) = current_stack_bounds();
        kwarn!(
            "sys_write: invalid user buffer fd={} buf={:#x} count={} stack_base={:#x} stack_top={:#x}",
            kind.fd(),
            buf,
            count,
            stack_base,
            stack_top
        );
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    if buf >= 0x8000_0000 {
        let (stack_base, stack_top) = current_stack_bounds();
        kwarn!(
            "sys_write: high user buffer fd={} buf={:#x} count={} stack_base={:#x} stack_top={:#x}",
            kind.fd(),
            buf,
            count,
            stack_base,
            stack_top
        );
    }

    let slice = unsafe { slice::from_raw_parts(buf as *const u8, count as usize) };
    ktrace!("Serial write bytes: {:?}", slice);

    let tty = scheduler::get_current_pid()
        .and_then(|pid| scheduler::get_process(pid))
        .map(|proc: Process| proc.tty())
        .unwrap_or_else(|| vt::active_terminal());

    let stream = match kind {
        StdStreamKind::Stdout => vt::StreamKind::Stdout,
        StdStreamKind::Stderr => vt::StreamKind::Stderr,
        StdStreamKind::Stdin => vt::StreamKind::Input,
    };

    vt::write_bytes(tty, slice, stream);

    posix::set_errno(0);
    count
}

/// Write system call
pub fn write(fd: u64, buf: u64, count: u64) -> u64 {
    if count == 0 {
        posix::set_errno(0);
        return 0;
    }

    if buf == 0 {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    if fd == STDOUT {
        return write_to_std_stream(StdStreamKind::Stdout, buf, count);
    }

    if fd == STDERR {
        return write_to_std_stream(StdStreamKind::Stderr, buf, count);
    }

    if fd < FD_BASE {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        if let Some(handle) = get_file_handle(idx) {
            match handle.backing {
                FileBacking::StdStream(StdStreamKind::Stdout) => {
                    return write_to_std_stream(StdStreamKind::Stdout, buf, count);
                }
                FileBacking::StdStream(StdStreamKind::Stderr) => {
                    return write_to_std_stream(StdStreamKind::Stderr, buf, count);
                }
                FileBacking::StdStream(StdStreamKind::Stdin) => {
                    posix::set_errno(posix::errno::EBADF);
                    return u64::MAX;
                }
                FileBacking::Socket(sock_handle) => {
                    // Handle TCP socket write
                    if sock_handle.socket_type == SOCK_STREAM {
                        ktrace!(
                            "[SYS_WRITE] TCP socket fd={} index={} count={}",
                            fd,
                            sock_handle.socket_index,
                            count
                        );

                        if sock_handle.socket_index == usize::MAX {
                            ktrace!("[SYS_WRITE] ERROR: Invalid socket index");
                            posix::set_errno(posix::errno::EBADF);
                            return u64::MAX;
                        }

                        if !user_buffer_in_range(buf, count) {
                            ktrace!("[SYS_WRITE] ERROR: Buffer out of range");
                            posix::set_errno(posix::errno::EFAULT);
                            return u64::MAX;
                        }

                        let data = core::slice::from_raw_parts(buf as *const u8, count as usize);

                        // Send data and poll to transmit
                        let (send_result, tx) = if let Some(res) =
                            crate::net::with_net_stack(|stack| {
                                let result = stack.tcp_send(sock_handle.socket_index, data);
                                let mut tx = Box::new(crate::net::stack::TxBatch::new());
                                if result.is_ok() {
                                    if let Err(e) =
                                        stack.tcp_poll(sock_handle.socket_index, &mut tx)
                                    {
                                        ktrace!("[SYS_WRITE] WARNING: tcp_poll failed: {:?}", e);
                                    }
                                }
                                (result, tx)
                            }) {
                            res
                        } else {
                            ktrace!("[SYS_WRITE] ERROR: Network stack unavailable");
                            posix::set_errno(posix::errno::ENETDOWN);
                            return u64::MAX;
                        };

                        // Transmit frames after releasing network stack lock
                        if !tx.is_empty() {
                            ktrace!("[SYS_WRITE] Transmitting {} frame(s)", tx.len());
                            if let Err(e) = crate::net::send_frames(sock_handle.device_index, &tx) {
                                ktrace!("[SYS_WRITE] ERROR: Failed to transmit frames: {:?}", e);
                                kwarn!("[SYS_WRITE] Failed to transmit frames: {:?}", e);
                            }
                        }

                        match send_result {
                            Ok(bytes_sent) => {
                                ktrace!("[SYS_WRITE] TCP sent {} bytes", bytes_sent);
                                posix::set_errno(0);
                                return bytes_sent as u64;
                            }
                            Err(e) => {
                                ktrace!("[SYS_WRITE] ERROR: tcp_send failed: {:?}", e);
                                posix::set_errno(posix::errno::EIO);
                                return u64::MAX;
                            }
                        }
                    }

                    // UDP socket - not supported via write(), must use sendto()
                    ktrace!("[SYS_WRITE] ERROR: UDP socket cannot use write(), use sendto()");
                    posix::set_errno(posix::errno::ENOTSUP);
                    return u64::MAX;
                }
                FileBacking::Socketpair(sp_handle) => {
                    ktrace!(
                        "[SYS_WRITE] Socketpair fd={} pair_id={}.{} count={}",
                        fd,
                        sp_handle.pair_id,
                        sp_handle.end,
                        count
                    );

                    if !user_buffer_in_range(buf, count) {
                        ktrace!("[SYS_WRITE] ERROR: Buffer out of range");
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }

                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);

                    match crate::ipc::socketpair_write(sp_handle.pair_id, sp_handle.end, data) {
                        Ok(bytes_written) => {
                            ktrace!(
                                "[SYS_WRITE] Socketpair wrote {} bytes",
                                bytes_written
                            );
                            posix::set_errno(0);
                            return bytes_written as u64;
                        }
                        Err(_) => {
                            ktrace!("[SYS_WRITE] ERROR: Socketpair write failed (peer closed)");
                            posix::set_errno(posix::errno::EPIPE);
                            return u64::MAX;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

/// Read from keyboard input
pub fn read_from_keyboard(buf: *mut u8, count: usize) -> u64 {
    use x86_64::instructions::interrupts;

    if count == 0 {
        return 0;
    }

    if !user_buffer_in_range(buf as u64, count as u64) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let were_enabled = interrupts::are_enabled();
    if !were_enabled {
        interrupts::enable();
    }

    let slice = unsafe { core::slice::from_raw_parts_mut(buf, count) };
    let tty = scheduler::get_current_pid()
        .and_then(|pid| scheduler::get_process(pid))
        .map(|proc: Process| proc.tty())
        .unwrap_or_else(|| vt::active_terminal());
    let read_len = crate::keyboard::read_raw_for_tty(tty, slice, count);

    if !were_enabled {
        interrupts::disable();
    }

    posix::set_errno(0);
    read_len as u64
}

/// Read system call
pub fn read(fd: u64, buf: *mut u8, count: usize) -> u64 {
    kinfo!("sys_read(fd={}, count={})", fd, count);
    if buf.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    if !user_buffer_in_range(buf as u64, count as u64) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    if count == 0 {
        posix::set_errno(0);
        return 0;
    }

    if fd == STDIN {
        return read_from_keyboard(buf, count);
    }

    if fd < FD_BASE {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        if let Some(handle) = get_file_handle(idx) {
            match handle.backing {
                FileBacking::StdStream(StdStreamKind::Stdin) => {
                    return read_from_keyboard(buf, count);
                }
                FileBacking::StdStream(_) => {
                    posix::set_errno(posix::errno::EBADF);
                    return u64::MAX;
                }
                FileBacking::Socket(sock_handle) => {
                    // Handle TCP socket read
                    if sock_handle.socket_type == SOCK_STREAM {
                        if sock_handle.socket_index == usize::MAX {
                            posix::set_errno(posix::errno::EBADF);
                            return u64::MAX;
                        }

                        let buffer = core::slice::from_raw_parts_mut(buf, count);

                        // Blocking TCP read
                        loop {
                            crate::net::poll();

                            let result = crate::net::with_net_stack(|stack| {
                                stack.tcp_recv(sock_handle.socket_index, buffer)
                            });

                            match result {
                                Some(Ok(bytes_recv)) => {
                                    ktrace!(
                                        "[syscall_read] TCP socket {}: returning {} bytes to PID {:?}",
                                        sock_handle.socket_index,
                                        bytes_recv,
                                        scheduler::current_pid()
                                    );
                                    posix::set_errno(0);
                                    return bytes_recv as u64;
                                }
                                Some(Err(crate::net::NetError::WouldBlock)) => {
                                    if let Some(current_pid) = scheduler::current_pid() {
                                        kinfo!(
                                            "sys_read: TCP socket {}: no data, adding PID {} to wait queue",
                                            sock_handle.socket_index, current_pid
                                        );

                                        let _ = crate::net::with_net_stack(|stack| {
                                            stack.tcp_add_waiter(
                                                sock_handle.socket_index,
                                                current_pid,
                                            )
                                        });
                                    }

                                    scheduler::set_current_process_state(ProcessState::Sleeping);
                                    continue;
                                }
                                Some(Err(_)) => {
                                    posix::set_errno(posix::errno::EIO);
                                    return u64::MAX;
                                }
                                None => {
                                    posix::set_errno(posix::errno::ENETDOWN);
                                    return u64::MAX;
                                }
                            }
                        }
                    }

                    posix::set_errno(posix::errno::ENOTSUP);
                    return u64::MAX;
                }
                FileBacking::Socketpair(sp_handle) => {
                    let buffer = core::slice::from_raw_parts_mut(buf, count);

                    // Non-blocking read from socketpair
                    match crate::ipc::socketpair_read(sp_handle.pair_id, sp_handle.end, buffer) {
                        Ok(bytes_read) => {
                            ktrace!(
                                "[sys_read] Socketpair {}.{}: read {} bytes",
                                sp_handle.pair_id,
                                sp_handle.end,
                                bytes_read
                            );
                            posix::set_errno(0);
                            return bytes_read as u64;
                        }
                        Err(_) => {
                            // Socketpair closed or error
                            posix::set_errno(posix::errno::EPIPE);
                            return u64::MAX;
                        }
                    }
                }
                FileBacking::Inline(data) => {
                    let remaining = data.len().saturating_sub(handle.position);
                    if remaining == 0 {
                        posix::set_errno(0);
                        return 0;
                    }
                    let to_copy = cmp::min(remaining, count);
                    ptr::copy_nonoverlapping(data.as_ptr().add(handle.position), buf, to_copy);
                    // Update position using accessor function
                    update_file_handle_position(idx, handle.position + to_copy);
                    posix::set_errno(0);
                    return to_copy as u64;
                }
                FileBacking::Ext2(file_ref) => {
                    let total = handle.metadata.size as usize;
                    if handle.position >= total {
                        posix::set_errno(0);
                        return 0;
                    }
                    let remaining = total - handle.position;
                    let to_read = cmp::min(remaining, count);
                    let dest = slice::from_raw_parts_mut(buf, to_read);
                    let read = file_ref.read_at(handle.position, dest);
                    // Update position using accessor function
                    update_file_handle_position(idx, handle.position.saturating_add(read));
                    posix::set_errno(0);
                    return read as u64;
                }
            }
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

/// Open system call
pub fn open(path_ptr: *const u8, len: usize) -> u64 {
    if path_ptr.is_null() || len == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let raw = unsafe { slice::from_raw_parts(path_ptr, len) };
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    let trimmed = &raw[..end];
    let Ok(mut path) = str::from_utf8(trimmed) else {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    };

    path = path.trim();
    if path.is_empty() {
        posix::set_errno(posix::errno::ENOENT);
        return u64::MAX;
    }

    let normalized = path;

    if let Some(opened) = crate::fs::open(normalized) {
        if matches!(opened.metadata.file_type, FileType::Directory) {
            posix::set_errno(posix::errno::EISDIR);
            return u64::MAX;
        }

        let crate::fs::OpenFile { content, metadata } = opened;
        let backing = match content {
            crate::fs::FileContent::Inline(data) => FileBacking::Inline(data),
            crate::fs::FileContent::Ext2Modular(file_ref) => FileBacking::Ext2(file_ref),
        };

        unsafe {
            if let Some(index) = find_empty_file_handle_slot() {
                set_file_handle(
                    index,
                    Some(FileHandle {
                        backing,
                        position: 0,
                        metadata,
                    }),
                );
                posix::set_errno(0);
                let fd = FD_BASE + index as u64;
                kinfo!("Opened file '{}' as fd {}", normalized, fd);
                return fd;
            }
        }
        posix::set_errno(posix::errno::EMFILE);
        kwarn!("No free file handles available");
        u64::MAX
    } else {
        posix::set_errno(posix::errno::ENOENT);
        kwarn!("sys_open: file '{}' not found", normalized);
        u64::MAX
    }
}

/// Close system call
pub fn close(fd: u64) -> u64 {
    if fd < FD_BASE {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }
    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        if let Some(handle) = get_file_handle(idx) {
            // Clean up socket resources if this is a socket
            if let FileBacking::Socket(ref sock_handle) = handle.backing {
                // Close netlink socket in network stack
                if sock_handle.domain == AF_NETLINK && sock_handle.socket_index != usize::MAX {
                    if let Some(_) = crate::net::with_net_stack(|stack| {
                        stack.netlink_close(sock_handle.socket_index)
                    }) {
                        kinfo!(
                            "Closed netlink socket {} for fd {}",
                            sock_handle.socket_index,
                            fd
                        );
                    }
                }
                // Close TCP socket
                else if sock_handle.socket_type == SOCK_STREAM
                    && sock_handle.socket_index != usize::MAX
                {
                    if let Some(_) = crate::net::with_net_stack(|stack| {
                        stack.tcp_close(sock_handle.socket_index)
                    }) {
                        kinfo!(
                            "Closed TCP socket {} for fd {}",
                            sock_handle.socket_index,
                            fd
                        );
                    }
                }
            }
            // Clean up socketpair resources
            else if let FileBacking::Socketpair(ref sp_handle) = handle.backing {
                if let Err(_) = crate::ipc::close_socketpair_end(sp_handle.pair_id, sp_handle.end) {
                    kinfo!(
                        "Warning: Failed to close socketpair {}.{} for fd {}",
                        sp_handle.pair_id,
                        sp_handle.end,
                        fd
                    );
                } else {
                    kinfo!(
                        "Closed socketpair {}.{} for fd {}",
                        sp_handle.pair_id,
                        sp_handle.end,
                        fd
                    );
                }
            }

            clear_file_handle(idx);
            kinfo!("Closed fd {}", fd);
            posix::set_errno(0);
            return 0;
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

/// List files system call
pub fn list_files(buf: *mut u8, count: usize, request_ptr: *const ListDirRequest) -> u64 {
    crate::serial_println!("SYSCALL_LIST_FILES: buf={:#x} count={} req={:#x}", 
        buf as u64, count, request_ptr as u64);
    
    if buf.is_null() || count == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let mut include_hidden = false;
    let mut path = "/";

    if !request_ptr.is_null() {
        let request = unsafe { &*request_ptr };
        include_hidden = (request.flags & LIST_FLAG_INCLUDE_HIDDEN) != 0;
        if request.path_ptr != 0 && request.path_len > 0 {
            let raw = unsafe {
                slice::from_raw_parts(request.path_ptr as *const u8, request.path_len as usize)
            };
            match str::from_utf8(raw) {
                Ok(p) => {
                    let trimmed = p.trim();
                    if !trimmed.is_empty() {
                        path = trimmed;
                    }
                }
                Err(_) => {
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
            }
        }
    }

    let normalized = if path.is_empty() { "/" } else { path };

    crate::serial_println!("SYSCALL_LIST_FILES: normalized='{}' (is_root={})", 
        normalized, normalized == "/");

    if normalized != "/" {
        crate::serial_println!("SYSCALL_LIST_FILES: calling stat('{}')", normalized);
        match crate::fs::stat(normalized) {
            Some(meta) => {
                if meta.file_type != FileType::Directory {
                    posix::set_errno(posix::errno::ENOTDIR);
                    return u64::MAX;
                }
            }
            None => {
                posix::set_errno(posix::errno::ENOENT);
                return u64::MAX;
            }
        }
        crate::serial_println!("SYSCALL_LIST_FILES: stat done");
    }

    let mut written = 0usize;
    let mut overflow = false;

    crate::serial_println!("SYSCALL_LIST_FILES: calling list_directory path='{}'", normalized);

    crate::fs::list_directory(normalized, |name, _meta| {
        if overflow {
            return;
        }
        if !include_hidden && name.starts_with('.') {
            return;
        }
        let name_bytes = name.as_bytes();
        let needed = name_bytes.len() + 1;
        if written + needed > count {
            overflow = true;
            return;
        }
        unsafe {
            ptr::copy_nonoverlapping(name_bytes.as_ptr(), buf.add(written), name_bytes.len());
            written += name_bytes.len();
            *buf.add(written) = b'\n';
            written += 1;
        }
    });
    
    crate::serial_println!("SYSCALL_LIST_FILES: done, written={}", written);

    if overflow {
        posix::set_errno(posix::errno::EAGAIN);
    } else {
        posix::set_errno(0);
    }
    written as u64
}

/// Stat system call
pub fn stat(path_ptr: *const u8, len: usize, stat_buf: *mut posix::Stat) -> u64 {
    if path_ptr.is_null() || stat_buf.is_null() || len == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let raw = unsafe { slice::from_raw_parts(path_ptr as *const u8, len) };
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    let trimmed = &raw[..end];
    let Ok(mut path) = str::from_utf8(trimmed) else {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    };

    path = path.trim();
    if path.is_empty() {
        posix::set_errno(posix::errno::ENOENT);
        return u64::MAX;
    }

    if let Some(metadata) = crate::fs::stat(path) {
        let stat = posix::Stat::from_metadata(&metadata);
        unsafe {
            ptr::write(stat_buf, stat);
        }
        posix::set_errno(0);
        0
    } else {
        posix::set_errno(posix::errno::ENOENT);
        u64::MAX
    }
}

/// Fstat system call
pub fn fstat(fd: u64, stat_buf: *mut posix::Stat) -> u64 {
    if stat_buf.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }
    let handle = match handle_for_fd(fd) {
        Ok(handle) => handle,
        Err(errno) => {
            posix::set_errno(errno);
            return u64::MAX;
        }
    };

    let stat = posix::Stat::from_metadata(&handle.metadata);
    unsafe {
        ptr::write(stat_buf, stat);
    }
    posix::set_errno(0);
    0
}

/// Lseek system call
pub fn lseek(fd: u64, offset: i64, whence: u64) -> u64 {
    if fd < FD_BASE {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }
    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        if let Some(handle) = get_file_handle(idx) {
            match handle.backing {
                FileBacking::StdStream(_) => {
                    posix::set_errno(posix::errno::ESPIPE);
                    return u64::MAX;
                }
                _ => {}
            }

            let base = match whence {
                0 => 0i64,
                1 => handle.position as i64,
                2 => handle.metadata.size as i64,
                _ => {
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
            };

            let new_pos = base.saturating_add(offset);
            if new_pos < 0 {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }

            let new_pos_u64 = new_pos as u64;
            let limited = new_pos_u64.min(usize::MAX as u64);
            update_file_handle_position(idx, limited as usize);
            posix::set_errno(0);
            return new_pos_u64;
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

/// Fcntl system call
pub fn fcntl(fd: u64, cmd: u64, arg: u64) -> u64 {
    match cmd {
        F_DUPFD => {
            let handle = match handle_for_fd(fd) {
                Ok(handle) => handle,
                Err(errno) => {
                    posix::set_errno(errno);
                    return u64::MAX;
                }
            };

            let requested_min = (arg as i32) as i64;
            if requested_min < 0 {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }

            let min_fd = requested_min.max(FD_BASE as i64) as u64;
            match allocate_duplicate_slot(min_fd, handle) {
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
        F_GETFL | F_SETFL => {
            posix::set_errno(0);
            0
        }
        _ => {
            kwarn!("fcntl: unsupported cmd={} for fd={}", cmd, fd);
            posix::set_errno(posix::errno::ENOSYS);
            u64::MAX
        }
    }
}

/// Get errno system call
pub fn get_errno() -> u64 {
    posix::errno() as u64
}
