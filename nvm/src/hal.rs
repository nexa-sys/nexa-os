//! Hardware Abstraction Layer (HAL)
//!
//! This module provides the interface between kernel code and emulated hardware.
//! In the real kernel, these operations go to real hardware via inline assembly.
//! In tests, they go to the emulated hardware.
//!
//! The HAL is designed to be a drop-in replacement for `src/safety/x86.rs` functions.

use std::sync::{Arc, RwLock};
use super::cpu::VirtualCpu;
use super::devices::{DeviceManager, IoAccess};
use super::memory::PhysicalMemory;
use super::pci::PciBus;

// Re-export x86_64 types for mock functions
use x86_64::structures::paging::{PhysFrame, Size4KiB};
use x86_64::registers::control::Cr3Flags;
use x86_64::PhysAddr;

// Global HAL instance (thread-local for test isolation)
thread_local! {
    static HAL: RwLock<Option<Arc<HardwareAbstractionLayer>>> = RwLock::new(None);
}

/// Set the HAL for current thread
pub fn set_hal(hal: Arc<HardwareAbstractionLayer>) {
    HAL.with(|h| {
        *h.write().unwrap() = Some(hal);
    });
}

/// Get the current HAL (panics if not set)
pub fn get_hal() -> Arc<HardwareAbstractionLayer> {
    HAL.with(|h| {
        h.read().unwrap().clone().expect("HAL not initialized")
    })
}

/// Clear the HAL for current thread
pub fn clear_hal() {
    HAL.with(|h| {
        *h.write().unwrap() = None;
    });
}

/// Check if HAL is initialized
pub fn hal_initialized() -> bool {
    HAL.with(|h| {
        h.read().unwrap().is_some()
    })
}

/// Hardware Abstraction Layer
pub struct HardwareAbstractionLayer {
    /// Virtual CPU(s)
    pub cpus: RwLock<Vec<Arc<VirtualCpu>>>,
    /// Current CPU index
    current_cpu: RwLock<usize>,
    /// Physical memory
    pub memory: Arc<PhysicalMemory>,
    /// Device manager
    pub devices: Arc<DeviceManager>,
    /// PCI bus
    pub pci: RwLock<PciBus>,
}

impl HardwareAbstractionLayer {
    pub fn new(memory_mb: usize) -> Self {
        let memory = Arc::new(PhysicalMemory::new(memory_mb));
        let devices = Arc::new(DeviceManager::new());
        
        let hal = Self {
            cpus: RwLock::new(vec![Arc::new(VirtualCpu::new_bsp())]),
            current_cpu: RwLock::new(0),
            memory,
            devices,
            pci: RwLock::new(PciBus::with_host_bridge()),
        };
        
        hal
    }
    
    /// Get current CPU
    pub fn cpu(&self) -> Arc<VirtualCpu> {
        let idx = *self.current_cpu.read().unwrap();
        self.cpus.read().unwrap()[idx].clone()
    }
    
    /// Add an AP (application processor)
    pub fn add_cpu(&self) -> u32 {
        let mut cpus = self.cpus.write().unwrap();
        let id = cpus.len() as u32;
        cpus.push(Arc::new(VirtualCpu::new_ap(id)));
        id
    }
    
    /// Switch to a different CPU
    pub fn switch_cpu(&self, id: u32) {
        let cpus = self.cpus.read().unwrap();
        if (id as usize) < cpus.len() {
            *self.current_cpu.write().unwrap() = id as usize;
        }
    }
    
    // ========================================================================
    // Port I/O Operations (replacement for src/safety/x86.rs)
    // ========================================================================
    
    pub fn inb(&self, port: u16) -> u8 {
        // Check for PCI config ports first
        match port {
            0xCF8..=0xCFB => {
                let pci = self.pci.read().unwrap();
                let addr = pci.read_address();
                let shift = ((port - 0xCF8) * 8) as u32;
                ((addr >> shift) & 0xFF) as u8
            }
            0xCFC..=0xCFF => {
                let pci = self.pci.read().unwrap();
                let data = pci.read_data();
                let shift = ((port - 0xCFC) * 8) as u32;
                ((data >> shift) & 0xFF) as u8
            }
            _ => self.devices.port_read(port, IoAccess::Byte) as u8,
        }
    }
    
    pub fn inw(&self, port: u16) -> u16 {
        match port {
            0xCF8 | 0xCFA => {
                let pci = self.pci.read().unwrap();
                let addr = pci.read_address();
                let shift = ((port - 0xCF8) * 8) as u32;
                ((addr >> shift) & 0xFFFF) as u16
            }
            0xCFC | 0xCFE => {
                let pci = self.pci.read().unwrap();
                let data = pci.read_data();
                let shift = ((port - 0xCFC) * 8) as u32;
                ((data >> shift) & 0xFFFF) as u16
            }
            _ => self.devices.port_read(port, IoAccess::Word) as u16,
        }
    }
    
    pub fn inl(&self, port: u16) -> u32 {
        match port {
            0xCF8 => self.pci.read().unwrap().read_address(),
            0xCFC => self.pci.read().unwrap().read_data(),
            _ => self.devices.port_read(port, IoAccess::Dword),
        }
    }
    
    pub fn outb(&self, port: u16, value: u8) {
        match port {
            0xCF8..=0xCFB => {
                let mut pci = self.pci.write().unwrap();
                let addr = pci.read_address();
                let shift = ((port - 0xCF8) * 8) as u32;
                let mask = !(0xFF << shift);
                pci.write_address((addr & mask) | ((value as u32) << shift));
            }
            0xCFC..=0xCFF => {
                let pci = self.pci.write().unwrap();
                let data = pci.read_data();
                let shift = ((port - 0xCFC) * 8) as u32;
                let mask = !(0xFF << shift);
                pci.write_data((data & mask) | ((value as u32) << shift));
            }
            _ => self.devices.port_write(port, value as u32, IoAccess::Byte),
        }
    }
    
    pub fn outw(&self, port: u16, value: u16) {
        match port {
            0xCF8 | 0xCFA => {
                let mut pci = self.pci.write().unwrap();
                let addr = pci.read_address();
                let shift = ((port - 0xCF8) * 8) as u32;
                let mask = !(0xFFFF << shift);
                pci.write_address((addr & mask) | ((value as u32) << shift));
            }
            0xCFC | 0xCFE => {
                let pci = self.pci.write().unwrap();
                let data = pci.read_data();
                let shift = ((port - 0xCFC) * 8) as u32;
                let mask = !(0xFFFF << shift);
                pci.write_data((data & mask) | ((value as u32) << shift));
            }
            _ => self.devices.port_write(port, value as u32, IoAccess::Word),
        }
    }
    
    pub fn outl(&self, port: u16, value: u32) {
        match port {
            0xCF8 => self.pci.write().unwrap().write_address(value),
            0xCFC => self.pci.write().unwrap().write_data(value),
            _ => self.devices.port_write(port, value, IoAccess::Dword),
        }
    }
    
    // ========================================================================
    // PCI Config Space
    // ========================================================================
    
    pub fn pci_config_read32(&self, bus: u8, device: u8, function: u8, offset: u8) -> u32 {
        let address: u32 = 0x8000_0000
            | ((bus as u32) << 16)
            | ((device as u32 & 0x1F) << 11)
            | ((function as u32 & 0x07) << 8)
            | ((offset as u32) & 0xFC);
        
        let mut pci = self.pci.write().unwrap();
        pci.write_address(address);
        pci.read_data()
    }
    
    pub fn pci_config_write32(&self, bus: u8, device: u8, function: u8, offset: u8, value: u32) {
        let address: u32 = 0x8000_0000
            | ((bus as u32) << 16)
            | ((device as u32 & 0x1F) << 11)
            | ((function as u32 & 0x07) << 8)
            | ((offset as u32) & 0xFC);
        
        let mut pci = self.pci.write().unwrap();
        pci.write_address(address);
        pci.write_data(value);
    }
    
    // ========================================================================
    // CPU Operations
    // ========================================================================
    
    pub fn rdtsc(&self) -> u64 {
        self.cpu().rdtsc()
    }
    
    pub fn cpuid(&self, leaf: u32) -> (u32, u32, u32, u32) {
        self.cpu().cpuid(leaf, 0)
    }
    
    pub fn cpuid_count(&self, leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
        self.cpu().cpuid(leaf, subleaf)
    }
    
    pub fn read_cr3(&self) -> u64 {
        self.cpu().read_cr3()
    }
    
    pub fn write_cr3(&self, value: u64) {
        self.cpu().write_cr3(value);
    }
    
    pub fn read_rsp(&self) -> u64 {
        self.cpu().read_rsp()
    }
    
    pub fn hlt(&self) {
        self.cpu().halt();
    }
    
    pub fn pause(&self) {
        // No-op in emulation
        self.cpu().advance_cycles(10);
    }
    
    pub fn lfence(&self) {
        // No-op in emulation
    }
    
    pub fn sfence(&self) {
        // No-op in emulation
    }
    
    pub fn mfence(&self) {
        // No-op in emulation
    }
    
    // ========================================================================
    // Memory Operations
    // ========================================================================
    
    pub fn read_phys_u64(&self, addr: u64) -> u64 {
        // Check for MMIO
        if self.devices.is_mmio(addr) {
            let lo = self.devices.mmio_read(addr, IoAccess::Dword);
            let hi = self.devices.mmio_read(addr + 4, IoAccess::Dword);
            (lo as u64) | ((hi as u64) << 32)
        } else {
            self.memory.read_u64(addr)
        }
    }
    
    pub fn write_phys_u64(&self, addr: u64, value: u64) {
        if self.devices.is_mmio(addr) {
            self.devices.mmio_write(addr, value as u32, IoAccess::Dword);
            self.devices.mmio_write(addr + 4, (value >> 32) as u32, IoAccess::Dword);
        } else {
            self.memory.write_u64(addr, value);
        }
    }
    
    // ========================================================================
    // Interrupt Operations
    // ========================================================================
    
    pub fn enable_interrupts(&self) {
        self.cpu().enable_interrupts();
    }
    
    pub fn disable_interrupts(&self) {
        self.cpu().disable_interrupts();
    }
    
    pub fn interrupts_enabled(&self) -> bool {
        self.cpu().interrupts_enabled()
    }
    
    // ========================================================================
    // Time Simulation
    // ========================================================================
    
    /// Advance time by given number of cycles
    /// 
    /// This implements the real x86 hardware behavior:
    /// 1. Advance CPU cycle counters
    /// 2. Tick devices (which forwards device IRQs to PIC)
    /// 3. Check PIC for pending interrupts
    /// 4. If CPU has interrupts enabled, inject interrupt to CPU
    pub fn tick(&self, cycles: u64) {
        // Phase 1: Advance all CPUs
        for cpu in self.cpus.read().unwrap().iter() {
            cpu.advance_cycles(cycles);
        }
        
        // Phase 2: Tick all devices (this also forwards IRQs to PIC)
        self.devices.tick(cycles);
        
        // Phase 3: Check PIC and inject interrupts to CPU
        // This models the INTR line from PIC to CPU
        self.check_and_deliver_interrupts();
    }
    
    /// Check PIC for pending interrupts and deliver to CPU
    /// 
    /// Models the x86 interrupt delivery: PIC raises INTR, CPU samples at
    /// instruction boundary, sends INTA, receives vector, jumps to ISR.
    fn check_and_deliver_interrupts(&self) {
        use crate::devices::pic::Pic8259;
        use crate::devices::DeviceId;
        
        // Get PIC device
        if let Some(pic_dev) = self.devices.get_device(DeviceId::PIC_MASTER) {
            let mut pic_guard = pic_dev.lock().unwrap();
            if let Some(pic) = pic_guard.as_any_mut().downcast_mut::<Pic8259>() {
                // Check if PIC has pending interrupt
                if pic.has_interrupt() {
                    // Get current CPU
                    let cpu = self.cpu();
                    
                    // Only deliver if CPU has interrupts enabled (IF=1)
                    if cpu.interrupts_enabled() {
                        // Get the interrupt vector FIRST, then ACK
                        // This mimics the CPU's INTA cycle which returns the vector
                        if let Some(vector) = pic.get_interrupt_vector() {
                            // ACK the interrupt - clears IRR, sets ISR
                            pic.ack_interrupt();
                            // Inject interrupt to CPU
                            cpu.inject_interrupt(vector);
                        }
                    }
                }
            }
        }
    }
}

// ============================================================================
// Global function wrappers (for drop-in replacement of kernel safety functions)
// These are used when the kernel code calls safety::inb() etc.
// ============================================================================

/// Port I/O read byte - uses HAL if available, otherwise panics
pub fn inb(port: u16) -> u8 {
    if hal_initialized() {
        get_hal().inb(port)
    } else {
        panic!("HAL not initialized, cannot perform port I/O");
    }
}

pub fn inw(port: u16) -> u16 {
    if hal_initialized() {
        get_hal().inw(port)
    } else {
        panic!("HAL not initialized");
    }
}

pub fn inl(port: u16) -> u32 {
    if hal_initialized() {
        get_hal().inl(port)
    } else {
        panic!("HAL not initialized");
    }
}

pub fn outb(port: u16, value: u8) {
    if hal_initialized() {
        get_hal().outb(port, value);
    } else {
        panic!("HAL not initialized");
    }
}

pub fn outw(port: u16, value: u16) {
    if hal_initialized() {
        get_hal().outw(port, value);
    } else {
        panic!("HAL not initialized");
    }
}

pub fn outl(port: u16, value: u32) {
    if hal_initialized() {
        get_hal().outl(port, value);
    } else {
        panic!("HAL not initialized");
    }
}

pub fn pci_config_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    if hal_initialized() {
        get_hal().pci_config_read32(bus, device, function, offset)
    } else {
        panic!("HAL not initialized");
    }
}

pub fn pci_config_write32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    if hal_initialized() {
        get_hal().pci_config_write32(bus, device, function, offset, value);
    } else {
        panic!("HAL not initialized");
    }
}

pub fn rdtsc() -> u64 {
    if hal_initialized() {
        get_hal().rdtsc()
    } else {
        0 // Safe default for timing
    }
}

pub fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    if hal_initialized() {
        get_hal().cpuid(leaf)
    } else {
        (0, 0, 0, 0)
    }
}

pub fn cpuid_count(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    if hal_initialized() {
        get_hal().cpuid_count(leaf, subleaf)
    } else {
        (0, 0, 0, 0)
    }
}

pub fn read_cr3() -> u64 {
    if hal_initialized() {
        get_hal().read_cr3()
    } else {
        0
    }
}

pub fn write_cr3(value: u64) {
    if hal_initialized() {
        get_hal().write_cr3(value);
    }
}

pub fn hlt() {
    if hal_initialized() {
        get_hal().hlt();
    }
}

pub fn pause() {
    if hal_initialized() {
        get_hal().pause();
    }
}

pub fn lfence() {
    if hal_initialized() {
        get_hal().lfence();
    }
}

pub fn sfence() {
    if hal_initialized() {
        get_hal().sfence();
    }
}

pub fn mfence() {
    if hal_initialized() {
        get_hal().mfence();
    }
}

/// MMIO read 32-bit
pub fn mmio_read_u32(addr: u64) -> u32 {
    if hal_initialized() {
        let hal = get_hal();
        hal.devices.mmio_read(addr, super::devices::IoAccess::Dword)
    } else {
        0
    }
}

/// MMIO write 32-bit
pub fn mmio_write_u32(addr: u64, value: u32) {
    if hal_initialized() {
        let hal = get_hal();
        hal.devices.mmio_write(addr, value, super::devices::IoAccess::Dword);
    }
}

/// CLI - disable interrupts
pub fn cli() {
    if hal_initialized() {
        get_hal().disable_interrupts();
    }
}

/// STI - enable interrupts
pub fn sti() {
    if hal_initialized() {
        get_hal().enable_interrupts();
    }
}

/// Check if interrupts are enabled
pub fn interrupts_enabled() -> bool {
    if hal_initialized() {
        get_hal().interrupts_enabled()
    } else {
        false
    }
}

/// Execute closure with interrupts disabled
pub fn interrupt_free<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let was_enabled = interrupts_enabled();
    cli();
    let result = f();
    if was_enabled {
        sti();
    }
    result
}

// ============================================================================
// Memory Operations (for kernel memory.rs mmap support)
// ============================================================================

/// Set memory region to a value - uses HAL memory if available
/// 
/// # Safety
/// - `dst` must be valid for `count` bytes when HAL is initialized
/// - When HAL is initialized, dst is treated as a physical/kernel address
///   and the operation goes through HAL's memory system
pub unsafe fn memset(dst: *mut u8, value: u8, count: usize) {
    if hal_initialized() {
        let hal = get_hal();
        let addr = dst as u64;
        for i in 0..count {
            hal.memory.write_u8(addr + i as u64, value);
        }
    } else {
        // No HAL - use real memory (this is what happens in the real kernel)
        core::ptr::write_bytes(dst, value, count);
    }
}

/// Zero-initialize a memory region
///
/// # Safety  
/// - `dst` must be valid for `count` bytes
pub unsafe fn memzero(dst: *mut u8, count: usize) {
    memset(dst, 0, count);
}

/// Copy memory from one region to another
///
/// # Safety
/// - `src` and `dst` must be valid for `count` bytes
/// - Regions must not overlap
pub unsafe fn memcpy(dst: *mut u8, src: *const u8, count: usize) {
    if hal_initialized() {
        let hal = get_hal();
        let dst_addr = dst as u64;
        let src_addr = src as u64;
        for i in 0..count {
            let byte = hal.memory.read_u8(src_addr + i as u64);
            hal.memory.write_u8(dst_addr + i as u64, byte);
        }
    } else {
        core::ptr::copy_nonoverlapping(src, dst, count);
    }
}

// ===========================================================================
// Mock functions for x86_64 crate hardware calls
// These are drop-in replacements that return compatible types
// ===========================================================================

/// Mock for Cr3::read() - returns (PhysFrame, Cr3Flags) like the real function
pub fn mock_cr3_read() -> (PhysFrame<Size4KiB>, Cr3Flags) {
    let cr3_value = read_cr3();
    // Create a PhysFrame from the CR3 value (must be page-aligned)
    let addr = if cr3_value == 0 { 0x1000 } else { cr3_value & !0xFFF };
    let frame = PhysFrame::from_start_address(PhysAddr::new(addr))
        .expect("CR3 mock: invalid physical address");
    (frame, Cr3Flags::empty())
}

/// Mock for Cr3::write(frame, flags) - takes PhysFrame and Cr3Flags like the real function
pub fn mock_cr3_write(frame: PhysFrame<Size4KiB>, _flags: Cr3Flags) {
    let cr3_value = frame.start_address().as_u64();
    write_cr3(cr3_value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    
    #[test]
    fn test_hal_basic() {
        let hal = Arc::new(HardwareAbstractionLayer::new(64));
        set_hal(hal.clone());
        
        // Test CPUID
        let (eax, ebx, ecx, edx) = cpuid(0);
        assert!(eax >= 1); // Should have at least leaf 1
        
        // Test TSC
        let tsc1 = rdtsc();
        let tsc2 = rdtsc();
        assert!(tsc2 > tsc1);
        
        clear_hal();
    }
    
    #[test]
    fn test_hal_pci() {
        let hal = Arc::new(HardwareAbstractionLayer::new(64));
        set_hal(hal.clone());
        
        // Read host bridge vendor ID
        let vendor = pci_config_read32(0, 0, 0, 0) & 0xFFFF;
        assert_eq!(vendor, super::super::pci::vendor::INTEL as u32);
        
        clear_hal();
    }
    
    #[test]
    fn test_keyboard_interrupt_delivery() {
        use crate::devices::{Device, DeviceId, IoAccess};
        use crate::devices::pic::Pic8259;
        use crate::devices::keyboard::Ps2Keyboard;
        use std::sync::Mutex;
        
        let hal = Arc::new(HardwareAbstractionLayer::new(64));
        set_hal(hal.clone());
        
        // Create and register PIC
        let pic = Arc::new(Mutex::new(Pic8259::new()));
        hal.devices.add_device_with_ports(pic.clone(), &[(0x20, 0x21), (0xA0, 0xA1)]);
        
        // Initialize PIC with vector base 0x20
        {
            let mut p = pic.lock().unwrap();
            // Use Device trait methods explicitly
            // ICW1: init + ICW4 needed
            Device::port_write(&mut *p, 0x20, 0x11, IoAccess::Byte);
            // ICW2: vector base 0x20
            Device::port_write(&mut *p, 0x21, 0x20, IoAccess::Byte);
            // ICW3: slave on IRQ2
            Device::port_write(&mut *p, 0x21, 0x04, IoAccess::Byte);
            // ICW4: 8086 mode
            Device::port_write(&mut *p, 0x21, 0x01, IoAccess::Byte);
            // Unmask IRQ1 (keyboard)
            Device::port_write(&mut *p, 0x21, 0x00, IoAccess::Byte);
        }
        
        // Create and register keyboard
        let kb = Arc::new(Mutex::new(Ps2Keyboard::new()));
        hal.devices.add_device_with_ports(kb.clone(), &[(0x60, 0x64)]);
        
        // Enable interrupts on CPU
        hal.cpu().enable_interrupts();
        
        // Verify initial state: no pending interrupts
        assert!(!hal.cpu().has_pending_interrupt(), "Should have no pending interrupt initially");
        
        // Inject a key
        kb.lock().unwrap().inject_key("a", false);
        
        // Verify keyboard has interrupt pending
        {
            let k = kb.lock().unwrap();
            assert!(Device::has_interrupt(&*k), "Keyboard should have interrupt pending");
        }
        
        // Tick HAL - this should:
        // 1. Forward keyboard IRQ1 to PIC
        // 2. Check PIC for interrupts
        // 3. Inject interrupt to CPU
        hal.tick(1000);
        
        // Verify CPU received the interrupt
        assert!(hal.cpu().has_pending_interrupt(), "CPU should have pending interrupt after tick");
        
        // Verify the interrupt vector is correct (0x20 + 1 = 0x21 for IRQ1)
        let vector = hal.cpu().deliver_interrupt();
        assert_eq!(vector, Some(0x21), "Interrupt vector should be 0x21 (IRQ1)");
        
        clear_hal();
    }
}
