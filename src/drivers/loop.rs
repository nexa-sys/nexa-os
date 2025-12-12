//! Loop Device Driver
//!
//! This module implements loop devices (/dev/loop*) which allow regular files
//! to be mounted as block devices. This is useful for mounting disk images,
//! ISO files, and similar use cases.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │  mount(loop0)   │────▶│  Loop Device     │────▶│  Backing File   │
//! │  Filesystem     │     │  /dev/loop0      │     │  (disk.img)     │
//! └─────────────────┘     └──────────────────┘     └─────────────────┘
//! ```
//!
//! # Supported ioctl commands
//!
//! - `LOOP_SET_FD`: Attach a file descriptor to the loop device
//! - `LOOP_CLR_FD`: Detach the file from the loop device
//! - `LOOP_GET_STATUS`: Get loop device status
//! - `LOOP_SET_STATUS`: Set loop device parameters
//! - `LOOP_CTL_GET_FREE`: Get the first free loop device number

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Maximum number of loop devices
pub const MAX_LOOP_DEVICES: usize = 8;

/// Loop device ioctl commands (Linux compatible values)
pub const LOOP_SET_FD: u64 = 0x4C00;
pub const LOOP_CLR_FD: u64 = 0x4C01;
pub const LOOP_SET_STATUS: u64 = 0x4C02;
pub const LOOP_GET_STATUS: u64 = 0x4C03;
pub const LOOP_SET_STATUS64: u64 = 0x4C04;
pub const LOOP_GET_STATUS64: u64 = 0x4C05;
pub const LOOP_CHANGE_FD: u64 = 0x4C06;
pub const LOOP_SET_CAPACITY: u64 = 0x4C07;
pub const LOOP_SET_DIRECT_IO: u64 = 0x4C08;
pub const LOOP_SET_BLOCK_SIZE: u64 = 0x4C09;
pub const LOOP_CTL_ADD: u64 = 0x4C80;
pub const LOOP_CTL_REMOVE: u64 = 0x4C81;
pub const LOOP_CTL_GET_FREE: u64 = 0x4C82;

/// Loop device flags
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopFlags {
    /// Device is read-only
    ReadOnly = 1,
    /// Automatically clear on last close
    Autoclear = 4,
    /// Partition scan
    PartScan = 8,
    /// Direct I/O mode
    DirectIO = 16,
}

/// Loop device status (64-bit version, compatible with Linux struct loop_info64)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LoopInfo64 {
    /// Loop device number
    pub lo_device: u64,
    /// inode of backing file
    pub lo_inode: u64,
    /// rdev of loop device
    pub lo_rdev: u64,
    /// Offset into backing file
    pub lo_offset: u64,
    /// Size limit (0 = no limit)
    pub lo_sizelimit: u64,
    /// Loop device number (ino of /dev/loopN)
    pub lo_number: u32,
    /// Encryption type (deprecated)
    pub lo_encrypt_type: u32,
    /// Encryption key size (deprecated)
    pub lo_encrypt_key_size: u32,
    /// Loop device flags
    pub lo_flags: u32,
    /// Name of backing file
    pub lo_file_name: [u8; 64],
    /// Crypt name (deprecated)
    pub lo_crypt_name: [u8; 64],
    /// Encryption key (deprecated)
    pub lo_encrypt_key: [u8; 32],
    /// Initial vector offset (deprecated)
    pub lo_init: [u64; 2],
}

impl LoopInfo64 {
    pub const fn zeroed() -> Self {
        Self {
            lo_device: 0,
            lo_inode: 0,
            lo_rdev: 0,
            lo_offset: 0,
            lo_sizelimit: 0,
            lo_number: 0,
            lo_encrypt_type: 0,
            lo_encrypt_key_size: 0,
            lo_flags: 0,
            lo_file_name: [0; 64],
            lo_crypt_name: [0; 64],
            lo_encrypt_key: [0; 32],
            lo_init: [0; 2],
        }
    }
}

/// Loop device state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    /// Device is not attached to any file
    Free,
    /// Device is attached to a file and ready
    Attached,
}

/// A loop device instance
pub struct LoopDevice {
    /// Device index (0-7 for loop0-loop7)
    pub index: usize,
    /// Current state
    pub state: LoopState,
    /// Backing file descriptor (if attached)
    pub backing_fd: Option<u64>,
    /// Backing file path (if attached)
    pub backing_path: Option<String>,
    /// Offset into backing file
    pub offset: u64,
    /// Size limit (0 = no limit)
    pub sizelimit: u64,
    /// Block size (default 512)
    pub block_size: u32,
    /// Flags
    pub flags: u32,
    /// Total size of the backing file in bytes
    pub total_size: u64,
}

impl LoopDevice {
    pub const fn new(index: usize) -> Self {
        Self {
            index,
            state: LoopState::Free,
            backing_fd: None,
            backing_path: None,
            offset: 0,
            sizelimit: 0,
            block_size: 512,
            flags: 0,
            total_size: 0,
        }
    }

    /// Check if this device is free
    pub fn is_free(&self) -> bool {
        self.state == LoopState::Free
    }

    /// Check if this device is attached
    pub fn is_attached(&self) -> bool {
        self.state == LoopState::Attached
    }

    /// Check if this device is read-only
    pub fn is_readonly(&self) -> bool {
        self.flags & LoopFlags::ReadOnly as u32 != 0
    }

    /// Get the effective size (sizelimit or total_size - offset)
    pub fn effective_size(&self) -> u64 {
        if self.sizelimit > 0 {
            self.sizelimit
        } else if self.total_size > self.offset {
            self.total_size - self.offset
        } else {
            0
        }
    }

    /// Get total sectors
    pub fn total_sectors(&self) -> u64 {
        self.effective_size() / self.block_size as u64
    }

    /// Convert a loop device sector to a byte offset in the backing file
    pub fn sector_to_offset(&self, sector: u64) -> u64 {
        self.offset + sector * self.block_size as u64
    }

    /// Get device info as LoopInfo64
    pub fn get_info64(&self) -> LoopInfo64 {
        let mut info = LoopInfo64::zeroed();
        info.lo_number = self.index as u32;
        info.lo_offset = self.offset;
        info.lo_sizelimit = self.sizelimit;
        info.lo_flags = self.flags;

        // Copy backing file name if available
        if let Some(ref path) = self.backing_path {
            let bytes = path.as_bytes();
            let copy_len = bytes.len().min(63);
            info.lo_file_name[..copy_len].copy_from_slice(&bytes[..copy_len]);
        }

        info
    }

    /// Clear/detach the device
    pub fn clear(&mut self) {
        self.state = LoopState::Free;
        self.backing_fd = None;
        self.backing_path = None;
        self.offset = 0;
        self.sizelimit = 0;
        self.total_size = 0;
        self.flags = 0;
    }
}

/// Global loop device table
static LOOP_DEVICES: Mutex<[LoopDevice; MAX_LOOP_DEVICES]> = Mutex::new([
    LoopDevice::new(0),
    LoopDevice::new(1),
    LoopDevice::new(2),
    LoopDevice::new(3),
    LoopDevice::new(4),
    LoopDevice::new(5),
    LoopDevice::new(6),
    LoopDevice::new(7),
]);

static INITIALIZED: spin::Once<()> = spin::Once::new();

/// Initialize the loop device subsystem
pub fn init() {
    INITIALIZED.call_once(|| {
        crate::kinfo!("Loop device subsystem initialized ({} devices)", MAX_LOOP_DEVICES);
    });
}

/// Get a free loop device index
pub fn get_free() -> Option<usize> {
    let devices = LOOP_DEVICES.lock();
    for (i, dev) in devices.iter().enumerate() {
        if dev.is_free() {
            return Some(i);
        }
    }
    None
}

/// Check if a loop device exists
pub fn exists(index: usize) -> bool {
    index < MAX_LOOP_DEVICES
}

/// Check if a loop device is attached
pub fn is_attached(index: usize) -> bool {
    if index >= MAX_LOOP_DEVICES {
        return false;
    }
    let devices = LOOP_DEVICES.lock();
    devices[index].is_attached()
}

/// Attach a file to a loop device
///
/// # Arguments
/// * `index` - Loop device index (0-7)
/// * `fd` - File descriptor of backing file
/// * `path` - Path to backing file (for display purposes)
/// * `file_size` - Size of the backing file in bytes
/// * `readonly` - Whether to mount read-only
///
/// # Returns
/// * `Ok(())` on success
/// * `Err(errno)` on failure
pub fn attach(
    index: usize,
    fd: u64,
    path: &str,
    file_size: u64,
    readonly: bool,
) -> Result<(), i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    let mut devices = LOOP_DEVICES.lock();
    let dev = &mut devices[index];

    if dev.is_attached() {
        return Err(crate::posix::errno::EBUSY);
    }

    dev.state = LoopState::Attached;
    dev.backing_fd = Some(fd);
    dev.backing_path = Some(String::from(path));
    dev.total_size = file_size;
    dev.offset = 0;
    dev.sizelimit = 0;
    dev.block_size = 512;

    if readonly {
        dev.flags |= LoopFlags::ReadOnly as u32;
    }

    crate::kinfo!("loop{}: attached to '{}' ({} bytes)", index, path, file_size);
    Ok(())
}

/// Detach a file from a loop device
pub fn detach(index: usize) -> Result<(), i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    let mut devices = LOOP_DEVICES.lock();
    let dev = &mut devices[index];

    if dev.is_free() {
        return Err(crate::posix::errno::ENXIO);
    }

    let path = dev.backing_path.clone().unwrap_or_default();
    dev.clear();

    crate::kinfo!("loop{}: detached from '{}'", index, path);
    Ok(())
}

/// Set loop device offset
pub fn set_offset(index: usize, offset: u64) -> Result<(), i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    let mut devices = LOOP_DEVICES.lock();
    let dev = &mut devices[index];

    if dev.is_free() {
        return Err(crate::posix::errno::ENXIO);
    }

    if offset >= dev.total_size {
        return Err(crate::posix::errno::EINVAL);
    }

    dev.offset = offset;
    Ok(())
}

/// Set loop device size limit
pub fn set_sizelimit(index: usize, sizelimit: u64) -> Result<(), i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    let mut devices = LOOP_DEVICES.lock();
    let dev = &mut devices[index];

    if dev.is_free() {
        return Err(crate::posix::errno::ENXIO);
    }

    dev.sizelimit = sizelimit;
    Ok(())
}

/// Set loop device block size
pub fn set_block_size(index: usize, block_size: u32) -> Result<(), i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    // Validate block size (must be power of 2, between 512 and 4096)
    if block_size < 512 || block_size > 4096 || !block_size.is_power_of_two() {
        return Err(crate::posix::errno::EINVAL);
    }

    let mut devices = LOOP_DEVICES.lock();
    let dev = &mut devices[index];

    if dev.is_free() {
        return Err(crate::posix::errno::ENXIO);
    }

    dev.block_size = block_size;
    Ok(())
}

/// Get loop device status (64-bit)
pub fn get_status64(index: usize) -> Result<LoopInfo64, i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    let devices = LOOP_DEVICES.lock();
    let dev = &devices[index];

    if dev.is_free() {
        return Err(crate::posix::errno::ENXIO);
    }

    Ok(dev.get_info64())
}

/// Set loop device status (64-bit)
pub fn set_status64(index: usize, info: &LoopInfo64) -> Result<(), i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    let mut devices = LOOP_DEVICES.lock();
    let dev = &mut devices[index];

    if dev.is_free() {
        return Err(crate::posix::errno::ENXIO);
    }

    dev.offset = info.lo_offset;
    dev.sizelimit = info.lo_sizelimit;
    dev.flags = info.lo_flags;

    Ok(())
}

/// Get backing file descriptor for a loop device
pub fn get_backing_fd(index: usize) -> Option<u64> {
    if index >= MAX_LOOP_DEVICES {
        return None;
    }

    let devices = LOOP_DEVICES.lock();
    devices[index].backing_fd
}

/// Get device info for block layer
pub fn get_device_info(index: usize) -> Option<(u64, u32, bool)> {
    if index >= MAX_LOOP_DEVICES {
        return None;
    }

    let devices = LOOP_DEVICES.lock();
    let dev = &devices[index];

    if dev.is_free() {
        return None;
    }

    Some((
        dev.total_sectors(),
        dev.block_size,
        dev.is_readonly(),
    ))
}

/// Read from a loop device
///
/// This function reads data from the loop device by translating the
/// sector-based read request to a byte-based read on the backing file.
///
/// # Arguments
/// * `index` - Loop device index
/// * `sector` - Starting sector
/// * `count` - Number of sectors to read
/// * `buf` - Buffer to read into
///
/// # Returns
/// * Number of bytes read, or negative error code
pub fn read_sectors(index: usize, sector: u64, count: u32, buf: &mut [u8]) -> Result<usize, i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    let (backing_fd, offset, block_size, total_sectors) = {
        let devices = LOOP_DEVICES.lock();
        let dev = &devices[index];

        if dev.is_free() {
            return Err(crate::posix::errno::ENXIO);
        }

        let backing_fd = dev.backing_fd.ok_or(crate::posix::errno::ENXIO)?;
        (
            backing_fd,
            dev.sector_to_offset(sector),
            dev.block_size,
            dev.total_sectors(),
        )
    };

    // Check bounds
    if sector >= total_sectors {
        return Ok(0); // EOF
    }

    let available_sectors = total_sectors - sector;
    let actual_count = (count as u64).min(available_sectors) as u32;
    let bytes_to_read = actual_count as usize * block_size as usize;

    if buf.len() < bytes_to_read {
        return Err(crate::posix::errno::EINVAL);
    }

    // Read from backing file using pread
    crate::syscalls::pread_internal(backing_fd, &mut buf[..bytes_to_read], offset as i64)
        .map_err(|_| crate::posix::errno::EIO)
}

/// Write to a loop device
pub fn write_sectors(index: usize, sector: u64, count: u32, buf: &[u8]) -> Result<usize, i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    let (backing_fd, offset, block_size, total_sectors, readonly) = {
        let devices = LOOP_DEVICES.lock();
        let dev = &devices[index];

        if dev.is_free() {
            return Err(crate::posix::errno::ENXIO);
        }

        if dev.is_readonly() {
            return Err(crate::posix::errno::EROFS);
        }

        let backing_fd = dev.backing_fd.ok_or(crate::posix::errno::ENXIO)?;
        (
            backing_fd,
            dev.sector_to_offset(sector),
            dev.block_size,
            dev.total_sectors(),
            dev.is_readonly(),
        )
    };

    if readonly {
        return Err(crate::posix::errno::EROFS);
    }

    // Check bounds
    if sector >= total_sectors {
        return Err(crate::posix::errno::ENOSPC);
    }

    let available_sectors = total_sectors - sector;
    let actual_count = (count as u64).min(available_sectors) as u32;
    let bytes_to_write = actual_count as usize * block_size as usize;

    if buf.len() < bytes_to_write {
        return Err(crate::posix::errno::EINVAL);
    }

    // Write to backing file using pwrite
    crate::syscalls::pwrite_internal(backing_fd, &buf[..bytes_to_write], offset as i64)
        .map_err(|_| crate::posix::errno::EIO)
}

/// Handle ioctl on /dev/loop-control
pub fn loop_control_ioctl(cmd: u64, arg: u64) -> Result<i64, i32> {
    match cmd {
        LOOP_CTL_GET_FREE => {
            get_free().map(|idx| idx as i64).ok_or(crate::posix::errno::ENOSPC)
        }
        LOOP_CTL_ADD => {
            let index = arg as usize;
            if index >= MAX_LOOP_DEVICES {
                Err(crate::posix::errno::EINVAL)
            } else {
                // Devices are pre-allocated, just return success if valid
                Ok(index as i64)
            }
        }
        LOOP_CTL_REMOVE => {
            let index = arg as usize;
            if index >= MAX_LOOP_DEVICES {
                Err(crate::posix::errno::EINVAL)
            } else {
                let devices = LOOP_DEVICES.lock();
                if devices[index].is_attached() {
                    Err(crate::posix::errno::EBUSY)
                } else {
                    Ok(0)
                }
            }
        }
        _ => Err(crate::posix::errno::ENOTTY),
    }
}

/// Handle ioctl on /dev/loopN
pub fn loop_device_ioctl(index: usize, cmd: u64, arg: u64) -> Result<i64, i32> {
    if index >= MAX_LOOP_DEVICES {
        return Err(crate::posix::errno::ENODEV);
    }

    match cmd {
        LOOP_SET_FD => {
            // arg is the file descriptor to attach
            let fd = arg;

            // Get file size from the fd
            let file_size = crate::syscalls::get_file_size(fd)
                .ok_or(crate::posix::errno::EBADF)?;

            // Get file path for display
            let path = crate::syscalls::get_file_path(fd)
                .unwrap_or_else(|| alloc::format!("fd:{}", fd));

            attach(index, fd, &path, file_size, false)?;
            Ok(0)
        }
        LOOP_CLR_FD => {
            detach(index)?;
            Ok(0)
        }
        LOOP_GET_STATUS64 => {
            let info = get_status64(index)?;
            // Copy info to user space
            unsafe {
                let user_ptr = arg as *mut LoopInfo64;
                if !crate::syscalls::user_buffer_in_range(
                    arg,
                    core::mem::size_of::<LoopInfo64>() as u64,
                ) {
                    return Err(crate::posix::errno::EFAULT);
                }
                *user_ptr = info;
            }
            Ok(0)
        }
        LOOP_SET_STATUS64 => {
            let info = unsafe {
                let user_ptr = arg as *const LoopInfo64;
                if !crate::syscalls::user_buffer_in_range(
                    arg,
                    core::mem::size_of::<LoopInfo64>() as u64,
                ) {
                    return Err(crate::posix::errno::EFAULT);
                }
                *user_ptr
            };
            set_status64(index, &info)?;
            Ok(0)
        }
        LOOP_SET_CAPACITY => {
            // Re-read the size from backing file
            let devices = LOOP_DEVICES.lock();
            let dev = &devices[index];
            if let Some(fd) = dev.backing_fd {
                drop(devices);
                if let Some(new_size) = crate::syscalls::get_file_size(fd) {
                    let mut devices = LOOP_DEVICES.lock();
                    devices[index].total_size = new_size;
                }
            }
            Ok(0)
        }
        LOOP_SET_BLOCK_SIZE => {
            set_block_size(index, arg as u32)?;
            Ok(0)
        }
        LOOP_SET_DIRECT_IO => {
            // Direct I/O flag - not fully implemented but accept the ioctl
            let mut devices = LOOP_DEVICES.lock();
            if arg != 0 {
                devices[index].flags |= LoopFlags::DirectIO as u32;
            } else {
                devices[index].flags &= !(LoopFlags::DirectIO as u32);
            }
            Ok(0)
        }
        _ => Err(crate::posix::errno::ENOTTY),
    }
}

/// List loop devices with their status (for debugging)
pub fn list_devices() -> Vec<(usize, bool, Option<String>)> {
    let devices = LOOP_DEVICES.lock();
    devices
        .iter()
        .enumerate()
        .map(|(i, dev)| {
            (i, dev.is_attached(), dev.backing_path.clone())
        })
        .collect()
}
