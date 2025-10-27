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
use multiboot2::{BootInformation, BootInformationHeader};

pub const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BADB002; // Multiboot v1
pub const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36d76289; // Multiboot v2

pub fn kernel_main(multiboot_info_address: u64, magic: u32) -> ! {
    let freq_hz = logger::init();
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
        let boot_info = unsafe {
            BootInformation::load(multiboot_info_address as *const BootInformationHeader)
                .expect("valid multiboot info structure")
        };

        memory::log_memory_overview(&boot_info);
        
        // Load initramfs from multiboot module if present
        if let Some(modules_tag) = boot_info.module_tags().next() {
            let module_start = modules_tag.start_address() as *const u8;
            let module_size = (modules_tag.end_address() - modules_tag.start_address()) as usize;
            if module_size > 0 {
                kinfo!("Found initramfs module at {:#x}, size {} bytes", 
                    module_start as usize, module_size);
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

    // Try to load /bin/sh from initramfs and execute in user mode
    if let Some(sh_data) = initramfs::find_file("bin/sh") {
        kinfo!("Found /bin/sh in initramfs ({} bytes), loading...", sh_data.len());
        
        // Debug: Check first few bytes of ELF
        if sh_data.len() >= 4 {
            kinfo!("ELF header: {:02x} {:02x} {:02x} {:02x}", 
                sh_data[0], sh_data[1], sh_data[2], sh_data[3]);
        }
        
        match process::Process::from_elf(sh_data) {
            Ok(mut proc) => {
                kinfo!("Successfully loaded /bin/sh as PID {}", proc.pid);
                kinfo!("Switching to REAL user mode (Ring 3)...");
                proc.execute(); // Never returns
            }
            Err(e) => {
                kfatal!("Failed to load /bin/sh: {}", e);
                arch::halt_loop();
            }
        }
    } else {
        kfatal!("No /bin/sh found in initramfs");
        arch::halt_loop();
    }

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
