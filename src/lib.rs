#![no_std]

pub mod arch;
pub mod memory;
pub mod serial;
pub mod vga_buffer;

use core::panic::PanicInfo;
use multiboot2::{BootInformation, BootInformationHeader};

pub const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36d76289;

const MULTIBOOT_HEADER_MAGIC: u32 = 0xe85250d6;
const MULTIBOOT_HEADER_ARCHITECTURE: u32 = 0;
const MULTIBOOT_HEADER_LENGTH: u32 = core::mem::size_of::<MultibootHeader>() as u32;
const MULTIBOOT_HEADER_CHECKSUM: u32 = (0u32)
    .wrapping_sub(MULTIBOOT_HEADER_MAGIC + MULTIBOOT_HEADER_ARCHITECTURE + MULTIBOOT_HEADER_LENGTH);

#[repr(C, packed)]
struct MultibootHeader {
    magic: u32,
    architecture: u32,
    length: u32,
    checksum: u32,
    end_tag_type: u16,
    end_tag_flags: u16,
    end_tag_size: u32,
}

#[link_section = ".boot.header"]
#[used]
static MULTIBOOT_HEADER: MultibootHeader = MultibootHeader {
    magic: MULTIBOOT_HEADER_MAGIC,
    architecture: MULTIBOOT_HEADER_ARCHITECTURE,
    length: MULTIBOOT_HEADER_LENGTH,
    checksum: MULTIBOOT_HEADER_CHECKSUM,
    end_tag_type: 0,
    end_tag_flags: 0,
    end_tag_size: 8,
};

pub fn kernel_main(multiboot_info_address: u64, magic: u32) -> ! {
    serial::init();
    vga_buffer::init();

    if magic != MULTIBOOT2_BOOTLOADER_MAGIC {
        serial_println!("Invalid Multiboot magic: {:#x}", magic);
        println!("Invalid Multiboot magic: {:#x}", magic);
        arch::halt_loop();
    }

    serial_println!("[NexaOS] Kernel entry.");
    println!("Welcome to NexaOS kernel bootstrap!");

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
