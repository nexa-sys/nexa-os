/// System call handler

/// System call numbers
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_EXIT: u64 = 60;
pub const SYS_GETPID: u64 = 39;

/// Handle system call
pub fn handle_syscall(syscall_num: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    match syscall_num {
        SYS_WRITE => syscall_write(arg1, arg2 as *const u8, arg3 as usize),
        SYS_READ => syscall_read(arg1, arg2 as *mut u8, arg3 as usize),
        SYS_EXIT => syscall_exit(arg1 as i32),
        SYS_GETPID => 100, // Return fake PID
        _ => {
            crate::kwarn!("Unknown syscall: {}", syscall_num);
            !0 // -1
        }
    }
}

/// Write system call
fn syscall_write(fd: u64, buf: *const u8, count: usize) -> u64 {
    if fd == 1 || fd == 2 {
        // stdout or stderr
        let slice = unsafe { core::slice::from_raw_parts(buf, count) };
        if let Ok(s) = core::str::from_utf8(slice) {
            crate::print!("{}", s);
            return count as u64;
        }
    }
    0
}

/// Read system call
fn syscall_read(fd: u64, buf: *mut u8, count: usize) -> u64 {
    if fd == 0 {
        // stdin - read from keyboard
        let slice = unsafe { core::slice::from_raw_parts_mut(buf, count) };
        let read = crate::keyboard::read_line(slice);
        return read as u64;
    }
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
