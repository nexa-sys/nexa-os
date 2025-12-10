//! reboot - Restart the system
//!
//! Usage:
//!   reboot [OPTIONS]
//!
//! Restarts the system. Requires root privileges.

use std::env;
use std::io::{self, Write};
use std::process;

// System call number for reboot (Linux-compatible)
const SYS_REBOOT: libc::c_long = 169;

// Linux reboot magic numbers
const LINUX_REBOOT_CMD_RESTART: libc::c_long = 0x01234567;

fn print_usage() {
    eprintln!("reboot - Restart the system");
    eprintln!();
    eprintln!("Usage: reboot [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -f, --force    Force immediate reboot (skip init)");
    eprintln!("  -h, --help     Show this help message");
    eprintln!();
    eprintln!("Requires root privileges.");
}

fn do_reboot() -> i32 {
    println!("System is rebooting...");
    let _ = io::stdout().flush();

    let ret = unsafe { libc::syscall(SYS_REBOOT, LINUX_REBOOT_CMD_RESTART) };

    if ret == -1 {
        eprintln!("reboot: Operation not permitted (are you root?)");
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
                // Force reboot - same as normal for now
            }
            _ => {
                eprintln!("reboot: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
        }
    }

    process::exit(do_reboot());
}
