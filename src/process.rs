/// Process management for user-space execution
use crate::elf::{ElfLoader, LoadResult};
use crate::{kdebug, ktrace};
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::alloc::{alloc, dealloc, Layout};

/// Process ID type
pub type Pid = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Sleeping,
    Zombie,
}

/// Virtual base address where userspace expects to be mapped.
pub const USER_VIRT_BASE: u64 = 0x400000;
/// Physical base address used when copying the userspace image.
pub const USER_PHYS_BASE: u64 = 0x400000;
/// Virtual address chosen for the base of the userspace stack region.
pub const STACK_BASE: u64 = 0x800000;
/// Size of the userspace stack in bytes (must stay 2 MiB aligned for huge pages).
pub const STACK_SIZE: u64 = 0x200000;
/// Virtual address where the heap begins in userspace.
pub const HEAP_BASE: u64 = USER_VIRT_BASE + 0x200000;
/// Size of the initial heap allocation reserved for userspace.
pub const HEAP_SIZE: u64 = 0x200000;
/// Virtual base where the dynamic loader and shared objects are staged.
pub const INTERP_BASE: u64 = STACK_BASE + STACK_SIZE;
/// Reserved size for the dynamic loader and dependent shared objects (multiple of 2 MiB).
pub const INTERP_REGION_SIZE: u64 = 0x600000;
/// Total virtual span that must be mapped for the userspace image, heap, stack, and interpreter region.
pub const USER_REGION_SIZE: u64 = (INTERP_BASE + INTERP_REGION_SIZE) - USER_VIRT_BASE;

/// Kernel stack size (32 KB)
pub const KERNEL_STACK_SIZE: usize = 32 * 1024;
/// Kernel stack alignment
pub const KERNEL_STACK_ALIGN: usize = 16;

/// CPU context saved during context switch
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Context {
    // General purpose registers
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    // Instruction pointer and stack pointer
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
}

impl Context {
    pub const fn zero() -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            rdx: 0,
            rcx: 0,
            rbx: 0,
            rax: 0,
            rip: 0,
            rsp: 0,
            rflags: 0x202, // IF flag set (interrupts enabled)
        }
    }
}

/// Process structure
#[derive(Clone, Copy)]
pub struct Process {
    pub pid: Pid,
    pub ppid: Pid, // Parent process ID (POSIX)
    pub state: ProcessState,
    pub entry_point: u64,
    pub stack_top: u64,
    pub heap_start: u64,
    pub heap_end: u64,
    pub signal_state: crate::signal::SignalState, // POSIX signal handling
    pub context: Context,                         // CPU context for context switching
    pub has_entered_user: bool,
    pub is_fork_child: bool, // True if this process was created by fork (not exec/init)
    pub cr3: u64, // Page table root (for process-specific page tables) - 0 means use kernel page table
    pub tty: usize, // Controlling virtual terminal index
    pub memory_base: u64, // Physical base address of process memory (for fork)
    pub memory_size: u64, // Size of process memory region (for fork)
    pub user_rip: u64, // Saved user-mode RIP for syscall return
    pub user_rsp: u64, // Saved user-mode RSP for syscall return
    pub user_rflags: u64, // Saved user-mode RFLAGS for syscall return
    pub exit_code: i32, // Last exit code reported by this process (if zombie)
    pub kernel_stack: u64, // Pointer to kernel stack allocation (bottom)
}

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

/// Allocate a new unique PID
pub fn allocate_pid() -> Pid {
    NEXT_PID.fetch_add(1, Ordering::SeqCst)
}

const DEFAULT_ARGV0: &[u8] = b"nexa";
pub const MAX_PROCESS_ARGS: usize = 32;
pub const MAX_PROCESSES: usize = 64;
const STACK_RANDOM_SEED: [u8; 16] = *b"NexaOSGuardSeed!";

const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_BASE: u64 = 7;
const AT_FLAGS: u64 = 8;
const AT_ENTRY: u64 = 9;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_RANDOM: u64 = 25;
const AT_EXECFN: u64 = 31;

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
        crate::kinfo!(
            "Process::from_elf_with_args_at_base called: phys_base={:#x}, cr3={:#x}",
            phys_base,
            existing_cr3
        );

        if elf_data.len() < 64 {
            crate::kerror!("ELF data too small: {} bytes", elf_data.len());
            return Err("ELF data too small");
        }

        if &elf_data[0..4] != b"\x7fELF" {
            crate::kerror!(
                "Invalid ELF magic: {:02x} {:02x} {:02x} {:02x}",
                elf_data[0],
                elf_data[1],
                elf_data[2],
                elf_data[3]
            );
            return Err("Invalid ELF magic");
        }

        // Clear existing memory before loading new ELF (POSIX requirement)
        crate::kinfo!(
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

        ktrace!(
            "[from_elf_with_args_at_base] Memory cleared successfully"
        );

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

        crate::kinfo!(
            "Program image loaded and adjusted: entry={:#x}, base={:#x}, phdr={:#x}",
            program_image.entry_point,
            program_image.base_addr,
            program_image.phdr_vaddr
        );

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
                    crate::kerror!(
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
            crate::kinfo!("Dynamic executable, interpreter: {}", interp_path);

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

                crate::kinfo!(
                    "Interpreter loaded and adjusted: entry={:#x}, base={:#x}",
                    interp_image.entry_point,
                    interp_image.base_addr
                );

                let stack = build_initial_stack(
                    final_args,
                    exec_slice,
                    STACK_BASE,
                    STACK_SIZE,
                    &program_image,
                    Some(&interp_image),
                )?;

                (interp_image.entry_point, stack)
            } else {
                crate::kwarn!("Interpreter '{}' not found, trying static", interp_path);
                let stack = build_initial_stack(
                    final_args,
                    exec_slice,
                    STACK_BASE,
                    STACK_SIZE,
                    &program_image,
                    None,
                )?;
                (program_image.entry_point, stack)
            }
        } else {
            crate::kinfo!("Static executable");
            let stack = build_initial_stack(
                final_args,
                exec_slice,
                STACK_BASE,
                STACK_SIZE,
                &program_image,
                None,
            )?;
            (program_image.entry_point, stack)
        };

        let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);
        let mut context = Context::zero();
        context.rip = entry_point;
        context.rsp = stack_ptr;

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
            kernel_stack: 0, // Initialize kernel stack pointer
        })
    }

    pub fn from_elf_with_args(
        elf_data: &'static [u8],
        argv: &[&[u8]],
        exec_path: Option<&[u8]>,
    ) -> Result<Self, &'static str> {
        crate::kinfo!(
            "Process::from_elf called with {} bytes of ELF data",
            elf_data.len()
        );

        if elf_data.len() < 64 {
            crate::kerror!("ELF data too small: {} bytes", elf_data.len());
            return Err("ELF data too small");
        }

        if &elf_data[0..4] != b"\x7fELF" {
            crate::kerror!(
                "Invalid ELF magic: {:02x} {:02x} {:02x} {:02x}",
                elf_data[0],
                elf_data[1],
                elf_data[2],
                elf_data[3]
            );
            return Err("Invalid ELF magic");
        }

        crate::kinfo!("ELF magic is valid");

        let loader = ElfLoader::new(elf_data)?;
        crate::kinfo!("ElfLoader created successfully");

        let program_image = loader.load(USER_PHYS_BASE)?;
        crate::kinfo!(
            "Program image loaded: entry={:#x}, base={:#x}, bias={:+}, phdr={:#x}",
            program_image.entry_point,
            program_image.base_addr,
            program_image.load_bias,
            program_image.phdr_vaddr
        );

        let mut arg_storage: [&[u8]; MAX_PROCESS_ARGS] = [&[]; MAX_PROCESS_ARGS];
        let mut argc = 0usize;

        if argv.is_empty() {
            let fallback = exec_path.filter(|p| !p.is_empty()).unwrap_or(DEFAULT_ARGV0);
            arg_storage[0] = fallback;
            argc = 1;
        } else {
            for arg in argv {
                if argc >= MAX_PROCESS_ARGS {
                    crate::kerror!(
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
            crate::kinfo!("Dynamic executable detected, interpreter: {}", interp_path);

            if let Some(interp_data) = crate::fs::read_file_bytes(interp_path) {
                crate::kinfo!("Found interpreter at {}, loading it", interp_path);

                let interp_loader = ElfLoader::new(interp_data)?;
                let interp_image = interp_loader.load(INTERP_BASE)?;
                crate::kinfo!(
                    "Interpreter image loaded: entry={:#x}, base={:#x}, bias={:+}",
                    interp_image.entry_point,
                    interp_image.base_addr,
                    interp_image.load_bias
                );

                let stack_ptr = build_initial_stack(
                    final_args,
                    exec_slice,
                    STACK_BASE,
                    STACK_SIZE,
                    &program_image,
                    Some(&interp_image),
                )?;

                let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);

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
                            crate::kerror!("Process::from_elf: Invalid CR3 {:#x}: {}", cr3, e);
                            return Err("Failed to create valid address space");
                        }
                        cr3
                    }
                    Err(err) => {
                        crate::kerror!("Failed to create address space for process: {}", err);
                        return Err("Failed to create process address space");
                    }
                };

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
                    kernel_stack: {
                        let layout = Layout::from_size_align(KERNEL_STACK_SIZE, KERNEL_STACK_ALIGN).unwrap();
                        let ptr = unsafe { alloc(layout) } as u64;
                        if ptr == 0 {
                            return Err("Failed to allocate kernel stack");
                        }
                        ptr
                    },
                });
            } else {
                crate::kwarn!(
                    "Interpreter '{}' not found, attempting static execution",
                    interp_path
                );
            }
        } else {
            crate::kinfo!("Static executable detected (no PT_INTERP)");
        }

        let stack_ptr = build_initial_stack(
            final_args,
            exec_slice,
            STACK_BASE,
            STACK_SIZE,
            &program_image,
            None,
        )?;

        let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);

        let mut context = Context::zero();
        context.rip = program_image.entry_point;
        context.rsp = stack_ptr;

        let cr3 =
            match crate::paging::create_process_address_space(USER_PHYS_BASE, USER_REGION_SIZE) {
                Ok(cr3) => {
                    // Validate CR3 before using it
                    if let Err(e) = crate::paging::validate_cr3(cr3, false) {
                        crate::kerror!("Process::from_elf: Invalid CR3 {:#x}: {}", cr3, e);
                        return Err("Failed to create valid address space");
                    }
                    cr3
                }
                Err(err) => {
                    crate::kerror!("Failed to create address space for process: {}", err);
                    return Err("Failed to create process address space");
                }
            };

        Ok(Process {
            pid,
            ppid: 0,
            state: ProcessState::Ready,
            entry_point: program_image.entry_point,
            exit_code: 0,
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
                let layout = Layout::from_size_align(KERNEL_STACK_SIZE, KERNEL_STACK_ALIGN).unwrap();
                let ptr = unsafe { alloc(layout) } as u64;
                if ptr == 0 {
                    return Err("Failed to allocate kernel stack");
                }
                ptr
            },
        })
    }

    /// Set parent process ID (POSIX)
    pub fn set_ppid(&mut self, ppid: Pid) {
        self.ppid = ppid;
    }

    /// Get process ID
    pub fn pid(&self) -> Pid {
        self.pid
    }

    /// Get parent process ID
    pub fn ppid(&self) -> Pid {
        self.ppid
    }

    /// Get process state
    pub fn state(&self) -> ProcessState {
        self.state
    }

    /// Execute the process in user mode (Ring 3)
    pub fn execute(&mut self) {
        self.state = ProcessState::Running;

        ktrace!(
            "[process::execute] PID={}, entry={:#x}, stack={:#x}, has_entered_user={}, is_fork_child={}",
            self.pid, self.entry_point, self.stack_top, self.has_entered_user, self.is_fork_child
        );

        crate::kinfo!(
            "Executing process PID={}, entry={:#x}, stack={:#x}",
            self.pid,
            self.entry_point,
            self.stack_top
        );

        // Mark as entered user mode BEFORE switching CR3
        self.has_entered_user = true;

        if self.is_fork_child {
            // Fork child: Return from syscall with RAX=0
            // Use sysret mechanism to return to userspace at syscall_return_addr
            ktrace!(
                "[process::execute] Fork child: returning to {:#x} with RAX=0",
                self.entry_point
            );

            // Set up syscall return context
            crate::interrupts::restore_user_syscall_context(
                self.entry_point, // user_rip (syscall return address)
                self.stack_top,   // user_rsp
                self.user_rflags, // user_rflags
            );

            // CRITICAL: Switch CR3 and return to userspace atomically
            // We must switch CR3 in the same assembly block that does sysretq
            // to avoid accessing kernel stack after address space switch
            unsafe {
                core::arch::asm!(
                    "cli",                 // Disable interrupts during transition
                    "mov cr3, {cr3}",      // Switch to child's address space
                    "mov rcx, {rip}",      // RCX = return RIP for sysretq
                    "mov r11, {rflags}",   // R11 = RFLAGS for sysretq
                    "mov rsp, {rsp}",      // RSP = user stack
                    "xor rax, rax",        // RAX = 0 (fork child return value)
                    "sysretq",             // Return to Ring 3
                    cr3 = in(reg) self.cr3,
                    rip = in(reg) self.entry_point,
                    rflags = in(reg) self.user_rflags,
                    rsp = in(reg) self.stack_top,
                    options(noreturn)
                );
            }
        } else {
            // Normal process (init/execve): Jump to entry point
            jump_to_usermode_with_cr3(self.entry_point, self.stack_top, self.cr3);
        }
    }

    pub fn set_tty(&mut self, tty: usize) {
        self.tty = tty;
    }

    pub fn tty(&self) -> usize {
        self.tty
    }
}

struct UserStackBuilder {
    cursor: u64,
    lower_bound: u64,
}

impl UserStackBuilder {
    fn new(base: u64, size: u64) -> Self {
        Self {
            cursor: base + size,
            lower_bound: base,
        }
    }

    fn current_ptr(&self) -> u64 {
        self.cursor
    }

    fn pad_to_alignment(&mut self, align: u64) -> Result<(), &'static str> {
        debug_assert!(align.is_power_of_two());
        if align == 0 {
            return Ok(());
        }

        let mask = align - 1;
        let remainder = self.cursor & mask;
        if remainder == 0 {
            return Ok(());
        }

        let padding = remainder;
        self.cursor = self.cursor.checked_sub(padding).ok_or("Stack overflow")?;
        if self.cursor < self.lower_bound {
            return Err("Stack overflow");
        }

        unsafe {
            ptr::write_bytes(self.cursor as *mut u8, 0, padding as usize);
        }

        Ok(())
    }

    fn push_bytes(&mut self, bytes: &[u8]) -> Result<u64, &'static str> {
        if bytes.is_empty() {
            return Ok(self.cursor);
        }

        let len = bytes.len() as u64;
        self.cursor = self.cursor.checked_sub(len).ok_or("Stack overflow")?;
        if self.cursor < self.lower_bound {
            return Err("Stack overflow");
        }

        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), self.cursor as *mut u8, bytes.len());
        }

        Ok(self.cursor)
    }

    fn push_cstring(&mut self, bytes: &[u8]) -> Result<u64, &'static str> {
        let null_ptr = self.push_bytes(&[0])?;
        if bytes.is_empty() {
            return Ok(null_ptr);
        }
        self.push_bytes(bytes)
    }

    fn push_u64(&mut self, value: u64) -> Result<u64, &'static str> {
        self.pad_to_alignment(8)?;
        self.cursor = self.cursor.checked_sub(8).ok_or("Stack overflow")?;
        if self.cursor < self.lower_bound {
            return Err("Stack overflow");
        }
        unsafe {
            (self.cursor as *mut u64).write(value);
        }
        Ok(self.cursor)
    }
}

fn build_initial_stack(
    argv: &[&[u8]],
    exec_path: &[u8],
    stack_base: u64,
    stack_size: u64,
    program: &LoadResult,
    interpreter: Option<&LoadResult>,
) -> Result<u64, &'static str> {
    let mut builder = UserStackBuilder::new(stack_base, stack_size);

    if argv.len() > MAX_PROCESS_ARGS {
        return Err("Too many arguments");
    }

    let random_ptr = builder.push_bytes(&STACK_RANDOM_SEED)?;
    let execfn_ptr = if exec_path.is_empty() {
        None
    } else {
        Some(builder.push_cstring(exec_path)?)
    };

    let mut arg_ptrs = [0u64; MAX_PROCESS_ARGS];
    for i in (0..argv.len()).rev() {
        arg_ptrs[i] = builder.push_cstring(argv[i])?;
    }

    let argc = argv.len();

    builder.pad_to_alignment(16)?;

    const AUX_MAX: usize = 16;
    let mut aux_entries: [(u64, u64); AUX_MAX] = [(AT_NULL, 0); AUX_MAX];
    let mut aux_len: usize = 0;

    aux_entries[aux_len] = (AT_PHDR, program.phdr_vaddr);
    aux_len += 1;
    aux_entries[aux_len] = (AT_PHENT, program.phentsize as u64);
    aux_len += 1;
    aux_entries[aux_len] = (AT_PHNUM, program.phnum as u64);
    aux_len += 1;
    aux_entries[aux_len] = (AT_PAGESZ, 4096);
    aux_len += 1;

    if let Some(interp) = interpreter {
        aux_entries[aux_len] = (AT_BASE, interp.base_addr);
        aux_len += 1;
    }

    aux_entries[aux_len] = (AT_FLAGS, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_ENTRY, program.entry_point);
    aux_len += 1;
    aux_entries[aux_len] = (AT_UID, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_EUID, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_GID, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_EGID, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_RANDOM, random_ptr);
    aux_len += 1;

    if let Some(ptr) = execfn_ptr {
        aux_entries[aux_len] = (AT_EXECFN, ptr);
        aux_len += 1;
    } else if argc > 0 {
        aux_entries[aux_len] = (AT_EXECFN, arg_ptrs[0]);
        aux_len += 1;
    }

    aux_entries[aux_len] = (AT_NULL, 0);
    aux_len += 1;

    for (key, value) in aux_entries[..aux_len].iter().rev() {
        builder.push_u64(*value)?;
        builder.push_u64(*key)?;
    }

    builder.push_u64(0)?; // envp NULL
    builder.push_u64(0)?; // argv NULL terminator

    for i in (0..argc).rev() {
        builder.push_u64(arg_ptrs[i])?;
    }

    builder.pad_to_alignment(16)?;
    builder.push_u64(argc as u64)?;

    Ok(builder.current_ptr())
}

/// Jump to user mode (Ring 3) and execute code at given address
/// This function never returns - execution continues in user space
#[inline(never)]
pub fn jump_to_usermode_with_cr3(entry: u64, stack: u64, cr3: u64) -> ! {
    // Use kdebug! macro for direct serial output
    kdebug!(
        "[jump_to_usermode_with_cr3] ENTRY: entry={:#x}, stack={:#x}, cr3={:#x}",
        entry, stack, cr3
    );

    kdebug!(
        "[jump_to_usermode_with_cr3] entry={:#018x} stack={:#018x} cr3={:#018x}",
        entry,
        stack,
        cr3
    );

    // Set GS data for syscall and Ring 3 switching
    crate::gdt::debug_dump_selectors("jump_to_usermode_with_cr3");
    let selectors = unsafe { crate::gdt::get_selectors() };
    let user_code_sel = selectors.user_code_selector.0;
    let user_data_sel = selectors.user_data_selector.0;

    kdebug!(
        "[jump_to_usermode_with_cr3] user_code_selector.0={:04x}, user_data_selector.0={:04x}",
        user_code_sel,
        user_data_sel
    );

    kdebug!(
        "[jump_to_usermode_with_cr3] Setting GS_DATA: entry={:#x}, stack={:#x}, user_cs={:#x}, user_ds={:#x}",
        entry,
        stack,
        user_code_sel as u64 | 3,
        user_data_sel as u64 | 3
    );

    unsafe {
        crate::interrupts::set_gs_data(
            entry,
            stack,
            user_code_sel as u64 | 3,
            user_data_sel as u64 | 3,
            user_data_sel as u64 | 3,
        );

        // Set GS base to point to GS_DATA for both kernel and user mode
        use x86_64::registers::model_specific::Msr;
        let gs_base = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
        Msr::new(0xc0000101).write(gs_base);
    }

    kdebug!(
        "[jump_to_usermode_with_cr3] About to switch CR3 and execute sysretq"
    );

    unsafe {
        kdebug!("BEFORE_SYSRET_WITH_CR3");

        // CRITICAL FIX: Switch CR3 and jump to usermode atomically
        // We MUST switch CR3 in the same assembly block as sysretq
        // to avoid accessing kernel stack after address space switch
        //
        // The sequence is:
        // 1. Disable interrupts (cli)
        // 2. Switch CR3 to user address space
        // 3. Set up sysretq registers (RCX, R11, RSP)
        // 4. Execute sysretq
        //
        // No Rust code can execute between steps 2 and 4, otherwise
        // we would access kernel stack through user page tables!
        core::arch::asm!(
            "cli",                 // Mask interrupts during the transition
            "mov cr3, {cr3}",      // Switch to user address space
            "mov rcx, {entry}",    // RCX = user RIP for sysretq
            "mov rsp, {stack}",    // Set user stack (safe after CR3 switch)
            "mov r11d, 0x202",     // User RFLAGS with IF=1, reserved bit=1
            "xor rax, rax",        // Clear return value
            "sysretq",             // Return to Ring 3
            cr3 = in(reg) cr3,
            entry = in(reg) entry,
            stack = in(reg) stack,
            options(noreturn)
        );
    }
}

pub fn jump_to_usermode(entry: u64, stack: u64) -> ! {
    // Use kdebug! macro for direct serial output
    kdebug!(
        "[jump_to_usermode] ENTRY: entry={:#x}, stack={:#x}",
        entry, stack
    );

    kdebug!(
        "[jump_to_usermode] entry={:#018x} stack={:#018x}",
        entry,
        stack
    );

    // Set GS data for syscall and Ring 3 switching
    crate::gdt::debug_dump_selectors("jump_to_usermode");
    let selectors = unsafe { crate::gdt::get_selectors() };
    let user_code_sel = selectors.user_code_selector.0;
    let user_data_sel = selectors.user_data_selector.0;

    kdebug!(
        "[jump_to_usermode] user_code_selector.0={:04x}, user_data_selector.0={:04x}",
        user_code_sel,
        user_data_sel
    );

    kdebug!(
        "[jump_to_usermode] Setting GS_DATA: entry={:#x}, stack={:#x}, user_cs={:#x}, user_ds={:#x}",
        entry,
        stack,
        user_code_sel as u64 | 3,
        user_data_sel as u64 | 3
    );

    unsafe {
        crate::interrupts::set_gs_data(
            entry,
            stack,
            user_code_sel as u64 | 3,
            user_data_sel as u64 | 3,
            user_data_sel as u64 | 3,
        );

        // Set GS base to point to GS_DATA for both kernel and user mode
        use x86_64::registers::model_specific::Msr;
        let gs_base = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
        Msr::new(0xc0000101).write(gs_base);
    }

    kdebug!(
        "[jump_to_usermode] About to execute sysretq"
    );

    unsafe {
        kdebug!("BEFORE_SYSRET");

        // CRITICAL FIX for exit syscall GP fault:
        // Don't manually set segment registers before sysretq!
        // sysretq automatically sets CS/SS from STAR MSR, and setting
        // DS/ES/FS/GS to user segments in kernel mode can cause GP faults.
        // Let the user program set DS/ES/FS after entering Ring 3.
        //
        // Ensure R11 (user RFLAGS) is programmed with the canonical 0x202
        // value explicitly to avoid allocator reuse that can leak stale bits
        // from prior syscalls and trigger a #GP during sysretq.
        //
        // Additionally, disable interrupts just before switching RSP to the
        // user stack so we never take an interrupt while still running in
        // kernel mode with a user-mode stack pointer. Otherwise the interrupt
        // handler would observe a bogus kernel stack and eventually crash with
        // an unpredictable #GP.
        core::arch::asm!(
            "cli",                 // Mask interrupts during the stack swap
            "mov rcx, {entry}",    // RCX = user RIP for sysretq
            "mov rsp, {stack}",    // Set user stack (now safe from interrupts)
            "mov r11d, 0x202",     // User RFLAGS with IF=1, reserved bit=1
            "xor rax, rax",        // Clear return value
            "sysretq",             // Return to Ring 3
            entry = in(reg) entry,
            stack = in(reg) stack,
            options(noreturn)
        );
    }
}

// Note: do_iretq function removed - iretq logic is now inline in jump_to_usermode

/// User process entry point and stack for Ring 3 switching
static mut USER_ENTRY: u64 = 0;
static mut USER_STACK: u64 = 0;

/// Get the stored user entry point
pub unsafe fn get_user_entry() -> u64 {
    USER_ENTRY
}

/// Get the stored user stack
pub unsafe fn get_user_stack() -> u64 {
    USER_STACK
}
