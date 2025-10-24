/// Process management for user-space execution
use crate::elf::ElfLoader;
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
        crate::kinfo!("Process::from_elf called with {} bytes of ELF data", elf_data.len());
        
        // Check if the data looks like a valid ELF
        if elf_data.len() < 64 {
            crate::kerror!("ELF data too small: {} bytes", elf_data.len());
            return Err("ELF data too small");
        }
        
        // Check ELF magic
        if &elf_data[0..4] != b"\x7fELF" {
            crate::kerror!("Invalid ELF magic: {:02x} {:02x} {:02x} {:02x}", 
                elf_data[0], elf_data[1], elf_data[2], elf_data[3]);
            return Err("Invalid ELF magic");
        }
        
        crate::kinfo!("ELF magic is valid");
        
        let loader = ElfLoader::new(elf_data)?;
        crate::kinfo!("ElfLoader created successfully");
        
        // Allocate user space memory
        const USER_BASE: u64 = 0x400000; // Standard Linux user space base
        const STACK_SIZE: u64 = 0x100000; // 1MB stack
        const HEAP_SIZE: u64 = 0x100000; // 1MB heap

        // Load ELF
        crate::kinfo!("About to call loader.load with base_addr={:#x}", USER_BASE);
        let entry_point = loader.load(USER_BASE)?;
        crate::kinfo!("ELF loaded successfully, entry_point={:#x}", entry_point);

        let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);

        // Initialize user stack in user space (not kernel space)
        let stack_base = USER_BASE + 0x200000; // User stack at 0x600000
        let stack_size = STACK_SIZE;

        Ok(Process {
            pid,
            state: ProcessState::Ready,
            entry_point,
            stack_top: (USER_BASE + 0x200000 - 16) & !15, // 64KB stack space, 16-byte aligned
            heap_start: stack_base + stack_size,
            heap_end: stack_base + stack_size + HEAP_SIZE,
        })
    }

    /// Execute the process in user mode (Ring 3)
    pub fn execute(&mut self) {
        self.state = ProcessState::Running;
        
        crate::kinfo!("Executing process PID={}, entry={:#x}, stack={:#x}", 
            self.pid, self.entry_point, self.stack_top);
        
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
pub unsafe fn jump_to_usermode(entry: u64, stack: u64) {
    // This should never return
    unsafe {
        core::arch::asm!(
            // Set up iretq stack for Ring 3
            "push 0x23",        // SS (user data segment, RPL=3)
            "push {1}",         // RSP (user stack)
            "push 0x202",       // RFLAGS (IF=1, reserved=1)
            "push 0x1B",        // CS (user code segment, RPL=3)
            "push {0}",         // RIP (user program entry)
            "iretq",
            in(reg) entry,
            in(reg) stack,
        );
    }
    
    // If we get here, iretq failed - this should never happen
    crate::kerror!("ERROR: iretq returned! This should not happen.");
}
