//! halt - Halt the system
//!
//! Usage:
//!   halt [OPTIONS]
//!
//! Halts the system. Requires root privileges.

use std::env;
use std::io::{self, Write};
use std::process;

// System call number for reboot (Linux-compatible)
const SYS_REBOOT: libc::c_long = 169;

// Linux reboot magic numbers
const LINUX_REBOOT_CMD_HALT: libc::c_long = 0x4321FEDC_u32 as libc::c_long;
const LINUX_REBOOT_CMD_POWER_OFF: libc::c_long = 0xCDEF0123_u32 as libc::c_long;

fn print_usage() {
    eprintln!("halt - Halt the system");
    eprintln!();
    eprintln!("Usage: halt [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -p, --poweroff  Power off after halt (default)");
    eprintln!("  -f, --force     Force immediate halt");
    eprintln!("  --no-wall       Don't send wall message");
    eprintln!("  -h, --help      Show this help message");
    eprintln!();
    eprintln!("Requires root privileges.");
}

fn do_halt(poweroff: bool) -> i32 {
    if poweroff {
        println!("System is powering off...");
    } else {
        println!("System is halting...");
    }
    let _ = io::stdout().flush();

    let cmd = if poweroff {
        LINUX_REBOOT_CMD_POWER_OFF
    } else {
        LINUX_REBOOT_CMD_HALT
    };

    let ret = unsafe { libc::syscall(SYS_REBOOT, cmd) };

    if ret == -1 {
        eprintln!("halt: Operation not permitted (are you root?)");
        return 1;
    }

    // Should never reach here
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut poweroff = true; // Default to poweroff

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-p" | "--poweroff" => {
                poweroff = true;
            }
            "-f" | "--force" => {
                // Force halt - same as normal for now
            }
            "--no-wall" => {
                // Wall message not implemented
            }
            _ => {
                eprintln!("halt: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
        }
    }

    process::exit(do_halt(poweroff));
}
