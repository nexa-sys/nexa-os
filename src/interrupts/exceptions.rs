//! Exception Handlers
//!
//! This module contains CPU exception handlers for faults like page faults,
//! general protection faults, divide errors, etc.

use x86_64::instructions::port::Port;
use x86_64::structures::idt::{InterruptStackFrame, PageFaultErrorCode};

use crate::interrupts::gs_context::{encode_hex_u64, write_hex_u64};
use crate::{kdebug, kerror, kinfo, kpanic, kwarn};

/// Breakpoint exception handler (#BP, vector 3)
pub extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    let ring = stack_frame.code_segment.0 & 3;
    // Low-level marker for breakpoint
    unsafe {
        let mut port = Port::new(0x3F8u16);
        port.write(b'B');
    }
    if ring == 3 {
        kinfo!(
            "BREAKPOINT from user mode (Ring 3) at {:#x}",
            stack_frame.instruction_pointer
        );
        // Just return for user mode breakpoints
    } else {
        kerror!("EXCEPTION: BREAKPOINT from Ring {}!", ring);
        kdebug!(
            "RIP: {:#x}, CS: {:#x}",
            stack_frame.instruction_pointer,
            stack_frame.code_segment.0
        );
        loop {
            x86_64::instructions::hlt();
        }
    }
}

/// Page fault exception handler (#PF, vector 14)
pub extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    
    let cr2 = Cr2::read().unwrap_or_else(|_| x86_64::VirtAddr::new(0));
    let fault_addr = cr2.as_u64();
    let rip = stack_frame.instruction_pointer.as_u64();
    
    // Check if this is a user-mode page fault using the error code's USER_MODE bit
    // This is more reliable than checking CS because:
    // 1. The error code USER_MODE bit directly indicates if the access was from user mode
    // 2. CS in the stack frame might be kernel CS if we're in an IST handler
    let is_user_mode = error_code.contains(PageFaultErrorCode::USER_MODE);

    if is_user_mode {
        // User-mode page fault - terminate the process gracefully
        if let Some(pid) = crate::scheduler::current_pid() {
            // Print coredump info using serial (avoids log system deadlock)
            crate::serial_println!("=== COREDUMP (PID {}) ===", pid);
            crate::serial_println!("  Signal: SIGSEGV (Segmentation fault)");
            crate::serial_println!("  Fault addr: {:#x}", fault_addr);
            crate::serial_println!("  RIP: {:#x}", rip);
            crate::serial_println!("  CS: {:#x}", stack_frame.code_segment.0);
            crate::serial_println!("  Error code: {:?}", error_code);
            crate::serial_println!("=== END COREDUMP ===");
            
            // Set exit code to 128 + SIGSEGV (11) = 139 (standard Unix convention)
            let exit_code = 128 + crate::ipc::signal::SIGSEGV as i32;
            let _ = crate::scheduler::set_process_exit_code(pid, exit_code);
            
            // Mark the process as zombie
            let _ = crate::scheduler::set_process_state(pid, crate::process::ProcessState::Zombie);
            
            // Schedule the next process
            crate::scheduler::do_schedule_from_interrupt();
            
            // If we return here, something went wrong
            kerror!("FATAL: do_schedule_from_interrupt returned after SIGSEGV termination");
        } else {
            kerror!("User-mode page fault but no current process - this should not happen!");
        }
        
        // Fallback: halt if we can't recover
        loop {
            x86_64::instructions::hlt();
        }
    }

    // Kernel-mode page fault - this is a serious error
    kerror!("EXCEPTION: KERNEL PAGE FAULT at {:#x}, RIP={:#x}", fault_addr, rip);
    kerror!("Error code: {:?}", error_code);
    kerror!("System halted due to unrecoverable kernel page fault");
    loop {
        x86_64::instructions::hlt();
    }
}

/// General protection fault exception handler (#GP, vector 13)
pub extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    use x86_64::instructions::interrupts;
    use crate::interrupts::gs_context::{GS_SLOT_USER_RSP, GS_SLOT_USER_CS};
    
    let ring = stack_frame.code_segment.0 & 3;
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    
    // Read saved user context from GS_DATA
    // Slots 10-14 are set by syscall return path (iretq preparation):
    //   slot 10 (gs:[80]) = user RIP
    //   slot 11 (gs:[88]) = user CS  
    //   slot 12 (gs:[96]) = user RFLAGS
    //   slot 13 (gs:[104]) = user RSP
    //   slot 14 (gs:[112]) = user SS
    let (gs_user_rsp_slot0, gs_user_cs_slot4, iret_user_rip, iret_user_cs, iret_user_rsp) = unsafe {
        let gs_ptr = core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *const u64;
        (
            gs_ptr.add(GS_SLOT_USER_RSP).read_volatile(),    // slot 0: USER_RSP (syscall entry)
            gs_ptr.add(GS_SLOT_USER_CS).read_volatile(),     // slot 4: USER_CS (syscall entry)
            gs_ptr.add(10).read_volatile(),                   // slot 10: iret user RIP
            gs_ptr.add(11).read_volatile(),                   // slot 11: iret user CS
            gs_ptr.add(13).read_volatile(),                   // slot 13: iret user RSP
        )
    };
    
    // Check if this GPF is related to a user process:
    // 1. Direct user-mode GPF: ring == 3
    // 2. GPF during syscall/iret for a user process: iret_user_cs or gs_user_cs indicates user mode
    let iret_user_ring = (iret_user_cs & 3) as u8;
    let gs_user_ring = (gs_user_cs_slot4 & 3) as u8;
    let is_user_related = ring == 3 || iret_user_ring == 3 || gs_user_ring == 3;
    
    // Also check if we have a current user process running
    let current_pid = crate::scheduler::current_pid();
    let has_user_process = current_pid.is_some() && current_pid != Some(0);
    
    if is_user_related && has_user_process {
        let pid = current_pid.unwrap();
        
        // User-mode related GPF - terminate the process, not the kernel
        kerror!(
            "GPF: User process {} crashed, error_code={:#x}",
            pid, error_code
        );
        
        // Log coredump-style information
        kerror!("=== COREDUMP INFO FOR PID {} (GPF) ===", pid);
        kerror!("  Exception RIP: {:#x}", rip);
        kerror!("  Exception RSP: {:#x}", rsp);
        kerror!("  Exception CS: {:#x} (Ring {})", stack_frame.code_segment.0, ring);
        kerror!("  IRET target RIP: {:#x}", iret_user_rip);
        kerror!("  IRET target CS: {:#x} (Ring {})", iret_user_cs, iret_user_ring);
        kerror!("  IRET target RSP: {:#x}", iret_user_rsp);
        kerror!("  Syscall entry RSP: {:#x}", gs_user_rsp_slot0);
        kerror!("  RFLAGS at exception: {:#x}", stack_frame.cpu_flags.bits());
        kerror!("  Error Code: {:#x}", error_code);
        kerror!("=== END COREDUMP INFO ===");
        
        // Set exit code to 128 + SIGSEGV (11) = 139 (standard Unix convention)
        let exit_code = 128 + crate::ipc::signal::SIGSEGV as i32;
        if let Err(e) = crate::scheduler::set_process_exit_code(pid, exit_code) {
            kerror!("Failed to set exit code for PID {}: {}", pid, e);
        }
        
        // Mark the process as zombie
        let _ = crate::scheduler::set_process_state(pid, crate::process::ProcessState::Zombie);
        
        kinfo!("Process {} terminated with signal SIGSEGV/GPF (exit code {})", pid, exit_code);
        
        // Schedule the next process
        crate::scheduler::do_schedule_from_interrupt();
        
        // If we return here, something went wrong
        kerror!("FATAL: do_schedule_from_interrupt returned after GPF termination");
        
        // Fallback: halt if we can't recover
        loop {
            x86_64::instructions::hlt();
        }
    }

    // Kernel-mode GPF with no user process involved - dump detailed debugging info
    let handler_rsp: u64;
    let (
        reg_rax,
        reg_rbx,
        reg_rcx,
        reg_rdx,
        reg_rsi,
        reg_rdi,
        reg_rbp,
        reg_r8,
        reg_r9,
        reg_r10,
        reg_r11,
        reg_r12,
        reg_r13,
        reg_r14,
        reg_r15,
    ): (
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
    );
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) handler_rsp);
        core::arch::asm!(
            "mov {}, rax\n\
             mov {}, rbx\n\
             mov {}, rcx\n\
             mov {}, rdx\n\
             mov {}, rsi\n\
             mov {}, rdi\n\
             mov {}, rbp\n\
             mov {}, r8\n\
             mov {}, r9\n\
             mov {}, r10\n\
             mov {}, r11\n\
             mov {}, r12\n\
             mov {}, r13\n\
             mov {}, r14\n\
             mov {}, r15",
            out(reg) reg_rax,
            out(reg) reg_rbx,
            out(reg) reg_rcx,
            out(reg) reg_rdx,
            out(reg) reg_rsi,
            out(reg) reg_rdi,
            out(reg) reg_rbp,
            out(reg) reg_r8,
            out(reg) reg_r9,
            out(reg) reg_r10,
            out(reg) reg_r11,
            out(reg) reg_r12,
            out(reg) reg_r13,
            out(reg) reg_r14,
            out(reg) reg_r15,
            options(nomem, nostack, preserves_flags)
        );
    }

    let (
        gs_user_rsp,
        gs_user_rsp_dbg,
        gs_user_cs,
        gs_user_ss,
        frame_rip,
        frame_cs,
        frame_rflags,
        frame_rsp,
        frame_ss,
        entry_cs,
        entry_ss,
        sysret_rip,
        sysret_rflags,
        sysret_rsp,
    ) = unsafe {
        let gs_ptr = core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *const u64;
        (
            gs_ptr.add(0).read_volatile(),
            gs_ptr.add(9).read_volatile(),
            gs_ptr.add(4).read_volatile(),
            gs_ptr.add(5).read_volatile(),
            gs_ptr.add(10).read_volatile(),
            gs_ptr.add(11).read_volatile(),
            gs_ptr.add(12).read_volatile(),
            gs_ptr.add(13).read_volatile(),
            gs_ptr.add(14).read_volatile(),
            gs_ptr.add(15).read_volatile(),
            gs_ptr.add(16).read_volatile(),
            gs_ptr.add(17).read_volatile(),
            gs_ptr.add(18).read_volatile(),
            gs_ptr.add(19).read_volatile(),
        )
    };

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
        port.write(b' ');
        write_hex_u64(&mut port, gs_user_rsp);
        port.write(b' ');
        write_hex_u64(&mut port, gs_user_cs);
        port.write(b' ');
        write_hex_u64(&mut port, gs_user_ss);
        port.write(b' ');
        write_hex_u64(&mut port, gs_user_rsp_dbg);
        port.write(b'\n');
        port.write(b' ');
        write_hex_u64(&mut port, frame_rip);
        port.write(b' ');
        write_hex_u64(&mut port, frame_cs);
        port.write(b' ');
        write_hex_u64(&mut port, frame_rflags);
        port.write(b' ');
        write_hex_u64(&mut port, frame_rsp);
        port.write(b' ');
        write_hex_u64(&mut port, frame_ss);
        port.write(b'\n');
        port.write(b' ');
        write_hex_u64(&mut port, entry_cs);
        port.write(b' ');
        write_hex_u64(&mut port, entry_ss);
        port.write(b'\n');
        port.write(b' ');
        write_hex_u64(&mut port, sysret_rip);
        port.write(b' ');
        write_hex_u64(&mut port, sysret_rflags);
        port.write(b' ');
        write_hex_u64(&mut port, sysret_rsp);
        port.write(b'\n');
        port.write(b' ');
        write_hex_u64(&mut port, stack_frame.stack_pointer.as_u64());
        port.write(b' ');
        write_hex_u64(&mut port, stack_frame.stack_segment.0 as u64);
        port.write(b' ');
        write_hex_u64(&mut port, handler_rsp);
        port.write(b' ');
        write_hex_u64(&mut port, stack_frame.code_segment.0 as u64);
        port.write(b'\n');
        // Dump general-purpose registers to correlate with faulting write
        port.write(b'R');
        port.write(b'A');
        port.write(b'X');
        port.write(b'=');
        write_hex_u64(&mut port, reg_rax);
        port.write(b' ');
        port.write(b'R');
        port.write(b'B');
        port.write(b'X');
        port.write(b'=');
        write_hex_u64(&mut port, reg_rbx);
        port.write(b' ');
        port.write(b'R');
        port.write(b'C');
        port.write(b'X');
        port.write(b'=');
        write_hex_u64(&mut port, reg_rcx);
        port.write(b' ');
        port.write(b'R');
        port.write(b'D');
        port.write(b'X');
        port.write(b'=');
        write_hex_u64(&mut port, reg_rdx);
        port.write(b'\n');

        port.write(b'R');
        port.write(b'S');
        port.write(b'I');
        port.write(b'=');
        write_hex_u64(&mut port, reg_rsi);
        port.write(b' ');
        port.write(b'R');
        port.write(b'D');
        port.write(b'I');
        port.write(b'=');
        write_hex_u64(&mut port, reg_rdi);
        port.write(b' ');
        port.write(b'R');
        port.write(b'B');
        port.write(b'P');
        port.write(b'=');
        write_hex_u64(&mut port, reg_rbp);
        port.write(b' ');
        port.write(b'R');
        port.write(b'8');
        port.write(b'=');
        write_hex_u64(&mut port, reg_r8);
        port.write(b'\n');

        port.write(b'R');
        port.write(b'9');
        port.write(b'=');
        write_hex_u64(&mut port, reg_r9);
        port.write(b' ');
        port.write(b'R');
        port.write(b'1');
        port.write(b'0');
        port.write(b'=');
        write_hex_u64(&mut port, reg_r10);
        port.write(b' ');
        port.write(b'R');
        port.write(b'1');
        port.write(b'1');
        port.write(b'=');
        write_hex_u64(&mut port, reg_r11);
        port.write(b' ');
        port.write(b'R');
        port.write(b'1');
        port.write(b'2');
        port.write(b'=');
        write_hex_u64(&mut port, reg_r12);
        port.write(b'\n');

        port.write(b'R');
        port.write(b'1');
        port.write(b'3');
        port.write(b'=');
        write_hex_u64(&mut port, reg_r13);
        port.write(b' ');
        port.write(b'R');
        port.write(b'1');
        port.write(b'4');
        port.write(b'=');
        write_hex_u64(&mut port, reg_r14);
        port.write(b' ');
        port.write(b'R');
        port.write(b'1');
        port.write(b'5');
        port.write(b'=');
        write_hex_u64(&mut port, reg_r15);
        port.write(b'\n');
        let handler_ptr = handler_rsp as *const u64;
        let mut i = 0usize;
        while i < 12 {
            let value = handler_ptr.add(i).read_volatile();
            port.write(b' ');
            write_hex_u64(&mut port, i as u64);
            port.write(b':');
            write_hex_u64(&mut port, value);
            port.write(b' ');
            i += 1;
        }
        port.write(b'\n');
    }

    interrupts::disable();
    loop {
        x86_64::instructions::hlt();
    }
}

/// Double fault exception handler (#DF, vector 8)
pub extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    kerror!(
        "DOUBLE FAULT: code={:#x} rip={:#x} rsp={:#x} ss={:#x}",
        error_code,
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        stack_frame.stack_segment.0
    );
    kpanic!(
        "DOUBLE FAULT: code={:#x} rip={:#x} rsp={:#x} ss={:#x}",
        error_code,
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        stack_frame.stack_segment.0
    );
}

/// Divide error exception handler (#DE, vector 0)
pub extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    let ring = stack_frame.code_segment.0 & 3;
    let rip = stack_frame.instruction_pointer.as_u64();
    
    // Low-level marker for divide error
    unsafe {
        let mut port = Port::new(0x3F8u16);
        port.write(b'D');
    }
    
    // Check if this is a user-mode divide error (Ring 3)
    if ring == 3 {
        kerror!("DIV/0: User process divide error at RIP={:#x}", rip);
        
        if let Some(pid) = crate::scheduler::current_pid() {
            kerror!("Process {} crashed with SIGFPE (divide by zero)", pid);
            kerror!("=== COREDUMP INFO FOR PID {} (DIV/0) ===", pid);
            kerror!("  Instruction Pointer: {:#x}", rip);
            kerror!("  Stack Pointer: {:#x}", stack_frame.stack_pointer.as_u64());
            kerror!("  RFLAGS: {:#x}", stack_frame.cpu_flags.bits());
            kerror!("=== END COREDUMP INFO ===");
            
            // Set exit code to 128 + SIGFPE (8) = 136
            let exit_code = 128 + crate::ipc::signal::SIGFPE as i32;
            let _ = crate::scheduler::set_process_exit_code(pid, exit_code);
            let _ = crate::scheduler::set_process_state(pid, crate::process::ProcessState::Zombie);
            
            kinfo!("Process {} terminated with signal SIGFPE (exit code {})", pid, exit_code);
            crate::scheduler::do_schedule_from_interrupt();
            
            kerror!("FATAL: do_schedule_from_interrupt returned after DIV/0 termination");
        }
        
        loop {
            x86_64::instructions::hlt();
        }
    }
    
    kpanic!("EXCEPTION: KERNEL DIVIDE ERROR\n{:#?}", stack_frame);
}

/// Segment not present exception handler (#NP, vector 11)
pub extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let ring = stack_frame.code_segment.0 & 3;
    let rip = stack_frame.instruction_pointer.as_u64();
    
    // Check if this is a user-mode exception (Ring 3)
    if ring == 3 {
        kerror!("SEGNP: User process segment not present at RIP={:#x}, error={:#x}", rip, error_code);
        
        if let Some(pid) = crate::scheduler::current_pid() {
            kerror!("Process {} crashed with SIGSEGV (segment not present)", pid);
            
            let exit_code = 128 + crate::ipc::signal::SIGSEGV as i32;
            let _ = crate::scheduler::set_process_exit_code(pid, exit_code);
            let _ = crate::scheduler::set_process_state(pid, crate::process::ProcessState::Zombie);
            
            kinfo!("Process {} terminated with signal SIGSEGV (exit code {})", pid, exit_code);
            crate::scheduler::do_schedule_from_interrupt();
            
            kerror!("FATAL: do_schedule_from_interrupt returned after SEGNP termination");
        }
        
        loop {
            x86_64::instructions::hlt();
        }
    }
    
    kpanic!(
        "EXCEPTION: KERNEL SEGMENT NOT PRESENT (error: {})\n{:#?}",
        error_code,
        stack_frame
    );
}

/// Invalid opcode exception handler (#UD, vector 6)
pub extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    let ring = stack_frame.code_segment.0 & 3;
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    
    // Low-level marker for invalid opcode
    unsafe {
        let mut port = Port::new(0x3F8u16);
        port.write(b'I');
    }
    
    // Check if this is a user-mode exception (Ring 3)
    if ring == 3 {
        kerror!("UD: User process invalid opcode at RIP={:#x}", rip);
        
        if let Some(pid) = crate::scheduler::current_pid() {
            kerror!("Process {} crashed with SIGILL (invalid opcode)", pid);
            kerror!("=== COREDUMP INFO FOR PID {} (UD) ===", pid);
            kerror!("  Instruction Pointer: {:#x}", rip);
            kerror!("  Stack Pointer: {:#x}", rsp);
            
            // Try to read bytes at RIP for debugging
            let mut bytes_at_rip: [u8; 16] = [0; 16];
            unsafe {
                let rip_ptr = rip as *const u8;
                for i in 0..16 {
                    bytes_at_rip[i] = rip_ptr.add(i).read_volatile();
                }
            }
            kerror!("  Bytes at RIP: {:02x?}", bytes_at_rip);
            kerror!("=== END COREDUMP INFO ===");
            
            // Set exit code to 128 + SIGILL (4) = 132
            let exit_code = 128 + crate::ipc::signal::SIGILL as i32;
            let _ = crate::scheduler::set_process_exit_code(pid, exit_code);
            let _ = crate::scheduler::set_process_state(pid, crate::process::ProcessState::Zombie);
            
            kinfo!("Process {} terminated with signal SIGILL (exit code {})", pid, exit_code);
            crate::scheduler::do_schedule_from_interrupt();
            
            kerror!("FATAL: do_schedule_from_interrupt returned after UD termination");
        }
        
        loop {
            x86_64::instructions::hlt();
        }
    }
    
    // Kernel-mode invalid opcode - this is a serious error
    let mut bytes_at_rip: [u8; 16] = [0; 16];
    let mut bytes_at_rsp: [u8; 16] = [0; 16];
    unsafe {
        let rip_ptr = rip as *const u8;
        let rsp_ptr = rsp as *const u8;
        for i in 0..16 {
            bytes_at_rip[i] = rip_ptr.add(i).read_volatile();
            bytes_at_rsp[i] = rsp_ptr.add(i).read_volatile();
        }
    }
    kpanic!(
        "EXCEPTION: KERNEL INVALID OPCODE rip={:#x} rsp={:#x} bytes rip={:02x?} stack={:02x?}\n{:#?}",
        rip,
        rsp,
        bytes_at_rip,
        bytes_at_rsp,
        stack_frame
    );
}
