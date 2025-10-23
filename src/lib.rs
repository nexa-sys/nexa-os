#![no_std]

pub mod arch;
pub mod memory;
pub mod serial;
pub mod vga_buffer;

use core::panic::PanicInfo;
use multiboot2::{BootInformation, BootInformationHeader};

pub const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BADB002;  // Multiboot v1
pub const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36d76289; // Multiboot v2

pub fn kernel_main(multiboot_info_address: u64, magic: u32) -> ! {
    // DON'T initialize serial - already done in assembly
    // serial::init();
    
    // Early marker - write directly to serial
    unsafe {
        let port = 0x3F8 as *mut u8;
        port.write_volatile(b'R');
        port.write_volatile(b'U');
        port.write_volatile(b'S');
        port.write_volatile(b'T');
        port.write_volatile(b'\n');
    }
    
    // Try to use serial_println macro
    serial_println!("[NexaOS] Kernel starting...");
    serial_println!("[NexaOS] Multiboot magic: {:#x}", magic);
    serial_println!("[NexaOS] Multiboot info: {:#x}", multiboot_info_address);
    
    // Skip VGA for now
    // vga_buffer::init();

    // Accept both Multiboot v1 and v2
    if magic != MULTIBOOT2_BOOTLOADER_MAGIC && magic != MULTIBOOT_BOOTLOADER_MAGIC {
        serial_println!("[ERROR] Invalid Multiboot magic: {:#x}", magic);
        // Don't use println! yet
        arch::halt_loop();
    }

    serial_println!("[NexaOS] Kernel entry successful.");
    serial_println!("[NexaOS] System halted.");

    // Skip all other initialization for now
    /*
    let boot_info = unsafe {
        BootInformation::load(multiboot_info_address as *const BootInformationHeader)
            .expect("valid multiboot info structure")
    };

    if let Some(cmdline) = boot_info.command_line_tag() {
        match cmdline.cmdline() {
            Ok(text) => println!("Boot cmdline: {}", text),
            Err(err) => serial_println!("[WARN] Failed to parse cmdline: {:?}", err),
        }
    }

    memory::log_memory_overview(&boot_info);

    serial_println!("[NexaOS] Initialization complete.");
    println!("System halted. Enjoy exploring the code!");
    */

    arch::halt_loop()
}

pub fn panic(info: &PanicInfo) -> ! {
    serial_println!("[PANIC] {}", info);
    println!("[PANIC] {}", info);
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
