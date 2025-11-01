use core::arch::global_asm;

/// System call numbers
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_EXIT: u64 = 60;
pub const SYS_GETPID: u64 = 39;

/// Write system call
fn syscall_write(fd: u64, buf: u64, count: u64) -> u64 {
    if fd == 1 {
        for i in 0..count as usize {
            let c = unsafe { *(buf as *const u8).add(i) };
            crate::serial::write_byte(c);
        }
        count
    } else {
        0
    }
}

/// Read system call
fn syscall_read(_fd: u64, _buf: *mut u8, _count: usize) -> u64 {
    0
}

/// Exit system call
fn syscall_exit(code: i32) -> u64 {
    crate::kinfo!("Process exited with code: {}", code);
    // In a real OS, we would switch to another process
    // For now, just halt
    loop {
        x86_64::instructions::hlt();
    }
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(nr: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    // Debug: write syscall number to VGA
    unsafe {
        *(0xB8000 as *mut u64) = 0x4142434445464748; // "ABCDEFGH" in ASCII
        *(0xB8008 as *mut u64) = nr; // Write syscall number
    }

    match nr {
        SYS_WRITE => {
            let ret = syscall_write(arg1, arg2, arg3);
            crate::kinfo!("SYSCALL_WRITE returned: {}", ret);
            ret
        }
        SYS_READ => syscall_read(arg1, arg2 as *mut u8, arg3 as usize),
        SYS_EXIT => syscall_exit(arg1 as i32),
        _ => {
            crate::kinfo!("Unknown syscall: {}", nr);
            0
        }
    }
}

global_asm!(
    ".global syscall_handler",
    "syscall_handler:",
    "push rbx",
    "push rcx", // return address
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11", // rflags
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // Call syscall_dispatch(nr=rax, arg1=rdi, arg2=rsi, arg3=rdx)
    "mov rcx, rdx", // arg3
    "mov rdx, rsi", // arg2
    "mov rsi, rdi", // arg1
    "mov rdi, rax", // nr
    "call syscall_dispatch",
    // Return value is in rax
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "sysretq"
);

