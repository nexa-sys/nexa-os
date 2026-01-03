//! Integration tests using the Virtual Machine emulation layer
//!
//! These tests verify that kernel subsystems work correctly together
//! by running against emulated hardware.

mod boot;
mod devices;
mod interrupt;
mod memory;
mod smp;
mod scheduler_smp;
