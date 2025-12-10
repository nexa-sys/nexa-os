//! adduser - Create a new user account
//!
//! Usage: adduser [-a] <username>
//!   -a    Create admin user
//!
//! Prompts for password and creates a new user in the kernel user database.

#![no_std]
#![no_main]

use core::arch::asm;

#[cfg(feature = "use-nrlib")]
extern crate nrlib;

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;
const SYS_USER_ADD: u64 = 220;

const STDIN: u64 = 0;
const STDOUT: u64 = 1;
const STDERR: u64 = 2;

const USER_FLAG_ADMIN: u64 = 0x1;
const MAX_INPUT: usize = 64;

#[repr(C)]
struct UserRequest {
    username_ptr: u64,
    username_len: u64,
    password_ptr: u64,
    password_len: u64,
    flags: u64,
}

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
            out("rcx") _,
            out("r8") _,
            out("r9") _,
            out("r10") _,
            out("r11") _
        );
    }
    ret
}

#[inline(always)]
fn syscall1(n: u64, a1: u64) -> u64 {
    syscall3(n, a1, 0, 0)
}

fn write(fd: u64, buf: &[u8]) {
    if !buf.is_empty() {
        syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64);
    }
}

fn read(fd: u64, buf: &mut [u8]) -> isize {
    let ret = syscall3(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64);
    if ret == u64::MAX {
        -1
    } else {
        ret as isize
    }
}

fn print(s: &str) {
    write(STDOUT, s.as_bytes());
}

fn eprint(s: &str) {
    write(STDERR, s.as_bytes());
}

fn exit(code: i32) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {
        unsafe { asm!("hlt") }
    }
}

fn read_line(buf: &mut [u8]) -> usize {
    let mut pos = 0;
    let mut byte = [0u8; 1];

    while pos < buf.len() - 1 {
        let n = read(STDIN, &mut byte);
        if n <= 0 {
            break;
        }
        if byte[0] == b'\n' || byte[0] == b'\r' {
            break;
        }
        buf[pos] = byte[0];
        pos += 1;
    }
    pos
}

fn add_user(username: &[u8], password: &[u8], admin: bool) -> Result<(), i32> {
    let request = UserRequest {
        username_ptr: username.as_ptr() as u64,
        username_len: username.len() as u64,
        password_ptr: password.as_ptr() as u64,
        password_len: password.len() as u64,
        flags: if admin { USER_FLAG_ADMIN } else { 0 },
    };

    let ret = syscall3(SYS_USER_ADD, &request as *const UserRequest as u64, 0, 0);
    if ret == u64::MAX {
        Err(-1)
    } else {
        Ok(())
    }
}

fn print_usage() {
    print("Usage: adduser [-a] <username>\n");
    print("  -a    Create admin user\n");
}

/// Parse C-style argv
unsafe fn get_arg(argv: *const *const u8, index: isize) -> Option<&'static [u8]> {
    let arg_ptr = *argv.offset(index);
    if arg_ptr.is_null() {
        return None;
    }

    // Find string length
    let mut len = 0;
    while *arg_ptr.add(len) != 0 {
        len += 1;
    }

    Some(core::slice::from_raw_parts(arg_ptr, len))
}

#[no_mangle]
pub extern "C" fn main(argc: isize, argv: *const *const u8) -> isize {
    let mut admin = false;
    let mut username: Option<&[u8]> = None;

    // Parse arguments
    let mut i = 1;
    while i < argc {
        let arg = unsafe { get_arg(argv, i) };
        match arg {
            Some(b"-a") => admin = true,
            Some(b"-h") | Some(b"--help") => {
                print_usage();
                exit(0);
            }
            Some(name) if !name.starts_with(b"-") => {
                username = Some(name);
            }
            Some(_) => {
                eprint("adduser: unknown option\n");
                print_usage();
                exit(1);
            }
            None => break,
        }
        i += 1;
    }

    let username = match username {
        Some(u) => u,
        None => {
            eprint("adduser: missing username\n");
            print_usage();
            exit(1);
        }
    };

    // Prompt for password
    print("New password: ");
    let mut password_buf = [0u8; MAX_INPUT];
    let password_len = read_line(&mut password_buf);
    print("\n");

    if password_len == 0 {
        eprint("adduser: password cannot be empty\n");
        exit(1);
    }

    let password = &password_buf[..password_len];

    match add_user(username, password, admin) {
        Ok(()) => {
            print("User created successfully.\n");
            0
        }
        Err(_) => {
            eprint("adduser: failed to create user\n");
            1
        }
    }
}
