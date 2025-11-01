#![no_std]
#![no_main]
#![feature(lang_items)]

use core::arch::asm;

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_STAT: u64 = 4;
const SYS_EXIT: u64 = 60;
const SYS_LIST_FILES: u64 = 200;
const SYS_GETERRNO: u64 = 201;
const SYS_IPC_CREATE: u64 = 210;
const SYS_IPC_SEND: u64 = 211;
const SYS_IPC_RECV: u64 = 212;
const SYS_USER_ADD: u64 = 220;
const SYS_USER_LOGIN: u64 = 221;
const SYS_USER_INFO: u64 = 222;

const LIST_FLAG_INCLUDE_HIDDEN: u64 = 0x1;
const USER_FLAG_ADMIN: u64 = 0x1;

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
            lateout("rax") ret
        );
    }
    ret
}

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

fn print_bytes(bytes: &[u8]) {
    write(1, bytes.as_ptr(), bytes.len());
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
    let mut idx = 0usize;
    let trimmed_base = if base == "/" {
        ""
    } else {
        base.trim_matches('/')
    };

    if !trimmed_base.is_empty() {
        let base_bytes = trimmed_base.as_bytes();
        if base_bytes.len() >= out.len() {
            return None;
        }
        out[..base_bytes.len()].copy_from_slice(base_bytes);
        idx = base_bytes.len();
        if idx >= out.len() {
            return None;
        }
        out[idx] = b'/';
        idx += 1;
    }

    let child_bytes = child.as_bytes();
    if child_bytes.len() == 0 {
        return None;
    }
    if idx + child_bytes.len() > out.len() {
        return None;
    }
    out[idx..idx + child_bytes.len()].copy_from_slice(child_bytes);
    idx += child_bytes.len();
    core::str::from_utf8(&out[..idx]).ok()
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

fn read_line(buf: &mut [u8]) -> usize {
    let len = read(0, buf.as_mut_ptr(), buf.len());
    if len == 0 {
        return 0;
    }
    let mut end = len;
    while end > 0 && (buf[end - 1] == b'\n' || buf[end - 1] == b'\r') {
        end -= 1;
    }
    end
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

fn list_directory_entries(path: &str, show_all: bool, long_format: bool) {
    let trimmed = path.trim();
    let mut request = ListDirRequest {
        path_ptr: 0,
        path_len: 0,
        flags: 0,
    };

    if show_all {
        request.flags |= LIST_FLAG_INCLUDE_HIDDEN;
    }

    if !trimmed.is_empty() {
        request.path_ptr = trimmed.as_ptr() as u64;
        request.path_len = trimmed.len() as u64;
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
        let mut path_buf = [0u8; 128];
        for entry in list.lines() {
            if entry.is_empty() {
                continue;
            }
            if !show_all && entry.starts_with('.') {
                continue;
            }

            if long_format {
                if let Some(full_path) = format_child_path(trimmed, entry, &mut path_buf) {
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

fn stat_path(path: &str) {
    let mut stat = Stat::zero();
    if !fetch_stat(path, &mut stat) {
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
    let len = read_line(&mut buffer);
    let password = core::str::from_utf8(&buffer[..len]).unwrap_or("");

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
    let len = read_line(&mut buffer);
    let password = core::str::from_utf8(&buffer[..len]).unwrap_or("");

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
    let mut request = IpcTransferRequest {
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

fn cat(path: &str) {
    if let Some(fd) = open_file(path) {
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
    println_str("  login <user>      Switch active user");
    println_str("  whoami            Show current user");
    println_str("  adduser [-a] <u>  Create a new user (-a for admin)");
    println_str("  ipc-create        Allocate IPC channel");
    println_str("  ipc-send <c> <m>  Send IPC message");
    println_str("  ipc-recv <c>      Receive IPC message");
    println_str("  clear             Clear the screen");
    println_str("  exit              Exit the shell");
}

fn clear_screen() {
    print_bytes(b"\x1b[2J\x1b[H");
}

fn prompt() {
    let mut info = UserInfo::zero();
    let username = if refresh_current_user(&mut info) {
        let len = info.username_len as usize;
        if len == 0 {
            "nexa"
        } else {
            core::str::from_utf8(&info.username[..len]).unwrap_or("nexa")
        }
    } else {
        "nexa"
    };
    print_str(username);
    print_str("@nexa> ");
}

fn handle_command(line: &str) {
    let mut parts = line.split_whitespace();
    let Some(cmd) = parts.next() else { return; };

    match cmd {
        "help" => show_help(),
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
            list_directory_entries(target, show_all, long_format);
        }
        "cat" => {
            if let Some(arg) = parts.next() {
                cat(arg);
            } else {
                println_str("cat: missing file name");
            }
        }
        "stat" => {
            if let Some(arg) = parts.next() {
                stat_path(arg);
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
            println_str("Unknown command");
        }
    }
}

fn shell_loop() -> ! {
    println_str("Welcome to NexaOS shell. Type 'help' for commands.");
    let mut buffer = [0u8; 256];

    loop {
        prompt();
        let len = read_line(&mut buffer);
        if len == 0 {
            continue;
        }
        if let Ok(line) = core::str::from_utf8(&buffer[..len]) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                handle_command(trimmed);
            }
        } else {
            println_str("Invalid UTF-8 input");
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(1);
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    shell_loop()
}

#[no_mangle]
pub extern "C" fn memset(dest: *mut u8, val: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        unsafe { *dest.add(i) = val as u8; }
        i += 1;
    }
    dest
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[no_mangle]
pub extern "C" fn main() {
    exit(0);
}