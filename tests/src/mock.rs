//! Hardware mocks for testing
//!
//! These modules mock the **underlying hardware** that the kernel depends on,
//! NOT the kernel's own implementations. This allows testing kernel code
//! without actual hardware.
//!
//! ## What should be mocked:
//! - Physical memory (page allocation)
//! - Port I/O (inb/outb)
//! - MMIO regions
//! - CPU state (CR3, interrupts)
//! - Timers and clocks
//!
//! ## What should NOT be mocked:
//! - Kernel subsystems (scheduler, VFS, IPC)
//! - Those should be tested using the real kernel code via #[path]

/// Mock physical memory allocator
/// Simulates physical page allocation without actual hardware
pub mod memory;
