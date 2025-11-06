#![no_std]
#![no_main]
#![feature(lang_items)]

//! Login - user authentication program
//! Prompts for username/password and authenticates against kernel user database

use core::arch::asm;
use core::panic::PanicInfo;

// System call numbers
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_USER_LOGIN: u64 = 221;
const SYS_USER_ADD: u64 = 220;

// Standard file descriptors
const STDIN: u64 = 0;
const STDOUT: u64 = 1;
const STDERR: u64 = 2;

const MAX_INPUT: usize = 64;

#[repr(C)]
struct UserRequest {
    username_ptr: u64,
    username_len: u64,
    password_ptr: u64,
    password_len: u64,
    flags: u64,
}

/// Syscall wrapper
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
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
fn syscall1(n: u64, a1: u64) -> u64 {
    syscall3(n, a1, 0, 0)
}

/// Write to file descriptor
fn write(fd: u64, buf: &[u8]) -> isize {
    if buf.is_empty() {
        return 0;
    }
    let ret = syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64);
    if ret == u64::MAX {
        -1
    } else {
        ret as isize
    }
}

/// Read from file descriptor
fn read(fd: u64, buf: &mut [u8]) -> isize {
    let ret = syscall3(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64);
    if ret == u64::MAX {
        -1
    } else {
        ret as isize
    }
}

/// Print string to stdout
fn print(s: &str) {
    write(STDOUT, s.as_bytes());
}

/// Exit process
fn exit(code: i32) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {
        unsafe { asm!("hlt") }
    }
}

/// Fork process
fn fork() -> i64 {
    let ret = syscall3(SYS_FORK, 0, 0, 0);
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

/// Execute program
fn execve(path: &str, argv: &[*const u8], envp: &[*const u8]) -> i64 {
    let ret = syscall3(
        SYS_EXECVE,
        path.as_ptr() as u64,
        argv.as_ptr() as u64,
        envp.as_ptr() as u64,
    );
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

/// Read a line from stdin
fn read_line(buf: &mut [u8]) -> usize {
    let mut pos = 0;
    let mut tmp = [0u8; 1];
    
    while pos < buf.len() {
        let n = read(STDIN, &mut tmp);
        if n < 0 {
            break;
        }
        if n == 0 {
            continue;
        }
        
        let ch = tmp[0];
        
        // Handle backspace
        if ch == 8 || ch == 127 {
            if pos > 0 {
                pos -= 1;
                print("\x08 \x08"); // Backspace, space, backspace
            }
            continue;
        }
        
        // Handle newline
        if ch == b'\n' || ch == b'\r' {
            print("\n");
            break;
        }
        
        // Printable characters
        if ch >= 32 && ch < 127 {
            buf[pos] = ch;
            pos += 1;
            write(STDOUT, &[ch]);
        }
    }
    
    pos
}

/// Read password (no echo)
fn read_password(buf: &mut [u8]) -> usize {
    let mut pos = 0;
    let mut tmp = [0u8; 1];
    
    while pos < buf.len() {
        let n = read(STDIN, &mut tmp);
        if n < 0 {
            break;
        }
        if n == 0 {
            continue;
        }
        
        let ch = tmp[0];
        
        // Handle backspace
        if ch == 8 || ch == 127 {
            if pos > 0 {
                pos -= 1;
                print("\x08 \x08"); // Backspace, space, backspace
            }
            continue;
        }
        
        // Handle newline
        if ch == b'\n' || ch == b'\r' {
            print("\n");
            break;
        }
        
        // Printable characters (but don't echo)
        if ch >= 32 && ch < 127 {
            buf[pos] = ch;
            pos += 1;
            print("*"); // Show asterisk instead
        }
    }
    
    pos
}

/// Add default user if no users exist
fn ensure_default_user() {
    let username = b"root";
    let password = b"root";
    
    let req = UserRequest {
        username_ptr: username.as_ptr() as u64,
        username_len: username.len() as u64,
        password_ptr: password.as_ptr() as u64,
        password_len: password.len() as u64,
        flags: 1, // Admin flag
    };
    
    // Try to add user (will fail if already exists, which is fine)
    syscall1(SYS_USER_ADD, &req as *const UserRequest as u64);
}

fn login_main() -> ! {
    // Ensure we have a default user
    ensure_default_user();

    let mut username_buf = [0u8; MAX_INPUT];
    let mut password_buf = [0u8; MAX_INPUT];
    
    print("\n");
    print("\x1b[1;32mNexaOS Login\x1b[0m\n");
    print("\x1b[0;36mDefault credentials: root/root\x1b[0m\n");
    print("\n");
    
    // Read username
    print("login: ");
    let username_len = read_line(&mut username_buf);
    
    if username_len == 0 {
        exit(1);
    }
    
    // Read password
    print("password: ");
    let password_len = read_password(&mut password_buf);
    
    // Attempt login
    let req = UserRequest {
        username_ptr: username_buf.as_ptr() as u64,
        username_len: username_len as u64,
        password_ptr: password_buf.as_ptr() as u64,
        password_len: password_len as u64,
        flags: 0,
    };

    debug_print_ptr("username_ptr", req.username_ptr);
    debug_print_ptr("password_ptr", req.password_ptr);
    
    let result = syscall1(SYS_USER_LOGIN, &req as *const UserRequest as u64);
    
    if result == 0 {
        // Login successful
        print("\n\x1b[1;32mLogin successful!\x1b[0m\n");
        print("Starting user session...\n\n");
        
        // Fork and exec shell as user session
        let pid = fork();
        
        if pid == 0 {
            // Child process - start user shell
            let shell_path = "/bin/sh\0";
            let shell_path_str = unsafe {
                core::str::from_utf8_unchecked(&shell_path.as_bytes()[..7])
            };
            
            let argv: [*const u8; 2] = [
                shell_path.as_ptr(),
                core::ptr::null(),
            ];
            let envp: [*const u8; 1] = [
                core::ptr::null(),
            ];
            
            execve(shell_path_str, &argv, &envp);
            
            // If exec fails
            print("Failed to start shell\n");
            exit(1);
        }
        
        // Parent returns to getty (which will wait for shell to exit)
        exit(0);
    } else {
        // Login failed
        print("\n\x1b[1;31mLogin incorrect\x1b[0m\n");
        exit(1);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("\nlogin: PANIC\n");
    exit(1);
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    login_main()
}

#[allow(dead_code)]
fn main() {
    loop {}
}

fn debug_print_ptr(label: &str, value: u64) {
    print(label);
    print(": 0x");
    let mut buf = [0u8; 16];
    for i in 0..16 {
        let shift = (15 - i) * 4;
        let nibble = ((value >> shift) & 0xF) as u8;
        buf[i] = match nibble {
            0..=9 => b'0' + nibble,
            _ => b'a' + (nibble - 10),
        };
    }
    let _ = write(STDOUT, &buf);
    print("\n");
}
