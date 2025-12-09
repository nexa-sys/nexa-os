//! devfs - Device Filesystem
//!
//! This module implements a simple device filesystem that provides access
//! to kernel device nodes like /dev/null, /dev/zero, /dev/random, etc.
//!
//! Unlike ext2 or other disk-based filesystems, devfs nodes are virtual
//! and their content is generated/consumed by the kernel at runtime.

use crate::posix::{FileType, Metadata};

use super::vfs::{FileContent, FileSystem, OpenFile};

/// Device types supported by devfs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// /dev/null - discards all writes, reads return EOF
    Null,
    /// /dev/zero - reads return zeros, writes discarded
    Zero,
    /// /dev/random - blocking random number generator
    Random,
    /// /dev/urandom - non-blocking random number generator  
    Urandom,
    /// /dev/console - system console
    Console,
    /// Network device (e.g., net0)
    Network(u8),
    /// Block device (e.g., block0, vda)
    Block(u8),
    /// Framebuffer (e.g., fb0)
    Framebuffer(u8),
}

/// A device entry in devfs
#[derive(Debug, Clone)]
pub struct DeviceEntry {
    pub name: &'static str,
    pub dev_type: DeviceType,
    pub major: u32,
    pub minor: u32,
}

/// Maximum number of device entries
const MAX_DEVICES: usize = 64;

/// Global device table
static DEVICES: spin::Mutex<[Option<DeviceEntry>; MAX_DEVICES]> =
    spin::Mutex::new([const { None }; MAX_DEVICES]);

/// Device count
static DEVICE_COUNT: spin::Mutex<usize> = spin::Mutex::new(0);

/// Register a device in devfs
pub fn register_device(name: &'static str, dev_type: DeviceType, major: u32, minor: u32) {
    let mut devices = DEVICES.lock();
    let mut count = DEVICE_COUNT.lock();

    if *count >= MAX_DEVICES {
        crate::kwarn!("devfs: device table full, cannot register '{}'", name);
        return;
    }

    // Check for duplicate
    for i in 0..*count {
        if let Some(ref dev) = devices[i] {
            if dev.name == name {
                crate::kdebug!("devfs: device '{}' already registered", name);
                return;
            }
        }
    }

    devices[*count] = Some(DeviceEntry {
        name,
        dev_type,
        major,
        minor,
    });
    *count += 1;
    crate::kdebug!("devfs: registered device '{}' ({}:{})", name, major, minor);
}

/// Initialize devfs with standard devices
pub fn init() {
    crate::kinfo!("devfs: initializing device filesystem");

    // Standard character devices
    register_device("null", DeviceType::Null, 1, 3);
    register_device("zero", DeviceType::Zero, 1, 5);
    register_device("random", DeviceType::Random, 1, 8);
    register_device("urandom", DeviceType::Urandom, 1, 9);
    register_device("console", DeviceType::Console, 5, 1);
    
    // TTY devices
    register_device("tty", DeviceType::Console, 5, 0);
    register_device("tty0", DeviceType::Console, 4, 0);
    register_device("tty1", DeviceType::Console, 4, 1);
    register_device("tty2", DeviceType::Console, 4, 2);
    register_device("tty3", DeviceType::Console, 4, 3);
    register_device("tty4", DeviceType::Console, 4, 4);
    register_device("ptmx", DeviceType::Console, 5, 2);

    let count = *DEVICE_COUNT.lock();
    crate::kinfo!("devfs: initialized with {} devices", count);
}

/// Register network device
pub fn register_network_device(index: u8) {
    // Leak a static string for the device name
    let name: &'static str = match index {
        0 => "net0",
        1 => "net1",
        2 => "net2",
        3 => "net3",
        _ => return,
    };
    register_device(name, DeviceType::Network(index), 10, index as u32);
}

/// Register block device
pub fn register_block_device(index: u8) {
    let name: &'static str = match index {
        0 => "block0",
        1 => "block1",
        2 => "block2",
        3 => "block3",
        4 => "vda",
        5 => "vda1",
        _ => return,
    };
    register_device(name, DeviceType::Block(index), 8, index as u32);
}

/// Register framebuffer device
pub fn register_framebuffer_device(index: u8) {
    let name: &'static str = match index {
        0 => "fb0",
        1 => "fb1",
        _ => return,
    };
    register_device(name, DeviceType::Framebuffer(index), 29, index as u32);
}

/// The devfs filesystem implementation
pub struct DevFs;

/// Static instance of devfs
pub static DEVFS: DevFs = DevFs;

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn read(&self, path: &str) -> Option<OpenFile> {
        let name = path.trim_start_matches('/');

        let devices = DEVICES.lock();
        let count = *DEVICE_COUNT.lock();

        for i in 0..count {
            if let Some(ref dev) = devices[i] {
                if dev.name == name {
                    let file_type = match dev.dev_type {
                        DeviceType::Block(_) => FileType::Block,
                        _ => FileType::Character,
                    };
                    let meta = Metadata::empty()
                        .with_type(file_type)
                        .with_mode(0o666);
                    
                    return Some(OpenFile {
                        content: FileContent::Inline(&[]),
                        metadata: meta,
                    });
                }
            }
        }
        None
    }

    fn metadata(&self, path: &str) -> Option<Metadata> {
        let name = path.trim_start_matches('/');

        // Root directory
        if name.is_empty() || name == "." {
            return Some(
                Metadata::empty()
                    .with_type(FileType::Directory)
                    .with_mode(0o755),
            );
        }

        let devices = DEVICES.lock();
        let count = *DEVICE_COUNT.lock();

        for i in 0..count {
            if let Some(ref dev) = devices[i] {
                if dev.name == name {
                    let file_type = match dev.dev_type {
                        DeviceType::Block(_) => FileType::Block,
                        _ => FileType::Character,
                    };
                    let meta = Metadata::empty()
                        .with_type(file_type)
                        .with_mode(0o666);
                    return Some(meta);
                }
            }
        }
        None
    }

    fn list(&self, path: &str, callback: &mut dyn FnMut(&str, Metadata)) {
        let name = path.trim_start_matches('/');

        // Only root directory is listable
        if !name.is_empty() && name != "." {
            return;
        }

        // First output . and .. directory entries
        let dir_meta = Metadata::empty()
            .with_type(FileType::Directory)
            .with_mode(0o755);
        callback(".", dir_meta);
        callback("..", dir_meta);

        let devices = DEVICES.lock();
        let count = *DEVICE_COUNT.lock();

        for i in 0..count {
            if let Some(ref dev) = devices[i] {
                let file_type = match dev.dev_type {
                    DeviceType::Block(_) => FileType::Block,
                    _ => FileType::Character,
                };
                let meta = Metadata::empty()
                    .with_type(file_type)
                    .with_mode(0o666);
                
                callback(dev.name, meta);
            }
        }
    }
}

/// Get device type by name
pub fn get_device_type(name: &str) -> Option<DeviceType> {
    let devices = DEVICES.lock();
    let count = *DEVICE_COUNT.lock();

    for i in 0..count {
        if let Some(ref dev) = devices[i] {
            if dev.name == name {
                return Some(dev.dev_type);
            }
        }
    }
    None
}

/// Check if a path refers to a device
pub fn is_device(path: &str) -> bool {
    let name = if path.starts_with("/dev/") {
        &path[5..]
    } else {
        path.trim_start_matches('/')
    };
    get_device_type(name).is_some()
}
