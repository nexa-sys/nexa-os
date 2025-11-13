/// Process management for user-space execution
use crate::elf::{ElfLoader, LoadResult};
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

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
    pub cr3: u64, // Page table root (for process-specific page tables) - 0 means use kernel page table
}

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

/// Allocate a new unique PID
pub fn allocate_pid() -> Pid {
    NEXT_PID.fetch_add(1, Ordering::SeqCst)
}

const DEFAULT_ARGV0: &[u8] = b"nexa";
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

        let program_name = DEFAULT_ARGV0;

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
                    program_name,
                    STACK_BASE,
                    STACK_SIZE,
                    &program_image,
                    Some(&interp_image),
                )?;

                let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);

                let mut context = Context::zero();
                context.rip = interp_image.entry_point;
                context.rsp = stack_ptr;

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
                    cr3: 0, // TODO: Allocate process-specific page table
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

        let stack_ptr =
            build_initial_stack(program_name, STACK_BASE, STACK_SIZE, &program_image, None)?;

        let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);

        let mut context = Context::zero();
        context.rip = program_image.entry_point;
        context.rsp = stack_ptr;

        Ok(Process {
            pid,
            ppid: 0,
            state: ProcessState::Ready,
            entry_point: program_image.entry_point,
            stack_top: stack_ptr,
            heap_start: HEAP_BASE,
            heap_end: HEAP_BASE + HEAP_SIZE,
            signal_state: crate::signal::SignalState::new(),
            context,
            has_entered_user: false,
            cr3: 0, // TODO: Allocate process-specific page table
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

        crate::kinfo!(
            "Executing process PID={}, entry={:#x}, stack={:#x}",
            self.pid,
            self.entry_point,
            self.stack_top
        );

        self.has_entered_user = true;

        // Jump to user mode - this never returns
        jump_to_usermode(self.entry_point, self.stack_top);
        // If we get here, iretq failed
        crate::kerror!("Failed to jump to user mode!");
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
    program_name: &[u8],
    stack_base: u64,
    stack_size: u64,
    program: &LoadResult,
    interpreter: Option<&LoadResult>,
) -> Result<u64, &'static str> {
    let mut builder = UserStackBuilder::new(stack_base, stack_size);

    let random_ptr = builder.push_bytes(&STACK_RANDOM_SEED)?;
    let argv0_ptr = if program_name.is_empty() {
        None
    } else {
        Some(builder.push_cstring(program_name)?)
    };

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

    if let Some(ptr) = argv0_ptr {
        aux_entries[aux_len] = (AT_EXECFN, ptr);
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

    if let Some(ptr) = argv0_ptr {
        builder.push_u64(ptr)?;
    }

    let argc = if argv0_ptr.is_some() { 1 } else { 0 };

    builder.pad_to_alignment(16)?;
    builder.push_u64(argc as u64)?;

    Ok(builder.current_ptr())
}

/// Jump to user mode (Ring 3) and execute code at given address
/// This function never returns - execution continues in user space
#[inline(never)]
pub fn jump_to_usermode(entry: u64, stack: u64) -> ! {
    // Use direct serial output to bypass logger restrictions
    unsafe {
        use x86_64::instructions::port::Port;
        let mut port = Port::<u8>::new(0x3F8);
        for &b in b"JUMP_TO_USER entry=" {
            port.write(b);
        }
        for shift in (0..16).rev() {
            let nibble = ((entry >> (shift * 4)) & 0xF) as u8;
            port.write(if nibble < 10 {
                b'0' + nibble
            } else {
                b'A' + nibble - 10
            });
        }
        for &b in b" stack=" {
            port.write(b);
        }
        for shift in (0..16).rev() {
            let nibble = ((stack >> (shift * 4)) & 0xF) as u8;
            port.write(if nibble < 10 {
                b'0' + nibble
            } else {
                b'A' + nibble - 10
            });
        }
        port.write(b'\n');
    }

    // Set GS data for syscall and Ring 3 switching
    unsafe {
        use x86_64::instructions::port::Port;

        let selectors = crate::gdt::get_selectors();

        // Debug output for selectors
        let mut port = Port::<u8>::new(0x3F8);
        for &b in b"SEL: ucode=" {
            port.write(b);
        }
        let uc = selectors.user_code_selector.0;
        for shift in (0..4).rev() {
            let nibble = ((uc >> (shift * 4)) & 0xF) as u8;
            port.write(if nibble < 10 {
                b'0' + nibble
            } else {
                b'A' + nibble - 10
            });
        }
        for &b in b" udata=" {
            port.write(b);
        }
        let ud = selectors.user_data_selector.0;
        for shift in (0..4).rev() {
            let nibble = ((ud >> (shift * 4)) & 0xF) as u8;
            port.write(if nibble < 10 {
                b'0' + nibble
            } else {
                b'A' + nibble - 10
            });
        }
        port.write(b'\n');

        crate::interrupts::set_gs_data(
            entry,
            stack,
            selectors.user_code_selector.0 as u64 | 3,
            selectors.user_data_selector.0 as u64 | 3,
            selectors.user_data_selector.0 as u64 | 3,
        );

        // Set GS base to point to GS_DATA for both kernel and user mode
        use x86_64::registers::model_specific::Msr;
        let gs_base = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
        Msr::new(0xc0000101).write(gs_base);
    }

    unsafe {
        // Touch the top of the user stack
        let stack_top_ptr = (stack - 8) as *mut u64;
        stack_top_ptr.write_volatile(0xdeadbeefdeadbeef);

        let selectors = crate::gdt::get_selectors();
        let user_ds = selectors.user_data_selector.0 | 3;

        // Debug output before sysret
        use x86_64::instructions::port::Port;
        let mut port = Port::<u8>::new(0x3F8);
        for &b in b"BEFORE_SYSRET\n" {
            port.write(b);
        }

        // Use sysretq to return to user mode
        // sysretq expects:
        //   RCX = user RIP
        //   R11 = user RFLAGS
        //   RSP = user stack pointer
        //   User DS/ES/FS/GS must be loaded manually
        core::arch::asm!(
            // Load user data segment into DS, ES, FS, GS
            "mov ax, {user_ds:x}",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            
            // Set up registers for sysretq
            "mov rcx, {entry}",     // RCX = user RIP
            "mov r11, {rflags}",    // R11 = user RFLAGS (IF enabled)
            "mov rsp, {stack}",     // RSP = user stack
            
            // Return to user mode
            "sysretq",
            
            user_ds = in(reg) user_ds as u64,
            entry = in(reg) entry,
            rflags = in(reg) 0x202u64,  // IF (0x200) + reserved bit 1 (0x2)
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
