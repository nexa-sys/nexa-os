//! Interrupts Subsystem Tests
//!
//! Tests for IDT, exception handlers, PIC configuration, and GS context.
//! Uses REAL kernel code - no simulated implementations.
//!
//! ## Coverage Areas
//! - GS slot constants and offsets
//! - PIC offset configuration
//! - IRQ vector numbers
//! - Exception handling constants
//! - IPI vectors

mod gs_context;
mod pic;
mod idt;
mod exceptions;
mod ipi;

pub use gs_context::*;
pub use pic::*;
pub use idt::*;
pub use exceptions::*;
pub use ipi::*;
