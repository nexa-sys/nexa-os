//! sysfs - System Information Pseudo-Filesystem
//!
//! This module implements a Linux-compatible /sys filesystem that provides
//! system and device information through a virtual filesystem interface.
//!
//! Supported entries:
//! - /sys/kernel/version - Kernel version string
//! - /sys/kernel/hostname - System hostname
//! - /sys/kernel/ostype - Operating system type
//! - /sys/kernel/osrelease - OS release version
//! - /sys/kernel/ngroups_max - Maximum number of groups
//! - /sys/kernel/pid_max - Maximum PID value
//! - /sys/kernel/threads-max - Maximum threads
//! - /sys/kernel/random/entropy_avail - Available entropy
//! - /sys/kernel/random/poolsize - Entropy pool size
//! - /sys/kernel/random/uuid - Random UUID
//! - /sys/class/ - Device classes directory
//! - /sys/class/tty/ - TTY devices
//! - /sys/class/block/ - Block devices
//! - /sys/class/net/ - Network devices
//! - /sys/devices/ - Device hierarchy
//! - /sys/block/ - Block device information
//! - /sys/bus/ - Bus types
//! - /sys/fs/ - Filesystem information
//! - /sys/power/state - Power management state
//! - /sys/power/mem_sleep - Memory sleep states

use crate::posix::{FileType, Metadata};
use core::fmt::Write;

/// Buffer size for dynamically generated sysfs content
const SYS_BUF_SIZE: usize = 2048;

/// Static buffer for sysfs content generation (protected by lock)
static SYS_BUFFER: spin::Mutex<[u8; SYS_BUF_SIZE]> = spin::Mutex::new([0u8; SYS_BUF_SIZE]);

/// A simple writer that writes to a fixed-size buffer
struct BufWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BufWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn len(&self) -> usize {
        self.pos
    }
}

impl<'a> Write for BufWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len() - self.pos;
        let to_write = bytes.len().min(remaining);
        self.buf[self.pos..self.pos + to_write].copy_from_slice(&bytes[..to_write]);
        self.pos += to_write;
        Ok(())
    }
}

/// NexaOS version string
const NEXAOS_VERSION: &str = "0.1.0";
const NEXAOS_OSTYPE: &str = "NexaOS";
const NEXAOS_HOSTNAME: &str = "nexaos";

// =============================================================================
// /sys/kernel/ entries
// =============================================================================

/// Generate /sys/kernel/version content
pub fn generate_kernel_version() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = write!(writer, "{}\n", NEXAOS_VERSION);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /sys/kernel/ostype content
pub fn generate_kernel_ostype() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = write!(writer, "{}\n", NEXAOS_OSTYPE);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /sys/kernel/osrelease content
pub fn generate_kernel_osrelease() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = write!(writer, "{}\n", NEXAOS_VERSION);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /sys/kernel/hostname content
pub fn generate_kernel_hostname() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = write!(writer, "{}\n", NEXAOS_HOSTNAME);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /sys/kernel/ngroups_max content
pub fn generate_kernel_ngroups_max() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = write!(writer, "65536\n");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /sys/kernel/pid_max content
pub fn generate_kernel_pid_max() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Use MAX_PROCESSES from process module
    let _ = write!(writer, "{}\n", crate::process::MAX_PROCESSES);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /sys/kernel/threads-max content
pub fn generate_kernel_threads_max() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = write!(writer, "{}\n", crate::process::MAX_PROCESSES * 4);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

// =============================================================================
// /sys/kernel/random/ entries
// =============================================================================

/// Generate /sys/kernel/random/entropy_avail content
pub fn generate_random_entropy_avail() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Placeholder entropy value
    let _ = write!(writer, "256\n");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /sys/kernel/random/poolsize content
pub fn generate_random_poolsize() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = write!(writer, "4096\n");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /sys/kernel/random/uuid content
pub fn generate_random_uuid() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Generate a simple pseudo-random UUID based on tick counter
    let tick = crate::scheduler::get_tick();
    let _ = write!(
        writer,
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}\n",
        (tick >> 32) as u32,
        ((tick >> 16) & 0xFFFF) as u16,
        (tick & 0xFFF) as u16,
        0x8000 | ((tick >> 48) & 0x3FFF) as u16,
        tick & 0xFFFFFFFFFFFF
    );

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

// =============================================================================
// /sys/power/ entries
// =============================================================================

/// Generate /sys/power/state content
pub fn generate_power_state() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Available power states
    let _ = write!(writer, "freeze mem disk\n");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

/// Generate /sys/power/mem_sleep content
pub fn generate_power_mem_sleep() -> (&'static [u8], usize) {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Available sleep states, current in brackets
    let _ = write!(writer, "s2idle [deep]\n");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    (slice, len)
}

// =============================================================================
// /sys/class/ entries
// =============================================================================

/// Get list of TTY devices for /sys/class/tty/
pub fn get_tty_devices() -> &'static [&'static str] {
    static TTYS: [&str; 4] = ["tty0", "tty1", "ttyS0", "console"];
    &TTYS
}

/// Get list of block devices for /sys/class/block/
pub fn get_block_devices() -> &'static [&'static str] {
    static BLOCKS: [&str; 2] = ["vda", "vda1"];
    &BLOCKS
}

/// Get list of network devices for /sys/class/net/
pub fn get_net_devices() -> &'static [&'static str] {
    static NETS: [&str; 2] = ["lo", "eth0"];
    &NETS
}

// =============================================================================
// /sys/block/[device]/ entries
// =============================================================================

/// Generate /sys/block/[device]/size content (in 512-byte sectors)
pub fn generate_block_size(device: &str) -> Option<(&'static [u8], usize)> {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Return a reasonable size for virtual disk (256MB in sectors)
    let sectors: u64 = match device {
        "vda" => 256 * 1024 * 2,  // 256 MB
        "vda1" => 255 * 1024 * 2, // Slightly smaller for partition
        _ => return None,
    };

    let _ = write!(writer, "{}\n", sectors);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /sys/block/[device]/stat content
pub fn generate_block_stat(device: &str) -> Option<(&'static [u8], usize)> {
    if !["vda", "vda1"].contains(&device) {
        return None;
    }

    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    // Block device statistics format:
    // read_ios read_merges read_sectors read_ticks write_ios write_merges write_sectors write_ticks
    // in_flight io_ticks time_in_queue discard_ios discard_merges discard_sectors discard_ticks
    let _ = write!(writer, "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /sys/block/[device]/device/model content
pub fn generate_block_model(device: &str) -> Option<(&'static [u8], usize)> {
    if !["vda", "vda1"].contains(&device) {
        return None;
    }

    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = write!(writer, "NexaOS Virtual Disk\n");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /sys/block/[device]/device/vendor content
pub fn generate_block_vendor(device: &str) -> Option<(&'static [u8], usize)> {
    if !["vda", "vda1"].contains(&device) {
        return None;
    }

    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let _ = write!(writer, "NexaOS\n");

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

// =============================================================================
// /sys/class/net/[device]/ entries
// =============================================================================

/// Generate /sys/class/net/[device]/address content (MAC address)
pub fn generate_net_address(device: &str) -> Option<(&'static [u8], usize)> {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    match device {
        "lo" => {
            let _ = write!(writer, "00:00:00:00:00:00\n");
        }
        "eth0" => {
            // Get real MAC address from network stack
            let mac =
                crate::net::with_net_stack(|stack| stack.get_device_info(0).map(|info| info.mac))
                    .flatten();

            if let Some(mac) = mac {
                let _ = write!(
                    writer,
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                );
            } else {
                // Fallback to QEMU default if stack not ready
                let _ = write!(writer, "52:54:00:12:34:56\n");
            }
        }
        _ => return None,
    };

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /sys/class/net/[device]/mtu content
pub fn generate_net_mtu(device: &str) -> Option<(&'static [u8], usize)> {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let mtu = match device {
        "lo" => 65536,
        "eth0" => 1500,
        _ => return None,
    };

    let _ = write!(writer, "{}\n", mtu);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /sys/class/net/[device]/operstate content
pub fn generate_net_operstate(device: &str) -> Option<(&'static [u8], usize)> {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let state = match device {
        "lo" => "unknown",
        "eth0" => "up",
        _ => return None,
    };

    let _ = write!(writer, "{}\n", state);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /sys/class/net/[device]/type content (ARPHRD type)
pub fn generate_net_type(device: &str) -> Option<(&'static [u8], usize)> {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let dev_type = match device {
        "lo" => 772, // ARPHRD_LOOPBACK
        "eth0" => 1, // ARPHRD_ETHER
        _ => return None,
    };

    let _ = write!(writer, "{}\n", dev_type);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

/// Generate /sys/class/net/[device]/flags content
pub fn generate_net_flags(device: &str) -> Option<(&'static [u8], usize)> {
    let mut buf = SYS_BUFFER.lock();
    let mut writer = BufWriter::new(&mut buf[..]);

    let flags = match device {
        "lo" => 0x49,     // IFF_UP | IFF_LOOPBACK | IFF_RUNNING
        "eth0" => 0x1003, // IFF_UP | IFF_BROADCAST | IFF_RUNNING
        _ => return None,
    };

    let _ = write!(writer, "0x{:x}\n", flags);

    let len = writer.len();
    let slice = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
    Some((slice, len))
}

// =============================================================================
// Metadata helpers
// =============================================================================

/// Metadata for sysfs file entries
pub fn sys_file_metadata(size: u64) -> Metadata {
    let mut meta = Metadata::empty()
        .with_type(FileType::Regular)
        .with_mode(0o444);
    meta.size = size;
    meta.nlink = 1;
    meta
}

/// Metadata for sysfs directory entries
pub fn sys_dir_metadata() -> Metadata {
    let mut meta = Metadata::empty()
        .with_type(FileType::Directory)
        .with_mode(0o555);
    meta.nlink = 2;
    meta
}
