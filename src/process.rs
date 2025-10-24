/// Process management for user-space execution
use crate::elf::ElfLoader;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::gdt;

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

        crate::kinfo!("Constants defined: USER_BASE={:#x}, STACK_SIZE={:#x}", USER_BASE, STACK_SIZE);

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
    crate::kinfo!("About to execute int 0x80 with entry={:#x}, stack={:#x}", entry, stack);
    
    // Debug: check if user code is loaded
    unsafe {
        let code_ptr = entry as *const u8;
        let code_bytes = [code_ptr.read(), code_ptr.add(1).read(), code_ptr.add(2).read(), code_ptr.add(3).read()];
        crate::kinfo!("User code at {:#x}: {:02x} {:02x} {:02x} {:02x}", entry, code_bytes[0], code_bytes[1], code_bytes[2], code_bytes[3]);
    }
    
    // Store entry and stack in global variables for the interrupt handler
    unsafe {
        USER_ENTRY = entry;
        USER_STACK = stack;
    }
    
    // Trigger interrupt 0x80 to switch to user mode
    unsafe {
        core::arch::asm!("int 0x80");
    }
    
    // If we get here, the switch failed
    crate::kerror!("ERROR: Ring 3 switch failed!");
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
