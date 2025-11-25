//! Hardware Interrupt Handlers
//!
//! This module contains handlers for hardware interrupts (IRQs) from the PIC,
//! including timer, keyboard, and spurious interrupt handlers.

use pic8259::ChainedPics;
use spin;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptStackFrame;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Timer interrupt handler (IRQ0, vector 32)
pub extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // Send EOI to PIC first to allow nested interrupts
    unsafe {
        PICS.lock().notify_end_of_interrupt(PIC_1_OFFSET);
    }

    // Timer tick for scheduler (1ms granularity)
    const TIMER_TICK_MS: u64 = 1;

    // Check if current process should be preempted
    if crate::scheduler::tick(TIMER_TICK_MS) {
        // Time slice expired or higher priority process ready
        // Trigger rescheduling via do_schedule()
        // Note: This is safe because we're already in an interrupt context
        // and the scheduler will handle context switching properly
        crate::kdebug!("Timer: Triggering preemptive reschedule");

        // Implement full preemptive scheduling via timer interrupt
        crate::scheduler::do_schedule_from_interrupt();
    }
}

/// Keyboard interrupt handler (IRQ1, vector 33)
pub extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    crate::keyboard::add_scancode(scancode);

    // Send EOI to PIC
    unsafe {
        PICS.lock().notify_end_of_interrupt(PIC_1_OFFSET + 1);
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
