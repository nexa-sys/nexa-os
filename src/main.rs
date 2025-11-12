#![no_std]
#![no_main]

use core::panic::PanicInfo;
use nexa_boot_info::BootInfo as UefiBootInfo;

// Force linker to include multiboot header
extern "C" {
    static multiboot_header_start: u8;
}

#[no_mangle]
pub extern "C" fn kmain(multiboot_info_address: u64, multiboot_magic: u32) -> ! {
    // Touch the multiboot header to prevent it from being optimized away
    unsafe {
        core::ptr::read_volatile(&multiboot_header_start as *const u8);
    }

    nexa_os::kernel_main(multiboot_info_address, multiboot_magic)
}

#[no_mangle]
pub extern "C" fn uefi_start(boot_info_ptr: *const nexa_boot_info::BootInfo) -> ! {
    nexa_os::kernel_main_uefi(boot_info_ptr)
}

#[used]
#[no_mangle]
#[link_section = ".nexa.uefi_entry"]
pub static NEXA_UEFI_ENTRY: extern "C" fn(*const UefiBootInfo) -> ! = uefi_start;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    nexa_os::panic(info)
}
