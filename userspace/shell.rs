#![no_std]
#![no_main]

use core::arch::asm;

const SYS_WRITE: u64 = 1;
const SYS_READ: u64 = 0;
const SYS_EXIT: u64 = 60;

fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        //asm!("int 0x80", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, lateout("rax") ret);
        // Try breakpoint interrupt instead
        asm!("int 3", lateout("rax") ret);
    }
    ret
}

fn syscall1(n: u64, a1: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("int 0x80", in("rax") n, in("rdi") a1, lateout("rax") ret);
    }
    ret
}

fn write(fd: u64, buf: *const u8, count: usize) {
    syscall3(SYS_WRITE, fd, buf as u64, count as u64);
}

fn read(fd: u64, buf: *mut u8, count: usize) -> usize {
    syscall3(SYS_READ, fd, buf as u64, count as u64) as usize
}

fn exit(code: i32) {
    syscall1(SYS_EXIT, code as u64);
    loop {} // Should not reach here
}

fn print(s: &str) {
    write(1, s.as_ptr(), s.len());
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(1);
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() {
    // Just exit immediately without any syscalls
    exit(0);
}