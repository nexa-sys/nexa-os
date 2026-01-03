//! File system related syscalls
//!
//! Implements: read, write, open, close, stat, fstat, lseek, fcntl, list_files

use super::types::*;
use crate::posix::{self, FileType};
use crate::process::{Process, ProcessState};
use crate::scheduler;
use crate::vt;
use crate::{kinfo, ktrace, kwarn};
use alloc::boxed::Box;
use alloc::string::String;
use core::{cmp, ptr, slice, str};

/// Mark a file descriptor as open in the current process's open_fds bitmap
pub fn mark_fd_open(fd: u64) {
    if fd < FD_BASE {
        return; // Don't track stdin/stdout/stderr
    }
    let bit = (fd - FD_BASE) as usize;
    if bit >= MAX_OPEN_FILES {
        return;
    }

    if let Some(pid) = scheduler::get_current_pid() {
        let mut table = scheduler::process_table_lock();
        if let Some(idx) = crate::process::lookup_pid(pid) {
            let idx = idx as usize;
            if idx < table.len() {
                if let Some(entry) = &mut table[idx] {
                    if entry.process.pid == pid {
                        entry.process.open_fds |= 1 << bit;
                        ktrace!(
                            "[mark_fd_open] PID {} fd {} marked open, open_fds={:#06x}",
                            pid,
                            fd,
                            entry.process.open_fds
                        );
                    }
                }
            }
        }
    }
}

/// Mark a file descriptor as closed in the current process's open_fds bitmap
pub fn mark_fd_closed(fd: u64) {
    if fd < FD_BASE {
        return; // Don't track stdin/stdout/stderr
    }
    let bit = (fd - FD_BASE) as usize;
    if bit >= MAX_OPEN_FILES {
        return;
    }

    if let Some(pid) = scheduler::get_current_pid() {
        let mut table = scheduler::process_table_lock();
        if let Some(idx) = crate::process::lookup_pid(pid) {
            let idx = idx as usize;
            if idx < table.len() {
                if let Some(entry) = &mut table[idx] {
                    if entry.process.pid == pid {
                        entry.process.open_fds &= !(1 << bit);
                        ktrace!(
                            "[mark_fd_closed] PID {} fd {} marked closed, open_fds={:#06x}",
                            pid,
                            fd,
                            entry.process.open_fds
                        );
                    }
                }
            }
        }
    }
}

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
                            ktrace!("[SYS_WRITE] Socketpair wrote {} bytes", bytes_written);
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
                FileBacking::PtyMaster(id) => {
                    if !user_buffer_in_range(buf, count) {
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }

                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);
                    let mut written_total = 0usize;

                    while written_total < data.len() {
                        match crate::tty::pty::try_write(
                            id as usize,
                            crate::tty::pty::PtyDirection::SlaveReads,
                            &data[written_total..],
                        ) {
                            crate::tty::pty::PtyIoResult::Bytes(n) => {
                                written_total += n;
                                if n == 0 {
                                    break;
                                }
                            }
                            crate::tty::pty::PtyIoResult::WouldBlock => {
                                scheduler::set_current_process_state(ProcessState::Sleeping);
                                scheduler::do_schedule();
                                continue;
                            }
                            crate::tty::pty::PtyIoResult::Eof => {
                                posix::set_errno(posix::errno::EPIPE);
                                return u64::MAX;
                            }
                        }
                    }

                    posix::set_errno(0);
                    return written_total as u64;
                }
                FileBacking::PtySlave(id) => {
                    if !user_buffer_in_range(buf, count) {
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }

                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);
                    let mut written_total = 0usize;

                    while written_total < data.len() {
                        match crate::tty::pty::try_write(
                            id as usize,
                            crate::tty::pty::PtyDirection::MasterReads,
                            &data[written_total..],
                        ) {
                            crate::tty::pty::PtyIoResult::Bytes(n) => {
                                written_total += n;
                                if n == 0 {
                                    break;
                                }
                            }
                            crate::tty::pty::PtyIoResult::WouldBlock => {
                                scheduler::set_current_process_state(ProcessState::Sleeping);
                                scheduler::do_schedule();
                                continue;
                            }
                            crate::tty::pty::PtyIoResult::Eof => {
                                posix::set_errno(posix::errno::EPIPE);
                                return u64::MAX;
                            }
                        }
                    }

                    posix::set_errno(0);
                    return written_total as u64;
                }
                FileBacking::Modular(file_handle) => {
                    ktrace!(
                        "[SYS_WRITE] Modular fs fd={} fs_index={} inode={} count={} position={}",
                        fd,
                        file_handle.fs_index,
                        file_handle.inode,
                        count,
                        handle.position
                    );

                    // Check if filesystem is writable
                    if !crate::fs::modular_fs_is_writable(file_handle.fs_index) {
                        ktrace!("[SYS_WRITE] ERROR: modular filesystem is read-only");
                        posix::set_errno(posix::errno::EROFS);
                        return u64::MAX;
                    }

                    if !user_buffer_in_range(buf, count) {
                        ktrace!("[SYS_WRITE] ERROR: Buffer out of range");
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }

                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);

                    // Write to modular file at current position
                    match crate::fs::modular_fs_write_at(&file_handle, handle.position, data) {
                        Ok(bytes_written) => {
                            // Update position
                            update_file_handle_position(idx, handle.position + bytes_written);
                            ktrace!("[SYS_WRITE] Modular fs wrote {} bytes", bytes_written);
                            posix::set_errno(0);
                            return bytes_written as u64;
                        }
                        Err(e) => {
                            ktrace!("[SYS_WRITE] ERROR: Modular fs write failed: {:?}", e);
                            posix::set_errno(posix::errno::EIO);
                            return u64::MAX;
                        }
                    }
                }
                #[allow(deprecated)]
                FileBacking::Ext2(file_ref) => {
                    ktrace!(
                        "[SYS_WRITE] Ext2 file fd={} inode={} count={} position={}",
                        fd,
                        file_ref.inode,
                        count,
                        handle.position
                    );

                    // Check if ext2 is writable
                    if !crate::fs::ext2_is_writable() {
                        ktrace!("[SYS_WRITE] ERROR: ext2 filesystem is read-only");
                        posix::set_errno(posix::errno::EROFS);
                        return u64::MAX;
                    }

                    if !user_buffer_in_range(buf, count) {
                        ktrace!("[SYS_WRITE] ERROR: Buffer out of range");
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }

                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);

                    // Write to ext2 file at current position
                    match crate::fs::ext2_write_at(&file_ref, handle.position, data) {
                        Ok(bytes_written) => {
                            // Update position
                            update_file_handle_position(idx, handle.position + bytes_written);
                            ktrace!("[SYS_WRITE] Ext2 wrote {} bytes", bytes_written);
                            posix::set_errno(0);
                            return bytes_written as u64;
                        }
                        Err(e) => {
                            ktrace!("[SYS_WRITE] ERROR: Ext2 write failed: {:?}", e);
                            posix::set_errno(posix::errno::EIO);
                            return u64::MAX;
                        }
                    }
                }
                FileBacking::Inline(_) => {
                    // Inline files (from initramfs) are read-only
                    ktrace!("[SYS_WRITE] ERROR: Inline file is read-only");
                    posix::set_errno(posix::errno::EROFS);
                    return u64::MAX;
                }
                FileBacking::DevRandom | FileBacking::DevUrandom => {
                    // Writing to /dev/random adds entropy to the pool
                    if !user_buffer_in_range(buf, count) {
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }
                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);
                    crate::drivers::dev_random_write(data);
                    posix::set_errno(0);
                    return count;
                }
                FileBacking::DevNull => {
                    // /dev/null discards all writes
                    posix::set_errno(0);
                    return count;
                }
                FileBacking::DevZero => {
                    // /dev/zero discards all writes
                    posix::set_errno(0);
                    return count;
                }
                FileBacking::DevFull => {
                    // /dev/full always fails writes with ENOSPC
                    posix::set_errno(posix::errno::ENOSPC);
                    return u64::MAX;
                }
                FileBacking::DevLoop(index) => {
                    // Write to loop device (block device)
                    if !user_buffer_in_range(buf, count) {
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }
                    if !crate::drivers::loop_is_attached(index as usize) {
                        posix::set_errno(posix::errno::ENXIO);
                        return u64::MAX;
                    }
                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);
                    let position = handle.position as u64;
                    let sector = position / 512;
                    let sector_count = ((count + 511) / 512) as u32;
                    match crate::drivers::loop_write_sectors(
                        index as usize,
                        sector,
                        sector_count,
                        data,
                    ) {
                        Ok(written) => {
                            let new_pos = handle.position + written;
                            let mut updated = handle;
                            updated.position = new_pos;
                            set_file_handle(idx, Some(updated));
                            posix::set_errno(0);
                            return written as u64;
                        }
                        Err(e) => {
                            posix::set_errno(e);
                            return u64::MAX;
                        }
                    }
                }
                FileBacking::DevLoopControl => {
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
                FileBacking::DevInputEvent(_) | FileBacking::DevInputMice => {
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
                FileBacking::DevWatchdog => {
                    // Writing to watchdog feeds it (keepalive)
                    match crate::drivers::watchdog::feed() {
                        Ok(()) => {
                            posix::set_errno(0);
                            return count;
                        }
                        Err(e) => {
                            posix::set_errno(e);
                            return u64::MAX;
                        }
                    }
                }
            }
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

/// pwrite64 system call - write at a specified offset without changing file position
/// Unlike write(), this does not modify the file offset.
pub fn pwrite64(fd: u64, buf: u64, count: u64, offset: i64) -> u64 {
    ktrace!("[SYS_PWRITE64] fd={} count={} offset={}", fd, count, offset);

    if count == 0 {
        posix::set_errno(0);
        return 0;
    }

    if buf == 0 {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    if offset < 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // pwrite64 doesn't work on stdout/stderr (they don't support seeking)
    if fd == STDOUT || fd == STDERR {
        posix::set_errno(posix::errno::ESPIPE);
        return u64::MAX;
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
                FileBacking::StdStream(_) => {
                    // Streams don't support positioned I/O
                    posix::set_errno(posix::errno::ESPIPE);
                    return u64::MAX;
                }
                FileBacking::Socket(_) | FileBacking::Socketpair(_) => {
                    // Sockets don't support positioned I/O
                    posix::set_errno(posix::errno::ESPIPE);
                    return u64::MAX;
                }
                FileBacking::PtyMaster(_) | FileBacking::PtySlave(_) => {
                    // PTYs don't support positioned I/O
                    posix::set_errno(posix::errno::ESPIPE);
                    return u64::MAX;
                }
                FileBacking::Modular(ref file_handle) => {
                    if !crate::fs::modular_fs_is_writable(file_handle.fs_index) {
                        ktrace!("[SYS_PWRITE64] ERROR: modular filesystem is read-only");
                        posix::set_errno(posix::errno::EROFS);
                        return u64::MAX;
                    }

                    if !user_buffer_in_range(buf, count) {
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }

                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);

                    // Write at the specified offset (don't update file position)
                    match crate::fs::modular_fs_write_at(file_handle, offset as usize, data) {
                        Ok(bytes_written) => {
                            ktrace!(
                                "[SYS_PWRITE64] Modular fs wrote {} bytes at offset {}",
                                bytes_written,
                                offset
                            );
                            posix::set_errno(0);
                            return bytes_written as u64;
                        }
                        Err(e) => {
                            ktrace!("[SYS_PWRITE64] ERROR: Modular fs write failed: {:?}", e);
                            posix::set_errno(posix::errno::EIO);
                            return u64::MAX;
                        }
                    }
                }
                #[allow(deprecated)]
                FileBacking::Ext2(ref file_ref) => {
                    if !crate::fs::ext2_is_writable() {
                        ktrace!("[SYS_PWRITE64] ERROR: ext2 filesystem is read-only");
                        posix::set_errno(posix::errno::EROFS);
                        return u64::MAX;
                    }

                    if !user_buffer_in_range(buf, count) {
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }

                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);

                    // Write at the specified offset (don't update file position)
                    match crate::fs::ext2_write_at(file_ref, offset as usize, data) {
                        Ok(bytes_written) => {
                            ktrace!(
                                "[SYS_PWRITE64] Ext2 wrote {} bytes at offset {}",
                                bytes_written,
                                offset
                            );
                            posix::set_errno(0);
                            return bytes_written as u64;
                        }
                        Err(e) => {
                            ktrace!("[SYS_PWRITE64] ERROR: Ext2 write failed: {:?}", e);
                            posix::set_errno(posix::errno::EIO);
                            return u64::MAX;
                        }
                    }
                }
                FileBacking::Inline(_) => {
                    // Inline files (from initramfs) are read-only
                    posix::set_errno(posix::errno::EROFS);
                    return u64::MAX;
                }
                FileBacking::DevRandom | FileBacking::DevUrandom => {
                    // Writing to /dev/random adds entropy (offset is ignored)
                    if !user_buffer_in_range(buf, count) {
                        posix::set_errno(posix::errno::EFAULT);
                        return u64::MAX;
                    }
                    let data = core::slice::from_raw_parts(buf as *const u8, count as usize);
                    crate::drivers::dev_random_write(data);
                    posix::set_errno(0);
                    return count;
                }
                FileBacking::DevNull | FileBacking::DevZero => {
                    // Discards all writes
                    posix::set_errno(0);
                    return count;
                }
                FileBacking::DevFull => {
                    // /dev/full always fails writes with ENOSPC
                    posix::set_errno(posix::errno::ENOSPC);
                    return u64::MAX;
                }
                FileBacking::DevLoop(_) | FileBacking::DevLoopControl => {
                    // Loop devices don't support positioned I/O directly
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
                FileBacking::DevInputEvent(_) | FileBacking::DevInputMice => {
                    // Input devices don't support positioned writes
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
                FileBacking::DevWatchdog => {
                    // Watchdog doesn't support positioned writes
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
            }
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

/// pread64 system call - read at a specified offset without changing file position
/// Unlike read(), this does not modify the file offset.
pub fn pread64(fd: u64, buf: *mut u8, count: usize, offset: i64) -> u64 {
    ktrace!("[SYS_PREAD64] fd={} count={} offset={}", fd, count, offset);

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

    if offset < 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // pread64 doesn't work on stdin
    if fd == STDIN {
        posix::set_errno(posix::errno::ESPIPE);
        return u64::MAX;
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
                FileBacking::StdStream(_) => {
                    posix::set_errno(posix::errno::ESPIPE);
                    return u64::MAX;
                }
                FileBacking::Socket(_) | FileBacking::Socketpair(_) => {
                    posix::set_errno(posix::errno::ESPIPE);
                    return u64::MAX;
                }
                FileBacking::PtyMaster(_) | FileBacking::PtySlave(_) => {
                    posix::set_errno(posix::errno::ESPIPE);
                    return u64::MAX;
                }
                FileBacking::Modular(ref file_handle) => {
                    let buffer = core::slice::from_raw_parts_mut(buf, count);

                    // Read at the specified offset (don't update file position)
                    match crate::fs::modular_fs_read_at(file_handle, offset as usize, buffer) {
                        Ok(bytes_read) => {
                            ktrace!(
                                "[SYS_PREAD64] Modular fs read {} bytes from offset {}",
                                bytes_read,
                                offset
                            );
                            posix::set_errno(0);
                            return bytes_read as u64;
                        }
                        Err(_) => {
                            posix::set_errno(posix::errno::EIO);
                            return u64::MAX;
                        }
                    }
                }
                #[allow(deprecated)]
                FileBacking::Ext2(ref file_ref) => {
                    let buffer = core::slice::from_raw_parts_mut(buf, count);

                    // Read at the specified offset (don't update file position)
                    let bytes_read = crate::fs::ext2_read_at(file_ref, offset as usize, buffer);
                    ktrace!(
                        "[SYS_PREAD64] Ext2 read {} bytes from offset {}",
                        bytes_read,
                        offset
                    );
                    posix::set_errno(0);
                    return bytes_read as u64;
                }
                FileBacking::Inline(data) => {
                    let file_size = data.len();
                    if offset as usize >= file_size {
                        posix::set_errno(0);
                        return 0;
                    }
                    let available = file_size - offset as usize;
                    let to_read = cmp::min(count, available);
                    let buffer = core::slice::from_raw_parts_mut(buf, to_read);
                    buffer.copy_from_slice(&data[offset as usize..offset as usize + to_read]);
                    posix::set_errno(0);
                    return to_read as u64;
                }
                FileBacking::DevRandom | FileBacking::DevUrandom => {
                    let buffer = core::slice::from_raw_parts_mut(buf, count);
                    crate::drivers::dev_random_read(buffer);
                    posix::set_errno(0);
                    return count as u64;
                }
                FileBacking::DevNull => {
                    posix::set_errno(0);
                    return 0;
                }
                FileBacking::DevZero => {
                    let buffer = core::slice::from_raw_parts_mut(buf, count);
                    buffer.fill(0);
                    posix::set_errno(0);
                    return count as u64;
                }
                FileBacking::DevFull => {
                    let buffer = core::slice::from_raw_parts_mut(buf, count);
                    buffer.fill(0);
                    posix::set_errno(0);
                    return count as u64;
                }
                FileBacking::DevLoop(_) | FileBacking::DevLoopControl => {
                    // Loop devices don't support positioned I/O directly
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
                FileBacking::DevInputEvent(_) | FileBacking::DevInputMice => {
                    // Input devices don't support positioned reads
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
                FileBacking::DevWatchdog => {
                    // Watchdog doesn't support positioned reads
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
            }
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

/// writev system call - write data from multiple buffers (scatter-gather I/O)
pub fn writev(fd: u64, iov: *const IoVec, iovcnt: i32) -> u64 {
    ktrace!("[SYS_WRITEV] fd={} iovcnt={}", fd, iovcnt);

    if iovcnt <= 0 {
        if iovcnt == 0 {
            posix::set_errno(0);
            return 0;
        }
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if iovcnt as usize > UIO_MAXIOV {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if iov.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Validate the iovec array is accessible
    let iov_size = (iovcnt as usize) * core::mem::size_of::<IoVec>();
    if !user_buffer_in_range(iov as u64, iov_size as u64) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let iovecs = unsafe { core::slice::from_raw_parts(iov, iovcnt as usize) };

    let mut total_written: u64 = 0;

    for vec in iovecs {
        if vec.iov_len == 0 {
            continue;
        }

        // Each buffer must be valid
        if !user_buffer_in_range(vec.iov_base as u64, vec.iov_len as u64) {
            if total_written > 0 {
                posix::set_errno(0);
                return total_written;
            }
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }

        let bytes_written = write(fd, vec.iov_base as u64, vec.iov_len as u64);

        if bytes_written == u64::MAX {
            // Error occurred
            if total_written > 0 {
                // Return partial write
                posix::set_errno(0);
                return total_written;
            }
            // errno is already set by write()
            return u64::MAX;
        }

        total_written += bytes_written;

        // Short write - stop here
        if bytes_written < vec.iov_len as u64 {
            break;
        }
    }

    posix::set_errno(0);
    total_written
}

/// readv system call - read data into multiple buffers (scatter-gather I/O)
pub fn readv(fd: u64, iov: *const IoVec, iovcnt: i32) -> u64 {
    ktrace!("[SYS_READV] fd={} iovcnt={}", fd, iovcnt);

    if iovcnt <= 0 {
        if iovcnt == 0 {
            posix::set_errno(0);
            return 0;
        }
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if iovcnt as usize > UIO_MAXIOV {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if iov.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Validate the iovec array is accessible
    let iov_size = (iovcnt as usize) * core::mem::size_of::<IoVec>();
    if !user_buffer_in_range(iov as u64, iov_size as u64) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let iovecs = unsafe { core::slice::from_raw_parts(iov, iovcnt as usize) };

    let mut total_read: u64 = 0;

    for vec in iovecs {
        if vec.iov_len == 0 {
            continue;
        }

        // Each buffer must be valid
        if !user_buffer_in_range(vec.iov_base as u64, vec.iov_len as u64) {
            if total_read > 0 {
                posix::set_errno(0);
                return total_read;
            }
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }

        let bytes_read = read(fd, vec.iov_base, vec.iov_len);

        if bytes_read == u64::MAX {
            // Error occurred
            if total_read > 0 {
                // Return partial read
                posix::set_errno(0);
                return total_read;
            }
            // errno is already set by read()
            return u64::MAX;
        }

        if bytes_read == 0 {
            // EOF
            break;
        }

        total_read += bytes_read;

        // Short read - stop here
        if bytes_read < vec.iov_len as u64 {
            break;
        }
    }

    posix::set_errno(0);
    total_read
}

/// Read from keyboard input
pub fn read_from_keyboard(buf: *mut u8, count: usize) -> u64 {
    use x86_64::instructions::interrupts;

    if count == 0 {
        kinfo!("[read_from_keyboard] count=0, returning 0");
        return 0;
    }

    if !user_buffer_in_range(buf as u64, count as u64) {
        kinfo!("[read_from_keyboard] buf out of range, returning MAX");
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

    kinfo!("[read_from_keyboard] tty={}, read_len={}", tty, read_len);
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
                                    scheduler::do_schedule();
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
                FileBacking::PtyMaster(id) => {
                    let buffer = core::slice::from_raw_parts_mut(buf, count);

                    loop {
                        let pid = scheduler::current_pid();
                        match crate::tty::pty::try_read(
                            id as usize,
                            crate::tty::pty::PtyDirection::MasterReads,
                            buffer,
                            pid,
                        ) {
                            crate::tty::pty::PtyIoResult::Bytes(n) => {
                                posix::set_errno(0);
                                return n as u64;
                            }
                            crate::tty::pty::PtyIoResult::Eof => {
                                posix::set_errno(0);
                                return 0;
                            }
                            crate::tty::pty::PtyIoResult::WouldBlock => {
                                scheduler::set_current_process_state(ProcessState::Sleeping);
                                scheduler::do_schedule();
                                continue;
                            }
                        }
                    }
                }
                FileBacking::PtySlave(id) => {
                    let buffer = core::slice::from_raw_parts_mut(buf, count);

                    loop {
                        let pid = scheduler::current_pid();
                        match crate::tty::pty::try_read(
                            id as usize,
                            crate::tty::pty::PtyDirection::SlaveReads,
                            buffer,
                            pid,
                        ) {
                            crate::tty::pty::PtyIoResult::Bytes(n) => {
                                posix::set_errno(0);
                                return n as u64;
                            }
                            crate::tty::pty::PtyIoResult::Eof => {
                                posix::set_errno(0);
                                return 0;
                            }
                            crate::tty::pty::PtyIoResult::WouldBlock => {
                                scheduler::set_current_process_state(ProcessState::Sleeping);
                                scheduler::do_schedule();
                                continue;
                            }
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
                FileBacking::Modular(file_handle) => {
                    let total = handle.metadata.size as usize;
                    if handle.position >= total {
                        posix::set_errno(0);
                        return 0;
                    }
                    let remaining = total - handle.position;
                    let to_read = cmp::min(remaining, count);
                    let dest = slice::from_raw_parts_mut(buf, to_read);
                    let read =
                        match crate::fs::modular_fs_read_at(&file_handle, handle.position, dest) {
                            Ok(n) => n,
                            Err(_) => {
                                posix::set_errno(posix::errno::EIO);
                                return u64::MAX;
                            }
                        };
                    // Update position using accessor function
                    update_file_handle_position(idx, handle.position.saturating_add(read));
                    posix::set_errno(0);
                    return read as u64;
                }
                #[allow(deprecated)]
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
                FileBacking::DevRandom => {
                    // Blocking read from /dev/random
                    let dest = slice::from_raw_parts_mut(buf, count);
                    crate::drivers::dev_random_read(dest);
                    posix::set_errno(0);
                    return count as u64;
                }
                FileBacking::DevUrandom => {
                    // Non-blocking read from /dev/urandom
                    let dest = slice::from_raw_parts_mut(buf, count);
                    crate::drivers::dev_urandom_read(dest);
                    posix::set_errno(0);
                    return count as u64;
                }
                FileBacking::DevNull => {
                    // /dev/null always returns EOF
                    posix::set_errno(0);
                    return 0;
                }
                FileBacking::DevZero => {
                    // /dev/zero returns zero bytes
                    let dest = slice::from_raw_parts_mut(buf, count);
                    for byte in dest.iter_mut() {
                        *byte = 0;
                    }
                    posix::set_errno(0);
                    return count as u64;
                }
                FileBacking::DevFull => {
                    // /dev/full reads like /dev/zero
                    let dest = slice::from_raw_parts_mut(buf, count);
                    for byte in dest.iter_mut() {
                        *byte = 0;
                    }
                    posix::set_errno(0);
                    return count as u64;
                }
                FileBacking::DevLoop(index) => {
                    // Read from loop device
                    let dest = slice::from_raw_parts_mut(buf, count);
                    match crate::drivers::r#loop::read_sectors(
                        index as usize,
                        0,
                        (count / 512) as u32,
                        dest,
                    ) {
                        Ok(bytes) => {
                            posix::set_errno(0);
                            return bytes as u64;
                        }
                        Err(e) => {
                            posix::set_errno(e);
                            return u64::MAX;
                        }
                    }
                }
                FileBacking::DevLoopControl => {
                    // loop-control doesn't support read, only ioctl
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
                FileBacking::DevInputEvent(index) => {
                    // Read input events
                    // Each InputEvent is 24 bytes (struct input_event)
                    use crate::drivers::input::event::InputEvent;
                    let event_size = core::mem::size_of::<InputEvent>();
                    let max_events = count / event_size;
                    if max_events == 0 {
                        posix::set_errno(posix::errno::EINVAL);
                        return u64::MAX;
                    }
                    let events_ptr = buf as *mut InputEvent;
                    let events_buf = slice::from_raw_parts_mut(events_ptr, max_events);
                    let events_read =
                        crate::drivers::input::read_events(index as usize, events_buf);
                    if events_read == 0 {
                        // No events available - would block
                        posix::set_errno(posix::errno::EAGAIN);
                        return u64::MAX;
                    }
                    posix::set_errno(0);
                    return (events_read * event_size) as u64;
                }
                FileBacking::DevInputMice => {
                    // Read combined mouse data (ImPS/2 format)
                    let dest = slice::from_raw_parts_mut(buf, count);
                    let bytes = crate::drivers::input::mouse::read_imps2(dest);
                    if bytes == 0 {
                        // No data available - would block
                        posix::set_errno(posix::errno::EAGAIN);
                        return u64::MAX;
                    }
                    posix::set_errno(0);
                    return bytes as u64;
                }
                FileBacking::DevWatchdog => {
                    // Reading from watchdog returns nothing (use ioctl for info)
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
            }
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

// Import open flags from types.rs
use super::types::{O_CREAT, O_TRUNC};

/// Open system call
/// flags: O_RDONLY, O_WRONLY, O_RDWR, O_CREAT, O_TRUNC, O_APPEND
/// mode: file permission bits (used when O_CREAT)
pub fn open(path_ptr: *const u8, flags: u64, mode: u64) -> u64 {
    if path_ptr.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Read path as null-terminated C string
    let raw = unsafe { slice::from_raw_parts(path_ptr, 4096) }; // max path length
    let end = raw.iter().position(|&c| c == 0).unwrap_or(4096);
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
    let create_if_missing = (flags & O_CREAT) != 0;
    let truncate = (flags & O_TRUNC) != 0;

    ktrace!(
        "[open] path='{}', flags={:#o}, mode={:#o}, create={}, trunc={}",
        normalized,
        flags,
        mode,
        create_if_missing,
        truncate
    );

    // Linux-compatible stdio path aliases
    // - /dev/stdin|stdout|stderr are typically symlinks into /proc/self/fd/{0,1,2}
    // - Opening either form should return a dup() of the referenced FD
    match normalized {
        "/dev/stdin" => return super::fd::dup(STDIN),
        "/dev/stdout" => return super::fd::dup(STDOUT),
        "/dev/stderr" => return super::fd::dup(STDERR),
        _ => {}
    }

    if let Some(fd_str) = normalized
        .strip_prefix("/proc/self/fd/")
        .or_else(|| normalized.strip_prefix("proc/self/fd/"))
    {
        if let Ok(fd_num) = fd_str.parse::<u64>() {
            return super::fd::dup(fd_num);
        }
        posix::set_errno(posix::errno::ENOENT);
        return u64::MAX;
    }

    // /proc/<pid>/fd/<n> (Linux-like behavior: opening duplicates the referenced FD)
    if let Some(rest) = normalized
        .strip_prefix("/proc/")
        .or_else(|| normalized.strip_prefix("proc/"))
    {
        if let Some((pid_str, fd_str)) = rest.split_once("/fd/") {
            if let (Ok(pid), Ok(fd_num)) = (pid_str.parse::<u64>(), fd_str.parse::<u64>()) {
                if crate::fs::procfs::pid_exists(pid) && pid_has_fd(pid, fd_num) {
                    return super::fd::dup(fd_num);
                }
                posix::set_errno(posix::errno::ENOENT);
                return u64::MAX;
            }
        }
    }

    // Check for special device files
    let special_backing = match normalized {
        "/dev/random" => Some(FileBacking::DevRandom),
        "/dev/urandom" => Some(FileBacking::DevUrandom),
        "/dev/null" => Some(FileBacking::DevNull),
        "/dev/zero" => Some(FileBacking::DevZero),
        "/dev/full" => Some(FileBacking::DevFull),
        _ => None,
    };

    if let Some(backing) = special_backing {
        unsafe {
            if let Some(index) = find_empty_file_handle_slot() {
                let metadata = posix::Metadata {
                    size: 0,
                    file_type: FileType::Character,
                    mode: 0o666 | FileType::Character.mode_bits(),
                    uid: 0,
                    gid: 0,
                    mtime: 0,
                    nlink: 1,
                    blocks: 0,
                };
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
                mark_fd_open(fd);
                ktrace!("Opened device '{}' as fd {}", normalized, fd);
                return fd;
            }
        }
        posix::set_errno(posix::errno::EMFILE);
        kwarn!("No free file handles available");
        return u64::MAX;
    }

    // Loop devices: /dev/loop0-7
    if let Some(rest) = normalized.strip_prefix("/dev/loop") {
        if let Ok(index) = rest.parse::<u8>() {
            if index < crate::drivers::MAX_LOOP_DEVICES as u8 {
                unsafe {
                    if let Some(slot) = find_empty_file_handle_slot() {
                        let metadata = posix::Metadata {
                            size: 0,
                            file_type: FileType::Block,
                            mode: 0o660 | FileType::Block.mode_bits(),
                            uid: 0,
                            gid: 0,
                            mtime: 0,
                            nlink: 1,
                            blocks: 0,
                        };
                        set_file_handle(
                            slot,
                            Some(FileHandle {
                                backing: FileBacking::DevLoop(index),
                                position: 0,
                                metadata,
                            }),
                        );
                        posix::set_errno(0);
                        let fd = FD_BASE + slot as u64;
                        mark_fd_open(fd);
                        ktrace!("Opened /dev/loop{} as fd {}", index, fd);
                        return fd;
                    }
                }
                posix::set_errno(posix::errno::EMFILE);
                return u64::MAX;
            }
        }
    }

    // Loop control device: /dev/loop-control
    if normalized == "/dev/loop-control" {
        unsafe {
            if let Some(slot) = find_empty_file_handle_slot() {
                let metadata = posix::Metadata {
                    size: 0,
                    file_type: FileType::Character,
                    mode: 0o660 | FileType::Character.mode_bits(),
                    uid: 0,
                    gid: 0,
                    mtime: 0,
                    nlink: 1,
                    blocks: 0,
                };
                set_file_handle(
                    slot,
                    Some(FileHandle {
                        backing: FileBacking::DevLoopControl,
                        position: 0,
                        metadata,
                    }),
                );
                posix::set_errno(0);
                let fd = FD_BASE + slot as u64;
                mark_fd_open(fd);
                ktrace!("Opened /dev/loop-control as fd {}", fd);
                return fd;
            }
        }
        posix::set_errno(posix::errno::EMFILE);
        return u64::MAX;
    }

    // Input event devices: /dev/input/event0-7
    if let Some(rest) = normalized
        .strip_prefix("/dev/input/event")
        .or_else(|| normalized.strip_prefix("dev/input/event"))
    {
        if let Ok(index) = rest.parse::<u8>() {
            if crate::drivers::input_device_exists(index as usize) {
                unsafe {
                    if let Some(slot) = find_empty_file_handle_slot() {
                        let metadata = posix::Metadata {
                            size: 0,
                            file_type: FileType::Character,
                            mode: 0o666 | FileType::Character.mode_bits(),
                            uid: 0,
                            gid: 0,
                            mtime: 0,
                            nlink: 1,
                            blocks: 0,
                        };
                        set_file_handle(
                            slot,
                            Some(FileHandle {
                                backing: FileBacking::DevInputEvent(index),
                                position: 0,
                                metadata,
                            }),
                        );
                        posix::set_errno(0);
                        let fd = FD_BASE + slot as u64;
                        mark_fd_open(fd);
                        ktrace!("Opened /dev/input/event{} as fd {}", index, fd);
                        return fd;
                    }
                }
                posix::set_errno(posix::errno::EMFILE);
                return u64::MAX;
            }
        }
        posix::set_errno(posix::errno::ENOENT);
        return u64::MAX;
    }

    // Combined mice device: /dev/input/mice
    if normalized == "/dev/input/mice" {
        unsafe {
            if let Some(slot) = find_empty_file_handle_slot() {
                let metadata = posix::Metadata {
                    size: 0,
                    file_type: FileType::Character,
                    mode: 0o666 | FileType::Character.mode_bits(),
                    uid: 0,
                    gid: 0,
                    mtime: 0,
                    nlink: 1,
                    blocks: 0,
                };
                set_file_handle(
                    slot,
                    Some(FileHandle {
                        backing: FileBacking::DevInputMice,
                        position: 0,
                        metadata,
                    }),
                );
                posix::set_errno(0);
                let fd = FD_BASE + slot as u64;
                mark_fd_open(fd);
                ktrace!("Opened /dev/input/mice as fd {}", fd);
                return fd;
            }
        }
        posix::set_errno(posix::errno::EMFILE);
        return u64::MAX;
    }

    // Watchdog device: /dev/watchdog
    if normalized == "/dev/watchdog" {
        // Initialize watchdog if not already done
        if !crate::drivers::watchdog::is_initialized() {
            crate::drivers::watchdog::init();
        }
        unsafe {
            if let Some(slot) = find_empty_file_handle_slot() {
                let metadata = posix::Metadata {
                    size: 0,
                    file_type: FileType::Character,
                    mode: 0o600 | FileType::Character.mode_bits(),
                    uid: 0,
                    gid: 0,
                    mtime: 0,
                    nlink: 1,
                    blocks: 0,
                };
                set_file_handle(
                    slot,
                    Some(FileHandle {
                        backing: FileBacking::DevWatchdog,
                        position: 0,
                        metadata,
                    }),
                );
                // Enable watchdog when opened (standard Linux behavior)
                let _ = crate::drivers::watchdog::enable();
                posix::set_errno(0);
                let fd = FD_BASE + slot as u64;
                mark_fd_open(fd);
                ktrace!("Opened /dev/watchdog as fd {}", fd);
                return fd;
            }
        }
        posix::set_errno(posix::errno::EMFILE);
        return u64::MAX;
    }

    // PTY: /dev/ptmx allocates a new PTY master
    if normalized == "/dev/ptmx" {
        let Some(id) = crate::tty::pty::allocate_ptmx() else {
            posix::set_errno(posix::errno::ENOSPC);
            return u64::MAX;
        };

        unsafe {
            if let Some(index) = find_empty_file_handle_slot() {
                let metadata = posix::Metadata {
                    size: 0,
                    file_type: FileType::Character,
                    mode: 0o666 | FileType::Character.mode_bits(),
                    uid: 0,
                    gid: 0,
                    mtime: 0,
                    nlink: 1,
                    blocks: 0,
                };
                set_file_handle(
                    index,
                    Some(FileHandle {
                        backing: FileBacking::PtyMaster(id as u32),
                        position: 0,
                        metadata,
                    }),
                );
                posix::set_errno(0);
                let fd = FD_BASE + index as u64;
                mark_fd_open(fd);
                return fd;
            }
        }

        crate::tty::pty::close_master(id);
        posix::set_errno(posix::errno::EMFILE);
        return u64::MAX;
    }

    // PTY: /dev/pts/<n> opens the slave side (after unlock)
    if let Some(rest) = normalized
        .strip_prefix("/dev/pts/")
        .or_else(|| normalized.strip_prefix("dev/pts/"))
    {
        if let Ok(id) = rest.parse::<usize>() {
            if crate::tty::pty::open_slave(id).is_ok() {
                unsafe {
                    if let Some(index) = find_empty_file_handle_slot() {
                        let metadata = posix::Metadata {
                            size: 0,
                            file_type: FileType::Character,
                            mode: 0o666 | FileType::Character.mode_bits(),
                            uid: 0,
                            gid: 0,
                            mtime: 0,
                            nlink: 1,
                            blocks: 0,
                        };
                        set_file_handle(
                            index,
                            Some(FileHandle {
                                backing: FileBacking::PtySlave(id as u32),
                                position: 0,
                                metadata,
                            }),
                        );
                        posix::set_errno(0);
                        let fd = FD_BASE + index as u64;
                        mark_fd_open(fd);
                        return fd;
                    }
                }

                crate::tty::pty::close_slave(id);
                posix::set_errno(posix::errno::EMFILE);
                return u64::MAX;
            }

            posix::set_errno(posix::errno::EACCES);
            return u64::MAX;
        }

        posix::set_errno(posix::errno::ENOENT);
        return u64::MAX;
    }

    // Try to open existing file first
    if let Some(opened) = crate::fs::open(normalized) {
        if matches!(opened.metadata.file_type, FileType::Directory) {
            posix::set_errno(posix::errno::EISDIR);
            return u64::MAX;
        }

        let crate::fs::OpenFile { content, metadata } = opened;

        // Handle O_TRUNC for existing file
        if truncate {
            // Truncate the file to zero length
            if let Err(_) = crate::fs::write_file(normalized, &[]) {
                kwarn!("[open] Failed to truncate file '{}'", normalized);
                // Continue anyway, truncate is best-effort here
            }
        }

        let backing = match content {
            crate::fs::FileContent::Inline(data) => {
                if truncate {
                    // Use empty static slice for truncated file
                    FileBacking::Inline(&[])
                } else {
                    FileBacking::Inline(data)
                }
            }
            crate::fs::FileContent::Modular(handle) => FileBacking::Modular(handle),
            #[allow(deprecated)]
            crate::fs::FileContent::Ext2Modular(file_ref) => {
                // Convert legacy ext2 handle to modular handle
                FileBacking::Modular(crate::fs::ModularFileHandle {
                    fs_index: 0,
                    fs_handle: file_ref.fs.0,
                    inode: file_ref.inode,
                    size: file_ref.size,
                    mode: file_ref.mode,
                    blocks: file_ref.blocks,
                    mtime: file_ref.mtime,
                    nlink: file_ref.nlink,
                    uid: file_ref.uid,
                    gid: file_ref.gid,
                })
            }
        };

        unsafe {
            if let Some(index) = find_empty_file_handle_slot() {
                let final_metadata = if truncate {
                    posix::Metadata {
                        size: 0,
                        ..metadata
                    }
                } else {
                    metadata
                };
                set_file_handle(
                    index,
                    Some(FileHandle {
                        backing,
                        position: 0,
                        metadata: final_metadata,
                    }),
                );
                posix::set_errno(0);
                let fd = FD_BASE + index as u64;
                mark_fd_open(fd);
                ktrace!("Opened file '{}' as fd {}", normalized, fd);
                return fd;
            }
        }
        posix::set_errno(posix::errno::EMFILE);
        kwarn!("No free file handles available");
        return u64::MAX;
    }

    // File doesn't exist - try to create if O_CREAT is set
    if create_if_missing {
        ktrace!(
            "[open] File '{}' not found, creating with O_CREAT",
            normalized
        );

        // Try to create the file
        if let Err(e) = crate::fs::create_file(normalized) {
            kwarn!("[open] Failed to create file '{}': {}", normalized, e);
            posix::set_errno(posix::errno::EACCES);
            return u64::MAX;
        }

        ktrace!("[open] Created file '{}'", normalized);

        // Now open the newly created file
        if let Some(opened) = crate::fs::open(normalized) {
            let crate::fs::OpenFile { content, metadata } = opened;
            let backing = match content {
                crate::fs::FileContent::Inline(data) => FileBacking::Inline(data),
                crate::fs::FileContent::Modular(handle) => FileBacking::Modular(handle),
                #[allow(deprecated)]
                crate::fs::FileContent::Ext2Modular(file_ref) => {
                    // Convert legacy ext2 handle to modular handle
                    FileBacking::Modular(crate::fs::ModularFileHandle {
                        fs_index: 0,
                        fs_handle: file_ref.fs.0,
                        inode: file_ref.inode,
                        size: file_ref.size,
                        mode: file_ref.mode,
                        blocks: file_ref.blocks,
                        mtime: file_ref.mtime,
                        nlink: file_ref.nlink,
                        uid: file_ref.uid,
                        gid: file_ref.gid,
                    })
                }
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
                    mark_fd_open(fd);
                    ktrace!("Opened newly created file '{}' as fd {}", normalized, fd);
                    return fd;
                }
            }
            posix::set_errno(posix::errno::EMFILE);
            kwarn!("No free file handles available");
            return u64::MAX;
        } else {
            kwarn!("[open] Created file '{}' but failed to open it", normalized);
            posix::set_errno(posix::errno::EIO);
            return u64::MAX;
        }
    }

    // File doesn't exist and O_CREAT not set
    posix::set_errno(posix::errno::ENOENT);
    kwarn!("sys_open: file '{}' not found", normalized);
    u64::MAX
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
                // Close UDP socket
                else if sock_handle.socket_type == SOCK_DGRAM
                    && sock_handle.socket_index != usize::MAX
                {
                    if let Some(_) = crate::net::with_net_stack(|stack| {
                        stack.udp_close(sock_handle.socket_index)
                    }) {
                        kinfo!(
                            "Closed UDP socket {} for fd {}",
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
            // Clean up PTY resources
            else if let FileBacking::PtyMaster(id) = handle.backing {
                crate::tty::pty::close_master(id as usize);
            } else if let FileBacking::PtySlave(id) = handle.backing {
                crate::tty::pty::close_slave(id as usize);
            }

            clear_file_handle(idx);
            mark_fd_closed(fd); // Track this FD as closed for the current process
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
    crate::serial_println!(
        "SYSCALL_LIST_FILES: buf={:#x} count={} req={:#x}",
        buf as u64,
        count,
        request_ptr as u64
    );

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

    crate::serial_println!(
        "SYSCALL_LIST_FILES: normalized='{}' (is_root={})",
        normalized,
        normalized == "/"
    );

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

    crate::serial_println!(
        "SYSCALL_LIST_FILES: calling list_directory path='{}'",
        normalized
    );

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

fn build_fd_link_target(fd: u64) -> Option<String> {
    if fd <= 2 {
        return Some(String::from("/dev/tty"));
    }

    let handle = handle_for_fd(fd).ok()?;

    let target = match handle.backing {
        FileBacking::StdStream(_) => String::from("/dev/tty"),
        FileBacking::DevNull => String::from("/dev/null"),
        FileBacking::DevZero => String::from("/dev/zero"),
        FileBacking::DevFull => String::from("/dev/full"),
        FileBacking::DevRandom => String::from("/dev/random"),
        FileBacking::DevUrandom => String::from("/dev/urandom"),
        FileBacking::PtyMaster(_) => String::from("/dev/ptmx"),
        FileBacking::PtySlave(id) => alloc::format!("/dev/pts/{}", id),
        FileBacking::Socket(sock) => alloc::format!("socket:[{}]", sock.socket_index),
        FileBacking::Socketpair(pair) => {
            alloc::format!("socketpair:[{}]:{}", pair.pair_id, pair.end)
        }
        FileBacking::Inline(_) => String::from("initramfs"),
        FileBacking::Modular(m) => alloc::format!("modfs:{}:inode:{}", m.fs_index, m.inode),
        #[allow(deprecated)]
        FileBacking::Ext2(_) => String::from("ext2"),
        FileBacking::DevLoop(n) => alloc::format!("/dev/loop{}", n),
        FileBacking::DevLoopControl => String::from("/dev/loop-control"),
        FileBacking::DevInputEvent(n) => alloc::format!("/dev/input/event{}", n),
        FileBacking::DevInputMice => String::from("/dev/input/mice"),
        FileBacking::DevWatchdog => String::from("/dev/watchdog"),
    };

    Some(target)
}

fn parse_proc_pid_fd(path: &str) -> Option<(u64, u64)> {
    let path = path.trim();
    // Accept both absolute and relative proc-style paths
    let path = path
        .strip_prefix("/proc/")
        .or_else(|| path.strip_prefix("proc/"))?;

    if let Some(fd_str) = path.strip_prefix("self/fd/") {
        let pid = scheduler::get_current_pid().unwrap_or(1);
        let fd = fd_str.parse::<u64>().ok()?;
        return Some((pid, fd));
    }

    let (pid_str, fd_str) = path.split_once("/fd/")?;
    let pid = pid_str.parse::<u64>().ok()?;
    let fd = fd_str.parse::<u64>().ok()?;
    Some((pid, fd))
}

fn pid_has_fd(pid: u64, fd: u64) -> bool {
    if fd <= 2 {
        return true;
    }
    if fd < FD_BASE {
        return false;
    }
    let bit = (fd - FD_BASE) as usize;
    if bit >= MAX_OPEN_FILES {
        return false;
    }
    scheduler::get_process(pid)
        .map(|p| (p.open_fds & (1 << bit)) != 0)
        .unwrap_or(false)
}

fn readlink_impl(path: &str) -> Option<String> {
    match path.trim() {
        "/dev/stdin" => return Some(String::from("/proc/self/fd/0")),
        "/dev/stdout" => return Some(String::from("/proc/self/fd/1")),
        "/dev/stderr" => return Some(String::from("/proc/self/fd/2")),
        _ => {}
    }

    if path.trim() == "/proc/self" || path.trim() == "proc/self" {
        let pid = scheduler::get_current_pid().unwrap_or(1);
        return Some(alloc::format!("{}", pid));
    }

    let (pid, fd) = parse_proc_pid_fd(path)?;
    if !crate::fs::procfs::pid_exists(pid) {
        return None;
    }
    if !pid_has_fd(pid, fd) {
        return None;
    }
    build_fd_link_target(fd)
}

/// readlink(2) - read the target of a symbolic link
pub fn readlink(path_ptr: *const u8, buf_ptr: *mut u8, bufsiz: usize) -> u64 {
    if path_ptr.is_null() || buf_ptr.is_null() || bufsiz == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if !user_buffer_in_range(buf_ptr as u64, bufsiz as u64) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Read path as null-terminated C string
    let raw = unsafe { slice::from_raw_parts(path_ptr, 4096) };
    let end = raw.iter().position(|&c| c == 0).unwrap_or(4096);
    let trimmed = &raw[..end];
    let Ok(path) = str::from_utf8(trimmed) else {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    };

    let Some(target) = readlink_impl(path) else {
        posix::set_errno(posix::errno::ENOENT);
        return u64::MAX;
    };

    let bytes = target.as_bytes();
    let n = bytes.len().min(bufsiz);
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr, n);
    }
    posix::set_errno(0);
    n as u64
}

/// readlinkat(2) - minimal support (absolute paths only, or AT_FDCWD)
pub fn readlinkat(dirfd: i32, pathname_ptr: *const u8, buf_ptr: *mut u8, bufsiz: usize) -> u64 {
    const AT_FDCWD: i32 = -100;

    if pathname_ptr.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Read pathname as C string
    let raw = unsafe { slice::from_raw_parts(pathname_ptr, 4096) };
    let end = raw.iter().position(|&c| c == 0).unwrap_or(4096);
    let trimmed = &raw[..end];
    let Ok(path) = str::from_utf8(trimmed) else {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    };

    // We only support absolute paths for now (or AT_FDCWD with an absolute path)
    if path.trim().starts_with('/') {
        return readlink(pathname_ptr, buf_ptr, bufsiz);
    }

    if dirfd == AT_FDCWD {
        posix::set_errno(posix::errno::ENOSYS);
        return u64::MAX;
    }

    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
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
                Ok(new_fd) => {
                    mark_fd_open(new_fd); // Track the new FD as open
                    posix::set_errno(0);
                    new_fd
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

/// Close all file descriptors for a process based on its open_fds bitmask.
/// This is called when a process exits to clean up its resources.
///
/// # Safety
/// This function modifies global FILE_HANDLES state and must be called
/// when it's safe to do so (e.g., during process cleanup).
pub fn close_all_fds_for_process(open_fds: u16) {
    use crate::kinfo;

    if open_fds == 0 {
        return; // No open file descriptors
    }

    kinfo!(
        "Closing all FDs for process, open_fds bitmap: {:#06x}",
        open_fds
    );

    for bit in 0..MAX_OPEN_FILES {
        if (open_fds & (1 << bit)) != 0 {
            let fd = FD_BASE + bit as u64;
            kinfo!("Auto-closing fd {} (bit {})", fd, bit);

            // Perform close without checking current process ownership
            // since this is called during process cleanup
            unsafe {
                if let Some(handle) = get_file_handle(bit) {
                    // Clean up socket resources if this is a socket
                    if let FileBacking::Socket(ref sock_handle) = handle.backing {
                        // Close netlink socket in network stack
                        if sock_handle.domain == AF_NETLINK
                            && sock_handle.socket_index != usize::MAX
                        {
                            if let Some(_) = crate::net::with_net_stack(|stack| {
                                stack.netlink_close(sock_handle.socket_index)
                            }) {
                                kinfo!(
                                    "Auto-closed netlink socket {} for fd {}",
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
                                    "Auto-closed TCP socket {} for fd {}",
                                    sock_handle.socket_index,
                                    fd
                                );
                            }
                        }
                        // Close UDP socket
                        else if sock_handle.socket_type == SOCK_DGRAM
                            && sock_handle.socket_index != usize::MAX
                        {
                            if let Some(_) = crate::net::with_net_stack(|stack| {
                                stack.udp_close(sock_handle.socket_index)
                            }) {
                                kinfo!(
                                    "Auto-closed UDP socket {} for fd {}",
                                    sock_handle.socket_index,
                                    fd
                                );
                            }
                        }
                    }
                    // Clean up socketpair resources
                    else if let FileBacking::Socketpair(ref sp_handle) = handle.backing {
                        let _ = crate::ipc::close_socketpair_end(sp_handle.pair_id, sp_handle.end);
                        kinfo!(
                            "Auto-closed socketpair {}.{} for fd {}",
                            sp_handle.pair_id,
                            sp_handle.end,
                            fd
                        );
                    }

                    clear_file_handle(bit);
                    kinfo!("Auto-closed fd {} during process cleanup", fd);
                }
            }
        }
    }
}

// ============================================================================
// Internal API for kernel subsystems (loop devices, etc.)
// ============================================================================

/// Get file size for a given file descriptor (internal API)
///
/// Returns the size in bytes, or None if the fd is invalid or doesn't have a size.
pub fn get_file_size(fd: u64) -> Option<u64> {
    if fd < FD_BASE {
        return None;
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        return None;
    }

    unsafe { get_file_handle(idx).map(|handle| handle.metadata.size) }
}

/// Get file path for a given file descriptor (internal API)
///
/// Returns the path if available, or a generated name otherwise.
pub fn get_file_path(fd: u64) -> Option<alloc::string::String> {
    use alloc::format;
    use alloc::string::String;

    if fd < FD_BASE {
        return match fd {
            0 => Some(String::from("/dev/stdin")),
            1 => Some(String::from("/dev/stdout")),
            2 => Some(String::from("/dev/stderr")),
            _ => None,
        };
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        return None;
    }

    unsafe {
        get_file_handle(idx).map(|handle| match handle.backing {
            FileBacking::StdStream(_) => String::from("/dev/tty"),
            FileBacking::DevNull => String::from("/dev/null"),
            FileBacking::DevZero => String::from("/dev/zero"),
            FileBacking::DevFull => String::from("/dev/full"),
            FileBacking::DevRandom => String::from("/dev/random"),
            FileBacking::DevUrandom => String::from("/dev/urandom"),
            FileBacking::PtyMaster(_) => String::from("/dev/ptmx"),
            FileBacking::PtySlave(id) => format!("/dev/pts/{}", id),
            FileBacking::Socket(sock) => format!("socket:[{}]", sock.socket_index),
            FileBacking::Socketpair(pair) => format!("socketpair:[{}]:{}", pair.pair_id, pair.end),
            FileBacking::Inline(_) => String::from("initramfs"),
            FileBacking::Modular(m) => format!("modfs:{}:inode:{}", m.fs_index, m.inode),
            #[allow(deprecated)]
            FileBacking::Ext2(_) => String::from("ext2"),
            FileBacking::DevLoop(n) => format!("/dev/loop{}", n),
            FileBacking::DevLoopControl => String::from("/dev/loop-control"),
            FileBacking::DevInputEvent(n) => format!("/dev/input/event{}", n),
            FileBacking::DevInputMice => String::from("/dev/input/mice"),
            FileBacking::DevWatchdog => String::from("/dev/watchdog"),
        })
    }
}

/// Read from a file descriptor at a specific offset (internal API)
///
/// This is similar to pread64 but for kernel-internal use.
pub fn pread_internal(fd: u64, buf: &mut [u8], offset: i64) -> Result<usize, i32> {
    use crate::posix;

    if buf.is_empty() {
        return Ok(0);
    }

    if offset < 0 {
        return Err(posix::errno::EINVAL);
    }

    if fd < FD_BASE {
        return Err(posix::errno::ESPIPE);
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        return Err(posix::errno::EBADF);
    }

    unsafe {
        if let Some(handle) = get_file_handle(idx) {
            match handle.backing {
                FileBacking::Modular(ref file_handle) => {
                    match crate::fs::modular_fs_read_at(file_handle, offset as usize, buf) {
                        Ok(bytes_read) => Ok(bytes_read),
                        Err(_) => Err(posix::errno::EIO),
                    }
                }
                #[allow(deprecated)]
                FileBacking::Ext2(ref file_ref) => {
                    let bytes_read = crate::fs::ext2_read_at(file_ref, offset as usize, buf);
                    Ok(bytes_read)
                }
                FileBacking::Inline(data) => {
                    let file_size = data.len();
                    if offset as usize >= file_size {
                        return Ok(0);
                    }
                    let available = file_size - offset as usize;
                    let to_read = core::cmp::min(buf.len(), available);
                    buf[..to_read]
                        .copy_from_slice(&data[offset as usize..offset as usize + to_read]);
                    Ok(to_read)
                }
                FileBacking::DevRandom | FileBacking::DevUrandom => {
                    crate::drivers::dev_random_read(buf);
                    Ok(buf.len())
                }
                FileBacking::DevNull => Ok(0),
                FileBacking::DevZero | FileBacking::DevFull => {
                    buf.fill(0);
                    Ok(buf.len())
                }
                FileBacking::StdStream(_)
                | FileBacking::Socket(_)
                | FileBacking::Socketpair(_)
                | FileBacking::PtyMaster(_)
                | FileBacking::PtySlave(_) => Err(posix::errno::ESPIPE),
                FileBacking::DevLoop(_)
                | FileBacking::DevLoopControl
                | FileBacking::DevInputEvent(_)
                | FileBacking::DevInputMice
                | FileBacking::DevWatchdog => Err(posix::errno::EINVAL),
            }
        } else {
            Err(posix::errno::EBADF)
        }
    }
}

/// Write to a file descriptor at a specific offset (internal API)
///
/// This is similar to pwrite64 but for kernel-internal use.
pub fn pwrite_internal(fd: u64, buf: &[u8], offset: i64) -> Result<usize, i32> {
    use crate::posix;

    if buf.is_empty() {
        return Ok(0);
    }

    if offset < 0 {
        return Err(posix::errno::EINVAL);
    }

    if fd < FD_BASE {
        return Err(posix::errno::ESPIPE);
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        return Err(posix::errno::EBADF);
    }

    unsafe {
        if let Some(handle) = get_file_handle(idx) {
            match handle.backing {
                FileBacking::Modular(ref file_handle) => {
                    if !crate::fs::modular_fs_is_writable(file_handle.fs_index) {
                        return Err(posix::errno::EROFS);
                    }
                    match crate::fs::modular_fs_write_at(file_handle, offset as usize, buf) {
                        Ok(bytes_written) => Ok(bytes_written),
                        Err(_) => Err(posix::errno::EIO),
                    }
                }
                #[allow(deprecated)]
                FileBacking::Ext2(ref file_ref) => {
                    if !crate::fs::ext2_is_writable() {
                        return Err(posix::errno::EROFS);
                    }
                    match crate::fs::ext2_write_at(file_ref, offset as usize, buf) {
                        Ok(bytes_written) => Ok(bytes_written),
                        Err(_) => Err(posix::errno::EIO),
                    }
                }
                FileBacking::Inline(_) => Err(posix::errno::EROFS),
                FileBacking::DevRandom | FileBacking::DevUrandom => {
                    crate::drivers::dev_random_write(buf);
                    Ok(buf.len())
                }
                FileBacking::DevNull | FileBacking::DevZero => Ok(buf.len()),
                FileBacking::DevFull => Err(posix::errno::ENOSPC),
                FileBacking::StdStream(_)
                | FileBacking::Socket(_)
                | FileBacking::Socketpair(_)
                | FileBacking::PtyMaster(_)
                | FileBacking::PtySlave(_) => Err(posix::errno::ESPIPE),
                FileBacking::DevLoop(_)
                | FileBacking::DevLoopControl
                | FileBacking::DevInputEvent(_)
                | FileBacking::DevInputMice
                | FileBacking::DevWatchdog => Err(posix::errno::EINVAL),
            }
        } else {
            Err(posix::errno::EBADF)
        }
    }
}
