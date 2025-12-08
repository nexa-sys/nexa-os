//! rmmod - Remove a kernel module
//!
//! This tool unloads a kernel module from the running kernel.
//! Similar to the Linux rmmod command.
//!
//! Usage:
//!   rmmod <module_name>      - Unload a module
//!   rmmod -f <module_name>   - Force unload
//!   rmmod -h                 - Show help

use std::env;
use std::process;

// NexaOS syscall interface for module management
mod kmod_syscalls {
    use std::arch::asm;

    const SYS_DELETE_MODULE: u64 = 176;
    const SYS_GETERRNO: u64 = 201;

    pub fn get_errno() -> i32 {
        let ret: u64;
        unsafe {
            asm!(
                "syscall",
                inout("rax") SYS_GETERRNO => ret,
                in("rdi") 0u64,
                out("rcx") _,
                out("r11") _,
                options(nostack, preserves_flags)
            );
        }
        ret as i32
    }

    /// Unload a module from the kernel
    /// name: null-terminated module name string
    /// flags: unload flags (0x1 = O_NONBLOCK, 0x2 = O_TRUNC for force)
    pub fn delete_module(name: *const u8, flags: u32) -> i32 {
        let ret: u64;
        unsafe {
            asm!(
                "syscall",
                inout("rax") SYS_DELETE_MODULE => ret,
                in("rdi") name as u64,
                in("rsi") flags as u64,
                out("rcx") _,
                out("r11") _,
                options(nostack, preserves_flags)
            );
        }
        
        if ret == u64::MAX || ret > i32::MAX as u64 {
            -1
        } else {
            ret as i32
        }
    }

    pub fn errno_str(errno: i32) -> &'static str {
        match errno {
            1 => "Operation not permitted",
            2 => "No such module",
            11 => "Resource temporarily unavailable (module is busy)",
            16 => "Device or resource busy",
            22 => "Invalid argument",
            _ => "Unknown error",
        }
    }
}

use kmod_syscalls::*;

const O_NONBLOCK: u32 = 0x1;
const O_TRUNC: u32 = 0x2;  // Used as force flag

fn print_usage() {
    eprintln!("rmmod - Remove a kernel module");
    eprintln!();
    eprintln!("Usage: rmmod [OPTIONS] <module_name>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -f, --force     Force removal even if module is in use");
    eprintln!("  -w, --wait      Wait for the module to become unloadable");
    eprintln!("  -v, --verbose   Verbose output");
    eprintln!("  -h, --help      Show this help message");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  <module_name>   Name of the module to remove (without .nkm extension)");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  rmmod ext2");
    eprintln!("  rmmod -f e1000");
    eprintln!();
    eprintln!("Note: Force removal may cause system instability if the module");
    eprintln!("      is still in use. Use with caution.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut module_name: Option<String> = None;
    let mut force = false;
    let mut wait = false;
    let mut verbose = false;
    
    // Parse arguments
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-f" | "--force" => {
                force = true;
            }
            "-w" | "--wait" => {
                wait = true;
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            _ if arg.starts_with('-') => {
                eprintln!("rmmod: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
            _ => {
                if module_name.is_some() {
                    eprintln!("rmmod: too many arguments");
                    print_usage();
                    process::exit(1);
                }
                // Strip .nkm extension if present
                let name = arg.trim_end_matches(".nkm");
                module_name = Some(name.to_string());
            }
        }
    }
    
    let module_name = match module_name {
        Some(name) => name,
        None => {
            eprintln!("rmmod: no module name specified");
            print_usage();
            process::exit(1);
        }
    };
    
    if verbose {
        println!("rmmod: removing module '{}'", module_name);
    }
    
    // Build flags
    let mut flags: u32 = 0;
    if force {
        flags |= O_TRUNC;  // Force flag
        if verbose {
            println!("rmmod: forcing removal");
        }
    }
    if !wait {
        flags |= O_NONBLOCK;  // Non-blocking (don't wait)
    }
    
    // Prepare null-terminated name
    let mut name_bytes = module_name.clone().into_bytes();
    name_bytes.push(0);
    
    // Unload the module
    let ret = delete_module(name_bytes.as_ptr(), flags);
    
    if ret < 0 {
        let errno = get_errno();
        eprintln!("rmmod: failed to remove module '{}': {} (errno={})", 
                  module_name, errno_str(errno), errno);
        
        // Provide additional hints
        match errno {
            2 => {
                eprintln!("Hint: Use 'lsmod' to see loaded modules.");
            }
            16 | 11 => {
                eprintln!("Hint: The module is still in use by the system.");
                if !force {
                    eprintln!("      Use -f to force removal (may cause instability).");
                }
            }
            1 => {
                eprintln!("Hint: Module removal requires root privileges.");
            }
            _ => {}
        }
        process::exit(1);
    }
    
    if verbose {
        println!("rmmod: module '{}' removed successfully", module_name);
    }
}
