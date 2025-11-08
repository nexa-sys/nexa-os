//! Minimal stdio support for NexaOS userspace (no_std)
//!
//! Goals:
//! - 提供与 libc 兼容的 C ABI: putchar/puts/getchar/gets/printf/scanf(占位)/fgets/fputs
//! - 直接通过内置的 syscall 封装进行读写，当前实现为无缓冲 I/O
//! - 简化的 printf 实现：支持 %s %c %d %u %x %%
//! - 线程模型：单线程，stdin=fd0、stdout=fd1、stderr=fd2

use core::{
    arch::asm,
    ffi::{c_void, VaListImpl},
    fmt::{self, Write},
    ptr, slice,
};

use crate::{get_errno, set_errno, translate_ret_isize, EINVAL};

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;

#[inline(always)]
fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
            clobber_abi("sysv64"),
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
fn write_fd(fd: i32, buf: *const u8, len: usize) -> isize {
    translate_ret_isize(syscall3(SYS_WRITE, fd as u64, buf as u64, len as u64))
}

#[inline(always)]
fn read_fd(fd: i32, buf: *mut u8, len: usize) -> isize {
    translate_ret_isize(syscall3(SYS_READ, fd as u64, buf as u64, len as u64))
}

const STDIN: i32 = 0;
const STDOUT: i32 = 1;
const STDERR: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FILE {
    fd: i32,
    flags: i32,
}

impl FILE {
    const fn new(fd: i32) -> Self {
        Self { fd, flags: 0 }
    }
}

static mut STDIN_FILE: FILE = FILE::new(STDIN);
static mut STDOUT_FILE: FILE = FILE::new(STDOUT);
static mut STDERR_FILE: FILE = FILE::new(STDERR);

#[no_mangle]
pub static mut stdin: *mut FILE = ptr::addr_of_mut!(STDIN_FILE);
#[no_mangle]
pub static mut stdout: *mut FILE = ptr::addr_of_mut!(STDOUT_FILE);
#[no_mangle]
pub static mut stderr: *mut FILE = ptr::addr_of_mut!(STDERR_FILE);

#[inline]
unsafe fn stream_fd(stream: *mut FILE) -> Result<i32, ()> {
    if stream.is_null() {
        set_errno(EINVAL);
        return Err(());
    }
    Ok((*stream).fd)
}

fn write_all_fd(fd: i32, mut buf: &[u8]) -> Result<(), i32> {
    while !buf.is_empty() {
        let written = write_fd(fd, buf.as_ptr(), buf.len());
        if written < 0 {
            return Err(get_errno());
        }
        let written = written as usize;
        if written == 0 {
            return Err(get_errno());
        }
        buf = &buf[written..];
    }
    Ok(())
}

fn echo_bytes(bytes: &[u8]) {
    let _ = write_all_fd(STDOUT, bytes);
}

fn read_blocking_byte() -> Result<u8, i32> {
    loop {
        let ch = getchar();
        if ch >= 0 {
            return Ok(ch as u8);
        }
        let err = get_errno();
        if err == 0 || err == 4 || err == 11 {
            continue;
        }
        return Err(err);
    }
}

enum EchoMode {
    None,
    Plain,
    Mask(u8),
}

fn read_line_internal(buf: &mut [u8], mode: EchoMode, skip_empty: bool) -> Result<usize, i32> {
    if buf.is_empty() {
        return Ok(0);
    }

    let max = buf.len().saturating_sub(1);
    let mut len = 0usize;

    loop {
        let byte = match read_blocking_byte() {
            Ok(b) => b,
            Err(err) => return Err(err),
        };

        match byte {
            b'\r' | b'\n' => {
                if len == 0 && skip_empty {
                    continue;
                }
                echo_bytes(b"\n");
                break;
            }
            8 | 127 => {
                if len > 0 {
                    len -= 1;
                    buf[len] = 0;
                    if !matches!(mode, EchoMode::None) {
                        echo_bytes(b"\x08 \x08");
                    }
                }
            }
            b if (0x20..=0x7e).contains(&b) => {
                if len < max {
                    buf[len] = b;
                    len += 1;
                    match mode {
                        EchoMode::Plain => echo_bytes(&[b]),
                        EchoMode::Mask(mask) => echo_bytes(&[mask]),
                        EchoMode::None => {}
                    }
                }
            }
            _ => {}
        }
    }

    buf[len] = 0;
    Ok(len)
}

#[no_mangle]
pub extern "C" fn putchar(c: i32) -> i32 {
    let ch = c as u8;
    let n = write_fd(STDOUT, &ch as *const u8, 1);
    if n == 1 {
        c
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn fputc(c: i32, _stream: *mut c_void) -> i32 {
    putchar(c)
}

#[no_mangle]
pub extern "C" fn puts(s: *const u8) -> i32 {
    if s.is_null() {
        set_errno(22); // EINVAL
        return -1;
    }
    let mut len = 0usize;
    unsafe {
        while ptr::read(s.add(len)) != 0 {
            len += 1;
        }
    }
    let bytes = unsafe { slice::from_raw_parts(s, len) };
    if write_all_fd(STDOUT, bytes).is_err() {
        return -1;
    }
    if write_all_fd(STDOUT, b"\n").is_err() {
        return -1;
    }
    1
}

#[no_mangle]
pub extern "C" fn fputs(s: *const u8, _stream: *mut c_void) -> i32 {
    if s.is_null() {
        set_errno(22);
        return -1;
    }
    let mut len = 0usize;
    unsafe {
        while ptr::read(s.add(len)) != 0 {
            len += 1;
        }
    }
    let bytes = unsafe { slice::from_raw_parts(s, len) };
    if write_all_fd(STDOUT, bytes).is_err() {
        return -1;
    }
    0
}

#[no_mangle]
pub extern "C" fn getchar() -> i32 {
    let mut ch = 0u8;
    let n = read_fd(STDIN, &mut ch as *mut u8, 1);
    if n == 1 {
        ch as i32
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn fgets(buf: *mut u8, size: i32, _stream: *mut c_void) -> *mut u8 {
    if buf.is_null() || size <= 1 {
        return core::ptr::null_mut();
    }

    let mut i = 0i32;
    while i < size - 1 {
        let c = getchar();
        if c < 0 {
            break;
        }
        unsafe {
            ptr::write(buf.add(i as usize), c as u8);
        }
        i += 1;
        if c as u8 == b'\n' {
            break;
        }
    }

    unsafe {
        ptr::write(buf.add(i as usize), 0);
    }
    if i == 0 {
        core::ptr::null_mut()
    } else {
        buf
    }
}

#[no_mangle]
pub unsafe extern "C" fn gets(buf: *mut u8) -> *mut u8 {
    if buf.is_null() {
        return core::ptr::null_mut();
    }
    let mut i = 0usize;
    loop {
        let c = getchar();
        if c <= 0 {
            break;
        }
        if c as u8 == b'\n' {
            break;
        }
        ptr::write(buf.add(i), c as u8);
        i += 1;
    }
    ptr::write(buf.add(i), 0);
    if i == 0 {
        core::ptr::null_mut()
    } else {
        buf
    }
}

pub fn stdout_write_all(buf: &[u8]) -> Result<(), i32> {
    write_all_fd(STDOUT, buf)
}

pub fn stdout_write_str(s: &str) -> Result<(), i32> {
    stdout_write_all(s.as_bytes())
}

pub fn stdout_write_fmt(args: fmt::Arguments<'_>) -> Result<(), i32> {
    struct Writer<'a> {
        last_error: &'a mut Option<i32>,
    }

    impl<'a> Write for Writer<'a> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            match stdout_write_str(s) {
                Ok(()) => Ok(()),
                Err(err) => {
                    *self.last_error = Some(err);
                    Err(fmt::Error)
                }
            }
        }
    }

    let mut last_error = None;
    {
        let mut writer = Writer {
            last_error: &mut last_error,
        };
        if writer.write_fmt(args).is_err() {
            return Err(last_error.unwrap_or_else(get_errno));
        }
    }
    Ok(())
}

pub fn stdout_flush() -> Result<(), i32> {
    unsafe {
        let stream = stdout;
        if stream.is_null() {
            return Ok(());
        }
        if fflush(stream) == 0 {
            Ok(())
        } else {
            Err(get_errno())
        }
    }
}

pub fn stderr_write_all(buf: &[u8]) -> Result<(), i32> {
    write_all_fd(STDERR, buf)
}

pub fn stderr_write_str(s: &str) -> Result<(), i32> {
    stderr_write_all(s.as_bytes())
}

pub fn stdin_read_line(buf: &mut [u8], skip_empty: bool) -> Result<usize, i32> {
    read_line_internal(buf, EchoMode::Plain, skip_empty)
}

pub fn stdin_read_line_masked(buf: &mut [u8], mask: u8, skip_empty: bool) -> Result<usize, i32> {
    read_line_internal(buf, EchoMode::Mask(mask), skip_empty)
}

pub fn stdin_read_line_noecho(buf: &mut [u8], skip_empty: bool) -> Result<usize, i32> {
    read_line_internal(buf, EchoMode::None, skip_empty)
}

unsafe fn write_formatted(fd: i32, fmt: *const u8, args: &mut VaListImpl<'_>) -> Result<i32, ()> {
    if fmt.is_null() {
        set_errno(EINVAL);
        return Err(());
    }

    let mut count = 0i32;
    let mut i = 0usize;

    loop {
        let ch = ptr::read(fmt.add(i));
        if ch == 0 {
            break;
        }

        if ch != b'%' {
            if write_fd(fd, &ch, 1) != 1 {
                return Err(());
            }
            count += 1;
            i += 1;
            continue;
        }

        i += 1;
        let spec = ptr::read(fmt.add(i));
        if spec == 0 {
            break;
        }

        match spec as char {
            '%' => {
                if write_fd(fd, &spec, 1) != 1 {
                    return Err(());
                }
                count += 1;
            }
            'c' => {
                let v: i32 = args.arg();
                let b = v as u8;
                if write_fd(fd, &b as *const u8, 1) != 1 {
                    return Err(());
                }
                count += 1;
            }
            's' => {
                let p: *const u8 = args.arg();
                if p.is_null() {
                    continue;
                }
                let mut len = 0usize;
                while ptr::read(p.add(len)) != 0 {
                    len += 1;
                }
                if write_fd(fd, p, len) != len as isize {
                    return Err(());
                }
                count += len as i32;
            }
            'd' | 'i' => {
                let v: i32 = args.arg();
                let mut buf = [0u8; 32];
                let s = itoa_signed(v as i64, &mut buf);
                if write_fd(fd, s.as_ptr(), s.len()) != s.len() as isize {
                    return Err(());
                }
                count += s.len() as i32;
            }
            'u' => {
                let v: u32 = args.arg();
                let mut buf = [0u8; 32];
                let s = itoa_unsigned(v as u64, &mut buf);
                if write_fd(fd, s.as_ptr(), s.len()) != s.len() as isize {
                    return Err(());
                }
                count += s.len() as i32;
            }
            'x' | 'X' => {
                let v: u64 = args.arg();
                let mut buf = [0u8; 32];
                let s = itoa_hex(v, &mut buf);
                if write_fd(fd, s.as_ptr(), s.len()) != s.len() as isize {
                    return Err(());
                }
                count += s.len() as i32;
            }
            _ => {
                if write_fd(fd, &b'%' as *const u8, 1) != 1 {
                    return Err(());
                }
                if write_fd(fd, &spec, 1) != 1 {
                    return Err(());
                }
                count += 2;
            }
        }

        i += 1;
    }

    set_errno(0);
    Ok(count)
}

#[no_mangle]
pub unsafe extern "C" fn printf(fmt: *const u8, mut args: ...) -> i32 {
    match write_formatted(STDOUT, fmt, &mut args) {
        Ok(count) => count,
        Err(()) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fprintf(stream: *mut FILE, fmt: *const u8, mut args: ...) -> i32 {
    let fd = match stream_fd(stream) {
        Ok(fd) => fd,
        Err(()) => return -1,
    };

    match write_formatted(fd, fmt, &mut args) {
        Ok(count) => count,
        Err(()) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fflush(stream: *mut FILE) -> i32 {
    if stream.is_null() {
        set_errno(0);
        return 0;
    }

    match stream_fd(stream) {
        Ok(_) => {
            set_errno(0);
            0
        }
        Err(()) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fwrite(
    ptr: *const c_void,
    size: usize,
    nmemb: usize,
    stream: *mut FILE,
) -> usize {
    if size == 0 || nmemb == 0 {
        return 0;
    }
    if ptr.is_null() {
        set_errno(EINVAL);
        return 0;
    }

    let fd = match stream_fd(stream) {
        Ok(fd) => fd,
        Err(()) => return 0,
    };

    let total = match size.checked_mul(nmemb) {
        Some(v) => v,
        None => {
            set_errno(EINVAL);
            return 0;
        }
    };

    set_errno(0);
    let mut written = 0usize;
    while written < total {
        let chunk = write_fd(fd, (ptr as *const u8).add(written), total - written);
        if chunk <= 0 {
            if chunk == 0 {
                set_errno(EINVAL);
            }
            break;
        }
        written += chunk as usize;
    }

    written / size
}

#[no_mangle]
pub unsafe extern "C" fn fread(
    ptr: *mut c_void,
    size: usize,
    nmemb: usize,
    stream: *mut FILE,
) -> usize {
    if size == 0 || nmemb == 0 {
        return 0;
    }
    if ptr.is_null() {
        set_errno(EINVAL);
        return 0;
    }

    let fd = match stream_fd(stream) {
        Ok(fd) => fd,
        Err(()) => return 0,
    };

    let total = match size.checked_mul(nmemb) {
        Some(v) => v,
        None => {
            set_errno(EINVAL);
            return 0;
        }
    };

    set_errno(0);
    let mut read_total = 0usize;
    while read_total < total {
        let chunk = read_fd(fd, (ptr as *mut u8).add(read_total), total - read_total);
        if chunk <= 0 {
            if chunk < 0 {
                // errno already set by read_fd
                return read_total / size;
            }
            break;
        }
        read_total += chunk as usize;
        if chunk as usize == 0 {
            break;
        }
    }

    read_total / size
}

fn itoa_unsigned(mut n: u64, buf: &mut [u8; 32]) -> &str {
    if n == 0 {
        buf[0] = b'0';
        return unsafe { core::str::from_utf8_unchecked(&buf[..1]) };
    }
    let mut i = 0usize;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    for j in 0..i / 2 {
        buf.swap(j, i - 1 - j);
    }
    unsafe { core::str::from_utf8_unchecked(&buf[..i]) }
}

fn itoa_signed(n: i64, buf: &mut [u8; 32]) -> &str {
    if n == 0 {
        buf[0] = b'0';
        return unsafe { core::str::from_utf8_unchecked(&buf[..1]) };
    }
    if n < 0 {
        let mut tmp = (-n) as u64;
        let mut i = 0usize;
        while tmp > 0 {
            buf[i + 1] = b'0' + (tmp % 10) as u8;
            tmp /= 10;
            i += 1;
        }
        for j in 0..i / 2 {
            buf.swap(j + 1, i - j);
        }
        buf[0] = b'-';
        unsafe { core::str::from_utf8_unchecked(&buf[..i + 1]) }
    } else {
        itoa_unsigned(n as u64, buf)
    }
}

fn itoa_hex(mut n: u64, buf: &mut [u8; 32]) -> &str {
    if n == 0 {
        buf[0] = b'0';
        return unsafe { core::str::from_utf8_unchecked(&buf[..1]) };
    }
    let mut i = 0usize;
    while n > 0 {
        let d = (n & 0xF) as u8;
        buf[i] = if d < 10 { b'0' + d } else { b'a' + (d - 10) };
        n >>= 4;
        i += 1;
    }
    for j in 0..i / 2 {
        buf.swap(j, i - 1 - j);
    }
    unsafe { core::str::from_utf8_unchecked(&buf[..i]) }
}
