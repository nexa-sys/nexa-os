//! modinfo - Show information about a kernel module
//!
//! This tool displays detailed information about a loaded kernel module.
//! Similar to the Linux modinfo command.
//!
//! Usage:
//!   modinfo <module>   - Show module information
//!   modinfo -d <mod>   - Show dependencies only
//!   modinfo -s <mod>   - Show exported symbols
//!   modinfo -h         - Show help

use std::env;
use std::process;

// NexaOS syscall interface for module management
mod kmod_syscalls {
    use std::arch::asm;

    const SYS_QUERY_MODULE: u64 = 178;
    const SYS_GETERRNO: u64 = 201;

    /// Query module operation types
    pub const QUERY_MODULE_INFO: u32 = 1;
    pub const QUERY_MODULE_DEPS: u32 = 3;
    pub const QUERY_MODULE_SYMBOLS: u32 = 5;

    /// Module detailed info (matches kernel struct)
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct ModuleDetailedInfo {
        pub name: [u8; 32],
        pub version: [u8; 32],
        pub description: [u8; 128],
        pub author: [u8; 64],
        pub license: [u8; 32],
        pub size: u64,
        pub base_addr: u64,
        pub ref_count: u32,
        pub dep_count: u32,
        pub symbol_count: u32,
        pub param_count: u32,
        pub state: u8,
        pub module_type: u8,
        pub signed: u8,
        pub taints: u8,
    }

    impl ModuleDetailedInfo {
        pub fn name_str(&self) -> &str {
            let len = self.name.iter().position(|&c| c == 0).unwrap_or(32);
            std::str::from_utf8(&self.name[..len]).unwrap_or("")
        }

        pub fn version_str(&self) -> &str {
            let len = self.version.iter().position(|&c| c == 0).unwrap_or(32);
            std::str::from_utf8(&self.version[..len]).unwrap_or("")
        }

        pub fn description_str(&self) -> &str {
            let len = self.description.iter().position(|&c| c == 0).unwrap_or(128);
            std::str::from_utf8(&self.description[..len]).unwrap_or("")
        }

        pub fn author_str(&self) -> &str {
            let len = self.author.iter().position(|&c| c == 0).unwrap_or(64);
            std::str::from_utf8(&self.author[..len]).unwrap_or("")
        }

        pub fn license_str(&self) -> &str {
            let len = self.license.iter().position(|&c| c == 0).unwrap_or(32);
            std::str::from_utf8(&self.license[..len]).unwrap_or("")
        }
    }

    /// Module dependency entry
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct ModuleDependency {
        pub name: [u8; 32],
    }

    impl ModuleDependency {
        pub fn name_str(&self) -> &str {
            let len = self.name.iter().position(|&c| c == 0).unwrap_or(32);
            std::str::from_utf8(&self.name[..len]).unwrap_or("")
        }
    }

    /// Module symbol entry
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct ModuleSymbol {
        pub name: [u8; 64],
        pub address: u64,
        pub sym_type: u8,
        pub gpl_only: u8,
        pub _reserved: [u8; 6],
    }

    impl ModuleSymbol {
        pub fn name_str(&self) -> &str {
            let len = self.name.iter().position(|&c| c == 0).unwrap_or(64);
            std::str::from_utf8(&self.name[..len]).unwrap_or("")
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

    pub fn get_module_info(name: &[u8], info: &mut ModuleDetailedInfo) -> bool {
        let ret = query_module(
            QUERY_MODULE_INFO,
            name.as_ptr(),
            info as *mut _ as *mut u8,
            std::mem::size_of::<ModuleDetailedInfo>(),
        );
        ret > 0
    }

    pub fn get_module_deps(name: &[u8], deps: &mut [ModuleDependency]) -> isize {
        query_module(
            QUERY_MODULE_DEPS,
            name.as_ptr(),
            deps.as_mut_ptr() as *mut u8,
            deps.len() * std::mem::size_of::<ModuleDependency>(),
        )
    }

    pub fn get_module_symbols(name: &[u8], syms: &mut [ModuleSymbol]) -> isize {
        query_module(
            QUERY_MODULE_SYMBOLS,
            name.as_ptr(),
            syms.as_mut_ptr() as *mut u8,
            syms.len() * std::mem::size_of::<ModuleSymbol>(),
        )
    }
}

use kmod_syscalls::*;

fn print_usage() {
    eprintln!("modinfo - Show information about a kernel module");
    eprintln!();
    eprintln!("Usage: modinfo [OPTIONS] <module>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -d       Show module dependencies");
    eprintln!("  -s       Show exported symbols");
    eprintln!("  -a       Show all information (default)");
    eprintln!("  -h       Show this help message");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  module   Name of the loaded module to query");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  modinfo ext2       Show ext2 module information");
    eprintln!("  modinfo -d ext2    Show ext2 dependencies");
    eprintln!("  modinfo -s e1000   Show e1000 exported symbols");
}

fn format_size(size: u64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else if size >= 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{} bytes", size)
    }
}

fn show_module_info(name: &str) {
    let mut name_bytes = name.as_bytes().to_vec();
    name_bytes.push(0);

    let mut info = ModuleDetailedInfo {
        name: [0; 32],
        version: [0; 32],
        description: [0; 128],
        author: [0; 64],
        license: [0; 32],
        size: 0,
        base_addr: 0,
        ref_count: 0,
        dep_count: 0,
        symbol_count: 0,
        param_count: 0,
        state: 0,
        module_type: 0,
        signed: 0,
        taints: 0,
    };

    if !get_module_info(&name_bytes, &mut info) {
        eprintln!(
            "modinfo: module '{}' not found (errno={})",
            name,
            get_errno()
        );
        process::exit(1);
    }

    let type_str = match info.module_type {
        1 => "filesystem",
        2 => "block device",
        3 => "character device",
        4 => "network",
        _ => "other",
    };

    let state_str = match info.state {
        0 => "loaded",
        1 => "running",
        2 => "unloading",
        3 => "error",
        _ => "unknown",
    };

    let signed_str = match info.signed {
        0 => "unsigned",
        1 => "signed (valid)",
        2 => "signed (invalid)",
        _ => "unknown",
    };

    println!("filename:        {}.nkm", info.name_str());
    println!("name:            {}", info.name_str());
    println!("version:         {}", info.version_str());
    println!("description:     {}", info.description_str());
    println!("author:          {}", info.author_str());
    println!("license:         {}", info.license_str());
    println!("type:            {}", type_str);
    println!("size:            {}", format_size(info.size));
    println!("base_address:    {:#x}", info.base_addr);
    println!("refcount:        {}", info.ref_count);
    println!("state:           {}", state_str);
    println!("signed:          {}", signed_str);
    println!(
        "taints_kernel:   {}",
        if info.taints != 0 { "yes" } else { "no" }
    );
    println!("dependencies:    {}", info.dep_count);
    println!("symbols:         {}", info.symbol_count);
    println!("parameters:      {}", info.param_count);
}

fn show_dependencies(name: &str) {
    let mut name_bytes = name.as_bytes().to_vec();
    name_bytes.push(0);

    let mut deps = [ModuleDependency { name: [0; 32] }; 16];

    let count = get_module_deps(&name_bytes, &mut deps);
    if count < 0 {
        eprintln!(
            "modinfo: module '{}' not found (errno={})",
            name,
            get_errno()
        );
        process::exit(1);
    }

    println!("Dependencies for '{}':", name);
    if count == 0 {
        println!("  (none)");
    } else {
        for dep in deps.iter().take(count as usize) {
            let dep_name = dep.name_str();
            if !dep_name.is_empty() {
                println!("  - {}", dep_name);
            }
        }
    }
}

fn show_symbols(name: &str) {
    let mut name_bytes = name.as_bytes().to_vec();
    name_bytes.push(0);

    let mut syms = [ModuleSymbol {
        name: [0; 64],
        address: 0,
        sym_type: 0,
        gpl_only: 0,
        _reserved: [0; 6],
    }; 64];

    let count = get_module_symbols(&name_bytes, &mut syms);
    if count < 0 {
        eprintln!(
            "modinfo: module '{}' not found (errno={})",
            name,
            get_errno()
        );
        process::exit(1);
    }

    println!("Exported symbols for '{}':", name);
    if count == 0 {
        println!("  (none)");
    } else {
        println!(
            "{:<40} {:>16}  {:>8}  {:>8}",
            "Symbol", "Address", "Type", "GPL"
        );
        println!("{}", "-".repeat(76));

        for sym in syms.iter().take(count as usize) {
            let sym_name = sym.name_str();
            if !sym_name.is_empty() {
                let type_str = if sym.sym_type == 0 { "func" } else { "data" };
                let gpl_str = if sym.gpl_only != 0 { "yes" } else { "no" };
                println!(
                    "{:<40} {:#16x}  {:>8}  {:>8}",
                    sym_name, sym.address, type_str, gpl_str
                );
            }
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("modinfo: missing module name");
        eprintln!("Usage: modinfo [OPTIONS] <module>");
        process::exit(1);
    }

    let mut show_deps = false;
    let mut show_syms = false;
    let mut module_name: Option<&str> = None;

    // Parse arguments
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-d" | "--deps" => {
                show_deps = true;
            }
            "-s" | "--symbols" => {
                show_syms = true;
            }
            "-a" | "--all" => {
                // Default behavior
            }
            _ if arg.starts_with('-') => {
                eprintln!("modinfo: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
            _ => {
                if module_name.is_some() {
                    eprintln!("modinfo: too many arguments");
                    process::exit(1);
                }
                module_name = Some(arg);
            }
        }
    }

    let module_name = match module_name {
        Some(name) => name,
        None => {
            eprintln!("modinfo: missing module name");
            eprintln!("Usage: modinfo [OPTIONS] <module>");
            process::exit(1);
        }
    };

    if show_deps {
        show_dependencies(module_name);
    } else if show_syms {
        show_symbols(module_name);
    } else {
        show_module_info(module_name);
        println!();
        show_dependencies(module_name);
    }
}
