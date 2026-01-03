//! Hardware Interrupt Handlers
//!
//! This module contains handlers for hardware interrupts (IRQs) from the PIC,
//! including timer, keyboard, and spurious interrupt handlers.
//!
//! ## Per-CPU Interrupt Tracking
//!
//! All interrupt handlers use per-CPU state to track:
//! - Interrupt context (to disable preemption)
//! - Interrupt statistics (for load monitoring)
//! - Reschedule requests (for deferred scheduling)

use pic8259::ChainedPics;
use spin;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptStackFrame;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Inner timer interrupt handler called from assembly wrapper.
///
/// This is called by `timer_interrupt_handler_asm` after it has:
/// - Saved all GPRs on the kernel stack
/// - Populated GS_DATA slots with user context (for Ring 3 interrupts)
/// - Called swapgs if from Ring 3
///
/// The assembly wrapper will restore GPRs and call iretq after this returns.
#[no_mangle]
pub extern "C" fn timer_interrupt_handler_inner() {
    // Mark entering interrupt context (disables preemption)
    crate::smp::enter_interrupt();

    // Record interrupt on per-CPU statistics
    crate::smp::record_interrupt();

    // Update per-CPU local tick counter
    if let Some(cpu_data) = crate::smp::current_cpu_data() {
        cpu_data
            .local_tick
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    // Timer tick for scheduler (1ms granularity)
    const TIMER_TICK_MS: u64 = 1;

    // Check sleeping processes and wake them if their time has come
    crate::syscalls::time::check_sleepers();

    // Poll network stack to receive packets and wake up waiting processes.
    crate::net::poll();

    // Check if current process should be preempted
    let should_resched = crate::scheduler::tick(TIMER_TICK_MS);

    // Mark leaving interrupt context and check for pending reschedule
    let resched_pending = crate::smp::leave_interrupt();

    // CRITICAL: Send EOI AFTER all processing is complete but BEFORE reschedule.
    unsafe {
        PICS.lock().notify_end_of_interrupt(PIC_1_OFFSET);
    }

    if should_resched || resched_pending {
        // The assembly wrapper has already saved user context to GS_DATA,
        // so we don't need to do it here. Just call do_schedule_from_interrupt.
        crate::smp::ensure_kernel_gs_base();
        crate::scheduler::do_schedule_from_interrupt();
    }
}

/// Timer interrupt handler (IRQ0, vector 32)
/// 
/// DEPRECATED: This handler is kept for compatibility but should not be used.
/// Use timer_interrupt_handler_asm (in timer_asm.rs) instead, which properly
/// saves all GPRs to GS_DATA before calling timer_interrupt_handler_inner.
///
/// This is the main scheduling tick for BSP. It:
/// 1. Marks the CPU as in interrupt context
/// 2. Updates per-CPU tick counter
/// 3. Checks for preemption
/// 4. Triggers reschedule if needed
pub extern "x86-interrupt" fn timer_interrupt_handler(stack_frame: InterruptStackFrame) {
    // Mark entering interrupt context (disables preemption)
    crate::smp::enter_interrupt();

    // Record interrupt on per-CPU statistics
    crate::smp::record_interrupt();

    // Update per-CPU local tick counter
    if let Some(cpu_data) = crate::smp::current_cpu_data() {
        cpu_data
            .local_tick
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    // Timer tick for scheduler (1ms granularity)
    const TIMER_TICK_MS: u64 = 1;

    // Check sleeping processes and wake them if their time has come
    crate::syscalls::time::check_sleepers();

    // Poll network stack to receive packets and wake up waiting processes.
    // This is critical for processes blocked on recvfrom() - they need
    // network packets to be processed and wake_process() to be called.
    // Without this, sleeping processes waiting for network I/O would never wake up.
    crate::net::poll();

    // Check if current process should be preempted
    let should_resched = crate::scheduler::tick(TIMER_TICK_MS);

    // Mark leaving interrupt context and check for pending reschedule
    let resched_pending = crate::smp::leave_interrupt();

    // CRITICAL: Send EOI AFTER all processing is complete but BEFORE reschedule.
    // Sending EOI too early allows nested timer interrupts which can cause
    // deadlock if the interrupt handler tries to acquire a lock already held.
    unsafe {
        PICS.lock().notify_end_of_interrupt(PIC_1_OFFSET);
    }

    if should_resched || resched_pending {
        // Time slice expired or higher priority process ready
        // Trigger rescheduling via do_schedule()
        // CRITICAL: Ensure GS base points to kernel GS_DATA before calling scheduler.
        // When timer interrupt occurs while CPU is executing user-mode code, the CPU enters
        // kernel via interrupt gate WITHOUT swapgs. The scheduler's switch_return_trampoline
        // uses gs:[xxx] to read sysretq parameters, which would read garbage if GS base is wrong.
        crate::smp::ensure_kernel_gs_base();

        // Save user-mode context from InterruptStackFrame to GS_DATA for scheduler
        // This is needed because do_schedule_from_interrupt uses GS_DATA to restore user context.
        // Only save if the interrupt was from user mode (ring 3).
        let cs_ring = stack_frame.code_segment.0 & 3;
        if cs_ring == 3 {
            // User mode interrupt - save context to GS_DATA
            unsafe {
                let gs_data_ptr = crate::smp::current_gs_data_ptr();
                // GS_SLOT_USER_RSP = 0
                gs_data_ptr
                    .add(super::gs_context::GS_SLOT_USER_RSP)
                    .write(stack_frame.stack_pointer.as_u64());
                // GS_SLOT_SAVED_RCX = 7 (user RIP for sysretq)
                gs_data_ptr
                    .add(super::gs_context::GS_SLOT_SAVED_RCX)
                    .write(stack_frame.instruction_pointer.as_u64());
                // GS_SLOT_SAVED_RFLAGS = 8
                gs_data_ptr
                    .add(super::gs_context::GS_SLOT_SAVED_RFLAGS)
                    .write(stack_frame.cpu_flags.bits());
            }
        }

        crate::scheduler::do_schedule_from_interrupt();
    }
}

/// Keyboard interrupt handler (IRQ1, vector 33)
pub extern "x86-interrupt" fn keyboard_interrupt_handler(stack_frame: InterruptStackFrame) {
    // Mark entering interrupt context
    crate::smp::enter_interrupt();

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    crate::keyboard::add_scancode(scancode);

    // Send EOI to PIC
    unsafe {
        PICS.lock().notify_end_of_interrupt(PIC_1_OFFSET + 1);
    }

    // Record interrupt and leave context
    crate::smp::record_interrupt();
    let resched_pending = crate::smp::leave_interrupt();

    // CRITICAL FIX: Check if a process was woken up by keyboard input.
    // When add_scancode() wakes a sleeping process (e.g., shell waiting for input),
    // it sets the need_resched flag via wake_process(). We must check this flag
    // and reschedule immediately, otherwise the woken process must wait for the
    // next timer tick (up to 1ms), causing noticeable input lag.
    if resched_pending {
        // Ensure GS base points to kernel GS_DATA before calling scheduler
        crate::smp::ensure_kernel_gs_base();

        // Save user-mode context from InterruptStackFrame to GS_DATA for scheduler
        let cs_ring = stack_frame.code_segment.0 & 3;
        if cs_ring == 3 {
            // User mode interrupt - save context to GS_DATA
            unsafe {
                let gs_data_ptr = crate::smp::current_gs_data_ptr();
                gs_data_ptr
                    .add(super::gs_context::GS_SLOT_USER_RSP)
                    .write(stack_frame.stack_pointer.as_u64());
                gs_data_ptr
                    .add(super::gs_context::GS_SLOT_SAVED_RCX)
                    .write(stack_frame.instruction_pointer.as_u64());
                gs_data_ptr
                    .add(super::gs_context::GS_SLOT_SAVED_RFLAGS)
                    .write(stack_frame.cpu_flags.bits());
            }
        }

        crate::scheduler::do_schedule_from_interrupt();
    }
}

/// Macro to define spurious IRQ handlers that mask the interrupt line
macro_rules! define_spurious_irq {
    ($name:ident, $vector:expr) => {
        pub extern "x86-interrupt" fn $name(_stack_frame: InterruptStackFrame) {
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

/// LAPIC timer interrupt vector for AP cores
/// Using vector 0xEC (236) which is above PIC range and below IPI vectors
pub const LAPIC_TIMER_VECTOR: u8 = 0xEC;

/// LAPIC timer interrupt handler for AP cores
///
/// This provides the timer tick for scheduling on non-BSP cores.
/// Uses per-CPU state tracking for interrupt context and statistics.
pub extern "x86-interrupt" fn lapic_timer_handler(_stack_frame: InterruptStackFrame) {
    // Mark entering interrupt context (disables preemption)
    crate::smp::enter_interrupt();

    // Send EOI to LAPIC first
    crate::lapic::send_eoi();

    // Record interrupt on per-CPU statistics
    crate::smp::record_interrupt();

    // Update per-CPU local tick counter
    if let Some(cpu_data) = crate::smp::current_cpu_data() {
        cpu_data
            .local_tick
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    // Timer tick for scheduler (1ms granularity)
    const TIMER_TICK_MS: u64 = 1;

    // Check sleeping processes and wake them if their time has come
    crate::syscalls::time::check_sleepers();

    // Poll network stack to receive packets and wake up waiting processes.
    // This is critical for processes blocked on recvfrom() - they need
    // network packets to be processed and wake_process() to be called.
    crate::net::poll();

    // Check if current process should be preempted
    let should_resched = crate::scheduler::tick(TIMER_TICK_MS);

    // Mark leaving interrupt context and check for pending reschedule
    let resched_pending = crate::smp::leave_interrupt();

    if should_resched || resched_pending {
        crate::kdebug!("LAPIC Timer: Triggering preemptive reschedule on AP");
        // CRITICAL: Ensure GS base points to kernel GS_DATA before calling scheduler.
        crate::smp::ensure_kernel_gs_base();
        crate::scheduler::do_schedule_from_interrupt();
    }
}
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
