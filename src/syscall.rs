use crate::posix::{self, FileType};
use crate::process::{USER_REGION_SIZE, USER_VIRT_BASE};
use crate::uefi_compat::{self, BlockDescriptor, CompatCounts, NetworkDescriptor};
use core::{
    arch::global_asm,
    cmp,
    fmt::{self, Write},
    mem, ptr, slice, str,
};
use nexa_boot_info::FramebufferInfo;
use x86_64::instructions::interrupts;

/// System call numbers (POSIX-compliant where possible)
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_STAT: u64 = 4;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_FCNTL: u64 = 72;
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
pub const SYS_REBOOT: u64 = 169; // sys_reboot (Linux)
pub const SYS_SHUTDOWN: u64 = 230; // Custom: system shutdown
pub const SYS_RUNLEVEL: u64 = 231; // Custom: get/set runlevel

// Filesystem management calls
pub const SYS_MOUNT: u64 = 165; // sys_mount (Linux)
pub const SYS_UMOUNT: u64 = 166; // sys_umount (Linux)
pub const SYS_PIVOT_ROOT: u64 = 155; // sys_pivot_root (Linux)
pub const SYS_CHROOT: u64 = 161; // sys_chroot (Linux)

const STDIN: u64 = 0;
const STDOUT: u64 = 1;
const STDERR: u64 = 2;

const FD_BASE: u64 = 3;
const MAX_OPEN_FILES: usize = 16;
const LIST_FLAG_INCLUDE_HIDDEN: u64 = 0x1;
const USER_FLAG_ADMIN: u64 = 0x1;
const F_DUPFD: u64 = 0;
const F_GETFL: u64 = 3;
const F_SETFL: u64 = 4;

// UEFI compatibility bridge syscalls
pub const SYS_UEFI_GET_COUNTS: u64 = 240;
pub const SYS_UEFI_GET_FB_INFO: u64 = 241;
pub const SYS_UEFI_GET_NET_INFO: u64 = 242;
pub const SYS_UEFI_GET_BLOCK_INFO: u64 = 243;

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
struct MountRequest {
    source_ptr: u64,
    source_len: u64,
    target_ptr: u64,
    target_len: u64,
    fstype_ptr: u64,
    fstype_len: u64,
    flags: u64,
}

#[repr(C)]
struct PivotRootRequest {
    new_root_ptr: u64,
    new_root_len: u64,
    put_old_ptr: u64,
    put_old_len: u64,
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
    StdStream(StdStreamKind),
}

#[derive(Clone, Copy)]
enum StdStreamKind {
    Stdin,
    Stdout,
    Stderr,
}

#[derive(Clone, Copy)]
struct FileHandle {
    backing: FileBacking,
    position: usize,
    metadata: crate::posix::Metadata,
}

static mut FILE_HANDLES: [Option<FileHandle>; MAX_OPEN_FILES] = [None; MAX_OPEN_FILES];

impl StdStreamKind {
    fn fd(self) -> u64 {
        match self {
            StdStreamKind::Stdin => STDIN,
            StdStreamKind::Stdout => STDOUT,
            StdStreamKind::Stderr => STDERR,
        }
    }
}

fn std_stream_metadata(kind: StdStreamKind) -> crate::posix::Metadata {
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

fn std_stream_handle(kind: StdStreamKind) -> FileHandle {
    FileHandle {
        backing: FileBacking::StdStream(kind),
        position: 0,
        metadata: std_stream_metadata(kind),
    }
}

fn handle_for_fd(fd: u64) -> Result<FileHandle, i32> {
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

fn allocate_duplicate_slot(min_fd: u64, handle: FileHandle) -> Result<u64, i32> {
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

fn write_to_std_stream(kind: StdStreamKind, buf: u64, count: u64) -> u64 {
    if !user_buffer_in_range(buf, count) {
        let (stack_base, stack_top) = current_stack_bounds();
        crate::kwarn!(
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
        crate::kwarn!(
            "sys_write: high user buffer fd={} buf={:#x} count={} stack_base={:#x} stack_top={:#x}",
            kind.fd(),
            buf,
            count,
            stack_base,
            stack_top
        );
    }

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
}

const USER_LOW_START: u64 = 0x1000; // Skip null page to catch obvious bugs
const USER_LOW_END: u64 = 0x4000_0000; // 1 GiB identity-mapped user region

/// Write system call
fn syscall_write(fd: u64, buf: u64, count: u64) -> u64 {
    // crate::kdebug!("sys_write(fd={}, buf={:#x}, count={})", fd, buf, count);
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
        if let Some(handle) = FILE_HANDLES[idx] {
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
                _ => {}
            }
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

#[inline(always)]
fn user_buffer_in_range(buf: u64, count: u64) -> bool {
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

#[inline(always)]
fn current_stack_bounds() -> (u64, u64) {
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

/// Read system call
fn syscall_read(fd: u64, buf: *mut u8, count: usize) -> u64 {
    crate::kinfo!("sys_read(fd={}, count={})", fd, count);
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
        if let Some(handle) = FILE_HANDLES[idx].as_mut() {
            match handle.backing {
                FileBacking::StdStream(StdStreamKind::Stdin) => {
                    return read_from_keyboard(buf, count);
                }
                FileBacking::StdStream(_) => {
                    posix::set_errno(posix::errno::EBADF);
                    return u64::MAX;
                }
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

/// Exit system call - terminate current process
fn syscall_exit(code: i32) -> ! {
    let pid = crate::scheduler::current_pid().unwrap_or(0);
    crate::kinfo!("Process {} exiting with code: {}", pid, code);

    if pid == 0 {
        crate::kpanic!("Cannot exit from kernel context (PID 0)!");
    }

    // Set process state to Zombie
    let _ = crate::scheduler::set_process_state(pid, crate::process::ProcessState::Zombie);

    // TODO: Save exit code in process structure for parent's wait4()
    // TODO: Send SIGCHLD to parent process
    // TODO: Wake up parent if it's sleeping in wait4()

    crate::kinfo!("Process {} terminated, switching to next process", pid);

    // Switch to next ready process (never returns)
    crate::scheduler::do_schedule();

    // If no other process to run, halt
    crate::kpanic!("No other process to schedule after exit!");
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
            handle.position = limited as usize;
            posix::set_errno(0);
            return new_pos_u64;
        }
    }

    posix::set_errno(posix::errno::EBADF);
    u64::MAX
}

fn syscall_fcntl(fd: u64, cmd: u64, arg: u64) -> u64 {
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
            crate::kwarn!("fcntl: unsupported cmd={} for fd={}", cmd, fd);
            posix::set_errno(posix::errno::ENOSYS);
            u64::MAX
        }
    }
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

    crate::kinfo!(
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

    crate::kinfo!(
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

    crate::kinfo!(
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

    if !user_buffer_in_range(buf as u64, count as u64) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
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
fn syscall_fork(syscall_return_addr: u64) -> u64 {
    use crate::process::{INTERP_BASE, INTERP_REGION_SIZE, USER_VIRT_BASE};

    crate::kdebug!(
        "syscall_fork: syscall_return_addr = {:#x}",
        syscall_return_addr
    );

    // Get current process
    let current_pid = match crate::scheduler::get_current_pid() {
        Some(pid) => pid,
        None => {
            crate::kerror!("fork() called but no current process");
            return u64::MAX;
        }
    };

    // Get current process info
    let parent_process = match crate::scheduler::get_process(current_pid) {
        Some(proc) => proc,
        None => {
            crate::kerror!("fork() - current process {} not found", current_pid);
            return u64::MAX;
        }
    };

    crate::kinfo!("fork() called from PID {}", current_pid);

    // Allocate new PID for child
    let child_pid = crate::process::allocate_pid();

    // Create child process - start by copying parent state
    let mut child_process = parent_process;
    child_process.pid = child_pid;
    child_process.ppid = current_pid;
    child_process.state = crate::process::ProcessState::Ready;

    // Copy parent's context - child will resume from same point
    child_process.context = parent_process.context;
    // Child should return 0 from fork and resume from syscall return point
    child_process.context.rax = 0;
    // FIX: Set child's RIP to the syscall return address, not parent's current RIP
    // This ensures child resumes execution from the fork() call site in user code
    child_process.context.rip = syscall_return_addr;

    crate::kdebug!(
        "Child RIP set to {:#x}, Child RAX = 0",
        child_process.context.rip
    );

    // TODO: Full production implementation needs:
    // 1. Copy entire memory space (code, data, heap, stack)
    //    - Allocate new physical pages for child
    //    - Copy USER_VIRT_BASE to (INTERP_BASE + INTERP_REGION_SIZE)
    //    - This includes: code, heap, stack, interpreter region
    // 2. Set up separate page tables for child process
    //    - Create new CR3 (page directory pointer)
    //    - Map child's physical pages to same virtual addresses
    //    - Implement copy-on-write (COW) for efficiency
    // 3. Copy file descriptor table
    //    - Duplicate all open file descriptors
    //    - Share underlying file objects but separate FD table
    // 4. Copy signal handlers and masks
    // 5. Handle shared memory regions correctly
    //
    // Current simplified version for fork+exec pattern:
    // - Shares parent's memory (no copy yet)
    // - Works because exec() immediately replaces memory
    // - Good enough for init/getty/shell pattern

    let memory_size = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
    crate::kinfo!(
        "fork() - memory copy needed: {:#x} bytes ({} KB) from {:#x}",
        memory_size,
        memory_size / 1024,
        USER_VIRT_BASE
    );

    // Simplified memory sharing for now
    // When we implement page tables per-process, we'll do proper COW
    crate::kdebug!("fork() - using shared memory model (OK for fork+exec pattern)");

    // Add child to scheduler
    if let Err(e) = crate::scheduler::add_process(child_process, 128) {
        crate::kerror!("fork() - failed to add child process: {}", e);
        return u64::MAX;
    }

    crate::kinfo!(
        "fork() created child PID {} from parent PID {} (child will return 0)",
        child_pid,
        current_pid
    );

    // Return child PID to parent (child gets 0 via context.rax)
    child_pid
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

    // Debug: Check if file exists before trying to read
    if crate::fs::file_exists(path_str) {
        crate::kinfo!("execve: file exists check passed for '{}'", path_str);
    } else {
        crate::kerror!("execve: file_exists returned false for '{}'", path_str);
    }

    // Note: In our simplified architecture, execve is mainly used by test programs.
    // Shell is launched via wait4() as part of the fork/wait sequence.

    // Try to load the ELF file from filesystem
    let elf_data = match crate::fs::read_file_bytes(path_str) {
        Some(data) => {
            crate::kinfo!(
                "execve: successfully read {} bytes from '{}'",
                data.len(),
                path_str
            );
            data
        }
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
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let entry = new_process.entry_point;
    let stack = new_process.stack_top;

    crate::kinfo!("execve: ELF loaded, entry={:#x}, stack={:#x}", entry, stack);
    crate::kinfo!("execve: switching to new process");

    unsafe {
        let gs_data_addr = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
        let gs_data_ptr = gs_data_addr as *mut u64;

        gs_data_ptr.add(0).write(stack);
        gs_data_ptr.add(2).write(entry);
        gs_data_ptr.add(3).write(stack);

        crate::kinfo!("execve: jumping to entry={:#x}, stack={:#x}", entry, stack);

        core::arch::asm!(
            "mov ax, (4 << 3) | 3",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",

            "push {user_ss}",
            "push {user_rsp}",
            "pushf",
            "pop rax",
            "or rax, 0x200",
            "push rax",
            "push {user_cs}",
            "push {user_rip}",

            "iretq",

            user_ss = in(reg) (4u64 << 3) | 3,
            user_rsp = in(reg) stack,
            user_cs = in(reg) (3u64 << 3) | 3,
            user_rip = in(reg) entry,
            options(noreturn)
        );
    }
}

/// POSIX wait4() system call - wait for process state change
/// SIMPLIFIED IMPLEMENTATION: Uses busy-waiting instead of true blocking
/// TODO: Proper implementation requires async/await or cooperative scheduling
fn syscall_wait4(pid: i64, status: *mut i32, options: i32, _rusage: *mut u8) -> u64 {
    crate::kinfo!("wait4(pid={}) called", pid);

    // Get current process PID
    let current_pid = match crate::scheduler::get_current_pid() {
        Some(pid) => pid,
        None => {
            crate::kerror!("wait4() called but no current process");
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!("wait4() from PID {} waiting for pid {}", current_pid, pid);

    // WNOHANG flag (non-blocking wait)
    const WNOHANG: i32 = 1;
    let is_nonblocking = (options & WNOHANG) != 0;

    // For now: implement as simple polling without context switching
    // This is a temporary solution until we have proper async/cooperative scheduling

    // Busy-wait loop: Keep checking child status
    // The child process needs to be scheduled out to run,
    // but we'll rely on the kernel's timer interrupt to preempt this process
    // and give other processes CPU time

    const MAX_CHECKS: u32 = 100000; // Increase significantly to give child more chances to run
    let mut check_count = 0u32;

    loop {
        check_count += 1;

        // Check if the specified child has exited
        let mut found_child = false;
        let mut child_exited = false;
        let mut child_exit_code = 0i32;
        let mut wait_pid = 0u64;

        // Query all children and look for matching one
        if pid == -1 {
            // Wait for any child
            for check_pid in 2..32 {
                if let Some(child_state) = crate::scheduler::get_child_state(current_pid, check_pid)
                {
                    found_child = true;
                    wait_pid = check_pid;

                    if child_state == crate::process::ProcessState::Zombie {
                        child_exited = true;
                        child_exit_code = 0;
                        crate::kinfo!(
                            "wait4() found exited child PID {} after {} checks",
                            check_pid,
                            check_count
                        );
                        break;
                    }
                }
            }
        } else if pid > 0 {
            // Wait for specific PID
            if let Some(child_state) = crate::scheduler::get_child_state(current_pid, pid as u64) {
                found_child = true;
                wait_pid = pid as u64;

                if child_state == crate::process::ProcessState::Zombie {
                    child_exited = true;
                    child_exit_code = 0;
                    crate::kinfo!(
                        "wait4() found exited specific child PID {} after {} checks",
                        pid,
                        check_count
                    );
                }
            }
        }

        // If child has exited, clean up and return
        if child_exited {
            let _ = crate::scheduler::remove_process(wait_pid);

            if !status.is_null() {
                unsafe {
                    *status = child_exit_code;
                }
            }

            crate::kinfo!(
                "wait4() returning child PID {} with status {}",
                wait_pid,
                child_exit_code
            );
            posix::set_errno(0);
            return wait_pid;
        }

        // If no child found at all, error
        if !found_child {
            crate::kinfo!("wait4() no matching child found");
            posix::set_errno(posix::errno::ECHILD);
            return u64::MAX;
        }

        // If non-blocking and child hasn't exited, return immediately
        if is_nonblocking {
            crate::kinfo!("wait4() WNOHANG: child not yet exited");
            posix::set_errno(0);
            return 0;
        }

        // Reached max checks
        if check_count >= MAX_CHECKS {
            crate::kwarn!(
                "wait4() exceeded max checks ({}) for child PID {}, returning anyway",
                MAX_CHECKS,
                wait_pid
            );
            // In a real implementation, the process would sleep here
            // For now, we return to avoid hanging
            posix::set_errno(0);
            return wait_pid;
        }

        // Busy wait: Just loop and keep checking
        // The kernel's timer interrupt will periodically context-switch to other processes
        // This is inefficient but will work until we implement proper blocking
        // Do a few busy loops to give other processes a chance to run
        for _ in 0..10 {
            unsafe {
                core::arch::asm!("nop");
            }
        }

        // Every few checks, yield to let other processes run
        if check_count % 100 == 0 {
            crate::kinfo!("wait4() yielding CPU at check {}", check_count);
            crate::scheduler::do_schedule();
        }
    }
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
fn syscall_dup2(oldfd: u64, newfd: u64) -> u64 {
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

/// POSIX sched_yield() system call - yield CPU to scheduler
fn syscall_sched_yield() -> u64 {
    crate::kinfo!("sched_yield() - yielding CPU to scheduler");

    // Perform context switch to next ready process
    crate::scheduler::do_schedule();

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

/// Mount a filesystem (simplified implementation)
///
/// TODO: This is a placeholder that validates arguments but doesn't perform actual mounting.
/// Real implementation requires:
/// - Block device layer for accessing storage
/// - Filesystem drivers (ext2, ext4, etc.)
/// - Mount point tracking
/// - VFS integration
fn syscall_mount(req_ptr: *const MountRequest) -> u64 {
    crate::kinfo!("syscall: mount");

    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("mount: permission denied");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Validate pointer is in user space (simplified check)
    let ptr_addr = req_ptr as usize;
    if req_ptr.is_null()
        || ptr_addr < USER_VIRT_BASE as usize
        || ptr_addr >= (USER_VIRT_BASE + USER_REGION_SIZE) as usize
    {
        crate::kwarn!("mount: invalid request pointer: {:#x}", ptr_addr);
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Read request structure
    let req = unsafe { &*req_ptr };

    // Read strings from userspace
    let source_slice =
        unsafe { slice::from_raw_parts(req.source_ptr as *const u8, req.source_len as usize) };
    let source = match str::from_utf8(source_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let target_slice =
        unsafe { slice::from_raw_parts(req.target_ptr as *const u8, req.target_len as usize) };
    let target = match str::from_utf8(target_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let fstype_slice =
        unsafe { slice::from_raw_parts(req.fstype_ptr as *const u8, req.fstype_len as usize) };
    let fstype = match str::from_utf8(fstype_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!(
        "mount: source='{}' target='{}' fstype='{}'",
        source,
        target,
        fstype
    );

    // PLACEHOLDER: Return not implemented
    // Real implementation would:
    // 1. Open block device at 'source'
    // 2. Detect/verify filesystem type
    // 3. Create VFS mount structure
    // 4. Add to mount table
    crate::kwarn!("mount syscall not fully implemented, returning ENOSYS");
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

/// Unmount a filesystem
fn syscall_umount(target_ptr: *const u8, target_len: usize) -> u64 {
    crate::kinfo!("syscall: umount");

    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("umount: permission denied");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    if target_ptr.is_null() || target_len == 0 {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let target_slice = unsafe { slice::from_raw_parts(target_ptr, target_len) };
    let target = match str::from_utf8(target_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!("umount: target='{}'", target);

    // PLACEHOLDER: Return not implemented
    crate::kwarn!("umount syscall not fully implemented, returning ENOSYS");
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

/// Change root directory (chroot)
fn syscall_chroot(path_ptr: *const u8, path_len: usize) -> u64 {
    crate::kinfo!("syscall: chroot");

    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("chroot: permission denied");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    if path_ptr.is_null() || path_len == 0 {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let path_slice = unsafe { slice::from_raw_parts(path_ptr, path_len) };
    let path = match str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!("chroot: path='{}'", path);

    // PLACEHOLDER: Return not implemented
    // Real implementation would update process root directory in PCB
    crate::kwarn!("chroot syscall not fully implemented, returning ENOSYS");
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

/// Pivot root - change root filesystem
///
/// TODO: This is a placeholder that validates arguments but doesn't perform actual pivot.
/// Real implementation requires:
/// - VFS root switching
/// - Mount point migration
/// - Process root directory updates
/// - Initramfs memory cleanup
fn syscall_pivot_root(req_ptr: *const PivotRootRequest) -> u64 {
    crate::kinfo!("syscall: pivot_root");

    // Check privilege
    if !crate::auth::is_superuser() {
        crate::kwarn!("pivot_root: permission denied");
        posix::set_errno(posix::errno::EPERM);
        return u64::MAX;
    }

    // Validate pointer is in user space
    let ptr_addr = req_ptr as usize;
    if req_ptr.is_null()
        || ptr_addr < USER_VIRT_BASE as usize
        || ptr_addr >= (USER_VIRT_BASE + USER_REGION_SIZE) as usize
    {
        crate::kwarn!("pivot_root: invalid request pointer: {:#x}", ptr_addr);
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Read request structure
    let req = unsafe { &*req_ptr };

    let new_root_slice =
        unsafe { slice::from_raw_parts(req.new_root_ptr as *const u8, req.new_root_len as usize) };
    let new_root = match str::from_utf8(new_root_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let put_old_slice =
        unsafe { slice::from_raw_parts(req.put_old_ptr as *const u8, req.put_old_len as usize) };
    let put_old = match str::from_utf8(put_old_slice) {
        Ok(s) => s,
        Err(_) => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    crate::kinfo!("pivot_root: new_root='{}' put_old='{}'", new_root, put_old);

    // PLACEHOLDER: Return not implemented
    // Real implementation would:
    // 1. Verify new_root is a mount point
    // 2. Verify put_old is under new_root
    // 3. Swap root filesystem
    // 4. Move old root to put_old
    // 5. Update all process root directories
    crate::kwarn!("pivot_root syscall not fully implemented, returning ENOSYS");
    posix::set_errno(posix::errno::ENOSYS);
    u64::MAX
}

fn syscall_uefi_get_counts(out: *mut CompatCounts) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }
    let length = mem::size_of::<CompatCounts>() as u64;
    if !user_buffer_in_range(out as u64, length) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let counts = uefi_compat::counts();
    unsafe {
        ptr::write(out, counts);
    }
    posix::set_errno(0);
    0
}

fn syscall_uefi_get_fb_info(out: *mut FramebufferInfo) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }
    let length = mem::size_of::<FramebufferInfo>() as u64;
    if !user_buffer_in_range(out as u64, length) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let Some(info) = uefi_compat::framebuffer() else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, info);
    }
    posix::set_errno(0);
    0
}

fn syscall_uefi_get_net_info(index: usize, out: *mut NetworkDescriptor) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let length = mem::size_of::<NetworkDescriptor>() as u64;
    if !user_buffer_in_range(out as u64, length) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let Some(descriptor) = uefi_compat::network_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, descriptor);
    }
    posix::set_errno(0);
    0
}

fn syscall_uefi_get_block_info(index: usize, out: *mut BlockDescriptor) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let length = mem::size_of::<BlockDescriptor>() as u64;
    if !user_buffer_in_range(out as u64, length) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let Some(descriptor) = uefi_compat::block_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, descriptor);
    }
    posix::set_errno(0);
    0
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(
    nr: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    syscall_return_addr: u64,
) -> u64 {
    let result = match nr {
        SYS_WRITE => syscall_write(arg1, arg2, arg3),
        SYS_READ => syscall_read(arg1, arg2 as *mut u8, arg3 as usize),
        SYS_OPEN => syscall_open(arg1 as *const u8, arg2 as usize),
        SYS_CLOSE => syscall_close(arg1),
        SYS_STAT => syscall_stat(arg1 as *const u8, arg2 as usize, arg3 as *mut posix::Stat),
        SYS_FSTAT => syscall_fstat(arg1, arg2 as *mut posix::Stat),
        SYS_LSEEK => syscall_lseek(arg1, arg2 as i64, arg3),
        SYS_FCNTL => syscall_fcntl(arg1, arg2, arg3),
        SYS_PIPE => syscall_pipe(arg1 as *mut [i32; 2]),
        SYS_DUP => syscall_dup(arg1),
        SYS_DUP2 => syscall_dup2(arg1, arg2),
        SYS_FORK => syscall_fork(syscall_return_addr),
        SYS_EXECVE => syscall_execve(arg1 as *const u8, arg2 as *const u64, arg3 as *const u64),
        SYS_EXIT => syscall_exit(arg1 as i32),
        SYS_WAIT4 => syscall_wait4(arg1 as i64, arg2 as *mut i32, arg3 as i32, 0 as *mut u8),
        SYS_KILL => syscall_kill(arg1, arg2),
        SYS_SIGACTION => syscall_sigaction(arg1, arg2 as *const u8, arg3 as *mut u8),
        SYS_SIGPROCMASK => syscall_sigprocmask(arg1 as i32, arg2 as *const u64, arg3 as *mut u64),
        SYS_GETPID => {
            crate::kdebug!("SYS_GETPID called");
            1
        }
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
        SYS_MOUNT => syscall_mount(arg1 as *const MountRequest),
        SYS_UMOUNT => syscall_umount(arg1 as *const u8, arg2 as usize),
        SYS_CHROOT => syscall_chroot(arg1 as *const u8, arg2 as usize),
        SYS_PIVOT_ROOT => syscall_pivot_root(arg1 as *const PivotRootRequest),
        SYS_UEFI_GET_COUNTS => syscall_uefi_get_counts(arg1 as *mut CompatCounts),
        SYS_UEFI_GET_FB_INFO => syscall_uefi_get_fb_info(arg1 as *mut FramebufferInfo),
        SYS_UEFI_GET_NET_INFO => {
            syscall_uefi_get_net_info(arg1 as usize, arg2 as *mut NetworkDescriptor)
        }
        SYS_UEFI_GET_BLOCK_INFO => {
            syscall_uefi_get_block_info(arg1 as usize, arg2 as *mut BlockDescriptor)
        }
        _ => {
            crate::kwarn!("Unknown syscall: {}", nr);
            posix::set_errno(posix::errno::ENOSYS);
            0
        }
    };
    // crate::kdebug!("syscall_dispatch return nr={} -> {:#x}", nr, result);
    result
}

global_asm!(
    ".global syscall_handler",
    "syscall_handler:",
    "push rbx",
    "push rcx", // return address (will be parameter 5)
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
    // Stack layout after all pushes and sub:
    // We pushed 16 registers (rbx, rcx, rdx, rsi, rdi, rbp, r8, r9, r10, r11, r12, r13, r14, r15)
    // That's 16 * 8 = 128 bytes
    // Then sub rsp, 8 adds 8 more
    // Original RCX is at position 2 from top (after rbx), which is [rsp + 120]
    // (120 = 15*8, since we need to skip 15 registers above rcx to reach it from new rsp)
    "mov r8, [rsp + 120]", // Get original RCX (syscall return address) -> r8 (param 5)
    "mov rcx, rdx",        // arg3 -> rcx (param 4)
    "mov rdx, rsi",        // arg2 -> rdx (param 3)
    "mov rsi, rdi",        // arg1 -> rsi (param 2)
    "mov rdi, rax",        // nr -> rdi (param 1)
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
