//! Architecture-specific code for NexaOS
//!
//! This module contains architecture-specific functionality including:
//! - Global Descriptor Table (GDT)
//! - Local APIC
//! - x86_64 specific code

pub mod gdt;
pub mod lapic;
pub mod x86_64;

pub use x86_64::halt_loop;

// Re-export GDT items
pub use gdt::{
    debug_dump_selectors, get_kernel_stack_top, init as init_gdt, init_ap as init_gdt_ap,
    Selectors, DOUBLE_FAULT_IST_INDEX, ERROR_CODE_IST_INDEX,
};

// Re-export LAPIC items
pub use lapic::{
    base as lapic_base, bsp_apic_id, current_apic_id, init as init_lapic, init_timer,
    read_error as lapic_read_error, send_eoi, send_init_ipi, send_ipi, send_startup_ipi,
    stop_timer,
};
