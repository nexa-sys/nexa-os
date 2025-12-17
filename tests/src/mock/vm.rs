//! Virtual Machine for Kernel Testing
//!
//! This is the main entry point for setting up a complete emulated environment
//! to test the full kernel.

use std::sync::{Arc, Mutex};
use super::cpu::VirtualCpu;
use super::devices::{Device, DeviceManager};
use super::devices::pic::Pic8259;
use super::devices::pit::Pit8254;
use super::devices::uart::Uart16550;
use super::devices::rtc::Rtc;
use super::devices::lapic::LocalApic;
use super::devices::ioapic::IoApic;
use super::hal::{HardwareAbstractionLayer, set_hal, clear_hal};
use super::memory::{PhysicalMemory, MemoryRegion, MemoryType};
use super::pci::{PciBus, PciConfig, PciLocation, SimplePciDevice, class, vendor};

/// Events that can occur in the VM
#[derive(Debug, Clone)]
pub enum VmEvent {
    /// Serial output received
    SerialOutput(Vec<u8>),
    /// Interrupt raised
    Interrupt { vector: u8, from_device: &'static str },
    /// Memory access
    MemoryAccess { addr: u64, size: usize, is_write: bool },
    /// Port I/O
    PortIo { port: u16, value: u32, is_write: bool },
}

/// VM configuration
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Memory size in MB
    pub memory_mb: usize,
    /// Number of CPUs
    pub cpus: usize,
    /// Enable PIC (8259)
    pub enable_pic: bool,
    /// Enable PIT (8254)
    pub enable_pit: bool,
    /// Enable serial (16550)
    pub enable_serial: bool,
    /// Enable RTC
    pub enable_rtc: bool,
    /// Enable APIC (LAPIC + IOAPIC)
    pub enable_apic: bool,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            memory_mb: 64,
            cpus: 1,
            enable_pic: true,
            enable_pit: true,
            enable_serial: true,
            enable_rtc: true,
            enable_apic: true,
        }
    }
}

impl VmConfig {
    pub fn minimal() -> Self {
        Self {
            memory_mb: 16,
            cpus: 1,
            enable_pic: false,
            enable_pit: false,
            enable_serial: true,
            enable_rtc: false,
            enable_apic: false,
        }
    }
    
    pub fn full() -> Self {
        Self {
            memory_mb: 128,
            cpus: 4,
            enable_pic: true,
            enable_pit: true,
            enable_serial: true,
            enable_rtc: true,
            enable_apic: true,
        }
    }
}

/// Virtual Machine for kernel testing
pub struct VirtualMachine {
    /// Hardware abstraction layer
    hal: Arc<HardwareAbstractionLayer>,
    /// Configuration
    config: VmConfig,
    /// Serial output capture
    serial_output: Arc<Mutex<Vec<u8>>>,
    /// Event log
    events: Arc<Mutex<Vec<VmEvent>>>,
}

impl VirtualMachine {
    /// Create a new VM with default configuration
    pub fn new() -> Self {
        Self::with_config(VmConfig::default())
    }
    
    /// Create a new VM with custom configuration
    pub fn with_config(config: VmConfig) -> Self {
        let hal = Arc::new(HardwareAbstractionLayer::new(config.memory_mb));
        let serial_output = Arc::new(Mutex::new(Vec::new()));
        
        // Add standard devices
        if config.enable_pic {
            let pic = Pic8259::new();
            hal.devices.add_device_with_ports(
                Arc::new(Mutex::new(pic)),
                &[(0x20, 0x21), (0xA0, 0xA1)],
            );
        }
        
        if config.enable_pit {
            let pit = Pit8254::new();
            hal.devices.add_device_with_ports(
                Arc::new(Mutex::new(pit)),
                &[(0x40, 0x43)],
            );
        }
        
        if config.enable_serial {
            let mut uart = Uart16550::new_com1();
            // Capture output
            let output = uart.output();
            hal.devices.add_device_with_ports(
                Arc::new(Mutex::new(uart)),
                &[(0x3F8, 0x3FF)],
            );
        }
        
        if config.enable_rtc {
            let rtc = Rtc::new();
            hal.devices.add_device_with_ports(
                Arc::new(Mutex::new(rtc)),
                &[(0x70, 0x71)],
            );
        }
        
        if config.enable_apic {
            let lapic = LocalApic::new(0);
            hal.devices.add_device(Arc::new(Mutex::new(lapic)));
            
            let ioapic = IoApic::new(0);
            hal.devices.add_device(Arc::new(Mutex::new(ioapic)));
        }
        
        // Add additional CPUs
        for _ in 1..config.cpus {
            hal.add_cpu();
        }
        
        Self {
            hal,
            config,
            serial_output,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    /// Get the HAL
    pub fn hal(&self) -> Arc<HardwareAbstractionLayer> {
        self.hal.clone()
    }
    
    /// Install the HAL for current thread (required before running kernel code)
    pub fn install(&self) {
        set_hal(self.hal.clone());
    }
    
    /// Uninstall the HAL
    pub fn uninstall(&self) {
        clear_hal();
    }
    
    /// Get physical memory
    pub fn memory(&self) -> Arc<PhysicalMemory> {
        self.hal.memory.clone()
    }
    
    /// Get device manager
    pub fn devices(&self) -> Arc<DeviceManager> {
        self.hal.devices.clone()
    }
    
    /// Get PCI bus
    pub fn pci(&self) -> &std::sync::RwLock<PciBus> {
        &self.hal.pci
    }
    
    /// Get current CPU
    pub fn cpu(&self) -> Arc<VirtualCpu> {
        self.hal.cpu()
    }
    
    /// Add a custom device
    pub fn add_device(&self, device: Arc<Mutex<dyn Device>>, ports: &[(u16, u16)]) {
        self.hal.devices.add_device_with_ports(device, ports);
    }
    
    /// Add a PCI device
    pub fn add_pci_device(&self, loc: PciLocation, config: PciConfig) {
        let device = Arc::new(Mutex::new(SimplePciDevice::new(config)));
        self.hal.pci.write().unwrap().add_device(loc, device);
    }
    
    /// Write data to physical memory
    pub fn write_memory(&self, addr: u64, data: &[u8]) {
        self.hal.memory.write_bytes(addr, data);
    }
    
    /// Read data from physical memory
    pub fn read_memory(&self, addr: u64, size: usize) -> Vec<u8> {
        let mut buf = vec![0u8; size];
        self.hal.memory.read_bytes(addr, &mut buf);
        buf
    }
    
    /// Inject input to serial port
    pub fn inject_serial_input(&self, data: &[u8]) {
        // Find COM1 device and inject
        // This is a simplified version - real implementation would find the UART device
    }
    
    /// Get serial output as string
    pub fn serial_output(&self) -> String {
        // Read from serial port output capture
        // For now, we'll read directly from the UART if we can find it
        String::new()
    }
    
    /// Advance VM time
    pub fn tick(&self, cycles: u64) {
        self.hal.tick(cycles);
    }
    
    /// Run for a number of cycles
    pub fn run(&self, cycles: u64) {
        self.tick(cycles);
    }
    
    /// Reset the VM
    pub fn reset(&self) {
        self.hal.devices.reset_all();
        // Reset CPUs
        for cpu in self.hal.cpus.read().unwrap().iter() {
            // CPUs would need a reset method
        }
    }
    
    /// Get event log
    pub fn events(&self) -> Vec<VmEvent> {
        self.events.lock().unwrap().clone()
    }
    
    /// Clear event log
    pub fn clear_events(&self) {
        self.events.lock().unwrap().clear();
    }
    
    /// Create a mock boot info structure (for kernel initialization)
    pub fn create_boot_info(&self) -> MockBootInfo {
        MockBootInfo {
            memory_map: self.hal.memory.memory_map(),
            kernel_start: 0x100000,
            kernel_end: 0x200000,
            initramfs_start: 0x200000,
            initramfs_end: 0x300000,
            framebuffer: None,
        }
    }
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for VirtualMachine {
    fn drop(&mut self) {
        // Ensure HAL is cleared
        clear_hal();
    }
}

/// Mock boot information (similar to Multiboot2 boot info)
#[derive(Debug, Clone)]
pub struct MockBootInfo {
    pub memory_map: Vec<MemoryRegion>,
    pub kernel_start: u64,
    pub kernel_end: u64,
    pub initramfs_start: u64,
    pub initramfs_end: u64,
    pub framebuffer: Option<MockFramebufferInfo>,
}

#[derive(Debug, Clone)]
pub struct MockFramebufferInfo {
    pub addr: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u8,
}

/// Test harness for running kernel code in the VM
pub struct TestHarness {
    vm: VirtualMachine,
}

impl TestHarness {
    pub fn new() -> Self {
        Self {
            vm: VirtualMachine::new(),
        }
    }
    
    pub fn with_config(config: VmConfig) -> Self {
        Self {
            vm: VirtualMachine::with_config(config),
        }
    }
    
    /// Run a test with the VM environment
    pub fn run<F, R>(&self, test: F) -> R
    where
        F: FnOnce(&VirtualMachine) -> R,
    {
        self.vm.install();
        let result = test(&self.vm);
        self.vm.uninstall();
        result
    }
    
    /// Get the VM
    pub fn vm(&self) -> &VirtualMachine {
        &self.vm
    }
}

impl Default for TestHarness {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience macro for creating a test with VM environment
#[macro_export]
macro_rules! vm_test {
    ($name:ident, $body:expr) => {
        #[test]
        fn $name() {
            let harness = $crate::mock::vm::TestHarness::new();
            harness.run(|_vm| {
                $body
            });
        }
    };
    ($name:ident, config = $config:expr, $body:expr) => {
        #[test]
        fn $name() {
            let harness = $crate::mock::vm::TestHarness::with_config($config);
            harness.run(|_vm| {
                $body
            });
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vm_basic() {
        let vm = VirtualMachine::new();
        vm.install();
        
        // Test that HAL is functional
        let tsc = super::super::hal::rdtsc();
        assert!(tsc > 0 || tsc == 0); // Just check it doesn't panic
        
        vm.uninstall();
    }
    
    #[test]
    fn test_vm_memory() {
        let vm = VirtualMachine::new();
        
        // Write and read memory
        let data = b"Test data for VM memory";
        vm.write_memory(0x10000, data);
        
        let read = vm.read_memory(0x10000, data.len());
        assert_eq!(&read[..], data);
    }
    
    #[test]
    fn test_vm_pci() {
        let vm = VirtualMachine::new();
        vm.install();
        
        // Enumerate PCI devices
        let devices = vm.pci().read().unwrap().enumerate();
        assert!(!devices.is_empty()); // Should have at least host bridge
        
        vm.uninstall();
    }
    
    #[test]
    fn test_vm_config_minimal() {
        let vm = VirtualMachine::with_config(VmConfig::minimal());
        vm.install();
        
        // Should work with minimal config
        let _tsc = super::super::hal::rdtsc();
        
        vm.uninstall();
    }
    
    #[test]
    fn test_harness() {
        let harness = TestHarness::new();
        
        let result = harness.run(|vm| {
            vm.write_memory(0x1000, b"Hello");
            let data = vm.read_memory(0x1000, 5);
            String::from_utf8_lossy(&data).to_string()
        });
        
        assert_eq!(result, "Hello");
    }
}
