//! insmod - Insert a kernel module
//!
//! This tool loads a kernel module from a file into the running kernel.
//! Similar to the Linux insmod command.
//!
//! Usage:
//!   insmod <module.nkm>              - Load a module
//!   insmod -f <module.nkm>           - Force load (skip signature check)
//!   insmod <module.nkm> param=value  - Load with parameters
//!   insmod -h                        - Show help

use std::env;
use std::fs;
use std::process;

// NexaOS syscall interface for module management
mod kmod_syscalls {
    use std::arch::asm;

    const SYS_INIT_MODULE: u64 = 175;
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

    /// Load a module into the kernel
    /// module_image: pointer to module binary data
    /// len: length of module binary
    /// param_values: null-terminated parameter string
    /// flags: loading flags (0x1 = FORCE_LOAD)
    pub fn init_module(module_image: *const u8, len: usize, param_values: *const u8, flags: u32) -> i32 {
        let ret: u64;
        unsafe {
            asm!(
                "syscall",
                inout("rax") SYS_INIT_MODULE => ret,
                in("rdi") module_image as u64,
                in("rsi") len as u64,
                in("rdx") param_values as u64,
                in("r10") flags as u64,
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
            2 => "No such file or directory",
            8 => "Exec format error (invalid module format)",
            12 => "Out of memory",
            14 => "Bad address",
            17 => "Module already exists",
            22 => "Invalid argument",
            129 => "Key was rejected (signature verification failed)",
            _ => "Unknown error",
        }
    }
}

use kmod_syscalls::*;

const FLAG_FORCE_LOAD: u32 = 0x1;

fn print_usage() {
    eprintln!("insmod - Insert a kernel module");
    eprintln!();
    eprintln!("Usage: insmod [OPTIONS] <module.nkm> [param=value ...]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -f, --force    Force loading (skip signature verification)");
    eprintln!("  -v, --verbose  Verbose output");
    eprintln!("  -h, --help     Show this help message");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  <module.nkm>   Path to the kernel module file");
    eprintln!("  param=value    Module parameters (optional)");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  insmod ext2.nkm");
    eprintln!("  insmod e1000.nkm debug=1");
    eprintln!("  insmod -f unsigned_module.nkm");
    eprintln!();
    eprintln!("Note: Module signature verification is enforced by default.");
    eprintln!("      Use -f only for development/testing with unsigned modules.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut module_path: Option<String> = None;
    let mut params: Vec<String> = Vec::new();
    let mut force = false;
    let mut verbose = false;
    
    // Parse arguments
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-f" | "--force" => {
                force = true;
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            _ if arg.starts_with('-') => {
                eprintln!("insmod: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
            _ => {
                if module_path.is_none() {
                    module_path = Some(arg.clone());
                } else {
                    // Collect as module parameter
                    params.push(arg.clone());
                }
            }
        }
        i += 1;
    }
    
    let module_path = match module_path {
        Some(p) => p,
        None => {
            eprintln!("insmod: no module path specified");
            print_usage();
            process::exit(1);
        }
    };
    
    // Read module file
    if verbose {
        println!("insmod: loading module from {}", module_path);
    }
    
    let module_data = match fs::read(&module_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("insmod: failed to read {}: {}", module_path, e);
            process::exit(1);
        }
    };
    
    if verbose {
        println!("insmod: module size: {} bytes", module_data.len());
    }
    
    // Build parameter string
    let param_str = if params.is_empty() {
        String::new()
    } else {
        params.join(" ")
    };
    
    if verbose && !param_str.is_empty() {
        println!("insmod: parameters: {}", param_str);
    }
    
    // Prepare null-terminated parameter string
    let mut param_bytes = param_str.into_bytes();
    param_bytes.push(0);
    
    // Set flags
    let flags = if force {
        if verbose {
            println!("insmod: forcing load (skipping signature check)");
        }
        FLAG_FORCE_LOAD
    } else {
        0
    };
    
    // Load the module
    let ret = init_module(
        module_data.as_ptr(),
        module_data.len(),
        param_bytes.as_ptr(),
        flags,
    );
    
    if ret < 0 {
        let errno = get_errno();
        eprintln!("insmod: failed to insert module '{}': {} (errno={})", 
                  module_path, errno_str(errno), errno);
        
        // Provide additional hints
        match errno {
            129 => {
                eprintln!("Hint: The module signature is invalid or missing.");
                eprintln!("      Use -f to force load unsigned modules (not recommended).");
            }
            8 => {
                eprintln!("Hint: The file is not a valid NexaOS kernel module (.nkm).");
            }
            17 => {
                eprintln!("Hint: A module with the same name is already loaded.");
                eprintln!("      Use 'rmmod' to unload it first.");
            }
            12 => {
                eprintln!("Hint: Insufficient kernel memory to load the module.");
            }
            _ => {}
        }
        process::exit(1);
    }
    
    // Extract module name from path for display
    let module_name = module_path
        .rsplit('/')
        .next()
        .unwrap_or(&module_path)
        .trim_end_matches(".nkm");
    
    if verbose {
        println!("insmod: module '{}' loaded successfully", module_name);
    }
}
