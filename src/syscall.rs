use crate::posix::{self, FileType};
use core::{
    arch::global_asm,
    cmp,
    fmt::{self, Write},
    ptr, slice, str,
};
use x86_64::instructions::interrupts;

/// System call numbers (POSIX-compliant where possible)
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_STAT: u64 = 4;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_PIPE: u64 = 22;
pub const SYS_DUP: u64 = 32;
pub const SYS_DUP2: u64 = 33;
pub const SYS_GETPID: u64 = 39;
pub const SYS_FORK: u64 = 57;
pub const SYS_EXECVE: u64 = 59;
pub const SYS_EXIT: u64 = 60;
pub const SYS_WAIT4: u64 = 61;
pub const SYS_KILL: u64 = 62;
pub const SYS_SIGACTION: u64 = 13;
pub const SYS_SIGPROCMASK: u64 = 14;
pub const SYS_GETPPID: u64 = 110;
pub const SYS_SCHED_YIELD: u64 = 24;
pub const SYS_LIST_FILES: u64 = 200;
pub const SYS_GETERRNO: u64 = 201;
pub const SYS_IPC_CREATE: u64 = 210;
pub const SYS_IPC_SEND: u64 = 211;
pub const SYS_IPC_RECV: u64 = 212;
pub const SYS_USER_ADD: u64 = 220;
pub const SYS_USER_LOGIN: u64 = 221;
pub const SYS_USER_INFO: u64 = 222;
pub const SYS_USER_LIST: u64 = 223;
pub const SYS_USER_LOGOUT: u64 = 224;

// Init system calls
pub const SYS_REBOOT: u64 = 169;        // sys_reboot (Linux)
pub const SYS_SHUTDOWN: u64 = 230;      // Custom: system shutdown
pub const SYS_RUNLEVEL: u64 = 231;      // Custom: get/set runlevel

const STDIN: u64 = 0;
const STDOUT: u64 = 1;
const STDERR: u64 = 2;

const FD_BASE: u64 = 3;
const MAX_OPEN_FILES: usize = 16;
const LIST_FLAG_INCLUDE_HIDDEN: u64 = 0x1;
const USER_FLAG_ADMIN: u64 = 0x1;

#[repr(C)]
struct ListDirRequest {
    path_ptr: u64,
    path_len: u64,
    flags: u64,
}

#[repr(C)]
struct UserRequest {
    username_ptr: u64,
    username_len: u64,
    password_ptr: u64,
    password_len: u64,
    flags: u64,
}

#[repr(C)]
struct UserInfoReply {
    username: [u8; 32],
    username_len: u64,
    uid: u32,
    gid: u32,
    is_admin: u32,
}

#[repr(C)]
struct IpcTransferRequest {
    channel_id: u32,
    flags: u32,
    buffer_ptr: u64,
    buffer_len: u64,
}

#[derive(Clone, Copy)]
enum FileBacking {
    Inline(&'static [u8]),
    Ext2(crate::fs::ext2::FileRef),
}

#[derive(Clone, Copy)]
struct FileHandle {
    backing: FileBacking,
    position: usize,
    metadata: crate::posix::Metadata,
}

static mut FILE_HANDLES: [Option<FileHandle>; MAX_OPEN_FILES] = [None; MAX_OPEN_FILES];

/// Write system call
fn syscall_write(fd: u64, buf: u64, count: u64) -> u64 {
    if count == 0 {
        posix::set_errno(0);
        return 0;
    }

    if buf == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if fd == STDOUT || fd == STDERR {
        let slice = unsafe { slice::from_raw_parts(buf as *const u8, count as usize) };

        crate::serial::write_bytes(slice);

        let utf8_view = str::from_utf8(slice);

        crate::vga_buffer::with_writer(|writer| {
            use core::fmt::Write;

            match utf8_view {
                Ok(text) => {
                    writer.write_str(text).ok();
                }
                Err(_) => {
                    for &byte in slice {
                        let ch = match byte {
                            b'\r' => '\r',
                            b'\n' => '\n',
                            b'\t' => '\t',
                            0x20..=0x7E => byte as char,
                            _ => '?',
                        };
                        writer.write_char(ch).ok();
                    }
                }
            }
        });

        match utf8_view {
            Ok(text) => crate::framebuffer::write_str(text),
            Err(_) => crate::framebuffer::write_bytes(slice),
        }

        posix::set_errno(0);
        count
    } else {
        posix::set_errno(posix::errno::EBADF);
        u64::MAX
    }
}

/// Read system call
fn syscall_read(fd: u64, buf: *mut u8, count: usize) -> u64 {
    crate::kinfo!("sys_read(fd={}, count={})", fd, count);
    if buf.is_null() {
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
        if let Some(handle) = FILE_HANDLES[idx].as_mut() {
            match handle.backing {
                FileBacking::Inline(data) => {
                    let remaining = data.len().saturating_sub(handle.position);
                    if remaining == 0 {
                        posix::set_errno(0);
                        return 0;
                    }
                    let to_copy = cmp::min(remaining, count);
                    ptr::copy_nonoverlapping(data.as_ptr().add(handle.position), buf, to_copy);
                    handle.position += to_copy;
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
                    handle.position = handle.position.saturating_add(read);
                    posix::set_errno(0);
                    return read as u64;
                }
            }
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

/// Exit system call
fn syscall_exit(code: i32) -> u64 {
    let pid = crate::scheduler::current_pid().unwrap_or(0);
    crate::kinfo!("Process {} exited with code: {}", pid, code);
    
    // Notify init system about process exit
    crate::init::handle_process_exit(pid, code);
    
    // Mark process as zombie and remove from scheduler
    if pid != 0 {
        let _ = crate::scheduler::set_process_state(pid, crate::process::ProcessState::Zombie);
        let _ = crate::scheduler::remove_process(pid);
    }
    
    // Schedule next process
    if let Some(next_pid) = crate::scheduler::schedule() {
        crate::kinfo!("Switching to next process: {}", next_pid);
        // TODO: Implement context switch to next process
    } else {
        crate::kinfo!("No more processes to run, halting system");
        crate::arch::halt_loop();
    }
    
    // Should never reach here
    0
}
fn syscall_open(path_ptr: *const u8, len: usize) -> u64 {
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
            crate::fs::FileContent::Ext2(file_ref) => FileBacking::Ext2(file_ref),
        };

        unsafe {
            for index in 0..MAX_OPEN_FILES {
                if FILE_HANDLES[index].is_none() {
                    FILE_HANDLES[index] = Some(FileHandle {
                        backing,
                        position: 0,
                        metadata,
                    });
                    posix::set_errno(0);
                    let fd = FD_BASE + index as u64;
                    crate::kinfo!("Opened file '{}' as fd {}", normalized, fd);
                    return fd;
                }
            }
        }
        posix::set_errno(posix::errno::EMFILE);
        crate::kwarn!("No free file handles available");
        u64::MAX
    } else {
        posix::set_errno(posix::errno::ENOENT);
        crate::kwarn!("sys_open: file '{}' not found", normalized);
        u64::MAX
    }
}

fn syscall_close(fd: u64) -> u64 {
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
        if FILE_HANDLES[idx].is_some() {
            FILE_HANDLES[idx] = None;
            crate::kinfo!("Closed fd {}", fd);
            posix::set_errno(0);
            return 0;
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

fn syscall_list_files(buf: *mut u8, count: usize, request_ptr: *const ListDirRequest) -> u64 {
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

    if normalized != "/" {
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
    }

    let mut written = 0usize;
    let mut overflow = false;

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

    if overflow {
        posix::set_errno(posix::errno::EAGAIN);
    } else {
        posix::set_errno(0);
    }
    written as u64
}

fn syscall_stat(path_ptr: *const u8, len: usize, stat_buf: *mut posix::Stat) -> u64 {
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

fn syscall_fstat(fd: u64, stat_buf: *mut posix::Stat) -> u64 {
    if stat_buf.is_null() {
        posix::set_errno(posix::errno::EINVAL);
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
        if let Some(handle) = FILE_HANDLES[idx] {
            let stat = posix::Stat::from_metadata(&handle.metadata);
            ptr::write(stat_buf, stat);
            posix::set_errno(0);
            return 0;
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

fn syscall_lseek(fd: u64, offset: i64, whence: u64) -> u64 {
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
        if let Some(handle) = FILE_HANDLES[idx].as_mut() {
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
            handle.position = limited as usize;
            posix::set_errno(0);
            return new_pos_u64;
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

fn syscall_get_errno() -> u64 {
    posix::errno() as u64
}

fn map_auth_error(err: crate::auth::AuthError) -> i32 {
    match err {
        crate::auth::AuthError::InvalidInput => posix::errno::EINVAL,
        crate::auth::AuthError::AlreadyExists => posix::errno::EEXIST,
        crate::auth::AuthError::TableFull => posix::errno::ENOSPC,
        crate::auth::AuthError::InvalidCredentials => posix::errno::EPERM,
        crate::auth::AuthError::AccessDenied => posix::errno::EACCES,
    }
}

struct BufferWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
    overflow: bool,
}

impl<'a> BufferWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            len: 0,
            overflow: false,
        }
    }

    fn written(&self) -> usize {
        self.len
    }

    fn overflowed(&self) -> bool {
        self.overflow
    }
}

impl fmt::Write for BufferWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.overflow {
            return Err(fmt::Error);
        }

        let bytes = s.as_bytes();
        if self.len + bytes.len() > self.buf.len() {
            self.overflow = true;
            return Err(fmt::Error);
        }

        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }
}

fn syscall_user_add(request_ptr: *const UserRequest) -> u64 {
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

fn syscall_user_login(request_ptr: *const UserRequest) -> u64 {
    if request_ptr.is_null() {
        posix::set_errno(posix::errno::EINVAL);
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

fn syscall_user_info(info_ptr: *mut UserInfoReply) -> u64 {
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

fn syscall_user_list(buf_ptr: *mut u8, count: usize) -> u64 {
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

fn syscall_user_logout() -> u64 {
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

fn map_ipc_error(err: crate::ipc::IpcError) -> i32 {
    match err {
        crate::ipc::IpcError::NoSuchChannel => posix::errno::ENOENT,
        crate::ipc::IpcError::TableFull => posix::errno::ENOSPC,
        crate::ipc::IpcError::WouldBlock | crate::ipc::IpcError::Empty => posix::errno::EAGAIN,
        crate::ipc::IpcError::InvalidInput => posix::errno::EINVAL,
    }
}

fn syscall_ipc_create() -> u64 {
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

fn syscall_ipc_send(request_ptr: *const IpcTransferRequest) -> u64 {
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

fn syscall_ipc_recv(request_ptr: *const IpcTransferRequest) -> u64 {
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

fn read_from_keyboard(buf: *mut u8, count: usize) -> u64 {
    if count == 0 {
        return 0;
    }

    // Allow the keyboard interrupt handler to run while we wait for input.
    // The INT 0x81 gate enters with IF=0, so without re-enabling here the
    // HLT inside `keyboard::read_raw` would never resume. Preserve the
    // previous interrupt state so nested callers remain well-behaved.
    let were_enabled = interrupts::are_enabled();
    if !were_enabled {
        interrupts::enable();
    }

    // Read directly into userspace buffer (no echo, userspace handles that)
    let slice = unsafe { core::slice::from_raw_parts_mut(buf, count) };
    let read_len = crate::keyboard::read_raw(slice, count);

    if !were_enabled {
        interrupts::disable();
    }

    posix::set_errno(0);
    read_len as u64
}

/// POSIX pipe() system call - creates a pipe
fn syscall_pipe(pipefd: *mut [i32; 2]) -> u64 {
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

/// POSIX kill() system call - send signal to process
fn syscall_kill(pid: u64, signum: u64) -> u64 {
    if signum >= crate::signal::NSIG as u64 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // For now, just log the signal send
    crate::kinfo!("kill(pid={}, sig={}) called", pid, signum);
    
    // TODO: Implement actual signal delivery to target process
    posix::set_errno(0);
    0
}

/// POSIX getppid() system call - get parent process ID
fn syscall_getppid() -> u64 {
    // For now, return 0 (no parent)
    // TODO: Implement proper parent PID tracking
    posix::set_errno(0);
    0
}

/// POSIX fork() system call - create child process
fn syscall_fork() -> u64 {
    // Fork is complex and requires full process management
    // For now, return error indicating not implemented
    crate::kwarn!("fork() system call not yet fully implemented");
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

/// POSIX execve() system call - execute program
fn syscall_execve(path: *const u8, _argv: *const u64, _envp: *const u64) -> u64 {
    if path.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Read the path string from user space
    let path_str = unsafe {
        let mut len = 0;
        while len < 256 && *path.add(len) != 0 {
            len += 1;
        }
        core::slice::from_raw_parts(path, len)
    };

    let path_str = match core::str::from_utf8(path_str) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!("execve: loading program '{}'", path_str);

    // Try to load the ELF file from filesystem
    let elf_data = match crate::fs::read_file_bytes(path_str) {
        Some(data) => data,
        None => {
            crate::kerror!("execve: file not found: {}", path_str);
            posix::set_errno(posix::errno::ENOENT);
            return u64::MAX;
        }
    };

    // Create a new process from the ELF data
    let new_process = match crate::process::Process::from_elf(elf_data) {
        Ok(proc) => proc,
        Err(e) => {
            crate::kerror!("execve: failed to load ELF: {:?}", e);
            posix::set_errno(posix::errno::EINVAL); // Use EINVAL instead of ENOEXEC
            return u64::MAX;
        }
    };

    let entry = new_process.entry_point;
    let stack = new_process.stack_top;
    
    crate::kinfo!("execve: ELF loaded, entry={:#x}, stack={:#x}", entry, stack);

    // Replace the current process with the new one
    // This is a simplified implementation - in a full OS, we would:
    // 1. Free current process memory
    // 2. Update process table
    // 3. Set up new memory mappings
    // 4. Return to user mode with new entry point
    
    // For now, we just switch to the new process
    // We don't actually "return" from this syscall - we jump to the new program
    crate::kinfo!("execve: switching to new process");
    
    unsafe {
        // Update GS_DATA with new values
        let gs_data_addr = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
        let gs_data_ptr = gs_data_addr as *mut u64;
        
        gs_data_ptr.add(0).write(stack); // User RSP
        gs_data_ptr.add(2).write(entry); // User entry point
        gs_data_ptr.add(3).write(stack); // User stack base
        
        crate::kinfo!("execve: jumping to entry={:#x}, stack={:#x}", entry, stack);
        
        // Build an interrupt frame to return to user mode with new process
        // This simulates a return from interrupt but with new RIP and RSP
        core::arch::asm!(
            // Set up user mode segments
            "mov ax, (4 << 3) | 3",  // User data segment with RPL=3
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            
            // Push stack frame for iretq
            "push {user_ss}",         // SS
            "push {user_rsp}",        // RSP
            "pushf",                  // RFLAGS
            "pop rax",
            "or rax, 0x200",          // Set IF (interrupts enabled)
            "push rax",               // RFLAGS with IF
            "push {user_cs}",         // CS
            "push {user_rip}",        // RIP
            
            // Return to user mode
            "iretq",
            
            user_ss = in(reg) (4u64 << 3) | 3,  // RPL=3
            user_rsp = in(reg) stack,
            user_cs = in(reg) (3u64 << 3) | 3,  // RPL=3
            user_rip = in(reg) entry,
            options(noreturn)
        );
    }
}

/// POSIX wait4() system call - wait for process state change
fn syscall_wait4(pid: i64, _status: *mut i32, _options: i32, _rusage: *mut u8) -> u64 {
    crate::kinfo!("wait4(pid={}) called", pid);
    
    // TODO: Implement proper wait with process state tracking
    posix::set_errno(posix::errno::ECHILD); // No child processes
    u64::MAX
}

/// POSIX sigaction() system call - examine and change signal action
fn syscall_sigaction(signum: u64, _act: *const u8, _oldact: *mut u8) -> u64 {
    if signum >= crate::signal::NSIG as u64 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // TODO: Implement full sigaction with user-space handlers
    crate::kinfo!("sigaction(sig={}) called", signum);
    posix::set_errno(0);
    0
}

/// POSIX sigprocmask() system call - examine and change blocked signals
fn syscall_sigprocmask(_how: i32, _set: *const u64, _oldset: *mut u64) -> u64 {
    // TODO: Implement signal masking
    crate::kinfo!("sigprocmask() called");
    posix::set_errno(0);
    0
}

/// POSIX dup() system call - duplicate file descriptor
fn syscall_dup(oldfd: u64) -> u64 {
    // TODO: Implement file descriptor duplication
    crate::kinfo!("dup(fd={}) called", oldfd);
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

/// POSIX dup2() system call - duplicate file descriptor to specific FD
fn syscall_dup2(oldfd: u64, newfd: u64) -> u64 {
    // TODO: Implement file descriptor duplication to target FD
    crate::kinfo!("dup2(oldfd={}, newfd={}) called", oldfd, newfd);
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

/// POSIX sched_yield() system call - yield CPU to scheduler
fn syscall_sched_yield() -> u64 {
    crate::kinfo!("sched_yield() called");
    
    // Trigger scheduler to select next process
    if let Some(_next_pid) = crate::scheduler::schedule() {
        // TODO: Perform context switch to next process
        crate::kinfo!("Scheduler selected next process");
    }
    
    posix::set_errno(0);
    0
}

/// System reboot - requires privilege (Linux compatible)
/// cmd values: 0x01234567=RESTART, 0x4321FEDC=HALT, 0xCDEF0123=POWER_OFF
fn syscall_reboot(cmd: i32) -> u64 {
    crate::kinfo!("reboot(cmd={:#x}) called", cmd);
    
    // Check if caller is root (UID 0) or has CAP_SYS_BOOT
    // For now, we allow any process to reboot (simplified security)
    if !crate::auth::is_superuser() {
        crate::kwarn!("Reboot attempted by non-root user");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }
    
    // Linux reboot magic numbers
    const LINUX_REBOOT_CMD_RESTART: i32 = 0x01234567;
    const LINUX_REBOOT_CMD_HALT: i32 = 0x4321FEDC_u32 as i32;
    const LINUX_REBOOT_CMD_POWER_OFF: i32 = 0xCDEF0123_u32 as i32;
    
    match cmd {
        LINUX_REBOOT_CMD_RESTART => {
            crate::kinfo!("System reboot requested via syscall");
            crate::init::reboot();
        }
        LINUX_REBOOT_CMD_HALT => {
            crate::kinfo!("System halt requested via syscall");
            crate::init::shutdown();
        }
        LINUX_REBOOT_CMD_POWER_OFF => {
            crate::kinfo!("System power off requested via syscall");
            crate::init::shutdown();
        }
        _ => {
            crate::kwarn!("Invalid reboot command: {:#x}", cmd);
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    }
    
    // Never returns
    posix::set_errno(0);
    0
}

/// System shutdown - power off the system
fn syscall_shutdown() -> u64 {
    crate::kinfo!("shutdown() called");
    
    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("Shutdown attempted by non-root user");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }
    
    crate::kinfo!("System shutdown requested via syscall");
    crate::init::shutdown();
    
    // Never returns
    posix::set_errno(0);
    0
}

/// Get or set system runlevel
/// arg < 0: get current runlevel (return value)
/// arg >= 0: set runlevel (requires root)
fn syscall_runlevel(level: i32) -> u64 {
    if level < 0 {
        // Get current runlevel
        let current = crate::init::current_runlevel();
        crate::kinfo!("runlevel: get -> {:?}", current);
        posix::set_errno(0);
        return current as u64;
    }
    
    // Set runlevel (requires privilege)
    if !crate::auth::is_superuser() {
        crate::kwarn!("Runlevel change attempted by non-root user");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }
    
    // Validate runlevel
    let new_level = match level {
        0 => crate::init::RunLevel::Halt,
        1 => crate::init::RunLevel::SingleUser,
        2 => crate::init::RunLevel::MultiUser,
        3 => crate::init::RunLevel::MultiUserNetwork,
        4 => crate::init::RunLevel::Unused,
        5 => crate::init::RunLevel::MultiUserGUI,
        6 => crate::init::RunLevel::Reboot,
        _ => {
            crate::kwarn!("Invalid runlevel: {}", level);
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };
    
    crate::kinfo!("runlevel: set -> {:?}", new_level);
    
    match crate::init::change_runlevel(new_level) {
        Ok(_) => {
            posix::set_errno(0);
            0
        }
        Err(e) => {
            crate::kerror!("Failed to change runlevel: {}", e);
            posix::set_errno(posix::errno::EINVAL);
            u64::MAX
        }
    }
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(nr: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    match nr {
        SYS_WRITE => syscall_write(arg1, arg2, arg3),
        SYS_READ => syscall_read(arg1, arg2 as *mut u8, arg3 as usize),
        SYS_OPEN => syscall_open(arg1 as *const u8, arg2 as usize),
        SYS_CLOSE => syscall_close(arg1),
        SYS_STAT => syscall_stat(arg1 as *const u8, arg2 as usize, arg3 as *mut posix::Stat),
        SYS_FSTAT => syscall_fstat(arg1, arg2 as *mut posix::Stat),
        SYS_LSEEK => syscall_lseek(arg1, arg2 as i64, arg3),
        SYS_PIPE => syscall_pipe(arg1 as *mut [i32; 2]),
        SYS_DUP => syscall_dup(arg1),
        SYS_DUP2 => syscall_dup2(arg1, arg2),
        SYS_FORK => syscall_fork(),
        SYS_EXECVE => syscall_execve(arg1 as *const u8, arg2 as *const u64, arg3 as *const u64),
        SYS_EXIT => syscall_exit(arg1 as i32),
        SYS_WAIT4 => syscall_wait4(arg1 as i64, arg2 as *mut i32, arg3 as i32, 0 as *mut u8),
        SYS_KILL => syscall_kill(arg1, arg2),
        SYS_SIGACTION => syscall_sigaction(arg1, arg2 as *const u8, arg3 as *mut u8),
        SYS_SIGPROCMASK => syscall_sigprocmask(arg1 as i32, arg2 as *const u64, arg3 as *mut u64),
        SYS_GETPID => 1,
        SYS_GETPPID => syscall_getppid(),
        SYS_SCHED_YIELD => syscall_sched_yield(),
        SYS_LIST_FILES => syscall_list_files(
            arg1 as *mut u8,
            arg2 as usize,
            arg3 as *const ListDirRequest,
        ),
        SYS_GETERRNO => syscall_get_errno(),
        SYS_IPC_CREATE => syscall_ipc_create(),
        SYS_IPC_SEND => syscall_ipc_send(arg1 as *const IpcTransferRequest),
        SYS_IPC_RECV => syscall_ipc_recv(arg1 as *const IpcTransferRequest),
        SYS_USER_ADD => syscall_user_add(arg1 as *const UserRequest),
        SYS_USER_LOGIN => syscall_user_login(arg1 as *const UserRequest),
        SYS_USER_INFO => syscall_user_info(arg1 as *mut UserInfoReply),
        SYS_USER_LIST => syscall_user_list(arg1 as *mut u8, arg2 as usize),
        SYS_USER_LOGOUT => syscall_user_logout(),
        SYS_REBOOT => syscall_reboot(arg1 as i32),
        SYS_SHUTDOWN => syscall_shutdown(),
        SYS_RUNLEVEL => syscall_runlevel(arg1 as i32),
        _ => {
            crate::kinfo!("Unknown syscall: {}", nr);
            posix::set_errno(posix::errno::ENOSYS);
            0
        }
    }
}

global_asm!(
    ".global syscall_handler",
    "syscall_handler:",
    "push rbx",
    "push rcx", // return address
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11", // rflags
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "sub rsp, 8", // maintain 16-byte stack alignment before calling into Rust
    // Call syscall_dispatch(nr=rax, arg1=rdi, arg2=rsi, arg3=rdx)
    "mov rcx, rdx", // arg3
    "mov rdx, rsi", // arg2
    "mov rsi, rdi", // arg1
    "mov rdi, rax", // nr
    "call syscall_dispatch",
    // Return value is in rax
    "add rsp, 8",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "iretq"
);
