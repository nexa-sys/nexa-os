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
                initramfs::init(module_start, module_size);
            }
        } else {
            kwarn!("No initramfs module found, using built-in filesystem");
        }
    } else {
        kwarn!("Multiboot v1 detected; memory overview is not yet supported.");
    }

    // Initialize paging (required for user mode)
    paging::init();

    // Initialize GDT for user/kernel mode
    gdt::init();

    kinfo!("About to call interrupts::init_interrupts()");

    // Initialize interrupts and system calls
    crate::interrupts::init_interrupts();

    kinfo!("interrupts::init() completed successfully");

    // Enable interrupts
    x86_64::instructions::interrupts::enable();

    // Keep all PIC interrupts masked for now to avoid spurious interrupts
    // TODO: Enable timer interrupt after testing syscall and user mode switching
    kinfo!("CPU interrupts enabled, PIC interrupts remain masked");

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
    for (&path) in INIT_PATHS.iter() {
        kinfo!("Trying init file: {}", path);
        try_init_exec!(path);
        if path == "/bin/sh" {
            kfatal!("'/bin/sh' not found in initramfs; cannot continue to user mode.");
            arch::halt_loop();
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
macro_rules! kfatal {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::Fatal, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kerror {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::Error, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kwarn {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::Warn, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kinfo {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::Info, $($arg)*);
    }};
}

#[macro_export]
macro_rules! kdebug {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::Debug, $($arg)*);
    }};
}

#[macro_export]
macro_rules! ktrace {
    ($($arg:tt)*) => {{
        $crate::klog!($crate::logger::LogLevel::Trace, $($arg)*);
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
