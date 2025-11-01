/// Process management for user-space execution
use crate::elf::ElfLoader;
use crate::gdt;
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

/// Process ID type
pub type Pid = u64;

/// Process state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Sleeping,
    Zombie,
}

/// Process structure
pub struct Process {
    pub pid: Pid,
    pub state: ProcessState,
    pub entry_point: u64,
    pub stack_top: u64,
    pub heap_start: u64,
    pub heap_end: u64,
}

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

impl Process {
    /// Create a new process from an ELF binary
    pub fn from_elf(elf_data: &'static [u8]) -> Result<Self, &'static str> {
        crate::kinfo!(
            "Process::from_elf called with {} bytes of ELF data",
            elf_data.len()
        );

        // Check if the data looks like a valid ELF
        if elf_data.len() < 64 {
            crate::kerror!("ELF data too small: {} bytes", elf_data.len());
            return Err("ELF data too small");
        }

        // Check ELF magic
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

        // Allocate user space memory
        const USER_BASE: u64 = 0x400000; // Physical base for user code
        const STACK_BASE: u64 = 0x600000; // Physical base for user stack
        const STACK_SIZE: u64 = 0x100000; // 1MB stack
        const HEAP_SIZE: u64 = 0x100000; // 1MB heap

        crate::kinfo!(
            "Constants defined: USER_BASE={:#x}, STACK_BASE={:#x}, STACK_SIZE={:#x}",
            USER_BASE,
            STACK_BASE,
            STACK_SIZE
        );

        // Load ELF
        crate::kinfo!("About to call loader.load with base_addr={:#x}", USER_BASE);
        let physical_entry = loader.load(USER_BASE)?;
        crate::kinfo!(
            "ELF loaded successfully, physical_entry={:#x}",
            physical_entry
        );

        // Calculate virtual entry point
        // The ELF entry point is relative to the first load segment
        let header = loader.header();
        let virtual_entry = header.entry_point();
        crate::kinfo!("Virtual entry point from ELF: {:#x}", virtual_entry);

        let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);

        // Initialize user stack
        let stack_base = STACK_BASE; // Virtual stack base will be mapped
        let stack_size = STACK_SIZE;
        let stack_top = stack_base + stack_size;

        let process = Process {
            pid,
            state: ProcessState::Ready,
            entry_point: virtual_entry, // Use virtual entry point for Ring 3 execution
            stack_top,
            heap_start: USER_BASE + 0x200000, // Heap after code
            heap_end: USER_BASE + 0x200000 + HEAP_SIZE,
        };

        Ok(process)
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

        // Jump to user mode - this never returns
        unsafe {
            jump_to_usermode(self.entry_point, self.stack_top);
            // If we get here, iretq failed
            crate::kerror!("Failed to jump to user mode!");
        }
    }
}

/// Jump to user mode (Ring 3) and execute code at given address
/// This function never returns - execution continues in user space
#[inline(never)]
pub fn jump_to_usermode(entry: u64, stack: u64) {
    crate::kinfo!(
        "About to execute iretq with entry={:#x}, stack={:#x}",
        entry,
        stack
    );

    // Set GS data for syscall and Ring 3 switching
    unsafe {
        let selectors = crate::gdt::get_selectors();
        crate::kinfo!(
            "Selectors: user_code={:#x}, user_data={:#x}",
            selectors.user_code_selector.0,
            selectors.user_data_selector.0
        );
        crate::interrupts::set_gs_data(
            entry,
            stack,
            selectors.user_code_selector.0 as u64 | 3,
            selectors.user_data_selector.0 as u64 | 3,
            selectors.user_data_selector.0 as u64 | 3,
        );

        // Set GS base to point to GS_DATA for both kernel and user mode
        use x86_64::registers::model_specific::Msr;
    Msr::new(0xc0000101).write(&raw const crate::initramfs::GS_DATA.0 as *const _ as u64); // GS base
        crate::kinfo!(
            "GS base set to GS_DATA at {:#x}",
            &raw const crate::initramfs::GS_DATA.0 as *const _ as u64
        );
    }

    unsafe {
        let selectors = crate::gdt::get_selectors();
        crate::kinfo!("About to push iretq parameters: ss={:#x}, rsp={:#x}, rflags=0x202, cs={:#x}, rip={:#x}", 
            selectors.user_data_selector.0 as u64 | 3, stack, selectors.user_code_selector.0 as u64 | 3, entry);
        asm!(
            "push {}",      // user ss
            "push {}",      // user stack
            "push 0x202",   // rflags (IF=1)
            "push {}",      // user cs
            "push {}",      // user entry point
            "iretq",
            in(reg) selectors.user_data_selector.0 as u64 | 3,
            in(reg) stack,
            in(reg) selectors.user_code_selector.0 as u64 | 3,
            in(reg) entry,
        );
        // This code never executes
        crate::kinfo!("ERROR: iretq returned!");
    }
}

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
