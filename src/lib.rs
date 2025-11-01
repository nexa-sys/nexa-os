#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(naked_functions)]
#![feature(global_asm)]

pub mod arch;
pub mod elf;
pub mod fs;
pub mod gdt;
pub mod initramfs;
pub mod interrupts;
pub mod keyboard;
pub mod logger;
pub mod memory;
pub mod paging;
pub mod process;
pub mod serial;
pub mod syscall;
pub mod vga_buffer;

use core::panic::PanicInfo;
use multiboot2::{BootInformation, BootInformationHeader, CommandLineTag};
pub const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BADB002; // Multiboot v1
pub const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36d76289; // Multiboot v2

pub fn kernel_main(multiboot_info_address: u64, magic: u32) -> ! {
    let freq_hz = logger::init();
    let boot_info = unsafe {
        BootInformation::load(multiboot_info_address as *const BootInformationHeader)
            .expect("valid multiboot info structure")
    };

    vga_buffer::init();

    kinfo!("NexaOS kernel bootstrap start");
    kdebug!("Multiboot magic: {:#x}", magic);
    kdebug!("Multiboot info struct at: {:#x}", multiboot_info_address);

    if logger::tsc_frequency_is_guessed() {
        kwarn!(
            "Falling back to default TSC frequency: {}.{:03} MHz",
            freq_hz / 1_000_000,
            (freq_hz % 1_000_000) / 1_000
        );
    } else {
        kinfo!(
            "Detected invariant TSC frequency: {}.{:03} MHz",
            freq_hz / 1_000_000,
            (freq_hz % 1_000_000) / 1_000
        );
    }

    if magic != MULTIBOOT2_BOOTLOADER_MAGIC && magic != MULTIBOOT_BOOTLOADER_MAGIC {
        kerror!("Invalid Multiboot magic value: {:#x}", magic);
        arch::halt_loop();
    }

    if magic == MULTIBOOT2_BOOTLOADER_MAGIC {
        memory::log_memory_overview(&boot_info);

        // Set GS base EARLY to avoid corrupting static variables during initramfs loading
        unsafe {
            // Get GS_DATA address without creating a reference that might corrupt nearby statics
            let gs_data_addr = &raw const crate::initramfs::GS_DATA as *const _ as u64;
            use x86_64::registers::model_specific::Msr;
            Msr::new(0xc0000101).write(gs_data_addr); // GS base
            kinfo!("GS base set to {:#x}", gs_data_addr);
        }

        // Load initramfs from multiboot module if present
        if let Some(modules_tag) = boot_info.module_tags().next() {
            let module_start = modules_tag.start_address() as *const u8;
            let module_size = (modules_tag.end_address() - modules_tag.start_address()) as usize;
            if module_size > 0 {
                kinfo!(
                    "Found initramfs module at {:#x}, size {} bytes",
                    module_start as usize,
                    module_size
                );
                kinfo!("About to call initramfs::init()");
                initramfs::init(module_start, module_size);
                kinfo!("initramfs::init() completed");
            }
        } else {
            kwarn!("No initramfs module found, using built-in filesystem");
        }
    } else {
        kwarn!("Multiboot v1 detected; memory overview is not yet supported.");
    }

    // Initialize paging (required for user mode)
    paging::init();

    // Check INITRAMFS after paging::init()
    {
        let test = crate::initramfs::get().is_some();
        kinfo!("INITRAMFS after paging::init(): {}", test);
    }

    // Initialize GDT for user/kernel mode
    gdt::init();

    // Check INITRAMFS after gdt::init()
    {
        let test = crate::initramfs::get().is_some();
        kinfo!("INITRAMFS after gdt::init(): {}", test);
    }

    kinfo!("About to call interrupts::init_interrupts()");

    // Initialize interrupts and system calls
    crate::interrupts::init_interrupts();

    kinfo!("interrupts::init() completed successfully");

    // Enable interrupts
    // x86_64::instructions::interrupts::enable();

    // Keep all PIC interrupts masked for now to avoid spurious interrupts
    // TODO: Enable timer interrupt after testing syscall and user mode switching
    kinfo!("CPU interrupts disabled, PIC interrupts remain masked");

    // Debug: Check initramfs state before filesystem init
    kinfo!("Checking initramfs state before fs::init()...");
    let initramfs_check = crate::initramfs::get();
    kinfo!("INITRAMFS check before fs::init(): {:?}", initramfs_check.is_some());

    // Initialize filesystem
    fs::init();

    let elapsed_us = logger::boot_time_us();
    kinfo!(
        "Kernel initialization completed in {}.{:03} ms",
        elapsed_us / 1_000,
        elapsed_us % 1_000
    );

    let cmdline = boot_info
        .command_line_tag()
        .map(|tag| {
            tag.cmdline()
                .expect("Invalid command line (not UTF-8 or not null-terminated)")
        })
        .unwrap_or("");
    let cmd_init_path = parse_init_from_cmdline(cmdline).unwrap_or("(none)");

    if cmd_init_path != "(none)" {
        kinfo!("Custom init path: {}", cmd_init_path);
        try_init_exec!(cmd_init_path);
    }

    const INIT_PATHS: &[&str] = &["/sbin/init", "/etc/init", "/bin/init", "/bin/sh"];

    kinfo!("Using default init file list: {}", INIT_PATHS.len());
    kinfo!("Pausing briefly before starting init");
    for &path in INIT_PATHS.iter() {
        kinfo!("Trying init file: {}", path);
        try_init_exec!(path);
        if path == "/bin/sh" {
            kpanic!("'/bin/sh' not found in initramfs; cannot continue to user mode.");
        }
    }

    // Try to load /bin/sh from initramfs and
    // If we reach here, /bin/sh executed successfully, but it should never return
    kfatal!("Unexpected return from user mode process");
    arch::halt_loop()
}

pub fn panic(info: &PanicInfo) -> ! {
    kfatal!("KERNEL PANIC: {}", info);
    
    arch::halt_loop()
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::vga_buffer::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => {{
        $crate::vga_buffer::_print(format_args!($($arg)*));
        $crate::vga_buffer::_print(format_args!("\n"));
    }};
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! serial_println {
    () => { $crate::serial_print!("\n") };
    ($($arg:tt)*) => {{
        $crate::serial::_print(format_args!($($arg)*));
        $crate::serial::_print(format_args!("\n"));
    }};
}

#[macro_export]
macro_rules! klog {
    ($level:expr, $($arg:tt)*) => {{
        $crate::logger::log($level, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! kpanic {
    ($($arg:tt)*) => {{
        // 自动捕获调用位置（仅行列信息，避免对 file() 的潜在未映射访问）
        let loc = core::panic::Location::caller();
        $crate::klog!(
            $crate::logger::LogLevel::PANIC,
            "PANIC at line {} column {} (file path unavailable): {}",
            loc.line(),
            loc.column(),
            format_args!($($arg)*)
        );

        // --- 栈回溯 ---
        {
            use core::arch::asm;
            $crate::klog!($crate::logger::LogLevel::PANIC, "Stack backtrace:");
            unsafe {
                let mut rbp: u64;
                asm!("mov {}, rbp", out(reg) rbp);
                for i in 0..20 {
                    let return_addr = *(rbp.wrapping_add(8) as *const u64);
                    if return_addr == 0 { break; }
                    $crate::klog!($crate::logger::LogLevel::PANIC, "  #{}: {:#018x}", i, return_addr);
                    let next_rbp = *(rbp as *const u64);
                    if next_rbp == 0 || next_rbp < 0x1000 || next_rbp <= rbp { break; }
                    rbp = next_rbp;
                }
            }
        }

        $crate::arch::halt_loop();
    }};
}

#[macro_export]
macro_rules! kfatal {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::FATAL, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kerror {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::ERROR, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kwarn {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::WARN, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kinfo {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::INFO, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kdebug {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::DEBUG, $($arg)*);
    }};
}

#[macro_export]
macro_rules! ktrace {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::TRACE, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {{
        $crate::vga_buffer::_print(format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    ($($arg:tt)*) => {{
        $crate::kprint!($($arg)*);
        $crate::kprint!("\n");
    }};
}

fn parse_init_from_cmdline(cmdline: &str) -> Option<&str> {
    for arg in cmdline.split_whitespace() {
        if let Some(value) = arg.strip_prefix("init=") {
            return Some(value);
        }
    }
    None
}

#[macro_export]
macro_rules! try_init_exec {
    ($path:expr) => {{
        let path: &str = $path; // 强制类型为 &str，提供一定类型安全
        if let Some(init_data) = initramfs::find_file(path) {
            kinfo!(
                "Found init file '{}' in initramfs ({} bytes), loading...",
                path,
                init_data.len()
            );

            // Debug: Check first few bytes of ELF
            if init_data.len() >= 4 {
                kinfo!(
                    "ELF header: {:02x} {:02x} {:02x} {:02x}",
                    init_data[0],
                    init_data[1],
                    init_data[2],
                    init_data[3]
                );
            }

            match process::Process::from_elf(init_data) {
                Ok(mut proc) => {
                    kinfo!("Successfully loaded '{}' as PID {}", path, proc.pid);
                    kinfo!("Switching to REAL user mode (Ring 3)...");
                    proc.execute(); // Never returns
                }
                Err(e) => {
                    kerror!("Failed to load '{}': {}", path, e);
                }
            }
        } else {
            kwarn!("Init file '{}' not found in initramfs", path);
        }
    }};
}
