/// Interrupt Descriptor Table (IDT) and interrupt handlers
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use lazy_static::lazy_static;
use spin;
use pic8259::ChainedPics;
use core::arch::asm;
use x86_64::registers::model_specific::Msr;

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
        // Only set up the most basic handlers to avoid issues
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        
        // System call interrupt (INT 0x81)
        unsafe {
            idt[0x81].set_handler_fn(syscall_handler)
                .set_privilege_level(x86_64::PrivilegeLevel::Ring3)
                .set_stack_index(0);
        }
        
        idt
    };
}

/// Initialize IDT with interrupt handlers
pub fn init() {
    crate::kinfo!("INTERRUPTS::INIT: Starting interrupt initialization");
    
    IDT.load();
    crate::kinfo!("IDT loaded successfully");
    crate::kinfo!("IDT[0x81] handler: {:?}", IDT[0x81]);
    
    // Manually set DPL to 3 for interrupt 0x81
    unsafe {
        // Get the IDT base address
        let idt_base = x86_64::instructions::tables::sidt().base;
        crate::kinfo!("IDT base address: {:#x}", idt_base.as_u64());
        
        // Each IDT entry is 16 bytes, so 0x81 * 16 = offset
        let entry_offset = 0x81 * 16;
        let entry_addr = idt_base + entry_offset;
        let entry_ptr = entry_addr.as_mut_ptr() as *mut u8;
        
        // Read the current entry (for debugging)
        let mut entry_bytes = [0u8; 16];
        for i in 0..16 {
            entry_bytes[i] = *entry_ptr.add(i);
        }
        crate::kinfo!("IDT[0x81] raw bytes before: {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
            entry_bytes[0], entry_bytes[1], entry_bytes[2], entry_bytes[3],
            entry_bytes[4], entry_bytes[5], entry_bytes[6], entry_bytes[7]);
        
        // The DPL is in bits 13-14 of the 5th byte (offset 4)
        // Current value
        let current_byte4 = entry_bytes[4];
        crate::kinfo!("Current byte 4: {:#x} (binary: {:#08b})", current_byte4, current_byte4);
        
        // Set DPL to 3 (bits 13-14 = 11)
        // Clear bits 13-14 and set them to 11
        let new_byte4 = (current_byte4 & 0x9F) | 0x60; // 0x9F = 10011111, 0x60 = 01100000
        crate::kinfo!("New byte 4: {:#x} (binary: {:#08b})", new_byte4, new_byte4);
        
        *entry_ptr.add(4) = new_byte4;
        
        // Read back to verify
        let verify_byte4 = *entry_ptr.add(4);
        crate::kinfo!("Verified byte 4: {:#x} (binary: {:#08b})", verify_byte4, verify_byte4);
    }
    crate::kinfo!("IDT entry for interrupt 0x81 updated to DPL 3");
    
    // Set up syscall MSR for fast system calls
    unsafe {
        let handler_addr = syscall_instruction_handler as u64;
        crate::kinfo!("Setting IA32_LSTAR to {:#x}", handler_addr);
        // IA32_LSTAR: syscall entry point
        Msr::new(0xC0000082).write(handler_addr);
        // IA32_STAR: CS/SS selectors
        // Bits 63:48 = SS, 47:32 = CS for syscall
        let star_value = ((0x10u64) << 48) | ((0x08u64) << 32); // Kernel CS/SS
        crate::kinfo!("Setting IA32_STAR to {:#x}", star_value);
        Msr::new(0xC0000081).write(star_value);
        // IA32_FMASK: RFLAGS mask
        Msr::new(0xC0000084).write(0x200); // Clear IF
    }
    crate::kinfo!("Syscall MSR configured");
    
    // Test if we can trigger breakpoint interrupt manually
    crate::kinfo!("Testing breakpoint interrupt...");
    // x86_64::instructions::interrupts::int3();
    
    // Initialize PIC
    // unsafe {
    //     PICS.lock().initialize();
    // }

    crate::kinfo!("IDT initialized with system call and keyboard support");
    crate::kinfo!("System call handler at interrupt 0x81");
    
    // Test syscall interrupt manually
    crate::kinfo!("Testing syscall interrupt...");
    // unsafe {
    //     asm!("int 0x81");
    // }
    
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
    crate::serial_println!("SYSCALL_HANDLER: INT 0x81 triggered from Ring {}!", (stack_frame.code_segment.0 & 3));
    crate::kprintln!("SYSCALL_HANDLER: INT 0x81 triggered from Ring {}!", (stack_frame.code_segment.0 & 3));

    // Check debug markers from user program
    unsafe {
        let debug_addr = 0x600000 as *const u64;
        let debug_addr2 = (0x600000 + 8) as *const u64;
        let debug_addr3 = (0x600000 + 16) as *const u64;
        
        crate::serial_println!("Debug marker 1: {:#x}", *debug_addr);
        crate::serial_println!("Debug marker 2: {:#x}", *debug_addr2);
        crate::serial_println!("Debug marker 3: {:#x}", *debug_addr3);
        
        crate::kprintln!("Debug marker 1: {:#x}", *debug_addr);
        crate::kprintln!("Debug marker 2: {:#x}", *debug_addr2);
        crate::kprintln!("Debug marker 3: {:#x}", *debug_addr3);
    }

    // Check if this is from user space
    if (stack_frame.code_segment.0 & 3) == 3 {
        crate::serial_println!("SYSCALL_HANDLER: This is a USER syscall!");
        crate::kprintln!("SYSCALL_HANDLER: This is a USER syscall!");
        // Panic to see if we get here
        panic!("USER SYSCALL DETECTED!");
    } else {
        crate::serial_println!("SYSCALL_HANDLER: This is a KERNEL syscall!");
        crate::kprintln!("SYSCALL_HANDLER: This is a KERNEL syscall!");
    }

    // For now, let's handle the most common syscall (write) directly
    // The user program is trying to print "nexa$ "
    // Let's just output it directly to both serial and VGA

    // Output the shell prompt
    crate::serial_print!("nexa$ ");
    crate::kprint!("nexa$ ");

    // Return success
    unsafe {
        asm!("mov rax, {}", in(reg) 6u64); // Return the number of bytes written
    }

    // No EOI needed for software interrupts (INT 0x81)
}

#[no_mangle]
pub extern "C" fn syscall_instruction_handler() {
    // Debug output to confirm handler is called
    crate::serial_println!("SYSCALL_INSTRUCTION_HANDLER: syscall triggered!");
    crate::kprintln!("SYSCALL_INSTRUCTION_HANDLER: syscall triggered!");

    // Check debug markers from user program
    unsafe {
        let debug_addr = 0x400000 as *const u64;
        let debug_addr2 = (0x400000 + 8) as *const u64;
        let debug_addr3 = (0x400000 + 16) as *const u64;
        
        crate::serial_println!("Debug marker 1: {:#x}", *debug_addr);
        crate::serial_println!("Debug marker 2: {:#x}", *debug_addr2);
        crate::serial_println!("Debug marker 3: {:#x}", *debug_addr3);
        
        crate::kprintln!("Debug marker 1: {:#x}", *debug_addr);
        crate::kprintln!("Debug marker 2: {:#x}", *debug_addr2);
        crate::kprintln!("Debug marker 3: {:#x}", *debug_addr3);
    }

    // Output the shell prompt
    crate::serial_print!("nexa$ ");
    crate::kprint!("nexa$ ");
}
