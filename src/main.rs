#![no_std]
#![no_main]

use core::panic::PanicInfo;

// Force linker to include multiboot header
extern "C" {
    static multiboot_header_start: u8;
}

#[no_mangle]
pub extern "C" fn kmain(_multiboot_info_address: u64, _multiboot_magic: u32) -> ! {
    // Touch the multiboot header to prevent it from being optimized away
    unsafe {
        core::ptr::read_volatile(&multiboot_header_start as *const u8);
    }
    
    // Absolute minimal - just write to serial and halt
    unsafe {
        let port = 0x3F8 as *mut u8;
        port.write_volatile(b'M');
        port.write_volatile(b'A');
        port.write_volatile(b'I');
        port.write_volatile(b'N');
        port.write_volatile(b'\n');
    }
    
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}
