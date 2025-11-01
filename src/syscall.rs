use crate::posix::{self, FileType};
use core::{arch::global_asm, cmp, ptr, slice, str};
use x86_64::instructions::interrupts;

/// System call numbers
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_STAT: u64 = 4;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_EXIT: u64 = 60;
pub const SYS_GETPID: u64 = 39;
pub const SYS_LIST_FILES: u64 = 200;
pub const SYS_GETERRNO: u64 = 201;

const STDIN: u64 = 0;
const STDOUT: u64 = 1;
const STDERR: u64 = 2;

const FD_BASE: u64 = 3;
const MAX_OPEN_FILES: usize = 16;
const MAX_STDIN_LINE: usize = 512;

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
    if fd == STDOUT || fd == STDERR {
        let slice = unsafe { slice::from_raw_parts(buf as *const u8, count as usize) };

        for &byte in slice {
            crate::serial::write_byte(byte);
        }

        crate::vga_buffer::with_writer(|writer| {
            use core::fmt::Write;

            if let Ok(text) = str::from_utf8(slice) {
                writer.write_str(text).ok();
            } else {
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
        });

        count
    } else {
        0
    }
}

/// Read system call
fn syscall_read(fd: u64, buf: *mut u8, count: usize) -> u64 {
    if count == 0 || buf.is_null() {
        return 0;
    }

    if fd == STDIN {
        return read_from_keyboard(buf, count);
    }

    if fd < FD_BASE {
        posix::set_errno(posix::errno::EBADF);
        return 0;
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return 0;
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
    0
}

/// Exit system call
fn syscall_exit(code: i32) -> u64 {
    crate::kinfo!("Process exited with code: {}", code);
    // In a real OS, we would switch to another process
    // For now, just halt
    loop {
        x86_64::instructions::hlt();
    }
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

fn syscall_list_files(buf: *mut u8, count: usize) -> u64 {
    if buf.is_null() || count == 0 {
        return 0;
    }

    let mut written = 0usize;
    let mut overflow = false;

    crate::fs::list_directory("/", |name, _meta| {
        if overflow {
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

    posix::set_errno(0);
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

fn read_from_keyboard(buf: *mut u8, count: usize) -> u64 {
    if count == 0 {
        return 0;
    }

    let mut line = [0u8; MAX_STDIN_LINE];
    let max_copy = cmp::min(count.saturating_sub(1), MAX_STDIN_LINE - 1);

    // Allow the keyboard interrupt handler to run while we wait for input.
    // The INT 0x81 gate enters with IF=0, so without re-enabling here the
    // HLT inside `keyboard::read_line` would never resume.
    interrupts::enable();
    let read_len = crate::keyboard::read_line(&mut line[..max_copy]);
    interrupts::disable();

    unsafe {
        if read_len > 0 {
            ptr::copy_nonoverlapping(line.as_ptr(), buf, read_len);
        }
        let mut total = read_len;
        if total < count {
            *buf.add(total) = b'\n';
            total += 1;
        }
        total as u64
    }
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(nr: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    match nr {
        SYS_WRITE => {
            let ret = syscall_write(arg1, arg2, arg3);
            crate::kdebug!("SYSCALL_WRITE returned: {}", ret);
            ret
        }
        SYS_READ => syscall_read(arg1, arg2 as *mut u8, arg3 as usize),
        SYS_OPEN => syscall_open(arg1 as *const u8, arg2 as usize),
        SYS_CLOSE => syscall_close(arg1),
        SYS_STAT => syscall_stat(arg1 as *const u8, arg2 as usize, arg3 as *mut posix::Stat),
        SYS_FSTAT => syscall_fstat(arg1, arg2 as *mut posix::Stat),
        SYS_LSEEK => syscall_lseek(arg1, arg2 as i64, arg3),
        SYS_LIST_FILES => syscall_list_files(arg1 as *mut u8, arg2 as usize),
        SYS_EXIT => syscall_exit(arg1 as i32),
        SYS_GETPID => 1,
        SYS_GETERRNO => syscall_get_errno(),
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
    // Call syscall_dispatch(nr=rax, arg1=rdi, arg2=rsi, arg3=rdx)
    "mov rcx, rdx", // arg3
    "mov rdx, rsi", // arg2
    "mov rsi, rdi", // arg1
    "mov rdi, rax", // nr
    "call syscall_dispatch",
    // Return value is in rax
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
    "sysretq"
);
