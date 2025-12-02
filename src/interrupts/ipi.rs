//! IPI (Inter-Processor Interrupt) Handlers for SMP Support
//!
//! This module contains handlers for IPIs used in multi-core systems.
//! These include reschedule requests, TLB flush notifications, function
//! calls, and halt requests between CPU cores.
//!
//! ## Per-CPU IPI Tracking
//!
//! All IPI handlers update per-CPU statistics for monitoring:
//! - ipi_received: Count of IPIs received on each CPU
//! - interrupt context management for proper scheduling

use x86_64::structures::idt::InterruptStackFrame;

/// IPI Vector constants
pub const IPI_RESCHEDULE: u8 = 0xF0;
pub const IPI_TLB_FLUSH: u8 = 0xF1;
pub const IPI_CALL_FUNCTION: u8 = 0xF2;
pub const IPI_HALT: u8 = 0xF3;

/// IPI handler for rescheduling requests
/// Triggered when another CPU wants this CPU to reschedule its processes
pub extern "x86-interrupt" fn ipi_reschedule_handler(_stack_frame: InterruptStackFrame) {
    // Enter interrupt context
    crate::smp::enter_interrupt();

    // Track IPI received on this CPU
    if let Some(cpu_data) = crate::smp::current_cpu_data() {
        cpu_data
            .ipi_received
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        cpu_data
            .reschedule_pending
            .store(true, core::sync::atomic::Ordering::Release);
    }

    // Send EOI to LAPIC first to allow nested interrupts
    crate::lapic::send_eoi();

    crate::ktrace!("IPI: Reschedule request received, triggering scheduler");

    // Leave interrupt context - will trigger reschedule due to pending flag
    let _ = crate::smp::leave_interrupt();

    // Actually trigger the scheduler to perform a context switch
    // This allows AP cores to pick up ready processes
    crate::scheduler::do_schedule_from_interrupt();
}

/// IPI handler for TLB flush requests
/// Ensures all CPUs invalidate their TLB when page tables are modified
pub extern "x86-interrupt" fn ipi_tlb_flush_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::tlb;

    // Enter interrupt context
    crate::smp::enter_interrupt();

    // Track IPI received and mark TLB flush pending
    if let Some(cpu_data) = crate::smp::current_cpu_data() {
        cpu_data
            .ipi_received
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        cpu_data
            .tlb_flush_pending
            .store(true, core::sync::atomic::Ordering::Release);
    }

    // Flush entire TLB immediately
    tlb::flush_all();

    // Send EOI to LAPIC
    crate::lapic::send_eoi();

    crate::ktrace!("IPI: TLB flush completed");

    // Leave interrupt context
    let _ = crate::smp::leave_interrupt();
}

/// IPI handler for function call requests
/// Allows one CPU to execute a function on another CPU
///
/// This is the core mechanism for parallel display operations.
/// When compositor needs multi-core rendering, it sends this IPI
/// to all AP cores which then call into compositor::ap_work_entry().
pub extern "x86-interrupt" fn ipi_call_function_handler(_stack_frame: InterruptStackFrame) {
    // Enter interrupt context
    crate::smp::enter_interrupt();

    // Track IPI received and update interrupt counter
    if let Some(cpu_data) = crate::smp::current_cpu_data() {
        cpu_data
            .ipi_received
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        cpu_data
            .interrupts_handled
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    // Execute compositor work if available
    // This is the primary use case for IPI_CALL_FUNCTION
    crate::drivers::compositor::ap_work_entry();

    // Send EOI to LAPIC
    crate::lapic::send_eoi();

    // Leave interrupt context
    let _ = crate::smp::leave_interrupt();
}

/// IPI handler for halt requests
/// Allows graceful shutdown of individual CPUs
pub extern "x86-interrupt" fn ipi_halt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::hlt;

    // Track IPI received
    if let Some(cpu_data) = crate::smp::current_cpu_data() {
        cpu_data
            .ipi_received
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    crate::kinfo!("IPI: Halt request received, stopping CPU");

    // Send EOI to LAPIC before halting
    crate::lapic::send_eoi();

    // Disable interrupts and halt
    crate::lapic::send_eoi();

    // Disable interrupts and halt
    x86_64::instructions::interrupts::disable();
    loop {
        hlt();
    }
}
