//! ELF loading for process creation
//!
//! This module contains the Process methods for loading ELF binaries,
//! including support for both static and dynamically linked executables.

use alloc::alloc::{alloc, Layout};
use core::ptr;

use crate::elf::ElfLoader;
use crate::{kerror, kinfo, ktrace, kwarn};

use super::pid_tree::allocate_pid;
use super::stack::build_initial_stack;
use super::types::{
    Context, Process, ProcessState, DEFAULT_ARGV0, HEAP_BASE, HEAP_SIZE, INTERP_BASE,
    KERNEL_STACK_ALIGN, KERNEL_STACK_SIZE, MAX_PROCESS_ARGS, STACK_BASE, STACK_SIZE,
    USER_PHYS_BASE, USER_REGION_SIZE, USER_VIRT_BASE, build_cmdline,
};

impl Process {
    /// Create a new process from an ELF binary
    /// Supports both static and dynamically linked executables via PT_INTERP
    pub fn from_elf(elf_data: &'static [u8]) -> Result<Self, &'static str> {
        Self::from_elf_with_args(elf_data, &[], None)
    }

    /// Load ELF at specified physical base and CR3 (for execve to reuse existing process memory)
    /// This is the POSIX-compliant way: exec replaces the image but keeps the same memory region
    pub fn from_elf_with_args_at_base(
        elf_data: &'static [u8],
        argv: &[&[u8]],
        exec_path: Option<&[u8]>,
        phys_base: u64,
        existing_cr3: u64,
    ) -> Result<Self, &'static str> {
        kinfo!(
            "Process::from_elf_with_args_at_base called: phys_base={:#x}, cr3={:#x}",
            phys_base,
            existing_cr3
        );

        if elf_data.len() < 64 {
            kerror!("ELF data too small: {} bytes", elf_data.len());
            return Err("ELF data too small");
        }

        if &elf_data[0..4] != b"\x7fELF" {
            kerror!(
                "Invalid ELF magic: {:02x} {:02x} {:02x} {:02x}",
                elf_data[0],
                elf_data[1],
                elf_data[2],
                elf_data[3]
            );
            return Err("Invalid ELF magic");
        }

        // Clear existing memory before loading new ELF (POSIX requirement)
        kinfo!(
            "Clearing process memory at base={:#x}, size={:#x}",
            phys_base,
            USER_REGION_SIZE
        );

        // Check current CR3 before clearing memory
        let current_cr3 = {
            use x86_64::registers::control::Cr3;
            let (frame, _) = Cr3::read();
            frame.start_address().as_u64()
        };
        ktrace!(
            "[from_elf_with_args_at_base] About to clear memory, current_CR3={:#x}, target_CR3={:#x}",
            current_cr3, existing_cr3
        );

        // CRITICAL: Check if we're about to overwrite any active page tables
        // Page tables are allocated starting from 0x08000000
        // User memory starts from phys_base (typically >= 0x10000000)
        // But we need to be absolutely certain
        let pt_region_start = 0x08000000u64;
        let pt_region_end = 0x08100000u64; // Allow 1MB for page tables
        let clear_start = phys_base;
        let clear_end = phys_base + USER_REGION_SIZE;

        if clear_start < pt_region_end && clear_end > pt_region_start {
            crate::kfatal!(
                "CRITICAL: About to clear memory region {:#x}-{:#x} which overlaps with \
                 page table region {:#x}-{:#x}! This would destroy active page tables!",
                clear_start,
                clear_end,
                pt_region_start,
                pt_region_end
            );
        }

        unsafe {
            ptr::write_bytes(phys_base as *mut u8, 0, USER_REGION_SIZE as usize);
        }

        ktrace!("[from_elf_with_args_at_base] Memory cleared successfully");

        let loader = ElfLoader::new(elf_data)?;

        // CRITICAL: ElfLoader writes to physical memory but returns virtual addresses
        // We need to adjust: write to phys_base but calculate addresses from USER_VIRT_BASE
        // Since kernel has identity mapping, we temporarily load at phys_base then adjust addresses
        let mut program_image = loader.load(phys_base)?;

        // Adjust addresses: ElfLoader calculated them relative to phys_base,
        // but userspace expects them relative to USER_VIRT_BASE
        let addr_adjustment = USER_VIRT_BASE as i64 - phys_base as i64;
        program_image.entry_point = ((program_image.entry_point as i64) + addr_adjustment) as u64;
        program_image.phdr_vaddr = ((program_image.phdr_vaddr as i64) + addr_adjustment) as u64;
        program_image.base_addr = USER_VIRT_BASE;
        program_image.load_bias = USER_VIRT_BASE as i64 - program_image.first_load_vaddr as i64;

        kinfo!(
            "Program image loaded and adjusted: entry={:#x}, base={:#x}, phdr={:#x}",
            program_image.entry_point,
            program_image.base_addr,
            program_image.phdr_vaddr
        );

        if program_image.phdr_vaddr == 0 {
            kwarn!("CRITICAL: phdr_vaddr is 0! This will cause dynamic linker failure.");
            // Attempt to fix it: assume it's at base + e_phoff
            // We need to access the ELF header again, but we don't have it here easily.
            // But we know base_addr is USER_VIRT_BASE.
            // And usually phdr is at offset 64 (0x40) for 64-bit ELF.
            program_image.phdr_vaddr = USER_VIRT_BASE + 64;
            kwarn!("Fixed phdr_vaddr to {:#x} (assuming standard offset)", program_image.phdr_vaddr);
        }

        // Build argument list
        let mut arg_storage: [&[u8]; MAX_PROCESS_ARGS] = [&[]; MAX_PROCESS_ARGS];
        let mut argc = 0usize;

        if argv.is_empty() {
            let fallback = exec_path.filter(|p| !p.is_empty()).unwrap_or(DEFAULT_ARGV0);
            arg_storage[0] = fallback;
            argc = 1;
        } else {
            for arg in argv {
                if argc >= MAX_PROCESS_ARGS {
                    kerror!(
                        "execve argument list exceeds MAX_PROCESS_ARGS={}",
                        MAX_PROCESS_ARGS
                    );
                    return Err("Too many arguments");
                }
                arg_storage[argc] = *arg;
                argc += 1;
            }
        }

        if argc == 0 {
            arg_storage[0] = DEFAULT_ARGV0;
            argc = 1;
        }

        let final_args = &arg_storage[..argc];
        let exec_slice = match exec_path {
            Some(path) if !path.is_empty() => path,
            _ => final_args[0],
        };

        // Handle dynamic/static executable
        let (entry_point, stack_ptr) = if let Some(interp_path) = loader.get_interpreter() {
            kinfo!("Dynamic executable, interpreter: {}", interp_path);

            if let Some(interp_data) = crate::fs::read_file_bytes(interp_path) {
                let interp_loader = ElfLoader::new(interp_data)?;

                // Calculate physical address for interpreter region
                // INTERP_BASE is virtual, need to map to physical
                let interp_offset = INTERP_BASE - USER_VIRT_BASE;
                let interp_phys = phys_base + interp_offset;

                let mut interp_image = interp_loader.load(interp_phys)?;

                // Adjust interpreter addresses to virtual space
                let interp_adjustment = INTERP_BASE as i64 - interp_phys as i64;
                interp_image.entry_point =
                    ((interp_image.entry_point as i64) + interp_adjustment) as u64;
                interp_image.phdr_vaddr =
                    ((interp_image.phdr_vaddr as i64) + interp_adjustment) as u64;
                interp_image.base_addr = INTERP_BASE;
                interp_image.load_bias = INTERP_BASE as i64 - interp_image.first_load_vaddr as i64;

                kinfo!(
                    "Interpreter loaded and adjusted: entry={:#x}, base={:#x}",
                    interp_image.entry_point,
                    interp_image.base_addr
                );
                
                kinfo!(
                    "DYNAMIC LINK DEBUG: program_image.entry_point={:#x}, program_image.phdr_vaddr={:#x}",
                    program_image.entry_point,
                    program_image.phdr_vaddr
                );

                // Calculate physical address for stack
                let stack_phys = phys_base + (STACK_BASE - USER_VIRT_BASE);
                let stack = build_initial_stack(
                    final_args,
                    exec_slice,
                    STACK_BASE,
                    STACK_SIZE,
                    stack_phys,
                    &program_image,
                    Some(&interp_image),
                )?;

                (interp_image.entry_point, stack)
            } else {
                kwarn!("Interpreter '{}' not found, trying static", interp_path);
                let stack_phys = phys_base + (STACK_BASE - USER_VIRT_BASE);
                let stack = build_initial_stack(
                    final_args,
                    exec_slice,
                    STACK_BASE,
                    STACK_SIZE,
                    stack_phys,
                    &program_image,
                    None,
                )?;
                (program_image.entry_point, stack)
            }
        } else {
            kinfo!("Static executable");
            let stack_phys = phys_base + (STACK_BASE - USER_VIRT_BASE);
            let stack = build_initial_stack(
                final_args,
                exec_slice,
                STACK_BASE,
                STACK_SIZE,
                stack_phys,
                &program_image,
                None,
            )?;
            (program_image.entry_point, stack)
        };

        let pid = allocate_pid();
        let mut context = Context::zero();
        context.rip = entry_point;
        context.rsp = stack_ptr;

        // Build cmdline from arguments
        let (cmdline, cmdline_len) = build_cmdline(final_args);

        Ok(Process {
            pid,
            ppid: 0,
            state: ProcessState::Ready,
            entry_point,
            stack_top: stack_ptr,
            heap_start: HEAP_BASE,
            heap_end: HEAP_BASE + HEAP_SIZE,
            signal_state: crate::signal::SignalState::new(),
            context,
            has_entered_user: false,
            is_fork_child: false, // Created by execve, not fork
            cr3: existing_cr3,    // Reuse existing CR3
            tty: 0,
            memory_base: phys_base, // Reuse existing memory base
            memory_size: USER_REGION_SIZE,
            user_rip: entry_point,
            user_rsp: stack_ptr,
            user_rflags: 0x202,
            exit_code: 0,
            term_signal: None,
            kernel_stack: 0, // Initialize kernel stack pointer
            fs_base: 0, // Initialize TLS base (will be set by CLONE_SETTLS or arch_prctl)
            cmdline,
            cmdline_len,
        })
    }

    /// Create a process from ELF with arguments and optional exec path
    pub fn from_elf_with_args(
        elf_data: &'static [u8],
        argv: &[&[u8]],
        exec_path: Option<&[u8]>,
    ) -> Result<Self, &'static str> {
        kinfo!(
            "Process::from_elf called with {} bytes of ELF data",
            elf_data.len()
        );

        if elf_data.len() < 64 {
            kerror!("ELF data too small: {} bytes", elf_data.len());
            return Err("ELF data too small");
        }

        if &elf_data[0..4] != b"\x7fELF" {
            kerror!(
                "Invalid ELF magic: {:02x} {:02x} {:02x} {:02x}",
                elf_data[0],
                elf_data[1],
                elf_data[2],
                elf_data[3]
            );
            return Err("Invalid ELF magic");
        }

        kinfo!("ELF magic is valid");

        let loader = ElfLoader::new(elf_data)?;
        kinfo!("ElfLoader created successfully");

        let mut program_image = loader.load(USER_PHYS_BASE)?;
        
        // CRITICAL FIX: Adjust addresses from physical to virtual space
        // ElfLoader wrote data to USER_PHYS_BASE but calculated addresses relative to it.
        // Userspace expects addresses relative to USER_VIRT_BASE since that's what
        // the page tables map.
        let addr_adjustment = USER_VIRT_BASE as i64 - USER_PHYS_BASE as i64;
        program_image.entry_point = ((program_image.entry_point as i64) + addr_adjustment) as u64;
        program_image.phdr_vaddr = ((program_image.phdr_vaddr as i64) + addr_adjustment) as u64;
        program_image.base_addr = USER_VIRT_BASE;
        program_image.load_bias = USER_VIRT_BASE as i64 - program_image.first_load_vaddr as i64;
        
        kinfo!(
            "Program image loaded and adjusted: entry={:#x}, base={:#x}, bias={:+}, phdr={:#x}",
            program_image.entry_point,
            program_image.base_addr,
            program_image.load_bias,
            program_image.phdr_vaddr
        );

        if program_image.phdr_vaddr == 0 {
            kwarn!("CRITICAL: phdr_vaddr is 0! This will cause dynamic linker failure.");
            program_image.phdr_vaddr = USER_VIRT_BASE + 64;
            kwarn!("Fixed phdr_vaddr to {:#x} (assuming standard offset)", program_image.phdr_vaddr);
        }

        let mut arg_storage: [&[u8]; MAX_PROCESS_ARGS] = [&[]; MAX_PROCESS_ARGS];
        let mut argc = 0usize;

        if argv.is_empty() {
            let fallback = exec_path.filter(|p| !p.is_empty()).unwrap_or(DEFAULT_ARGV0);
            arg_storage[0] = fallback;
            argc = 1;
        } else {
            for arg in argv {
                if argc >= MAX_PROCESS_ARGS {
                    kerror!(
                        "execve argument list exceeds MAX_PROCESS_ARGS={}",
                        MAX_PROCESS_ARGS
                    );
                    return Err("Too many arguments");
                }
                arg_storage[argc] = *arg;
                argc += 1;
            }
        }

        if argc == 0 {
            arg_storage[0] = DEFAULT_ARGV0;
            argc = 1;
        }

        let final_args = &arg_storage[..argc];
        let exec_slice = match exec_path {
            Some(path) if !path.is_empty() => path,
            _ => final_args[0],
        };

        if let Some(interp_path) = loader.get_interpreter() {
            kinfo!("Dynamic executable detected, interpreter: {}", interp_path);

            if let Some(interp_data) = crate::fs::read_file_bytes(interp_path) {
                kinfo!("Found interpreter at {}, loading it", interp_path);

                let interp_loader = ElfLoader::new(interp_data)?;
                let interp_image = interp_loader.load(INTERP_BASE)?;
                kinfo!(
                    "Interpreter image loaded: entry={:#x}, base={:#x}, bias={:+}",
                    interp_image.entry_point,
                    interp_image.base_addr,
                    interp_image.load_bias
                );

                // Calculate physical address for stack based on our memory layout
                let stack_phys = USER_PHYS_BASE + (STACK_BASE - USER_VIRT_BASE);
                let stack_ptr = build_initial_stack(
                    final_args,
                    exec_slice,
                    STACK_BASE,
                    STACK_SIZE,
                    stack_phys,  // Correct physical address for stack
                    &program_image,
                    Some(&interp_image),
                )?;

                let pid = allocate_pid();

                let mut context = Context::zero();
                context.rip = interp_image.entry_point;
                context.rsp = stack_ptr;

                let cr3 = match crate::paging::create_process_address_space(
                    USER_PHYS_BASE,
                    USER_REGION_SIZE,
                ) {
                    Ok(cr3) => {
                        // Validate CR3 before using it
                        if let Err(e) = crate::paging::validate_cr3(cr3, false) {
                            kerror!("Process::from_elf: Invalid CR3 {:#x}: {}", cr3, e);
                            return Err("Failed to create valid address space");
                        }
                        cr3
                    }
                    Err(err) => {
                        kerror!("Failed to create address space for process: {}", err);
                        return Err("Failed to create process address space");
                    }
                };

                // Build cmdline from arguments
                let (cmdline, cmdline_len) = build_cmdline(final_args);

                return Ok(Process {
                    pid,
                    ppid: 0,
                    state: ProcessState::Ready,
                    entry_point: interp_image.entry_point,
                    stack_top: stack_ptr,
                    heap_start: HEAP_BASE,
                    heap_end: HEAP_BASE + HEAP_SIZE,
                    signal_state: crate::signal::SignalState::new(),
                    context,
                    has_entered_user: false,
                    is_fork_child: false, // New process from ELF, not fork
                    cr3,
                    tty: 0,
                    memory_base: USER_PHYS_BASE,
                    memory_size: USER_REGION_SIZE,
                    user_rip: interp_image.entry_point,
                    user_rsp: stack_ptr,
                    user_rflags: 0x202,
                    exit_code: 0,
                    term_signal: None,
                    kernel_stack: {
                        let layout =
                            Layout::from_size_align(KERNEL_STACK_SIZE, KERNEL_STACK_ALIGN).unwrap();
                        let ptr = unsafe { alloc(layout) } as u64;
                        if ptr == 0 {
                            return Err("Failed to allocate kernel stack");
                        }
                        ptr
                    },
                    fs_base: 0, // Initialize TLS base
                    cmdline,
                    cmdline_len,
                });
            } else {
                kwarn!(
                    "Interpreter '{}' not found, attempting static execution",
                    interp_path
                );
            }
        } else {
            kinfo!("Static executable detected (no PT_INTERP)");
        }

        // Calculate physical address for stack based on our memory layout:
        // Virtual STACK_BASE maps to physical USER_PHYS_BASE + (STACK_BASE - USER_VIRT_BASE)
        let stack_phys = USER_PHYS_BASE + (STACK_BASE - USER_VIRT_BASE);
        let stack_ptr = build_initial_stack(
            final_args,
            exec_slice,
            STACK_BASE,
            STACK_SIZE,
            stack_phys,  // Correct physical address for stack
            &program_image,
            None,
        )?;

        let pid = allocate_pid();

        let mut context = Context::zero();
        context.rip = program_image.entry_point;
        context.rsp = stack_ptr;

        let cr3 =
            match crate::paging::create_process_address_space(USER_PHYS_BASE, USER_REGION_SIZE) {
                Ok(cr3) => {
                    // Validate CR3 before using it
                    if let Err(e) = crate::paging::validate_cr3(cr3, false) {
                        kerror!("Process::from_elf: Invalid CR3 {:#x}: {}", cr3, e);
                        return Err("Failed to create valid address space");
                    }
                    cr3
                }
                Err(err) => {
                    kerror!("Failed to create address space for process: {}", err);
                    return Err("Failed to create process address space");
                }
            };

        // Build cmdline from arguments
        let (cmdline, cmdline_len) = build_cmdline(final_args);

        Ok(Process {
            pid,
            ppid: 0,
            state: ProcessState::Ready,
            entry_point: program_image.entry_point,
            exit_code: 0,
            term_signal: None,
            stack_top: stack_ptr,
            heap_start: HEAP_BASE,
            heap_end: HEAP_BASE + HEAP_SIZE,
            signal_state: crate::signal::SignalState::new(),
            context,
            has_entered_user: false,
            is_fork_child: false, // New process from ELF, not fork
            cr3,
            tty: 0,
            memory_base: USER_PHYS_BASE,
            memory_size: USER_REGION_SIZE,
            user_rip: program_image.entry_point,
            user_rsp: stack_ptr,
            user_rflags: 0x202,
            kernel_stack: {
                let layout =
                    Layout::from_size_align(KERNEL_STACK_SIZE, KERNEL_STACK_ALIGN).unwrap();
                let ptr = unsafe { alloc(layout) } as u64;
                if ptr == 0 {
                    return Err("Failed to allocate kernel stack");
                }
                ptr
            },
            fs_base: 0, // Initialize TLS base
            cmdline,
            cmdline_len,
        })
    }
}
