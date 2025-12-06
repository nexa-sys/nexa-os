//! users - List registered users
//!
//! Usage:
//!   users

use std::arch::asm;
use std::env;
use std::process;

// NexaOS syscall numbers
const SYS_GETERRNO: u64 = 201;
const SYS_USER_LIST: u64 = 223;

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
            clobber_abi("sysv64")
        );
    }
    ret
}

fn syscall1(n: u64, a1: u64) -> u64 {
    syscall3(n, a1, 0, 0)
}

fn errno() -> i32 {
    syscall1(SYS_GETERRNO, 0) as i32
}

fn list_users() -> Result<String, i32> {
    let mut buffer = vec![0u8; 512];
    let written = syscall3(SYS_USER_LIST, buffer.as_mut_ptr() as u64, buffer.len() as u64, 0);
    if written == u64::MAX {
        return Err(errno());
    }
    buffer.truncate(written as usize);
    String::from_utf8(buffer).map_err(|_| -1)
}

fn print_usage() {
    println!("users - List registered users");
    println!();
    println!("Usage: users [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -h    Show this help message");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        }
    }

    match list_users() {
        Ok(list) => {
            if list.is_empty() {
                println!("(no users)");
            } else {
                print!("{}", list);
            }
        }
        Err(e) => {
            eprintln!("users: failed (errno {})", e);
            process::exit(1);
        }
    }
}
