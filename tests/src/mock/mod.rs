//! Hardware Emulation Layer for Kernel Testing
//!
//! This module provides a complete hardware emulation layer that allows testing
//! the FULL kernel without QEMU or any external emulator. Instead of mocking
//! individual functions, we emulate the actual hardware behavior.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                        Test Code (cargo test)                       │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                    Real Kernel Code (via #[path])                   │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                  Hardware Abstraction Layer (HAL)                   │
//! │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐  │
//! │  │  Port IO │ │   MMIO   │ │ CPU Regs │ │   IRQs   │ │  Timers  │  │
//! │  └────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘  │
//! ├───────┴────────────┴────────────┴────────────┴────────────┴────────┤
//! │                    Hardware Emulation Engine                        │
//! │  ┌────────────────────────────────────────────────────────────────┐│
//! │  │                     Virtual Machine State                      ││
//! │  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐  ││
//! │  │  │ vCPU(s) │ │ vMemory │ │ vDevices│ │vInterrupt│ │ vTimers │  ││
//! │  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘  ││
//! │  └────────────────────────────────────────────────────────────────┘│
//! │                                                                     │
//! │  Emulated Devices:                                                  │
//! │  • PIC (8259) - Interrupt Controller                               │
//! │  • PIT (8254) - Programmable Interval Timer                        │
//! │  • UART (16550) - Serial Port                                      │
//! │  • PCI Bus - Device Enumeration                                    │
//! │  • LAPIC/IOAPIC - Advanced Interrupt Controllers                   │
//! │  • RTC - Real Time Clock                                           │
//! │  • E1000/Virtio - Network Devices                                  │
//! │  • IDE/AHCI/NVMe - Storage Devices                                 │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Principles
//!
//! 1. **Emulate hardware, not kernel code** - The kernel code runs unchanged
//! 2. **Behavioral accuracy over cycle accuracy** - We care about correctness
//! 3. **Deterministic execution** - Tests must be reproducible
//! 4. **Full observability** - Inspect all VM state for assertions
//!
//! # Usage
//!
//! ```rust,ignore
//! use nexa_os_tests::mock::VirtualMachine;
//!
//! #[test]
//! fn test_kernel_boot() {
//!     let mut vm = VirtualMachine::new();
//!     vm.configure_memory(64 * 1024 * 1024); // 64MB RAM
//!     vm.attach_device(Box::new(Serial16550::new()));
//!     vm.attach_device(Box::new(Pic8259::new()));
//!     
//!     // The kernel code runs against emulated hardware
//!     kernel_main(&vm.boot_info());
//!     
//!     // Verify serial output
//!     assert!(vm.serial_output().contains("[INFO] NexaOS booting"));
//! }
//! ```

pub mod cpu;
pub mod devices;
pub mod hal;
pub mod memory;
pub mod pci;
pub mod vm;

// Re-export main components
pub use cpu::{VirtualCpu, CpuState, Registers};
pub use devices::{Device, DeviceId, DeviceManager};
pub use hal::HardwareAbstractionLayer;
pub use memory::{MockPageAllocator, VirtualMemory, PhysicalMemory, MemoryRegion};
pub use pci::{PciBus, PciDevice, PciConfig};
pub use vm::{VirtualMachine, VmConfig, VmEvent};
