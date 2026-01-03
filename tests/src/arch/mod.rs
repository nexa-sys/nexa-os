//! Architecture (x86_64) Subsystem Tests
//!
//! Tests for GDT, TSS, segment selectors, LAPIC and low-level architecture support.
//! Uses REAL kernel code - no simulated implementations.
//!
//! ## Coverage Areas
//! - GDT constants and structure
//! - TSS alignment and configuration
//! - Segment selector values
//! - IST (Interrupt Stack Table) indices
//! - LAPIC registers and constants

mod gdt;
mod tss;
mod selectors;
mod lapic;

pub use gdt::*;
pub use tss::*;
pub use selectors::*;
pub use lapic::*;
