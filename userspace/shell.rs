#![no_std]
#![no_main]
#![feature(lang_items)]

use core::arch::asm;

const SYS_WRITE: u64 = 1;
const SYS_READ: u64 = 0;
const SYS_EXIT: u64 = 60;

fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("int 0x80", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, lateout("rax") ret);
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

fn print_number(mut n: u64) {
    if n == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 0;
    while n > 0 {
        buf[i] = (n % 10) as u8 + b'0';
        n /= 10;
        i += 1;
    }
    // Reverse
    let mut j = 0;
    while j < i / 2 {
        let temp = buf[j];
        buf[j] = buf[i - 1 - j];
        buf[i - 1 - j] = temp;
        j += 1;
    }
    write(1, buf.as_ptr(), i);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(1);
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() {
    // 显示shell提示符
    print("nexa$ ");
    
    // 简单的shell循环
    loop {
        // 暂时只是循环，稍后添加命令处理
    }
}

#[no_mangle]
pub extern "C" fn memset(dest: *mut u8, val: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        unsafe { *dest.add(i) = val as u8; }
        i += 1;
    }
    dest
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}