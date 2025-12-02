use spin::Mutex;
use alloc::vec::Vec;

use crate::bootinfo;
use crate::posix::{self, FileType, Metadata};
use crate::safety::static_slice_from_raw_parts;

use super::ext2_modular::{self, FileRefHandle};

// Default ni configuration shipped when initramfs does not provide one.
// The format mirrors a minimal subset of systemd units with an Init section
// and one or more Service blocks.
const DEFAULT_NI_CONF: &[u8] = b"# Nexa Init configuration (ni)\n\
# This file is loaded by /sbin/init on boot if no custom config exists\n\
[Init]\n\
DefaultTarget=multi-user.target\n\
FallbackTarget=rescue.target\n\
\n\
[Service \"bootstrap-shell\"]\n\
Description=Interactive bootstrap shell\n\
ExecStart=/bin/sh\n\
Restart=always\n\
RestartSec=1\n\
RestartLimitIntervalSec=60\n\
RestartLimitBurst=5\n\
WantedBy=multi-user.target rescue.target\n\
\n\
[Service \"uefi-compatd\"]\n\
Description=UEFI compatibility bridge\n\
ExecStart=/sbin/uefi-compatd\n\
Restart=no\n\
WantedBy=multi-user.target rescue.target\n";

const EXT2_READ_CACHE_SIZE: usize = 8 * 1024 * 1024; // 8 MiB max file read size

/// Heap-allocated ext2 read cache (allocated on demand)
struct Ext2ReadCache {
    buffer: Option<Vec<u8>>,
}

impl Ext2ReadCache {
    const fn new() -> Self {
        Self { buffer: None }
    }

    /// Get or allocate a buffer of the required size
    fn get_buffer(&mut self, size: usize) -> Option<&mut [u8]> {
        if size > EXT2_READ_CACHE_SIZE {
            return None;
        }
        
        // Allocate or resize buffer as needed
        if self.buffer.is_none() || self.buffer.as_ref().unwrap().len() < size {
            // Allocate with some headroom to avoid frequent reallocations
            let alloc_size = size.max(64 * 1024); // At least 64KB
            let mut new_buf = Vec::new();
            if new_buf.try_reserve_exact(alloc_size).is_err() {
                crate::kwarn!("Failed to allocate {} bytes for ext2 read cache", alloc_size);
                return None;
            }
            new_buf.resize(alloc_size, 0);
            self.buffer = Some(new_buf);
        }
        
        self.buffer.as_mut().map(|b| &mut b[..size])
    }
    
    /// Get a static slice reference to the cached data
    /// SAFETY: Caller must ensure the lock is held and data won't be modified
    unsafe fn as_static_slice(&self, size: usize) -> Option<&'static [u8]> {
        self.buffer.as_ref().map(|b| {
            core::slice::from_raw_parts(b.as_ptr(), size)
        })
    }
}

static EXT2_READ_CACHE: Mutex<Ext2ReadCache> = Mutex::new(Ext2ReadCache::new());
static EMPTY_EXT2_FILE: [u8; 0] = [];

#[derive(Clone, Copy)]
pub struct File {
    pub name: &'static str,
    pub content: &'static [u8],
    pub is_dir: bool,
}

const MAX_FILES: usize = 64;
const MAX_MOUNTS: usize = 8;

static FILES: Mutex<[Option<File>; MAX_FILES]> = Mutex::new([None; MAX_FILES]);
static FILE_METADATA: Mutex<[Option<Metadata>; MAX_FILES]> = Mutex::new([None; MAX_FILES]);
static FILE_COUNT: Mutex<usize> = Mutex::new(0);
static MOUNTS: Mutex<[Option<MountEntry>; MAX_MOUNTS]> = Mutex::new([None; MAX_MOUNTS]);

#[derive(Clone, Copy)]
pub enum FileContent {
    Inline(&'static [u8]),
    Ext2Modular(FileRefHandle),
}

#[derive(Clone, Copy)]
pub struct OpenFile {
    pub content: FileContent,
    pub metadata: Metadata,
}

#[derive(Clone, Copy)]
struct ListedEntry {
    name: [u8; 256],
    name_len: usize,
    metadata: Metadata,
}

impl ListedEntry {
    fn new(name: &str, metadata: Metadata) -> Self {
        let mut buf = [0u8; 256];
        let len = name.len().min(255);
        buf[..len].copy_from_slice(&name.as_bytes()[..len]);
        Self {
            name: buf,
            name_len: len,
            metadata,
        }
    }
    
    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
    
    fn matches(&self, other: &str) -> bool {
        self.name_str() == other
    }
}

pub trait FileSystem: Sync {
    fn name(&self) -> &'static str;
    fn read(&self, path: &str) -> Option<OpenFile>;
    fn metadata(&self, path: &str) -> Option<Metadata>;
    fn list(&self, path: &str, cb: &mut dyn FnMut(&str, Metadata));

    // Optional write support - default implementation returns error
    fn write(&self, _path: &str, _data: &[u8]) -> Result<usize, &'static str> {
        Err("write not supported")
    }

    fn create(&self, _path: &str) -> Result<(), &'static str> {
        Err("create not supported")
    }
}

#[derive(Clone, Copy)]
struct MountEntry {
    mount_point: &'static str,
    fs: &'static dyn FileSystem,
}

struct InitramfsFilesystem;

static INITFS: InitramfsFilesystem = InitramfsFilesystem;

fn normalize_component(path: &str) -> &str {
    if path.is_empty() || path == "/" {
        ""
    } else {
        path.trim_matches('/')
    }
}

fn split_first_component(path: &'static str) -> (&'static str, Option<&'static str>) {
    if let Some(pos) = path.find('/') {
        let (head, rest) = path.split_at(pos);
        let remainder = &rest[1..];
        if remainder.is_empty() {
            (head, None)
        } else {
            (head, Some(remainder))
        }
    } else {
        (path, None)
    }
}

fn default_dir_meta() -> Metadata {
    let mut meta = Metadata::empty()
        .with_type(FileType::Directory)
        .with_mode(0o755);
    meta.nlink = 2;
    meta
}

fn emit_unique(entries: &mut [Option<ListedEntry>; MAX_FILES], name: &str, meta: Metadata) {
    for slot in entries.iter_mut() {
        if let Some(existing) = slot {
            if existing.matches(name) {
                if existing.metadata.file_type != FileType::Directory
                    && meta.file_type == FileType::Directory
                {
                    *slot = Some(ListedEntry::new(name, meta));
                }
                return;
            }
        }
    }

    if let Some(slot) = entries.iter_mut().find(|slot| slot.is_none()) {
        *slot = Some(ListedEntry::new(name, meta));
    }
}

pub fn init() {
    crate::kinfo!("Filesystem init: start");

    {
        let mut files = FILES.lock();
        let mut metas = FILE_METADATA.lock();
        for slot in files.iter_mut() {
            *slot = None;
        }
        for slot in metas.iter_mut() {
            *slot = None;
        }
        *FILE_COUNT.lock() = 0;
    }

    {
        let mut mounts = MOUNTS.lock();
        for slot in mounts.iter_mut() {
            *slot = None;
        }
    }

    mount("/", &INITFS).expect("root mount must succeed");

    if crate::initramfs::get().is_none() {
        crate::kwarn!("Filesystem init: no initramfs available; starting empty");
        return;
    }

    let mut entry_count = 0usize;
    let mut ext_candidate: Option<&'static [u8]> = None;

    crate::initramfs::for_each_entry(|entry| {
        entry_count += 1;
        let name = entry.name.strip_prefix('/').unwrap_or(entry.name);
        let (mode, file_type) = posix::split_mode(entry.mode);

        let mut meta = Metadata::empty().with_mode(mode).with_type(file_type);
        meta.size = entry.data.len() as u64;
        meta.nlink = 1;
        meta.blocks = ((meta.size + 511) / 512).max(1);

        let is_dir = matches!(file_type, FileType::Directory);

        // Debug: log file registration
        if name == "bin/sh" || name.ends_with("/sh") {
            crate::kinfo!(
                "Registering shell: '{}' (size: {} bytes, is_dir: {})",
                name,
                entry.data.len(),
                is_dir
            );
        }

        add_file_with_metadata(name, entry.data, is_dir, meta);

        if ext_candidate.is_none()
            && matches!(file_type, FileType::Regular)
            && (name.ends_with(".ext2") || name.ends_with(".ext3") || name.ends_with(".ext4"))
        {
            ext_candidate = Some(entry.data);
        }
    });

    if ext_candidate.is_none() {
        if let Some(rootfs) = bootinfo::rootfs_slice() {
            crate::kinfo!(
                "Registering UEFI-staged rootfs image as /rootfs.ext2 ({} bytes)",
                rootfs.len()
            );
            add_file_bytes("/rootfs.ext2", rootfs, false);
            ext_candidate = Some(rootfs);
        }
    } else if let Some(rootfs) = bootinfo::rootfs_slice() {
        crate::kinfo!(
            "UEFI-staged rootfs also available ({} bytes) as /rootfs-uefi.ext2",
            rootfs.len()
        );
        add_file_bytes("/rootfs-uefi.ext2", rootfs, false);
    }

    if let Some(image) = ext_candidate {
        // Check if ext2 module is loaded
        if ext2_modular::is_module_loaded() {
            match ext2_modular::new(image) {
                Ok(()) => {
                    crate::kinfo!("Mounted ext2 image at /mnt/ext via module");
                    let mut dir_meta = Metadata::empty().with_type(FileType::Directory);
                    dir_meta.nlink = 2;
                    dir_meta.mode |= 0o755;
                    add_file_with_metadata("mnt", &[], true, dir_meta);
                    add_file_with_metadata("mnt/ext", &[], true, dir_meta);
                }
                Err(err) => {
                    crate::kwarn!("Failed to parse ext2 image: {:?}", err);
                }
            }
        } else {
            crate::kwarn!("ext2 module not loaded, cannot mount ext2 image");
        }
    }

    // Ensure ni configuration hierarchy exists when initramfs is minimal.
    if stat("/etc").is_none() {
        add_file_bytes("etc", &[], true);
    }
    if stat("/etc/ni").is_none() {
        add_file_bytes("etc/ni", &[], true);
    }

    // Add default ni configuration file if not already present
    if stat("/etc/ni/ni.conf").is_none() {
        add_file_bytes("etc/ni/ni.conf", DEFAULT_NI_CONF, false);
        crate::kinfo!("Added default /etc/ni/ni.conf configuration");
    }

    let files_total = *FILE_COUNT.lock();
    crate::kinfo!(
        "Filesystem initialized with {} files ({} initramfs entries processed)",
        files_total,
        entry_count
    );
}

pub fn add_file(name: &'static str, content: &'static str, is_dir: bool) {
    add_file_bytes(name, content.as_bytes(), is_dir);
}

pub fn add_directory(name: &'static str) {
    add_file_bytes(name, &[], true);
}

pub fn add_file_bytes(name: &'static str, content: &'static [u8], is_dir: bool) {
    let mut meta = Metadata::empty();
    meta.size = content.len() as u64;
    meta.blocks = ((meta.size + 511) / 512).max(1);
    meta.nlink = 1;
    meta = meta.with_type(if is_dir {
        FileType::Directory
    } else {
        FileType::Regular
    });
    add_file_with_metadata(name, content, is_dir, meta);
}

pub fn add_file_with_metadata(
    name: &'static str,
    content: &'static [u8],
    is_dir: bool,
    metadata: Metadata,
) {
    register_entry(
        File {
            name,
            content,
            is_dir,
        },
        metadata.normalize(),
    );
}

/// Handle procfs virtual file reads
fn handle_procfs_read(path: &str) -> Option<OpenFile> {
    use super::procfs;
    
    let path = path.trim_start_matches('/');
    
    // Global procfs files
    match path {
        "proc/version" => {
            let (content, len) = procfs::generate_version();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_file_metadata(len as u64),
            });
        }
        "proc/uptime" => {
            let (content, len) = procfs::generate_uptime();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_file_metadata(len as u64),
            });
        }
        "proc/loadavg" => {
            let (content, len) = procfs::generate_loadavg();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_file_metadata(len as u64),
            });
        }
        "proc/meminfo" => {
            let (content, len) = procfs::generate_meminfo();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_file_metadata(len as u64),
            });
        }
        "proc/cpuinfo" => {
            let (content, len) = procfs::generate_cpuinfo();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_file_metadata(len as u64),
            });
        }
        "proc/stat" => {
            let (content, len) = procfs::generate_stat();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_file_metadata(len as u64),
            });
        }
        "proc/filesystems" => {
            let (content, len) = procfs::generate_filesystems();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_file_metadata(len as u64),
            });
        }
        "proc/mounts" => {
            let (content, len) = procfs::generate_mounts();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_file_metadata(len as u64),
            });
        }
        "proc/cmdline" => {
            let (content, len) = procfs::generate_cmdline();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_file_metadata(len as u64),
            });
        }
        "proc/self" => {
            let (content, _len) = procfs::generate_self();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: procfs::proc_link_metadata(),
            });
        }
        _ => {}
    }

    // Handle /proc/self/... by resolving to current PID
    if path.starts_with("proc/self/") {
        let file_path = &path[10..]; // Remove "proc/self/"
        if let Some(current_pid) = crate::scheduler::get_current_pid() {
            match file_path {
                "cmdline" => {
                    if let Some((content, len)) = procfs::generate_pid_cmdline(current_pid) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: procfs::proc_file_metadata(len as u64),
                        });
                    }
                }
                "status" => {
                    if let Some((content, len)) = procfs::generate_pid_status(current_pid) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: procfs::proc_file_metadata(len as u64),
                        });
                    }
                }
                "stat" => {
                    if let Some((content, len)) = procfs::generate_pid_stat(current_pid) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: procfs::proc_file_metadata(len as u64),
                        });
                    }
                }
                "maps" => {
                    if let Some((content, len)) = procfs::generate_pid_maps(current_pid) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: procfs::proc_file_metadata(len as u64),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    
    // Per-process files: /proc/[pid]/...
    if path.starts_with("proc/") {
        let rest = &path[5..]; // Remove "proc/"
        if let Some(slash_pos) = rest.find('/') {
            let pid_str = &rest[..slash_pos];
            let file_path = &rest[slash_pos + 1..];
            
            if let Ok(pid) = pid_str.parse::<u64>() {
                if procfs::pid_exists(pid) {
                    match file_path {
                        "status" => {
                            if let Some((content, len)) = procfs::generate_pid_status(pid) {
                                return Some(OpenFile {
                                    content: FileContent::Inline(content),
                                    metadata: procfs::proc_file_metadata(len as u64),
                                });
                            }
                        }
                        "stat" => {
                            if let Some((content, len)) = procfs::generate_pid_stat(pid) {
                                return Some(OpenFile {
                                    content: FileContent::Inline(content),
                                    metadata: procfs::proc_file_metadata(len as u64),
                                });
                            }
                        }
                        "cmdline" => {
                            if let Some((content, len)) = procfs::generate_pid_cmdline(pid) {
                                return Some(OpenFile {
                                    content: FileContent::Inline(content),
                                    metadata: procfs::proc_file_metadata(len as u64),
                                });
                            }
                        }
                        "maps" => {
                            if let Some((content, len)) = procfs::generate_pid_maps(pid) {
                                return Some(OpenFile {
                                    content: FileContent::Inline(content),
                                    metadata: procfs::proc_file_metadata(len as u64),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    
    None
}

/// Handle sysfs virtual file reads
fn handle_sysfs_read(path: &str) -> Option<OpenFile> {
    use super::sysfs;
    
    let path = path.trim_start_matches('/');
    
    // Kernel info files
    match path {
        "sys/kernel/version" => {
            let (content, len) = sysfs::generate_kernel_version();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/kernel/ostype" => {
            let (content, len) = sysfs::generate_kernel_ostype();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/kernel/osrelease" => {
            let (content, len) = sysfs::generate_kernel_osrelease();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/kernel/hostname" => {
            let (content, len) = sysfs::generate_kernel_hostname();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/kernel/ngroups_max" => {
            let (content, len) = sysfs::generate_kernel_ngroups_max();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/kernel/pid_max" => {
            let (content, len) = sysfs::generate_kernel_pid_max();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/kernel/threads-max" => {
            let (content, len) = sysfs::generate_kernel_threads_max();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/kernel/random/entropy_avail" => {
            let (content, len) = sysfs::generate_random_entropy_avail();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/kernel/random/poolsize" => {
            let (content, len) = sysfs::generate_random_poolsize();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/kernel/random/uuid" => {
            let (content, len) = sysfs::generate_random_uuid();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/power/state" => {
            let (content, len) = sysfs::generate_power_state();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        "sys/power/mem_sleep" => {
            let (content, len) = sysfs::generate_power_mem_sleep();
            return Some(OpenFile {
                content: FileContent::Inline(content),
                metadata: sysfs::sys_file_metadata(len as u64),
            });
        }
        _ => {}
    }
    
    // Block device files: /sys/block/[device]/...
    if path.starts_with("sys/block/") {
        let rest = &path[10..]; // Remove "sys/block/"
        if let Some(slash_pos) = rest.find('/') {
            let device = &rest[..slash_pos];
            let file_path = &rest[slash_pos + 1..];
            
            match file_path {
                "size" => {
                    if let Some((content, len)) = sysfs::generate_block_size(device) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: sysfs::sys_file_metadata(len as u64),
                        });
                    }
                }
                "stat" => {
                    if let Some((content, len)) = sysfs::generate_block_stat(device) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: sysfs::sys_file_metadata(len as u64),
                        });
                    }
                }
                "device/model" => {
                    if let Some((content, len)) = sysfs::generate_block_model(device) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: sysfs::sys_file_metadata(len as u64),
                        });
                    }
                }
                "device/vendor" => {
                    if let Some((content, len)) = sysfs::generate_block_vendor(device) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: sysfs::sys_file_metadata(len as u64),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    
    // Network device files: /sys/class/net/[device]/...
    if path.starts_with("sys/class/net/") {
        let rest = &path[14..]; // Remove "sys/class/net/"
        if let Some(slash_pos) = rest.find('/') {
            let device = &rest[..slash_pos];
            let file_path = &rest[slash_pos + 1..];
            
            match file_path {
                "address" => {
                    if let Some((content, len)) = sysfs::generate_net_address(device) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: sysfs::sys_file_metadata(len as u64),
                        });
                    }
                }
                "mtu" => {
                    if let Some((content, len)) = sysfs::generate_net_mtu(device) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: sysfs::sys_file_metadata(len as u64),
                        });
                    }
                }
                "operstate" => {
                    if let Some((content, len)) = sysfs::generate_net_operstate(device) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: sysfs::sys_file_metadata(len as u64),
                        });
                    }
                }
                "type" => {
                    if let Some((content, len)) = sysfs::generate_net_type(device) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: sysfs::sys_file_metadata(len as u64),
                        });
                    }
                }
                "flags" => {
                    if let Some((content, len)) = sysfs::generate_net_flags(device) {
                        return Some(OpenFile {
                            content: FileContent::Inline(content),
                            metadata: sysfs::sys_file_metadata(len as u64),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    
    None
}

pub fn open(path: &str) -> Option<OpenFile> {
    // Check for procfs paths first
    if path.starts_with("/proc") || path.starts_with("proc") {
        if let Some(result) = handle_procfs_read(path) {
            return Some(result);
        }
    }
    
    // Check for sysfs paths
    if path.starts_with("/sys") || path.starts_with("sys") {
        if let Some(result) = handle_sysfs_read(path) {
            return Some(result);
        }
    }
    
    let (fs, relative) = resolve_mount(path)?;
    fs.read(relative)
}

/// Handle procfs virtual directory stat
fn handle_procfs_stat(path: &str) -> Option<Metadata> {
    use super::procfs;
    
    let path = path.trim_start_matches('/');
    
    // Directory entries
    match path {
        "proc" => return Some(procfs::proc_dir_metadata()),
        "proc/self" => return Some(procfs::proc_link_metadata()),
        _ => {}
    }
    
    // Global procfs files
    match path {
        "proc/version" | "proc/uptime" | "proc/loadavg" | "proc/meminfo" |
        "proc/cpuinfo" | "proc/stat" | "proc/filesystems" | "proc/mounts" |
        "proc/cmdline" => {
            return Some(procfs::proc_file_metadata(0)); // Size determined at read time
        }
        _ => {}
    }
    
    // Per-process directories and files
    if path.starts_with("proc/") {
        let rest = &path[5..];
        
        // Check if it's a PID directory
        if let Some(slash_pos) = rest.find('/') {
            let pid_str = &rest[..slash_pos];
            let file_path = &rest[slash_pos + 1..];
            
            if let Ok(pid) = pid_str.parse::<u64>() {
                if procfs::pid_exists(pid) {
                    match file_path {
                        "status" | "stat" | "cmdline" | "maps" => {
                            return Some(procfs::proc_file_metadata(0));
                        }
                        "fd" => return Some(procfs::proc_dir_metadata()),
                        _ => {}
                    }
                }
            }
        } else {
            // Just a PID directory
            if let Ok(pid) = rest.parse::<u64>() {
                if procfs::pid_exists(pid) {
                    return Some(procfs::proc_dir_metadata());
                }
            }
        }
    }
    
    None
}

/// Handle sysfs virtual directory stat
fn handle_sysfs_stat(path: &str) -> Option<Metadata> {
    use super::sysfs;
    
    let path = path.trim_start_matches('/');
    
    // Directory entries
    match path {
        "sys" | "sys/kernel" | "sys/kernel/random" | "sys/class" |
        "sys/class/tty" | "sys/class/block" | "sys/class/net" |
        "sys/block" | "sys/devices" | "sys/bus" | "sys/fs" | "sys/power" => {
            return Some(sysfs::sys_dir_metadata());
        }
        _ => {}
    }
    
    // Kernel info files
    match path {
        "sys/kernel/version" | "sys/kernel/ostype" | "sys/kernel/osrelease" |
        "sys/kernel/hostname" | "sys/kernel/ngroups_max" | "sys/kernel/pid_max" |
        "sys/kernel/threads-max" | "sys/kernel/random/entropy_avail" |
        "sys/kernel/random/poolsize" | "sys/kernel/random/uuid" |
        "sys/power/state" | "sys/power/mem_sleep" => {
            return Some(sysfs::sys_file_metadata(0));
        }
        _ => {}
    }
    
    // Block device directories and files
    if path.starts_with("sys/block/") {
        let rest = &path[10..];
        for dev in sysfs::get_block_devices() {
            if rest == *dev {
                return Some(sysfs::sys_dir_metadata());
            }
            if rest.starts_with(dev) {
                let suffix = &rest[dev.len()..];
                if suffix.starts_with('/') {
                    let file = &suffix[1..];
                    match file {
                        "size" | "stat" | "device/model" | "device/vendor" => {
                            return Some(sysfs::sys_file_metadata(0));
                        }
                        "device" => return Some(sysfs::sys_dir_metadata()),
                        _ => {}
                    }
                }
            }
        }
    }
    
    // Network device directories and files
    if path.starts_with("sys/class/net/") {
        let rest = &path[14..];
        for dev in sysfs::get_net_devices() {
            if rest == *dev {
                return Some(sysfs::sys_dir_metadata());
            }
            if rest.starts_with(dev) {
                let suffix = &rest[dev.len()..];
                if suffix.starts_with('/') {
                    let file = &suffix[1..];
                    match file {
                        "address" | "mtu" | "operstate" | "type" | "flags" => {
                            return Some(sysfs::sys_file_metadata(0));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    
    // TTY device directories
    if path.starts_with("sys/class/tty/") {
        let rest = &path[14..];
        for dev in sysfs::get_tty_devices() {
            if rest == *dev {
                return Some(sysfs::sys_dir_metadata());
            }
        }
    }
    
    None
}

pub fn stat(path: &str) -> Option<Metadata> {
    let normalized = if path.is_empty() { "/" } else { path };
    if normalized == "/" || normalized.trim_matches('/').is_empty() {
        return Some(default_dir_meta());
    }

    // Check for procfs paths first
    if normalized.starts_with("/proc") || normalized.starts_with("proc") {
        if let Some(meta) = handle_procfs_stat(normalized) {
            return Some(meta);
        }
    }
    
    // Check for sysfs paths
    if normalized.starts_with("/sys") || normalized.starts_with("sys") {
        if let Some(meta) = handle_sysfs_stat(normalized) {
            return Some(meta);
        }
    }

    let (fs, relative) = resolve_mount(normalized)?;
    fs.metadata(relative)
}

/// Handle procfs directory listing
fn handle_procfs_list<F>(path: &str, cb: &mut F) -> bool
where
    F: FnMut(&str, Metadata),
{
    use super::procfs;
    
    let path = path.trim_start_matches('/').trim_end_matches('/');
    
    match path {
        "proc" | "" => {
            // Root /proc directory - list global files and process directories
            cb("version", procfs::proc_file_metadata(0));
            cb("uptime", procfs::proc_file_metadata(0));
            cb("loadavg", procfs::proc_file_metadata(0));
            cb("meminfo", procfs::proc_file_metadata(0));
            cb("cpuinfo", procfs::proc_file_metadata(0));
            cb("stat", procfs::proc_file_metadata(0));
            cb("filesystems", procfs::proc_file_metadata(0));
            cb("mounts", procfs::proc_file_metadata(0));
            cb("cmdline", procfs::proc_file_metadata(0));
            cb("self", procfs::proc_link_metadata());
            
            // List all process directories
            // PIDs are managed by radix tree and can be any value up to MAX_PID
            let pids = procfs::get_all_pids();
            for pid_opt in pids.iter() {
                if let Some(pid) = pid_opt {
                    let pid_str = procfs::get_pid_string(*pid);
                    cb(&pid_str, procfs::proc_dir_metadata());
                }
            }
            return true;
        }
        _ => {}
    }
    
    // Per-process directory listing
    if path.starts_with("proc/") {
        let rest = &path[5..];
        if let Ok(pid) = rest.parse::<u64>() {
            if procfs::pid_exists(pid) {
                cb("status", procfs::proc_file_metadata(0));
                cb("stat", procfs::proc_file_metadata(0));
                cb("cmdline", procfs::proc_file_metadata(0));
                cb("maps", procfs::proc_file_metadata(0));
                cb("fd", procfs::proc_dir_metadata());
                return true;
            }
        }
    }
    
    false
}

/// Handle sysfs directory listing
fn handle_sysfs_list<F>(path: &str, cb: &mut F) -> bool
where
    F: FnMut(&str, Metadata),
{
    use super::sysfs;
    
    let path = path.trim_start_matches('/').trim_end_matches('/');
    
    match path {
        "sys" | "" => {
            cb("kernel", sysfs::sys_dir_metadata());
            cb("class", sysfs::sys_dir_metadata());
            cb("block", sysfs::sys_dir_metadata());
            cb("devices", sysfs::sys_dir_metadata());
            cb("bus", sysfs::sys_dir_metadata());
            cb("fs", sysfs::sys_dir_metadata());
            cb("power", sysfs::sys_dir_metadata());
            return true;
        }
        "sys/kernel" => {
            cb("version", sysfs::sys_file_metadata(0));
            cb("ostype", sysfs::sys_file_metadata(0));
            cb("osrelease", sysfs::sys_file_metadata(0));
            cb("hostname", sysfs::sys_file_metadata(0));
            cb("ngroups_max", sysfs::sys_file_metadata(0));
            cb("pid_max", sysfs::sys_file_metadata(0));
            cb("threads-max", sysfs::sys_file_metadata(0));
            cb("random", sysfs::sys_dir_metadata());
            return true;
        }
        "sys/kernel/random" => {
            cb("entropy_avail", sysfs::sys_file_metadata(0));
            cb("poolsize", sysfs::sys_file_metadata(0));
            cb("uuid", sysfs::sys_file_metadata(0));
            return true;
        }
        "sys/class" => {
            cb("tty", sysfs::sys_dir_metadata());
            cb("block", sysfs::sys_dir_metadata());
            cb("net", sysfs::sys_dir_metadata());
            return true;
        }
        "sys/class/tty" => {
            for dev in sysfs::get_tty_devices() {
                cb(dev, sysfs::sys_dir_metadata());
            }
            return true;
        }
        "sys/class/block" => {
            for dev in sysfs::get_block_devices() {
                cb(dev, sysfs::sys_dir_metadata());
            }
            return true;
        }
        "sys/class/net" => {
            for dev in sysfs::get_net_devices() {
                cb(dev, sysfs::sys_dir_metadata());
            }
            return true;
        }
        "sys/block" => {
            for dev in sysfs::get_block_devices() {
                cb(dev, sysfs::sys_dir_metadata());
            }
            return true;
        }
        "sys/power" => {
            cb("state", sysfs::sys_file_metadata(0));
            cb("mem_sleep", sysfs::sys_file_metadata(0));
            return true;
        }
        _ => {}
    }
    
    // Block device subdirectories
    if path.starts_with("sys/block/") {
        let rest = &path[10..];
        for dev in sysfs::get_block_devices() {
            if rest == *dev {
                cb("size", sysfs::sys_file_metadata(0));
                cb("stat", sysfs::sys_file_metadata(0));
                cb("device", sysfs::sys_dir_metadata());
                return true;
            }
            let dev_device = alloc::format!("{}/device", dev);
            if rest == dev_device {
                cb("model", sysfs::sys_file_metadata(0));
                cb("vendor", sysfs::sys_file_metadata(0));
                return true;
            }
        }
    }
    
    // Network device subdirectories
    if path.starts_with("sys/class/net/") {
        let rest = &path[14..];
        for dev in sysfs::get_net_devices() {
            if rest == *dev {
                cb("address", sysfs::sys_file_metadata(0));
                cb("mtu", sysfs::sys_file_metadata(0));
                cb("operstate", sysfs::sys_file_metadata(0));
                cb("type", sysfs::sys_file_metadata(0));
                cb("flags", sysfs::sys_file_metadata(0));
                return true;
            }
        }
    }
    
    false
}

pub fn list_directory<F>(path: &str, mut cb: F)
where
    F: FnMut(&str, Metadata),
{
    // Check for procfs paths first
    if path.starts_with("/proc") || path.starts_with("proc") || path == "/proc" {
        if handle_procfs_list(path, &mut cb) {
            return;
        }
    }
    
    // Check for sysfs paths
    if path.starts_with("/sys") || path.starts_with("sys") || path == "/sys" {
        if handle_sysfs_list(path, &mut cb) {
            return;
        }
    }
    
    if let Some((fs, relative)) = resolve_mount(path) {
        fs.list(relative, &mut cb);
    }
}

pub fn list_files() -> &'static [Option<File>] {
    let files = FILES.lock();
    // SAFETY: FILES is a static Mutex, its backing array has 'static lifetime.
    // The pointer remains valid and the data is Copy, so this is safe.
    static_slice_from_raw_parts(files.as_ptr(), MAX_FILES)
}

pub fn read_file_bytes(name: &str) -> Option<&'static [u8]> {
    crate::kinfo!("read_file_bytes: opening '{}'", name);
    let opened = open(name)?;
    crate::kinfo!("read_file_bytes: opened, checking content type");

    match opened.content {
        FileContent::Inline(bytes) => {
            crate::kinfo!("read_file_bytes: Inline content, {} bytes", bytes.len());
            Some(bytes)
        }
        FileContent::Ext2Modular(file_ref) => {
            crate::kinfo!("read_file_bytes: Ext2Modular content, inode={}, size={}", file_ref.inode, file_ref.size);
            
            // CRITICAL FIX: Switch to kernel CR3 before accessing EXT2_READ_CACHE
            // The cache buffer is allocated in kernel address space (heap at ~0x7cc0000)
            // which may not be mapped in user process page tables.
            // We must be in kernel CR3 for both buffer allocation AND data reading.
            let kernel_cr3 = crate::paging::kernel_pml4_phys();
            let saved_cr3: u64;
            unsafe {
                core::arch::asm!("mov {}, cr3", out(reg) saved_cr3, options(nomem, nostack));
                if saved_cr3 != kernel_cr3 {
                    crate::kinfo!("read_file_bytes: switching CR3 from {:#x} to kernel {:#x}", saved_cr3, kernel_cr3);
                    core::arch::asm!("mov cr3, {}", in(reg) kernel_cr3, options(nostack));
                }
            }
            
            let size = file_ref.size as usize;
            if size == 0 {
                // Restore CR3 before returning
                unsafe {
                    if saved_cr3 != kernel_cr3 {
                        core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                    }
                }
                return Some(&EMPTY_EXT2_FILE);
            }
            if size > EXT2_READ_CACHE_SIZE {
                crate::kwarn!(
                    "ext2 file '{}' is {} bytes, exceeds {} byte limit",
                    name,
                    size,
                    EXT2_READ_CACHE_SIZE
                );
                // Restore CR3 before returning
                unsafe {
                    if saved_cr3 != kernel_cr3 {
                        core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                    }
                }
                return None;
            }

            // Lock the cache and get/allocate buffer (now in kernel CR3)
            let mut cache = EXT2_READ_CACHE.lock();
            let dest = match cache.get_buffer(size) {
                Some(d) => d,
                None => {
                    drop(cache);
                    // Restore CR3 before returning
                    unsafe {
                        if saved_cr3 != kernel_cr3 {
                            core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                        }
                    }
                    return None;
                }
            };
            crate::kinfo!("read_file_bytes: got buffer at {:#x}, size {}", dest.as_ptr() as u64, size);
            
            let mut read_offset = 0usize;
            while read_offset < size {
                let read = ext2_modular::read_at(&file_ref, read_offset, &mut dest[read_offset..]);
                if read == 0 {
                    crate::kwarn!(
                        "short read while loading '{}' from ext2 (offset {} of {})",
                        name,
                        read_offset,
                        size
                    );
                    drop(cache);
                    // Restore CR3 before returning
                    unsafe {
                        if saved_cr3 != kernel_cr3 {
                            core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                        }
                    }
                    return None;
                }
                read_offset += read;
            }
            
            // Debug: log first 16 bytes
            let n = core::cmp::min(16, size);
            crate::kinfo!("read_file_bytes: complete, first {} bytes: {:02x?}", n, &dest[..n]);

            // SAFETY: We hold the lock, buffer content is valid for 'static
            // as long as the lock protocol is followed by all callers.
            // The buffer persists in the static Mutex.
            let result = unsafe { cache.as_static_slice(size) };
            drop(cache);
            
            // Restore CR3 after all operations complete
            unsafe {
                if saved_cr3 != kernel_cr3 {
                    crate::kinfo!("read_file_bytes: restoring CR3 to {:#x}", saved_cr3);
                    core::arch::asm!("mov cr3, {}", in(reg) saved_cr3, options(nostack));
                }
            }
            
            result
        }
    }
}

pub fn read_file(name: &str) -> Option<&'static str> {
    read_file_bytes(name).and_then(|b| core::str::from_utf8(b).ok())
}

pub fn file_exists(name: &str) -> bool {
    stat(name).is_some()
}

/// Write data to a file
pub fn write_file(path: &str, data: &[u8]) -> Result<usize, &'static str> {
    let (fs, relative) = resolve_mount(path).ok_or("path not found")?;
    fs.write(relative, data)
}

/// Create a new file
pub fn create_file(path: &str) -> Result<(), &'static str> {
    let (fs, relative) = resolve_mount(path).ok_or("path not found")?;
    fs.create(relative)
}

/// Enable write support for ext2 filesystem (if available)
pub fn enable_ext2_write() -> Result<(), &'static str> {
    if !ext2_modular::is_module_loaded() {
        return Err("ext2 module not loaded");
    }
    ext2_modular::enable_write_mode();
    crate::kinfo!("ext2 write mode enabled");
    Ok(())
}

fn register_entry(file: File, metadata: Metadata) {
    let mut files = FILES.lock();
    let mut metas = FILE_METADATA.lock();
    let mut count = FILE_COUNT.lock();

    for idx in 0..*count {
        if let Some(existing) = files[idx] {
            if existing.name == file.name {
                files[idx] = Some(file);
                metas[idx] = Some(metadata);
                return;
            }
        }
    }

    if *count < MAX_FILES {
        files[*count] = Some(file);
        metas[*count] = Some(metadata);
        *count += 1;
    } else {
        crate::kwarn!("File table full, ignoring '{}'", file.name);
    }
}

fn mount(mount_point: &'static str, fs: &'static dyn FileSystem) -> Result<(), MountError> {
    let normalized = if mount_point == "/" {
        "/"
    } else {
        mount_point.trim_end_matches('/')
    };

    let mut mounts = MOUNTS.lock();
    if mounts
        .iter()
        .flatten()
        .any(|entry| entry.mount_point == normalized)
    {
        return Err(MountError::AlreadyMounted);
    }

    for slot in mounts.iter_mut() {
        if slot.is_none() {
            *slot = Some(MountEntry {
                mount_point: normalized,
                fs,
            });
            crate::kinfo!("Mounted {} at {}", fs.name(), normalized);
            return Ok(());
        }
    }

    Err(MountError::TableFull)
}

/// Public interface to mount a filesystem at a given path
pub fn mount_at(
    mount_point: &'static str,
    fs: &'static dyn FileSystem,
) -> Result<(), &'static str> {
    mount(mount_point, fs).map_err(|e| match e {
        MountError::AlreadyMounted => "Already mounted",
        MountError::TableFull => "Mount table full",
    })
}

/// Remount root filesystem (used for pivot_root)
/// This replaces the root mount point with a new filesystem
pub fn remount_root(fs: &'static dyn FileSystem) -> Result<(), &'static str> {
    let mut mounts = MOUNTS.lock();

    // Find and replace the root mount
    for entry in mounts.iter_mut() {
        if let Some(mount) = entry {
            if mount.mount_point == "/" {
                crate::kinfo!("Replacing root mount: {} -> {}", mount.fs.name(), fs.name());
                mount.fs = fs;
                return Ok(());
            }
        }
    }

    Err("Root not mounted")
}

#[derive(Debug)]
enum MountError {
    AlreadyMounted,
    TableFull,
}

fn resolve_mount(path: &str) -> Option<(&'static dyn FileSystem, &str)> {
    if path.is_empty() {
        return None;
    }

    let is_absolute = path.starts_with('/');
    let mut best: Option<(&'static dyn FileSystem, &str, usize)> = None;
    let mounts = MOUNTS.lock();

    // Debug: log available mounts
    crate::kinfo!("resolve_mount('{}') - checking mounts:", path);
    for entry in mounts.iter().flatten() {
        crate::kinfo!("  mount: '{}' -> fs={}", entry.mount_point, entry.fs.name());
    }

    for entry in mounts.iter().flatten() {
        if entry.mount_point == "/" {
            let relative = if is_absolute {
                path.trim_start_matches('/')
            } else {
                path
            };
            if best.map_or(true, |(_, _, len)| len <= 1) {
                best = Some((entry.fs, relative, 1));
            }
        } else if is_absolute && path.starts_with(entry.mount_point) {
            let rest = &path[entry.mount_point.len()..];
            let relative = rest.trim_start_matches('/');
            let mp_len = entry.mount_point.len();
            if best.map_or(true, |(_, _, len)| mp_len > len) {
                best = Some((entry.fs, relative, mp_len));
            }
        }
    }

    if let Some((fs, rel, _)) = best {
        crate::kinfo!("resolve_mount: resolved to fs='{}', rel='{}'", fs.name(), rel);
    } else {
        crate::kinfo!("resolve_mount: no mount found for '{}'", path);
    }

    best.map(|(fs, rel, _)| (fs, rel))
}

fn find_file_index(name: &str) -> Option<usize> {
    let files = FILES.lock();
    let count = *FILE_COUNT.lock();
    let target = name.trim_matches('/');

    // Debug: log the lookup
    if target == "bin/sh" || target.ends_with("/sh") {
        crate::kinfo!(
            "find_file_index: looking for '{}' (trimmed: '{}')",
            name,
            target
        );
        crate::kinfo!("find_file_index: file_count = {}", count);
        for idx in 0..count {
            if let Some(file) = files[idx] {
                crate::kinfo!("  [{}]: '{}'", idx, file.name);
            }
        }
    }

    for idx in 0..count {
        if let Some(file) = files[idx] {
            if file.name == target {
                return Some(idx);
            }
        }
    }
    None
}

impl FileSystem for InitramfsFilesystem {
    fn name(&self) -> &'static str {
        "initramfs"
    }

    fn read(&self, path: &str) -> Option<OpenFile> {
        let idx = find_file_index(path)?;
        let files = FILES.lock();
        let metas = FILE_METADATA.lock();
        let file = files[idx]?;
        let meta = metas[idx].unwrap_or_else(Metadata::empty);
        Some(OpenFile {
            content: FileContent::Inline(file.content),
            metadata: meta,
        })
    }

    fn metadata(&self, path: &str) -> Option<Metadata> {
        let idx = find_file_index(path)?;
        let metas = FILE_METADATA.lock();
        metas[idx]
    }

    fn list(&self, path: &str, cb: &mut dyn FnMut(&str, Metadata)) {
        let target = normalize_component(path);
        let files_guard = FILES.lock();
        let metas_guard = FILE_METADATA.lock();
        let count = *FILE_COUNT.lock();

        let mut emitted: [Option<ListedEntry>; MAX_FILES] = [None; MAX_FILES];

        for idx in 0..count {
            let Some(file) = files_guard[idx] else {
                continue;
            };
            let meta = metas_guard[idx].unwrap_or_else(Metadata::empty);
            if target.is_empty() {
                let (child, remainder) = split_first_component(file.name);
                if child.is_empty() {
                    continue;
                }
                let child_meta = if remainder.is_some() && !file.is_dir {
                    default_dir_meta()
                } else {
                    meta
                };
                emit_unique(&mut emitted, child, child_meta);
            } else {
                let name = file.name;
                if name == target {
                    continue;
                }
                if !name.starts_with(target) {
                    continue;
                }
                let suffix = &name[target.len()..];
                if suffix.is_empty() {
                    continue;
                }
                if !suffix.starts_with('/') {
                    continue;
                }
                let suffix = &suffix[1..];
                if suffix.is_empty() {
                    continue;
                }
                let (child, remainder) = split_first_component(suffix);
                if child.is_empty() {
                    continue;
                }
                let child_meta = if remainder.is_some() {
                    default_dir_meta()
                } else if file.is_dir {
                    meta
                } else {
                    meta
                };
                emit_unique(&mut emitted, child, child_meta);
            }
        }

        drop(metas_guard);
        drop(files_guard);

        for entry in emitted.iter().flatten() {
            cb(entry.name_str(), entry.metadata);
        }
    }

    fn write(&self, _path: &str, _data: &[u8]) -> Result<usize, &'static str> {
        Err("initramfs is read-only")
    }

    fn create(&self, _path: &str) -> Result<(), &'static str> {
        Err("cannot create files in initramfs")
    }
}
