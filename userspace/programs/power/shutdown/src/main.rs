//! shutdown - Power off or reboot the system
//!
//! Usage:
//!   shutdown [OPTIONS] [TIME] [MESSAGE]
//!
//! Brings the system down in a safe way. Requires root privileges.

use std::env;
use std::io::{self, Write};
use std::process;

// System call number for reboot (Linux-compatible)
const SYS_REBOOT: libc::c_long = 169;

// Linux reboot magic numbers
const LINUX_REBOOT_CMD_RESTART: libc::c_long = 0x01234567;
const LINUX_REBOOT_CMD_HALT: libc::c_long = 0x4321FEDC_u32 as libc::c_long;
const LINUX_REBOOT_CMD_POWER_OFF: libc::c_long = 0xCDEF0123_u32 as libc::c_long;

fn print_usage() {
    eprintln!("shutdown - Power off or reboot the system");
    eprintln!();
    eprintln!("Usage: shutdown [OPTIONS] [TIME] [MESSAGE]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -h, --halt     Halt the system");
    eprintln!("  -P, --poweroff Power off the system (default)");
    eprintln!("  -r, --reboot   Reboot the system");
    eprintln!("  -c             Cancel a pending shutdown");
    eprintln!("  --help         Show this help message");
    eprintln!();
    eprintln!("Time:");
    eprintln!("  now            Shut down immediately");
    eprintln!("  +m             Shut down in m minutes");
    eprintln!("  hh:mm          Shut down at specified time");
    eprintln!();
    eprintln!("Requires root privileges.");
}

#[derive(Clone, Copy, PartialEq)]
enum ShutdownMode {
    PowerOff,
    Halt,
    Reboot,
}

fn do_shutdown(mode: ShutdownMode) -> i32 {
    let action = match mode {
        ShutdownMode::PowerOff => "powering off",
        ShutdownMode::Halt => "halting",
        ShutdownMode::Reboot => "rebooting",
    };
    
    println!("System is {}...", action);
    let _ = io::stdout().flush();
    
    let cmd = match mode {
        ShutdownMode::PowerOff => LINUX_REBOOT_CMD_POWER_OFF,
        ShutdownMode::Halt => LINUX_REBOOT_CMD_HALT,
        ShutdownMode::Reboot => LINUX_REBOOT_CMD_RESTART,
    };
    
    let ret = unsafe { libc::syscall(SYS_REBOOT, cmd) };
    
    if ret == -1 {
        eprintln!("shutdown: Operation not permitted (are you root?)");
        return 1;
    }
    
    // Should never reach here
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut mode = ShutdownMode::PowerOff;
    let mut immediate = false;
    
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--help" => {
                print_usage();
                process::exit(0);
            }
            "-h" | "--halt" => {
                mode = ShutdownMode::Halt;
            }
            "-P" | "--poweroff" => {
                mode = ShutdownMode::PowerOff;
            }
            "-r" | "--reboot" => {
                mode = ShutdownMode::Reboot;
            }
            "-c" => {
                eprintln!("shutdown: Cancel not implemented (no pending shutdown)");
                process::exit(0);
            }
            "now" => {
                immediate = true;
            }
            _ if arg.starts_with('+') => {
                // Time delay - not implemented, treat as immediate
                eprintln!("shutdown: Delayed shutdown not implemented, shutting down now");
                immediate = true;
            }
            _ => {
                // Could be a message or invalid option
                if arg.starts_with('-') {
                    eprintln!("shutdown: unknown option: {}", arg);
                    print_usage();
                    process::exit(1);
                }
                // Treat as wall message (ignore for now)
            }
        }
        i += 1;
    }
    
    // Default to immediate if no time specified
    if !immediate && args.len() == 1 {
        // No arguments, default to immediate poweroff
        immediate = true;
    }
    
    process::exit(do_shutdown(mode));
}
