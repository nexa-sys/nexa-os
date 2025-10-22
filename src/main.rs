#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[no_mangle]
pub extern "C" fn kmain(multiboot_info_address: u64, multiboot_magic: u32) -> ! {
    nexa_os::kernel_main(multiboot_info_address, multiboot_magic)
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    nexa_os::panic(info)
}
