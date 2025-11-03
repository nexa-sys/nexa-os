/// Process management for user-space execution
use crate::elf::ElfLoader;
use core::arch::asm;
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
/// Total virtual span that must be mapped for the userspace image, heap, and stack.
pub const USER_REGION_SIZE: u64 = (STACK_BASE + STACK_SIZE) - USER_VIRT_BASE;
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
        crate::kinfo!(
            "Userspace layout: phys_base={:#x}, virt_base={:#x}, stack_base={:#x}, stack_size={:#x}",
            USER_PHYS_BASE,
            USER_VIRT_BASE,
            STACK_BASE,
            STACK_SIZE
        );

        // Load ELF
        crate::kinfo!(
            "About to call loader.load with phys_base={:#x} (virt base {:#x})",
            USER_PHYS_BASE,
            USER_VIRT_BASE
        );
        let physical_entry = loader.load(USER_PHYS_BASE)?;
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
        let stack_base = STACK_BASE; // Virtual stack base (identity mapped)
        let stack_size = STACK_SIZE;
        // SysV ABI expects 16-byte alignment before a CALL instruction pushes the
        // return RIP. Because we jump directly into user mode with IRET (no call),
        // we need to enter with RSP % 16 == 8 so that typical function prologues
        // realign the stack correctly before using SSE instructions (e.g. movaps).
        let stack_top = stack_base + stack_size - 8;

        let process = Process {
            pid,
            state: ProcessState::Ready,
            entry_point: virtual_entry, // Use virtual entry point for Ring 3 execution
            stack_top,
            heap_start: HEAP_BASE,
            heap_end: HEAP_BASE + HEAP_SIZE,
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

        crate::logger::disable_runtime_console_output();

        // Jump to user mode - this never returns
        jump_to_usermode(self.entry_point, self.stack_top);
        // If we get here, iretq failed
        crate::kerror!("Failed to jump to user mode!");
    }
}

/// Jump to user mode (Ring 3) and execute code at given address
/// This function never returns - execution continues in user space
#[inline(never)]
pub fn jump_to_usermode(entry: u64, stack: u64) {
    crate::kdebug!(
        "About to execute iretq with entry={:#x}, stack={:#x}",
        entry,
        stack
    );

    // Set GS data for syscall and Ring 3 switching
    unsafe {
        let selectors = crate::gdt::get_selectors();
        crate::kdebug!(
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
        let gs_base = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
        Msr::new(0xc0000101).write(gs_base);
        crate::kdebug!("GS base set to GS_DATA at {:#x}", gs_base);
    }

    unsafe {
        // Touch the top of the user stack to ensure the mapping is present and
        // writable before we attempt to transition. If this write triggers a
        // fault we will catch it while still on the kernel stack, which makes
        // debugging substantially easier than chasing a double fault.
        let stack_top_ptr = (stack - 8) as *mut u64;
        stack_top_ptr.write_volatile(0xdeadbeefdeadbeef);

        let rsp_before: u64;
        core::arch::asm!("mov {}, rsp", out(reg) rsp_before);
        crate::kdebug!(
            "Kernel RSP before iret: {:#x} (mod16={})",
            rsp_before,
            rsp_before & 0xF
        );
        let selectors = crate::gdt::get_selectors();
        let user_ss = selectors.user_data_selector.0 | 3;
        let user_cs = selectors.user_code_selector.0 | 3;
        crate::kdebug!(
            "About to push iretq parameters: ss={:#x}, rsp={:#x}, rflags=0x202, cs={:#x}, rip={:#x}",
            user_ss,
            stack,
            user_cs,
            entry
        );
        asm!(
            "push {ss}",
            "push {stack}",
            "push 0x202",
            "push {cs}",
            "push {entry}",
            "iretq",
            ss = in(reg) user_ss as u64,
            stack = in(reg) stack,
            cs = in(reg) user_cs as u64,
            entry = in(reg) entry,
            options(noreturn)
        );
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
