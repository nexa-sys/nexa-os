//! lsmod - List loaded kernel modules
//!
//! This tool displays information about currently loaded kernel modules.
//! Similar to the Linux lsmod command.
//!
//! Usage:
//!   lsmod           - List all loaded modules
//!   lsmod -v        - Verbose output (include type, state, signature)
//!   lsmod -s        - Show module statistics
//!   lsmod -h        - Show help

use std::env;
use std::process;

// NexaOS syscall interface for module management
mod kmod_syscalls {
    use std::arch::asm;

    const SYS_QUERY_MODULE: u64 = 178;
    const SYS_GETERRNO: u64 = 201;

    /// Query module operation types
    pub const QUERY_MODULE_LIST: u32 = 0;
    pub const QUERY_MODULE_STATS: u32 = 4;

    /// Module list entry (matches kernel struct)
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct ModuleListEntry {
        pub name: [u8; 32],
        pub size: u64,
        pub ref_count: u32,
        pub state: u8,
        pub module_type: u8,
        pub signed: u8,
        pub taints: u8,
    }

    impl ModuleListEntry {
        pub fn name_str(&self) -> &str {
            let len = self.name.iter().position(|&c| c == 0).unwrap_or(32);
            std::str::from_utf8(&self.name[..len]).unwrap_or("")
        }

        pub fn state_str(&self) -> &str {
            match self.state {
                0 => "loaded",
                1 => "running",
                2 => "unloading",
                3 => "error",
                _ => "unknown",
            }
        }

        pub fn type_str(&self) -> &str {
            match self.module_type {
                1 => "fs",
                2 => "blk",
                3 => "chr",
                4 => "net",
                _ => "other",
            }
        }
    }

    /// Module statistics (matches kernel struct)
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct ModuleStatistics {
        pub loaded_count: u32,
        pub total_memory: u64,
        pub fs_count: u32,
        pub blk_count: u32,
        pub chr_count: u32,
        pub net_count: u32,
        pub other_count: u32,
        pub symbol_count: u32,
        pub is_tainted: u8,
        pub _reserved: [u8; 3],
        pub taint_string: [u8; 32],
    }

    impl ModuleStatistics {
        pub fn taint_str(&self) -> &str {
            let len = self.taint_string.iter().position(|&c| c == 0).unwrap_or(32);
            std::str::from_utf8(&self.taint_string[..len]).unwrap_or("")
        }
    }

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

    pub fn query_module(operation: u32, name: *const u8, buf: *mut u8, buf_size: usize) -> isize {
        let ret: u64;
        unsafe {
            asm!(
                "syscall",
                inout("rax") SYS_QUERY_MODULE => ret,
                in("rdi") operation as u64,
                in("rsi") name as u64,
                in("rdx") buf as u64,
                in("r10") buf_size as u64,
                out("rcx") _,
                out("r11") _,
                options(nostack, preserves_flags)
            );
        }

        if ret == u64::MAX {
            -1
        } else {
            ret as isize
        }
    }

    pub fn list_modules(entries: &mut [ModuleListEntry]) -> isize {
        query_module(
            QUERY_MODULE_LIST,
            std::ptr::null(),
            entries.as_mut_ptr() as *mut u8,
            entries.len() * std::mem::size_of::<ModuleListEntry>(),
        )
    }

    pub fn get_module_stats(stats: &mut ModuleStatistics) -> bool {
        let ret = query_module(
            QUERY_MODULE_STATS,
            std::ptr::null(),
            stats as *mut _ as *mut u8,
            std::mem::size_of::<ModuleStatistics>(),
        );
        ret > 0
    }
}

use kmod_syscalls::*;

fn print_usage() {
    eprintln!("lsmod - List loaded kernel modules");
    eprintln!();
    eprintln!("Usage: lsmod [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -v       Verbose output (include type, state, signature status)");
    eprintln!("  -s       Show module subsystem statistics");
    eprintln!("  -h       Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  lsmod        List all loaded modules");
    eprintln!("  lsmod -v     Verbose module listing");
    eprintln!("  lsmod -s     Show statistics only");
}

fn print_statistics() {
    let mut stats = ModuleStatistics {
        loaded_count: 0,
        total_memory: 0,
        fs_count: 0,
        blk_count: 0,
        chr_count: 0,
        net_count: 0,
        other_count: 0,
        symbol_count: 0,
        is_tainted: 0,
        _reserved: [0; 3],
        taint_string: [0; 32],
    };

    if !get_module_stats(&mut stats) {
        eprintln!(
            "lsmod: failed to get module statistics (errno={})",
            get_errno()
        );
        process::exit(1);
    }

    println!("=== Kernel Module Statistics ===");
    println!("Kernel: {}", stats.taint_str());
    println!("Loaded modules: {}", stats.loaded_count);
    println!(
        "Total memory: {} bytes ({} KB)",
        stats.total_memory,
        stats.total_memory / 1024
    );
    println!("Kernel symbols: {}", stats.symbol_count);
    println!();
    println!("By type:");
    println!("  Filesystem:    {}", stats.fs_count);
    println!("  Block device:  {}", stats.blk_count);
    println!("  Char device:   {}", stats.chr_count);
    println!("  Network:       {}", stats.net_count);
    println!("  Other:         {}", stats.other_count);
}

fn list_modules_simple() {
    let mut entries = [ModuleListEntry {
        name: [0; 32],
        size: 0,
        ref_count: 0,
        state: 0,
        module_type: 0,
        signed: 0,
        taints: 0,
    }; 64];

    let count = list_modules(&mut entries);
    if count < 0 {
        eprintln!("lsmod: failed to list modules (errno={})", get_errno());
        process::exit(1);
    }

    if count == 0 {
        println!("No modules loaded.");
        return;
    }

    // Print header
    println!("{:<20} {:>8} {:>4}", "Module", "Size", "Used");

    // Print modules
    for entry in entries.iter().take(count as usize) {
        let name = entry.name_str();
        if name.is_empty() {
            continue;
        }
        println!("{:<20} {:>8} {:>4}", name, entry.size, entry.ref_count);
    }
}

fn list_modules_verbose() {
    let mut entries = [ModuleListEntry {
        name: [0; 32],
        size: 0,
        ref_count: 0,
        state: 0,
        module_type: 0,
        signed: 0,
        taints: 0,
    }; 64];

    let count = list_modules(&mut entries);
    if count < 0 {
        eprintln!("lsmod: failed to list modules (errno={})", get_errno());
        process::exit(1);
    }

    if count == 0 {
        println!("No modules loaded.");
        return;
    }

    // Print header
    println!(
        "{:<16} {:>8} {:>4} {:>6} {:>10} {:>8} {:>6}",
        "Module", "Size", "Used", "Type", "State", "Signed", "Taint"
    );

    // Print modules
    for entry in entries.iter().take(count as usize) {
        let name = entry.name_str();
        if name.is_empty() {
            continue;
        }

        let signed_str = match entry.signed {
            0 => "no",
            1 => "yes",
            2 => "invalid",
            _ => "?",
        };

        let taint_str = if entry.taints != 0 { "yes" } else { "no" };

        println!(
            "{:<16} {:>8} {:>4} {:>6} {:>10} {:>8} {:>6}",
            name,
            entry.size,
            entry.ref_count,
            entry.type_str(),
            entry.state_str(),
            signed_str,
            taint_str
        );
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut verbose = false;
    let mut show_stats = false;

    // Parse arguments
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            "-s" | "--stats" => {
                show_stats = true;
            }
            _ if arg.starts_with('-') => {
                eprintln!("lsmod: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
            _ => {
                eprintln!("lsmod: unexpected argument: {}", arg);
                print_usage();
                process::exit(1);
            }
        }
    }

    if show_stats {
        print_statistics();
    } else if verbose {
        list_modules_verbose();
    } else {
        list_modules_simple();
    }
}
