//! swapoff - Disable swap space
//!
//! Usage:
//!   swapoff [OPTIONS] [DEVICE]
//!   swapoff -a          Disable all swap devices
//!
//! Disables devices/files for paging and swapping.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// Linux-compatible system call numbers (x86_64)
const SYS_SWAPOFF: libc::c_long = 168;

fn print_usage() {
    eprintln!("swapoff - Disable swap space");
    eprintln!();
    eprintln!("Usage: swapoff [OPTIONS] [DEVICE]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -a, --all        Disable all swap");
    eprintln!("  -v, --verbose    Be verbose");
    eprintln!("  -h, --help       Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  swapoff /dev/vdb     Disable swap on /dev/vdb");
    eprintln!("  swapoff -a           Disable all swap devices");
}

fn do_swapoff(path: &str, verbose: bool) -> i32 {
    let c_path = match std::ffi::CString::new(path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("swapoff: invalid path: {}", path);
            return 1;
        }
    };

    if verbose {
        println!("swapoff: disabling {}", path);
    }

    let ret = unsafe { libc::syscall(SYS_SWAPOFF, c_path.as_ptr()) };

    if ret == -1 {
        let errno = unsafe { *libc::__errno_location() };
        match errno {
            libc::EPERM => eprintln!("swapoff: {}: Operation not permitted (are you root?)", path),
            libc::ENOENT => eprintln!("swapoff: {}: No such file or directory", path),
            libc::EINVAL => eprintln!("swapoff: {}: Not a swap device", path),
            libc::ENOMEM => eprintln!(
                "swapoff: {}: Not enough memory to disable swap (too many pages in use)",
                path
            ),
            libc::ENOSYS => eprintln!("swapoff: {}: Function not implemented", path),
            _ => eprintln!("swapoff: {}: Error (errno={})", path, errno),
        }
        return 1;
    }

    if verbose {
        println!("swapoff: {}: swap disabled", path);
    }

    0
}

fn disable_all_swap(verbose: bool) -> i32 {
    // Read /proc/swaps to get list of active swap devices
    let content = match fs::read_to_string("/proc/swaps") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("swapoff: cannot read /proc/swaps: {}", e);
            return 1;
        }
    };

    let mut exit_code = 0;
    let mut found_swap = false;

    for (i, line) in content.lines().enumerate() {
        // Skip header line
        if i == 0 {
            continue;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if !parts.is_empty() {
            found_swap = true;
            let device = parts[0];

            if do_swapoff(device, verbose) != 0 {
                exit_code = 1;
            }
        }
    }

    if !found_swap {
        if verbose {
            println!("swapoff: no swap devices active");
        }
    }

    exit_code
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut devices: Vec<String> = Vec::new();
    let mut disable_all = false;
    let mut verbose = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-a" | "--all" => {
                disable_all = true;
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            arg if arg.starts_with('-') => {
                eprintln!("swapoff: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
            arg => {
                devices.push(arg.to_string());
            }
        }
        i += 1;
    }

    // Handle -a (all) option
    if disable_all {
        process::exit(disable_all_swap(verbose));
    }

    // Need at least one device
    if devices.is_empty() {
        eprintln!("swapoff: need a device or -a option");
        print_usage();
        process::exit(1);
    }

    // Disable specified devices
    let mut exit_code = 0;
    for device in &devices {
        if do_swapoff(device, verbose) != 0 {
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
