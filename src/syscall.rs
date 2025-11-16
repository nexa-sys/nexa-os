use crate::paging;
use crate::posix::{self, FileType};
use crate::process::{USER_REGION_SIZE, USER_VIRT_BASE};
use crate::scheduler;
use crate::uefi_compat::{self, BlockDescriptor, CompatCounts, NetworkDescriptor, UsbHostDescriptor, HidInputDescriptor};
use crate::vt;
use core::{
    arch::global_asm,
    cmp,
    fmt::{self, Write},
    mem, ptr, slice, str,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use nexa_boot_info::FramebufferInfo;
use x86_64::instructions::interrupts;

/// Exec context - stores entry/stack/segments for exec syscall
/// Protected by atomics with release/acquire ordering for safety
struct ExecContext {
    pending: AtomicBool,
    entry: AtomicU64,
    stack: AtomicU64,
    #[allow(dead_code)]
    user_data_sel: AtomicU64, // User data segment selector (with RPL=3)
}

static EXEC_CONTEXT: ExecContext = ExecContext {
    pending: AtomicBool::new(false),
    entry: AtomicU64::new(0),
    stack: AtomicU64::new(0),
    user_data_sel: AtomicU64::new(0),
};

/// Get and clear exec context (called from assembly)
/// Returns: AL = 1 if exec was pending, 0 otherwise
/// Outputs: entry_out, stack_out, user_data_sel_out (each 8 bytes)
#[no_mangle]
pub extern "C" fn get_exec_context(entry_out: *mut u64, stack_out: *mut u64) -> bool {
    // Use SeqCst to ensure proper synchronization
    if EXEC_CONTEXT
        .pending
        .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        // All values are guaranteed to be visible after the compare_exchange
        let entry = EXEC_CONTEXT.entry.load(Ordering::SeqCst);
        let stack = EXEC_CONTEXT.stack.load(Ordering::SeqCst);
        unsafe {
            *entry_out = entry;
            *stack_out = stack;
        }
        crate::serial::_print(format_args!(
            "[get_exec_context] returning entry={:#x}, stack={:#x}\n",
            entry, stack
        ));
        true
    } else {
        crate::serial::_print(format_args!("[get_exec_context] no exec pending!\n"));
        false
    }
}

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

// Network socket calls (POSIX-compatible)
pub const SYS_SOCKET: u64 = 41;     // sys_socket (Linux)
pub const SYS_BIND: u64 = 49;       // sys_bind (Linux)
pub const SYS_SENDTO: u64 = 44;     // sys_sendto (Linux)
pub const SYS_RECVFROM: u64 = 45;   // sys_recvfrom (Linux)
pub const SYS_CONNECT: u64 = 42;    // sys_connect (Linux)
pub const SYS_GETSOCKNAME: u64 = 51; // sys_getsockname (Linux)
pub const SYS_GETPEERNAME: u64 = 52; // sys_getpeername (Linux)

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

// Socket domain constants (POSIX)
const AF_INET: i32 = 2;       // IPv4
const AF_INET6: i32 = 10;     // IPv6

// Socket type constants (POSIX)
const SOCK_STREAM: i32 = 1;   // TCP
const SOCK_DGRAM: i32 = 2;    // UDP
const SOCK_RAW: i32 = 3;      // Raw sockets

// Socket protocol constants (POSIX)
const IPPROTO_IP: i32 = 0;    // Dummy protocol for TCP
const IPPROTO_ICMP: i32 = 1;  // ICMP
const IPPROTO_TCP: i32 = 6;   // TCP
const IPPROTO_UDP: i32 = 17;  // UDP

// UEFI compatibility bridge syscalls
pub const SYS_UEFI_GET_COUNTS: u64 = 240;
pub const SYS_UEFI_GET_FB_INFO: u64 = 241;
pub const SYS_UEFI_GET_NET_INFO: u64 = 242;
pub const SYS_UEFI_GET_BLOCK_INFO: u64 = 243;
pub const SYS_UEFI_MAP_NET_MMIO: u64 = 244;
pub const SYS_UEFI_GET_USB_INFO: u64 = 245;
pub const SYS_UEFI_GET_HID_INFO: u64 = 246;
pub const SYS_UEFI_MAP_USB_MMIO: u64 = 247;

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

/// sockaddr_in structure (POSIX)
#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn {
    sin_family: u16,    // AF_INET
    sin_port: u16,      // Port number (network byte order)
    sin_addr: u32,      // IPv4 address (network byte order)
    sin_zero: [u8; 8],  // Padding to match sockaddr size
}

/// Generic sockaddr structure (POSIX)
#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddr {
    sa_family: u16,     // Address family
    sa_data: [u8; 14],  // Address data
}

/// Socket handle - references a socket in the network stack
#[derive(Clone, Copy)]
struct SocketHandle {
    socket_index: usize,  // Index into network stack's socket table
    domain: i32,          // AF_INET, AF_INET6
    socket_type: i32,     // SOCK_STREAM, SOCK_DGRAM
    protocol: i32,        // IPPROTO_TCP, IPPROTO_UDP
}

#[derive(Clone, Copy)]
enum FileBacking {
    Inline(&'static [u8]),
    Ext2(crate::fs::ext2::FileRef),
    StdStream(StdStreamKind),
    Socket(SocketHandle),
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

    let tty = scheduler::get_current_pid()
        .and_then(|pid| scheduler::get_process(pid))
        .map(|proc| proc.tty())
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
                FileBacking::Socket(_) => {
                    // Socket read not yet implemented
                    posix::set_errno(posix::errno::ENOTSUP);
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

    let slice = unsafe { core::slice::from_raw_parts_mut(buf, count) };
    let tty = scheduler::get_current_pid()
        .and_then(|pid| scheduler::get_process(pid))
        .map(|proc| proc.tty())
        .unwrap_or_else(|| vt::active_terminal());
    let read_len = crate::keyboard::read_raw_for_tty(tty, slice, count);

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

    // CRITICAL: Get the ACTUAL userspace RSP from GS_DATA
    // The syscall_interrupt_handler stores user_rsp in GS_DATA at a known offset.
    // We'll use the assembler interface to read it from GS segment.
    let user_rsp: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[0]",  // Read from GS_DATA slot 0 (assuming user_rsp is stored there)
            out(reg) user_rsp
        );
    }
    
    crate::serial::_print(format_args!(
        "[fork] Retrieved user_rsp from GS_DATA: {:#x}\n",
        user_rsp
    ));

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
    crate::serial::_print(format_args!("[fork] PID {} calling fork\n", current_pid));

    let parent_pid = current_pid;  // Store for later use

    // Allocate new PID for child
    let child_pid = crate::process::allocate_pid();
    
    crate::serial::_print(format_args!("[fork] Allocated child PID {}\n", child_pid));

    // Create child process - start by copying parent state
    let mut child_process = parent_process;
    child_process.pid = child_pid;
    child_process.ppid = current_pid;
    child_process.state = crate::process::ProcessState::Ready;

    // Forked children resume in user mode directly, so drive them through the
    // first-run path instead of inheriting the parent's in-kernel context.
    child_process.has_entered_user = false;

    // Repoint entry/stack so jump_to_usermode will return at the original
    // userspace instruction that invoked fork().
    child_process.entry_point = syscall_return_addr;
    child_process.stack_top = user_rsp;

    // Provide a clean context block for debugging/diagnostics even though the
    // first-run path bypasses it.
    child_process.context = crate::process::Context::zero();
    child_process.context.rax = 0;
    child_process.context.rip = syscall_return_addr;
    child_process.context.rsp = user_rsp;

    crate::kdebug!(
        "Child entry set to {:#x}, stack={:#x}",
        child_process.entry_point,
        child_process.stack_top
    );

    crate::serial::_print(format_args!(
        "[fork] User RSP (from GS_DATA)={:#x}, Kernel RSP={:#x}, Child stack_top={:#x}\n",
        user_rsp, parent_process.context.rsp, child_process.stack_top
    ));

    // FULL FORK IMPLEMENTATION: Copy entire user space memory
    // This ensures child has its own copy of code, data, heap, and stack
    let memory_size = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
    crate::kinfo!(
        "fork() - copying {:#x} bytes ({} KB) from {:#x}",
        memory_size,
        memory_size / 1024,
        USER_VIRT_BASE
    );
    
    crate::serial::_print(format_args!("[fork] Copying {} KB of memory\n", memory_size / 1024));

    // Allocate new physical memory for child process
    // We need to find a free physical region to copy parent's memory
    // For now, we'll allocate at a different physical location
    let child_phys_base = match crate::paging::allocate_user_region(memory_size) {
        Some(addr) => addr,
        None => {
            crate::kerror!("fork() - failed to allocate memory for child process");
            return u64::MAX;
        }
    };

    crate::kdebug!(
        "fork() - allocated child memory at physical {:#x}",
        child_phys_base
    );

    // Copy parent's memory to child's new physical location
    // CRITICAL: Parent's data is at VIRTUAL address USER_VIRT_BASE,
    // but we need to read from parent's PHYSICAL memory.
    // When we're in syscall context, the page tables are still parent's,
    // so accessing USER_VIRT_BASE will give us parent's data.
    // However, we need to be careful about which physical memory to copy from.
    
    let parent_phys_base = parent_process.memory_base;
    
    crate::serial::_print(format_args!(
        "[fork] Parent PID {} phys_base={:#x}, child PID {} phys_base={:#x}\n",
        parent_pid, parent_phys_base, child_pid, child_phys_base));
    crate::serial::_print(format_args!("[fork] Copying {} KB from parent to child\n",
                                       memory_size / 1024));
    
    // DEBUG: Check parent's memory at path_buf location (0x9fe390 from shell output)
    unsafe {
        let test_addr = 0x9fe390u64 as *const u8;
        let test_bytes = core::slice::from_raw_parts(test_addr, 16);
        crate::serial::_print(format_args!(
            "[fork-debug] Parent mem at path_buf (0x9fe390): {:02x?}\n",
            test_bytes
        ));
    }
    
    unsafe {
        // THE KEY INSIGHT: 
        // We're currently running in syscall context with PARENT'S page tables active.
        // USER_VIRT_BASE points to parent's physical memory via parent's page tables.
        // But since we're in kernel mode, we can ALSO directly access physical memory.
        // 
        // Option 1: Copy from virtual (USER_VIRT_BASE) - uses parent's current mapping
        // Option 2: Copy from parent's physical address directly
        //
        // We use Option 1 because parent's page tables are active NOW.
        let src_ptr = USER_VIRT_BASE as *const u8;
        let dst_ptr = child_phys_base as *mut u8;
        
        crate::serial::_print(format_args!(
            "[fork] Detailed: reading from VIRT {:#x} (maps to parent phys {:#x}), writing to child PHYS {:#x}\n",
            src_ptr as u64, parent_phys_base, dst_ptr as u64));
        
        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, memory_size as usize);
        
        // DEBUG: Verify copy worked - check path_buf address in child's physical memory
        let child_test_addr = (child_phys_base + (0x9fe390 - USER_VIRT_BASE)) as *const u8;
        let child_test_bytes = core::slice::from_raw_parts(child_test_addr, 16);
        crate::serial::_print(format_args!(
            "[fork-debug] Child phys mem at path_buf (0x9fe390): {:02x?}\n",
            child_test_bytes
        ));
    }

    crate::kinfo!(
        "fork() - memory copied successfully, {} KB",
        memory_size / 1024
    );
    
    crate::serial::_print(format_args!("[fork] Memory copied successfully\n"));

    // Store child's physical base in the process struct
    child_process.memory_base = child_phys_base;
    child_process.memory_size = memory_size;
    child_process.cr3 = match crate::paging::create_process_address_space(
        child_phys_base,
        memory_size,
    ) {
        Ok(cr3) => cr3,
        Err(err) => {
            crate::kerror!(
                "fork() - failed to build page tables for child {}: {}",
                child_pid, err
            );
            return u64::MAX;
        }
    };
    
    crate::serial::_print(format_args!("[fork] Child memory_base={:#x}, memory_size={:#x}\n", 
                                       child_phys_base, memory_size));

    // Copy file descriptor table
    // For now, we share the FD table (TODO: implement proper copy)
    crate::kdebug!("fork() - FD table shared (TODO: implement copy)");

    // Add child to scheduler
    if let Err(e) = crate::scheduler::add_process(child_process, 128) {
        crate::kerror!("fork() - failed to add child process: {}", e);
        // TODO: Free allocated memory
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
///
/// Production-grade implementation:
/// - Replaces current process image while preserving PID/PPID (POSIX requirement)
/// - Resets signal handlers to SIG_DFL
/// - Returns special value to syscall_dispatch which modifies stack frame
/// - This allows sysretq to jump to new program without segment issues
///
/// Returns:
/// - Special encoded value on success (high 32 bits = upper entry, low 32 bits set to magic)
/// - u64::MAX on error with errno set
fn syscall_execve(path: *const u8, _argv: *const *const u8, _envp: *const *const u8) -> u64 {
    use crate::scheduler::get_current_pid;

    crate::serial::_print(format_args!("[syscall_execve] Called\n"));

    if path.is_null() {
        crate::serial::_print(format_args!("[syscall_execve] Error: path is null\n"));
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Read the path string from user space
    let path_str = unsafe {
        let mut len = 0;
        while len < 256 && *path.add(len) != 0 {
            len += 1;
        }
        if len >= 256 {
            crate::serial::_print(format_args!("[syscall_execve] Error: path too long\n"));
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
        let slice = core::slice::from_raw_parts(path, len);
        match core::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => {
                crate::serial::_print(format_args!("[syscall_execve] Error: invalid UTF-8 in path\n"));
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }
        }
    };
    
    crate::serial::_print(format_args!("[syscall_execve] Path: {}\n", path_str));
    
    // Load the ELF file from filesystem
    let elf_data = match crate::fs::read_file_bytes(path_str) {
        Some(data) => {
            crate::serial::_print(format_args!("[syscall_execve] Found file, {} bytes\n", data.len()));
            data
        }
        None => {
            crate::serial::_print(format_args!("[syscall_execve] Error: file not found: {}\n", path_str));
            posix::set_errno(posix::errno::ENOENT);
            return u64::MAX;
        }
    };

    // Create new process image from ELF
    let new_process = match crate::process::Process::from_elf(elf_data) {
        Ok(proc) => {
            crate::serial::_print(format_args!("[syscall_execve] Successfully loaded ELF, entry={:#x}, stack={:#x}\n", 
                    proc.entry_point, proc.stack_top));
            proc
        }
        Err(e) => {
            crate::serial::_print(format_args!("[syscall_execve] Error loading ELF: {}\n", e));
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    // Get current process and replace it with new image
    let current_pid = match get_current_pid() {
        Some(pid) => pid,
        None => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    // Update process table with new image
    {
        let mut table = crate::scheduler::process_table_lock();
        let mut found = false;

        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == current_pid {
                    found = true;

                    // Replace process image while preserving identity
                    entry.process.entry_point = new_process.entry_point;
                    entry.process.stack_top = new_process.stack_top;
                    entry.process.heap_start = new_process.heap_start;
                    entry.process.heap_end = new_process.heap_end;
                    entry.process.context = new_process.context;
                    entry.process.cr3 = new_process.cr3;
                    entry.process.memory_base = new_process.memory_base;
                    entry.process.memory_size = new_process.memory_size;
                    entry.process.has_entered_user = false;

                    // Reset signal handlers to SIG_DFL (POSIX requirement)
                    entry.process.signal_state.reset_to_default();
                    break;
                }
            }
        }

        if !found {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    }

    // Store new entry/stack for syscall_handler to use
    // CRITICAL: Store in specific order with proper orderings
    // Both entry and stack must be visible before pending is set to true

    // Get user data segment selector (kept for future use but not stored for iretq path)
    let _user_data_sel = unsafe {
        let selectors = crate::gdt::get_selectors();
        let sel = selectors.user_data_selector.0 as u64;
        sel | 3 // Add RPL=3
    };

    // Store entry first
    EXEC_CONTEXT
        .entry
        .store(new_process.entry_point, Ordering::SeqCst);
    // Store stack second
    EXEC_CONTEXT
        .stack
        .store(new_process.stack_top, Ordering::SeqCst);

    // Finally, signal that exec context is ready
    // SeqCst ensures all prior stores are visible before this store
    EXEC_CONTEXT.pending.store(true, Ordering::SeqCst);

    // Return magic value 0xEXEC0000 to signal exec
    0x4558454300000000 // "EXEC" + nulls
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

fn syscall_uefi_map_net_mmio(index: usize) -> u64 {
    let Some(descriptor) = uefi_compat::network_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    if descriptor.mmio_base == 0 {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    }

    let span = if descriptor.mmio_length == 0 {
        0x1000
    } else {
        descriptor
            .mmio_length
            .min(u64::from(usize::MAX as u32))
            .max(0x1000)
    } as usize;

    let map_result = unsafe { paging::map_user_device_region(descriptor.mmio_base, span) };
    match map_result {
        Ok(ptr) => {
            posix::set_errno(0);
            ptr as u64
        }
        Err(paging::MapDeviceError::OutOfTableSpace) => {
            posix::set_errno(posix::errno::ENOMEM);
            u64::MAX
        }
    }
}

/// SYS_SOCKET - Create a socket
/// Returns: socket fd on success, -1 on error
fn syscall_socket(domain: i32, socket_type: i32, protocol: i32) -> u64 {
    // Validate parameters
    if domain != AF_INET {
        posix::set_errno(posix::errno::ENOSYS); // Only IPv4 supported for now
        return u64::MAX;
    }

    if socket_type != SOCK_DGRAM {
        posix::set_errno(posix::errno::ENOSYS); // Only UDP supported for now
        return u64::MAX;
    }

    if protocol != 0 && protocol != IPPROTO_UDP {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Find free file descriptor
    unsafe {
        for idx in 0..MAX_OPEN_FILES {
            if FILE_HANDLES[idx].is_none() {
                // Socket will be created later on bind()
                // For now, just reserve the fd with a socket handle
                let socket_handle = SocketHandle {
                    socket_index: usize::MAX, // Not allocated yet
                    domain,
                    socket_type,
                    protocol: IPPROTO_UDP,
                };

                let metadata = crate::posix::Metadata::empty()
                    .with_type(crate::posix::FileType::Socket)
                    .with_uid(0)
                    .with_gid(0)
                    .with_mode(0o0600);

                let handle = FileHandle {
                    backing: FileBacking::Socket(socket_handle),
                    position: 0,
                    metadata,
                };

                FILE_HANDLES[idx] = Some(handle);
                posix::set_errno(0);
                return FD_BASE + idx as u64;
            }
        }
    }

    // No free file descriptors
    posix::set_errno(posix::errno::EMFILE);
    u64::MAX
}

/// SYS_BIND - Bind socket to local address
/// Returns: 0 on success, -1 on error
fn syscall_bind(sockfd: u64, addr: *const SockAddr, addrlen: u32) -> u64 {
    if addr.is_null() || addrlen < 16 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Get socket handle
    let idx = if sockfd >= FD_BASE {
        (sockfd - FD_BASE) as usize
    } else {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    };

    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = FILE_HANDLES[idx].as_mut() else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        let FileBacking::Socket(ref mut sock_handle) = handle.backing else {
            posix::set_errno(posix::errno::ENOTSOCK);
            return u64::MAX;
        };

        // Parse sockaddr_in from generic sockaddr
        let addr_ref = &*addr;
        if addr_ref.sa_family != AF_INET as u16 {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        // Extract port from sa_data (first 2 bytes, network byte order)
        let port = u16::from_be_bytes([addr_ref.sa_data[0], addr_ref.sa_data[1]]);

        // TODO: Allocate socket in network stack and bind to port
        // For now, just store the socket index as port for testing
        sock_handle.socket_index = port as usize;

        crate::kinfo!("[SYS_BIND] sockfd={} bound to port {}", sockfd, port);
        posix::set_errno(0);
        0
    }
}

/// SYS_SENDTO - Send datagram to specified address
/// Returns: number of bytes sent on success, -1 on error
fn syscall_sendto(
    sockfd: u64,
    buf: *const u8,
    len: usize,
    _flags: i32,
    dest_addr: *const SockAddr,
    addrlen: u32,
) -> u64 {
    if buf.is_null() || len == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if dest_addr.is_null() || addrlen < 16 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Get socket handle
    let idx = if sockfd >= FD_BASE {
        (sockfd - FD_BASE) as usize
    } else {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    };

    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = FILE_HANDLES[idx].as_ref() else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        let FileBacking::Socket(_sock_handle) = handle.backing else {
            posix::set_errno(posix::errno::ENOTSOCK);
            return u64::MAX;
        };

        // Parse destination address
        let addr_ref = &*dest_addr;
        if addr_ref.sa_family != AF_INET as u16 {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        // Extract port and IP address
        let port = u16::from_be_bytes([addr_ref.sa_data[0], addr_ref.sa_data[1]]);
        let ip = [
            addr_ref.sa_data[2],
            addr_ref.sa_data[3],
            addr_ref.sa_data[4],
            addr_ref.sa_data[5],
        ];

        // Copy user data to kernel buffer
        let data_slice = core::slice::from_raw_parts(buf, len);

        crate::kinfo!(
            "[SYS_SENDTO] sockfd={} sending {} bytes to {}.{}.{}.{}:{}",
            sockfd,
            len,
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            port
        );

        // TODO: Actually send via network stack
        // For now, just pretend we sent it
        posix::set_errno(0);
        len as u64
    }
}

/// SYS_RECVFROM - Receive datagram and source address
/// Returns: number of bytes received on success, -1 on error
fn syscall_recvfrom(
    sockfd: u64,
    buf: *mut u8,
    len: usize,
    _flags: i32,
    src_addr: *mut SockAddr,
    addrlen: *mut u32,
) -> u64 {
    if buf.is_null() || len == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Get socket handle
    let idx = if sockfd >= FD_BASE {
        (sockfd - FD_BASE) as usize
    } else {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    };

    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = FILE_HANDLES[idx].as_ref() else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        let FileBacking::Socket(_sock_handle) = handle.backing else {
            posix::set_errno(posix::errno::ENOTSOCK);
            return u64::MAX;
        };

        // TODO: Actually receive from network stack
        // For now, return EAGAIN (would block)
        posix::set_errno(posix::errno::EAGAIN);
        u64::MAX
    }
}

/// SYS_CONNECT - Connect socket to remote address
/// Returns: 0 on success, -1 on error
fn syscall_connect(sockfd: u64, addr: *const SockAddr, addrlen: u32) -> u64 {
    if addr.is_null() || addrlen < 16 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Get socket handle
    let idx = if sockfd >= FD_BASE {
        (sockfd - FD_BASE) as usize
    } else {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    };

    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = FILE_HANDLES[idx].as_ref() else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        let FileBacking::Socket(_sock_handle) = handle.backing else {
            posix::set_errno(posix::errno::ENOTSOCK);
            return u64::MAX;
        };

        // Parse destination address
        let addr_ref = &*addr;
        if addr_ref.sa_family != AF_INET as u16 {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        // For UDP, connect() just stores the default destination
        // It doesn't actually establish a connection
        posix::set_errno(0);
        0
    }
}

fn syscall_uefi_get_usb_info(index: usize, out: *mut UsbHostDescriptor) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let Some(descriptor) = uefi_compat::usb_host_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, descriptor);
    }
    posix::set_errno(0);
    0
}

fn syscall_uefi_get_hid_info(index: usize, out: *mut HidInputDescriptor) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let Some(descriptor) = uefi_compat::hid_input_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, descriptor);
    }
    posix::set_errno(0);
    0
}

fn syscall_uefi_map_usb_mmio(index: usize) -> u64 {
    let Some(descriptor) = uefi_compat::usb_host_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    if descriptor.mmio_base == 0 {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    }

    let span = if descriptor.mmio_size == 0 {
        0x1000
    } else {
        descriptor
            .mmio_size
            .min(u64::from(usize::MAX as u32))
            .max(0x1000)
    } as usize;

    let map_result = unsafe { paging::map_user_device_region(descriptor.mmio_base, span) };
    match map_result {
        Ok(ptr) => {
            posix::set_errno(0);
            ptr as u64
        }
        Err(paging::MapDeviceError::OutOfTableSpace) => {
            posix::set_errno(posix::errno::ENOMEM);
            u64::MAX
        }
    }
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
        SYS_EXECVE => syscall_execve(
            arg1 as *const u8,
            arg2 as *const *const u8,
            arg3 as *const *const u8,
        ),
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
        SYS_SOCKET => syscall_socket(arg1 as i32, arg2 as i32, arg3 as i32),
        SYS_BIND => syscall_bind(arg1, arg2 as *const SockAddr, arg3 as u32),
        SYS_SENDTO => syscall_sendto(
            arg1,
            arg2 as *const u8,
            arg3 as usize,
            0,                           // flags (arg4, need to access r10)
            0 as *const SockAddr,        // dest_addr (arg5, need to access r8)
            0,                            // addrlen (arg6, need to access r9)
        ),
        SYS_RECVFROM => syscall_recvfrom(
            arg1,
            arg2 as *mut u8,
            arg3 as usize,
            0,                      // flags (arg4)
            0 as *mut SockAddr,     // src_addr (arg5)
            0 as *mut u32,          // addrlen (arg6)
        ),
        SYS_CONNECT => syscall_connect(arg1, arg2 as *const SockAddr, arg3 as u32),
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
        SYS_UEFI_MAP_NET_MMIO => syscall_uefi_map_net_mmio(arg1 as usize),
        SYS_UEFI_GET_USB_INFO => {
            syscall_uefi_get_usb_info(arg1 as usize, arg2 as *mut UsbHostDescriptor)
        }
        SYS_UEFI_GET_HID_INFO => {
            syscall_uefi_get_hid_info(arg1 as usize, arg2 as *mut HidInputDescriptor)
        }
        SYS_UEFI_MAP_USB_MMIO => syscall_uefi_map_usb_mmio(arg1 as usize),
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
    // Save user RCX (return address) and R11 (rflags) because sysretq needs them
    "push r11", // save user rflags
    "push rcx", // save user return address
    "push rbx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "sub rsp, 8", // maintain 16-byte stack alignment before calling into Rust
    // Stack layout after sub rsp,8:
    // [rsp+0] = alignment padding
    // [rsp+8] = r15 (最后push的)
    // [rsp+16] = r14
    // [rsp+24] = r13
    // [rsp+32] = r12
    // [rsp+40] = r10
    // [rsp+48] = r9
    // [rsp+56] = r8
    // [rsp+64] = rbp
    // [rsp+72] = rdi
    // [rsp+80] = rsi
    // [rsp+88] = rdx
    // [rsp+96] = rbx
    // [rsp+104] = rcx (user return address, DO NOT OVERWRITE!) <-- THIS ONE
    // [rsp+112] = r11 (user rflags, 第一个push的)
    "mov r8, [rsp + 104]", // Get original RCX (syscall return address) -> r8 (param 5)
    // WARNING: RCX register will be used for param 4, but we must NOT overwrite [rsp+104]!
    // Prepare arguments for syscall_dispatch:
    // RDI = nr, RSI = arg1, RDX = arg2, RCX = arg3, R8 = syscall_return_addr
    "mov rcx, rdx", // arg3 -> rcx (param 4) - THIS OVERWRITES RCX REGISTER BUT NOT STACK
    "mov rdx, rsi", // arg2 -> rdx (param 3)
    "mov rsi, rdi", // arg1 -> rsi (param 2)
    "mov rdi, rax", // nr -> rdi (param 1)
    "call syscall_dispatch",
    // Return value is in rax
    // Check if this is exec returning (magic value 0x4558454300000000 = "EXEC")
    // Build magic value: 0x45 58 45 43 00 00 00 00
    // "EXEC" in little endian = 0x43 0x45 0x58 0x45, then 4 zero bytes
    // As u64: 0x0000000045584543, but we want "EXEC" in high bytes
    // Actually: ASCII "E"=0x45, "X"=0x58, "E"=0x45, "C"=0x43
    // Little endian u64 of these bytes in order: 0x43 0x45 0x58 0x45 0x00 0x00 0x00 0x00
    // Which is: 0x0000000045584543
    // Wait, let me recalculate: if we write "EXEC" as bytes: 'E'(0x45) 'X'(0x58) 'E'(0x45) 'C'(0x43)
    // In memory (little endian): [0x43, 0x45, 0x58, 0x45, 0x00, 0x00, 0x00, 0x00]
    // As u64: 0x0000000045584543
    // But the code says: 0x4558454300000000
    // Let me check: 0x45 = 'E', 0x58 = 'X', 0x45 = 'E', 0x43 = 'C'
    // If we want bytes to be [0x45, 0x58, 0x45, 0x43, 0, 0, 0, 0] in memory:
    // Little endian interprets this as: 0x00000000_43455845
    // Hmm, this doesn't match 0x4558454300000000
    // Let me think differently:
    // Magic value 0x4558454300000000 in hex
    // High 32 bits: 0x45584543 = bytes [0x43, 0x45, 0x58, 0x45] = "CEXE" reversed = "EXEC"
    // Low 32 bits: 0x00000000 = four zero bytes
    // So the magic is correct as 0x45584543_00000000
    "movabs rbx, 0x4558454300000000",
    "cmp rax, rbx",
    "jne .Lnormal_return", // Not exec, normal return
    // Exec return: call get_exec_context to get entry/stack
    ".Lexec_return:",
    // CRITICAL: We're still in syscall_handler's stack frame here!
    // Stack state: [rsp+0]=alignment, [rsp+8...112]=saved regs
    // We need to call get_exec_context, which needs 16-byte aligned stack
    // Current rsp is already aligned (after sub rsp,8 at entry)
    // Allocate 32 bytes on top of current stack for output parameters
    // (entry 8 bytes at rsp+24, stack 8 bytes at rsp+16, user_data_sel 8 bytes at rsp+8, plus 8 for alignment)
    "sub rsp, 32", // Keep 16-byte alignment: 32 is divisible by 16
    // Now rsp is still 16-byte aligned (was aligned, sub 32 keeps it aligned)
    "lea rdi, [rsp + 24]", // entry_out = rsp+24 (first parameter)
    "lea rsi, [rsp + 16]", // stack_out = rsp+16 (second parameter)
    "lea rdx, [rsp + 8]",  // user_data_sel_out = rsp+8 (third parameter)
    "call get_exec_context",
    "test al, al",                   // Check if exec was pending
    "jz .Lnormal_return_after_exec", // Not exec, treat as normal
    // Exec successful: load new entry, stack, and user_data_sel
    "mov rcx, [rsp + 24]", // Load entry -> RCX (for sysretq)
    "mov r8, [rsp + 16]",  // Load stack -> R8 (temp, we'll move to RSP)
    "mov r9, [rsp + 8]",   // Load user_data_sel -> R9 (temp for segment selector)
    "mov rsp, r8",         // Switch to new user stack
    "mov r11, 0x202",      // User rflags (IF=1, reserved bit=1)
    "xor rax, rax",        // Clear return value for exec
    // Set user segment selectors from user_data_sel (in R9)
    "mov ax, r9w", // user data segment selector
    "mov ds, ax",
    "mov es, ax",
    "mov fs, ax",
    "mov gs, ax",
    // sysretq: rcx=rip, r11=rflags, rsp already set to user stack
    "sysretq",
    ".Lnormal_return_after_exec:",
    "add rsp, 32", // Clean up the 32 bytes we allocated
    ".Lnormal_return:",
    // Normal syscall return path
    "add rsp, 8", // remove alignment padding
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rbx",
    // Restore RCX and R11 for sysretq
    "pop rcx", // user return address
    "pop r11", // user rflags
    // Restore user segment registers before sysretq
    // Note: user data segment is now entry 3 (0x18 | 3 = 0x1B)
    "mov r8w, 0x1B", // user data segment selector (0x18 | 3)
    "mov ds, r8w",
    "mov es, r8w",
    "mov fs, r8w",
    "mov gs, r8w",
    // Return to user mode via sysretq (RCX=rip, R11=rflags, RAX=return value)
    "sysretq"
);
