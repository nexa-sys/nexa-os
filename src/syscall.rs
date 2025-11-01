use core::{arch::global_asm, cmp, ptr, slice, str};

/// System call numbers
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_EXIT: u64 = 60;
pub const SYS_GETPID: u64 = 39;
pub const SYS_LIST_FILES: u64 = 200;

const STDIN: u64 = 0;
const STDOUT: u64 = 1;
const STDERR: u64 = 2;

const FD_BASE: u64 = 3;
const MAX_OPEN_FILES: usize = 16;
const MAX_STDIN_LINE: usize = 512;

#[derive(Clone, Copy)]
struct FileHandle {
    data: &'static [u8],
    position: usize,
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
        return 0;
    }

    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        return 0;
    }

    unsafe {
        if let Some(handle) = FILE_HANDLES[idx].as_mut() {
            let remaining = handle.data.len().saturating_sub(handle.position);
            if remaining == 0 {
                return 0;
            }

            let to_copy = cmp::min(remaining, count);
            ptr::copy_nonoverlapping(
                handle.data.as_ptr().add(handle.position),
                buf,
                to_copy,
            );
            handle.position += to_copy;
            return to_copy as u64;
        }
    }

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
        return u64::MAX;
    }

    let raw = unsafe { slice::from_raw_parts(path_ptr, len) };
    let end = raw
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(raw.len());
    let trimmed = &raw[..end];
    let Ok(mut path) = str::from_utf8(trimmed) else {
        return u64::MAX;
    };

    path = path.trim();
    if path.is_empty() {
        return u64::MAX;
    }
    let normalized = path.strip_prefix('/').unwrap_or(path);

    if let Some(data) = crate::fs::read_file_bytes(normalized) {
        unsafe {
            for index in 0..MAX_OPEN_FILES {
                if FILE_HANDLES[index].is_none() {
                    FILE_HANDLES[index] = Some(FileHandle { data, position: 0 });
                    let fd = FD_BASE + index as u64;
                    crate::kinfo!("Opened file '{}' as fd {}", normalized, fd);
                    return fd;
                }
            }
        }
        crate::kwarn!("No free file handles available");
        u64::MAX
    } else {
        crate::kwarn!("sys_open: file '{}' not found", normalized);
        u64::MAX
    }
}

fn syscall_close(fd: u64) -> u64 {
    if fd < FD_BASE {
        return u64::MAX;
    }
    let idx = (fd - FD_BASE) as usize;
    if idx >= MAX_OPEN_FILES {
        return u64::MAX;
    }

    unsafe {
        if FILE_HANDLES[idx].is_some() {
            FILE_HANDLES[idx] = None;
            crate::kinfo!("Closed fd {}", fd);
            return 0;
        }
    }

    u64::MAX
}

fn syscall_list_files(buf: *mut u8, count: usize) -> u64 {
    if buf.is_null() || count == 0 {
        return 0;
    }

    let files = crate::fs::list_files();
    let mut written = 0usize;

    for entry in files.iter() {
        if let Some(file) = entry {
            let name = file.name.as_bytes();
            if written + name.len() + 1 > count {
                break;
            }
            unsafe {
                ptr::copy_nonoverlapping(name.as_ptr(), buf.add(written), name.len());
                written += name.len();
                *buf.add(written) = b'\n';
                written += 1;
            }
        }
    }

    written as u64
}

fn read_from_keyboard(buf: *mut u8, count: usize) -> u64 {
    if count == 0 {
        return 0;
    }

    let mut line = [0u8; MAX_STDIN_LINE];
    let max_copy = cmp::min(count.saturating_sub(1), MAX_STDIN_LINE - 1);
    let read_len = crate::keyboard::read_line(&mut line[..max_copy]);

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
    // Debug: write syscall number to VGA
    unsafe {
        *(0xB8000 as *mut u64) = 0x4142434445464748; // "ABCDEFGH" in ASCII
        *(0xB8008 as *mut u64) = nr; // Write syscall number
    }

    match nr {
        SYS_WRITE => {
            let ret = syscall_write(arg1, arg2, arg3);
            crate::kinfo!("SYSCALL_WRITE returned: {}", ret);
            ret
        }
        SYS_READ => syscall_read(arg1, arg2 as *mut u8, arg3 as usize),
        SYS_OPEN => syscall_open(arg1 as *const u8, arg2 as usize),
        SYS_CLOSE => syscall_close(arg1),
        SYS_LIST_FILES => syscall_list_files(arg1 as *mut u8, arg2 as usize),
        SYS_EXIT => syscall_exit(arg1 as i32),
        _ => {
            crate::kinfo!("Unknown syscall: {}", nr);
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
