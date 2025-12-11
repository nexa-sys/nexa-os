//! IDE/ATA Block Device Driver Kernel Module for NexaOS
//!
//! This is a loadable kernel module (.nkm) that provides IDE/ATA block device support.
//! It supports both primary and secondary IDE channels with master/slave configurations.
//!
//! # IDE/ATA Overview
//!
//! IDE (Integrated Drive Electronics) / ATA (AT Attachment) is the traditional
//! interface for connecting hard drives and CD-ROMs to x86 PCs.
//!
//! Standard I/O ports:
//! - Primary channel: 0x1F0-0x1F7 (data), 0x3F6-0x3F7 (control)
//! - Secondary channel: 0x170-0x177 (data), 0x376-0x377 (control)
//!
//! # Supported Features
//!
//! - PIO mode data transfer (no DMA)
//! - LBA28 and LBA48 addressing
//! - Master/slave device detection
//! - ATA IDENTIFY command
//! - Read/write sectors
//! - Cache flush
//!
//! # Module Entry Points
//!
//! - `module_init`: Called when module is loaded
//! - `module_exit`: Called when module is unloaded

#![no_std]
#![allow(dead_code)]

use core::ptr;

// ============================================================================
// Module Metadata
// ============================================================================

/// Module name
pub const MODULE_NAME: &[u8] = b"ide\0";
/// Module version
pub const MODULE_VERSION: &[u8] = b"1.0.0\0";
/// Module description
pub const MODULE_DESC: &[u8] = b"IDE/ATA Block Device driver for NexaOS\0";
/// Module type (2 = Block Device)
pub const MODULE_TYPE: u8 = 2;
/// Module license
pub const MODULE_LICENSE: &[u8] = b"MIT\0";
/// Module author
pub const MODULE_AUTHOR: &[u8] = b"NexaOS Team\0";
/// Source version
pub const MODULE_SRCVERSION: &[u8] = b"in-tree\0";

// ============================================================================
// Kernel API declarations (resolved at load time from kernel symbol table)
// ============================================================================

extern "C" {
    fn kmod_log_info(msg: *const u8, len: usize);
    fn kmod_log_error(msg: *const u8, len: usize);
    fn kmod_log_warn(msg: *const u8, len: usize);
    fn kmod_log_debug(msg: *const u8, len: usize);
    fn kmod_alloc(size: usize, align: usize) -> *mut u8;
    fn kmod_zalloc(size: usize, align: usize) -> *mut u8;
    fn kmod_dealloc(ptr: *mut u8, size: usize, align: usize);
    fn kmod_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
    fn kmod_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8;
    fn kmod_spinlock_init(lock: *mut u64);
    fn kmod_spinlock_lock(lock: *mut u64);
    fn kmod_spinlock_unlock(lock: *mut u64);
    fn kmod_fence();
    fn kmod_spin_hint();
    
    // I/O port access functions
    fn kmod_inb(port: u16) -> u8;
    fn kmod_inw(port: u16) -> u16;
    fn kmod_inl(port: u16) -> u32;
    fn kmod_outb(port: u16, value: u8);
    fn kmod_outw(port: u16, value: u16);
    fn kmod_outl(port: u16, value: u32);
    
    // Block device modular API
    fn kmod_blk_register(ops: *const BlockDriverOps) -> i32;
    fn kmod_blk_unregister(name: *const u8, name_len: usize) -> i32;
}

// ============================================================================
// Logging Helpers
// ============================================================================

macro_rules! mod_info {
    ($msg:expr) => {
        unsafe { kmod_log_info($msg.as_ptr(), $msg.len()) }
    };
}

macro_rules! mod_error {
    ($msg:expr) => {
        unsafe { kmod_log_error($msg.as_ptr(), $msg.len()) }
    };
}

macro_rules! mod_warn {
    ($msg:expr) => {
        unsafe { kmod_log_warn($msg.as_ptr(), $msg.len()) }
    };
}

macro_rules! mod_debug {
    ($msg:expr) => {
        unsafe { kmod_log_debug($msg.as_ptr(), $msg.len()) }
    };
}

/// Helper to log hex values
fn log_hex(prefix: &[u8], value: u64) {
    let mut buf = [0u8; 64];
    let prefix_len = prefix.len().min(40);
    unsafe {
        core::ptr::copy_nonoverlapping(prefix.as_ptr(), buf.as_mut_ptr(), prefix_len);
    }
    buf[prefix_len] = b'0';
    buf[prefix_len + 1] = b'x';

    let hex_chars = b"0123456789abcdef";
    let mut pos = prefix_len + 2;
    let mut started = false;
    for i in (0..16).rev() {
        let nibble = ((value >> (i * 4)) & 0xF) as usize;
        if nibble != 0 || started || i == 0 {
            buf[pos] = hex_chars[nibble];
            pos += 1;
            started = true;
        }
    }
    buf[pos] = b'\n';
    pos += 1;

    unsafe { kmod_log_info(buf.as_ptr(), pos); }
}

/// Helper to log decimal values
fn log_dec(prefix: &[u8], value: u64) {
    let mut buf = [0u8; 64];
    let prefix_len = prefix.len().min(40);
    unsafe {
        core::ptr::copy_nonoverlapping(prefix.as_ptr(), buf.as_mut_ptr(), prefix_len);
    }

    let mut pos = prefix_len;
    let mut num = value;
    let mut digits = [0u8; 20];
    let mut digit_count = 0;
    
    if num == 0 {
        buf[pos] = b'0';
        pos += 1;
    } else {
        while num > 0 {
            digits[digit_count] = b'0' + (num % 10) as u8;
            digit_count += 1;
            num /= 10;
        }
        for i in (0..digit_count).rev() {
            buf[pos] = digits[i];
            pos += 1;
        }
    }
    buf[pos] = b'\n';
    pos += 1;

    unsafe { kmod_log_info(buf.as_ptr(), pos); }
}

// ============================================================================
// IDE/ATA Constants
// ============================================================================

// PCI vendor/device IDs for IDE controllers
const PCI_CLASS_STORAGE_IDE: u16 = 0x0101;
const INTEL_PIIX_IDE: u16 = 0x7010;
const INTEL_PIIX3_IDE: u16 = 0x7111;
const INTEL_ICH_IDE: u16 = 0x2820;
const INTEL_VENDOR_ID: u16 = 0x8086;

// Standard IDE I/O ports
const IDE_PRIMARY_BASE: u16 = 0x1F0;
const IDE_PRIMARY_CTRL: u16 = 0x3F6;
const IDE_SECONDARY_BASE: u16 = 0x170;
const IDE_SECONDARY_CTRL: u16 = 0x376;

// IDE register offsets (from base port)
const IDE_REG_DATA: u16 = 0;        // Data port (R/W)
const IDE_REG_ERROR: u16 = 1;       // Error register (R)
const IDE_REG_FEATURES: u16 = 1;    // Features register (W)
const IDE_REG_SECCOUNT: u16 = 2;    // Sector count (R/W)
const IDE_REG_LBA0: u16 = 3;        // LBA low byte (R/W)
const IDE_REG_LBA1: u16 = 4;        // LBA mid byte (R/W)
const IDE_REG_LBA2: u16 = 5;        // LBA high byte (R/W)
const IDE_REG_HDDEVSEL: u16 = 6;    // Drive/head select (R/W)
const IDE_REG_STATUS: u16 = 7;      // Status register (R)
const IDE_REG_COMMAND: u16 = 7;     // Command register (W)

// Control register offsets (from control port)
const IDE_CTRL_ALTSTATUS: u16 = 0;  // Alternate status (R)
const IDE_CTRL_DEVCTRL: u16 = 0;    // Device control (W)

// IDE Status register bits
const IDE_SR_BSY: u8 = 0x80;        // Busy
const IDE_SR_DRDY: u8 = 0x40;       // Device ready
const IDE_SR_DF: u8 = 0x20;         // Device fault
const IDE_SR_DSC: u8 = 0x10;        // Device seek complete
const IDE_SR_DRQ: u8 = 0x08;        // Data request
const IDE_SR_CORR: u8 = 0x04;       // Corrected data
const IDE_SR_IDX: u8 = 0x02;        // Index mark
const IDE_SR_ERR: u8 = 0x01;        // Error

// IDE Error register bits
const IDE_ER_BBK: u8 = 0x80;        // Bad block
const IDE_ER_UNC: u8 = 0x40;        // Uncorrectable data
const IDE_ER_MC: u8 = 0x20;         // Media changed
const IDE_ER_IDNF: u8 = 0x10;       // ID mark not found
const IDE_ER_MCR: u8 = 0x08;        // Media change request
const IDE_ER_ABRT: u8 = 0x04;       // Aborted command
const IDE_ER_TK0NF: u8 = 0x02;      // Track 0 not found
const IDE_ER_AMNF: u8 = 0x01;       // Address mark not found

// IDE Commands
const IDE_CMD_READ_PIO: u8 = 0x20;          // Read sectors (PIO)
const IDE_CMD_READ_PIO_EXT: u8 = 0x24;      // Read sectors EXT (LBA48)
const IDE_CMD_WRITE_PIO: u8 = 0x30;         // Write sectors (PIO)
const IDE_CMD_WRITE_PIO_EXT: u8 = 0x34;     // Write sectors EXT (LBA48)
const IDE_CMD_CACHE_FLUSH: u8 = 0xE7;       // Flush write cache
const IDE_CMD_CACHE_FLUSH_EXT: u8 = 0xEA;   // Flush write cache EXT
const IDE_CMD_IDENTIFY: u8 = 0xEC;          // Identify device
const IDE_CMD_IDENTIFY_PACKET: u8 = 0xA1;   // Identify packet device

// Device control register bits
const IDE_CTRL_NIEN: u8 = 0x02;     // Disable interrupts
const IDE_CTRL_SRST: u8 = 0x04;     // Software reset
const IDE_CTRL_HOB: u8 = 0x80;      // High order byte (for LBA48)

// Drive selection
const IDE_MASTER: u8 = 0xA0;
const IDE_SLAVE: u8 = 0xB0;
const IDE_LBA: u8 = 0x40;           // Use LBA addressing

// Timeouts (in iterations)
const IDE_TIMEOUT: u32 = 100000;
const IDE_IDENTIFY_TIMEOUT: u32 = 500000;

// Sector size
const SECTOR_SIZE: u32 = 512;

// Maximum LBA28 address
const LBA28_MAX: u64 = 0x0FFFFFFF;

// ============================================================================
// FFI Types (must match kernel's block/mod.rs)
// ============================================================================

/// Boot block device descriptor
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BootBlockDevice {
    pub pci_segment: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub sector_size: u32,
    pub total_sectors: u64,
    pub features: u64,
}

/// Block device info
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BlockDeviceInfo {
    pub name: [u8; 16],
    pub sector_size: u32,
    pub total_sectors: u64,
    pub read_only: bool,
    pub removable: bool,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
}

/// Block device handle
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct BlockDeviceHandle(pub *mut u8);

/// Function pointer types
pub type FnBlkProbe = extern "C" fn(vendor_id: u16, device_id: u16) -> i32;
pub type FnBlkNew = extern "C" fn(desc: *const BootBlockDevice) -> BlockDeviceHandle;
pub type FnBlkDestroy = extern "C" fn(handle: BlockDeviceHandle);
pub type FnBlkInit = extern "C" fn(handle: BlockDeviceHandle) -> i32;
pub type FnBlkGetInfo = extern "C" fn(handle: BlockDeviceHandle, info: *mut BlockDeviceInfo) -> i32;
pub type FnBlkRead = extern "C" fn(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *mut u8) -> i32;
pub type FnBlkWrite = extern "C" fn(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *const u8) -> i32;
pub type FnBlkFlush = extern "C" fn(handle: BlockDeviceHandle) -> i32;

/// Block driver operations table
#[repr(C)]
pub struct BlockDriverOps {
    pub name: [u8; 32],
    pub probe: Option<FnBlkProbe>,
    pub new: Option<FnBlkNew>,
    pub destroy: Option<FnBlkDestroy>,
    pub init: Option<FnBlkInit>,
    pub get_info: Option<FnBlkGetInfo>,
    pub read: Option<FnBlkRead>,
    pub write: Option<FnBlkWrite>,
    pub flush: Option<FnBlkFlush>,
}

// ============================================================================
// IDE Device Types
// ============================================================================

/// IDE channel (primary or secondary)
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IdeChannel {
    Primary = 0,
    Secondary = 1,
}

/// IDE drive position (master or slave)
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IdeDrive {
    Master = 0,
    Slave = 1,
}

/// Device type detected
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IdeDeviceType {
    None = 0,
    Ata = 1,
    Atapi = 2,
}

/// IDE channel state
#[repr(C)]
struct IdeChannelState {
    /// Base I/O port for data registers
    base: u16,
    /// Control I/O port
    ctrl: u16,
    /// Currently selected drive (0 = master, 1 = slave)
    selected: u8,
}

/// IDE device instance
#[repr(C)]
struct IdeDevice {
    /// Channel state
    channel: IdeChannelState,
    /// Device position (master/slave)
    drive: IdeDrive,
    /// Device type
    device_type: IdeDeviceType,
    /// Is this device present and working
    present: bool,
    /// Supports LBA48
    lba48: bool,
    /// Device is read-only (for ATAPI)
    read_only: bool,
    /// Total sectors (capacity)
    total_sectors: u64,
    /// Sector size
    sector_size: u32,
    /// Model name
    model: [u8; 41],
    /// Serial number
    serial: [u8; 21],
    /// Firmware revision
    firmware: [u8; 9],
    /// PCI address
    pci_bus: u8,
    pci_device: u8,
    pci_function: u8,
    /// Device index (0-3)
    index: u8,
    /// Lock for synchronization
    lock: u64,
}

// ============================================================================
// I/O Port Helpers
// ============================================================================

#[inline]
unsafe fn inb(port: u16) -> u8 {
    kmod_inb(port)
}

#[inline]
unsafe fn inw(port: u16) -> u16 {
    kmod_inw(port)
}

#[inline]
unsafe fn outb(port: u16, value: u8) {
    kmod_outb(port, value);
}

#[inline]
unsafe fn outw(port: u16, value: u16) {
    kmod_outw(port, value);
}

/// 400ns delay by reading alternate status 4 times
#[inline]
unsafe fn ide_400ns_delay(ctrl: u16) {
    for _ in 0..4 {
        let _ = inb(ctrl);
    }
}

// ============================================================================
// IDE Low-Level Operations
// ============================================================================

/// Wait for BSY to clear
unsafe fn ide_wait_not_busy(base: u16, timeout: u32) -> bool {
    for _ in 0..timeout {
        let status = inb(base + IDE_REG_STATUS);
        if (status & IDE_SR_BSY) == 0 {
            return true;
        }
        kmod_spin_hint();
    }
    false
}

/// Wait for DRQ to be set (data ready)
unsafe fn ide_wait_drq(base: u16, timeout: u32) -> Result<(), i32> {
    for _ in 0..timeout {
        let status = inb(base + IDE_REG_STATUS);
        if (status & IDE_SR_BSY) == 0 {
            if (status & IDE_SR_ERR) != 0 {
                return Err(-2); // Error occurred
            }
            if (status & IDE_SR_DF) != 0 {
                return Err(-3); // Device fault
            }
            if (status & IDE_SR_DRQ) != 0 {
                return Ok(()); // Data ready
            }
        }
        kmod_spin_hint();
    }
    Err(-1) // Timeout
}

/// Wait for drive ready
unsafe fn ide_wait_ready(base: u16, timeout: u32) -> Result<(), i32> {
    for _ in 0..timeout {
        let status = inb(base + IDE_REG_STATUS);
        if (status & IDE_SR_BSY) == 0 {
            if (status & IDE_SR_ERR) != 0 {
                return Err(-2);
            }
            if (status & IDE_SR_DF) != 0 {
                return Err(-3);
            }
            if (status & IDE_SR_DRDY) != 0 {
                return Ok(());
            }
        }
        kmod_spin_hint();
    }
    Err(-1)
}

/// Select drive on the channel
unsafe fn ide_select_drive(channel: &mut IdeChannelState, drive: IdeDrive) {
    let drive_bit = match drive {
        IdeDrive::Master => IDE_MASTER,
        IdeDrive::Slave => IDE_SLAVE,
    };
    
    if channel.selected != drive as u8 {
        outb(channel.base + IDE_REG_HDDEVSEL, drive_bit);
        ide_400ns_delay(channel.ctrl);
        channel.selected = drive as u8;
    }
}

/// Perform software reset on the channel
unsafe fn ide_reset_channel(channel: &IdeChannelState) {
    // Set SRST bit
    outb(channel.ctrl, IDE_CTRL_SRST | IDE_CTRL_NIEN);
    ide_400ns_delay(channel.ctrl);
    
    // Clear SRST bit, keep interrupts disabled
    outb(channel.ctrl, IDE_CTRL_NIEN);
    ide_400ns_delay(channel.ctrl);
    
    // Wait for reset to complete
    ide_wait_not_busy(channel.base, IDE_TIMEOUT);
}

/// Check if a device is present on the channel
unsafe fn ide_detect_device(channel: &mut IdeChannelState, drive: IdeDrive) -> IdeDeviceType {
    ide_select_drive(channel, drive);
    
    // Write signature bytes
    outb(channel.base + IDE_REG_SECCOUNT, 0);
    outb(channel.base + IDE_REG_LBA0, 0);
    outb(channel.base + IDE_REG_LBA1, 0);
    outb(channel.base + IDE_REG_LBA2, 0);
    
    // Send IDENTIFY command
    outb(channel.base + IDE_REG_COMMAND, IDE_CMD_IDENTIFY);
    ide_400ns_delay(channel.ctrl);
    
    // Check if device exists
    let status = inb(channel.base + IDE_REG_STATUS);
    if status == 0 || status == 0xFF {
        return IdeDeviceType::None;
    }
    
    // Wait for BSY to clear
    if !ide_wait_not_busy(channel.base, IDE_IDENTIFY_TIMEOUT) {
        return IdeDeviceType::None;
    }
    
    // Read signature bytes to determine device type
    let lba1 = inb(channel.base + IDE_REG_LBA1);
    let lba2 = inb(channel.base + IDE_REG_LBA2);
    
    // Check device type signature
    if lba1 == 0x00 && lba2 == 0x00 {
        // ATA device
        IdeDeviceType::Ata
    } else if lba1 == 0x14 && lba2 == 0xEB {
        // ATAPI device
        IdeDeviceType::Atapi
    } else if lba1 == 0x69 && lba2 == 0x96 {
        // ATAPI device (alternate signature)
        IdeDeviceType::Atapi
    } else {
        IdeDeviceType::None
    }
}

/// Read IDENTIFY data from device
unsafe fn ide_identify(device: &mut IdeDevice) -> i32 {
    ide_select_drive(&mut device.channel, device.drive);
    
    // Determine command based on device type
    let cmd = match device.device_type {
        IdeDeviceType::Ata => IDE_CMD_IDENTIFY,
        IdeDeviceType::Atapi => IDE_CMD_IDENTIFY_PACKET,
        IdeDeviceType::None => return -1,
    };
    
    // Send command
    outb(device.channel.base + IDE_REG_COMMAND, cmd);
    ide_400ns_delay(device.channel.ctrl);
    
    // Wait for DRQ
    if let Err(e) = ide_wait_drq(device.channel.base, IDE_IDENTIFY_TIMEOUT) {
        return e;
    }
    
    // Read 256 words (512 bytes) of identify data
    let mut identify_buf = [0u16; 256];
    for i in 0..256 {
        identify_buf[i] = inw(device.channel.base + IDE_REG_DATA);
    }
    
    // Parse identify data
    // Word 0: General configuration
    let config = identify_buf[0];
    
    // Words 27-46: Model number (40 chars, swapped bytes)
    for i in 0..20 {
        let word = identify_buf[27 + i];
        device.model[i * 2] = (word >> 8) as u8;
        device.model[i * 2 + 1] = (word & 0xFF) as u8;
    }
    device.model[40] = 0;
    trim_string(&mut device.model);
    
    // Words 10-19: Serial number (20 chars, swapped bytes)
    for i in 0..10 {
        let word = identify_buf[10 + i];
        device.serial[i * 2] = (word >> 8) as u8;
        device.serial[i * 2 + 1] = (word & 0xFF) as u8;
    }
    device.serial[20] = 0;
    trim_string(&mut device.serial);
    
    // Words 23-26: Firmware revision (8 chars, swapped bytes)
    for i in 0..4 {
        let word = identify_buf[23 + i];
        device.firmware[i * 2] = (word >> 8) as u8;
        device.firmware[i * 2 + 1] = (word & 0xFF) as u8;
    }
    device.firmware[8] = 0;
    trim_string(&mut device.firmware);
    
    // Word 49: Capabilities
    let caps = identify_buf[49];
    let lba_supported = (caps & (1 << 9)) != 0;
    
    // Word 83: Command set supported (LBA48)
    let cmd_set_2 = identify_buf[83];
    device.lba48 = (cmd_set_2 & (1 << 10)) != 0;
    
    // Get capacity
    if device.lba48 {
        // Words 100-103: LBA48 sector count
        device.total_sectors = 
            (identify_buf[100] as u64) |
            ((identify_buf[101] as u64) << 16) |
            ((identify_buf[102] as u64) << 32) |
            ((identify_buf[103] as u64) << 48);
    } else if lba_supported {
        // Words 60-61: LBA28 sector count
        device.total_sectors = 
            (identify_buf[60] as u64) |
            ((identify_buf[61] as u64) << 16);
    } else {
        // CHS mode (legacy, use words 1, 3, 6)
        let cylinders = identify_buf[1] as u64;
        let heads = identify_buf[3] as u64;
        let sectors = identify_buf[6] as u64;
        device.total_sectors = cylinders * heads * sectors;
    }
    
    // Word 106: Physical/Logical sector size
    let sector_info = identify_buf[106];
    if (sector_info & (1 << 14)) != 0 && (sector_info & (1 << 15)) == 0 {
        // Large logical sectors are supported
        if (sector_info & (1 << 12)) != 0 {
            // Words 117-118: Logical sector size
            let logical_size = 
                (identify_buf[117] as u32) |
                ((identify_buf[118] as u32) << 16);
            device.sector_size = logical_size * 2;
        } else {
            device.sector_size = SECTOR_SIZE;
        }
    } else {
        device.sector_size = SECTOR_SIZE;
    }
    
    // ATAPI devices are typically read-only
    device.read_only = device.device_type == IdeDeviceType::Atapi;
    
    0
}

/// Trim trailing spaces from a string
fn trim_string(s: &mut [u8]) {
    let mut end = s.len();
    while end > 0 && (s[end - 1] == 0 || s[end - 1] == b' ') {
        end -= 1;
    }
    for i in end..s.len() {
        s[i] = 0;
    }
}

/// Read sectors using PIO mode (LBA28 or LBA48)
unsafe fn ide_read_sectors(
    device: &mut IdeDevice,
    lba: u64,
    count: u32,
    buf: *mut u8,
) -> i32 {
    if count == 0 || count > 256 {
        return -1;
    }
    
    // Select drive with LBA bit
    ide_select_drive(&mut device.channel, device.drive);
    
    let use_lba48 = device.lba48 && (lba > LBA28_MAX || count > 255);
    let base = device.channel.base;
    
    if use_lba48 {
        // LBA48 mode
        // Write high bytes first
        outb(base + IDE_REG_SECCOUNT, ((count >> 8) & 0xFF) as u8);
        outb(base + IDE_REG_LBA0, ((lba >> 24) & 0xFF) as u8);
        outb(base + IDE_REG_LBA1, ((lba >> 32) & 0xFF) as u8);
        outb(base + IDE_REG_LBA2, ((lba >> 40) & 0xFF) as u8);
        
        // Write low bytes
        outb(base + IDE_REG_SECCOUNT, (count & 0xFF) as u8);
        outb(base + IDE_REG_LBA0, (lba & 0xFF) as u8);
        outb(base + IDE_REG_LBA1, ((lba >> 8) & 0xFF) as u8);
        outb(base + IDE_REG_LBA2, ((lba >> 16) & 0xFF) as u8);
        
        // Drive select with LBA bit (no head bits for LBA48)
        let drive_sel = match device.drive {
            IdeDrive::Master => IDE_MASTER | IDE_LBA,
            IdeDrive::Slave => IDE_SLAVE | IDE_LBA,
        };
        outb(base + IDE_REG_HDDEVSEL, drive_sel);
        
        // Send command
        outb(base + IDE_REG_COMMAND, IDE_CMD_READ_PIO_EXT);
    } else {
        // LBA28 mode
        let count8 = if count == 256 { 0 } else { count as u8 };
        
        outb(base + IDE_REG_SECCOUNT, count8);
        outb(base + IDE_REG_LBA0, (lba & 0xFF) as u8);
        outb(base + IDE_REG_LBA1, ((lba >> 8) & 0xFF) as u8);
        outb(base + IDE_REG_LBA2, ((lba >> 16) & 0xFF) as u8);
        
        // Drive select with LBA bits 24-27
        let drive_sel = match device.drive {
            IdeDrive::Master => IDE_MASTER | IDE_LBA | ((lba >> 24) & 0x0F) as u8,
            IdeDrive::Slave => IDE_SLAVE | IDE_LBA | ((lba >> 24) & 0x0F) as u8,
        };
        outb(base + IDE_REG_HDDEVSEL, drive_sel);
        
        // Send command
        outb(base + IDE_REG_COMMAND, IDE_CMD_READ_PIO);
    }
    
    // Read sectors
    let sector_words = (device.sector_size / 2) as usize;
    let mut offset = 0usize;
    
    for _ in 0..count {
        // Wait for data
        if let Err(e) = ide_wait_drq(base, IDE_TIMEOUT) {
            return e;
        }
        
        // Read sector data (word at a time)
        for _ in 0..sector_words {
            let word = inw(base + IDE_REG_DATA);
            *buf.add(offset) = (word & 0xFF) as u8;
            *buf.add(offset + 1) = (word >> 8) as u8;
            offset += 2;
        }
    }
    
    0
}

/// Write sectors using PIO mode (LBA28 or LBA48)
unsafe fn ide_write_sectors(
    device: &mut IdeDevice,
    lba: u64,
    count: u32,
    buf: *const u8,
) -> i32 {
    if count == 0 || count > 256 {
        return -1;
    }
    
    if device.read_only {
        return -4; // Read-only device
    }
    
    // Select drive with LBA bit
    ide_select_drive(&mut device.channel, device.drive);
    
    let use_lba48 = device.lba48 && (lba > LBA28_MAX || count > 255);
    let base = device.channel.base;
    
    if use_lba48 {
        // LBA48 mode
        outb(base + IDE_REG_SECCOUNT, ((count >> 8) & 0xFF) as u8);
        outb(base + IDE_REG_LBA0, ((lba >> 24) & 0xFF) as u8);
        outb(base + IDE_REG_LBA1, ((lba >> 32) & 0xFF) as u8);
        outb(base + IDE_REG_LBA2, ((lba >> 40) & 0xFF) as u8);
        
        outb(base + IDE_REG_SECCOUNT, (count & 0xFF) as u8);
        outb(base + IDE_REG_LBA0, (lba & 0xFF) as u8);
        outb(base + IDE_REG_LBA1, ((lba >> 8) & 0xFF) as u8);
        outb(base + IDE_REG_LBA2, ((lba >> 16) & 0xFF) as u8);
        
        let drive_sel = match device.drive {
            IdeDrive::Master => IDE_MASTER | IDE_LBA,
            IdeDrive::Slave => IDE_SLAVE | IDE_LBA,
        };
        outb(base + IDE_REG_HDDEVSEL, drive_sel);
        
        outb(base + IDE_REG_COMMAND, IDE_CMD_WRITE_PIO_EXT);
    } else {
        // LBA28 mode
        let count8 = if count == 256 { 0 } else { count as u8 };
        
        outb(base + IDE_REG_SECCOUNT, count8);
        outb(base + IDE_REG_LBA0, (lba & 0xFF) as u8);
        outb(base + IDE_REG_LBA1, ((lba >> 8) & 0xFF) as u8);
        outb(base + IDE_REG_LBA2, ((lba >> 16) & 0xFF) as u8);
        
        let drive_sel = match device.drive {
            IdeDrive::Master => IDE_MASTER | IDE_LBA | ((lba >> 24) & 0x0F) as u8,
            IdeDrive::Slave => IDE_SLAVE | IDE_LBA | ((lba >> 24) & 0x0F) as u8,
        };
        outb(base + IDE_REG_HDDEVSEL, drive_sel);
        
        outb(base + IDE_REG_COMMAND, IDE_CMD_WRITE_PIO);
    }
    
    // Write sectors
    let sector_words = (device.sector_size / 2) as usize;
    let mut offset = 0usize;
    
    for _ in 0..count {
        // Wait for ready to accept data
        if let Err(e) = ide_wait_drq(base, IDE_TIMEOUT) {
            return e;
        }
        
        // Write sector data (word at a time)
        for _ in 0..sector_words {
            let word = (*buf.add(offset) as u16) | ((*buf.add(offset + 1) as u16) << 8);
            outw(base + IDE_REG_DATA, word);
            offset += 2;
        }
    }
    
    // Wait for write to complete
    if let Err(e) = ide_wait_ready(base, IDE_TIMEOUT) {
        return e;
    }
    
    0
}

/// Flush cache
unsafe fn ide_flush_cache(device: &mut IdeDevice) -> i32 {
    ide_select_drive(&mut device.channel, device.drive);
    
    let cmd = if device.lba48 {
        IDE_CMD_CACHE_FLUSH_EXT
    } else {
        IDE_CMD_CACHE_FLUSH
    };
    
    outb(device.channel.base + IDE_REG_COMMAND, cmd);
    
    // Wait for completion
    if let Err(e) = ide_wait_ready(device.channel.base, IDE_TIMEOUT * 10) {
        return e;
    }
    
    0
}

// ============================================================================
// Driver Implementation
// ============================================================================

/// Check if we support this PCI device
extern "C" fn ide_probe(vendor_id: u16, device_id: u16) -> i32 {
    // Check for common IDE controller IDs
    if vendor_id == INTEL_VENDOR_ID {
        match device_id {
            INTEL_PIIX_IDE | INTEL_PIIX3_IDE | INTEL_ICH_IDE => return 0,
            _ => {}
        }
    }
    
    // Also check for standard IDE devices using legacy ports
    // Device ID 0x0101 is typically PCI class code for IDE
    // We accept vendor_id 0 for legacy ISA-style detection
    if vendor_id == 0 && device_id == 0 {
        return 0; // Allow legacy detection
    }
    
    -1 // Not supported
}

/// Create a new IDE device instance
extern "C" fn ide_new(desc: *const BootBlockDevice) -> BlockDeviceHandle {
    if desc.is_null() {
        return BlockDeviceHandle(ptr::null_mut());
    }

    let desc = unsafe { &*desc };
    mod_info!(b"ide: Creating new device instance\n");
    
    // Allocate device structure
    let device = unsafe {
        let ptr = kmod_zalloc(
            core::mem::size_of::<IdeDevice>(),
            core::mem::align_of::<IdeDevice>(),
        ) as *mut IdeDevice;
        if ptr.is_null() {
            mod_error!(b"ide: Failed to allocate device structure\n");
            return BlockDeviceHandle(ptr::null_mut());
        }
        &mut *ptr
    };
    
    // Determine channel and drive from features or mmio_base
    // features bits:
    //   bit 0-1: drive index (0-3 for primary master, primary slave, secondary master, secondary slave)
    let drive_index = (desc.features & 0x03) as u8;
    
    device.index = drive_index;
    
    match drive_index {
        0 => {
            device.channel.base = IDE_PRIMARY_BASE;
            device.channel.ctrl = IDE_PRIMARY_CTRL;
            device.drive = IdeDrive::Master;
        }
        1 => {
            device.channel.base = IDE_PRIMARY_BASE;
            device.channel.ctrl = IDE_PRIMARY_CTRL;
            device.drive = IdeDrive::Slave;
        }
        2 => {
            device.channel.base = IDE_SECONDARY_BASE;
            device.channel.ctrl = IDE_SECONDARY_CTRL;
            device.drive = IdeDrive::Master;
        }
        3 => {
            device.channel.base = IDE_SECONDARY_BASE;
            device.channel.ctrl = IDE_SECONDARY_CTRL;
            device.drive = IdeDrive::Slave;
        }
        _ => {
            mod_error!(b"ide: Invalid drive index\n");
            unsafe {
                kmod_dealloc(
                    device as *mut IdeDevice as *mut u8,
                    core::mem::size_of::<IdeDevice>(),
                    core::mem::align_of::<IdeDevice>(),
                );
            }
            return BlockDeviceHandle(ptr::null_mut());
        }
    }
    
    device.channel.selected = 0xFF; // Force selection on first access
    device.pci_bus = desc.pci_bus;
    device.pci_device = desc.pci_device;
    device.pci_function = desc.pci_function;
    device.sector_size = SECTOR_SIZE;
    
    unsafe { kmod_spinlock_init(&mut device.lock) };
    
    log_hex(b"ide: Base port: ", device.channel.base as u64);
    log_hex(b"ide: Ctrl port: ", device.channel.ctrl as u64);
    
    BlockDeviceHandle(device as *mut IdeDevice as *mut u8)
}

/// Destroy an IDE device instance
extern "C" fn ide_destroy(handle: BlockDeviceHandle) {
    if handle.0.is_null() {
        return;
    }

    let device = handle.0 as *mut IdeDevice;
    unsafe {
        kmod_dealloc(
            device as *mut u8,
            core::mem::size_of::<IdeDevice>(),
            core::mem::align_of::<IdeDevice>(),
        );
    }

    mod_info!(b"ide: Device destroyed\n");
}

/// Initialize the IDE device
extern "C" fn ide_init(handle: BlockDeviceHandle) -> i32 {
    if handle.0.is_null() {
        return -1;
    }

    let device = unsafe { &mut *(handle.0 as *mut IdeDevice) };

    mod_info!(b"ide: Initializing device...\n");
    
    unsafe {
        // Disable interrupts on the channel
        outb(device.channel.ctrl, IDE_CTRL_NIEN);
        
        // Reset the channel
        ide_reset_channel(&device.channel);
        
        // Detect device type
        device.device_type = ide_detect_device(&mut device.channel, device.drive);
        
        match device.device_type {
            IdeDeviceType::None => {
                mod_warn!(b"ide: No device detected\n");
                return -2;
            }
            IdeDeviceType::Ata => {
                mod_info!(b"ide: ATA device detected\n");
            }
            IdeDeviceType::Atapi => {
                mod_info!(b"ide: ATAPI device detected\n");
            }
        }
        
        // Read IDENTIFY data
        if ide_identify(device) != 0 {
            mod_error!(b"ide: Failed to identify device\n");
            return -3;
        }
        
        device.present = true;
        
        // Log device info
        mod_info!(b"ide: Device initialized:\n");
        
        // Log model (simplified - just print if non-empty)
        if device.model[0] != 0 {
            let mut msg = [0u8; 64];
            let prefix = b"  Model: ";
            msg[..prefix.len()].copy_from_slice(prefix);
            let mut pos = prefix.len();
            for &c in &device.model {
                if c == 0 { break; }
                msg[pos] = c;
                pos += 1;
            }
            msg[pos] = b'\n';
            pos += 1;
            kmod_log_info(msg.as_ptr(), pos);
        }
        
        log_dec(b"  Sectors: ", device.total_sectors);
        log_hex(b"  Sector size: ", device.sector_size as u64);
        
        let capacity_mb = (device.total_sectors * device.sector_size as u64) / (1024 * 1024);
        log_dec(b"  Capacity (MB): ", capacity_mb);
        
        if device.lba48 {
            mod_info!(b"  LBA48 supported\n");
        } else {
            mod_info!(b"  LBA28 mode\n");
        }
    }
    
    mod_info!(b"ide: Device initialized successfully\n");
    0
}

/// Get device information
extern "C" fn ide_get_info(handle: BlockDeviceHandle, info: *mut BlockDeviceInfo) -> i32 {
    if handle.0.is_null() || info.is_null() {
        return -1;
    }

    let device = unsafe { &*(handle.0 as *mut IdeDevice) };
    let info = unsafe { &mut *info };

    // Generate device name based on index (hda, hdb, hdc, hdd)
    let names = [b"hda\0", b"hdb\0", b"hdc\0", b"hdd\0"];
    let name = names[device.index as usize % 4];
    info.name[..name.len()].copy_from_slice(name);

    info.sector_size = device.sector_size;
    info.total_sectors = device.total_sectors;
    info.read_only = device.read_only;
    info.removable = device.device_type == IdeDeviceType::Atapi;
    info.pci_bus = device.pci_bus;
    info.pci_device = device.pci_device;
    info.pci_function = device.pci_function;

    0
}

/// Read sectors from the device
extern "C" fn ide_read(
    handle: BlockDeviceHandle,
    sector: u64,
    count: u32,
    buf: *mut u8,
) -> i32 {
    if handle.0.is_null() || buf.is_null() || count == 0 {
        return -1;
    }

    let device = unsafe { &mut *(handle.0 as *mut IdeDevice) };

    // Check bounds
    if sector + count as u64 > device.total_sectors {
        mod_error!(b"ide: Read beyond device capacity\n");
        return -2;
    }
    
    if !device.present {
        mod_error!(b"ide: Device not present\n");
        return -3;
    }

    unsafe {
        kmod_spinlock_lock(&mut device.lock);
    }

    // Read in chunks of 256 sectors max
    let mut remaining = count;
    let mut current_sector = sector;
    let mut buf_offset = 0usize;
    let mut result = 0i32;

    while remaining > 0 {
        let chunk = remaining.min(256);
        
        result = unsafe {
            ide_read_sectors(
                device,
                current_sector,
                chunk,
                buf.add(buf_offset),
            )
        };
        
        if result != 0 {
            break;
        }
        
        remaining -= chunk;
        current_sector += chunk as u64;
        buf_offset += (chunk * device.sector_size) as usize;
    }

    unsafe {
        kmod_spinlock_unlock(&mut device.lock);
    }

    result
}

/// Write sectors to the device
extern "C" fn ide_write(
    handle: BlockDeviceHandle,
    sector: u64,
    count: u32,
    buf: *const u8,
) -> i32 {
    if handle.0.is_null() || buf.is_null() || count == 0 {
        return -1;
    }

    let device = unsafe { &mut *(handle.0 as *mut IdeDevice) };

    if device.read_only {
        mod_error!(b"ide: Device is read-only\n");
        return -4;
    }

    // Check bounds
    if sector + count as u64 > device.total_sectors {
        mod_error!(b"ide: Write beyond device capacity\n");
        return -2;
    }
    
    if !device.present {
        mod_error!(b"ide: Device not present\n");
        return -3;
    }

    unsafe {
        kmod_spinlock_lock(&mut device.lock);
    }

    // Write in chunks of 256 sectors max
    let mut remaining = count;
    let mut current_sector = sector;
    let mut buf_offset = 0usize;
    let mut result = 0i32;

    while remaining > 0 {
        let chunk = remaining.min(256);
        
        result = unsafe {
            ide_write_sectors(
                device,
                current_sector,
                chunk,
                buf.add(buf_offset),
            )
        };
        
        if result != 0 {
            break;
        }
        
        remaining -= chunk;
        current_sector += chunk as u64;
        buf_offset += (chunk * device.sector_size) as usize;
    }

    unsafe {
        kmod_spinlock_unlock(&mut device.lock);
    }

    result
}

/// Flush write cache
extern "C" fn ide_flush(handle: BlockDeviceHandle) -> i32 {
    if handle.0.is_null() {
        return -1;
    }

    let device = unsafe { &mut *(handle.0 as *mut IdeDevice) };
    
    if !device.present {
        return -3;
    }

    unsafe {
        kmod_spinlock_lock(&mut device.lock);
    }

    let result = unsafe { ide_flush_cache(device) };

    unsafe {
        kmod_spinlock_unlock(&mut device.lock);
    }

    result
}

// ============================================================================
// Module Entry Points
// ============================================================================

/// Module entry point table
#[used]
#[no_mangle]
pub static MODULE_ENTRY_POINTS: [unsafe extern "C" fn() -> i32; 2] = [
    module_init_wrapper,
    module_exit_wrapper,
];

#[no_mangle]
unsafe extern "C" fn module_init_wrapper() -> i32 {
    module_init()
}

#[no_mangle]
unsafe extern "C" fn module_exit_wrapper() -> i32 {
    module_exit()
}

/// Module initialization
#[no_mangle]
#[inline(never)]
pub extern "C" fn module_init() -> i32 {
    mod_info!(b"ide module: initializing...\n");

    // Create driver name
    let mut name = [0u8; 32];
    let name_bytes = b"ide";
    name[..name_bytes.len()].copy_from_slice(name_bytes);

    // Create operations table
    let ops = BlockDriverOps {
        name,
        probe: Some(ide_probe),
        new: Some(ide_new),
        destroy: Some(ide_destroy),
        init: Some(ide_init),
        get_info: Some(ide_get_info),
        read: Some(ide_read),
        write: Some(ide_write),
        flush: Some(ide_flush),
    };

    // Register with the kernel
    let result = unsafe { kmod_blk_register(&ops) };

    if result != 0 {
        mod_error!(b"ide module: failed to register with kernel\n");
        return -1;
    }

    mod_info!(b"ide module: initialized successfully\n");
    0
}

/// Module cleanup
#[no_mangle]
#[inline(never)]
pub extern "C" fn module_exit() -> i32 {
    mod_info!(b"ide module: unloading...\n");

    let name = b"ide";
    unsafe {
        kmod_blk_unregister(name.as_ptr(), name.len());
    }

    mod_info!(b"ide module: unloaded\n");
    0
}

// ============================================================================
// Panic Handler (required for no_std)
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    mod_error!(b"ide: PANIC!\n");
    loop {
        core::hint::spin_loop();
    }
}
