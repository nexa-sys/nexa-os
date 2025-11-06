#![no_std]
#![feature(lang_items)]

use core::arch::asm;

// Minimal syscall wrappers that mirror the userspace convention (int 0x81)
#[inline(always)]
pub fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
            clobber_abi("sysv64"),
        );
    }
    ret
}

#[inline(always)]
pub fn syscall1(n: u64, a1: u64) -> u64 {
    syscall3(n, a1, 0, 0)
}

// Minimal C runtime helpers
#[no_mangle]
pub extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        let mut i = 0usize;
        while i < n {
            core::ptr::write(dest.add(i), core::ptr::read(src.add(i)));
            i += 1;
        }
        dest
    }
}

#[no_mangle]
pub extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    unsafe {
        let mut i = 0usize;
        while i < n {
            core::ptr::write(s.add(i), c as u8);
            i += 1;
        }
        s
    }
}

#[no_mangle]
pub extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    unsafe {
        let mut i = 0usize;
        while i < n {
            let va = core::ptr::read(a.add(i));
            let vb = core::ptr::read(b.add(i));
            if va != vb {
                return (va as i32) - (vb as i32);
            }
            i += 1;
        }
        0
    }
}

#[no_mangle]
pub extern "C" fn strlen(s: *const u8) -> usize {
    unsafe {
        let mut i = 0usize;
        loop {
            if core::ptr::read(s.add(i)) == 0 {
                return i;
            }
            i += 1;
        }
    }
}

// Minimal abort -> call exit via syscall 60
#[no_mangle]
pub extern "C" fn abort() -> ! {
    const SYS_EXIT: u64 = 60;
    unsafe {
        syscall1(SYS_EXIT, 1);
        loop { asm!("hlt"); }
    }
}

// lang items
#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    abort()
}
