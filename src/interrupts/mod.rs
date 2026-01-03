//! Interrupt Descriptor Table (IDT) and Interrupt Handlers
//!
//! This module provides interrupt handling infrastructure for the NexaOS kernel,
//! including:
//! - Exception handlers (page fault, GP fault, etc.)
//! - Hardware interrupt handlers (timer, keyboard)
//! - System call entry points (int 0x81 and SYSCALL instruction)
//! - IPI handlers for SMP support
//!
//! # Module Organization
//!
//! - `idt`: IDT initialization and configuration
//! - `exceptions`: CPU exception handlers
//! - `handlers`: Hardware interrupt handlers (PIC IRQs)
//! - `syscall_asm`: Assembly entry points for system calls
//! - `gs_context`: GS segment data management for user/kernel transitions
//! - `ipi`: Inter-Processor Interrupt handlers for SMP
//! - `timer_asm`: Assembly wrapper for timer interrupt to save all GPRs

pub mod exceptions;
pub mod gs_context;
pub mod handlers;
pub mod idt;
pub mod ipi;
pub mod syscall_asm;
pub mod timer_asm;  // Assembly timer handler that saves all GPRs

// Re-export commonly used items at module level
pub use gs_context::{
    encode_hex_u64, restore_user_syscall_context, restore_user_syscall_context_full, set_gs_data,
    write_hex_u64, GS_SLOT_KERNEL_RSP, GS_SLOT_KERNEL_STACK_GUARD, GS_SLOT_KERNEL_STACK_SNAPSHOT,
    GS_SLOT_SAVED_RAX, GS_SLOT_SAVED_RCX, GS_SLOT_SAVED_RFLAGS, GS_SLOT_USER_CS, GS_SLOT_USER_DS,
    GS_SLOT_USER_ENTRY, GS_SLOT_USER_RSP, GS_SLOT_USER_RSP_DEBUG, GS_SLOT_USER_SS,
    GS_SLOT_USER_STACK, GUARD_SOURCE_INT_GATE, GUARD_SOURCE_SYSCALL,
};
pub use handlers::{PICS, PIC_1_OFFSET, PIC_2_OFFSET};
pub use idt::{
    init_interrupts, init_interrupts_ap, is_canonical_address, is_cpu_idt_initialized,
    is_idt_initialized, setup_syscall,
};
