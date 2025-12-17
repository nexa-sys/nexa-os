//! PCI Bus Emulation
//!
//! Emulates PCI configuration space access for device enumeration.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// PCI configuration space size per function
pub const PCI_CONFIG_SIZE: usize = 256;

/// PCI vendor IDs
pub mod vendor {
    pub const INTEL: u16 = 0x8086;
    pub const QEMU: u16 = 0x1234;
    pub const REDHAT: u16 = 0x1AF4;
    pub const NEXAOS: u16 = 0xFFFF; // Fake vendor for testing
}

/// PCI device classes
pub mod class {
    pub const UNCLASSIFIED: u8 = 0x00;
    pub const STORAGE: u8 = 0x01;
    pub const NETWORK: u8 = 0x02;
    pub const DISPLAY: u8 = 0x03;
    pub const MULTIMEDIA: u8 = 0x04;
    pub const MEMORY: u8 = 0x05;
    pub const BRIDGE: u8 = 0x06;
    pub const SERIAL: u8 = 0x0C;
}

/// PCI location (BDF - Bus:Device.Function)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciLocation {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciLocation {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        Self { bus, device, function }
    }
    
    /// Convert to configuration address
    pub fn config_address(&self, offset: u8) -> u32 {
        0x8000_0000
            | ((self.bus as u32) << 16)
            | ((self.device as u32 & 0x1F) << 11)
            | ((self.function as u32 & 0x07) << 8)
            | ((offset as u32) & 0xFC)
    }
}

/// PCI configuration space for a device
#[derive(Clone)]
pub struct PciConfig {
    data: [u8; PCI_CONFIG_SIZE],
}

impl PciConfig {
    pub fn new() -> Self {
        Self { data: [0xFF; PCI_CONFIG_SIZE] }
    }
    
    /// Create a new device configuration
    pub fn device(
        vendor_id: u16,
        device_id: u16,
        class_code: u8,
        subclass: u8,
        prog_if: u8,
    ) -> Self {
        let mut config = Self::new();
        
        // Vendor ID (offset 0x00)
        config.write16(0x00, vendor_id);
        // Device ID (offset 0x02)
        config.write16(0x02, device_id);
        
        // Status (offset 0x06)
        config.write16(0x06, 0x0000);
        
        // Class code (offset 0x09-0x0B)
        config.data[0x09] = prog_if;
        config.data[0x0A] = subclass;
        config.data[0x0B] = class_code;
        
        // Header type (offset 0x0E) - type 0 (endpoint)
        config.data[0x0E] = 0x00;
        
        // Capabilities pointer (offset 0x34) - no capabilities
        config.data[0x34] = 0x00;
        
        config
    }
    
    /// Set BAR (Base Address Register)
    pub fn set_bar(&mut self, bar: usize, value: u32, is_io: bool, is_64bit: bool) {
        if bar >= 6 {
            return;
        }
        
        let offset = 0x10 + (bar * 4);
        let flags = if is_io {
            0x01 // I/O space
        } else if is_64bit {
            0x04 // 64-bit memory
        } else {
            0x00 // 32-bit memory
        };
        
        self.write32(offset, value | flags);
    }
    
    /// Set interrupt line and pin
    pub fn set_interrupt(&mut self, line: u8, pin: u8) {
        self.data[0x3C] = line;
        self.data[0x3D] = pin;
    }
    
    /// Set subsystem IDs
    pub fn set_subsystem(&mut self, vendor: u16, device: u16) {
        self.write16(0x2C, vendor);
        self.write16(0x2E, device);
    }
    
    pub fn read8(&self, offset: usize) -> u8 {
        self.data.get(offset).copied().unwrap_or(0xFF)
    }
    
    pub fn read16(&self, offset: usize) -> u16 {
        let lo = self.read8(offset) as u16;
        let hi = self.read8(offset + 1) as u16;
        lo | (hi << 8)
    }
    
    pub fn read32(&self, offset: usize) -> u32 {
        let lo = self.read16(offset) as u32;
        let hi = self.read16(offset + 2) as u32;
        lo | (hi << 16)
    }
    
    pub fn write8(&mut self, offset: usize, value: u8) {
        if offset < PCI_CONFIG_SIZE {
            self.data[offset] = value;
        }
    }
    
    pub fn write16(&mut self, offset: usize, value: u16) {
        self.write8(offset, value as u8);
        self.write8(offset + 1, (value >> 8) as u8);
    }
    
    pub fn write32(&mut self, offset: usize, value: u32) {
        self.write16(offset, value as u16);
        self.write16(offset + 2, (value >> 16) as u16);
    }
}

impl Default for PciConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// PCI device with configuration space and optional behavior
pub trait PciDevice: Send + Sync {
    /// Get device configuration
    fn config(&self) -> &PciConfig;
    
    /// Get mutable device configuration
    fn config_mut(&mut self) -> &mut PciConfig;
    
    /// Handle BAR read
    fn bar_read(&mut self, bar: usize, offset: u32, size: usize) -> u32 {
        let _ = (bar, offset, size);
        0
    }
    
    /// Handle BAR write
    fn bar_write(&mut self, bar: usize, offset: u32, value: u32, size: usize) {
        let _ = (bar, offset, value, size);
    }
}

/// Simple PCI device with just configuration space
pub struct SimplePciDevice {
    config: PciConfig,
}

impl SimplePciDevice {
    pub fn new(config: PciConfig) -> Self {
        Self { config }
    }
}

impl PciDevice for SimplePciDevice {
    fn config(&self) -> &PciConfig {
        &self.config
    }
    
    fn config_mut(&mut self) -> &mut PciConfig {
        &mut self.config
    }
}

/// PCI bus emulation
pub struct PciBus {
    /// Devices indexed by location
    devices: HashMap<PciLocation, Arc<Mutex<dyn PciDevice>>>,
    /// Current config address register value
    config_address: Mutex<u32>,
}

impl PciBus {
    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
            config_address: Mutex::new(0),
        }
    }
    
    /// Add a device to the bus
    pub fn add_device(&mut self, loc: PciLocation, device: Arc<Mutex<dyn PciDevice>>) {
        self.devices.insert(loc, device);
    }
    
    /// Create standard host bridge
    pub fn with_host_bridge() -> Self {
        let mut bus = Self::new();
        
        // Host bridge at 0:0.0
        let config = PciConfig::device(
            vendor::INTEL,
            0x1237, // i440FX
            class::BRIDGE,
            0x00, // Host bridge
            0x00,
        );
        
        bus.add_device(
            PciLocation::new(0, 0, 0),
            Arc::new(Mutex::new(SimplePciDevice::new(config))),
        );
        
        bus
    }
    
    /// Parse config address
    fn parse_address(addr: u32) -> Option<(PciLocation, u8)> {
        if addr & 0x8000_0000 == 0 {
            return None;
        }
        
        let bus = ((addr >> 16) & 0xFF) as u8;
        let device = ((addr >> 11) & 0x1F) as u8;
        let function = ((addr >> 8) & 0x07) as u8;
        let offset = (addr & 0xFC) as u8;
        
        Some((PciLocation::new(bus, device, function), offset))
    }
    
    /// Read from config address port (0xCF8)
    pub fn read_address(&self) -> u32 {
        *self.config_address.lock().unwrap()
    }
    
    /// Write to config address port (0xCF8)
    pub fn write_address(&self, value: u32) {
        *self.config_address.lock().unwrap() = value;
    }
    
    /// Read from config data port (0xCFC)
    pub fn read_data(&self) -> u32 {
        let addr = *self.config_address.lock().unwrap();
        
        if let Some((loc, offset)) = Self::parse_address(addr) {
            if let Some(device) = self.devices.get(&loc) {
                let dev = device.lock().unwrap();
                return dev.config().read32(offset as usize);
            }
        }
        
        0xFFFFFFFF // No device
    }
    
    /// Write to config data port (0xCFC)
    pub fn write_data(&self, value: u32) {
        let addr = *self.config_address.lock().unwrap();
        
        if let Some((loc, offset)) = Self::parse_address(addr) {
            if let Some(device) = self.devices.get(&loc) {
                let mut dev = device.lock().unwrap();
                dev.config_mut().write32(offset as usize, value);
            }
        }
    }
    
    /// Enumerate all devices
    pub fn enumerate(&self) -> Vec<(PciLocation, u16, u16)> {
        let mut result = Vec::new();
        
        for (&loc, device) in &self.devices {
            let dev = device.lock().unwrap();
            let vendor = dev.config().read16(0x00);
            let device_id = dev.config().read16(0x02);
            
            if vendor != 0xFFFF {
                result.push((loc, vendor, device_id));
            }
        }
        
        result
    }
}

impl Default for PciBus {
    fn default() -> Self {
        Self::with_host_bridge()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pci_config() {
        let config = PciConfig::device(
            vendor::INTEL,
            0x100E, // e1000
            class::NETWORK,
            0x00,
            0x00,
        );
        
        assert_eq!(config.read16(0x00), vendor::INTEL);
        assert_eq!(config.read16(0x02), 0x100E);
        assert_eq!(config.read8(0x0B), class::NETWORK);
    }
    
    #[test]
    fn test_pci_bus_enumerate() {
        let mut bus = PciBus::with_host_bridge();
        
        // Add a network card
        let e1000_config = PciConfig::device(
            vendor::INTEL,
            0x100E,
            class::NETWORK,
            0x00,
            0x00,
        );
        
        bus.add_device(
            PciLocation::new(0, 3, 0),
            Arc::new(Mutex::new(SimplePciDevice::new(e1000_config))),
        );
        
        let devices = bus.enumerate();
        assert_eq!(devices.len(), 2);
    }
    
    #[test]
    fn test_pci_config_access() {
        let bus = PciBus::with_host_bridge();
        
        // Read host bridge vendor ID
        let addr = PciLocation::new(0, 0, 0).config_address(0x00);
        bus.write_address(addr);
        
        let data = bus.read_data();
        assert_eq!(data & 0xFFFF, vendor::INTEL as u32);
    }
}
