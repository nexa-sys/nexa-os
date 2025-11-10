use core::arch::asm;
use core::arch::global_asm;
use pic8259::ChainedPics;
use spin;
use x86_64::instructions::port::Port;
use x86_64::registers::model_specific::Msr;
/// Interrupt Descriptor Table (IDT) and interrupt handlers
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Toggle IA32_* MSR configuration for `syscall` fast path.
/// With dynamic linking we now encounter Glibc-generated `syscall` instructions
/// immediately during interpreter startup, so keep this enabled by default and
/// fall back to the legacy `int 0x81` gateway only if MSR programming fails.
const ENABLE_SYSCALL_MSRS: bool = true;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

unsafe fn write_hex_u64(port: &mut x86_64::instructions::port::Port<u8>, value: u64) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for i in (0..16).rev() {
        let shift = i * 4;
        let nibble = ((value >> shift) & 0xF) as usize;
        port.write(HEX[nibble]);
    }
}

global_asm!(
    ".global syscall_interrupt_handler",
    "syscall_interrupt_handler:",
    // On int gate from Ring 3, CPU pushed: RIP, CS, RFLAGS, RSP, SS
    // Save RIP (return address) before we push other registers
    "mov r10, [rsp + 0]", // r10 = user RIP (at top of stack after int)
    // Now save other registers we might clobber
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // Align stack to 16 bytes before calling into Rust (SysV ABI requires
    // %rsp % 16 == 8 at the call site so the callee observes 16-byte alignment).
    "sub rsp, 8",
    // Prepare arguments for syscall_dispatch(nr=rax, arg1=rdi, arg2=rsi, arg3=rdx, syscall_return_addr=r10)
    // System V x86_64 ABI: rdi, rsi, rdx, rcx, r8
    "mov r8, r10", // r8 = syscall_return_addr (from r10)
    "mov rcx, rdx", // rcx = arg3
    "mov rdx, rsi", // rdx = arg2
    "mov rsi, rdi", // rsi = arg1
    "mov rdi, rax", // rdi = nr
    "call syscall_dispatch",
    // Return value already in rax
    "add rsp, 8",
    // Restore registers (reverse order)
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "pop rbx",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "iretq"
);

extern "C" {
    fn syscall_interrupt_handler();
    #[allow(dead_code)]
    fn syscall_handler();
}

/// Exception handlers
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    let ring = stack_frame.code_segment.0 & 3;
    // Low-level marker for breakpoint
    unsafe {
        let mut port = x86_64::instructions::port::Port::new(0x3F8u16);
        port.write(b'B');
    }
    if ring == 3 {
        crate::kinfo!(
            "BREAKPOINT from user mode (Ring 3) at {:#x}",
            stack_frame.instruction_pointer
        );
        // Just return for user mode breakpoints
    } else {
        crate::kerror!("EXCEPTION: BREAKPOINT from Ring {}!", ring);
        crate::kdebug!(
            "RIP: {:#x}, CS: {:#x}",
            stack_frame.instruction_pointer,
            stack_frame.code_segment.0
        );
        loop {
            x86_64::instructions::hlt();
        }
    }
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    let cr2 = Cr2::read().unwrap_or_else(|_| x86_64::VirtAddr::new(0));
    crate::kerror!(
        "EXCEPTION: PAGE FAULT at {:#x}, RIP={:#x}",
        cr2.as_u64(),
        stack_frame.instruction_pointer.as_u64()
    );
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    use x86_64::instructions::interrupts;
    use x86_64::instructions::port::Port;

    unsafe {
        let mut port = Port::<u8>::new(0x3F8);
        port.write(b'G');
        port.write(b'P');
        port.write(b' ');

        write_hex_u64(&mut port, error_code);
        port.write(b' ');
        write_hex_u64(&mut port, stack_frame.instruction_pointer.as_u64());
        port.write(b' ');
        write_hex_u64(&mut port, stack_frame.code_segment.0 as u64);
        port.write(b'\n');
    }

    interrupts::disable();
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    crate::serial::_print(format_args!(
        "\nDOUBLE FAULT: code={:#x} rip={:#x} rsp={:#x} ss={:#x}\n",
        error_code,
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        stack_frame.stack_segment.0
    ));
    crate::kpanic!(
        "DOUBLE FAULT: code={:#x} rip={:#x} rsp={:#x} ss={:#x}",
        error_code,
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        stack_frame.stack_segment.0
    );
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    // Low-level marker for divide error
    unsafe {
        let mut port = x86_64::instructions::port::Port::new(0x3F8u16);
        port.write(b'D');
    }
    crate::kpanic!("EXCEPTION: DIVIDE ERROR\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::kpanic!(
        "EXCEPTION: SEGMENT NOT PRESENT (error: {})\n{:#?}",
        error_code,
        stack_frame
    );
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    // Low-level marker for invalid opcode
    unsafe {
        let mut port = x86_64::instructions::port::Port::new(0x3F8u16);
        port.write(b'I');
    }
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    let mut bytes_at_rip: [u8; 16] = [0; 16];
    let mut bytes_at_rsp: [u8; 16] = [0; 16];
    unsafe {
        let rip_ptr = rip as *const u8;
        let rsp_ptr = rsp as *const u8;
        for i in 0..16 {
            // Use read_volatile so the compiler does not optimise the loads away
            bytes_at_rip[i] = rip_ptr.add(i).read_volatile();
            bytes_at_rsp[i] = rsp_ptr.add(i).read_volatile();
        }
    }
    crate::kpanic!(
        "EXCEPTION: INVALID OPCODE rip={:#x} rsp={:#x} bytes rip={:02x?} stack={:02x?}\n{:#?}",
        rip,
        rsp,
        bytes_at_rip,
        bytes_at_rsp,
        stack_frame
    );
}

// Ring 3 switch handler - interrupt 0x80
global_asm!(
    ".global ring3_switch_handler",
    "ring3_switch_handler:",
    // Stack layout from int 0x80: [ss, rsp, rflags, cs, rip] + pushed values [entry, stack, rflags, cs, ss]
    // We need to set up sysret parameters
    "mov rcx, [rsp + 8]",  // entry point (rip for sysret)
    "mov r11, [rsp + 16]", // rflags
    "mov rsp, [rsp]",      // stack pointer
    // Set user data segments
    "mov ax, 0x23",
    "mov ds, ax",
    "mov es, ax",
    "mov fs, ax",
    "mov gs, ax",
    // Return to user mode
    "sysretq"
);

extern "C" {
    fn ring3_switch_handler();
}

use core::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use lazy_static::lazy_static;

/// SMP/Multi-core IDT Strategy Documentation
///
/// CURRENT IMPLEMENTATION (Single-core):
/// - This IDT is shared across all cores (if SMP is enabled in future)
/// - Initialization happens on the BSP (Bootstrap Processor) only
/// - APs (Application Processors) will load the same IDT via IDT.load()
/// - This is safe for read-only operations but limits per-core customization
///
/// ASSUMPTIONS:
/// 1. IDT initialization completes on BSP before any AP starts
/// 2. All cores share the same interrupt handlers (no per-core handlers yet)
/// 3. IST (Interrupt Stack Table) entries point to BSP stacks (NOT per-core)
/// 4. No concurrent modifications to IDT after initialization
///
/// FUTURE SMP IMPROVEMENTS (TODO):
/// - Implement per-core IDT tables for true isolation
/// - Per-core IST stacks to avoid stack corruption in multi-core scenarios
/// - Per-core interrupt affinity and load balancing
/// - Spinlock protection for any runtime IDT modifications
/// - Proper APIC initialization and IPI handling
///
/// TEMPORARY PROTECTION:
/// - IDT_INITIALIZED flag prevents re-initialization
/// - lazy_static ensures single initialization even with concurrent access
/// - interrupts::disable() during init prevents race conditions
///
/// See: https://wiki.osdev.org/SMP for SMP initialization sequence
/// See: https://wiki.osdev.org/APIC for advanced interrupt routing

/// Flag to track if IDT has been initialized (prevents re-initialization)
static IDT_INITIALIZED: AtomicBool = AtomicBool::new(false);

lazy_static! {
    /// Global IDT instance - using lazy_static to avoid stack overflow
    /// InterruptDescriptorTable is ~4KB and would overflow the stack if created inline
    ///
    /// IMPORTANT: Currently shared across all cores in SMP configurations.
    /// This is safe for our current single-core focus but will need per-core
    /// IDTs for true SMP support with per-core interrupt handling.
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        unsafe {
            // Set up interrupt handlers
            idt.breakpoint.set_handler_fn(breakpoint_handler);
            idt.page_fault.set_handler_fn(page_fault_handler);
            idt.general_protection_fault
                .set_handler_fn(general_protection_fault_handler);
            idt.divide_error.set_handler_fn(divide_error_handler);
            // Use a dedicated IST entry for double fault to ensure the CPU
            // switches to a known-good stack when a double fault occurs. This
            // reduces the chance of a triple fault caused by stack corruption.
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX as u16);
            idt.segment_not_present
                .set_handler_fn(segment_not_present_handler);
            idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
            idt.invalid_tss.set_handler_fn(segment_not_present_handler); // Reuse handler
            idt.stack_segment_fault
                .set_handler_fn(segment_not_present_handler); // Reuse handler

            // Set up hardware interrupts
            idt[PIC_1_OFFSET].set_handler_fn(timer_interrupt_handler);
            idt[PIC_1_OFFSET + 1].set_handler_fn(keyboard_interrupt_handler);
            idt[PIC_1_OFFSET + 2].set_handler_fn(spurious_irq2_handler);
            idt[PIC_1_OFFSET + 3].set_handler_fn(spurious_irq3_handler);
            idt[PIC_1_OFFSET + 4].set_handler_fn(spurious_irq4_handler);
            idt[PIC_1_OFFSET + 5].set_handler_fn(spurious_irq5_handler);
            idt[PIC_1_OFFSET + 6].set_handler_fn(spurious_irq6_handler);
            idt[PIC_1_OFFSET + 7].set_handler_fn(spurious_irq7_handler);

            idt[PIC_2_OFFSET].set_handler_fn(spurious_irq8_handler);
            idt[PIC_2_OFFSET + 1].set_handler_fn(spurious_irq9_handler);
            idt[PIC_2_OFFSET + 2].set_handler_fn(spurious_irq10_handler);
            idt[PIC_2_OFFSET + 3].set_handler_fn(spurious_irq11_handler);
            idt[PIC_2_OFFSET + 4].set_handler_fn(spurious_irq12_handler);
            idt[PIC_2_OFFSET + 5].set_handler_fn(spurious_irq13_handler);
            idt[PIC_2_OFFSET + 6].set_handler_fn(spurious_irq14_handler);
            idt[PIC_2_OFFSET + 7].set_handler_fn(spurious_irq15_handler);

            // Set up syscall interrupt handler at 0x81 (callable from Ring 3)
            use x86_64::PrivilegeLevel;
            idt[0x81]
                .set_handler_addr(x86_64::VirtAddr::new_truncate(
                    syscall_interrupt_handler as u64,
                ))
                .set_privilege_level(PrivilegeLevel::Ring3);

            // Set up ring3 switch handler at 0x80 (also callable from Ring 3)
            idt[0x80]
                .set_handler_addr(x86_64::VirtAddr::new_truncate(ring3_switch_handler as u64))
                .set_privilege_level(PrivilegeLevel::Ring3);
        }

        idt
    };
}

/// Initialize IDT with interrupt handlers
///
/// CRITICAL SAFETY NOTES:
/// 1. This function MUST be called only once per system boot (on BSP)
/// 2. All interrupts are disabled during initialization to prevent races
/// 3. IDT is loaded before unmasking any hardware interrupts
/// 4. Uses atomic flag to prevent accidental re-initialization
///
/// SMP CONSIDERATIONS:
/// - Currently designed for single-core/BSP initialization only
/// - APs should NOT call this function (they should only call IDT.load())
/// - Future: Implement init_interrupts_ap() for per-core setup
///
/// CALL SEQUENCE:
/// 1. Disable interrupts globally
/// 2. Check if already initialized (prevent double-init)
/// 3. Mask all PIC interrupts
/// 4. Initialize PIC hardware
/// 5. Load IDT (triggers lazy_static initialization)
/// 6. Apply final interrupt masks
/// 7. Mark as initialized
pub fn init_interrupts() {
    // Ensure interrupts are disabled during initialization
    // This is critical to prevent race conditions and ensure atomic setup
    x86_64::instructions::interrupts::disable();

    // Check if already initialized (protection against double-init)
    if IDT_INITIALIZED.load(AtomicOrdering::SeqCst) {
        crate::kwarn!("init_interrupts: Already initialized, skipping");
        return;
    }

    // Safe to log now that interrupts are disabled
    crate::kinfo!("init_interrupts: Starting IDT initialization (BSP)");

    // Mask all interrupts BEFORE initializing PICs to prevent spurious interrupts during setup
    unsafe {
        let mut port = Port::<u8>::new(0x21); // Master PIC IMR
        port.write(0xFF);
        let mut port = Port::<u8>::new(0xA1); // Slave PIC IMR
        port.write(0xFF);
    }
    crate::kinfo!("init_interrupts: interrupts masked");

    // Initialize PICs AFTER masking interrupts
    unsafe {
        PICS.lock().initialize();
    }
    crate::kinfo!("init_interrupts: PICs initialized");

    // Load IDT before applying PIC masks to ensure handlers are in place
    // Access to IDT via lazy_static will initialize it on first access
    crate::kinfo!("init_interrupts: loading IDT");

    // Ensure IDT structure is fully written before loading
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    IDT.load();
    // Ensure load completes before continuing
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    crate::kinfo!("init_interrupts: IDT loaded successfully");

    // Now set final PIC masks - unmask only keyboard IRQ
    unsafe {
        let mut master_port = Port::<u8>::new(0x21);
        master_port.write(0xFD); // Unmask only keyboard IRQ (IRQ1)
        let mut slave_port = Port::<u8>::new(0xA1);
        slave_port.write(0xFF); // Keep all slave IRQs masked
    }
    crate::kinfo!("init_interrupts: PIC masks applied (keyboard unmasked)");

    if ENABLE_SYSCALL_MSRS {
        crate::kinfo!("init_interrupts: enabling SYSCALL MSR fast path");
        setup_syscall();
        crate::kinfo!("init_interrupts: setup_syscall completed");
    } else {
        crate::kinfo!("init_interrupts: skipping SYSCALL MSR setup (using int 0x81 gateway)");
    }

    // Mark IDT as initialized to prevent re-initialization
    IDT_INITIALIZED.store(true, AtomicOrdering::SeqCst);

    // Final memory barrier to ensure all initialization is visible to other cores
    core::sync::atomic::fence(AtomicOrdering::SeqCst);

    crate::kinfo!("init_interrupts: Initialization complete and marked ready");
}

/// Check if IDT has been initialized
///
/// Useful for AP (Application Processor) cores in SMP configurations
/// to verify that BSP has completed IDT setup before loading it.
#[allow(dead_code)]
pub fn is_idt_initialized() -> bool {
    IDT_INITIALIZED.load(AtomicOrdering::SeqCst)
}

/// Load IDT on an AP (Application Processor) core
///
/// This function should be called by AP cores after BSP has initialized
/// the shared IDT. It only loads the IDT without re-initializing PICs.
///
/// REQUIREMENTS:
/// - BSP must have called init_interrupts() first
/// - Interrupts should be disabled before calling
/// - PICs are already initialized by BSP
///
/// TODO: In future SMP implementation, this should:
/// - Load per-core IDT instead of shared IDT
/// - Set up per-core APIC instead of PIC
/// - Configure per-core IST stacks
#[allow(dead_code)]
pub fn init_interrupts_ap() {
    // Ensure interrupts are disabled
    x86_64::instructions::interrupts::disable();

    // Verify that BSP has initialized the IDT
    if !is_idt_initialized() {
        crate::kpanic!("AP attempted to load IDT before BSP initialization");
    }

    crate::kinfo!("init_interrupts_ap: Loading IDT on AP core");

    // Load the shared IDT on this core
    core::sync::atomic::compiler_fence(AtomicOrdering::SeqCst);
    IDT.load();
    core::sync::atomic::compiler_fence(AtomicOrdering::SeqCst);

    crate::kinfo!("init_interrupts_ap: IDT loaded on AP core");
}

// Hardware interrupt handlers
extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // Send EOI to PIC
    unsafe {
        PICS.lock().notify_end_of_interrupt(PIC_1_OFFSET);
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    crate::keyboard::add_scancode(scancode);

    // Send EOI to PIC
    unsafe {
        PICS.lock().notify_end_of_interrupt(PIC_1_OFFSET + 1);
    }
}

macro_rules! define_spurious_irq {
    ($name:ident, $vector:expr) => {
        extern "x86-interrupt" fn $name(_stack_frame: InterruptStackFrame) {
            crate::kwarn!("Unhandled IRQ vector {} received; masking line", $vector);
            unsafe {
                PICS.lock().notify_end_of_interrupt($vector);
                if $vector < PIC_2_OFFSET {
                    let irq_index = ($vector - PIC_1_OFFSET) as u8;
                    let mut port = Port::<u8>::new(0x21);
                    let mask = port.read() | (1 << irq_index);
                    port.write(mask);
                    crate::kwarn!("Masked master PIC line {} (IMR={:#010b})", irq_index, mask);
                } else {
                    let irq_index = ($vector - PIC_2_OFFSET) as u8;
                    let mut port = Port::<u8>::new(0xA1);
                    let mask = port.read() | (1 << irq_index);
                    port.write(mask);
                    crate::kwarn!("Masked slave PIC line {} (IMR={:#010b})", irq_index, mask);
                }
            }
        }
    };
}

define_spurious_irq!(spurious_irq2_handler, PIC_1_OFFSET + 2);
define_spurious_irq!(spurious_irq3_handler, PIC_1_OFFSET + 3);
define_spurious_irq!(spurious_irq4_handler, PIC_1_OFFSET + 4);
define_spurious_irq!(spurious_irq5_handler, PIC_1_OFFSET + 5);
define_spurious_irq!(spurious_irq6_handler, PIC_1_OFFSET + 6);
define_spurious_irq!(spurious_irq7_handler, PIC_1_OFFSET + 7);
define_spurious_irq!(spurious_irq8_handler, PIC_2_OFFSET + 0);
define_spurious_irq!(spurious_irq9_handler, PIC_2_OFFSET + 1);
define_spurious_irq!(spurious_irq10_handler, PIC_2_OFFSET + 2);
define_spurious_irq!(spurious_irq11_handler, PIC_2_OFFSET + 3);
define_spurious_irq!(spurious_irq12_handler, PIC_2_OFFSET + 4);
define_spurious_irq!(spurious_irq13_handler, PIC_2_OFFSET + 5);
define_spurious_irq!(spurious_irq14_handler, PIC_2_OFFSET + 6);
define_spurious_irq!(spurious_irq15_handler, PIC_2_OFFSET + 7);

#[unsafe(naked)]
extern "C" fn syscall_instruction_handler() {
    core::arch::naked_asm!(
        // On SYSCALL entry the CPU stores the user return RIP in RCX and the
        // user RFLAGS in R11. Capture that state alongside the user stack so
        // the kernel can restore it exactly before executing SYSRET.
        "mov gs:[0], rsp",  // GS[0]  = user RSP snapshot
        "mov rsp, gs:[8]",  // RSP    = kernel stack top
        "mov gs:[56], rcx", // GS[7]  = user return RIP (RCX)
        "mov gs:[64], r11", // GS[8]  = user RFLAGS (R11)
        // Preserve callee-saved registers that Rust expects us to maintain.
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push rbx",
        "push rbp",
        // See note in int 0x81 handler: ensure 16-byte stack alignment before call.
        "sub rsp, 8",
        // Arrange SysV ABI arguments for syscall_dispatch(nr, arg1, arg2, arg3).
        "mov rcx, rdx", // rcx = arg3
        "mov rdx, rsi", // rdx = arg2
        "mov rsi, rdi", // rsi = arg1
        "mov rdi, rax", // rdi = syscall number
        "call syscall_dispatch",
        "add rsp, 8",
        // Restore the callee-saved register set before we leave the kernel stack.
        "pop rbp",
        "pop rbx",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",
        // Recover the saved user return context and jump back with SYSRETQ.
        "mov rcx, gs:[56]", // rcx = user RIP
        "mov r11, gs:[64]", // r11 = user RFLAGS
        "mov rsp, gs:[0]",  // rsp = user RSP
        "sysretq",
    );
}

#[unsafe(no_mangle)]
extern "C" fn syscall_instruction_handler_inner() {
    // This function is called from naked assembly
    // Registers: rax=syscall_num, rdi=arg1, rsi=arg2, rdx=arg3
    let syscall_num: u64;
    let arg1: u64;
    let arg2: u64;
    let arg3: u64;

    unsafe {
        asm!(
            "mov {}, rax",
            "mov {}, rdi",
            "mov {}, rsi",
            "mov {}, rdx",
            out(reg) syscall_num,
            out(reg) arg1,
            out(reg) arg2,
            out(reg) arg3,
        );
    }

    crate::kdebug!(
        "SYSCALL_INSTRUCTION_HANDLER: syscall={} arg1={:#x} arg2={:#x} arg3={:#x}",
        syscall_num,
        arg1,
        arg2,
        arg3
    );

    if syscall_num == 1 {
        // write
        let fd = arg1;
        let buf_ptr = arg2 as *const u8;
        let count = arg3 as usize;

        crate::kdebug!(
            "SYSCALL: write fd={} buf={:#x} count={}",
            fd,
            buf_ptr as u64,
            count
        );

        // For simplicity, assume fd=1 and print to VGA and serial
        for i in 0..count {
            let byte = unsafe { *buf_ptr.add(i) };
            crate::kdebug!("SYSCALL: writing byte {}", byte as char);
            write_char_to_vga(byte);
            write_char_to_serial(byte);
        }

        // Return count
        unsafe {
            asm!("mov rax, {}", in(reg) count as u64);
        }
    } else {
        crate::kdebug!("SYSCALL: unknown syscall {}", syscall_num);
        unsafe {
            asm!("mov rax, {}", in(reg) (-1i64 as u64));
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn write_char_to_vga(c: u8) {
    use core::fmt::Write;
    crate::vga_buffer::with_writer(|writer| {
        let _ = write!(writer, "{}", c as char);
    });
}

#[unsafe(no_mangle)]
extern "C" fn write_char_to_serial(c: u8) {
    crate::serial::_print(format_args!("{}", c as char));
}

// Debug function for Ring 3 switch
#[unsafe(no_mangle)]
extern "C" fn ring3_debug_print() {
    // This function is called from assembly with registers set
    // rsi = entry, rdi = stack, rdx = cs, rcx = ss, r8 = ds
    let entry: u64;
    let stack: u64;
    let cs: u64;
    let ss: u64;
    let ds: u64;

    unsafe {
        asm!(
            "mov {}, rsi",
            "mov {}, rdi",
            "mov {}, rdx",
            "mov {}, rcx",
            "mov {}, r8",
            out(reg) entry,
            out(reg) stack,
            out(reg) cs,
            out(reg) ss,
            out(reg) ds,
        );
    }

    crate::kinfo!(
        "RING3_SWITCH: entry={:#x}, stack={:#x}, cs={:#x}, ss={:#x}, ds={:#x}",
        entry,
        stack,
        cs,
        ss,
        ds
    );
}

// Debug function for Ring 3 switch GS check
#[unsafe(no_mangle)]
extern "C" fn ring3_debug_print2() {
    // This function is called from assembly with registers set
    // rax = gs:[0], rbx = gs:[8], rcx = gs:[40]
    let gs0: u64;
    let gs8: u64;
    let gs40: u64;

    unsafe {
        asm!(
            "mov {}, rax",
            "mov {}, rbx",
            "mov {}, rcx",
            out(reg) gs0,
            out(reg) gs8,
            out(reg) gs40,
        );
    }

    crate::kdebug!(
        "GS check: gs:[0]={:#x}, gs:[8]={:#x}, gs:[40]={:#x}",
        gs0,
        gs8,
        gs40
    );
}

/// Set GS data for Ring 3 switch
pub unsafe fn set_gs_data(entry: u64, stack: u64, user_cs: u64, user_ss: u64, user_ds: u64) {
    // Get kernel stack from TSS privilege stack table
    let kernel_stack = crate::gdt::get_kernel_stack_top();

    // Get GS_DATA address without creating a reference that might corrupt nearby statics
    let gs_data_addr = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
    let gs_data_ptr = gs_data_addr as *mut u64;

    unsafe {
        gs_data_ptr.add(0).write(stack); // user RSP at gs:[0]
        gs_data_ptr.add(1).write(kernel_stack); // kernel RSP at gs:[8]
        gs_data_ptr.add(2).write(entry); // USER_ENTRY at gs:[16]
        gs_data_ptr.add(3).write(stack); // USER_STACK at gs:[24]
        gs_data_ptr.add(4).write(user_cs); // user_cs at gs:[32]
        gs_data_ptr.add(5).write(user_ss); // user_ss at gs:[40]
        gs_data_ptr.add(6).write(user_ds); // user_ds at gs:[48]
        gs_data_ptr.add(7).write(0); // Clear saved RCX slot
        gs_data_ptr.add(8).write(0); // Clear saved RFLAGS slot
    }
}

pub fn setup_syscall() {
    let handler_addr = syscall_instruction_handler as u64;

    crate::kinfo!("Setting syscall handler to {:#x}", handler_addr);

    if !cpu_supports_syscall() {
        crate::kwarn!("CPU lacks SYSCALL/SYSRET support; skipping setup");
        return;
    }

    if !is_canonical_address(handler_addr) {
        crate::kerror!(
            "SYSCALL handler address {:#x} is non-canonical; keeping int 0x81 path",
            handler_addr
        );
        return;
    }

    let selectors = unsafe { crate::gdt::get_selectors() };
    let kernel_cs = selectors.code_selector.0 as u16;
    let kernel_ss = selectors.data_selector.0 as u16;
    let user_cs = selectors.user_code_selector.0 as u16;
    let user_ss = selectors.user_data_selector.0 as u16;

    if kernel_ss != kernel_cs + 8 {
        crate::kwarn!(
            "Kernel SS ({:#x}) does not equal kernel CS+8 ({:#x}); STAR will assume the latter",
            kernel_ss,
            kernel_cs + 8
        );
    }

    if user_ss != user_cs + 8 {
        crate::kwarn!(
            "User SS ({:#x}) does not equal user CS+8 ({:#x}); STAR will assume the latter",
            user_ss,
            user_cs + 8
        );
    }

    let kernel_cs_star = (kernel_cs & !0x7) as u64; // ensure RPL=0
    let user_cs_star = ((user_cs | 0x3) & 0xFFFF) as u64; // ensure RPL=3

    let star_value = (kernel_cs_star << 32) | (user_cs_star << 48);
    crate::kdebug!(
        "MSR: STAR composed from selectors kernel_cs={:#x}, user_cs={:#x} -> {:#x}",
        kernel_cs,
        user_cs,
        star_value
    );

    unsafe {
        // Get GS_DATA address without creating a reference that might corrupt nearby statics
        let gs_data_addr = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;

        // Initialize GS data for syscall - write directly to the address
        let gs_data_ptr = gs_data_addr as *mut u64;
        gs_data_ptr.add(1).write(crate::gdt::get_kernel_stack_top()); // Kernel stack for syscall at gs:[8]
        crate::kdebug!(
            "setup_syscall: initramfs available? {}",
            crate::initramfs::get().is_some()
        );

        crate::kdebug!("MSR: about to write KERNEL_GS_BASE");
        Msr::new(0xc0000102).write(gs_data_addr); // Kernel GS base used by swapgs
        crate::kdebug!("MSR: KERNEL_GS_BASE written");

        // Set GS base to GS_DATA address
        // GS base is already set in kernel_main before interrupt initialization
        // let gs_base = gs_data_addr;
        // Msr::new(0xc0000101).write(gs_base); // GS base

        // Use kernel logging for MSR write tracing so it follows the
        // kernel logging convention (serial + optional VGA). logger
        // will skip VGA until it's ready, so this is safe during early boot.
        crate::kdebug!("MSR: about to enable EFER.SCE");
        let mut efer_msr = Msr::new(0xc0000080);
        let mut efer_val = efer_msr.read();
        let had_sce = (efer_val & (1 << 0)) != 0;
        efer_val |= 1 << 0; // IA32_EFER.SCE
        efer_msr.write(efer_val);
        crate::kdebug!(
            "MSR: EFER updated (prev_sce={}, new_val={:#x})",
            had_sce,
            efer_val
        );

        crate::kdebug!("MSR: about to write STAR");
        Msr::new(0xc0000081).write(star_value);
        crate::kdebug!("MSR: STAR written");

        // Point LSTAR to the Rust/assembly syscall handler which prepares
        // arguments (moves rax->rdi, etc.) and uses sysretq.
        crate::kdebug!("MSR: about to write LSTAR");
        Msr::new(0xc0000082).write(handler_addr); // LSTAR
        let lstar_val = Msr::new(0xc0000082).read();
        crate::kdebug!(
            "MSR: LSTAR written (handler={:#x}, readback={:#x})",
            handler_addr,
            lstar_val
        );

        crate::kdebug!("MSR: about to write FMASK");
        Msr::new(0xc0000084).write(0x200); // FMASK
        crate::kdebug!("MSR: FMASK written");
    }
}

fn cpu_supports_syscall() -> bool {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let res = core::arch::x86_64::__cpuid(0x8000_0001);
        (res.edx & (1 << 11)) != 0
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

fn is_canonical_address(addr: u64) -> bool {
    let sign = (addr >> 47) & 1;
    let upper = addr >> 48;
    if sign == 0 {
        upper == 0
    } else {
        upper == 0xFFFF
    }
}
