#![feature(naked_functions)]

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
    crate::kerror!("EXCEPTION: BREAKPOINT from Ring {}!", (stack_frame.code_segment.0 & 3));
    crate::kdebug!("RIP: {:#x}, CS: {:#x}", stack_frame.instruction_pointer, stack_frame.code_segment.0);
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    crate::kerror!("EXCEPTION: PAGE FAULT (error: {:?}) from Ring {}!", error_code, (stack_frame.code_segment.0 & 3));
    crate::kdebug!("RIP: {:#x}, CS: {:#x}", stack_frame.instruction_pointer, stack_frame.code_segment.0);
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::kerror!("EXCEPTION: GENERAL PROTECTION FAULT (error: {}) from Ring {}!", error_code, (stack_frame.code_segment.0 & 3));
    crate::kdebug!("RIP: {:#x}, CS: {:#x}", stack_frame.instruction_pointer, stack_frame.code_segment.0);
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    crate::kerror!("EXCEPTION: DIVIDE ERROR\n{:#?}", stack_frame);
    loop {}
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    crate::kerror!("EXCEPTION: DOUBLE FAULT (error: {})\n{:#?}", error_code, stack_frame);
    loop {}
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::kerror!("EXCEPTION: SEGMENT NOT PRESENT (error: {})\n{:#?}", error_code, stack_frame);
    loop {}
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    crate::kerror!("EXCEPTION: INVALID OPCODE\n{:#?}", stack_frame);
    loop {}
}

// Ring 3 switch handler - interrupt 0x80
extern "x86-interrupt" fn ring3_switch_handler(_stack_frame: InterruptStackFrame) {
    crate::kdebug!("RING3_SWITCH: Switching to user mode by building new iretq frame");
    
    // Get stored user entry and stack
    let entry = unsafe { crate::process::get_user_entry() };
    let stack = unsafe { crate::process::get_user_stack() };
    
    // Get user selectors
    let selectors = unsafe { crate::gdt::get_selectors() };
    let user_cs = selectors.user_code_selector.0 as u64;
    let user_ss = selectors.user_data_selector.0 as u64;
    let user_ds = selectors.user_data_selector.0 as u64;
    
    crate::kdebug!("RING3_SWITCH: Using CS={:#x}, SS={:#x}, DS={:#x}, RIP={:#x}, RSP={:#x}", 
        user_cs, user_ss, user_ds, entry, stack);
    
    // Read GDTR to find real GDT base
    crate::kdebug!("RING3_SWITCH: Reading GDTR to find real GDT base");
    let mut gdtr: [u8; 10] = [0; 10];
    unsafe {
        core::arch::asm!("sgdt [{}]", in(reg) gdtr.as_mut_ptr());
    }
    let limit = u16::from_le_bytes([gdtr[0], gdtr[1]]);
    let mut base_bytes = [0u8; 8];
    base_bytes.copy_from_slice(&gdtr[2..10]);
    let gdt_base = u64::from_le_bytes(base_bytes);
    crate::kdebug!("GDTR: limit={:#x}, base={:#x}", limit, gdt_base);
    
    // Dump first 8 descriptors from real GDT base
    unsafe {
        for i in 0..8 {
            let low = *(gdt_base as *const u32).add(i * 2);
            let high = *(gdt_base as *const u32).add(i * 2 + 1);
            let desc = ((high as u64) << 32) | (low as u64);
            crate::kdebug!("GDT[{}]: {:#018x}", i, desc);
        }
    }
    
    // Build iretq frame on current stack
    // iretq expects: SS, RSP, RFLAGS, CS, RIP (in that order, from high to low addresses)
    // All values must be 64-bit
    crate::kdebug!("RING3_SWITCH: Building iretq frame with 64-bit values");
    
    // Check current RFLAGS
    let current_rflags: u64;
    unsafe {
        asm!("pushfq; pop {}", out(reg) current_rflags);
    }
    crate::kdebug!("RING3_SWITCH: Current RFLAGS = {:#x}", current_rflags);
    
    unsafe {
        asm!(
            // Save current stack pointer
            "mov r15, rsp",
            
            // Build iretq frame (push in reverse order: RIP, CS, RFLAGS, RSP, SS)
            "push {user_ss}",     // SS (64-bit)
            "push {user_stack}",  // RSP (64-bit) 
            "push {rflags}",      // RFLAGS (64-bit)
            "push {user_cs}",     // CS (64-bit)
            "push {user_entry}",  // RIP (64-bit)
            
            // Set DS to user data selector
            "mov ds, {user_ds:x}",
            "mov es, {user_ds:x}",
            "mov fs, {user_ds:x}",
            "mov gs, {user_ds:x}",
            
            // Execute iretq to switch to Ring 3
            "iretq",
            
            user_ss = in(reg) user_ss,
            user_stack = in(reg) stack,
            rflags = in(reg) current_rflags,
            user_cs = in(reg) user_cs,
            user_entry = in(reg) entry,
            user_ds = in(reg) user_ds,
        );
    }
    
    // This should never be reached if iretq succeeds
    crate::kerror!("ERROR: iretq failed to execute!");
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        
        // Set up interrupt handlers
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.invalid_tss.set_handler_fn(segment_not_present_handler); // Reuse handler
        idt.stack_segment_fault.set_handler_fn(segment_not_present_handler); // Reuse handler
        
        // Set up system call interrupt at 0x81
        idt[0x81].set_handler_fn(syscall_handler).set_privilege_level(x86_64::PrivilegeLevel::Ring3);
        
        // Set up Ring 3 switch interrupt at 0x80
        idt[0x80].set_handler_fn(ring3_switch_handler);
        
        // Set up hardware interrupts
        idt[PIC_1_OFFSET].set_handler_fn(timer_interrupt_handler);
        idt[PIC_1_OFFSET + 1].set_handler_fn(keyboard_interrupt_handler);
        
        idt
    };
}

/// Initialize IDT with interrupt handlers
pub fn init() {
    crate::kinfo!("INTERRUPTS::INIT: Starting interrupt initialization");
    
    IDT.load();
    crate::kinfo!("IDT loaded successfully");
    
    // Set up syscall MSR for fast system calls
    unsafe {
        let handler_addr = syscall_instruction_handler as u64;
        crate::kinfo!("Setting IA32_LSTAR to {:#x}", handler_addr);
        // IA32_LSTAR: syscall entry point
        Msr::new(0xC0000082).write(handler_addr);
        // IA32_STAR: CS/SS selectors for syscall/sysret
        // syscall: CS = STAR[47:32] & 0xFFFC, SS = STAR[47:32] + 8
        // sysret: CS = STAR[63:48] + 16, SS = STAR[63:48] + 24  
        // For our GDT: kernel CS=0x8, SS=0x10, user CS=0x18|3=0x1b, SS=0x20|3=0x23
        let star_value = (0x08u64 << 48) | (0x1bu64 << 32) | (0x23u64 << 16) | 0x10u64;
        crate::kinfo!("Setting IA32_STAR to {:#x}", star_value);
        Msr::new(0xC0000081).write(star_value);
        // IA32_FMASK: RFLAGS mask
        Msr::new(0xC0000084).write(0x200); // Clear IF
        
        // Enable syscall instruction
        let efer = Msr::new(0xC0000080).read();
        crate::kinfo!("Current IA32_EFER: {:#x}", efer);
        Msr::new(0xC0000080).write(efer | 1); // Set SCE bit
        crate::kinfo!("Enabled syscall instruction (SCE=1)");
    }
    crate::kinfo!("Syscall MSR configured");
    
    // Test if we can trigger breakpoint interrupt manually
    // crate::kinfo!("Testing breakpoint interrupt...");
    // x86_64::instructions::interrupts::int3();
    
    // Initialize PIC
    // unsafe {
    //     PICS.lock().initialize();
    // }

    crate::kinfo!("IDT initialized with system call and keyboard support");
    crate::kinfo!("System call handler at interrupt 0x81");
    
    // Test syscall interrupt manually
    crate::kinfo!("Testing syscall interrupt...");
    unsafe {
        asm!("int 0x81");
    }
    
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
    crate::kdebug!("SYSCALL_HANDLER: INT 0x81 triggered from Ring {}!", (stack_frame.code_segment.0 & 3));

    // Check if this is from user space
    if (stack_frame.code_segment.0 & 3) == 3 {
        crate::kdebug!("SYSCALL_HANDLER: This is a USER syscall!");
        
        // Check debug markers from user program
        unsafe {
            let debug_addr = 0x600000 as *const u64;
            let debug_addr2 = (0x600000 + 8) as *const u64;
            let debug_addr3 = (0x600000 + 16) as *const u64;
            
            crate::kdebug!("Debug marker 1: {:#x}", *debug_addr);
            crate::kdebug!("Debug marker 2: {:#x}", *debug_addr2);
            crate::kdebug!("Debug marker 3: {:#x}", *debug_addr3);
        }
        
        // Output the shell prompt
        crate::serial_print!("nexa$ ");
        // Return success
        unsafe {
            asm!("mov rax, {}", in(reg) 6u64); // Return the number of bytes written
        }
        return;
    } else {
        crate::kdebug!("SYSCALL_HANDLER: This is a KERNEL syscall!");
    }

    // For now, let's handle the most common syscall (write) directly
    // The user program is trying to print "nexa$ "
    // Let's just output it directly to both serial and VGA

    // Output the shell prompt
    crate::serial_print!("nexa$ ");

    // Return success
    unsafe {
        asm!("mov rax, {}", in(reg) 6u64); // Return the number of bytes written
    }

    // No EOI needed for software interrupts (INT 0x81)
}

extern "C" fn syscall_instruction_handler() {
    unsafe {
        // syscall saves user RIP in RCX, RFLAGS in R11
        let user_rip: u64;
        let user_rflags: u64;
        let syscall_num: u64;
        let arg1: u64;
        let arg2: u64;
        let arg3: u64;
        
        asm!("", 
            out("rcx") user_rip, 
            out("r11") user_rflags,
            out("rax") syscall_num,
            out("rdi") arg1,
            out("rsi") arg2,
            out("rdx") arg3
        );
        
        // Handle the syscall
        match syscall_num {
            1 => { // SYS_WRITE
                let fd = arg1;
                let buf = arg2 as *const u8;
                let count = arg3 as usize;
                
                if fd == 1 { // stdout
                    // Write to serial port from kernel space using proper logging macro
                    let s = core::str::from_utf8_unchecked(core::slice::from_raw_parts(buf, count));
                    crate::serial_print!("{}", s);
                }
            },
            _ => {
                // Unknown syscall - do nothing
            }
        }
        
        // For now, don't return to user space - just continue in kernel
        // This will cause the user process to be terminated, but let's see if syscall works
    }
}
