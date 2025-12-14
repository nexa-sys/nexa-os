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

/// Exit system call - terminate current process or thread
///
/// For threads (created with CLONE_THREAD):
/// - Only terminates the calling thread, not the entire process
/// - Handles CLONE_CHILD_CLEARTID: clears tid at clear_child_tid address
/// - Wakes any threads waiting on that futex (for pthread_join)
///
/// For processes (main thread or non-threaded process):
/// - Terminates the entire process
pub fn exit(code: i32) -> ! {
    let pid = crate::scheduler::current_pid().unwrap_or(0);
    kinfo!("[SYS_EXIT] PID {} exiting with code: {}", pid, code);

    if pid == 0 {
        kpanic!("Cannot exit from kernel context (PID 0)!");
    }

    // Handle thread exit: clear_child_tid and futex wake
    if let Some(futex_addr) = crate::scheduler::handle_thread_exit(pid) {
        // Wake any threads waiting on this futex (e.g., pthread_join)
        ktrace!(
            "[SYS_EXIT] Waking waiters on futex {:#x} for PID {}",
            futex_addr,
            pid
        );
        super::thread::futex_wake_internal(futex_addr, i32::MAX);
    }

    // Check if this is a thread or the main process
    let is_thread = crate::scheduler::is_thread(pid);

    if let Err(e) = crate::scheduler::set_process_exit_code(pid, code) {
        kerror!("Failed to record exit code for PID {}: {}", pid, e);
    }

    ktrace!(
        "[SYS_EXIT] Setting PID {} to Zombie state (is_thread={})",
        pid,
        is_thread
    );
    let _ = crate::scheduler::set_process_state(pid, crate::process::ProcessState::Zombie);

    // Wake up parent process if it's sleeping (waiting for this child to exit)
    if let Some(process) = crate::scheduler::get_process(pid) {
        let ppid = process.ppid;
        if ppid != 0 {
            ktrace!("[SYS_EXIT] Waking parent process {}", ppid);
            crate::scheduler::wake_process(ppid);
        }
    }

    ktrace!("Process {} marked as zombie, yielding to scheduler", pid);

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

    ktrace!("fork() called from PID {}", current_pid);
    ktrace!("[fork] PID {} calling fork", current_pid);

    let parent_pid = current_pid;
    let child_pid = crate::process::allocate_pid();

    ktrace!("[fork] Allocated child PID {}", child_pid);

    let mut child_process = parent_process;
    child_process.pid = child_pid;
    child_process.ppid = current_pid;
    child_process.tgid = child_pid; // Fork creates new process: tgid equals pid
    child_process.state = crate::process::ProcessState::Ready;
    child_process.has_entered_user = false;
    child_process.context_valid = false; // Context not yet saved by context_switch
    child_process.is_fork_child = true;
    child_process.is_thread = false; // Fork creates new process, not thread
    child_process.exit_code = 0;
    child_process.clear_child_tid = 0; // No clear_child_tid for fork

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

    // CRITICAL: Copy parent's CALLEE-SAVED registers to child's context
    // These were saved to GS_DATA by syscall_interrupt_handler at int 0x81 entry
    // BEFORE the syscall wrapper modified them for argument passing.
    // The child must inherit these registers for the fork() call to work correctly.
    //
    // Note: We use slots 22-27 (offset 176-216) which are set in syscall_interrupt_handler
    // These contain the ORIGINAL callee-saved register values, not the syscall args.
    let (saved_rbx, saved_rbp, saved_r12, saved_r13, saved_r14, saved_r15) = unsafe {
        let rbx: u64;
        let rbp: u64;
        let r12: u64;
        let r13: u64;
        let r14: u64;
        let r15: u64;
        core::arch::asm!(
            "mov {0}, gs:[176]",  // GS_SLOT_INT81_RBX (slot 22)
            "mov {1}, gs:[184]",  // GS_SLOT_INT81_RBP (slot 23)
            "mov {2}, gs:[192]",  // GS_SLOT_INT81_R12 (slot 24)
            "mov {3}, gs:[200]",  // GS_SLOT_INT81_R13 (slot 25)
            "mov {4}, gs:[208]",  // GS_SLOT_INT81_R14 (slot 26)
            "mov {5}, gs:[216]",  // GS_SLOT_INT81_R15 (slot 27)
            out(reg) rbx,
            out(reg) rbp,
            out(reg) r12,
            out(reg) r13,
            out(reg) r14,
            out(reg) r15,
            options(nostack, preserves_flags)
        );
        (rbx, rbp, r12, r13, r14, r15)
    };

    // Also get saved RFLAGS
    let saved_rflags = unsafe {
        let rflags: u64;
        core::arch::asm!(
            "mov {0}, gs:[64]",   // GS_SLOT_SAVED_RFLAGS
            out(reg) rflags,
            options(nostack, preserves_flags)
        );
        rflags
    };

    // CRITICAL FIX: Also restore CALLER-SAVED registers (rdi, rsi, rdx, r8, r9, r10)
    // These were saved to GS_DATA at syscall entry (before syscall_dispatch modified them)
    // Without these, function arguments like argv/envp are lost after fork!
    let (saved_rdi, saved_rsi, saved_rdx, saved_r8, saved_r9, saved_r10) = unsafe {
        let rdi: u64;
        let rsi: u64;
        let rdx: u64;
        let r8: u64;
        let r9: u64;
        let r10: u64;
        core::arch::asm!(
            "mov {0}, gs:[88]",   // GS_SLOT_SAVED_RDI (syscall entry value)
            "mov {1}, gs:[96]",   // GS_SLOT_SAVED_RSI
            "mov {2}, gs:[104]",  // GS_SLOT_SAVED_RDX
            "mov {3}, gs:[128]",  // GS_SLOT_SAVED_R8
            "mov {4}, gs:[136]",  // GS_SLOT_SAVED_R9
            "mov {5}, gs:[144]",  // GS_SLOT_SAVED_R10
            out(reg) rdi,
            out(reg) rsi,
            out(reg) rdx,
            out(reg) r8,
            out(reg) r9,
            out(reg) r10,
            options(nostack, preserves_flags)
        );
        (rdi, rsi, rdx, r8, r9, r10)
    };

    child_process.context = crate::process::Context::zero();
    child_process.context.rax = 0; // fork returns 0 in child
    child_process.context.rip = syscall_return_addr;
    child_process.context.rsp = user_rsp;
    // Restore callee-saved registers (these are what the compiler expects to be preserved)
    child_process.context.rbx = saved_rbx;
    child_process.context.rbp = saved_rbp;
    child_process.context.r12 = saved_r12;
    child_process.context.r13 = saved_r13;
    child_process.context.r14 = saved_r14;
    child_process.context.r15 = saved_r15;
    // CRITICAL FIX: Also restore caller-saved registers for proper function continuation
    child_process.context.rdi = saved_rdi;
    child_process.context.rsi = saved_rsi;
    child_process.context.rdx = saved_rdx;
    child_process.context.r8 = saved_r8;
    child_process.context.r9 = saved_r9;
    child_process.context.r10 = saved_r10;
    // Also update user_rflags
    child_process.user_rflags = saved_rflags;

    kdebug!(
        "Child fork: entry={:#x}, stack={:#x}, will return RAX=0",
        child_process.entry_point,
        child_process.stack_top
    );

    let memory_size = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;
    kdebug!(
        "fork() - copying {:#x} bytes ({} KB) from {:#x}",
        memory_size,
        memory_size / 1024,
        USER_VIRT_BASE
    );

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

    unsafe {
        // CRITICAL: Copy from parent's PHYSICAL base, not USER_VIRT_BASE!
        // USER_VIRT_BASE is the virtual address in the process's address space,
        // but we're in kernel context with identity mapping, so we need to use
        // the actual physical addresses.
        let src_ptr = parent_phys_base as *const u8;
        let dst_ptr = child_phys_base as *mut u8;

        if dst_ptr as u64 + memory_size > 0x1_0000_0000 {
            kerror!(
                "fork: Child physical address {:#x} + size {:#x} exceeds physical limit!",
                child_phys_base,
                memory_size
            );
            return u64::MAX;
        }

        // Perform the memory copy
        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, memory_size as usize);

        // Memory barrier to ensure copy is visible
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Verify copy succeeded (use volatile to bypass caches)
        let verify_src = core::ptr::read_volatile(src_ptr);
        let verify_dst = core::ptr::read_volatile(dst_ptr);

        if verify_src != verify_dst {
            kerror!(
                "fork: Memory copy verification FAILED! src={:#x}, dst={:#x}, src_addr={:#x}, dst_addr={:#x}",
                verify_src,
                verify_dst,
                src_ptr as u64,
                dst_ptr as u64
            );
            kwarn!(
                "Fork memory copy may be corrupted - this may indicate insufficient QEMU memory"
            );
        }

        // Flush TLB after memory copy
        use x86_64::instructions::tlb::flush_all;
        flush_all();
    }

    kdebug!(
        "fork() - memory copied successfully, {} KB",
        memory_size / 1024
    );

    child_process.memory_base = child_phys_base;
    child_process.memory_size = memory_size;

    // Fork: use demand paging - child process pages will be mapped on first access
    // Physical memory is already copied, but page table entries are cleared
    match crate::paging::create_process_address_space(child_phys_base, memory_size, true) {
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

    if let Err(e) = crate::scheduler::add_process(child_process, 128) {
        kerror!("fork() - failed to add child process: {}", e);
        return u64::MAX;
    }

    ktrace!(
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
    use alloc::vec::Vec;

    let current_pid = get_current_pid().unwrap_or(0);
    ktrace!(
        "[syscall_execve] PID={} path_ptr={:#x}",
        current_pid,
        path as u64
    );

    if path.is_null() {
        kerror!("[syscall_execve] Error: path is null");
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // CRITICAL: Copy user strings BEFORE switching to kernel CR3
    // User memory is only accessible under user CR3
    const MAX_EXEC_PATH_LEN: usize = 256;
    const MAX_EXEC_ARGS: usize = crate::process::MAX_PROCESS_ARGS;
    const MAX_EXEC_ARG_LEN: usize = 256;

    let mut path_buf = [0u8; MAX_EXEC_PATH_LEN];
    let path_len = match copy_user_c_string(path, &mut path_buf) {
        Ok(len) => len,
        Err(_) => {
            kerror!("[syscall_execve] Error: path too long or invalid");
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    // Also copy argv BEFORE CR3 switch
    let mut argv_storage: Vec<Vec<u8>> = Vec::new();
    if !_argv.is_null() {
        let mut arg_index = 0usize;
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

            let mut arg_buf = [0u8; MAX_EXEC_ARG_LEN];
            let len = match copy_user_c_string(arg_ptr, &mut arg_buf) {
                Ok(len) => len,
                Err(_) => {
                    posix::set_errno(posix::errno::E2BIG);
                    return u64::MAX;
                }
            };

            argv_storage.push(arg_buf[..len].to_vec());
            arg_index += 1;
        }
    }

    // CRITICAL FIX: Switch to kernel CR3 for the rest of execve
    // This ensures that all memory allocations (EXT2_READ_CACHE, heap buffers, etc.)
    // are accessed consistently under the same page tables
    let kernel_cr3 = crate::paging::kernel_pml4_phys();
    let saved_cr3: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) saved_cr3, options(nomem, nostack));
        if saved_cr3 != kernel_cr3 {
            core::arch::asm!("mov cr3, {}", in(reg) kernel_cr3, options(nostack));
        }
    }

    let path_slice = &path_buf[..path_len];
    let path_str = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => {
            kerror!("[syscall_execve] Error: invalid UTF-8 in path");
            // Restore CR3 before returning
            unsafe {
                if saved_cr3 != kernel_cr3 {
                    core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                }
            }
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    };

    // Build references for downstream code (argv_storage was populated before CR3 switch)
    let argv_refs: Vec<&[u8]> = argv_storage.iter().map(|v| v.as_slice()).collect();
    let argv_list = argv_refs.as_slice();

    kinfo!(
        "[syscall_execve] Path: {}, argc={}",
        path_str,
        argv_list.len()
    );

    let exec_path_bytes = path_slice;

    let elf_data = match crate::fs::read_file_bytes(path_str) {
        Some(data) => data,
        None => {
            kerror!("[syscall_execve] Error: file not found: {}", path_str);
            // Restore CR3 before returning
            unsafe {
                if saved_cr3 != kernel_cr3 {
                    core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                }
            }
            posix::set_errno(posix::errno::ENOENT);
            return u64::MAX;
        }
    };

    let current_pid = match get_current_pid() {
        Some(pid) => pid,
        None => {
            // Restore CR3 before returning
            unsafe {
                if saved_cr3 != kernel_cr3 {
                    core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                }
            }
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
                // Restore CR3 before returning
                unsafe {
                    if saved_cr3 != kernel_cr3 {
                        core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                    }
                }
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }
        }
    };

    let new_process = match crate::process::Process::from_elf_with_args_at_base(
        elf_data,
        argv_list,
        Some(exec_path_bytes),
        current_memory_base,
        current_cr3,
    ) {
        Ok(proc) => proc,
        Err(e) => {
            kerror!("[syscall_execve] Error loading ELF: {}", e);
            // Restore CR3 before returning
            unsafe {
                if saved_cr3 != kernel_cr3 {
                    core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                }
            }
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
                    // Update cmdline from new process
                    entry.process.cmdline = new_process.cmdline;
                    entry.process.cmdline_len = new_process.cmdline_len;
                    // CRITICAL: Do NOT overwrite kernel_stack - keep the existing one!
                    // The kernel_stack was allocated during fork() and must be preserved
                    // for syscall handling. new_process.kernel_stack is 0 which would
                    // cause a kernel page fault on the next syscall.
                    // Also reset TLS (fs_base) since new program needs fresh TLS setup.
                    entry.process.fs_base = 0;
                    // IMPORTANT: Do NOT set context_valid=false here!
                    // The process is already running and will return via the syscall return path.
                    // Setting context_valid=false would cause the scheduler to treat this as
                    // a first-run process if preempted, leading to double-execution.
                    // The EXEC_CONTEXT mechanism handles jumping to the new entry point
                    // in the syscall return path.
                    // 
                    // Also clear is_fork_child since execve replaces the process image.
                    entry.process.is_fork_child = false;

                    ktrace!(
                        "[syscall_execve] Updated: entry={:#x}, stack={:#x}, cr3={:#x}, kernel_stack={:#x}",
                        entry.process.entry_point,
                        entry.process.stack_top,
                        entry.process.cr3,
                        entry.process.kernel_stack
                    );

                    entry.process.signal_state.reset_to_default();
                    break;
                }
            }
        }

        if !found {
            // Restore CR3 before returning
            unsafe {
                if saved_cr3 != kernel_cr3 {
                    core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                }
            }
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    }

    let user_data_sel = unsafe {
        let selectors = crate::gdt::get_selectors();
        let sel = selectors.user_data_selector.0 as u64;
        sel | 3
    };

    set_exec_context(
        new_process.entry_point,
        new_process.stack_top,
        user_data_sel,
    );

    // Restore CR3 before returning to user mode
    // Note: The process will be scheduled with its new CR3 when it runs
    unsafe {
        if saved_cr3 != kernel_cr3 {
            core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
        }
    }

    // Return magic value to signal exec
    0x4558454300000000
}

/// POSIX wait4() system call - wait for process state change
pub fn wait4(pid: i64, status: *mut i32, options: i32, _rusage: *mut u8) -> u64 {
    let current_pid = match crate::scheduler::get_current_pid() {
        Some(pid) => pid,
        None => {
            kerror!("wait4() called but no current process");
            posix::set_errno(posix::errno::ESRCH);
            return u64::MAX;
        }
    };

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

    loop {
        let mut found_any_child = false;
        let mut child_exited = false;
        let mut child_exit_code: Option<i32> = None;
        let mut child_term_signal: Option<i32> = None;
        let mut wait_pid = 0u64;

        if pid == -1 {
            // Use efficient single-pass lookup instead of iterating all possible PIDs
            let (has_child, zombie_pid, exit_code, term_signal) =
                crate::scheduler::find_any_child_for_wait(current_pid);
            
            found_any_child = has_child;
            
            if let Some(zpid) = zombie_pid {
                wait_pid = zpid;
                child_exited = true;
                child_exit_code = exit_code;
                child_term_signal = term_signal;
            }
        } else if pid > 0 {
            if let Some(child_state) = crate::scheduler::get_child_state(current_pid, pid as u64) {
                found_any_child = true;
                wait_pid = pid as u64;

                if child_state == crate::process::ProcessState::Zombie {
                    child_exited = true;

                    if let Some(proc) = crate::scheduler::get_process(wait_pid) {
                        child_exit_code = Some(proc.exit_code);
                        child_term_signal = proc.term_signal;
                    } else {
                        child_exit_code = Some(0);
                    }
                }
            }
        }

        if child_exited {
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
                "wait4() returning child PID {} with status {:#x}",
                wait_pid,
                encoded_status
            );
            posix::set_errno(0);
            return wait_pid;
        }

        if !found_any_child {
            posix::set_errno(posix::errno::ECHILD);
            return u64::MAX;
        }

        if is_nonblocking {
            posix::set_errno(0);
            return 0;
        }

        // Put the process to sleep waiting for a child to exit
        // This avoids busy-waiting which causes the system to be unresponsive
        crate::scheduler::set_current_process_state(crate::process::ProcessState::Sleeping);
        crate::scheduler::do_schedule();
    }
}

/// POSIX kill() system call - send signal to process
///
/// If pid > 0, signal is sent to process with that PID.
/// If pid == 0, signal is sent to every process in the calling process's process group.
/// If pid == -1, signal is sent to every process except the init process.
/// If pid < -1, signal is sent to every process in process group -pid.
///
/// If signum == 0, no signal is sent but error checking is still performed.
pub fn kill(pid: i64, signum: u64) -> u64 {
    use crate::signal::{NSIG, SIGCONT, SIGKILL, SIGSTOP, SIGTERM};

    // Validate signal number
    if signum >= NSIG as u64 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let current_pid = crate::scheduler::get_current_pid().unwrap_or(0);
    ktrace!(
        "kill(pid={}, sig={}) called from PID {}",
        pid,
        signum,
        current_pid
    );

    // signum == 0: just check permissions, don't send signal
    if signum == 0 {
        if pid > 0 {
            // Check if target process exists
            if crate::scheduler::get_process(pid as u64).is_some() {
                posix::set_errno(0);
                return 0;
            } else {
                posix::set_errno(posix::errno::ESRCH);
                return u64::MAX;
            }
        }
        posix::set_errno(0);
        return 0;
    }

    if pid > 0 {
        // Send to specific process
        let target_pid = pid as u64;

        // Get the target process
        let target_process = match crate::scheduler::get_process(target_pid) {
            Some(p) => p,
            None => {
                kerror!("kill: process {} not found", target_pid);
                posix::set_errno(posix::errno::ESRCH);
                return u64::MAX;
            }
        };

        // Don't allow killing init (PID 1) with most signals
        if target_pid == 1 && signum as u32 != SIGCONT {
            kwarn!(
                "kill: cannot kill init process (PID 1) with signal {}",
                signum
            );
            posix::set_errno(posix::errno::EPERM);
            return u64::MAX;
        }

        // Handle the signal based on its type
        match signum as u32 {
            SIGKILL => {
                // SIGKILL cannot be caught or ignored - terminate immediately
                ktrace!("Sending SIGKILL to PID {}", target_pid);

                // Set termination signal
                if let Err(e) =
                    crate::scheduler::set_process_term_signal(target_pid, SIGKILL as i32)
                {
                    kerror!("Failed to set term signal for PID {}: {}", target_pid, e);
                }

                // Mark as zombie (terminated by signal)
                if let Err(e) =
                    crate::scheduler::set_process_state(target_pid, ProcessState::Zombie)
                {
                    kerror!("Failed to set PID {} to Zombie: {}", target_pid, e);
                    posix::set_errno(posix::errno::ESRCH);
                    return u64::MAX;
                }

                ktrace!("Process {} killed with SIGKILL", target_pid);
            }
            SIGTERM => {
                // SIGTERM - request termination (can be caught)
                ktrace!("Sending SIGTERM to PID {}", target_pid);

                // For now, treat SIGTERM like SIGKILL (terminate immediately)
                // TODO: Implement proper signal delivery to user-space handlers
                if let Err(e) =
                    crate::scheduler::set_process_term_signal(target_pid, SIGTERM as i32)
                {
                    kerror!("Failed to set term signal for PID {}: {}", target_pid, e);
                }

                if let Err(e) =
                    crate::scheduler::set_process_state(target_pid, ProcessState::Zombie)
                {
                    kerror!("Failed to set PID {} to Zombie: {}", target_pid, e);
                    posix::set_errno(posix::errno::ESRCH);
                    return u64::MAX;
                }

                ktrace!("Process {} terminated with SIGTERM", target_pid);
            }
            SIGSTOP => {
                // SIGSTOP - stop process (cannot be caught)
                ktrace!("Sending SIGSTOP to PID {}", target_pid);

                if let Err(e) =
                    crate::scheduler::set_process_state(target_pid, ProcessState::Sleeping)
                {
                    kerror!("Failed to stop PID {}: {}", target_pid, e);
                    posix::set_errno(posix::errno::ESRCH);
                    return u64::MAX;
                }

                ktrace!("Process {} stopped with SIGSTOP", target_pid);
            }
            SIGCONT => {
                // SIGCONT - continue if stopped
                ktrace!("Sending SIGCONT to PID {}", target_pid);

                if target_process.state == ProcessState::Sleeping {
                    if let Err(e) =
                        crate::scheduler::set_process_state(target_pid, ProcessState::Ready)
                    {
                        kerror!("Failed to continue PID {}: {}", target_pid, e);
                        posix::set_errno(posix::errno::ESRCH);
                        return u64::MAX;
                    }
                    ktrace!("Process {} continued with SIGCONT", target_pid);
                }
            }
            _ => {
                // Other signals - deliver to process signal queue
                ktrace!("Sending signal {} to PID {}", signum, target_pid);

                // For unhandled signals that terminate by default, terminate the process
                let terminates = matches!(
                    signum as u32,
                    crate::signal::SIGHUP
                        | crate::signal::SIGINT
                        | crate::signal::SIGQUIT
                        | crate::signal::SIGILL
                        | crate::signal::SIGABRT
                        | crate::signal::SIGFPE
                        | crate::signal::SIGSEGV
                        | crate::signal::SIGPIPE
                        | crate::signal::SIGALRM
                        | crate::signal::SIGUSR1
                        | crate::signal::SIGUSR2
                );

                if terminates {
                    if let Err(e) =
                        crate::scheduler::set_process_term_signal(target_pid, signum as i32)
                    {
                        kerror!("Failed to set term signal for PID {}: {}", target_pid, e);
                    }

                    if let Err(e) =
                        crate::scheduler::set_process_state(target_pid, ProcessState::Zombie)
                    {
                        kerror!("Failed to terminate PID {}: {}", target_pid, e);
                        posix::set_errno(posix::errno::ESRCH);
                        return u64::MAX;
                    }

                    kinfo!("Process {} terminated by signal {}", target_pid, signum);
                }
                // Non-terminating signals (like SIGCHLD) are just noted in the signal queue
                // TODO: Implement proper signal delivery
            }
        }
    } else if pid == 0 {
        // Send to all processes in the same process group as caller
        // TODO: Implement process groups
        kwarn!("kill: process groups not implemented, ignoring pid=0");
    } else if pid == -1 {
        // Send to all processes (except init)
        kwarn!("kill: broadcast to all processes not implemented");
    } else {
        // pid < -1: send to process group -pid
        // TODO: Implement process groups
        kwarn!("kill: process groups not implemented, ignoring pid={}", pid);
    }

    posix::set_errno(0);
    0
}
