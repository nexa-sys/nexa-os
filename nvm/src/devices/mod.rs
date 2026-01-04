//! Device Emulation Framework
//!
//! Provides a trait-based system for emulating hardware devices.
//! Each device can respond to Port I/O, MMIO, and generate interrupts.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use super::memory::PhysAddr;

/// Device identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId(pub u32);

impl DeviceId {
    pub const PIC_MASTER: Self = Self(0x01);
    pub const PIC_SLAVE: Self = Self(0x02);
    pub const PIT: Self = Self(0x03);
    pub const UART_COM1: Self = Self(0x04);
    pub const UART_COM2: Self = Self(0x05);
    pub const RTC: Self = Self(0x06);
    pub const LAPIC: Self = Self(0x07);
    pub const IOAPIC: Self = Self(0x08);
    pub const PCI_HOST: Self = Self(0x10);
    pub const E1000: Self = Self(0x20);
    pub const VIRTIO_NET: Self = Self(0x21);
    pub const VIRTIO_BLK: Self = Self(0x22);
    pub const IDE: Self = Self(0x30);
    pub const AHCI: Self = Self(0x31);
    pub const NVME: Self = Self(0x32);
    pub const FRAMEBUFFER: Self = Self(0x40);
}

/// I/O access type
#[derive(Debug, Clone, Copy)]
pub enum IoAccess {
    Byte,
    Word,
    Dword,
}

/// Device trait - all emulated devices implement this
pub trait Device: Send + Sync {
    /// Device identifier
    fn id(&self) -> DeviceId;
    
    /// Human-readable name
    fn name(&self) -> &str;
    
    /// Reset device to initial state
    fn reset(&mut self);
    
    /// Port I/O read
    fn port_read(&mut self, port: u16, access: IoAccess) -> u32 {
        let _ = (port, access);
        0xFFFFFFFF // Default: return all 1s (no device)
    }
    
    /// Port I/O write
    fn port_write(&mut self, port: u16, value: u32, access: IoAccess) {
        let _ = (port, value, access);
    }
    
    /// Check if device handles this port
    fn handles_port(&self, port: u16) -> bool {
        let _ = port;
        false
    }
    
    /// MMIO read
    fn mmio_read(&mut self, addr: PhysAddr, access: IoAccess) -> u32 {
        let _ = (addr, access);
        0xFFFFFFFF
    }
    
    /// MMIO write
    fn mmio_write(&mut self, addr: PhysAddr, value: u32, access: IoAccess) {
        let _ = (addr, value, access);
    }
    
    /// Check if device handles this MMIO address
    fn handles_mmio(&self, addr: PhysAddr) -> bool {
        let _ = addr;
        false
    }
    
    /// Get MMIO region (base, size) if applicable
    fn mmio_region(&self) -> Option<(PhysAddr, usize)> {
        None
    }
    
    /// Check for pending interrupt
    fn has_interrupt(&self) -> bool {
        false
    }
    
    /// Get interrupt vector (if has_interrupt() is true)
    fn interrupt_vector(&self) -> Option<u8> {
        None
    }
    
    /// Acknowledge interrupt
    fn ack_interrupt(&mut self) {}
    
    /// Step device state (for timer-like devices)
    fn tick(&mut self, _cycles: u64) {}
}

/// Device manager - orchestrates all emulated devices
pub struct DeviceManager {
    devices: RwLock<Vec<Arc<Mutex<dyn Device>>>>,
    /// Port -> Device mapping for fast lookup
    port_map: RwLock<HashMap<u16, usize>>,
    /// MMIO region -> Device mapping
    mmio_map: RwLock<Vec<(PhysAddr, PhysAddr, usize)>>, // (start, end, device_idx)
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(Vec::new()),
            port_map: RwLock::new(HashMap::new()),
            mmio_map: RwLock::new(Vec::new()),
        }
    }
    
    /// Add a device
    pub fn add_device(&self, device: Arc<Mutex<dyn Device>>) {
        let mut devices = self.devices.write().unwrap();
        let idx = devices.len();
        
        // Build port mapping
        {
            let dev = device.lock().unwrap();
            let mut port_map = self.port_map.write().unwrap();
            
            // Check all possible ports (0-65535)
            // In practice, devices declare their port ranges
            for port in 0..=0xFFFF_u16 {
                if dev.handles_port(port) {
                    port_map.insert(port, idx);
                }
            }
            
            // Add MMIO region
            if let Some((base, size)) = dev.mmio_region() {
                self.mmio_map.write().unwrap().push((base, base + size as u64, idx));
            }
        }
        
        devices.push(device);
    }
    
    /// Add device with explicit port ranges (more efficient)
    pub fn add_device_with_ports(&self, device: Arc<Mutex<dyn Device>>, ports: &[(u16, u16)]) {
        let mut devices = self.devices.write().unwrap();
        let idx = devices.len();
        
        {
            let dev = device.lock().unwrap();
            let mut port_map = self.port_map.write().unwrap();
            
            for &(start, end) in ports {
                for port in start..=end {
                    port_map.insert(port, idx);
                }
            }
            
            if let Some((base, size)) = dev.mmio_region() {
                self.mmio_map.write().unwrap().push((base, base + size as u64, idx));
            }
        }
        
        devices.push(device);
    }
    
    /// Port I/O read
    pub fn port_read(&self, port: u16, access: IoAccess) -> u32 {
        let port_map = self.port_map.read().unwrap();
        if let Some(&idx) = port_map.get(&port) {
            let devices = self.devices.read().unwrap();
            let mut dev = devices[idx].lock().unwrap();
            dev.port_read(port, access)
        } else {
            0xFFFFFFFF // No device
        }
    }
    
    /// Port I/O write
    pub fn port_write(&self, port: u16, value: u32, access: IoAccess) {
        let port_map = self.port_map.read().unwrap();
        if let Some(&idx) = port_map.get(&port) {
            let devices = self.devices.read().unwrap();
            let mut dev = devices[idx].lock().unwrap();
            dev.port_write(port, value, access);
        }
    }
    
    /// MMIO read
    pub fn mmio_read(&self, addr: PhysAddr, access: IoAccess) -> u32 {
        let mmio_map = self.mmio_map.read().unwrap();
        for &(start, end, idx) in mmio_map.iter() {
            if addr >= start && addr < end {
                let devices = self.devices.read().unwrap();
                let mut dev = devices[idx].lock().unwrap();
                return dev.mmio_read(addr, access);
            }
        }
        0xFFFFFFFF
    }
    
    /// MMIO write
    pub fn mmio_write(&self, addr: PhysAddr, value: u32, access: IoAccess) {
        let mmio_map = self.mmio_map.read().unwrap();
        for &(start, end, idx) in mmio_map.iter() {
            if addr >= start && addr < end {
                let devices = self.devices.read().unwrap();
                let mut dev = devices[idx].lock().unwrap();
                dev.mmio_write(addr, value, access);
                return;
            }
        }
    }
    
    /// Check if address is MMIO
    pub fn is_mmio(&self, addr: PhysAddr) -> bool {
        let mmio_map = self.mmio_map.read().unwrap();
        mmio_map.iter().any(|&(start, end, _)| addr >= start && addr < end)
    }
    
    /// Get pending interrupts
    pub fn pending_interrupts(&self) -> Vec<(DeviceId, u8)> {
        let devices = self.devices.read().unwrap();
        let mut pending = Vec::new();
        
        for dev in devices.iter() {
            let d = dev.lock().unwrap();
            if d.has_interrupt() {
                if let Some(vec) = d.interrupt_vector() {
                    pending.push((d.id(), vec));
                }
            }
        }
        
        pending
    }
    
    /// Acknowledge interrupt for device
    pub fn ack_interrupt(&self, id: DeviceId) {
        let devices = self.devices.read().unwrap();
        for dev in devices.iter() {
            let mut d = dev.lock().unwrap();
            if d.id() == id {
                d.ack_interrupt();
                return;
            }
        }
    }
    
    /// Tick all devices
    pub fn tick(&self, cycles: u64) {
        let devices = self.devices.read().unwrap();
        for dev in devices.iter() {
            let mut d = dev.lock().unwrap();
            d.tick(cycles);
        }
    }
    
    /// Reset all devices
    pub fn reset_all(&self) {
        let devices = self.devices.read().unwrap();
        for dev in devices.iter() {
            let mut d = dev.lock().unwrap();
            d.reset();
        }
    }
    
    /// Get device by ID
    pub fn get_device(&self, id: DeviceId) -> Option<Arc<Mutex<dyn Device>>> {
        let devices = self.devices.read().unwrap();
        for dev in devices.iter() {
            if dev.lock().unwrap().id() == id {
                return Some(dev.clone());
            }
        }
        None
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Standard Device Implementations
// ============================================================================

/// 8259 PIC (Programmable Interrupt Controller) emulation
pub mod pic;

/// 8254 PIT (Programmable Interval Timer) emulation  
pub mod pit;

/// 16550 UART (Serial Port) emulation
pub mod uart;

/// RTC (Real Time Clock) emulation
pub mod rtc;

/// Local APIC emulation
pub mod lapic;

/// I/O APIC emulation
pub mod ioapic;

/// VGA/Framebuffer emulation
pub mod vga;

/// PS/2 Keyboard Controller emulation
pub mod keyboard;

#[cfg(test)]
mod tests {
    use super::*;
    
    struct DummyDevice {
        id: DeviceId,
        ports: Vec<u16>,
    }
    
    impl Device for DummyDevice {
        fn id(&self) -> DeviceId { self.id }
        fn name(&self) -> &str { "Dummy" }
        fn reset(&mut self) {}
        fn handles_port(&self, port: u16) -> bool {
            self.ports.contains(&port)
        }
        fn port_read(&mut self, _port: u16, _access: IoAccess) -> u32 {
            0x42
        }
    }
    
    #[test]
    fn test_device_manager_basic() {
        let mgr = DeviceManager::new();
        
        let dev = Arc::new(Mutex::new(DummyDevice {
            id: DeviceId(100),
            ports: vec![0x100, 0x101, 0x102],
        }));
        
        mgr.add_device_with_ports(dev, &[(0x100, 0x102)]);
        
        assert_eq!(mgr.port_read(0x100, IoAccess::Byte), 0x42);
        assert_eq!(mgr.port_read(0x999, IoAccess::Byte), 0xFFFFFFFF);
    }
}
