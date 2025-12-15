//! Driver Registry for User-space Driver Framework
//!
//! Manages registration, discovery, and lifecycle of user-space drivers.
//!
//! # Design
//!
//! The registry provides:
//! - Driver registration and discovery
//! - Driver metadata management
//! - Dependency tracking
//! - Version management

use spin::Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

/// Driver ID type
pub type DriverId = u32;

/// Maximum registered drivers
pub const MAX_DRIVERS: usize = super::MAX_UDRV_DRIVERS;

/// Driver information
#[derive(Debug, Clone)]
pub struct DriverInfo {
    /// Driver ID (assigned on registration)
    pub id: DriverId,
    /// Driver name
    pub name: [u8; 32],
    /// Driver version
    pub version: DriverVersion,
    /// Driver class
    pub class: DriverClass,
    /// Supported devices (vendor:device pairs)
    pub devices: Vec<DeviceId>,
    /// Required isolation class
    pub isolation: super::IsolationClass,
    /// Entry point address
    pub entry_point: u64,
    /// Driver state
    pub state: DriverState,
    /// Flags
    pub flags: u32,
}

/// Driver version
#[derive(Debug, Clone, Copy, Default)]
pub struct DriverVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl DriverVersion {
    pub fn new(major: u8, minor: u8, patch: u8) -> Self {
        Self { major, minor, patch }
    }
}

/// Driver class/category
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DriverClass {
    /// Network interface
    Network = 0,
    /// Block device (disk, etc)
    Block = 1,
    /// Character device
    Char = 2,
    /// Input device (keyboard, mouse)
    Input = 3,
    /// Display/GPU
    Display = 4,
    /// Audio
    Audio = 5,
    /// USB
    Usb = 6,
    /// PCI bus driver
    Pci = 7,
    /// Platform device
    Platform = 8,
    /// Filesystem
    Filesystem = 9,
    /// Virtual device
    Virtual = 10,
    /// Other
    Other = 255,
}

/// Device ID (vendor:device)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceId {
    /// Vendor ID
    pub vendor: u16,
    /// Device ID  
    pub device: u16,
    /// Subsystem vendor (optional)
    pub subsys_vendor: u16,
    /// Subsystem device (optional)
    pub subsys_device: u16,
    /// Class code
    pub class_code: u32,
}

impl DeviceId {
    pub fn new(vendor: u16, device: u16) -> Self {
        Self {
            vendor,
            device,
            subsys_vendor: 0,
            subsys_device: 0,
            class_code: 0,
        }
    }
    
    pub fn with_class(vendor: u16, device: u16, class_code: u32) -> Self {
        Self {
            vendor,
            device,
            subsys_vendor: 0,
            subsys_device: 0,
            class_code,
        }
    }
    
    pub fn matches(&self, other: &DeviceId) -> bool {
        if self.vendor != 0 && self.vendor != other.vendor {
            return false;
        }
        if self.device != 0 && self.device != other.device {
            return false;
        }
        if self.class_code != 0 && self.class_code != other.class_code {
            return false;
        }
        true
    }
}

/// Well-known vendor IDs
pub mod vendor {
    pub const INTEL: u16 = 0x8086;
    pub const AMD: u16 = 0x1022;
    pub const NVIDIA: u16 = 0x10DE;
    pub const REALTEK: u16 = 0x10EC;
    pub const QEMU: u16 = 0x1234;
    pub const VIRTIO: u16 = 0x1AF4;
    pub const REDHAT: u16 = 0x1B36;
}

/// Driver state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DriverState {
    /// Registered but not loaded
    Registered = 0,
    /// Loading in progress
    Loading = 1,
    /// Loaded and ready
    Ready = 2,
    /// Bound to device
    Bound = 3,
    /// Running
    Running = 4,
    /// Stopped
    Stopped = 5,
    /// Failed
    Failed = 6,
}

/// Driver flags
pub mod driver_flags {
    /// Driver supports hot-plug
    pub const HOTPLUG: u32 = 1 << 0;
    /// Driver is built-in (not loadable)
    pub const BUILTIN: u32 = 1 << 1;
    /// Driver requires DMA
    pub const NEEDS_DMA: u32 = 1 << 2;
    /// Driver requires MMIO
    pub const NEEDS_MMIO: u32 = 1 << 3;
    /// Driver handles interrupts
    pub const HANDLES_IRQ: u32 = 1 << 4;
    /// Driver supports power management
    pub const POWER_MGMT: u32 = 1 << 5;
    /// Driver is a twin driver
    pub const TWIN_DRIVER: u32 = 1 << 6;
}

/// Driver registry
#[derive(Debug)]
pub struct DriverRegistry {
    drivers: [Option<DriverInfo>; MAX_DRIVERS],
    count: usize,
    next_id: u32,
}

impl DriverRegistry {
    /// Create new registry
    pub fn new() -> Self {
        Self {
            drivers: core::array::from_fn(|_| None),
            count: 0,
            next_id: 1,
        }
    }
    
    /// Register a driver
    pub fn register(&mut self, mut info: DriverInfo) -> Result<DriverId, super::RegistryError> {
        if self.count >= MAX_DRIVERS {
            return Err(super::RegistryError::TableFull);
        }
        
        // Assign ID
        let id = self.next_id;
        self.next_id += 1;
        info.id = id;
        info.state = DriverState::Registered;
        
        // Find empty slot
        for slot in self.drivers.iter_mut() {
            if slot.is_none() {
                *slot = Some(info);
                self.count += 1;
                return Ok(id);
            }
        }
        
        Err(super::RegistryError::TableFull)
    }
    
    /// Unregister a driver
    pub fn unregister(&mut self, id: DriverId) -> Result<(), super::RegistryError> {
        for slot in self.drivers.iter_mut() {
            if let Some(driver) = slot {
                if driver.id == id {
                    if driver.state == DriverState::Running || driver.state == DriverState::Bound {
                        return Err(super::RegistryError::InvalidState);
                    }
                    *slot = None;
                    self.count -= 1;
                    return Ok(());
                }
            }
        }
        Err(super::RegistryError::NotFound)
    }
    
    /// Get driver info
    pub fn get_info(&self, id: DriverId) -> Option<DriverInfo> {
        self.drivers.iter()
            .find_map(|slot| slot.as_ref().filter(|d| d.id == id))
            .cloned()
    }
    
    /// Find driver by name
    pub fn find_by_name(&self, name: &str) -> Option<DriverId> {
        let name_bytes = name.as_bytes();
        self.drivers.iter()
            .find_map(|slot| {
                slot.as_ref().and_then(|d| {
                    let len = d.name.iter().position(|&b| b == 0).unwrap_or(32);
                    if &d.name[..len] == name_bytes {
                        Some(d.id)
                    } else {
                        None
                    }
                })
            })
    }
    
    /// Find drivers for device
    pub fn find_for_device(&self, device: &DeviceId) -> Vec<DriverId> {
        self.drivers.iter()
            .filter_map(|slot| {
                slot.as_ref().and_then(|d| {
                    if d.devices.iter().any(|dev_id| dev_id.matches(device)) {
                        Some(d.id)
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
    
    /// Find drivers by class
    pub fn find_by_class(&self, class: DriverClass) -> Vec<DriverId> {
        self.drivers.iter()
            .filter_map(|slot| {
                slot.as_ref().and_then(|d| {
                    if d.class == class {
                        Some(d.id)
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
    
    /// List all drivers
    pub fn list_drivers(&self) -> Vec<DriverId> {
        self.drivers.iter()
            .filter_map(|slot| slot.as_ref().map(|d| d.id))
            .collect()
    }
    
    /// Update driver state
    pub fn set_state(&mut self, id: DriverId, state: DriverState) -> Result<(), super::RegistryError> {
        for slot in self.drivers.iter_mut() {
            if let Some(driver) = slot {
                if driver.id == id {
                    driver.state = state;
                    return Ok(());
                }
            }
        }
        Err(super::RegistryError::NotFound)
    }
    
    /// Get driver count
    pub fn count(&self) -> usize {
        self.count
    }
}

/// Initialize registry subsystem
pub fn init() {
    crate::kinfo!("UDRV/Registry: Initializing driver registry");
    crate::kinfo!("UDRV/Registry: {} max drivers supported", MAX_DRIVERS);
}

/// Create a new driver info builder
pub fn driver_builder(name: &str, class: DriverClass) -> DriverInfoBuilder {
    DriverInfoBuilder::new(name, class)
}

/// Driver info builder for ergonomic construction
pub struct DriverInfoBuilder {
    info: DriverInfo,
}

impl DriverInfoBuilder {
    pub fn new(name: &str, class: DriverClass) -> Self {
        let mut name_buf = [0u8; 32];
        let name_bytes = name.as_bytes();
        let len = core::cmp::min(name_bytes.len(), 31);
        name_buf[..len].copy_from_slice(&name_bytes[..len]);
        
        Self {
            info: DriverInfo {
                id: 0,
                name: name_buf,
                version: DriverVersion::default(),
                class,
                devices: Vec::new(),
                isolation: super::IsolationClass::IC2,
                entry_point: 0,
                state: DriverState::Registered,
                flags: 0,
            }
        }
    }
    
    pub fn version(mut self, major: u8, minor: u8, patch: u8) -> Self {
        self.info.version = DriverVersion::new(major, minor, patch);
        self
    }
    
    pub fn device(mut self, vendor: u16, device: u16) -> Self {
        self.info.devices.push(DeviceId::new(vendor, device));
        self
    }
    
    pub fn device_class(mut self, vendor: u16, device: u16, class_code: u32) -> Self {
        self.info.devices.push(DeviceId::with_class(vendor, device, class_code));
        self
    }
    
    pub fn isolation(mut self, iso: super::IsolationClass) -> Self {
        self.info.isolation = iso;
        self
    }
    
    pub fn entry_point(mut self, addr: u64) -> Self {
        self.info.entry_point = addr;
        self
    }
    
    pub fn flags(mut self, flags: u32) -> Self {
        self.info.flags = flags;
        self
    }
    
    pub fn build(self) -> DriverInfo {
        self.info
    }
}

// ---- Device matching utilities ----

/// PCI class codes
pub mod pci_class {
    pub const NETWORK_ETHERNET: u32 = 0x020000;
    pub const STORAGE_IDE: u32 = 0x010100;
    pub const STORAGE_SATA: u32 = 0x010600;
    pub const STORAGE_NVME: u32 = 0x010802;
    pub const DISPLAY_VGA: u32 = 0x030000;
    pub const MULTIMEDIA_AUDIO: u32 = 0x040100;
    pub const SERIAL_USB: u32 = 0x0C0300;
}

/// Match device against all registered drivers
pub fn match_device(device: &DeviceId) -> Option<DriverId> {
    let registry = super::DRIVER_REGISTRY.lock();
    registry
        .as_ref()?
        .find_for_device(device)
        .first()
        .copied()
}
