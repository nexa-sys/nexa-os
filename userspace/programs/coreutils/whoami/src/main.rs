//! whoami - Print effective user name
//!
//! Usage:
//!   whoami

use std::arch::asm;
use std::env;
use std::process;

// NexaOS syscall numbers
const SYS_GETERRNO: u64 = 201;
const SYS_USER_INFO: u64 = 222;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserInfo {
    username: [u8; 32],
    username_len: u64,
    uid: u32,
    gid: u32,
    is_admin: u32,
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
            clobber_abi("sysv64")
        );
    }
    ret
}

fn get_user_info() -> Option<UserInfo> {
    let mut info = UserInfo::default();
    let ret = syscall3(SYS_USER_INFO, &mut info as *mut UserInfo as u64, 0, 0);
    if ret != u64::MAX { Some(info) } else { None }
}

fn print_usage() {
    println!("whoami - Print effective user name");
    println!();
    println!("Usage: whoami [OPTIONS]");
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

    match get_user_info() {
        Some(info) => {
            let len = info.username_len as usize;
            if len == 0 {
                println!("anonymous");
            } else if let Ok(name) = std::str::from_utf8(&info.username[..len]) {
                println!("{}", name);
            } else {
                eprintln!("whoami: invalid username");
                process::exit(1);
            }
        }
        None => {
            eprintln!("whoami: cannot get user info");
            process::exit(1);
        }
    }
}
