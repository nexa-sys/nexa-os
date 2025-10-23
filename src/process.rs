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
        
        // Allocate user space memory (simplified - should use proper memory management)
        const USER_BASE: u64 = 0x400000; // Standard Linux user space base
        const STACK_SIZE: u64 = 0x100000; // 1MB stack
        const HEAP_SIZE: u64 = 0x100000; // 1MB heap

        // Load ELF
        let entry_point = loader.load(USER_BASE)?;

        let pid = NEXT_PID.fetch_add(1, Ordering::SeqCst);

        Ok(Process {
            pid,
            state: ProcessState::Ready,
            entry_point,
            stack_top: USER_BASE + 0x800000 + STACK_SIZE, // 8MB offset for code/data
            heap_start: USER_BASE + 0x800000 + STACK_SIZE,
            heap_end: USER_BASE + 0x800000 + STACK_SIZE + HEAP_SIZE,
        })
    }

    /// Execute the process
    pub fn execute(&mut self) -> Result<(), &'static str> {
        self.state = ProcessState::Running;
        
        // Setup user mode context and jump to entry point
        unsafe {
            switch_to_user_mode(self.entry_point, self.stack_top);
        }

        Ok(())
    }
}

/// Switch to user mode (ring 3) and execute at the given entry point
#[inline(never)]
unsafe fn switch_to_user_mode(entry: u64, stack: u64) {
    // Setup segments for user mode
    // User code segment: GDT entry 3 (0x18), RPL=3 -> 0x1B
    // User data segment: GDT entry 4 (0x20), RPL=3 -> 0x23
    
    core::arch::asm!(
        // Push SS (user data segment with RPL=3)
        "push 0x23",
        // Push RSP (user stack pointer)
        "push {stack}",
        // Push RFLAGS (enable interrupts)
        "pushfq",
        "pop rax",
        "or rax, 0x200",  // Set IF (interrupt enable flag)
        "push rax",
        // Push CS (user code segment with RPL=3)
        "push 0x1B",
        // Push RIP (entry point)
        "push {entry}",
        // Use iretq to switch to ring 3
        "iretq",
        entry = in(reg) entry,
        stack = in(reg) stack,
        options(noreturn)
    );
}

/// System call handler
#[no_mangle]
pub extern "C" fn syscall_handler() {
    // System call implementation
    // RAX: syscall number
    // RDI, RSI, RDX, R10, R8, R9: arguments
    
    let syscall_number: u64;
    let arg1: u64;
    let arg2: u64;
    let arg3: u64;
    
    unsafe {
        core::arch::asm!(
            "",
            out("rax") syscall_number,
            out("rdi") arg1,
            out("rsi") arg2,
            out("rdx") arg3,
        );
    }
    
    match syscall_number {
        1 => syscall_write(arg1, arg2 as *const u8, arg3),
        60 => syscall_exit(arg1 as i32),
        _ => {
            crate::kwarn!("Unknown syscall: {}", syscall_number);
        }
    }
}

/// Write system call
fn syscall_write(fd: u64, buf: *const u8, count: u64) {
    if fd == 1 || fd == 2 {
        // stdout or stderr
        let slice = unsafe { core::slice::from_raw_parts(buf, count as usize) };
        if let Ok(s) = core::str::from_utf8(slice) {
            crate::kprint!("{}", s);
        }
    }
}

/// Exit system call
fn syscall_exit(code: i32) {
    crate::kinfo!("Process exited with code: {}", code);
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}
