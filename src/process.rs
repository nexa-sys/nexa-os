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
        let loader = ElfLoader::new(elf_data)?;
        
        // Allocate user space memory
        const USER_BASE: u64 = 0x400000; // Standard Linux user space base
        const STACK_SIZE: u64 = 0x100000; // 1MB stack
        const HEAP_SIZE: u64 = 0x100000; // 1MB heap

        // Load ELF
        let entry_point = loader.load(USER_BASE)?;

        let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);

        // Initialize user stack in user space (not kernel space)
        let stack_base = USER_BASE + 0x200000; // User stack at 0x600000
        let stack_size = STACK_SIZE;
        unsafe {
            core::ptr::write_bytes(stack_base as *mut u8, 0, stack_size as usize);
        }

        Ok(Process {
            pid,
            state: ProcessState::Ready,
            entry_point,
            stack_top: (stack_base + stack_size) & !15, // 16-byte align
            heap_start: stack_base + stack_size,
            heap_end: stack_base + stack_size + HEAP_SIZE,
        })
    }

    /// Execute the process in user mode (Ring 3)
    pub fn execute(&mut self) -> ! {
        self.state = ProcessState::Running;
        
        crate::kinfo!("Executing process PID={}, entry={:#x}, stack={:#x}", 
            self.pid, self.entry_point, self.stack_top);
        
        // Jump to user mode - this never returns
        unsafe {
            jump_to_usermode(self.entry_point, self.stack_top);
        }
    }
}

/// Jump to user mode (Ring 3) and execute code at given address
/// This function never returns - execution continues in user space
#[inline(never)]
pub unsafe fn jump_to_usermode(entry: u64, stack: u64) -> ! {
    crate::kinfo!("Jumping to user mode: entry={:#x}, stack={:#x}", entry, stack);
    
    // User code segment: GDT index 3, RPL=3 -> (3 << 3) | 3 = 0x1B
    // User data segment: GDT index 4, RPL=3 -> (4 << 3) | 3 = 0x23
    
    core::arch::asm!(
        // Set up data segments for user mode BEFORE pushing stack
        "mov ax, 0x23",      // User data segment selector
        "mov ds, ax",
        "mov es, ax",
        "mov fs, ax",
        "mov gs, ax",
        
        // Push stack frame for iretq (correct order: SS, RSP, RFLAGS, CS, RIP)
        "push 0x23",         // SS (user data segment)
        "push {stack}",      // RSP (user stack pointer)
        "push 0x3202",       // RFLAGS (IF=1, IOPL=3, reserved=1)
        "push 0x1B",         // CS (user code segment)
        "push {entry}",      // RIP (entry point)
        
        // Switch to user mode via iretq
        "iretq",
        
        entry = in(reg) entry,
        stack = in(reg) stack,
        options(noreturn)
    );
}
