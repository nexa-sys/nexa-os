//! poweroff - Power off the system
//!
//! Usage:
//!   poweroff [OPTIONS]
//!
//! Powers off the system. Requires root privileges.

use std::env;
use std::io::{self, Write};
use std::process;

// System call number for reboot (Linux-compatible)
const SYS_REBOOT: libc::c_long = 169;

// Linux reboot magic numbers
const LINUX_REBOOT_CMD_POWER_OFF: libc::c_long = 0xCDEF0123_u32 as libc::c_long;

fn print_usage() {
    eprintln!("poweroff - Power off the system");
    eprintln!();
    eprintln!("Usage: poweroff [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -f, --force    Force immediate power off");
    eprintln!("  --no-wall      Don't send wall message");
    eprintln!("  -h, --help     Show this help message");
    eprintln!();
    eprintln!("Requires root privileges.");
}

fn do_poweroff() -> i32 {
    println!("System is powering off...");
    let _ = io::stdout().flush();
    
    let ret = unsafe { libc::syscall(SYS_REBOOT, LINUX_REBOOT_CMD_POWER_OFF) };
    
    if ret == -1 {
        eprintln!("poweroff: Operation not permitted (are you root?)");
        return 1;
    }
    
    // Should never reach here
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-f" | "--force" => {
                // Force poweroff - same as normal for now
            }
            "--no-wall" => {
                // Wall message not implemented
            }
            _ => {
                eprintln!("poweroff: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
        }
    }
    
    process::exit(do_poweroff());
}
