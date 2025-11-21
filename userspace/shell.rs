use std::{arch::asm, cell::UnsafeCell, panic};
use core::matches;
use core::marker::Sync;
use core::marker::Copy;
use core::result::Result::Err;
use core::result::Result::Ok;
use core::option::Option::Some;
use core::option::Option::None;
use core::option::Option;
use core::prelude::rust_2024::derive;
use core::iter::Iterator;
use core::clone::Clone;

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_STAT: u64 = 4;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;
const SYS_LIST_FILES: u64 = 200;
const SYS_GETERRNO: u64 = 201;
const SYS_IPC_CREATE: u64 = 210;
const SYS_IPC_SEND: u64 = 211;
const SYS_IPC_RECV: u64 = 212;
const SYS_USER_ADD: u64 = 220;
const SYS_USER_LOGIN: u64 = 221;
const SYS_USER_INFO: u64 = 222;
const SYS_USER_LIST: u64 = 223;
const SYS_USER_LOGOUT: u64 = 224;

const LIST_FLAG_INCLUDE_HIDDEN: u64 = 0x1;
const USER_FLAG_ADMIN: u64 = 0x1;

const HOSTNAME: &str = "nexa";
const MAX_PATH: usize = 256;
const PRINT_SCRATCH_SIZE: usize = 128;

struct ScratchBuffer<const N: usize> {
    inner: UnsafeCell<[u8; N]>,
}

impl<const N: usize> ScratchBuffer<N> {
    const fn new() -> Self {
        Self {
            inner: UnsafeCell::new([0; N]),
        }
    }

    unsafe fn get(&self) -> &mut [u8; N] {
        &mut *self.inner.get()
    }
}

unsafe impl<const N: usize> Sync for ScratchBuffer<N> {}

static PRINT_SCRATCH: ScratchBuffer<PRINT_SCRATCH_SIZE> = ScratchBuffer::new();

fn install_panic_hook() {
    panic::set_hook(Box::new(|_info| {
        exit(1);
    }));
}

fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    // Route all syscalls via int 0x81 so the CPU saves/restores SS:RSP for Ring3 safely.
    // Kernel handler expects: rax=nr, rdi=arg1, rsi=arg2, rdx=arg3 (SysV order).
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
            clobber_abi("sysv64")
        );
    }
    ret
}

fn syscall0(n: u64) -> u64 { syscall3(n, 0, 0, 0) }
fn syscall1(n: u64, a1: u64) -> u64 { syscall3(n, a1, 0, 0) }

#[repr(C)]
#[derive(Clone, Copy)]
struct Stat {
    st_dev: u64,
    st_ino: u64,
    st_mode: u32,
    st_nlink: u32,
    st_uid: u32,
    st_gid: u32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atime: i64,
    st_atime_nsec: i64,
    st_mtime: i64,
    st_mtime_nsec: i64,
    st_ctime: i64,
    st_ctime_nsec: i64,
    st_reserved: [i64; 3],
}

impl Stat {
    const fn zero() -> Self {
        Self {
            st_dev: 0,
            st_ino: 0,
            st_mode: 0,
            st_nlink: 0,
            st_uid: 0,
            st_gid: 0,
            st_rdev: 0,
            st_size: 0,
            st_blksize: 0,
            st_blocks: 0,
            st_atime: 0,
            st_atime_nsec: 0,
            st_mtime: 0,
            st_mtime_nsec: 0,
            st_ctime: 0,
            st_ctime_nsec: 0,
            st_reserved: [0; 3],
        }
    }
}

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
#[derive(Clone, Copy)]
struct UserInfo {
    username: [u8; 32],
    username_len: u64,
    uid: u32,
    gid: u32,
    is_admin: u32,
}

impl UserInfo {
    const fn zero() -> Self {
        Self {
            username: [0; 32],
            username_len: 0,
            uid: 0,
            gid: 0,
            is_admin: 0,
        }
    }
}

struct ShellState {
    path: [u8; MAX_PATH],
    len: usize,
}

impl ShellState {
    fn new() -> Self {
        let mut path = [0u8; MAX_PATH];
        path[0] = b'/';
        Self { path, len: 1 }
    }

    fn current_path(&self) -> &str {
        core::str::from_utf8(&self.path[..self.len]).unwrap_or("/")
    }

    fn set_path(&mut self, path: &str) {
        let bytes = path.as_bytes();
        let mut len = core::cmp::min(bytes.len(), MAX_PATH);
        while len > 1 && bytes[len - 1] == b'/' {
            len -= 1;
        }
        if len == 0 {
            self.path[0] = b'/';
            self.len = 1;
        } else {
            self.path[..len].copy_from_slice(&bytes[..len]);
            self.len = len;
        }
    }

    fn resolve<'a>(&self, input: &str, out: &'a mut [u8]) -> Option<&'a str> {
        normalize_path(self.current_path(), input, out)
    }
}

const COMMANDS: [&str; 19] = [
    "help",
    "ls",
    "cat",
    "stat",
    "pwd",
    "cd",
    "echo",
    "uname",
    "mkdir",
    "login",
    "whoami",
    "users",
    "logout",
    "adduser",
    "ipc-create",
    "ipc-send",
    "ipc-recv",
    "clear",
    "exit",
];

const MAX_COMPLETIONS: usize = 32;
const COMPLETION_BUFFER_SIZE: usize = 2048;

fn trim_line<'a>(buf: &'a [u8], mut len: usize) -> &'a str {
    while len > 0 && matches!(buf[len - 1], b'\n' | b'\r' | 0) {
        len -= 1;
    }
    core::str::from_utf8(&buf[..len]).unwrap_or("")
}

fn normalize_path<'a>(base: &str, input: &str, out: &'a mut [u8]) -> Option<&'a str> {
    if out.is_empty() {
        return None;
    }

    if input.is_empty() {
        let bytes = base.as_bytes();
        if bytes.len() > out.len() {
            return None;
        }
        out[..bytes.len()].copy_from_slice(bytes);
        return core::str::from_utf8(&out[..bytes.len()]).ok();
    }

    let mut path_len;
    let mut remaining = input;

    if remaining.starts_with('/') {
        out[0] = b'/';
        path_len = 1;
        remaining = remaining.trim_start_matches('/');
    } else {
        let base_bytes = base.as_bytes();
        if base_bytes.len() > out.len() {
            return None;
        }
        path_len = base_bytes.len();
        out[..path_len].copy_from_slice(base_bytes);
    }

    for part in remaining.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if path_len > 1 {
                if out[path_len - 1] == b'/' && path_len > 1 {
                    path_len -= 1;
                }
                while path_len > 0 && out[path_len - 1] != b'/' {
                    path_len -= 1;
                }
                if path_len == 0 {
                    out[0] = b'/';
                    path_len = 1;
                }
            }
            continue;
        }

        if path_len > 1 && out[path_len - 1] != b'/' {
            if path_len >= out.len() {
                return None;
            }
            out[path_len] = b'/';
            path_len += 1;
        } else if path_len == 0 {
            out[path_len] = b'/';
            path_len += 1;
        }

        let bytes = part.as_bytes();
        if path_len + bytes.len() > out.len() {
            return None;
        }
        out[path_len..path_len + bytes.len()].copy_from_slice(bytes);
        path_len += bytes.len();
    }

    if path_len > 1 && out[path_len - 1] == b'/' {
        path_len -= 1;
    }

    if path_len == 0 {
        out[0] = b'/';
        path_len = 1;
    }

    core::str::from_utf8(&out[..path_len]).ok()
}

fn is_directory(mode: u32) -> bool {
    (mode & 0o170000) == 0o040000
}

#[repr(C)]
struct IpcTransferRequest {
    channel_id: u32,
    flags: u32,
    buffer_ptr: u64,
    buffer_len: u64,
}

fn errno() -> i32 {
    syscall1(SYS_GETERRNO, 0) as i32
}

fn write(fd: u64, buf: *const u8, count: usize) {
    syscall3(SYS_WRITE, fd, buf as u64, count as u64);
}

fn read(fd: u64, buf: *mut u8, count: usize) -> usize {
    syscall3(SYS_READ, fd, buf as u64, count as u64) as usize
}

fn exit(code: i32) {
    syscall1(SYS_EXIT, code as u64);
    loop {} // Should not reach here
}

fn fork() -> i32 {
    syscall0(SYS_FORK) as i32
}

fn execve(path: *const u8, argv: *const *const u8, envp: *const *const u8) -> i32 {
    syscall3(SYS_EXECVE, path as u64, argv as u64, envp as u64) as i32
}

fn wait4(pid: i32, status: *mut i32, options: i32) -> i32 {
    syscall3(SYS_WAIT4, pid as u64, status as u64, options as u64) as i32
}

// POSIX wait status macros
fn wexitstatus(status: i32) -> i32 {
    (status >> 8) & 0xff
}

fn wifexited(status: i32) -> bool {
    (status & 0x7f) == 0
}

fn wifsignaled(status: i32) -> bool {
    ((status & 0x7f) + 1) as i8 >= 2
}

fn wtermsig(status: i32) -> i32 {
    status & 0x7f
}

fn print_hex(val: u64) {
    let hex_chars = b"0123456789abcdef";
    let mut buf = [0u8; 16];
    for i in 0..16 {
        let nibble = ((val >> (60 - i * 4)) & 0xf) as usize;
        buf[i] = hex_chars[nibble];
    }
    write(1, buf.as_ptr(), buf.len());
}

fn print_bytes(bytes: &[u8]) {
    const USER_BASE: u64 = 0x400000;
    const USER_END: u64 = 0xA00000; // exclusive upper bound

    if bytes.is_empty() {
        return;
    }

    let ptr = bytes.as_ptr() as u64;
    let len = bytes.len() as u64;

    let in_user_range = ptr >= USER_BASE
        && ptr.checked_add(len).map_or(false, |end| end <= USER_END);

    if in_user_range {
        write(1, bytes.as_ptr(), bytes.len());
        return;
    }

    let mut offset = 0usize;
    while offset < bytes.len() {
        let chunk = core::cmp::min(PRINT_SCRATCH_SIZE, bytes.len() - offset);
        unsafe {
            let scratch = PRINT_SCRATCH.get();
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr().add(offset),
                scratch.as_mut_ptr(),
                chunk,
            );
            write(1, scratch.as_ptr(), chunk);
        }
        offset += chunk;
    }
}

fn print_str(s: &str) {
    print_bytes(s.as_bytes());
}

fn println_str(s: &str) {
    print_str(s);
    print_bytes(b"\n");
}

fn print_u64(mut value: u64) {
    if value == 0 {
        print_bytes(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut idx = 0;
    while value > 0 {
        buf[idx] = b'0' + (value % 10) as u8;
        value /= 10;
        idx += 1;
    }
    while idx > 0 {
        idx -= 1;
        print_bytes(&buf[idx..idx + 1]);
    }
}

fn print_i64(value: i64) {
    if value < 0 {
        print_bytes(b"-");
        print_u64((-value) as u64);
    } else {
        print_u64(value as u64);
    }
}

fn print_i32(value: i32) {
    if value < 0 {
        print_bytes(b"-");
        print_u64((-value) as u64);
    } else {
        print_u64(value as u64);
    }
}

fn print_octal(mut value: u32) {
    let mut buf = [0u8; 12];
    let mut idx = 0;
    if value == 0 {
        print_bytes(b"0");
        return;
    }
    while value > 0 {
        buf[idx] = b'0' + (value & 0x7) as u8;
        value >>= 3;
        idx += 1;
    }
    while idx > 0 {
        idx -= 1;
        print_bytes(&buf[idx..idx + 1]);
    }
}

fn println_errno(err: i32) {
    print_str("errno: ");
    print_i64(err as i64);
    println_str("");
}

fn fetch_stat(path: &str, out: &mut Stat) -> bool {
    if path.is_empty() {
        return false;
    }
    let ret = syscall3(
        SYS_STAT,
        path.as_ptr() as u64,
        path.len() as u64,
        out as *mut Stat as u64,
    );
    ret != u64::MAX
}

fn format_child_path<'a>(base: &'a str, child: &'a str, out: &'a mut [u8]) -> Option<&'a str> {
    if child.is_empty() {
        return None;
    }
    normalize_path(base, child, out)
}

fn refresh_current_user(info: &mut UserInfo) -> bool {
    let ret = syscall3(
        SYS_USER_INFO,
        info as *mut UserInfo as u64,
        0,
        0,
    );
    ret != u64::MAX
}

fn print_mode_short(mode: u32) {
    let file_type = match mode & 0o170000 {
        0o040000 => b'd',
        0o100000 => b'-',
        0o120000 => b'l',
        0o020000 => b'c',
        0o060000 => b'b',
        0o010000 => b'p',
        0o140000 => b's',
        _ => b'?',
    };
    let mut buf = [0u8; 10];
    buf[0] = file_type;
    let perms = [
        (mode >> 6) & 0o7,
        (mode >> 3) & 0o7,
        mode & 0o7,
    ];
    for i in 0..3 {
        let p = perms[i as usize];
        buf[1 + i * 3] = if (p & 0o4) != 0 { b'r' } else { b'-' };
        buf[2 + i * 3] = if (p & 0o2) != 0 { b'w' } else { b'-' };
        buf[3 + i * 3] = if (p & 0o1) != 0 { b'x' } else { b'-' };
    }
    print_bytes(&buf);
}

fn report_stdin_error(err: i32) {
    print_str("stdin read failed (errno ");
    print_i64(err as i64);
    println_str(")");
}

fn read_line_raw(buf: &mut [u8]) -> usize {
    loop {
        let len = read(0, buf.as_mut_ptr(), buf.len());
        if len == usize::MAX {
            let err = errno();
            if err != 0 {
                report_stdin_error(err);
            }
            continue;
        }

        if len == 0 {
            let err = errno();
            if err != 0 {
                report_stdin_error(err);
            }
            return 0;
        }

        let mut end = len;
        while end > 0 && (buf[end - 1] == b'\n' || buf[end - 1] == b'\r') {
            end -= 1;
        }
        return end;
    }
}

fn append_to_buffer(buf: &mut [u8], len: &mut usize, text: &[u8]) -> bool {
    if *len + text.len() > buf.len() {
        return false;
    }
    buf[*len..*len + text.len()].copy_from_slice(text);
    *len += text.len();
    true
}

fn beep() {
    print_bytes(b"\x07");
}

fn erase_last_char(buf: &mut [u8], len: &mut usize) -> bool {
    if *len == 0 {
        return false;
    }
    *len -= 1;
    buf[*len] = 0;
    print_bytes(b"\x08 \x08");
    true
}

fn erase_last_word(buf: &mut [u8], len: &mut usize) {
    while *len > 0 && buf[*len - 1].is_ascii_whitespace() {
        if !erase_last_char(buf, len) {
            return;
        }
    }
    while *len > 0 && !buf[*len - 1].is_ascii_whitespace() {
        if !erase_last_char(buf, len) {
            break;
        }
    }
}

fn longest_common_prefix<'a>(items: &[&'a str]) -> &'a str {
    if items.is_empty() {
        return "";
    }
    let first = items[0].as_bytes();
    let mut prefix_len = first.len();
    for item in &items[1..] {
        let bytes = item.as_bytes();
        let mut i = 0;
        let limit = core::cmp::min(prefix_len, bytes.len());
        while i < limit && first[i] == bytes[i] {
            i += 1;
        }
        prefix_len = i;
        if prefix_len == 0 {
            break;
        }
    }
    core::str::from_utf8(&first[..prefix_len]).unwrap_or("")
}

fn command_accepts_path(command: &str, token_index: usize) -> bool {
    if token_index == 0 {
        return false;
    }
    matches!(command, "ls" | "cat" | "stat" | "cd" | "mkdir")
}

fn complete_commands(state: &ShellState, buffer: &mut [u8], len: &mut usize, prefix: &str) {
    let mut matches = [""; COMMANDS.len()];
    let mut count = 0usize;

    for &cmd in COMMANDS.iter() {
        if cmd.starts_with(prefix) {
            if count < matches.len() {
                matches[count] = cmd;
                count += 1;
            }
        }
    }

    if count == 0 {
        beep();
        return;
    }

    let prefix_len = prefix.len();

    if count == 1 {
        let candidate = matches[0];
        let addition = &candidate.as_bytes()[prefix_len..];
        if append_to_buffer(buffer, len, addition) {
            print_bytes(addition);
            if append_to_buffer(buffer, len, b" ") {
                print_bytes(b" ");
            }
        } else {
            beep();
        }
        return;
    }

    let lcp = longest_common_prefix(&matches[..count]);
    if lcp.len() > prefix_len {
        let addition = &lcp.as_bytes()[prefix_len..];
        if append_to_buffer(buffer, len, addition) {
            print_bytes(addition);
            return;
        }
        beep();
        return;
    }

    print_bytes(b"\n");
    for idx in 0..count {
        println_str(matches[idx]);
    }
    prompt(state);
    print_bytes(&buffer[..*len]);
}

fn complete_path(state: &ShellState, buffer: &mut [u8], len: &mut usize, prefix: &str) {
    let (dir_input, name_prefix) = match prefix.rfind('/') {
        Some(idx) => (&prefix[..=idx], &prefix[idx + 1..]),
        None => ("", prefix),
    };

    let mut resolved_dir_buf = [0u8; MAX_PATH];
    let directory = if prefix.starts_with('/') {
        if dir_input.is_empty() {
            "/"
        } else if let Some(abs) = normalize_path("/", dir_input, &mut resolved_dir_buf) {
            abs
        } else {
            beep();
            return;
        }
    } else if dir_input.is_empty() {
        state.current_path()
    } else if let Some(abs) = state.resolve(dir_input, &mut resolved_dir_buf) {
        abs
    } else {
        beep();
        return;
    };

    let show_hidden = name_prefix.starts_with('.');
    let mut request = ListDirRequest {
        path_ptr: 0,
        path_len: 0,
        flags: 0,
    };

    if show_hidden {
        request.flags |= LIST_FLAG_INCLUDE_HIDDEN;
    }

    if directory != "/" {
        request.path_ptr = directory.as_ptr() as u64;
        request.path_len = directory.len() as u64;
    }

    let req_ptr = if request.path_len == 0 && request.flags == 0 {
        0
    } else {
        &request as *const ListDirRequest as u64
    };

    let mut list_buf = [0u8; COMPLETION_BUFFER_SIZE];
    let written = syscall3(
        SYS_LIST_FILES,
        list_buf.as_mut_ptr() as u64,
        list_buf.len() as u64,
        req_ptr,
    );
    if written == u64::MAX {
        beep();
        return;
    }

    let list_len = written as usize;
    let list = match core::str::from_utf8(&list_buf[..list_len]) {
        Ok(text) => text,
        Err(_) => {
            beep();
            return;
        }
    };

    let mut matches = [""; MAX_COMPLETIONS];
    let mut match_is_dir = [false; MAX_COMPLETIONS];
    let mut count = 0usize;
    let mut path_buf = [0u8; MAX_PATH];

    for entry in list.lines() {
        if entry.is_empty() {
            continue;
        }
        if !show_hidden && entry.starts_with('.') {
            continue;
        }
        if name_prefix.is_empty() && (entry == "." || entry == "..") {
            continue;
        }
        if entry.starts_with(name_prefix) {
            if count < matches.len() {
                matches[count] = entry;
                if let Some(full_path) = format_child_path(directory, entry, &mut path_buf) {
                    let mut stat = Stat::zero();
                    if fetch_stat(full_path, &mut stat) && is_directory(stat.st_mode) {
                        match_is_dir[count] = true;
                    }
                }
                count += 1;
            }
        }
    }

    if count == 0 {
        beep();
        return;
    }

    let prefix_len = name_prefix.len();

    if count == 1 {
        let candidate = matches[0];
        let addition = &candidate.as_bytes()[prefix_len..];
        if !append_to_buffer(buffer, len, addition) {
            beep();
            return;
        }
        print_bytes(addition);
        if match_is_dir[0] {
            if append_to_buffer(buffer, len, b"/") {
                print_bytes(b"/");
            }
        } else if append_to_buffer(buffer, len, b" ") {
            print_bytes(b" ");
        }
        return;
    }

    let lcp = longest_common_prefix(&matches[..count]);
    let mut appended = false;
    if lcp.len() > name_prefix.len() {
        let addition = &lcp.as_bytes()[name_prefix.len()..];
        if append_to_buffer(buffer, len, addition) {
            print_bytes(addition);
            appended = true;
        } else {
            beep();
            return;
        }
    }

    if !appended {
        print_bytes(b"\n");
        for idx in 0..count {
            print_str(dir_input);
            print_str(matches[idx]);
            if match_is_dir[idx] {
                print_bytes(b"/");
            }
            print_bytes(b"\n");
        }
        prompt(state);
        print_bytes(&buffer[..*len]);
    }
}

fn handle_tab_completion(state: &ShellState, buffer: &mut [u8], len: &mut usize) {
    if core::str::from_utf8(&buffer[..*len]).is_err() {
        beep();
        return;
    }

    let mut token_end = *len;
    while token_end > 0 && buffer[token_end - 1].is_ascii_whitespace() {
        token_end -= 1;
    }
    let has_trailing_whitespace = token_end != *len;

    let mut token_start = token_end;
    while token_start > 0 && !buffer[token_start - 1].is_ascii_whitespace() {
        token_start -= 1;
    }

    if has_trailing_whitespace {
        token_start = *len;
        token_end = *len;
    }

    let prefix_len = token_end - token_start;
    if prefix_len > MAX_PATH {
        beep();
        return;
    }

    let mut prefix_buf = [0u8; MAX_PATH];
    if prefix_len > 0 {
        prefix_buf[..prefix_len].copy_from_slice(&buffer[token_start..token_end]);
    }
    let prefix = match core::str::from_utf8(&prefix_buf[..prefix_len]) {
        Ok(p) => p,
        Err(_) => {
            beep();
            return;
        }
    };

    let mut token_index = 0usize;
    let mut i = 0usize;
    while i < token_start {
        while i < token_start && buffer[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= token_start {
            break;
        }
        token_index += 1;
        while i < token_start && !buffer[i].is_ascii_whitespace() {
            i += 1;
        }
    }

    let mut command_buf = [0u8; MAX_PATH];
    let mut command_len = 0usize;
    let mut j = 0usize;
    while j < *len && buffer[j].is_ascii_whitespace() {
        j += 1;
    }
    let command_start = j;
    while j < *len && !buffer[j].is_ascii_whitespace() {
        j += 1;
    }
    if j > command_start {
        command_len = j - command_start;
        if command_len > MAX_PATH {
            beep();
            return;
        }
        command_buf[..command_len].copy_from_slice(&buffer[command_start..j]);
    }

    if token_index == 0 {
        complete_commands(state, buffer, len, prefix);
        return;
    }

    if command_len == 0 {
        beep();
        return;
    }

    let command = match core::str::from_utf8(&command_buf[..command_len]) {
        Ok(c) => c,
        Err(_) => {
            beep();
            return;
        }
    };

    if command_accepts_path(command, token_index) {
        complete_path(state, buffer, len, prefix);
    } else {
        beep();
    }
}

fn discard_escape_sequence() {
    let mut consumed = 0;
    loop {
        let mut ch = 0u8;
        let read_len = read(0, &mut ch as *mut u8, 1);
        if read_len != 1 {
            break;
        }
        consumed += 1;
        if (0x40..=0x7e).contains(&ch) || consumed >= 4 {
            break;
        }
    }
}

fn read_byte_blocking() -> Option<u8> {
    loop {
        let mut ch = 0u8;
        let read_len = read(0, &mut ch as *mut u8, 1);
        if read_len == usize::MAX {
            let err = errno();
            if err != 0 {
                report_stdin_error(err);
                continue;
            }
            return None;
        }
        if read_len == 0 {
            continue;
        }
        return Some(ch);
    }
}

fn read_line(state: &ShellState, buf: &mut [u8]) -> usize {
    prompt(state);
    let mut len = 0usize;

    loop {
        let Some(ch) = read_byte_blocking() else {
            continue;
        };

        match ch {
            b'\r' | b'\n' => {
                print_bytes(b"\n");
                return len;
            }
            0x03 => {
                for idx in 0..len {
                    buf[idx] = 0;
                }
                print_bytes(b"^C\n");
                return 0;
            }
            0x04 => {
                if len == 0 {
                    println_str("exit");
                    exit(0);
                } else {
                    beep();
                }
            }
            0x08 | 0x7f => {
                if !erase_last_char(buf, &mut len) {
                    beep();
                }
            }
            b'\t' => {
                handle_tab_completion(state, buf, &mut len);
            }
            0x15 => {
                while erase_last_char(buf, &mut len) {}
            }
            0x17 => {
                erase_last_word(buf, &mut len);
            }
            0x0c => {
                clear_screen();
                prompt(state);
                if len > 0 {
                    print_bytes(&buf[..len]);
                }
            }
            0x1b => {
                discard_escape_sequence();
            }
            ch if ch < 0x20 => {
                beep();
            }
            _ => {
                if len < buf.len() {
                    buf[len] = ch;
                    len += 1;
                    print_bytes(&[ch]);
                } else {
                    beep();
                }
            }
        }
    }
}

fn open_file(path: &str) -> Option<u64> {
    let fd = syscall3(SYS_OPEN, path.as_ptr() as u64, path.len() as u64, 0);
    if fd == u64::MAX {
        None
    } else {
        Some(fd)
    }
}

fn parse_u32(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    for &b in value.as_bytes() {
        if b < b'0' || b > b'9' {
            return None;
        }
        acc = acc.checked_mul(10)?;
        acc = acc.checked_add((b - b'0') as u32)?;
    }
    Some(acc)
}

fn close_file(fd: u64) {
    syscall3(SYS_CLOSE, fd, 0, 0);
}

fn list_directory_entries(state: &ShellState, path: &str, show_all: bool, long_format: bool) {
    let mut resolved_buf = [0u8; MAX_PATH];
    let effective = if path.is_empty() {
        state.current_path()
    } else {
        match state.resolve(path, &mut resolved_buf) {
            Some(p) => p,
            None => {
                println_str("ls: invalid path");
                return;
            }
        }
    };
    let mut request = ListDirRequest {
        path_ptr: 0,
        path_len: 0,
        flags: 0,
    };

    if show_all {
        request.flags |= LIST_FLAG_INCLUDE_HIDDEN;
    }

    if effective != "/" {
        request.path_ptr = effective.as_ptr() as u64;
        request.path_len = effective.len() as u64;
    }

    let req_ptr = if request.path_len == 0 && request.flags == 0 {
        0
    } else {
        &request as *const ListDirRequest as u64
    };

    let mut buf = [0u8; 1024];
    let written = syscall3(SYS_LIST_FILES, buf.as_mut_ptr() as u64, buf.len() as u64, req_ptr);
    if written == u64::MAX {
        println_str("ls: failed to read directory");
        println_errno(errno());
        return;
    }

    let len = written as usize;
    if len == 0 {
        println_str("(empty)");
        return;
    }

    if let Ok(list) = core::str::from_utf8(&buf[..len]) {
        let mut path_buf = [0u8; MAX_PATH];
        for entry in list.lines() {
            if entry.is_empty() {
                continue;
            }
            if !show_all && entry.starts_with('.') {
                continue;
            }

            if long_format {
                if let Some(full_path) = format_child_path(effective, entry, &mut path_buf) {
                    let mut stat = Stat::zero();
                    if fetch_stat(full_path, &mut stat) {
                        print_mode_short(stat.st_mode);
                        print_bytes(b" ");
                        print_u64(stat.st_uid as u64);
                        print_bytes(b" ");
                        print_u64(stat.st_gid as u64);
                        print_bytes(b" ");
                        print_i64(stat.st_size);
                        print_bytes(b" ");
                        println_str(entry);
                        continue;
                    }
                }
            }

            println_str(entry);
        }
    } else {
        println_str("ls: kernel returned invalid UTF-8");
    }
}

fn stat_path(state: &ShellState, path: &str) {
    let mut buf = [0u8; MAX_PATH];
    let Some(full_path) = state.resolve(path, &mut buf) else {
        println_str("stat: invalid path");
        return;
    };

    let mut stat = Stat::zero();
    if !fetch_stat(full_path, &mut stat) {
        println_str("stat: failed");
        println_errno(errno());
        return;
    }

    println_str("File statistics:");
    print_str("  size: ");
    print_i64(stat.st_size);
    println_str(" bytes");

    print_str("  blocks: ");
    print_i64(stat.st_blocks);
    println_str("");

    print_str("  mode: 0o");
    print_octal(stat.st_mode as u32);
    println_str("");

    print_str("  links: ");
    print_u64(stat.st_nlink as u64);
    println_str("");
}

fn login_user(username: &str) {
    if username.is_empty() {
        println_str("login: missing user name");
        return;
    }

    print_str("password: ");
    let mut buffer = [0u8; 64];
    let len = read_line_raw(&mut buffer);
    let password = trim_line(&buffer, len);

    let request = UserRequest {
        username_ptr: username.as_ptr() as u64,
        username_len: username.len() as u64,
        password_ptr: password.as_ptr() as u64,
        password_len: password.len() as u64,
        flags: 0,
    };

    let ret = syscall3(SYS_USER_LOGIN, &request as *const UserRequest as u64, 0, 0);
    if ret == u64::MAX {
        println_str("login failed");
        println_errno(errno());
    } else {
        println_str("login successful");
    }
}

fn add_user(username: &str, admin: bool) {
    if username.is_empty() {
        println_str("adduser: missing user name");
        return;
    }
    print_str("new password: ");
    let mut buffer = [0u8; 64];
    let len = read_line_raw(&mut buffer);
    let password = trim_line(&buffer, len);

    let request = UserRequest {
        username_ptr: username.as_ptr() as u64,
        username_len: username.len() as u64,
        password_ptr: password.as_ptr() as u64,
        password_len: password.len() as u64,
        flags: if admin { USER_FLAG_ADMIN } else { 0 },
    };

    let ret = syscall3(SYS_USER_ADD, &request as *const UserRequest as u64, 0, 0);
    if ret == u64::MAX {
        println_str("adduser: failed");
        println_errno(errno());
    } else {
        println_str("adduser: user created");
    }
}

fn whoami() {
    let mut info = UserInfo::zero();
    if refresh_current_user(&mut info) {
        let len = info.username_len as usize;
        if len == 0 {
            println_str("(anonymous)");
        } else if let Ok(name) = core::str::from_utf8(&info.username[..len]) {
            println_str(name);
        } else {
            println_str("(invalid username)");
        }
    } else {
        println_str("whoami: failed");
        println_errno(errno());
    }
}

fn list_users() {
    let mut buffer = [0u8; 512];
    let written = syscall3(
        SYS_USER_LIST,
        buffer.as_mut_ptr() as u64,
        buffer.len() as u64,
        0,
    );
    if written == u64::MAX {
        println_str("users: failed");
        println_errno(errno());
        return;
    }

    let len = written as usize;
    if len == 0 {
        println_str("(no users)");
        return;
    }

    if let Ok(text) = core::str::from_utf8(&buffer[..len]) {
        print_str(text);
    } else {
        println_str("users: invalid data");
    }
}

fn logout_user() {
    let ret = syscall1(SYS_USER_LOGOUT, 0);
    if ret == u64::MAX {
        println_str("logout: failed");
        println_errno(errno());
    } else {
        println_str("logged out");
    }
}

fn ipc_create_channel() {
    let id = syscall3(SYS_IPC_CREATE, 0, 0, 0);
    if id == u64::MAX {
        println_str("ipc-create: failed");
        println_errno(errno());
    } else {
        print_str("channel ");
        print_u64(id);
        println_str(" created");
    }
}

fn ipc_send_message(channel: u32, message: &str) {
    if message.is_empty() {
        println_str("ipc-send: message cannot be empty");
        return;
    }
    let request = IpcTransferRequest {
        channel_id: channel,
        flags: 0,
        buffer_ptr: message.as_ptr() as u64,
        buffer_len: message.len() as u64,
    };

    let ret = syscall3(SYS_IPC_SEND, &request as *const IpcTransferRequest as u64, 0, 0);
    if ret == u64::MAX {
        println_str("ipc-send: failed");
        println_errno(errno());
    } else {
        println_str("ipc-send: message queued");
    }
}

fn ipc_receive_message(channel: u32) {
    let mut buffer = [0u8; 256];
    let request = IpcTransferRequest {
        channel_id: channel,
        flags: 0,
        buffer_ptr: buffer.as_mut_ptr() as u64,
        buffer_len: buffer.len() as u64,
    };

    let ret = syscall3(SYS_IPC_RECV, &request as *const IpcTransferRequest as u64, 0, 0);
    if ret == u64::MAX {
        println_str("ipc-recv: failed");
        println_errno(errno());
        return;
    }

    let len = ret as usize;
    if let Ok(text) = core::str::from_utf8(&buffer[..len]) {
        print_str("ipc-recv: ");
        println_str(text);
    } else {
        println_str("ipc-recv: <binary data>");
    }
}

fn cat(state: &ShellState, path: &str) {
    let mut buf = [0u8; MAX_PATH];
    let Some(full_path) = state.resolve(path, &mut buf) else {
        println_str("cat: invalid path");
        return;
    };

    if let Some(fd) = open_file(full_path) {
        let mut chunk = [0u8; 256];
        loop {
            let read = syscall3(SYS_READ, fd, chunk.as_mut_ptr() as u64, chunk.len() as u64) as usize;
            if read == 0 {
                break;
            }
            print_bytes(&chunk[..read]);
        }
        close_file(fd);
        print_bytes(b"\n");
    } else {
        println_str("cat: file not found");
        println_errno(errno());
    }
}

fn show_help() {
    println_str("Available commands:");
    println_str("  help              Show this message");
    println_str("  ls [-a] [-l] [p]  List directory contents");
    println_str("  cat <file>        Print file contents");
    println_str("  stat <file>       Show file metadata");
    println_str("  pwd               Print working directory");
    println_str("  cd <path>         Change directory");
    println_str("  echo [text...]    Print text to output");
    println_str("  uname [-a]        Show system information");
    println_str("  mkdir <path>      Create directory (stub)");
    println_str("  login <user>      Switch active user");
    println_str("  whoami            Show current user");
    println_str("  users             List registered users");
    println_str("  logout            Log out current user");
    println_str("  adduser [-a] <u>  Create a new user (-a for admin)");
    println_str("  ipc-create        Allocate IPC channel");
    println_str("  ipc-send <c> <m>  Send IPC message");
    println_str("  ipc-recv <c>      Receive IPC message");
    println_str("  clear             Clear the screen");
    println_str("  exit              Exit the shell");
    println_str("");
    println_str("Editing keys:");
    println_str("  Tab               Complete command/path");
    println_str("  Backspace         Delete character");
    println_str("  Ctrl-C            Cancel line");
    println_str("  Ctrl-D            Exit (on empty line)");
    println_str("  Ctrl-U            Clear line");
    println_str("  Ctrl-W            Delete word");
    println_str("  Ctrl-L            Refresh screen");
}

fn clear_screen() {
    print_bytes(b"\x1b[2J\x1b[H");
}

fn show_uname(all: bool) {
    if all {
        println_str("NexaOS 0.1.0 x86_64 (experimental hybrid kernel)");
    } else {
        println_str("NexaOS");
    }
}

fn cmd_pwd(state: &ShellState) {
    println_str(state.current_path());
}

fn cmd_cd(state: &mut ShellState, path: &str) {
    if path.is_empty() {
        state.set_path("/");
        return;
    }

    let mut buf = [0u8; MAX_PATH];
    let Some(resolved) = state.resolve(path, &mut buf) else {
        println_str("cd: invalid path");
        return;
    };

    let mut stat = Stat::zero();
    if !fetch_stat(resolved, &mut stat) {
        println_str("cd: path not found");
        return;
    }

    if !is_directory(stat.st_mode) {
        println_str("cd: not a directory");
        return;
    }

    state.set_path(resolved);
}

fn cmd_echo(args: &str) {
    println_str(args);
}

fn cmd_mkdir(state: &ShellState, path: &str) {
    if path.is_empty() {
        println_str("mkdir: missing operand");
        return;
    }

    let mut buf = [0u8; MAX_PATH];
    let Some(_resolved) = state.resolve(path, &mut buf) else {
        println_str("mkdir: invalid path");
        return;
    };

    println_str("mkdir: not yet implemented (filesystem is read-only)");
}

// Helper function to check if a file exists and is executable
fn file_exists(path: &str) -> bool {
    let mut path_bytes = [0u8; MAX_PATH];
    let bytes = path.as_bytes();
    if bytes.len() >= MAX_PATH {
        return false;
    }
    path_bytes[..bytes.len()].copy_from_slice(bytes);
    path_bytes[bytes.len()] = 0; // null terminate

    let mut stat_buf = Stat::zero();
    let result = syscall3(
        SYS_STAT,
        path_bytes.as_ptr() as u64,
        bytes.len() as u64,
        &mut stat_buf as *mut Stat as u64,
    );
    result == 0
}

// Search for an executable in standard paths
fn find_executable(cmd: &str) -> Option<[u8; MAX_PATH]> {
    const PATHS: &[&str] = &["/bin", "/sbin", "/usr/bin", "/usr/sbin"];
    
    for dir in PATHS {
        let dir_bytes = dir.as_bytes();
        let cmd_bytes = cmd.as_bytes();
        
        // Build path: /dir/cmd
        let total_len = dir_bytes.len() + 1 + cmd_bytes.len();
        if total_len >= MAX_PATH {
            continue;
        }
        
        // Create a fresh buffer for each iteration to avoid contamination
        let mut full_path = [0u8; MAX_PATH];
        full_path[..dir_bytes.len()].copy_from_slice(dir_bytes);
        full_path[dir_bytes.len()] = b'/';
        full_path[dir_bytes.len() + 1..dir_bytes.len() + 1 + cmd_bytes.len()]
            .copy_from_slice(cmd_bytes);
        full_path[total_len] = 0; // null terminate
        
        // Check if file exists
        if let Ok(path_str) = core::str::from_utf8(&full_path[..total_len]) {
            if file_exists(path_str) {
                return Some(full_path);
            }
        }
    }
    
    None
}

// Execute an external command
fn execute_external_command(cmd: &str, args: &[&str]) -> bool {
    // Try to find the executable
    let path_buf = match find_executable(cmd) {
        Some(p) => p,
        None => {
            print_str("Command not found: ");
            println_str(cmd);
            return false;
        }
    };
    
    // Find the actual length of the path (until first null byte)
    let mut path_len = 0;
    while path_len < MAX_PATH && path_buf[path_len] != 0 {
        path_len += 1;
    }
    
    // Verify we found a valid null-terminated string
    if path_len == 0 {
        println_str("Error: empty path");
        return false;
    }
    if path_len >= MAX_PATH {
        println_str("Error: path too long");
        return false;
    }
    // path_buf[path_len] should be 0 (null terminator)
    
    // Debug: print the path we found
    print_str("Executing: ");
    if let Ok(path_str) = core::str::from_utf8(&path_buf[..path_len]) {
        println_str(path_str);
    } else {
        println_str("<invalid UTF-8>");
        return false;
    }
    
    // Prepare argv array (cmd + args + NULL)
    let mut argv_ptrs: [*const u8; 32] = [core::ptr::null(); 32];
    let mut argv_storage: [[u8; 64]; 32] = [[0; 64]; 32];
    
    // First argument is the command itself (just the command name, not full path)
    let cmd_bytes = cmd.as_bytes();
    let cmd_len = core::cmp::min(cmd_bytes.len(), 63);
    argv_storage[0][..cmd_len].copy_from_slice(&cmd_bytes[..cmd_len]);
    argv_storage[0][cmd_len] = 0; // Null terminate
    argv_ptrs[0] = argv_storage[0].as_ptr();
    
    // Copy additional arguments
    let mut arg_count = 1;
    for (i, arg) in args.iter().enumerate() {
        if arg_count >= 31 {
            break;
        }
        let arg_bytes = arg.as_bytes();
        let arg_len = core::cmp::min(arg_bytes.len(), 63);
        argv_storage[arg_count][..arg_len].copy_from_slice(&arg_bytes[..arg_len]);
        argv_storage[arg_count][arg_len] = 0; // Null terminate
        argv_ptrs[arg_count] = argv_storage[arg_count].as_ptr();
        arg_count += 1;
    }
    // argv_ptrs[arg_count] is already null (array initialized with nulls)
    
    // Empty environment for now
    let envp: [*const u8; 1] = [core::ptr::null()];
    
    // DEBUG: Verify path_buf contents BEFORE fork
    /* 
    print_str("BEFORE FORK: path_buf addr=0x");
    print_hex(path_buf.as_ptr() as u64);
    print_str(", first 16 bytes: ");
    for i in 0..16 {
        print_hex(path_buf[i] as u64);
        print_str(" ");
    }
    println_str("");
    */
    // Check stack pointer BEFORE any more function calls
    let sp_before: u64;
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) sp_before);
    }
    /* 
    print_str("Parent SP before fork: 0x");
    print_hex(sp_before);
    print_str(", path_buf offset from SP: 0x");
    print_hex((path_buf.as_ptr() as u64).wrapping_sub(sp_before));
    println_str("");
    
    println_str("About to fork...");
    */
    // Fork and execute
    let pid = fork();
    /*
    print_str("fork returned: ");
    print_hex(pid as u64);
    println_str("");
    */
    if pid < 0 {
        // println_str("fork failed");
        return false;
    }
    
    if pid == 0 {
        // Child process - execute the command
        // path_buf is a [u8; 256] array on stack that was copied by fork
        
        // CRITICAL: path_len is a local variable that may not be valid after fork!
        // Recalculate it from path_buf
        let mut actual_path_len = 0;
        while actual_path_len < MAX_PATH && path_buf[actual_path_len] != 0 {
            actual_path_len += 1;
        }
        
        // Debug output
        /*
        print_str("Child: path_buf addr=0x");
        print_hex(path_buf.as_ptr() as u64);
        print_str(", len=");
        print_hex(actual_path_len as u64);
        print_str(", path=");
        if actual_path_len > 0 {
            if let Ok(s) = core::str::from_utf8(&path_buf[..actual_path_len]) {
                println_str(s);
            } else {
                println_str("<invalid UTF-8>");
            }
        } else {
            println_str("<empty>");
        }
        */
        
        // Execve with the path
        let result = execve(path_buf.as_ptr(), argv_ptrs.as_ptr(), envp.as_ptr());
        
        
        // If we get here, execve failed
        print_str("Child: execve failed, error=");
        print_hex(result as u64);
        println_str("");
        exit(1);
    }
    
    // Parent process - wait for child
    let mut status: i32 = 0;
    let wait_result = wait4(pid, &mut status as *mut i32, 0);
    if wait_result < 0 {
        println_str("wait failed");
        return false;
    }
    
    // Check if child exited normally or was terminated by signal
    if wifexited(status) {
        let exit_code = wexitstatus(status);
        if exit_code != 0 {
            print_str("Command exited with status ");
            print_i32(exit_code);
            println_str("");
        }
    } else if wifsignaled(status) {
        print_str("Command terminated by signal ");
        print_i32(wtermsig(status));
        println_str("");
    }
    
    true
}

fn prompt(state: &ShellState) {
    let mut info = UserInfo::zero();
    let username = if refresh_current_user(&mut info) {
        let len = info.username_len as usize;
        if len == 0 {

            "anonymous"
        } else {
            core::str::from_utf8(&info.username[..len]).unwrap_or("nexa")
        }
    } else {
        "unknown"
    };
    print_str(username);
    print_str("@");
    print_str(HOSTNAME);
    print_str(":");
    print_str(state.current_path());
    print_str("$ ");
}

fn handle_command(state: &mut ShellState, line: &str) {
    let mut parts = line.split_whitespace();
    let Some(cmd) = parts.next() else { return; };

    match cmd {
        "help" => {
            show_help();
        }
        "pwd" => {
            cmd_pwd(state);
        }
        "cd" => {
            if let Some(path) = parts.next() {
                cmd_cd(state, path);
            } else {
                cmd_cd(state, "/");
            }
        }
        "echo" => {
            let rest = line.strip_prefix("echo").unwrap_or("").trim();
            cmd_echo(rest);
        }
        "uname" => {
            let show_all = parts.next().map_or(false, |arg| arg == "-a");
            show_uname(show_all);
        }
        "mkdir" => {
            if let Some(path) = parts.next() {
                cmd_mkdir(state, path);
            } else {
                println_str("mkdir: missing operand");
            }
        }
        "ls" => {
            let mut show_all = false;
            let mut long_format = false;
            let mut target = "";
            while let Some(arg) = parts.next() {
                if let Some(rest) = arg.strip_prefix('-') {
                    for flag in rest.as_bytes() {
                        match flag {
                            b'a' => show_all = true,
                            b'l' => long_format = true,
                            other => {
                                print_str("ls: unknown option -");
                                print_bytes(&[*other]);
                                println_str("");
                            }
                        }
                    }
                } else {
                    target = arg;
                }
            }
            list_directory_entries(state, target, show_all, long_format);
        }
        "cat" => {
            if let Some(arg) = parts.next() {
                cat(state, arg);
            } else {
                println_str("cat: missing file name");
            }
        }
        "stat" => {
            if let Some(arg) = parts.next() {
                stat_path(state, arg);
            } else {
                println_str("stat: missing file name");
            }
        }
        "login" => {
            if let Some(user) = parts.next() {
                login_user(user);
            } else {
                println_str("login: missing user name");
            }
        }
        "whoami" => whoami(),
        "users" => list_users(),
        "logout" => logout_user(),
        "adduser" => {
            let mut admin = false;
            let mut username: Option<&str> = None;
            while let Some(arg) = parts.next() {
                if arg == "-a" {
                    admin = true;
                } else {
                    username = Some(arg);
                }
            }
            if let Some(user) = username {
                add_user(user, admin);
            } else {
                println_str("adduser: missing user name");
            }
        }
        "ipc-create" => ipc_create_channel(),
        "ipc-send" => {
            if let Some(chan) = parts.next() {
                if let Some(id) = parse_u32(chan) {
                    if let Some(msg) = parts.next() {
                        ipc_send_message(id, msg);
                    } else {
                        println_str("ipc-send: missing message");
                    }
                } else {
                    println_str("ipc-send: invalid channel");
                }
            } else {
                println_str("ipc-send: missing channel");
            }
        }
        "ipc-recv" => {
            if let Some(chan) = parts.next() {
                if let Some(id) = parse_u32(chan) {
                    ipc_receive_message(id);
                } else {
                    println_str("ipc-recv: invalid channel");
                }
            } else {
                println_str("ipc-recv: missing channel");
            }
        }
        "clear" => clear_screen(),
        "exit" => {
            println_str("Bye!");
            exit(0);
        }
        _ => {
            // Try to execute as external command
            let args: std::vec::Vec<&str> = parts.collect();
            if !execute_external_command(cmd, &args) {
                // execute_external_command already prints error if command not found
            }
        }
    }
}

fn shell_loop() -> ! {
    // println_str("Welcome to NexaOS shell. Type 'help' for commands.");
    let mut buffer = [0u8; 256];
    let mut state = ShellState::new();

    loop {
        let len = read_line(&state, &mut buffer);
        if len == 0 {
            continue;
        }
        if let Ok(line) = core::str::from_utf8(&buffer[..len]) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                handle_command(&mut state, trimmed);
            }
        } else {
            println_str("Invalid UTF-8 input");
        }
    }
}

fn main() -> ! {
    install_panic_hook();
    shell_loop()
}
