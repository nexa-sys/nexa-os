//! Modular Network Driver Support
//!
//! This module provides the kernel-side interface for loadable network drivers
//! (such as e1000.nkm) which are loaded as kernel modules rather than being
//! compiled into the kernel.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────────┐     ┌────────────────┐
//! │   Net Stack │────▶│  net_modular    │────▶│  e1000.nkm     │
//! │   (net/)    │     │  (this module)  │     │  (loadable)    │
//! └─────────────┘     └─────────────────┘     └────────────────┘
//!                            │
//!                            ▼
//!                     ┌─────────────────┐
//!                     │  Net Driver Ops │
//!                     │  (FFI callbacks)│
//!                     └─────────────────┘
//! ```
//!
//! Network driver modules register their operations through `kmod_net_register()`
//! when loaded. The kernel then routes network operations through these callbacks.

use spin::Mutex;

// ============================================================================
// Error Types
// ============================================================================

/// Network driver errors
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(i32)]
pub enum NetDriverError {
    UnsupportedDevice = -1,
    DeviceMissing = -2,
    RxExhausted = -3,
    TxBusy = -4,
    InvalidDescriptor = -5,
    HardwareFault = -6,
    BufferTooSmall = -7,
    ModuleNotLoaded = -100,
    InvalidOperation = -101,
    AlreadyRegistered = -102,
    TooManyDevices = -103,
}

impl NetDriverError {
    /// Convert from module return code
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            -1 => Some(Self::UnsupportedDevice),
            -2 => Some(Self::DeviceMissing),
            -3 => Some(Self::RxExhausted),
            -4 => Some(Self::TxBusy),
            -5 => Some(Self::InvalidDescriptor),
            -6 => Some(Self::HardwareFault),
            -7 => Some(Self::BufferTooSmall),
            -100 => Some(Self::ModuleNotLoaded),
            -101 => Some(Self::InvalidOperation),
            -102 => Some(Self::AlreadyRegistered),
            -103 => Some(Self::TooManyDevices),
            _ => None,
        }
    }
}

// ============================================================================
// FFI Types for Module Callbacks
// ============================================================================

/// Network device descriptor passed from kernel to driver module
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NetDeviceDescriptor {
    /// Index in kernel's device table
    pub index: usize,
    /// MMIO base address
    pub mmio_base: u64,
    /// MMIO region length
    pub mmio_length: u64,
    /// PCI segment
    pub pci_segment: u16,
    /// PCI bus
    pub pci_bus: u8,
    /// PCI device
    pub pci_device: u8,
    /// PCI function
    pub pci_function: u8,
    /// Interrupt line
    pub interrupt_line: u8,
    /// MAC address length
    pub mac_len: u8,
    /// MAC address (up to 32 bytes)
    pub mac_address: [u8; 32],
    /// Reserved for alignment
    pub _reserved: [u8; 5],
}

impl NetDeviceDescriptor {
    pub const fn empty() -> Self {
        Self {
            index: 0,
            mmio_base: 0,
            mmio_length: 0,
            pci_segment: 0,
            pci_bus: 0,
            pci_device: 0,
            pci_function: 0,
            interrupt_line: 0,
            mac_len: 0,
            mac_address: [0; 32],
            _reserved: [0; 5],
        }
    }
}

/// Opaque handle to a network driver instance (managed by module)
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct NetDriverHandle(pub *mut u8);

impl NetDriverHandle {
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

// SAFETY: NetDriverHandle is just a pointer that the module manages
unsafe impl Send for NetDriverHandle {}
unsafe impl Sync for NetDriverHandle {}

// ============================================================================
// Module Operations Table (registered by e1000.nkm etc.)
// ============================================================================

/// Create new driver instance from device descriptor
pub type FnNetDriverNew = extern "C" fn(desc: *const NetDeviceDescriptor) -> NetDriverHandle;

/// Destroy driver instance
pub type FnNetDriverDestroy = extern "C" fn(handle: NetDriverHandle);

/// Initialize driver hardware
pub type FnNetDriverInit = extern "C" fn(handle: NetDriverHandle) -> i32;

/// Update DMA addresses after driver relocation
pub type FnNetDriverUpdateDma = extern "C" fn(handle: NetDriverHandle);

/// Transmit a frame
pub type FnNetDriverTransmit =
    extern "C" fn(handle: NetDriverHandle, frame: *const u8, len: usize) -> i32;

/// Drain RX queue, returns frame length or 0 if no frames
pub type FnNetDriverDrainRx =
    extern "C" fn(handle: NetDriverHandle, buf: *mut u8, buf_len: usize) -> i32;

/// Perform periodic maintenance (link status check, etc.)
pub type FnNetDriverMaintenance = extern "C" fn(handle: NetDriverHandle) -> i32;

/// Get MAC address
pub type FnNetDriverGetMac = extern "C" fn(handle: NetDriverHandle, mac: *mut u8);

/// Check if driver supports a specific PCI vendor/device ID
pub type FnNetDriverProbe = extern "C" fn(vendor_id: u16, device_id: u16) -> i32;

/// Module operations table for network drivers
#[repr(C)]
pub struct NetDriverOps {
    /// Driver name (null-terminated, max 32 bytes)
    pub name: [u8; 32],
    /// Probe function - check if driver supports device
    pub probe: Option<FnNetDriverProbe>,
    /// Create new driver instance
    pub new: Option<FnNetDriverNew>,
    /// Destroy driver instance
    pub destroy: Option<FnNetDriverDestroy>,
    /// Initialize hardware
    pub init: Option<FnNetDriverInit>,
    /// Update DMA addresses
    pub update_dma: Option<FnNetDriverUpdateDma>,
    /// Transmit frame
    pub transmit: Option<FnNetDriverTransmit>,
    /// Drain RX queue
    pub drain_rx: Option<FnNetDriverDrainRx>,
    /// Maintenance callback
    pub maintenance: Option<FnNetDriverMaintenance>,
    /// Get MAC address
    pub get_mac: Option<FnNetDriverGetMac>,
}

impl NetDriverOps {
    pub const fn empty() -> Self {
        Self {
            name: [0; 32],
            probe: None,
            new: None,
            destroy: None,
            init: None,
            update_dma: None,
            transmit: None,
            drain_rx: None,
            maintenance: None,
            get_mac: None,
        }
    }

    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(32);
        core::str::from_utf8(&self.name[..end]).unwrap_or("unknown")
    }

    fn is_valid(&self) -> bool {
        self.new.is_some()
            && self.init.is_some()
            && self.transmit.is_some()
            && self.drain_rx.is_some()
            && self.get_mac.is_some()
    }
}

// ============================================================================
// Global State
// ============================================================================

/// Maximum number of registered network drivers
const MAX_NET_DRIVERS: usize = 8;

/// Registered driver slot
struct RegisteredDriver {
    ops: NetDriverOps,
    active: bool,
}

impl RegisteredDriver {
    const fn empty() -> Self {
        Self {
            ops: NetDriverOps::empty(),
            active: false,
        }
    }
}

/// Registered network drivers
static NET_DRIVERS: Mutex<[RegisteredDriver; MAX_NET_DRIVERS]> = Mutex::new([
    RegisteredDriver::empty(),
    RegisteredDriver::empty(),
    RegisteredDriver::empty(),
    RegisteredDriver::empty(),
    RegisteredDriver::empty(),
    RegisteredDriver::empty(),
    RegisteredDriver::empty(),
    RegisteredDriver::empty(),
]);

/// Active driver instances per device
const MAX_NET_DEVICES: usize = 4;

struct ActiveDevice {
    handle: NetDriverHandle,
    driver_index: usize,
    active: bool,
}

impl ActiveDevice {
    const fn empty() -> Self {
        Self {
            handle: NetDriverHandle(core::ptr::null_mut()),
            driver_index: 0,
            active: false,
        }
    }
}

static ACTIVE_DEVICES: Mutex<[ActiveDevice; MAX_NET_DEVICES]> = Mutex::new([
    ActiveDevice::empty(),
    ActiveDevice::empty(),
    ActiveDevice::empty(),
    ActiveDevice::empty(),
]);

// ============================================================================
// Module Registration API (called by e1000.nkm etc.)
// ============================================================================

/// Register a network driver module
/// Called by driver module during module_init
#[no_mangle]
pub extern "C" fn kmod_net_register(ops: *const NetDriverOps) -> i32 {
    if ops.is_null() {
        crate::kerror!("net_modular: null ops pointer");
        return -1;
    }

    let ops = unsafe { &*ops };

    // Validate required operations
    if ops.new.is_none() {
        crate::kerror!("net_modular: missing 'new' operation");
        return -1;
    }
    if ops.init.is_none() {
        crate::kerror!("net_modular: missing 'init' operation");
        return -1;
    }
    if ops.transmit.is_none() {
        crate::kerror!("net_modular: missing 'transmit' operation");
        return -1;
    }
    if ops.drain_rx.is_none() {
        crate::kerror!("net_modular: missing 'drain_rx' operation");
        return -1;
    }

    let mut drivers = NET_DRIVERS.lock();

    // Find empty slot
    let slot = drivers.iter_mut().find(|d| !d.active);

    match slot {
        Some(slot) => {
            slot.ops.name.copy_from_slice(&ops.name);
            slot.ops.probe = ops.probe;
            slot.ops.new = ops.new;
            slot.ops.destroy = ops.destroy;
            slot.ops.init = ops.init;
            slot.ops.update_dma = ops.update_dma;
            slot.ops.transmit = ops.transmit;
            slot.ops.drain_rx = ops.drain_rx;
            slot.ops.maintenance = ops.maintenance;
            slot.ops.get_mac = ops.get_mac;
            slot.active = true;

            crate::kinfo!("net_modular: registered driver '{}'", ops.name_str());
            0
        }
        None => {
            crate::kerror!("net_modular: no free driver slots");
            NetDriverError::TooManyDevices as i32
        }
    }
}

/// Unregister a network driver module
#[no_mangle]
pub extern "C" fn kmod_net_unregister(name: *const u8, name_len: usize) -> i32 {
    if name.is_null() || name_len == 0 {
        return -1;
    }

    let name_bytes = unsafe { core::slice::from_raw_parts(name, name_len.min(32)) };
    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");

    let mut drivers = NET_DRIVERS.lock();

    // First, find the driver index
    let mut found_idx: Option<usize> = None;
    let mut destroy_fn: Option<FnNetDriverDestroy> = None;

    for (idx, slot) in drivers.iter().enumerate() {
        if slot.active && slot.ops.name_str() == name_str {
            found_idx = Some(idx);
            destroy_fn = slot.ops.destroy;
            break;
        }
    }

    if let Some(driver_idx) = found_idx {
        // Destroy any active devices using this driver
        {
            let mut devices = ACTIVE_DEVICES.lock();
            for dev in devices.iter_mut() {
                if dev.active && dev.driver_index == driver_idx {
                    if let Some(destroy) = destroy_fn {
                        destroy(dev.handle);
                    }
                    *dev = ActiveDevice::empty();
                }
            }
        }

        // Clear the driver slot
        drivers[driver_idx] = RegisteredDriver::empty();
        crate::kinfo!("net_modular: unregistered driver '{}'", name_str);
        return 0;
    }

    crate::kwarn!("net_modular: driver '{}' not found", name_str);
    -1
}

// ============================================================================
// Kernel API (used by net/ subsystem)
// ============================================================================

/// Check if any network driver modules are loaded
pub fn has_drivers() -> bool {
    NET_DRIVERS.lock().iter().any(|d| d.active)
}

/// Find a driver that supports the given PCI device
pub fn find_driver_for_device(vendor_id: u16, device_id: u16) -> Option<usize> {
    let drivers = NET_DRIVERS.lock();

    for (idx, slot) in drivers.iter().enumerate() {
        if !slot.active {
            continue;
        }

        if let Some(probe) = slot.ops.probe {
            if probe(vendor_id, device_id) == 0 {
                return Some(idx);
            }
        }
    }

    None
}

/// Create a driver instance for a device
pub fn create_driver_instance(
    driver_index: usize,
    device_index: usize,
    desc: &NetDeviceDescriptor,
) -> Result<(), NetDriverError> {
    let drivers = NET_DRIVERS.lock();

    if driver_index >= MAX_NET_DRIVERS || !drivers[driver_index].active {
        return Err(NetDriverError::ModuleNotLoaded);
    }

    let new_fn = drivers[driver_index]
        .ops
        .new
        .ok_or(NetDriverError::InvalidOperation)?;
    let init_fn = drivers[driver_index]
        .ops
        .init
        .ok_or(NetDriverError::InvalidOperation)?;

    drop(drivers); // Release lock before calling into module

    let handle = new_fn(desc);
    if handle.is_null() {
        return Err(NetDriverError::InvalidDescriptor);
    }

    let result = init_fn(handle);
    if result != 0 {
        // Cleanup on init failure
        let drivers = NET_DRIVERS.lock();
        if let Some(destroy) = drivers[driver_index].ops.destroy {
            destroy(handle);
        }
        return Err(NetDriverError::from_code(result).unwrap_or(NetDriverError::HardwareFault));
    }

    // Store active device
    {
        let mut devices = ACTIVE_DEVICES.lock();
        if device_index >= MAX_NET_DEVICES {
            return Err(NetDriverError::TooManyDevices);
        }
        devices[device_index] = ActiveDevice {
            handle,
            driver_index,
            active: true,
        };
    }

    crate::kinfo!(
        "net_modular: created driver instance for device {}",
        device_index
    );
    Ok(())
}

/// Update DMA addresses for a device
pub fn update_dma_addresses(device_index: usize) {
    let devices = ACTIVE_DEVICES.lock();
    if device_index >= MAX_NET_DEVICES || !devices[device_index].active {
        return;
    }

    let driver_index = devices[device_index].driver_index;
    let handle = devices[device_index].handle;
    drop(devices);

    let drivers = NET_DRIVERS.lock();
    if let Some(update_dma) = drivers[driver_index].ops.update_dma {
        drop(drivers);
        update_dma(handle);
    }
}

/// Transmit a frame on a device
pub fn transmit(device_index: usize, frame: &[u8]) -> Result<(), NetDriverError> {
    let devices = ACTIVE_DEVICES.lock();
    if device_index >= MAX_NET_DEVICES || !devices[device_index].active {
        return Err(NetDriverError::DeviceMissing);
    }

    let driver_index = devices[device_index].driver_index;
    let handle = devices[device_index].handle;
    drop(devices);

    let drivers = NET_DRIVERS.lock();
    let transmit_fn = drivers[driver_index]
        .ops
        .transmit
        .ok_or(NetDriverError::InvalidOperation)?;
    drop(drivers);

    let result = transmit_fn(handle, frame.as_ptr(), frame.len());
    if result != 0 {
        return Err(NetDriverError::from_code(result).unwrap_or(NetDriverError::TxBusy));
    }

    Ok(())
}

/// Drain RX queue for a device
pub fn drain_rx(device_index: usize, buf: &mut [u8]) -> Option<usize> {
    let devices = ACTIVE_DEVICES.lock();
    if device_index >= MAX_NET_DEVICES || !devices[device_index].active {
        return None;
    }

    let driver_index = devices[device_index].driver_index;
    let handle = devices[device_index].handle;
    drop(devices);

    let drivers = NET_DRIVERS.lock();
    let drain_fn = drivers[driver_index].ops.drain_rx?;
    drop(drivers);

    let result = drain_fn(handle, buf.as_mut_ptr(), buf.len());
    if result > 0 {
        Some(result as usize)
    } else {
        None
    }
}

/// Perform maintenance on a device
pub fn maintenance(device_index: usize) -> Result<(), NetDriverError> {
    let devices = ACTIVE_DEVICES.lock();
    if device_index >= MAX_NET_DEVICES || !devices[device_index].active {
        return Err(NetDriverError::DeviceMissing);
    }

    let driver_index = devices[device_index].driver_index;
    let handle = devices[device_index].handle;
    drop(devices);

    let drivers = NET_DRIVERS.lock();
    if let Some(maint_fn) = drivers[driver_index].ops.maintenance {
        drop(drivers);
        let result = maint_fn(handle);
        if result != 0 {
            return Err(NetDriverError::from_code(result).unwrap_or(NetDriverError::HardwareFault));
        }
    }

    Ok(())
}

/// Get MAC address for a device
pub fn get_mac_address(device_index: usize) -> Option<[u8; 6]> {
    let devices = ACTIVE_DEVICES.lock();
    if device_index >= MAX_NET_DEVICES || !devices[device_index].active {
        return None;
    }

    let driver_index = devices[device_index].driver_index;
    let handle = devices[device_index].handle;
    drop(devices);

    let drivers = NET_DRIVERS.lock();
    let get_mac_fn = drivers[driver_index].ops.get_mac?;
    drop(drivers);

    let mut mac = [0u8; 6];
    get_mac_fn(handle, mac.as_mut_ptr());
    Some(mac)
}

/// Check if a device is active
pub fn is_device_active(device_index: usize) -> bool {
    let devices = ACTIVE_DEVICES.lock();
    device_index < MAX_NET_DEVICES && devices[device_index].active
}

// ============================================================================
// Symbol Registration (for module loader)
// ============================================================================

/// Register network modular symbols with kernel symbol table
pub fn register_symbols() {
    use crate::kmod::symbols::{register_symbol, SymbolType};

    register_symbol(
        "kmod_net_register",
        kmod_net_register as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_net_unregister",
        kmod_net_unregister as *const () as u64,
        SymbolType::Function,
    );

    // Register I/O helper functions for drivers
    register_symbol(
        "kmod_mmio_read32",
        kmod_mmio_read32 as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_mmio_write32",
        kmod_mmio_write32 as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_pci_read_config_word",
        kmod_pci_read_config_word as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_pci_write_config_word",
        kmod_pci_write_config_word as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_inl",
        kmod_inl as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_outl",
        kmod_outl as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_fence",
        kmod_fence as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_spin_hint",
        kmod_spin_hint as *const () as u64,
        SymbolType::Function,
    );

    crate::kinfo!("net_modular: registered kernel symbols");
}

// ============================================================================
// I/O Helper Functions (exported to modules)
// ============================================================================

/// Read 32-bit value from MMIO address
#[no_mangle]
pub extern "C" fn kmod_mmio_read32(addr: u64) -> u32 {
    unsafe { crate::safety::volatile_read(addr as *const u32) }
}

/// Write 32-bit value to MMIO address
#[no_mangle]
pub extern "C" fn kmod_mmio_write32(addr: u64, value: u32) {
    unsafe { crate::safety::volatile_write(addr as *mut u32, value) }
}

/// Read PCI config word
#[no_mangle]
pub extern "C" fn kmod_pci_read_config_word(bus: u8, device: u8, function: u8, offset: u32) -> u16 {
    let address = 0x80000000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | (offset & 0xFC);

    crate::safety::outl(0xCF8, address);
    let data = crate::safety::inl(0xCFC);
    ((data >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

/// Write PCI config word
#[no_mangle]
pub extern "C" fn kmod_pci_write_config_word(
    bus: u8,
    device: u8,
    function: u8,
    offset: u32,
    value: u16,
) {
    let address = 0x80000000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | (offset & 0xFC);

    crate::safety::outl(0xCF8, address);
    let shift = (offset & 2) * 8;
    let mut data = crate::safety::inl(0xCFC);
    data = (data & !(0xFFFF << shift)) | ((value as u32) << shift);
    crate::safety::outl(0xCFC, data);
}

/// Read 32-bit value from I/O port
#[no_mangle]
pub extern "C" fn kmod_inl(port: u16) -> u32 {
    crate::safety::inl(port)
}

/// Write 32-bit value to I/O port
#[no_mangle]
pub extern "C" fn kmod_outl(port: u16, value: u32) {
    crate::safety::outl(port, value);
}

/// Memory fence (full barrier)
#[no_mangle]
pub extern "C" fn kmod_fence() {
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// CPU spin hint
#[no_mangle]
pub extern "C" fn kmod_spin_hint() {
    core::hint::spin_loop();
}
