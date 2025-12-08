//! swapon - Enable swap space
//!
//! Usage:
//!   swapon [OPTIONS] [DEVICE]
//!   swapon -a          Enable all swap devices from /etc/fstab
//!   swapon -s          Show swap status
//!
//! Enables devices/files for paging and swapping.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// Linux-compatible system call numbers (x86_64)
const SYS_SWAPON: libc::c_long = 167;

// Swap flags
const SWAP_FLAG_PREFER: i32 = 0x8000;
const SWAP_FLAG_PRIO_MASK: i32 = 0x7fff;
const SWAP_FLAG_PRIO_SHIFT: i32 = 0;
const SWAP_FLAG_DISCARD: i32 = 0x10000;
const SWAP_FLAG_DISCARD_ONCE: i32 = 0x20000;
const SWAP_FLAG_DISCARD_PAGES: i32 = 0x40000;

fn print_usage() {
    eprintln!("swapon - Enable swap space");
    eprintln!();
    eprintln!("Usage: swapon [OPTIONS] [DEVICE]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -a, --all           Enable all swap from /etc/fstab");
    eprintln!("  -s, --summary       Display swap usage summary");
    eprintln!("  -p, --priority N    Set swap priority (0-32767)");
    eprintln!("  -d, --discard       Enable discard/TRIM");
    eprintln!("  -h, --help          Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  swapon /dev/vdb         Enable swap on /dev/vdb");
    eprintln!("  swapon -p 10 /dev/vdb   Enable with priority 10");
    eprintln!("  swapon -a               Enable all swap from /etc/fstab");
    eprintln!("  swapon -s               Show swap status");
}

fn show_swap_summary() {
    // Read /proc/swaps
    match fs::read_to_string("/proc/swaps") {
        Ok(content) => {
            print!("{}", content);
        }
        Err(e) => {
            eprintln!("swapon: cannot read /proc/swaps: {}", e);
        }
    }
}

fn show_meminfo_swap() {
    // Read /proc/meminfo for swap statistics
    match fs::read_to_string("/proc/meminfo") {
        Ok(content) => {
            for line in content.lines() {
                if line.starts_with("Swap") {
                    println!("{}", line);
                }
            }
        }
        Err(e) => {
            eprintln!("swapon: cannot read /proc/meminfo: {}", e);
        }
    }
}

fn do_swapon(path: &str, flags: i32) -> i32 {
    let c_path = match std::ffi::CString::new(path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("swapon: invalid path: {}", path);
            return 1;
        }
    };
    
    let ret = unsafe {
        libc::syscall(SYS_SWAPON, c_path.as_ptr(), flags)
    };
    
    if ret == -1 {
        let errno = unsafe { *libc::__errno_location() };
        match errno {
            libc::EPERM => eprintln!("swapon: {}: Operation not permitted (are you root?)", path),
            libc::ENOENT => eprintln!("swapon: {}: No such file or directory", path),
            libc::EINVAL => eprintln!("swapon: {}: Invalid swap signature or already enabled", path),
            libc::EBUSY => eprintln!("swapon: {}: Device or resource busy (already in use)", path),
            libc::ENOSYS => eprintln!("swapon: {}: Function not implemented", path),
            _ => eprintln!("swapon: {}: Error (errno={})", path, errno),
        }
        return 1;
    }
    
    println!("swapon: {}: swap enabled", path);
    0
}

fn enable_from_fstab() -> i32 {
    // Read /etc/fstab and enable all swap entries
    let content = match fs::read_to_string("/etc/fstab") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("swapon: cannot read /etc/fstab: {}", e);
            return 1;
        }
    };
    
    let mut exit_code = 0;
    let mut found_swap = false;
    
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[2] == "swap" {
            found_swap = true;
            let device = parts[0];
            
            // Parse options for priority
            let mut flags = 0i32;
            if parts.len() >= 4 {
                let opts = parts[3];
                for opt in opts.split(',') {
                    if let Some(prio_str) = opt.strip_prefix("pri=") {
                        if let Ok(prio) = prio_str.parse::<i32>() {
                            flags |= SWAP_FLAG_PREFER | (prio & SWAP_FLAG_PRIO_MASK);
                        }
                    } else if opt == "discard" {
                        flags |= SWAP_FLAG_DISCARD;
                    }
                }
            }
            
            if do_swapon(device, flags) != 0 {
                exit_code = 1;
            }
        }
    }
    
    if !found_swap {
        eprintln!("swapon: no swap entries found in /etc/fstab");
        return 1;
    }
    
    exit_code
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut device: Option<String> = None;
    let mut show_summary = false;
    let mut enable_all = false;
    let mut priority: Option<i32> = None;
    let mut discard = false;
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-s" | "--summary" => {
                show_summary = true;
            }
            "-a" | "--all" => {
                enable_all = true;
            }
            "-d" | "--discard" => {
                discard = true;
            }
            "-p" | "--priority" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("swapon: --priority requires an argument");
                    process::exit(1);
                }
                priority = match args[i].parse() {
                    Ok(p) if p >= 0 && p <= 32767 => Some(p),
                    _ => {
                        eprintln!("swapon: invalid priority: {}", args[i]);
                        process::exit(1);
                    }
                };
            }
            arg if arg.starts_with('-') => {
                eprintln!("swapon: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
            arg => {
                device = Some(arg.to_string());
            }
        }
        i += 1;
    }
    
    // Handle -s (summary) option
    if show_summary {
        show_swap_summary();
        println!();
        show_meminfo_swap();
        process::exit(0);
    }
    
    // Handle -a (all) option
    if enable_all {
        process::exit(enable_from_fstab());
    }
    
    // Need a device
    let device = match device {
        Some(d) => d,
        None => {
            // If no arguments, show summary
            show_swap_summary();
            process::exit(0);
        }
    };
    
    // Build flags
    let mut flags = 0i32;
    if let Some(prio) = priority {
        flags |= SWAP_FLAG_PREFER | (prio & SWAP_FLAG_PRIO_MASK);
    }
    if discard {
        flags |= SWAP_FLAG_DISCARD;
    }
    
    process::exit(do_swapon(&device, flags));
}
