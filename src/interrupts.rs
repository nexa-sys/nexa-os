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
    // On int gate CPU pushed: RFLAGS, CS, RIP, (SS, RSP on CPL change)
    // Save registers we might clobber. Preserve callee-saved too (rbx, rbp, r12-r15)
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
    // Call syscall_dispatch(nr=rax, arg1=rdi, arg2=rsi, arg3=rdx)
    "mov rcx, rdx", // rcx = arg3
    "mov rdx, rsi", // rdx = arg2
    "mov rsi, rdi", // rsi = arg1
    "mov rdi, rax", // rdi = nr
    "call syscall_dispatch",
    // Return value already in rax
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
    crate::kerror!(
        "DOUBLE FAULT: code={:#x} rip={:#x} rsp={:#x} ss={:#x}",
        error_code,
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        stack_frame.stack_segment.0
    );
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    // Low-level marker for divide error
    unsafe {
        let mut port = x86_64::instructions::port::Port::new(0x3F8u16);
        port.write(b'D');
    }
    crate::kerror!("EXCEPTION: DIVIDE ERROR\n{:#?}", stack_frame);
    loop {}
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::kerror!(
        "EXCEPTION: SEGMENT NOT PRESENT (error: {})\n{:#?}",
        error_code,
        stack_frame
    );
    loop {}
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    // Low-level marker for invalid opcode
    unsafe {
        let mut port = x86_64::instructions::port::Port::new(0x3F8u16);
        port.write(b'I');
    }
    crate::kerror!("EXCEPTION: INVALID OPCODE\n{:#?}", stack_frame);
    loop {}
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

/// Global IDT instance - initialized at runtime to avoid static initialization conflicts
static mut IDT: Option<InterruptDescriptorTable> = None;

/// Initialize IDT with interrupt handlers
pub fn init_interrupts() {
    crate::kinfo!("init_interrupts: START");
    let idt_ptr = core::ptr::addr_of!(IDT) as usize;
    crate::kdebug!("IDT storage address: {:#x}", idt_ptr);
    let gs_ptr = unsafe { &raw const crate::initramfs::GS_DATA.0 as *const _ } as usize;
    crate::kdebug!("GS_DATA address: {:#x}", gs_ptr);

    // Initialize IDT at runtime instead of using lazy_static
    unsafe {
        IDT = Some({
            let mut idt = InterruptDescriptorTable::new();

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

            idt
        });
    }

    crate::kinfo!("init_interrupts: IDT initialized");

    // Mask all interrupts BEFORE initializing PICs to prevent spurious interrupts during setup
    crate::kinfo!("init_interrupts: about to mask interrupts");
    unsafe {
        crate::kinfo!("init_interrupts: masking master PIC (0x21)");
        let mut port = Port::<u8>::new(0x21); // Master PIC IMR
        port.write(0xFF);
        crate::kinfo!("init_interrupts: master PIC masked");

        crate::kinfo!("init_interrupts: masking slave PIC (0xA1)");
        let mut port = Port::<u8>::new(0xA1); // Slave PIC IMR
        port.write(0xFF);
        crate::kinfo!("init_interrupts: slave PIC masked");
    }
    crate::kinfo!("init_interrupts: interrupts masked");

    // Initialize PICs AFTER masking interrupts
    crate::kinfo!("init_interrupts: about to initialize PICs");
    unsafe {
        PICS.lock().initialize();
    }
    crate::kinfo!("init_interrupts: PICs initialized");

    crate::kinfo!("init_interrupts: about to call setup_syscall");
    setup_syscall();
    crate::initramfs::debug_dump_state("after-setup-syscall");
    crate::kinfo!("init_interrupts: setup_syscall completed");

    // Load IDT LAST to avoid corrupting static variables
    unsafe {
        if let Some(ref idt) = IDT {
            idt.load();
        }
    }
    crate::kinfo!("init_interrupts: IDT loaded");

    unsafe {
        let mut master_port = Port::<u8>::new(0x21);
        let current_mask = master_port.read();
        let new_mask = current_mask & !(1 << 1); // Unmask keyboard IRQ (IRQ1)
        master_port.write(new_mask);
        crate::kinfo!(
            "init_interrupts: keyboard IRQ unmasked (IMR={:#010b})",
            new_mask
        );
    }

    crate::initramfs::debug_dump_state("after-init-interrupts");
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

#[unsafe(naked)]
extern "C" fn syscall_instruction_handler() {
    core::arch::naked_asm!(
        // syscall entry: RIP->RCX, RFLAGS->R11, CS/SS set from STAR, RSP->RSP0

        // GS base is already set to GS_DATA in setup_syscall
        "mov gs:[0], rsp", // Save user RSP
        "mov rsp, gs:[8]", // Load kernel RSP
        // Save syscall args
        "push rax",
        "push rdi",
        "push rsi",
        "push rdx",
        // Call handler
        "call syscall_dispatch",
        // Save return value
        "push rax",
        // Restore syscall args
        "pop rdx",
        "pop rsi",
        "pop rdi",
        "add rsp, 8", // Skip original rax on stack
        // Restore return value to rax
        "pop rax",
        // Prepare stack for sysret: RIP, RFLAGS, RSP
        "push rcx",    // User RIP
        "push r11",    // User RFLAGS
        "push gs:[0]", // User RSP
        "sysret",      // Return to user mode
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
    }
}

pub fn setup_syscall() {
    // Setup syscall
    crate::kinfo!(
        "Setting syscall handler to {:#x}",
        syscall_instruction_handler as u64
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

        // Set GS base to GS_DATA address
        // GS base is already set in kernel_main before interrupt initialization
        // let gs_base = gs_data_addr;
        // Msr::new(0xc0000101).write(gs_base); // GS base

        // Use kernel logging for MSR write tracing so it follows the
        // kernel logging convention (serial + optional VGA). logger
        // will skip VGA until it's ready, so this is safe during early boot.
        crate::kdebug!("MSR: about to set EFER.SCE");
        Msr::new(0xc0000080).write(1 << 0); // IA32_EFER.SCE = 1
        crate::kdebug!("MSR: EFER.SCE set");

        crate::kdebug!("MSR: about to write STAR");
        Msr::new(0xc0000081).write((0x08 << 32) | (0x1b << 48)); // STAR
        crate::kdebug!("MSR: STAR written");

        // Point LSTAR to the Rust/assembly syscall handler which prepares
        // arguments (moves rax->rdi, etc.) and uses sysretq.
        crate::kdebug!("MSR: about to write LSTAR");
        Msr::new(0xc0000082).write(syscall_handler as u64); // LSTAR
        crate::kdebug!("MSR: LSTAR written");

        crate::kdebug!("MSR: about to write FMASK");
        Msr::new(0xc0000084).write(0x200); // FMASK
        crate::kdebug!("MSR: FMASK written");
    }
}
