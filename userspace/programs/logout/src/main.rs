//! logout - Log out current user
//!
//! Usage: logout
//!
//! Logs out the current user session from the kernel.

#![no_std]
#![no_main]

use core::arch::asm;

#[cfg(feature = "use-nrlib")]
extern crate nrlib;

const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;
const SYS_USER_LOGOUT: u64 = 224;

const STDOUT: u64 = 1;
const STDERR: u64 = 2;

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

fn logout() -> Result<(), i32> {
    let ret = syscall1(SYS_USER_LOGOUT, 0);
    if ret == u64::MAX {
        Err(-1)
    } else {
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn main(_argc: isize, _argv: *const *const u8) -> isize {
    match logout() {
        Ok(()) => {
            print("Logged out.\n");
            exit(0);
        }
        Err(_) => {
            eprint("logout: failed to log out\n");
            exit(1);
        }
    }
}
