//! ipc-send - Send a message to an IPC channel
//!
//! Usage: ipc-send <channel> <message>
//!
//! Sends a message to the specified IPC channel.

#![no_std]
#![no_main]

use core::arch::asm;

#[cfg(feature = "use-nrlib")]
extern crate nrlib;

const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;
const SYS_IPC_SEND: u64 = 211;

const STDOUT: u64 = 1;
const STDERR: u64 = 2;

#[repr(C)]
struct IpcTransferRequest {
    channel_id: u32,
    flags: u32,
    buffer_ptr: u64,
    buffer_len: u64,
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

fn write_fd(fd: u64, buf: &[u8]) {
    if !buf.is_empty() {
        syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64);
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

fn ipc_send(channel: u32, message: &[u8]) -> Result<(), i32> {
    let request = IpcTransferRequest {
        channel_id: channel,
        flags: 0,
        buffer_ptr: message.as_ptr() as u64,
        buffer_len: message.len() as u64,
    };
    let ret = syscall3(SYS_IPC_SEND, &request as *const IpcTransferRequest as u64, 0, 0);
    if ret == u64::MAX { Err(-1) } else { Ok(()) }
}

/// Parse C-style argv
unsafe fn get_arg(argv: *const *const u8, index: isize) -> Option<&'static [u8]> {
    let arg_ptr = *argv.offset(index);
    if arg_ptr.is_null() {
        return None;
    }
    let mut len = 0;
    while *arg_ptr.add(len) != 0 {
        len += 1;
    }
    Some(core::slice::from_raw_parts(arg_ptr, len))
}

/// Simple atoi for channel ID
fn parse_u32(bytes: &[u8]) -> Option<u32> {
    let mut result: u32 = 0;
    for &b in bytes {
        if b < b'0' || b > b'9' {
            return None;
        }
        result = result.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(result)
}

fn print_usage() {
    print("Usage: ipc-send <channel> <message>\n");
}

#[no_mangle]
pub extern "C" fn main(argc: isize, argv: *const *const u8) -> isize {
    if argc < 3 {
        eprint("ipc-send: missing arguments\n");
        print_usage();
        exit(1);
    }
    
    let channel_arg = unsafe { get_arg(argv, 1) };
    let message_arg = unsafe { get_arg(argv, 2) };
    
    let channel = match channel_arg.and_then(parse_u32) {
        Some(c) => c,
        None => {
            eprint("ipc-send: invalid channel\n");
            exit(1);
        }
    };
    
    let message = match message_arg {
        Some(m) => m,
        None => {
            eprint("ipc-send: missing message\n");
            exit(1);
        }
    };
    
    if message.is_empty() {
        eprint("ipc-send: message cannot be empty\n");
        exit(1);
    }
    
    match ipc_send(channel, message) {
        Ok(()) => {
            print("Message sent.\n");
            0
        }
        Err(_) => {
            eprint("ipc-send: failed to send message\n");
            1
        }
    }
}
