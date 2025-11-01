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

fn close_file(fd: u64) {
    syscall3(SYS_CLOSE, fd, 0, 0);
}

fn list_files() {
    let mut buf = [0u8; 1024];
    let written = syscall3(SYS_LIST_FILES, buf.as_mut_ptr() as u64, buf.len() as u64, 0) as usize;
    if written == 0 {
        println_str("(no files)");
        return;
    }

    if let Ok(list) = core::str::from_utf8(&buf[..written]) {
        for line in list.lines() {
            println_str(line);
        }
    } else {
        println_str("ls: kernel returned invalid UTF-8");
    }
}

fn stat_path(path: &str) {
    let mut stat = Stat::zero();
    let ret = syscall3(
        SYS_STAT,
        path.as_ptr() as u64,
        path.len() as u64,
        &mut stat as *mut Stat as u64,
    );
    if ret == u64::MAX {
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
    println_str("  help          Show this message");
    println_str("  ls            List files in virtual filesystem" );
    println_str("  cat <file>    Print file contents");
    println_str("  stat <file>   Show file metadata");
    println_str("  clear         Clear the screen");
    println_str("  exit          Exit the shell");
}

fn clear_screen() {
    print_bytes(b"\x1b[2J\x1b[H");
}

fn prompt() {
    print_str("nexa> ");
}

fn handle_command(line: &str) {
    let mut parts = line.split_whitespace();
    let Some(cmd) = parts.next() else { return; };

    match cmd {
        "help" => show_help(),
        "ls" => list_files(),
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