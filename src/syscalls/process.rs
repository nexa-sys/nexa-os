//! Process management related syscalls
//!
//! Implements: fork, execve, exit, wait4, getpid, getppid, kill

use super::exec::set_exec_context;
use super::types::*;
use crate::posix::{self, FileType};
use crate::process::{Process, ProcessState, USER_REGION_SIZE, USER_VIRT_BASE};
use crate::scheduler;
use crate::{kdebug, kerror, kfatal, kinfo, kpanic, ktrace, kwarn};
use alloc::alloc::{alloc, dealloc, Layout};
use core::sync::atomic::Ordering;

/// Exit system call - terminate current process
pub fn exit(code: i32) -> ! {
    let pid = crate::scheduler::current_pid().unwrap_or(0);
    ktrace!("[SYS_EXIT] PID {} exiting with code: {}", pid, code);
    kinfo!("Process {} exiting with code: {}", pid, code);

    if pid == 0 {
        kpanic!("Cannot exit from kernel context (PID 0)!");
    }

    if let Err(e) = crate::scheduler::set_process_exit_code(pid, code) {
        kerror!("Failed to record exit code for PID {}: {}", pid, e);
    }

    ktrace!("[SYS_EXIT] Setting PID {} to Zombie state", pid);
    let _ = crate::scheduler::set_process_state(pid, crate::process::ProcessState::Zombie);

    kinfo!("Process {} marked as zombie, yielding to scheduler", pid);

    crate::scheduler::do_schedule();

    kpanic!("Zombie process {} still running after do_schedule()!", pid);
}

/// POSIX getppid() system call - get parent process ID
pub fn getppid() -> u64 {
    if let Some(pid) = crate::scheduler::get_current_pid() {
        if let Some(process) = crate::scheduler::get_process(pid) {
            posix::set_errno(0);
            return process.ppid;
        }
    }
    posix::set_errno(posix::errno::ESRCH);
    u64::MAX
}

/// POSIX fork() system call - create child process
pub fn fork(syscall_return_addr: u64) -> u64 {
    use crate::process::{INTERP_BASE, INTERP_REGION_SIZE, USER_VIRT_BASE};

    kdebug!(
        "syscall_fork: syscall_return_addr = {:#x}",
        syscall_return_addr
    );

    let user_rsp: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[0]",
            out(reg) user_rsp
        );
    }

    ktrace!("[fork] Retrieved user_rsp from GS_DATA: {:#x}", user_rsp);

    let current_pid = match crate::scheduler::get_current_pid() {
        Some(pid) => pid,
        None => {
            kerror!("fork() called but no current process");
            return u64::MAX;
        }
    };

    let parent_process = match crate::scheduler::get_process(current_pid) {
        Some(proc) => proc,
        None => {
            kerror!("fork() - current process {} not found", current_pid);
            return u64::MAX;
        }
    };

    kinfo!("fork() called from PID {}", current_pid);
    ktrace!("[fork] PID {} calling fork", current_pid);

    let parent_pid = current_pid;
    let child_pid = crate::process::allocate_pid();

    ktrace!("[fork] Allocated child PID {}", child_pid);

    let mut child_process = parent_process;
    child_process.pid = child_pid;
    child_process.ppid = current_pid;
    child_process.state = crate::process::ProcessState::Ready;
    child_process.has_entered_user = false;
    child_process.is_fork_child = true;
    child_process.exit_code = 0;

    let kernel_stack_layout = Layout::from_size_align(
        crate::process::KERNEL_STACK_SIZE,
        crate::process::KERNEL_STACK_ALIGN,
    )
    .unwrap();
    let kernel_stack = unsafe { alloc(kernel_stack_layout) } as u64;
    if kernel_stack == 0 {
        kerror!("fork() - failed to allocate kernel stack");
        return u64::MAX;
    }
    child_process.kernel_stack = kernel_stack;

    child_process.entry_point = syscall_return_addr;
    child_process.stack_top = user_rsp;
    child_process.user_rip = syscall_return_addr;
    child_process.user_rsp = user_rsp;
    child_process.user_rflags = parent_process.user_rflags;

    child_process.context = crate::process::Context::zero();
    child_process.context.rax = 0;
    child_process.context.rip = syscall_return_addr;
    child_process.context.rsp = user_rsp;

    kdebug!(
        "Child fork: entry={:#x}, stack={:#x}, will return RAX=0",
        child_process.entry_point,
        child_process.stack_top
    );

    ktrace!(
        "[fork] User RSP (from GS_DATA)={:#x}, Kernel RSP={:#x}, Child stack_top={:#x}",
        user_rsp,
        parent_process.context.rsp,
        child_process.stack_top
    );

    let memory_size = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
    kinfo!(
        "fork() - copying {:#x} bytes ({} KB) from {:#x}",
        memory_size,
        memory_size / 1024,
        USER_VIRT_BASE
    );

    ktrace!("[fork] Copying {} KB of memory", memory_size / 1024);

    let child_phys_base = match crate::paging::allocate_user_region(memory_size) {
        Some(addr) => addr,
        None => {
            kerror!("fork() - failed to allocate memory for child process");
            return u64::MAX;
        }
    };

    kdebug!(
        "fork() - allocated child memory at physical {:#x}",
        child_phys_base
    );

    let parent_phys_base = parent_process.memory_base;

    ktrace!(
        "[fork] Parent PID {} phys_base={:#x}, child PID {} phys_base={:#x}",
        parent_pid,
        parent_phys_base,
        child_pid,
        child_phys_base
    );
    ktrace!(
        "[fork] Copying {} KB from parent to child",
        memory_size / 1024
    );

    unsafe {
        let test_addr = 0x9fe390u64 as *const u8;
        let test_bytes = core::slice::from_raw_parts(test_addr, 16);
        ktrace!(
            "[fork-debug] Parent mem at path_buf (0x9fe390): {:02x?}",
            test_bytes
        );
    }

    unsafe {
        let src_ptr = USER_VIRT_BASE as *const u8;
        let dst_ptr = child_phys_base as *mut u8;

        if dst_ptr as u64 + memory_size > 0x1_0000_0000 {
            kerror!(
                "fork: Child physical address {:#x} + size {:#x} exceeds physical limit!",
                child_phys_base,
                memory_size
            );
            return u64::MAX;
        }

        ktrace!(
            "[fork] VALIDATED: Copying from VIRT {:#x} to PHYS {:#x}, size {:#x}",
            src_ptr as u64,
            dst_ptr as u64,
            memory_size
        );

        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, memory_size as usize);

        let verify_src = core::ptr::read(src_ptr);
        let verify_dst = core::ptr::read(dst_ptr);

        if verify_src != verify_dst {
            kerror!(
                "fork: Memory copy verification FAILED! src_byte={:#x}, dst_byte={:#x}",
                verify_src,
                verify_dst
            );
            kfatal!("Fork memory copy corrupted");
        }

        let child_test_addr = (child_phys_base + (0x9fe390 - USER_VIRT_BASE)) as *const u8;
        let child_test_bytes = core::slice::from_raw_parts(child_test_addr, 16);
        ktrace!(
            "[fork-debug] Child phys mem at path_buf (0x9fe390): {:02x?}",
            child_test_bytes
        );

        use x86_64::instructions::tlb::flush_all;
        ktrace!("[fork] Flushing TLB after memory copy");
        flush_all();
    }

    kinfo!(
        "fork() - memory copied successfully, {} KB",
        memory_size / 1024
    );

    ktrace!("[fork] Memory copied successfully");

    child_process.memory_base = child_phys_base;
    child_process.memory_size = memory_size;

    match crate::paging::create_process_address_space(child_phys_base, memory_size) {
        Ok(cr3) => {
            if let Err(e) = crate::paging::validate_cr3(cr3, false) {
                kerror!(
                    "fork: create_process_address_space() returned invalid CR3 {:#x}: {}",
                    cr3,
                    e
                );
                kfatal!("Failed to create valid page tables for child");
            }

            child_process.cr3 = cr3;

            ktrace!(
                "[fork] VALIDATED CR3: {:#x} for child PID {}",
                cr3,
                child_pid
            );
        }
        Err(err) => {
            kerror!(
                "fork() - failed to build page tables for child {}: {}",
                child_pid,
                err
            );
            kfatal!("Page table creation failed");
        }
    }

    ktrace!(
        "[fork] Child memory_base={:#x}, memory_size={:#x}",
        child_phys_base,
        memory_size
    );

    kdebug!("fork() - FD table shared (TODO: implement copy)");

    if let Err(e) = crate::scheduler::add_process(child_process, 128) {
        kerror!("fork() - failed to add child process: {}", e);
        return u64::MAX;
    }

    kinfo!(
        "fork() created child PID {} from parent PID {} (child will return 0)",
        child_pid,
        current_pid
    );

    child_pid
}

/// Copy a C string from userspace
fn copy_user_c_string(ptr: *const u8, buffer: &mut [u8]) -> Result<usize, ()> {
    if ptr.is_null() {
        return Err(());
    }

    let mut len = 0usize;
    while len < buffer.len() {
        let byte = unsafe { ptr.add(len).read() };
        if byte == 0 {
            return Ok(len);
        }
        buffer[len] = byte;
        len += 1;
    }
    Err(())
}

/// POSIX execve() system call - execute program
pub fn execve(path: *const u8, _argv: *const *const u8, _envp: *const *const u8) -> u64 {
    use crate::scheduler::get_current_pid;

    ktrace!("[syscall_execve] Called");

    if path.is_null() {
        ktrace!("[syscall_execve] Error: path is null");
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    const MAX_EXEC_PATH_LEN: usize = 256;
    const MAX_EXEC_ARGS: usize = crate::process::MAX_PROCESS_ARGS;
    const MAX_EXEC_ARG_LEN: usize = 256;

    let mut path_buf = [0u8; MAX_EXEC_PATH_LEN];
    let path_len = match copy_user_c_string(path, &mut path_buf) {
        Ok(len) => len,
        Err(_) => {
            ktrace!("[syscall_execve] Error: path too long or invalid");
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let path_slice = &path_buf[..path_len];
    let path_str = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => {
            ktrace!("[syscall_execve] Error: invalid UTF-8 in path");
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let mut argv_storage = [[0u8; MAX_EXEC_ARG_LEN]; MAX_EXEC_ARGS];
    let mut arg_lengths = [0usize; MAX_EXEC_ARGS];
    let mut arg_index = 0usize;

    if !_argv.is_null() {
        loop {
            let arg_ptr = unsafe { *_argv.add(arg_index) };
            if arg_ptr.is_null() {
                break;
            }

            if arg_index >= MAX_EXEC_ARGS {
                ktrace!(
                    "[syscall_execve] Error: too many arguments (> {})",
                    MAX_EXEC_ARGS
                );
                posix::set_errno(posix::errno::E2BIG);
                return u64::MAX;
            }

            let len;
            {
                let storage = &mut argv_storage[arg_index];
                len = match copy_user_c_string(arg_ptr, storage) {
                    Ok(len) => len,
                    Err(_) => {
                        posix::set_errno(posix::errno::E2BIG);
                        return u64::MAX;
                    }
                };
            }

            arg_lengths[arg_index] = len;
            arg_index += 1;
        }
    }

    let mut argv_refs: [&[u8]; MAX_EXEC_ARGS] = [&[]; MAX_EXEC_ARGS];
    for i in 0..arg_index {
        argv_refs[i] = &argv_storage[i][..arg_lengths[i]];
    }

    let argv_list = &argv_refs[..arg_index];

    ktrace!("[syscall_execve] Path: {}", path_str);
    ktrace!("[syscall_execve] argc={}", argv_list.len());
    for (i, arg) in argv_list.iter().enumerate() {
        let disp = core::str::from_utf8(arg).unwrap_or("<non-utf8>");
        ktrace!("  argv[{}] = {}", i, disp);
    }

    let exec_path_bytes = path_slice;

    let elf_data = match crate::fs::read_file_bytes(path_str) {
        Some(data) => {
            ktrace!("[syscall_execve] Found file, {} bytes", data.len());
            data
        }
        None => {
            ktrace!("[syscall_execve] Error: file not found: {}", path_str);
            posix::set_errno(posix::errno::ENOENT);
            return u64::MAX;
        }
    };

    let current_pid = match get_current_pid() {
        Some(pid) => pid,
        None => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    let (current_memory_base, current_cr3) = {
        let table = crate::scheduler::process_table_lock();
        let mut result = None;

        for slot in table.iter() {
            if let Some(entry) = slot {
                if entry.process.pid == current_pid {
                    result = Some((entry.process.memory_base, entry.process.cr3));
                    break;
                }
            }
        }

        match result {
            Some(values) => values,
            None => {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }
        }
    };

    ktrace!(
        "[syscall_execve] Current process memory_base={:#x}, cr3={:#x}",
        current_memory_base,
        current_cr3
    );

    let new_process = match crate::process::Process::from_elf_with_args_at_base(
        elf_data,
        argv_list,
        Some(exec_path_bytes),
        current_memory_base,
        current_cr3,
    ) {
        Ok(proc) => {
            ktrace!(
                "[syscall_execve] Successfully loaded ELF at existing base, entry={:#x}, stack={:#x}",
                proc.entry_point,
                proc.stack_top
            );
            proc
        }
        Err(e) => {
            ktrace!("[syscall_execve] Error loading ELF: {}", e);
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    // Update process table with new image
    {
        let mut table = crate::scheduler::process_table_lock();
        let mut found = false;

        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == current_pid {
                    found = true;

                    ktrace!(
                        "[syscall_execve] Updating PID {} in table: old_cr3={:#x}, new_cr3={:#x}",
                        current_pid,
                        entry.process.cr3,
                        new_process.cr3
                    );

                    entry.process.entry_point = new_process.entry_point;
                    entry.process.stack_top = new_process.stack_top;
                    entry.process.heap_start = new_process.heap_start;
                    entry.process.heap_end = new_process.heap_end;
                    entry.process.context = new_process.context;
                    entry.process.cr3 = new_process.cr3;
                    entry.process.memory_base = new_process.memory_base;
                    entry.process.memory_size = new_process.memory_size;
                    entry.process.user_rip = new_process.entry_point;
                    entry.process.user_rsp = new_process.stack_top;
                    entry.process.user_rflags = 0x202;
                    entry.process.exit_code = 0;

                    ktrace!(
                        "[syscall_execve] Updated: entry={:#x}, stack={:#x}, cr3={:#x}, has_entered_user={}",
                        entry.process.entry_point,
                        entry.process.stack_top,
                        entry.process.cr3,
                        entry.process.has_entered_user
                    );

                    entry.process.signal_state.reset_to_default();
                    break;
                }
            }
        }

        if !found {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    }

    let user_data_sel = unsafe {
        let selectors = crate::gdt::get_selectors();
        let sel = selectors.user_data_selector.0 as u64;
        sel | 3
    };

    set_exec_context(new_process.entry_point, new_process.stack_top, user_data_sel);

    // Return magic value to signal exec
    0x4558454300000000
}

/// POSIX wait4() system call - wait for process state change
pub fn wait4(pid: i64, status: *mut i32, options: i32, _rusage: *mut u8) -> u64 {
    kinfo!("wait4(pid={}, options={:#x}) called", pid, options);

    let current_pid = match crate::scheduler::get_current_pid() {
        Some(pid) => pid,
        None => {
            kerror!("wait4() called but no current process");
            posix::set_errno(posix::errno::ESRCH);
            return u64::MAX;
        }
    };

    kinfo!("wait4() from PID {} waiting for pid {}", current_pid, pid);

    const WNOHANG: i32 = 1;
    const WUNTRACED: i32 = 2;
    const WCONTINUED: i32 = 8;

    let is_nonblocking = (options & WNOHANG) != 0;

    if (options & !(WNOHANG | WUNTRACED | WCONTINUED)) != 0 {
        kerror!("wait4: unsupported options {:#x}", options);
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if pid == 0 || pid < -1 {
        kerror!(
            "wait4: process group waiting (pid={}) not yet implemented",
            pid
        );
        posix::set_errno(posix::errno::ENOSYS);
        return u64::MAX;
    }

    let mut loop_count = 0;
    loop {
        loop_count += 1;
        ktrace!(
            "[wait4] ===== Loop {} BEGIN: PID {} waiting for pid {} =====",
            loop_count,
            current_pid,
            pid
        );

        let mut found_any_child = false;
        let mut child_exited = false;
        let mut child_exit_code: Option<i32> = None;
        let mut child_term_signal: Option<i32> = None;
        let mut wait_pid = 0u64;

        if pid == -1 {
            for check_pid in 2..32 {
                if let Some(child_state) = crate::scheduler::get_child_state(current_pid, check_pid)
                {
                    found_any_child = true;

                    if child_state == crate::process::ProcessState::Zombie {
                        wait_pid = check_pid;
                        child_exited = true;

                        if let Some(proc) = crate::scheduler::get_process(check_pid) {
                            child_exit_code = Some(proc.exit_code);
                            child_term_signal = None;
                        } else {
                            child_exit_code = Some(0);
                        }

                        kinfo!("wait4() found exited child PID {}", check_pid);
                        break;
                    }
                }
            }
        } else if pid > 0 {
            if loop_count <= 3 || (loop_count % 100 == 0) {
                ktrace!(
                    "[wait4] Loop {}: Checking if PID {} is child of PID {}",
                    loop_count,
                    pid,
                    current_pid
                );
            }

            if let Some(child_state) = crate::scheduler::get_child_state(current_pid, pid as u64) {
                found_any_child = true;
                wait_pid = pid as u64;

                if loop_count <= 3 || (loop_count % 100 == 0) {
                    ktrace!(
                        "[wait4] Loop {}: PID {} found, state={:?}",
                        loop_count,
                        pid,
                        child_state
                    );
                }

                if child_state == crate::process::ProcessState::Zombie {
                    child_exited = true;

                    if let Some(proc) = crate::scheduler::get_process(wait_pid) {
                        child_exit_code = Some(proc.exit_code);
                        child_term_signal = None;
                    } else {
                        child_exit_code = Some(0);
                    }

                    kinfo!("wait4() found exited specific child PID {}", pid);
                }
            }
        }

        if child_exited {
            ktrace!(
                "[wait4] Child {} exited, found_any_child={}, proceeding to cleanup",
                wait_pid,
                found_any_child
            );

            let encoded_status = if let Some(signal) = child_term_signal {
                signal & 0x7f
            } else {
                let exit_code = child_exit_code.unwrap_or(0);
                (exit_code & 0xff) << 8
            };

            if !status.is_null() {
                unsafe {
                    *status = encoded_status;
                }
            }

            if let Err(e) = crate::scheduler::remove_process(wait_pid) {
                kerror!("wait4: Failed to remove process {}: {}", wait_pid, e);
            }

            crate::init::handle_process_exit(wait_pid, child_exit_code.unwrap_or(0));

            kinfo!(
                "wait4() returning child PID {} with encoded status {:#x}",
                wait_pid,
                encoded_status
            );
            posix::set_errno(0);

            ktrace!(
                "[wait4] Returning to PID {} with child PID {}",
                current_pid,
                wait_pid
            );
            return wait_pid;
        }

        if !found_any_child {
            ktrace!(
                "[wait4] No matching child found for PID {} (pid arg={}) - returning ECHILD",
                current_pid,
                pid
            );
            kinfo!("wait4() no matching child found");
            posix::set_errno(posix::errno::ECHILD);
            return u64::MAX;
        }

        if is_nonblocking {
            kinfo!("wait4() WNOHANG: child not yet exited");
            posix::set_errno(0);
            return 0;
        }

        if loop_count <= 3 || (loop_count % 100 == 0) {
            ktrace!(
                "[wait4] Loop {}: Child not ready, calling do_schedule()",
                loop_count
            );
        }
        crate::scheduler::do_schedule();
        if loop_count <= 3 || (loop_count % 100 == 0) {
            ktrace!(
                "[wait4] Loop {}: Returned from do_schedule(), re-checking child state",
                loop_count
            );
        }
    }
}

/// POSIX kill() system call - send signal to process
pub fn kill(pid: u64, signum: u64) -> u64 {
    if signum >= crate::signal::NSIG as u64 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    kinfo!("kill(pid={}, sig={}) called", pid, signum);

    posix::set_errno(0);
    0
}
