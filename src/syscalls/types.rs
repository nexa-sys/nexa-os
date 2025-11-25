//! Shared type definitions for syscalls
//!
//! This module contains all the common types used across syscall implementations.

use crate::posix::{self, FileType};
use crate::vt;
use crate::{kdebug, kerror, ktrace, kwarn};
use core::ptr::{addr_of, addr_of_mut};
use core::{cmp, ptr, slice, str};

// File descriptor constants
pub const STDIN: u64 = 0;
pub const STDOUT: u64 = 1;
pub const STDERR: u64 = 2;
pub const FD_BASE: u64 = 3;
pub const MAX_OPEN_FILES: usize = 16;

// List files flags
pub const LIST_FLAG_INCLUDE_HIDDEN: u64 = 0x1;

// User flags
pub const USER_FLAG_ADMIN: u64 = 0x1;

// fcntl commands
pub const F_DUPFD: u64 = 0;
pub const F_GETFL: u64 = 3;
pub const F_SETFL: u64 = 4;

// Clock IDs
pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;
pub const CLOCK_BOOTTIME: i32 = 7;

// Socket domain and protocol constants (subset of POSIX)
pub const AF_INET: i32 = 2;
pub const AF_NETLINK: i32 = 16;
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;
pub const SOCK_RAW: i32 = 3;
pub const IPPROTO_TCP: i32 = 6;
pub const IPPROTO_UDP: i32 = 17;

// Socket option constants
pub const SOL_SOCKET: i32 = 1;
pub const SO_REUSEADDR: i32 = 2;
pub const SO_BROADCAST: i32 = 6;
pub const SO_RCVTIMEO: i32 = 20;
pub const SO_SNDTIMEO: i32 = 21;

// User address space bounds
pub const USER_LOW_START: u64 = 0x1000;
pub const USER_LOW_END: u64 = 0x4000_0000;

/// Request structure for listing directory contents
#[repr(C)]
pub struct ListDirRequest {
    pub path_ptr: u64,
    pub path_len: u64,
    pub flags: u64,
}

/// Request structure for user operations
#[repr(C)]
pub struct UserRequest {
    pub username_ptr: u64,
    pub username_len: u64,
    pub password_ptr: u64,
    pub password_len: u64,
    pub flags: u64,
}

/// Reply structure for user info
#[repr(C)]
pub struct UserInfoReply {
    pub username: [u8; 32],
    pub username_len: u64,
    pub uid: u32,
    pub gid: u32,
    pub is_admin: u32,
}

/// Request structure for mount operations
#[repr(C)]
pub struct MountRequest {
    pub source_ptr: u64,
    pub source_len: u64,
    pub target_ptr: u64,
    pub target_len: u64,
    pub fstype_ptr: u64,
    pub fstype_len: u64,
    pub flags: u64,
}

/// Request structure for pivot_root operations
#[repr(C)]
pub struct PivotRootRequest {
    pub new_root_ptr: u64,
    pub new_root_len: u64,
    pub put_old_ptr: u64,
    pub put_old_len: u64,
}

/// Request structure for IPC transfers
#[repr(C)]
pub struct IpcTransferRequest {
    pub channel_id: u32,
    pub flags: u32,
    pub buffer_ptr: u64,
    pub buffer_len: u64,
}

/// Time specification structure (POSIX timespec)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TimeSpec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// Generic sockaddr structure (POSIX)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockAddr {
    pub sa_family: u16,
    pub sa_data: [u8; 14],
}

/// Socket handle - references a socket in the network stack
#[derive(Clone, Copy)]
pub struct SocketHandle {
    pub socket_index: usize,
    pub domain: i32,
    pub socket_type: i32,
    pub protocol: i32,
    pub device_index: usize,
    pub broadcast_enabled: bool,
    pub recv_timeout_ms: u64,
}

/// File backing type
#[derive(Clone, Copy)]
pub enum FileBacking {
    Inline(&'static [u8]),
    Ext2(crate::fs::ext2::FileRef),
    StdStream(StdStreamKind),
    Socket(SocketHandle),
}

/// Standard stream kind
#[derive(Clone, Copy)]
pub enum StdStreamKind {
    Stdin,
    Stdout,
    Stderr,
}

impl StdStreamKind {
    pub fn fd(self) -> u64 {
        match self {
            StdStreamKind::Stdin => STDIN,
            StdStreamKind::Stdout => STDOUT,
            StdStreamKind::Stderr => STDERR,
        }
    }
}

/// File handle structure
#[derive(Clone, Copy)]
pub struct FileHandle {
    pub backing: FileBacking,
    pub position: usize,
    pub metadata: crate::posix::Metadata,
}

/// Global file handle table
pub static mut FILE_HANDLES: [Option<FileHandle>; MAX_OPEN_FILES] = [None; MAX_OPEN_FILES];

// Safe accessor functions for FILE_HANDLES to avoid creating references to static mut

/// Get a file handle by index (read-only copy)
///
/// # Safety
/// This must be called from a context where FILE_HANDLES is not being modified.
#[inline]
pub unsafe fn get_file_handle(idx: usize) -> Option<FileHandle> {
    if idx >= MAX_OPEN_FILES {
        return None;
    }
    let handles_ptr = addr_of!(FILE_HANDLES) as *const [Option<FileHandle>; MAX_OPEN_FILES];
    ptr::read((*handles_ptr).as_ptr().add(idx))
}

/// Set a file handle by index
///
/// # Safety
/// This must be called from a context where no other code is accessing FILE_HANDLES.
#[inline]
pub unsafe fn set_file_handle(idx: usize, handle: Option<FileHandle>) {
    if idx >= MAX_OPEN_FILES {
        return;
    }
    let handles_ptr = addr_of_mut!(FILE_HANDLES) as *mut [Option<FileHandle>; MAX_OPEN_FILES];
    ptr::write((*handles_ptr).as_mut_ptr().add(idx), handle);
}

/// Check if a file handle slot is empty
#[inline]
pub unsafe fn is_file_handle_empty(idx: usize) -> bool {
    get_file_handle(idx).is_none()
}

/// Update the position field of a file handle
///
/// # Safety
/// This must be called from a context where no other code is accessing FILE_HANDLES.
#[inline]
pub unsafe fn update_file_handle_position(idx: usize, new_position: usize) {
    if idx >= MAX_OPEN_FILES {
        return;
    }
    if let Some(mut handle) = get_file_handle(idx) {
        handle.position = new_position;
        set_file_handle(idx, Some(handle));
    }
}

/// Clear a file handle slot (set to None)
#[inline]
pub unsafe fn clear_file_handle(idx: usize) {
    set_file_handle(idx, None);
}

/// Find the first empty slot and return its index
#[inline]
pub unsafe fn find_empty_file_handle_slot() -> Option<usize> {
    for idx in 0..MAX_OPEN_FILES {
        if is_file_handle_empty(idx) {
            return Some(idx);
        }
    }
    None
}

/// Create metadata for standard streams
pub fn std_stream_metadata(kind: StdStreamKind) -> crate::posix::Metadata {
    use crate::posix::FileType;

    let mut meta = crate::posix::Metadata::empty()
        .with_type(FileType::Character)
        .with_uid(0)
        .with_gid(0);

    let perm: u16 = match kind {
        StdStreamKind::Stdin => 0o0600,
        StdStreamKind::Stdout | StdStreamKind::Stderr => 0o0600,
    };

    meta.mode = meta.mode | perm;
    meta.normalize()
}

/// Create a file handle for standard streams
pub fn std_stream_handle(kind: StdStreamKind) -> FileHandle {
    FileHandle {
        backing: FileBacking::StdStream(kind),
        position: 0,
        metadata: std_stream_metadata(kind),
    }
}

/// Get file handle for a given fd
pub fn handle_for_fd(fd: u64) -> Result<FileHandle, i32> {
    match fd {
        STDIN => Ok(std_stream_handle(StdStreamKind::Stdin)),
        STDOUT => Ok(std_stream_handle(StdStreamKind::Stdout)),
        STDERR => Ok(std_stream_handle(StdStreamKind::Stderr)),
        _ if fd >= FD_BASE => {
            let idx = (fd - FD_BASE) as usize;
            if idx >= MAX_OPEN_FILES {
                return Err(posix::errno::EBADF);
            }
            unsafe {
                if let Some(handle) = FILE_HANDLES[idx] {
                    Ok(handle)
                } else {
                    Err(posix::errno::EBADF)
                }
            }
        }
        _ => Err(posix::errno::EBADF),
    }
}

/// Allocate a duplicate file descriptor slot
pub fn allocate_duplicate_slot(min_fd: u64, handle: FileHandle) -> Result<u64, i32> {
    let min_fd = min_fd.max(FD_BASE);
    let start_idx = if min_fd <= FD_BASE {
        0
    } else {
        let offset = min_fd - FD_BASE;
        if offset >= MAX_OPEN_FILES as u64 {
            return Err(posix::errno::EMFILE);
        }
        offset as usize
    };

    unsafe {
        for idx in start_idx..MAX_OPEN_FILES {
            if FILE_HANDLES[idx].is_none() {
                FILE_HANDLES[idx] = Some(handle);
                posix::set_errno(0);
                return Ok(FD_BASE + idx as u64);
            }
        }
    }

    Err(posix::errno::EMFILE)
}

/// Check if a user buffer is within valid address range
#[inline(always)]
pub fn user_buffer_in_range(buf: u64, count: u64) -> bool {
    use crate::process::{USER_REGION_SIZE, USER_VIRT_BASE};

    if count == 0 {
        return true;
    }

    let Some(end) = buf.checked_add(count) else {
        return false;
    };

    let user_base = USER_VIRT_BASE;
    let user_end = USER_VIRT_BASE + USER_REGION_SIZE;

    let in_high_region = buf >= user_base && end <= user_end;
    let in_low_region = buf >= USER_LOW_START && end <= USER_LOW_END;

    in_high_region || in_low_region
}

/// Get current stack bounds from GS_DATA
#[inline(always)]
pub fn current_stack_bounds() -> (u64, u64) {
    unsafe {
        let gs_ptr = core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *const u64;
        let stack_top = gs_ptr.add(3).read();
        if stack_top == 0 {
            (0, 0)
        } else {
            let stack_base = stack_top.saturating_sub(crate::process::STACK_SIZE);
            (stack_base, stack_top)
        }
    }
}

/// Map auth error to errno
pub fn map_auth_error(err: crate::auth::AuthError) -> i32 {
    match err {
        crate::auth::AuthError::InvalidInput => posix::errno::EINVAL,
        crate::auth::AuthError::AlreadyExists => posix::errno::EEXIST,
        crate::auth::AuthError::TableFull => posix::errno::ENOSPC,
        crate::auth::AuthError::InvalidCredentials => posix::errno::EPERM,
        crate::auth::AuthError::AccessDenied => posix::errno::EACCES,
    }
}

/// Map IPC error to errno
pub fn map_ipc_error(err: crate::ipc::IpcError) -> i32 {
    match err {
        crate::ipc::IpcError::NoSuchChannel => posix::errno::ENOENT,
        crate::ipc::IpcError::TableFull => posix::errno::ENOSPC,
        crate::ipc::IpcError::WouldBlock | crate::ipc::IpcError::Empty => posix::errno::EAGAIN,
        crate::ipc::IpcError::InvalidInput => posix::errno::EINVAL,
    }
}

/// Buffer writer for formatting output
pub struct BufferWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
    overflow: bool,
}

impl<'a> BufferWriter<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            len: 0,
            overflow: false,
        }
    }

    pub fn written(&self) -> usize {
        self.len
    }

    pub fn overflowed(&self) -> bool {
        self.overflow
    }
}

impl core::fmt::Write for BufferWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if self.overflow {
            return Err(core::fmt::Error);
        }

        let bytes = s.as_bytes();
        if self.len + bytes.len() > self.buf.len() {
            self.overflow = true;
            return Err(core::fmt::Error);
        }

        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }
}
