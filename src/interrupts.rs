/// Interrupt Descriptor Table (IDT) and interrupt handlers
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use lazy_static::lazy_static;
use spin;
use pic8259::ChainedPics;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> = spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

// Exception handlers
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    crate::serial_println!("BREAKPOINT_HANDLER: int 3 triggered from Ring {}!", (stack_frame.code_segment.0 & 3));
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
    crate::kerror!("RIP: {:#x}", stack_frame.instruction_pointer);
    crate::kerror!("CS: {:#x}", stack_frame.code_segment.0);
    crate::kerror!("RFLAGS: {:#x}", stack_frame.cpu_flags);
    crate::kerror!("RSP: {:#x}", stack_frame.stack_pointer);
    crate::kerror!("SS: {:#x}", stack_frame.stack_segment.0);
    crate::kerror!("{:#?}", stack_frame);
    
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    crate::serial_println!("EXCEPTION: DIVIDE ERROR\n{:#?}", stack_frame);
    loop {}
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT (error: {})\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn segment_not_present_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic!("EXCEPTION: SEGMENT NOT PRESENT (error: {})\n{:#?}", error_code, stack_frame);
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler)
            .set_privilege_level(x86_64::PrivilegeLevel::Ring3);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        
        // Timer interrupt (IRQ0 -> INT 32)
        idt[32].set_handler_fn(timer_interrupt_handler);
        
        // Keyboard interrupt (IRQ1 -> INT 33)
        idt[33].set_handler_fn(keyboard_interrupt_handler);
        
        // System call interrupt (INT 0x80)
        unsafe {
            idt[0x80].set_handler_fn(syscall_handler)
                .set_privilege_level(x86_64::PrivilegeLevel::Ring3)
                .set_stack_index(0); // Use privilege stack table[0]
        }
        
        idt
    };
}

/// Initialize IDT with interrupt handlers
pub fn init() {
    IDT.load();
    crate::kinfo!("IDT loaded successfully");
    crate::kinfo!("IDT[0x80] handler: {:?}", IDT[0x80]);
    
    // Test if we can trigger breakpoint interrupt manually
    crate::kinfo!("Testing breakpoint interrupt...");
    x86_64::instructions::interrupts::int3();
    
    // Initialize PIC
    unsafe {
        PICS.lock().initialize();
    }

    crate::kinfo!("IDT initialized with system call and keyboard support");
    crate::kinfo!("System call handler at interrupt 0x80");
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

extern "x86-interrupt" fn syscall_handler(stack_frame: InterruptStackFrame) {
    // Debug output to confirm handler is called
    crate::serial_println!("SYSCALL_HANDLER: INT 0x80 triggered from Ring {}!", (stack_frame.code_segment.0 & 3));
    
    // For now, just return 0 as syscall result
    // We'll implement proper syscall handling later
    
    crate::serial_println!("SYSCALL_HANDLER: Returning 0");
    
    // No EOI needed for software interrupts (INT 0x80)
}
