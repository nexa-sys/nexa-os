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
        idt[0x80].set_handler_fn(syscall_handler)
            .set_privilege_level(x86_64::PrivilegeLevel::Ring3);
        
        idt
    };
}

/// Initialize IDT with interrupt handlers
pub fn init() {
    IDT.load();

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
    crate::serial_println!("SYSCALL_HANDLER called!");
    
    // System call handling via INT 0x80
    // Arguments in: RAX (syscall#), RDI (arg1), RSI (arg2), RDX (arg3)
    // Return value in: RAX
    
    // Get syscall arguments from stack
    // Stack: ... RAX, RBX, RCX, RDX, RSI, RDI, RBP, ... error_code, RIP, CS, RFLAGS, RSP, SS
    let stack_ptr = &stack_frame as *const InterruptStackFrame as *const u64;
    let syscall_num = unsafe { *stack_ptr.add(6) }; // RAX
    let arg1 = unsafe { *stack_ptr.add(11) }; // RDI
    let arg2 = unsafe { *stack_ptr.add(10) }; // RSI
    let arg3 = unsafe { *stack_ptr.add(9) }; // RDX
    
    crate::serial_println!("SYSCALL: num={} arg1={:#x} arg2={:#x} arg3={:#x}", syscall_num, arg1, arg2, arg3);
    
    // Handle the system call
    let result = crate::syscall::handle_syscall(syscall_num, arg1, arg2, arg3);
    
    // Write result back to RAX on stack
    unsafe {
        * (stack_ptr.add(5) as *mut u64) = result;
    }
    
    // No EOI needed for software interrupts (INT 0x80)
}
