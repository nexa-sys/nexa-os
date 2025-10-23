/// Interrupt Descriptor Table (IDT) and interrupt handlers
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use spin::Mutex;

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

/// Initialize IDT with interrupt handlers
pub fn init() {
    unsafe {
        IDT.breakpoint.set_handler_fn(breakpoint_handler);
        IDT.page_fault.set_handler_fn(page_fault_handler);
        IDT.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        IDT.divide_error.set_handler_fn(divide_error_handler);
        
        // Keyboard interrupt (IRQ1 -> INT 0x21)
        IDT[0x21].set_handler_fn(keyboard_interrupt_handler);
        
        // System call interrupt (INT 0x80)
        IDT[0x80].set_handler_fn(syscall_handler)
            .set_privilege_level(x86_64::PrivilegeLevel::Ring3);
        
        IDT.load();
    }

    // Enable interrupts in PIC
    unsafe {
        // Initialize PIC
        pic8259::ChainedPics::new(0x20, 0x28).initialize();
        
        // Enable keyboard interrupt (IRQ1)
        let mut port = x86_64::instructions::port::Port::<u8>::new(0x21);
        let mask = port.read();
        port.write(mask & !0x02); // Enable IRQ1
    }

    crate::kinfo!("IDT initialized with system call and keyboard support");
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    crate::kinfo!("BREAKPOINT: {:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    
    crate::kerror!("PAGE FAULT");
    crate::kerror!("Accessed Address: {:?}", Cr2::read());
    crate::kerror!("Error Code: {:?}", error_code);
    crate::kerror!("{:#?}", stack_frame);
    
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::kerror!("GENERAL PROTECTION FAULT");
    crate::kerror!("Error Code: {:#x}", error_code);
    crate::kerror!("{:#?}", stack_frame);
    
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    crate::kerror!("DIVIDE ERROR: {:#?}", stack_frame);
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    
    crate::keyboard::add_scancode(scancode);
    
    // Send EOI to PIC
    unsafe {
        let mut pic = Port::<u8>::new(0x20);
        pic.write(0x20);
    }
}

extern "x86-interrupt" fn syscall_handler(stack_frame: InterruptStackFrame) {
    // System call handling via registers
    // For now, we'll just acknowledge the interrupt
    // Real syscall implementation would use SYSCALL/SYSRET instructions
    
    // Send EOI
    unsafe {
        use x86_64::instructions::port::Port;
        let mut pic = Port::<u8>::new(0x20);
        pic.write(0x20);
    }
}
