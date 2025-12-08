//! fstab - Filesystem Table Configuration
//!
//! This module implements parsing and handling of /etc/fstab for automatic
//! filesystem mounting during boot. It follows the standard Linux fstab format:
//!
//! ```text
//! # <device>  <mount_point>  <fs_type>  <options>  <dump>  <pass>
//! /dev/vda1   /              ext2       defaults   0       1
//! tmpfs       /tmp           tmpfs      defaults   0       0
//! proc        /proc          proc       defaults   0       0
//! ```
//!
//! Supported filesystem types:
//! - ext2/ext3/ext4 (via modular ext2 driver)
//! - tmpfs (in-memory filesystem)
//! - proc (process information pseudo-filesystem)
//! - sysfs (system information pseudo-filesystem)
//! - devtmpfs (device pseudo-filesystem)

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Maximum number of fstab entries
const MAX_FSTAB_ENTRIES: usize = 32;

/// Represents a single fstab entry
#[derive(Debug, Clone)]
pub struct FstabEntry {
    /// Device or pseudo-device name (e.g., "/dev/vda1", "tmpfs", "proc")
    pub device: String,
    /// Mount point path (e.g., "/", "/tmp", "/proc")
    pub mount_point: String,
    /// Filesystem type (e.g., "ext2", "tmpfs", "proc")
    pub fs_type: String,
    /// Mount options (e.g., "defaults", "ro", "noexec")
    pub options: String,
    /// Dump frequency (0 = no dump, 1 = dump)
    pub dump: u8,
    /// fsck pass number (0 = skip, 1 = root, 2 = other)
    pub pass: u8,
}

impl FstabEntry {
    /// Create a new fstab entry
    pub fn new(
        device: &str,
        mount_point: &str,
        fs_type: &str,
        options: &str,
        dump: u8,
        pass: u8,
    ) -> Self {
        Self {
            device: String::from(device),
            mount_point: String::from(mount_point),
            fs_type: String::from(fs_type),
            options: String::from(options),
            dump,
            pass,
        }
    }

    /// Parse options string into a vector of individual options
    pub fn parse_options(&self) -> Vec<&str> {
        self.options.split(',').collect()
    }

    /// Check if this entry should be mounted at boot
    pub fn mount_at_boot(&self) -> bool {
        let opts = self.parse_options();
        !opts.contains(&"noauto")
    }

    /// Check if this entry should be mounted read-only
    pub fn is_readonly(&self) -> bool {
        let opts = self.parse_options();
        opts.contains(&"ro")
    }

    /// Check if the "defaults" option is set
    pub fn has_defaults(&self) -> bool {
        let opts = self.parse_options();
        opts.contains(&"defaults")
    }

    /// Get the size option for tmpfs (returns size in bytes, 0 if not specified)
    pub fn get_size_option(&self) -> usize {
        for opt in self.parse_options() {
            if let Some(size_str) = opt.strip_prefix("size=") {
                return parse_size(size_str);
            }
        }
        0
    }

    /// Get the mode option for tmpfs (returns mode, 0 if not specified)
    pub fn get_mode_option(&self) -> u16 {
        for opt in self.parse_options() {
            if let Some(mode_str) = opt.strip_prefix("mode=") {
                if let Ok(mode) = u16::from_str_radix(mode_str, 8) {
                    return mode;
                }
            }
        }
        0
    }
}

/// Parse a size string like "10M", "1G", "4096" into bytes
fn parse_size(s: &str) -> usize {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }

    let (num_str, multiplier) = if s.ends_with('K') || s.ends_with('k') {
        (&s[..s.len() - 1], 1024)
    } else if s.ends_with('M') || s.ends_with('m') {
        (&s[..s.len() - 1], 1024 * 1024)
    } else if s.ends_with('G') || s.ends_with('g') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024)
    } else if s.ends_with('%') {
        // Percentage of total RAM - not implemented yet
        return 0;
    } else {
        (s, 1)
    };

    num_str.parse::<usize>().unwrap_or(0) * multiplier
}

/// Global fstab entries storage
static FSTAB: Mutex<Vec<FstabEntry>> = Mutex::new(Vec::new());

/// Parse an fstab file content and return entries
pub fn parse_fstab(content: &str) -> Vec<FstabEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        // Skip comments and empty lines
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Split by whitespace
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            crate::kwarn!("fstab: invalid line (too few fields): {}", line);
            continue;
        }

        let device = parts[0];
        let mount_point = parts[1];
        let fs_type = parts[2];
        let options = parts[3];
        let dump = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        let pass = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);

        entries.push(FstabEntry::new(
            device,
            mount_point,
            fs_type,
            options,
            dump,
            pass,
        ));
    }

    entries
}

/// Load fstab from /etc/fstab
pub fn load_fstab() -> Result<usize, &'static str> {
    // Try to read /etc/fstab
    let content = match crate::fs::read_file("/etc/fstab") {
        Some(c) => c,
        None => {
            crate::kinfo!("fstab: /etc/fstab not found, using defaults");
            return Ok(0);
        }
    };

    let entries = parse_fstab(content);
    let count = entries.len();

    let mut fstab = FSTAB.lock();
    *fstab = entries;

    crate::kinfo!("fstab: loaded {} entries from /etc/fstab", count);
    Ok(count)
}

/// Get all fstab entries
pub fn get_entries() -> Vec<FstabEntry> {
    FSTAB.lock().clone()
}

/// Get fstab entries sorted by pass number for fsck ordering
pub fn get_entries_by_pass() -> Vec<FstabEntry> {
    let mut entries = get_entries();
    entries.sort_by_key(|e| e.pass);
    entries
}

/// Get entries that should be auto-mounted at boot
pub fn get_auto_mount_entries() -> Vec<FstabEntry> {
    get_entries()
        .into_iter()
        .filter(|e| e.mount_at_boot())
        .collect()
}

/// Find fstab entry by mount point
pub fn find_by_mount_point(mount_point: &str) -> Option<FstabEntry> {
    FSTAB
        .lock()
        .iter()
        .find(|e| e.mount_point == mount_point)
        .cloned()
}

/// Find fstab entry by device
pub fn find_by_device(device: &str) -> Option<FstabEntry> {
    FSTAB.lock().iter().find(|e| e.device == device).cloned()
}

/// Add an entry to the in-memory fstab (does not persist)
pub fn add_entry(entry: FstabEntry) -> Result<(), &'static str> {
    let mut fstab = FSTAB.lock();
    if fstab.len() >= MAX_FSTAB_ENTRIES {
        return Err("fstab table full");
    }
    fstab.push(entry);
    Ok(())
}

/// Mount all filesystems from fstab
///
/// This function processes fstab entries and mounts filesystems in order:
/// 1. First pass (pass=1): Root filesystem and critical mounts
/// 2. Second pass (pass=2+): Other filesystems
/// 3. Pseudo-filesystems (proc, sys, tmpfs) with pass=0
pub fn mount_all() -> Result<usize, &'static str> {
    let entries = get_auto_mount_entries();
    let mut mounted = 0;
    let mut errors = 0;

    // Sort entries: pass=1 first, then pass=2+, then pass=0 (pseudo-fs)
    let mut sorted_entries = entries.clone();
    sorted_entries.sort_by(|a, b| {
        match (a.pass, b.pass) {
            (1, _) => core::cmp::Ordering::Less,
            (_, 1) => core::cmp::Ordering::Greater,
            (0, 0) => core::cmp::Ordering::Equal,
            (0, _) => core::cmp::Ordering::Greater,
            (_, 0) => core::cmp::Ordering::Less,
            (a, b) => a.cmp(&b),
        }
    });

    for entry in sorted_entries {
        crate::kinfo!(
            "fstab: mounting {} on {} (type: {})",
            entry.device,
            entry.mount_point,
            entry.fs_type
        );

        match mount_entry(&entry) {
            Ok(()) => {
                mounted += 1;
                crate::kinfo!("fstab: successfully mounted {}", entry.mount_point);
            }
            Err(e) => {
                errors += 1;
                crate::kwarn!(
                    "fstab: failed to mount {} on {}: {}",
                    entry.device,
                    entry.mount_point,
                    e
                );
            }
        }
    }

    if errors > 0 {
        crate::kwarn!("fstab: {} mount errors occurred", errors);
    }

    Ok(mounted)
}

/// Mount a single fstab entry
pub fn mount_entry(entry: &FstabEntry) -> Result<(), &'static str> {
    use super::tmpfs;

    match entry.fs_type.as_str() {
        "tmpfs" => {
            // Create mount point if it doesn't exist
            if !crate::fs::file_exists(&entry.mount_point) {
                crate::fs::add_directory(leak_string(&entry.mount_point));
            }

            // Parse tmpfs options
            let mut options = tmpfs::TmpfsMountOptions::default();
            if let size = entry.get_size_option() {
                if size > 0 {
                    options.size = size;
                }
            }
            if let mode = entry.get_mode_option() {
                if mode > 0 {
                    options.mode = mode;
                }
            }

            // Mount tmpfs
            tmpfs::mount_tmpfs(leak_string(&entry.mount_point), options)
                .map_err(|_| "failed to mount tmpfs")?;

            // Track in mount state
            track_mount(&entry.mount_point, &entry.fs_type, &entry.device);

            Ok(())
        }

        "proc" => {
            // proc is typically already mounted during early boot
            if crate::boot::stages::is_mounted("proc") {
                crate::kinfo!("fstab: /proc already mounted");
                return Ok(());
            }

            // Create mount point
            if !crate::fs::file_exists(&entry.mount_point) {
                crate::fs::add_directory(leak_string(&entry.mount_point));
            }

            crate::boot::stages::mark_mounted("proc");
            track_mount(&entry.mount_point, &entry.fs_type, &entry.device);
            Ok(())
        }

        "sysfs" => {
            // sysfs is typically already mounted during early boot
            if crate::boot::stages::is_mounted("sys") {
                crate::kinfo!("fstab: /sys already mounted");
                return Ok(());
            }

            // Create mount point
            if !crate::fs::file_exists(&entry.mount_point) {
                crate::fs::add_directory(leak_string(&entry.mount_point));
            }

            crate::boot::stages::mark_mounted("sys");
            track_mount(&entry.mount_point, &entry.fs_type, &entry.device);
            Ok(())
        }

        "devtmpfs" | "devfs" => {
            // devtmpfs is typically already mounted during early boot
            if crate::boot::stages::is_mounted("dev") {
                crate::kinfo!("fstab: /dev already mounted");
                return Ok(());
            }

            crate::boot::stages::mark_mounted("dev");
            track_mount(&entry.mount_point, &entry.fs_type, &entry.device);
            Ok(())
        }

        "ext2" | "ext3" | "ext4" => {
            // Block device filesystem - requires block device access
            // This is handled separately during root mount
            if entry.mount_point == "/" {
                if crate::boot::stages::is_mounted("rootfs") {
                    crate::kinfo!("fstab: root filesystem already mounted");
                    return Ok(());
                }
            }

            // For non-root ext2/3/4 mounts, we need block device access
            crate::kwarn!(
                "fstab: ext2/3/4 mount for {} not yet implemented",
                entry.mount_point
            );
            Err("ext2/3/4 mount not fully implemented")
        }

        "none" | "bind" => {
            // No-op or bind mount
            crate::kinfo!("fstab: skipping none/bind mount for {}", entry.mount_point);
            Ok(())
        }

        "swap" => {
            // Swap device - call swapon syscall handler
            crate::kinfo!("fstab: enabling swap on {}", entry.device);
            
            // Parse priority from options (pri=N)
            let mut flags: i32 = 0;
            for opt in entry.options.split(',') {
                if let Some(prio_str) = opt.strip_prefix("pri=") {
                    if let Ok(prio) = prio_str.parse::<i32>() {
                        // SWAP_FLAG_PREFER | priority
                        flags = 0x8000 | (prio & 0x7fff);
                    }
                }
            }
            
            // Call swapon through the syscall handler
            match crate::syscalls::swap::sys_swapon(&entry.device, flags) {
                Ok(_) => {
                    crate::kinfo!("fstab: swap enabled on {}", entry.device);
                    Ok(())
                }
                Err(e) => {
                    crate::kwarn!("fstab: failed to enable swap on {}: {:?}", entry.device, e);
                    Err("failed to enable swap")
                }
            }
        }

        _ => {
            crate::kwarn!("fstab: unknown filesystem type: {}", entry.fs_type);
            Err("unknown filesystem type")
        }
    }
}

/// Track a mount in the mount table for /proc/mounts
static MOUNT_TABLE: Mutex<Vec<MountInfo>> = Mutex::new(Vec::new());

#[derive(Debug, Clone)]
pub struct MountInfo {
    pub device: String,
    pub mount_point: String,
    pub fs_type: String,
    pub options: String,
}

fn track_mount(mount_point: &str, fs_type: &str, device: &str) {
    let mut table = MOUNT_TABLE.lock();
    
    // Check if already tracked
    if table.iter().any(|m| m.mount_point == mount_point) {
        return;
    }
    
    table.push(MountInfo {
        device: String::from(device),
        mount_point: String::from(mount_point),
        fs_type: String::from(fs_type),
        options: String::from("rw"),
    });
}

/// Get all tracked mounts
pub fn get_mounts() -> Vec<MountInfo> {
    MOUNT_TABLE.lock().clone()
}

/// Leak a String to get a &'static str
/// SAFETY: This leaks memory intentionally for static references needed by the VFS
fn leak_string(s: &str) -> &'static str {
    let boxed = alloc::boxed::Box::leak(String::from(s).into_boxed_str());
    boxed
}

/// Generate default fstab content
pub fn default_fstab() -> &'static str {
    r#"# /etc/fstab: static file system information
# <device>      <mount_point>  <fs_type>  <options>       <dump>  <pass>

# Root filesystem (already mounted by kernel)
/dev/vda1       /              ext2       defaults        0       1

# Virtual filesystems
proc            /proc          proc       defaults        0       0
sysfs           /sys           sysfs      defaults        0       0
devtmpfs        /dev           devtmpfs   defaults        0       0

# Temporary filesystems
tmpfs           /tmp           tmpfs      defaults,mode=1777,size=64M    0       0
tmpfs           /run           tmpfs      defaults,mode=0755,size=32M    0       0
tmpfs           /dev/shm       tmpfs      defaults,mode=1777,size=64M    0       0
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("1024"), 1024);
        assert_eq!(parse_size("10K"), 10 * 1024);
        assert_eq!(parse_size("10M"), 10 * 1024 * 1024);
        assert_eq!(parse_size("1G"), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_fstab() {
        let content = r#"
# Comment line
/dev/vda1   /       ext2    defaults    0   1
tmpfs       /tmp    tmpfs   size=64M    0   0
"#;
        let entries = parse_fstab(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].device, "/dev/vda1");
        assert_eq!(entries[0].mount_point, "/");
        assert_eq!(entries[1].fs_type, "tmpfs");
    }
}
