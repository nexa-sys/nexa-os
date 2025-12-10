//! Block Device Subsystem for NexaOS
//!
//! This module provides the kernel-side interface for block devices
//! and the modular driver framework for block device drivers.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────────┐     ┌────────────────┐
//! │   VFS/FS    │────▶│  block layer    │────▶│  virtio_blk.nkm│
//! │   Layer     │     │  (this module)  │     │  (loadable)    │
//! └─────────────┘     └─────────────────┘     └────────────────┘
//!                            │
//!                            ▼
//!                     ┌─────────────────┐
//!                     │  Block Ops      │
//!                     │  (FFI callbacks)│
//!                     └─────────────────┘
//! ```
//!
//! Block device drivers register their operations through `kmod_blk_register()`.
//! The kernel then routes all block I/O operations through these callbacks.

use alloc::vec::Vec;
use core::ptr;
use spin::Mutex;

// ============================================================================
// Error Types
// ============================================================================

/// Block device operation errors
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(i32)]
pub enum BlockError {
    /// Device not found
    NotFound = -1,
    /// I/O error during read/write
    IoError = -2,
    /// Invalid sector number
    InvalidSector = -3,
    /// Device is read-only
    ReadOnly = -4,
    /// Device not ready
    NotReady = -5,
    /// Buffer alignment error
    Alignment = -6,
    /// Device busy
    Busy = -7,
    /// Driver not loaded
    DriverNotLoaded = -100,
    /// Invalid operation
    InvalidOp = -101,
}

impl BlockError {
    /// Convert from driver return code
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            -1 => Some(Self::NotFound),
            -2 => Some(Self::IoError),
            -3 => Some(Self::InvalidSector),
            -4 => Some(Self::ReadOnly),
            -5 => Some(Self::NotReady),
            -6 => Some(Self::Alignment),
            -7 => Some(Self::Busy),
            -100 => Some(Self::DriverNotLoaded),
            -101 => Some(Self::InvalidOp),
            _ => None,
        }
    }
}

// ============================================================================
// Block Device Types
// ============================================================================

/// Block device handle
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct BlockDeviceHandle(pub *mut u8);

impl BlockDeviceHandle {
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

// SAFETY: BlockDeviceHandle is just a pointer managed by the driver
unsafe impl Send for BlockDeviceHandle {}
unsafe impl Sync for BlockDeviceHandle {}

/// Block device information
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BlockDeviceInfo {
    /// Device name (e.g., "vda", "sda")
    pub name: [u8; 16],
    /// Sector size in bytes (typically 512)
    pub sector_size: u32,
    /// Total number of sectors
    pub total_sectors: u64,
    /// Device is read-only
    pub read_only: bool,
    /// Device is removable
    pub removable: bool,
    /// PCI address (if applicable)
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
}

impl BlockDeviceInfo {
    pub const fn empty() -> Self {
        Self {
            name: [0; 16],
            sector_size: 512,
            total_sectors: 0,
            read_only: false,
            removable: false,
            pci_bus: 0,
            pci_device: 0,
            pci_function: 0,
        }
    }

    /// Get device name as string
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(16);
        core::str::from_utf8(&self.name[..end]).unwrap_or("unknown")
    }

    /// Get total capacity in bytes
    pub fn capacity(&self) -> u64 {
        self.total_sectors * self.sector_size as u64
    }
}

/// Block device descriptor for boot info
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BootBlockDevice {
    pub pci_segment: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub sector_size: u32,
    pub total_sectors: u64,
    pub features: u64,
}

impl BootBlockDevice {
    pub const fn empty() -> Self {
        Self {
            pci_segment: 0,
            pci_bus: 0,
            pci_device: 0,
            pci_function: 0,
            mmio_base: 0,
            mmio_length: 0,
            sector_size: 512,
            total_sectors: 0,
            features: 0,
        }
    }
}

// ============================================================================
// Module Operations Table (registered by block device drivers)
// ============================================================================

/// Function pointer types for driver operations
pub type FnBlkProbe = extern "C" fn(vendor_id: u16, device_id: u16) -> i32;
pub type FnBlkNew = extern "C" fn(desc: *const BootBlockDevice) -> BlockDeviceHandle;
pub type FnBlkDestroy = extern "C" fn(handle: BlockDeviceHandle);
pub type FnBlkInit = extern "C" fn(handle: BlockDeviceHandle) -> i32;
pub type FnBlkGetInfo = extern "C" fn(handle: BlockDeviceHandle, info: *mut BlockDeviceInfo) -> i32;
pub type FnBlkRead =
    extern "C" fn(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *mut u8) -> i32;
pub type FnBlkWrite =
    extern "C" fn(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *const u8) -> i32;
pub type FnBlkFlush = extern "C" fn(handle: BlockDeviceHandle) -> i32;

/// Block driver operations table
#[repr(C)]
pub struct BlockDriverOps {
    /// Driver name (null-terminated)
    pub name: [u8; 32],
    /// Check if driver supports this PCI device
    pub probe: Option<FnBlkProbe>,
    /// Create driver instance for a device
    pub new: Option<FnBlkNew>,
    /// Destroy driver instance
    pub destroy: Option<FnBlkDestroy>,
    /// Initialize the device
    pub init: Option<FnBlkInit>,
    /// Get device information
    pub get_info: Option<FnBlkGetInfo>,
    /// Read sectors
    pub read: Option<FnBlkRead>,
    /// Write sectors
    pub write: Option<FnBlkWrite>,
    /// Flush write cache
    pub flush: Option<FnBlkFlush>,
}

// ============================================================================
// Global Block Device Registry
// ============================================================================

/// Registered block device driver
struct RegisteredDriver {
    ops: BlockDriverOps,
}

/// Active block device instance
struct BlockDevice {
    /// Driver name
    driver_name: [u8; 32],
    /// Device handle from driver
    handle: BlockDeviceHandle,
    /// Device info
    info: BlockDeviceInfo,
    /// Index in device array
    index: usize,
}

/// Global block subsystem state
struct BlockSubsystem {
    /// Registered drivers
    drivers: Vec<RegisteredDriver>,
    /// Active block devices
    devices: Vec<BlockDevice>,
    /// Module is initialized
    initialized: bool,
}

impl BlockSubsystem {
    const fn new() -> Self {
        Self {
            drivers: Vec::new(),
            devices: Vec::new(),
            initialized: false,
        }
    }
}

static BLOCK_SUBSYSTEM: Mutex<BlockSubsystem> = Mutex::new(BlockSubsystem::new());

// ============================================================================
// Kernel API (exported to modules)
// ============================================================================

/// Register a block device driver
#[no_mangle]
pub extern "C" fn kmod_blk_register(ops: *const BlockDriverOps) -> i32 {
    if ops.is_null() {
        return -1;
    }

    let ops = unsafe { &*ops };
    let mut subsystem = BLOCK_SUBSYSTEM.lock();

    // Copy the ops table
    let driver = RegisteredDriver {
        ops: BlockDriverOps {
            name: ops.name,
            probe: ops.probe,
            new: ops.new,
            destroy: ops.destroy,
            init: ops.init,
            get_info: ops.get_info,
            read: ops.read,
            write: ops.write,
            flush: ops.flush,
        },
    };

    let name = driver_name_str(&driver.ops.name);
    crate::kinfo!("Registering block driver: {}", name);

    subsystem.drivers.push(driver);
    0
}

/// Unregister a block device driver
#[no_mangle]
pub extern "C" fn kmod_blk_unregister(name: *const u8, name_len: usize) -> i32 {
    if name.is_null() || name_len == 0 {
        return -1;
    }

    let name_bytes = unsafe { core::slice::from_raw_parts(name, name_len) };
    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");

    let mut subsystem = BLOCK_SUBSYSTEM.lock();

    // Find and remove the driver
    if let Some(pos) = subsystem
        .drivers
        .iter()
        .position(|d| driver_name_str(&d.ops.name) == name_str)
    {
        subsystem.drivers.swap_remove(pos);
        crate::kinfo!("Unregistered block driver: {}", name_str);
        0
    } else {
        -1
    }
}

/// Helper to get driver name as string
fn driver_name_str(name: &[u8; 32]) -> &str {
    let end = name.iter().position(|&c| c == 0).unwrap_or(32);
    core::str::from_utf8(&name[..end]).unwrap_or("unknown")
}

// ============================================================================
// Public Kernel Interface
// ============================================================================

/// Initialize the block device subsystem
pub fn init() {
    let mut subsystem = BLOCK_SUBSYSTEM.lock();
    if subsystem.initialized {
        return;
    }
    subsystem.initialized = true;
    drop(subsystem);

    // Register kernel symbols for module API
    use crate::kmod::symbols::{register_symbol, SymbolType};

    let mut ok_count = 0usize;

    if register_symbol(
        "kmod_blk_register",
        kmod_blk_register as *const () as u64,
        SymbolType::Function,
    ) {
        ok_count += 1;
    }

    if register_symbol(
        "kmod_blk_unregister",
        kmod_blk_unregister as *const () as u64,
        SymbolType::Function,
    ) {
        ok_count += 1;
    }

    if register_symbol(
        "kmod_blk_read_bytes",
        kmod_blk_read_bytes as *const () as u64,
        SymbolType::Function,
    ) {
        ok_count += 1;
    }

    if register_symbol(
        "kmod_blk_write_bytes",
        kmod_blk_write_bytes as *const () as u64,
        SymbolType::Function,
    ) {
        ok_count += 1;
    }

    if register_symbol(
        "kmod_blk_get_info",
        kmod_blk_get_info as *const () as u64,
        SymbolType::Function,
    ) {
        ok_count += 1;
    }

    if register_symbol(
        "kmod_blk_device_count",
        kmod_blk_device_count as *const () as u64,
        SymbolType::Function,
    ) {
        ok_count += 1;
    }

    if register_symbol(
        "kmod_blk_find_rootfs",
        kmod_blk_find_rootfs as *const () as u64,
        SymbolType::Function,
    ) {
        ok_count += 1;
    }

    crate::kinfo!(
        "Block device subsystem initialized ({}/7 symbols registered)",
        ok_count
    );
}

/// Check if any block driver is loaded
pub fn has_driver() -> bool {
    let subsystem = BLOCK_SUBSYSTEM.lock();
    !subsystem.drivers.is_empty()
}

/// Probe and initialize a block device
pub fn probe_device(desc: &BootBlockDevice) -> Result<usize, BlockError> {
    let mut subsystem = BLOCK_SUBSYSTEM.lock();

    // Check if this device is already registered (by PCI location)
    for device in subsystem.devices.iter() {
        if device.info.pci_bus == desc.pci_bus
            && device.info.pci_device == desc.pci_device
            && device.info.pci_function == desc.pci_function
        {
            // Device already probed, return existing index
            return Ok(device.index);
        }
    }

    // Find a driver that supports this device
    // For virtio-blk: vendor 0x1AF4, device 0x1001 (legacy) or 0x1042 (modern)
    let vendor_id = 0x1AF4; // Virtio vendor ID (would come from PCI enumeration)
    let device_id = 0x1001; // Virtio block device

    let driver_idx = subsystem.drivers.iter().position(|d| {
        if let Some(probe) = d.ops.probe {
            probe(vendor_id, device_id) == 0
        } else {
            false
        }
    });

    let driver_idx = driver_idx.ok_or(BlockError::DriverNotLoaded)?;

    // Create device instance
    let driver = &subsystem.drivers[driver_idx];
    let new_fn = driver.ops.new.ok_or(BlockError::InvalidOp)?;
    let init_fn = driver.ops.init.ok_or(BlockError::InvalidOp)?;
    let get_info_fn = driver.ops.get_info.ok_or(BlockError::InvalidOp)?;

    let handle = new_fn(desc as *const BootBlockDevice);
    if handle.is_null() {
        return Err(BlockError::NotReady);
    }

    // Initialize the device
    let result = init_fn(handle);
    if result != 0 {
        if let Some(destroy) = driver.ops.destroy {
            destroy(handle);
        }
        return Err(BlockError::from_code(result).unwrap_or(BlockError::IoError));
    }

    // Get device info
    let mut info = BlockDeviceInfo::empty();
    let result = get_info_fn(handle, &mut info);
    if result != 0 {
        if let Some(destroy) = driver.ops.destroy {
            destroy(handle);
        }
        return Err(BlockError::from_code(result).unwrap_or(BlockError::IoError));
    }

    // Assign device index
    let index = subsystem.devices.len();

    // Set device name if not already set
    if info.name[0] == 0 {
        let name = format_device_name(index);
        for (i, &b) in name.as_bytes().iter().enumerate() {
            if i >= 15 {
                break;
            }
            info.name[i] = b;
        }
    }

    let device = BlockDevice {
        driver_name: driver.ops.name,
        handle,
        info,
        index,
    };

    crate::kinfo!(
        "Block device initialized: {} ({} sectors, {} bytes/sector)",
        info.name_str(),
        info.total_sectors,
        info.sector_size
    );

    subsystem.devices.push(device);
    Ok(index)
}

fn format_device_name(index: usize) -> &'static str {
    match index {
        0 => "vda",
        1 => "vdb",
        2 => "vdc",
        3 => "vdd",
        _ => "vdx",
    }
}

/// Get device count
pub fn device_count() -> usize {
    BLOCK_SUBSYSTEM.lock().devices.len()
}

/// Get device info by index
pub fn get_device_info(index: usize) -> Option<BlockDeviceInfo> {
    let subsystem = BLOCK_SUBSYSTEM.lock();
    subsystem.devices.get(index).map(|d| d.info)
}

/// Read sectors from a block device
pub fn read_sectors(
    index: usize,
    sector: u64,
    count: u32,
    buf: &mut [u8],
) -> Result<(), BlockError> {
    let subsystem = BLOCK_SUBSYSTEM.lock();

    let device = subsystem.devices.get(index).ok_or(BlockError::NotFound)?;
    let driver = subsystem
        .drivers
        .iter()
        .find(|d| d.ops.name == device.driver_name)
        .ok_or(BlockError::DriverNotLoaded)?;

    let read_fn = driver.ops.read.ok_or(BlockError::InvalidOp)?;

    // Verify buffer size
    let required_size = count as usize * device.info.sector_size as usize;
    if buf.len() < required_size {
        return Err(BlockError::Alignment);
    }

    let result = read_fn(device.handle, sector, count, buf.as_mut_ptr());
    if result == 0 {
        Ok(())
    } else {
        Err(BlockError::from_code(result).unwrap_or(BlockError::IoError))
    }
}

/// Write sectors to a block device
pub fn write_sectors(index: usize, sector: u64, count: u32, buf: &[u8]) -> Result<(), BlockError> {
    let subsystem = BLOCK_SUBSYSTEM.lock();

    let device = subsystem.devices.get(index).ok_or(BlockError::NotFound)?;
    if device.info.read_only {
        return Err(BlockError::ReadOnly);
    }

    let driver = subsystem
        .drivers
        .iter()
        .find(|d| d.ops.name == device.driver_name)
        .ok_or(BlockError::DriverNotLoaded)?;

    let write_fn = driver.ops.write.ok_or(BlockError::InvalidOp)?;

    // Verify buffer size
    let required_size = count as usize * device.info.sector_size as usize;
    if buf.len() < required_size {
        return Err(BlockError::Alignment);
    }

    let result = write_fn(device.handle, sector, count, buf.as_ptr());
    if result == 0 {
        Ok(())
    } else {
        Err(BlockError::from_code(result).unwrap_or(BlockError::IoError))
    }
}

/// Read bytes at arbitrary offset (handles sector alignment)
pub fn read_bytes(index: usize, offset: u64, buf: &mut [u8]) -> Result<usize, BlockError> {
    let info = match get_device_info(index) {
        Some(i) => i,
        None => {
            return Err(BlockError::NotFound);
        }
    };
    let sector_size = info.sector_size as u64;

    if buf.is_empty() {
        return Ok(0);
    }

    let start_sector = offset / sector_size;
    let end_offset = offset + buf.len() as u64;
    let end_sector = (end_offset + sector_size - 1) / sector_size;
    let sector_count = (end_sector - start_sector) as u32;

    // Allocate temporary buffer for full sectors
    let temp_size = sector_count as usize * sector_size as usize;
    let mut temp = alloc::vec![0u8; temp_size];

    // Read full sectors
    read_sectors(index, start_sector, sector_count, &mut temp)?;

    // Copy the relevant portion
    let start_offset_in_temp = (offset % sector_size) as usize;
    let copy_len = buf.len().min(temp_size - start_offset_in_temp);
    buf[..copy_len].copy_from_slice(&temp[start_offset_in_temp..start_offset_in_temp + copy_len]);

    Ok(copy_len)
}

/// Find block device by PCI address
pub fn find_by_pci(bus: u8, device: u8, function: u8) -> Option<usize> {
    let subsystem = BLOCK_SUBSYSTEM.lock();
    subsystem
        .devices
        .iter()
        .find(|d| {
            d.info.pci_bus == bus && d.info.pci_device == device && d.info.pci_function == function
        })
        .map(|d| d.index)
}

// ============================================================================
// Module-callable API (exported to filesystem modules like ext2)
// ============================================================================

/// Read bytes from block device at arbitrary offset (callable from modules)
/// Returns number of bytes read, or negative error code
#[no_mangle]
pub extern "C" fn kmod_blk_read_bytes(
    device_index: usize,
    offset: u64,
    buf: *mut u8,
    len: usize,
) -> i64 {
    if buf.is_null() || len == 0 {
        return 0;
    }

    let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    match read_bytes(device_index, offset, buf_slice) {
        Ok(n) => n as i64,
        Err(e) => e as i32 as i64,
    }
}

/// Write bytes to block device at arbitrary offset (callable from modules)
/// Returns number of bytes written, or negative error code
#[no_mangle]
pub extern "C" fn kmod_blk_write_bytes(
    device_index: usize,
    offset: u64,
    buf: *const u8,
    len: usize,
) -> i64 {
    if buf.is_null() || len == 0 {
        return 0;
    }

    let info = match get_device_info(device_index) {
        Some(i) => i,
        None => return BlockError::NotFound as i32 as i64,
    };

    let sector_size = info.sector_size as u64;
    let buf_slice = unsafe { core::slice::from_raw_parts(buf, len) };

    // Calculate sectors to write
    let start_sector = offset / sector_size;
    let offset_in_sector = (offset % sector_size) as usize;

    // If not sector-aligned, we need read-modify-write
    if offset_in_sector != 0 || len % sector_size as usize != 0 {
        // Read-modify-write for unaligned writes
        let end_offset = offset + len as u64;
        let end_sector = (end_offset + sector_size - 1) / sector_size;
        let sector_count = (end_sector - start_sector) as u32;
        let temp_size = sector_count as usize * sector_size as usize;

        let mut temp = alloc::vec![0u8; temp_size];

        // Read existing data
        if read_sectors(device_index, start_sector, sector_count, &mut temp).is_err() {
            return BlockError::IoError as i32 as i64;
        }

        // Modify
        let start_in_temp = offset_in_sector;
        temp[start_in_temp..start_in_temp + len].copy_from_slice(buf_slice);

        // Write back
        match write_sectors(device_index, start_sector, sector_count, &temp) {
            Ok(()) => len as i64,
            Err(e) => e as i32 as i64,
        }
    } else {
        // Aligned write
        let sector_count = (len / sector_size as usize) as u32;
        match write_sectors(device_index, start_sector, sector_count, buf_slice) {
            Ok(()) => len as i64,
            Err(e) => e as i32 as i64,
        }
    }
}

/// Get block device info (callable from modules)
/// Returns 0 on success, negative error code on failure
#[no_mangle]
pub extern "C" fn kmod_blk_get_info(device_index: usize, info: *mut BlockDeviceInfo) -> i32 {
    if info.is_null() {
        return BlockError::InvalidOp as i32;
    }

    match get_device_info(device_index) {
        Some(dev_info) => {
            unsafe { *info = dev_info };
            0
        }
        None => BlockError::NotFound as i32,
    }
}

/// Get block device count (callable from modules)
#[no_mangle]
pub extern "C" fn kmod_blk_device_count() -> usize {
    device_count()
}

/// Find the first block device (for rootfs mount)
/// Returns device index or -1 if no device found
#[no_mangle]
pub extern "C" fn kmod_blk_find_rootfs() -> i32 {
    if device_count() > 0 {
        0 // First block device is typically the rootfs
    } else {
        -1
    }
}
