//! ipc-create - Create an IPC channel
//!
//! Usage: ipc-create
//!
//! Creates a new IPC channel and prints its ID.

#![no_std]
#![no_main]

use core::arch::asm;
use core::fmt::Write;

#[cfg(feature = "use-nrlib")]
extern crate nrlib;

const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;
const SYS_IPC_CREATE: u64 = 210;

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

#[inline(always)]
fn syscall0(n: u64) -> u64 {
    syscall3(n, 0, 0, 0)
}

fn write_fd(fd: u64, buf: &[u8]) {
    if !buf.is_empty() {
        syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64);
    }
}

struct FdWriter(u64);

impl Write for FdWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_fd(self.0, s.as_bytes());
        Ok(())
    }
}

fn print(s: &str) {
    write_fd(STDOUT, s.as_bytes());
}

fn eprint(s: &str) {
    write_fd(STDERR, s.as_bytes());
}

fn exit(code: i32) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {
        unsafe { asm!("hlt") }
    }
}

fn ipc_create() -> Result<u64, i32> {
    let id = syscall0(SYS_IPC_CREATE);
    if id == u64::MAX {
        Err(-1)
    } else {
        Ok(id)
    }
}

#[no_mangle]
pub extern "C" fn main(_argc: isize, _argv: *const *const u8) -> isize {
    match ipc_create() {
        Ok(id) => {
            let mut writer = FdWriter(STDOUT);
            let _ = write!(writer, "Channel {} created\n", id);
            0
        }
        Err(_) => {
            eprint("ipc-create: failed to create channel\n");
            1
        }
    }
}
