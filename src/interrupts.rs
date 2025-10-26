#![feature(naked_functions)]

/// Interrupt Descriptor Table (IDT) and interrupt handlers
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use lazy_static::lazy_static;
use spin;
use pic8259::ChainedPics;
use core::arch::asm;
use core::arch::naked_asm;
use core::arch::global_asm;
use x86_64::registers::model_specific::Msr;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> = spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

global_asm!(
    ".global syscall_interrupt_handler",
    "syscall_interrupt_handler:",
    // Save all registers
    "push rax",
    "push rbx",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    
    // Call syscall_dispatch(nr=rax, arg1=rdi, arg2=rsi, arg3=rdx)
    "mov rcx, rdx", // arg3
    "mov rdx, rsi", // arg2
    "mov rsi, rdi", // arg1
    "mov rdi, rax", // nr
    "call syscall_dispatch",
    // Return value is in rax
    
    // Restore registers
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "pop rax",
    
    "iretq"
);

extern "C" {
    fn syscall_interrupt_handler();
    fn syscall_handler();
}

/// Exception handlers
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    let ring = stack_frame.code_segment.0 & 3;
    if ring == 3 {
        crate::kinfo!("BREAKPOINT from user mode (Ring 3) at {:#x}", stack_frame.instruction_pointer);
        // Just return for user mode breakpoints
    } else {
        crate::kerror!("EXCEPTION: BREAKPOINT from Ring {}!", ring);
        crate::kdebug!("RIP: {:#x}, CS: {:#x}", stack_frame.instruction_pointer, stack_frame.code_segment.0);
        loop {
            x86_64::instructions::hlt();
        }
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
global_asm!(
    ".global ring3_switch_handler",
    "ring3_switch_handler:",
    "add rsp, 40",
    "pop r14",
    "pop r15",
    "pop r13",
    "pop r15",
    "pop r12",
    "mov ax, 0x23",
    "mov ds, ax",
    "mov es, ax",
    "mov fs, ax",
    "mov gs, ax",
    "mov rcx, r14",
    "mov r11, 0x202",
    "mov rsp, r15",
    "sysretq"
);

extern "C" {
    fn ring3_switch_handler();
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        // crate::kinfo!("Initializing IDT...");
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
        
        // Set up hardware interrupts
        idt[PIC_1_OFFSET].set_handler_fn(timer_interrupt_handler);
        idt[PIC_1_OFFSET + 1].set_handler_fn(keyboard_interrupt_handler);
        
        // Set up syscall interrupt handler at 0x81
        unsafe {
            idt[0x81].set_handler_addr(x86_64::VirtAddr::new(syscall_interrupt_handler as u64));
        }
        
        idt
    };
}

/// Initialize IDT with interrupt handlers
pub fn init_interrupts() {
    // Load the IDT
    IDT.load();
    
    // Initialize PICs
    unsafe { PICS.lock().initialize(); }
    
    // Setup syscall
    setup_syscall();
    
    crate::kinfo!("IDT loaded and syscall configured");
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
        "swapgs",                    // Switch to kernel GS
        "mov gs:[0], rsp",           // Save user RSP
        "mov rsp, gs:[8]",           // Load kernel RSP
        
        // Push user state for sysret
        "push 0x23",                 // User SS
        "push gs:[0]",               // User RSP
        "push r11",                  // User RFLAGS
        "push 0x1b",                 // User CS
        "push rcx",                  // User RIP
        
        // Save syscall args
        "push rax",
        "push rdi",
        "push rsi", 
        "push rdx",
        
        // Call handler
        "call syscall_dispatch",
        
        // Restore syscall args
        "pop rdx",
        "pop rsi",
        "pop rdi", 
        "pop rax",
        
        // Restore user state from stack
        "pop rcx",                   // User RIP
        "add rsp, 8",                // Skip user CS
        "pop r11",                   // User RFLAGS
        "pop rsp",                   // User RSP
        "add rsp, 8",                // Skip user SS
        
        "swapgs",                    // Switch back to user GS
        "sysret",                    // Return to user mode
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
    
    crate::kdebug!("SYSCALL_INSTRUCTION_HANDLER: syscall={} arg1={:#x} arg2={:#x} arg3={:#x}", syscall_num, arg1, arg2, arg3);
    
    if syscall_num == 1 { // write
        let fd = arg1;
        let buf_ptr = arg2 as *const u8;
        let count = arg3 as usize;
        
        crate::kdebug!("SYSCALL: write fd={} buf={:#x} count={}", fd, buf_ptr as u64, count);
        
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
    
    crate::kinfo!("RING3_SWITCH: entry={:#x}, stack={:#x}, cs={:#x}, ss={:#x}, ds={:#x}", 
        entry, stack, cs, ss, ds);
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
    
    crate::kdebug!("GS check: gs:[0]={:#x}, gs:[8]={:#x}, gs:[40]={:#x}", gs0, gs8, gs40);
}

/// Set GS data for Ring 3 switch
pub unsafe fn set_gs_data(entry: u64, stack: u64, user_cs: u64, user_ss: u64, user_ds: u64) {
    unsafe {
        // Get kernel stack from TSS privilege stack table
        let kernel_stack = crate::gdt::get_kernel_stack_top();
        
        crate::interrupts::GS_DATA[0] = stack; // user RSP at gs:[0]
        crate::interrupts::GS_DATA[1] = kernel_stack; // kernel RSP at gs:[8]
        crate::interrupts::GS_DATA[2] = entry; // USER_ENTRY at gs:[16]
        crate::interrupts::GS_DATA[3] = stack; // USER_STACK at gs:[24]
        crate::interrupts::GS_DATA[4] = user_cs; // user_cs at gs:[32]
        crate::interrupts::GS_DATA[5] = user_ss; // user_ss at gs:[40]
        crate::interrupts::GS_DATA[6] = user_ds; // user_ds at gs:[48]
        
        crate::kdebug!("GS_DATA set: entry={:#x}, stack={:#x}, cs={:#x}, ss={:#x}, ds={:#x}", entry, stack, user_cs, user_ss, user_ds);
        crate::kdebug!("GS_DATA[1] (kernel stack) = {:#x}", crate::interrupts::GS_DATA[1]);
    }
}

// GS data for syscall and Ring 3 switch
pub static mut GS_DATA: [u64; 16] = [0; 16];

pub fn setup_syscall() {
    // Setup syscall
    crate::kinfo!("Setting syscall handler to {:#x}", syscall_instruction_handler as u64);
    unsafe {
        Msr::new(0xc0000101).write(0); // GS base
        Msr::new(0xc0000080).write(1 << 0); // IA32_EFER.SCE = 1
        Msr::new(0xc0000081).write((0x08 << 32) | (0x1b << 48)); // STAR
        Msr::new(0xc0000082).write(syscall_instruction_handler as u64); // LSTAR
        Msr::new(0xc0000084).write(0x200); // FMASK
    }
}

extern "C" fn debug_params(stack: u64, entry: u64) {
    crate::kinfo!("RING3_SWITCH: entry={:#x}, stack={:#x}", entry, stack);
}
