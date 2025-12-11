//! System call wrappers for the NexaOS dynamic linker

use core::arch::asm;

use crate::constants::*;

// ============================================================================
// Raw System Call Functions
// ============================================================================

#[inline]
pub unsafe fn syscall1(nr: u64, a1: u64) -> u64 {
    let ret: u64;
    asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline]
pub unsafe fn syscall2(nr: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        in("rsi") a2,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline]
pub unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline]
pub unsafe fn syscall6(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> u64 {
    let ret: u64;
    asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        in("r9") a6,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

// ============================================================================
// High-Level System Call Wrappers
// ============================================================================

pub unsafe fn write(fd: i32, buf: *const u8, len: usize) -> isize {
    syscall3(SYS_WRITE, fd as u64, buf as u64, len as u64) as isize
}

pub unsafe fn exit(code: i32) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {
        asm!("ud2", options(noreturn));
    }
}

/// Open a file and return file descriptor
/// NexaOS sys_open takes (path_ptr, flags, mode) - standard POSIX interface
pub unsafe fn open_file(path: *const u8) -> i64 {
    // Pass path pointer, flags=O_RDONLY(0), mode=0
    syscall3(SYS_OPEN, path as u64, 0, 0) as i64
}

/// Close a file descriptor
pub unsafe fn close_file(fd: i32) {
    syscall1(SYS_CLOSE, fd as u64);
}

/// Read from file
pub unsafe fn read_bytes(fd: i32, buf: *mut u8, len: usize) -> isize {
    syscall3(SYS_READ, fd as u64, buf as u64, len as u64) as isize
}

/// Seek in file
pub unsafe fn lseek(fd: i32, offset: i64, whence: i32) -> i64 {
    syscall3(SYS_LSEEK, fd as u64, offset as u64, whence as u64) as i64
}

/// mmap wrapper
pub unsafe fn mmap(addr: u64, length: u64, prot: u64, flags: u64, fd: i64, offset: u64) -> u64 {
    syscall6(SYS_MMAP, addr, length, prot, flags, fd as u64, offset)
}
