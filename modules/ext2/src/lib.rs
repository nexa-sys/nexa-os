//! ext2 Filesystem Kernel Module for NexaOS
//!
//! This is a loadable kernel module (.nkm) that provides ext2 filesystem support.
//! It is loaded from initramfs during boot and dynamically linked to the kernel.
//!
//! # Module Entry Points
//!
//! - `module_init`: Called when module is loaded
//! - `module_exit`: Called when module is unloaded
//!
//! # Kernel API Usage
//!
//! This module uses the kernel's exported symbol table for:
//! - Logging (kmod_log_*)
//! - Memory allocation (kmod_alloc, kmod_dealloc)
//! - Filesystem registration (kmod_register_fs)

#![no_std]
#![allow(dead_code)]

use core::cmp;

// ============================================================================
// Module Metadata
// ============================================================================

/// Module name
pub const MODULE_NAME: &[u8] = b"ext2\0";
/// Module version
pub const MODULE_VERSION: &[u8] = b"1.0.0\0";
/// Module description
pub const MODULE_DESC: &[u8] = b"ext2 filesystem driver for NexaOS\0";
/// Module type (1 = Filesystem)
pub const MODULE_TYPE: u8 = 1;
/// Module license (GPL-compatible, doesn't taint kernel)
pub const MODULE_LICENSE: &[u8] = b"MIT\0";
/// Module author
pub const MODULE_AUTHOR: &[u8] = b"NexaOS Team\0";
/// Source version (in-tree module)
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
    fn kmod_register_fs(name: *const u8, name_len: usize, init_fn: usize, lookup_fn: usize) -> i32;
    fn kmod_unregister_fs(name: *const u8, name_len: usize) -> i32;
    fn kmod_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
    fn kmod_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8;
    fn kmod_memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
    fn kmod_strlen(s: *const u8) -> usize;
    fn kmod_strcmp(s1: *const u8, s2: *const u8) -> i32;
    fn kmod_strncmp(s1: *const u8, s2: *const u8, n: usize) -> i32;
    fn kmod_spinlock_init(lock: *mut u64);
    fn kmod_spinlock_lock(lock: *mut u64);
    fn kmod_spinlock_unlock(lock: *mut u64);
    fn kmod_spinlock_trylock(lock: *mut u64) -> i32;
    
    // New ext2 modular API
    fn kmod_ext2_register(ops: *const Ext2ModuleOps) -> i32;
    fn kmod_ext2_unregister() -> i32;
    
    // Block device API (for reading from real block devices)
    fn kmod_blk_read_bytes(device_index: usize, offset: u64, buf: *mut u8, len: usize) -> i64;
    fn kmod_blk_write_bytes(device_index: usize, offset: u64, buf: *const u8, len: usize) -> i64;
    fn kmod_blk_device_count() -> usize;
    fn kmod_blk_find_rootfs() -> i32;
}

// ============================================================================
// Logging helpers
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

// Helper to print a u64 as hex string (DEBUG level)
fn log_hex(prefix: &[u8], value: u64) {
    // Format: "prefix0x1234567890ABCDEF\n"
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
    
    unsafe { kmod_log_debug(buf.as_ptr(), pos); }
}

// ============================================================================
// Constants
// ============================================================================

const SUPERBLOCK_OFFSET: usize = 1024;
const SUPERBLOCK_SIZE: usize = 1024;
const EXT2_SUPER_MAGIC: u16 = 0xEF53;
const EXT2_NDIR_BLOCKS: usize = 12;
const EXT2_IND_BLOCK: usize = 12;
const EXT2_BLOCK_POINTER_SIZE: usize = core::mem::size_of::<u32>();

// ============================================================================
// Module Operations Table (for kmod_ext2_register)
// ============================================================================

/// Opaque handle type for the kernel
pub type Ext2Handle = *mut u8;

/// File reference handle for the kernel (matches kernel's FileRefHandle)
#[repr(C)]
pub struct FileRefHandle {
    pub fs: Ext2Handle,
    pub inode: u32,
    pub size: u64,
    pub mode: u16,
    pub blocks: u64,
    pub mtime: u64,
    pub nlink: u32,
    pub uid: u16,
    pub gid: u16,
}

/// Directory entry callback type
pub type DirEntryCallback = extern "C" fn(name: *const u8, name_len: usize, inode: u32, file_type: u8, ctx: *mut u8);

/// Filesystem statistics
#[repr(C)]
#[derive(Default)]
pub struct Ext2Stats {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub mtime: u32,
}

/// Module operations table - must match kernel's Ext2ModuleOps
#[repr(C)]
pub struct Ext2ModuleOps {
    pub new: Option<extern "C" fn(image: *const u8, size: usize) -> Ext2Handle>,
    pub destroy: Option<extern "C" fn(handle: Ext2Handle)>,
    pub lookup: Option<extern "C" fn(handle: Ext2Handle, path: *const u8, path_len: usize, out: *mut FileRefHandle) -> i32>,
    pub read_at: Option<extern "C" fn(file: *const FileRefHandle, offset: usize, buf: *mut u8, len: usize) -> i32>,
    pub write_at: Option<extern "C" fn(file: *const FileRefHandle, offset: usize, data: *const u8, len: usize) -> i32>,
    pub list_dir: Option<extern "C" fn(handle: Ext2Handle, path: *const u8, path_len: usize, cb: DirEntryCallback, ctx: *mut u8) -> i32>,
    pub get_stats: Option<extern "C" fn(handle: Ext2Handle, stats: *mut Ext2Stats) -> i32>,
    pub set_writable: Option<extern "C" fn(writable: bool)>,
    pub is_writable: Option<extern "C" fn() -> bool>,
    pub create_file: Option<extern "C" fn(handle: Ext2Handle, path: *const u8, path_len: usize, mode: u16) -> i32>,
}

// ============================================================================
// Error types
// ============================================================================

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub enum Ext2Error {
    BadMagic = 1,
    ImageTooSmall = 2,
    UnsupportedInodeSize = 3,
    InvalidGroupDescriptor = 4,
    InodeOutOfBounds = 5,
    NoSpaceLeft = 6,
    ReadOnly = 7,
    InvalidInode = 8,
    InvalidBlockNumber = 9,
}

// ============================================================================
// File type enumeration
// ============================================================================

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum FileType {
    Regular = 1,
    Directory = 2,
    Symlink = 3,
    Character = 4,
    Block = 5,
    Fifo = 6,
    Socket = 7,
    Unknown = 255,
}

// ============================================================================
// Metadata structure
// ============================================================================

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct Metadata {
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub mtime: u64,
    pub file_type: FileType,
    pub nlink: u32,
    pub blocks: u64,
}

// ============================================================================
// Internal structures
// ============================================================================

#[derive(Debug, Clone)]
struct Superblock {
    inodes_count: u32,
    blocks_count: u32,
    first_data_block: u32,
    log_block_size: u32,
    blocks_per_group: u32,
    inodes_per_group: u32,
    magic: u16,
    rev_level: u32,
    first_ino: u32,
    inode_size: u16,
    mtime: u32,
}

#[derive(Debug, Clone)]
struct GroupDescriptor {
    block_bitmap: u32,
    inode_bitmap: u32,
    inode_table_block: u32,
    free_blocks_count: u16,
    free_inodes_count: u16,
    used_dirs_count: u16,
}

#[derive(Debug, Clone)]
struct Inode {
    mode: u16,
    uid: u16,
    size_lo: u32,
    atime: u32,
    ctime: u32,
    mtime: u32,
    dtime: u32,
    gid: u16,
    links_count: u16,
    blocks_lo: u32,
    flags: u32,
    block: [u32; 15],
    file_acl: u32,
    size_high: u32,
}

// ============================================================================
// Ext2 Filesystem structure
// ============================================================================

/// Block device based ext2 filesystem
#[repr(C)]
pub struct Ext2Filesystem {
    /// Block device index (from kmod_blk_find_rootfs)
    block_device_index: usize,
    /// Total size of the filesystem in bytes
    total_size: u64,
    /// Filesystem block size (1024, 2048, or 4096)
    block_size: usize,
    /// Inode size (128 or 256)
    inode_size: usize,
    /// Inodes per block group
    inodes_per_group: u32,
    /// Blocks per block group
    blocks_per_group: u32,
    /// Total number of block groups
    total_groups: u32,
    /// First data block (0 for >1K blocks, 1 for 1K blocks)
    first_data_block: u32,
    /// Total inode count from superblock
    sb_inodes_count: u32,
    /// Total block count from superblock
    sb_blocks_count: u32,
    /// Magic number (should be 0xEF53)
    sb_magic: u16,
    /// Revision level
    sb_rev_level: u32,
}

// ============================================================================
// File reference for open files
// ============================================================================

#[repr(C)]
pub struct FileRef {
    fs: *const Ext2Filesystem,
    inode: u32,
    size: u64,
    mode: u16,
    blocks: u64,
    mtime: u64,
    nlink: u32,
    uid: u16,
    gid: u16,
}

// ============================================================================
// Module state
// ============================================================================

static mut EXT2_FS_INSTANCE: Option<Ext2Filesystem> = None;
static mut MODULE_INITIALIZED: bool = false;
static mut EXT2_WRITABLE: bool = false;

/// Module entry point table - used to prevent linker from removing entry functions
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

// ============================================================================
// Module entry points
// ============================================================================

/// Module initialization - called when module is loaded by the kernel
/// 
/// This is the main entry point that the kernel calls after loading
/// and relocating the module.
#[no_mangle]
#[inline(never)]
pub extern "C" fn module_init() -> i32 {
    mod_info!(b"ext2 module: initializing...");
    
    unsafe {
        if MODULE_INITIALIZED {
            mod_warn!(b"ext2 module: already initialized");
            return 0;
        }
        
        // Create the operations table
        let ops = Ext2ModuleOps {
            new: Some(ext2_mod_new),
            destroy: Some(ext2_mod_destroy),
            lookup: Some(ext2_mod_lookup),
            read_at: Some(ext2_mod_read_at),
            write_at: Some(ext2_mod_write_at),
            list_dir: Some(ext2_mod_list_dir),
            get_stats: Some(ext2_mod_get_stats),
            set_writable: Some(ext2_mod_set_writable),
            is_writable: Some(ext2_mod_is_writable),
            create_file: Some(ext2_mod_create_file),
        };
        
        // Register with the kernel's ext2 modular layer
        let result = kmod_ext2_register(&ops);
        
        if result != 0 {
            mod_error!(b"ext2 module: failed to register with kernel");
            return -1;
        }
        
        MODULE_INITIALIZED = true;
    }
    
    mod_info!(b"ext2 module: initialized successfully");
    0
}

/// Module cleanup - called when module is unloaded
#[no_mangle]
#[inline(never)]
pub extern "C" fn module_exit() -> i32 {
    mod_info!(b"ext2 module: unloading...");
    
    unsafe {
        if !MODULE_INITIALIZED {
            return 0;
        }
        
        // Unregister from kernel
        kmod_ext2_unregister();
        
        // Clean up instance
        EXT2_FS_INSTANCE = None;
        MODULE_INITIALIZED = false;
    }
    
    mod_info!(b"ext2 module: unloaded");
    0
}

// Legacy entry points for compatibility
#[no_mangle]
pub extern "C" fn ext2_module_init() -> i32 {
    module_init()
}

#[no_mangle]
pub extern "C" fn ext2_module_exit() -> i32 {
    module_exit()
}

// ============================================================================
// Module Operation Functions (for Ext2ModuleOps table)
// ============================================================================

/// Create new ext2 filesystem instance from image
extern "C" fn ext2_mod_new(image: *const u8, size: usize) -> Ext2Handle {
    let fs = ext2_new(image, size);
    fs as Ext2Handle
}

/// Destroy ext2 filesystem instance
extern "C" fn ext2_mod_destroy(_handle: Ext2Handle) {
    // Clean up the global instance
    unsafe {
        EXT2_FS_INSTANCE = None;
    }
}

/// Lookup file by path
extern "C" fn ext2_mod_lookup(
    handle: Ext2Handle,
    path: *const u8,
    path_len: usize,
    out: *mut FileRefHandle,
) -> i32 {
    if handle.is_null() || path.is_null() || out.is_null() {
        return -1;
    }

    let fs = handle as *const Ext2Filesystem;
    let mut file_ref = FileRef {
        fs,
        inode: 0,
        size: 0,
        mode: 0,
        blocks: 0,
        mtime: 0,
        nlink: 0,
        uid: 0,
        gid: 0,
    };

    let result = ext2_lookup(fs, path, path_len, &mut file_ref as *mut FileRef);
    if result != 0 {
        return result;
    }

    // Convert FileRef to FileRefHandle
    unsafe {
        (*out).fs = handle;
        (*out).inode = file_ref.inode;
        (*out).size = file_ref.size;
        (*out).mode = file_ref.mode;
        (*out).blocks = file_ref.blocks;
        (*out).mtime = file_ref.mtime;
        (*out).nlink = file_ref.nlink;
        (*out).uid = file_ref.uid;
        (*out).gid = file_ref.gid;
    }

    0
}

/// Read file content at offset
extern "C" fn ext2_mod_read_at(
    file: *const FileRefHandle,
    offset: usize,
    buf: *mut u8,
    len: usize,
) -> i32 {
    if file.is_null() || buf.is_null() {
        return -1;
    }

    let file_ref = unsafe { &*file };
    let fs = file_ref.fs as *const Ext2Filesystem;
    
    let bytes_read = ext2_read(fs, file_ref.inode, offset, buf, len);
    bytes_read as i32
}

/// List directory entries
extern "C" fn ext2_mod_list_dir(
    handle: Ext2Handle,
    path: *const u8,
    path_len: usize,
    cb: DirEntryCallback,
    ctx: *mut u8,
) -> i32 {
    if handle.is_null() || path.is_null() {
        return -1;
    }

    let fs = unsafe { &*(handle as *const Ext2Filesystem) };
    let path_bytes = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    // Look up the directory
    let file_ref = match fs.lookup_internal(path_str) {
        Some(fr) => fr,
        None => return -1,
    };

    // Check if it's a directory
    let mode = file_ref.mode;
    if (mode & 0o170000) != 0o040000 {
        return -1; // Not a directory
    }

    // Load the inode and iterate entries
    let inode = match fs.load_inode(file_ref.inode) {
        Ok(i) => i,
        Err(_) => return -1,
    };

    fs.for_each_dir_entry(&inode, |name, entry_inode, file_type| {
        cb(name.as_ptr(), name.len(), entry_inode, file_type, ctx);
    });

    0
}

/// Get filesystem statistics
extern "C" fn ext2_mod_get_stats(handle: Ext2Handle, stats: *mut Ext2Stats) -> i32 {
    if handle.is_null() || stats.is_null() {
        return -1;
    }

    let fs = unsafe { &*(handle as *const Ext2Filesystem) };
    
    unsafe {
        (*stats).inodes_count = fs.sb_inodes_count;
        (*stats).blocks_count = fs.sb_blocks_count;
        (*stats).free_blocks_count = 0; // Would need to parse group descriptors
        (*stats).free_inodes_count = 0;
        (*stats).block_size = fs.block_size as u32;
        (*stats).blocks_per_group = fs.blocks_per_group;
        (*stats).inodes_per_group = fs.inodes_per_group;
        (*stats).mtime = 0;
    }

    0
}

/// Write data to a file at offset
extern "C" fn ext2_mod_write_at(
    file: *const FileRefHandle,
    offset: usize,
    data: *const u8,
    len: usize,
) -> i32 {
    if file.is_null() || data.is_null() {
        return -1;
    }

    // Check if write mode is enabled
    unsafe {
        if !EXT2_WRITABLE {
            mod_warn!(b"ext2: write denied - filesystem is read-only");
            return Ext2Error::ReadOnly as i32;
        }
    }

    let file_ref = unsafe { &*file };
    let fs = file_ref.fs as *const Ext2Filesystem;
    
    if fs.is_null() {
        return -1;
    }

    let data_slice = unsafe { core::slice::from_raw_parts(data, len) };
    
    match ext2_write_internal(fs, file_ref.inode, offset, data_slice) {
        Ok(bytes_written) => bytes_written as i32,
        Err(e) => -(e as i32),
    }
}

/// Set writable mode for the filesystem
extern "C" fn ext2_mod_set_writable(writable: bool) {
    unsafe {
        EXT2_WRITABLE = writable;
        if writable {
            mod_info!(b"ext2: write mode ENABLED");
        } else {
            mod_info!(b"ext2: write mode DISABLED");
        }
    }
}

/// Check if filesystem is writable
extern "C" fn ext2_mod_is_writable() -> bool {
    unsafe { EXT2_WRITABLE }
}

/// Create a file in the filesystem
extern "C" fn ext2_mod_create_file(
    handle: Ext2Handle,
    path: *const u8,
    path_len: usize,
    mode: u16,
) -> i32 {
    if handle.is_null() || path.is_null() {
        mod_error!(b"ext2_mod_create_file: null handle or path");
        return -1;
    }
    
    unsafe {
        if !EXT2_WRITABLE {
            mod_error!(b"ext2_mod_create_file: filesystem is read-only");
            return -1;
        }
    }
    
    let fs = unsafe { &*(handle as *const Ext2Filesystem) };
    let path_bytes = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => {
            mod_error!(b"ext2_mod_create_file: invalid UTF-8 path");
            return -1;
        }
    };
    
    match fs.create_file(path_str, mode) {
        Ok(inode) => {
            mod_info!(b"ext2_mod_create_file: success");
            inode as i32
        }
        Err(_e) => {
            mod_error!(b"ext2_mod_create_file: failed");
            -1
        }
    }
}

// ============================================================================
// Filesystem operations (exported to kernel)
// ============================================================================

/// Initialize ext2 filesystem from disk image
/// Legacy: kept for API compatibility but now uses block device
#[no_mangle]
pub extern "C" fn ext2_fs_init(_image: *const u8, _size: usize) -> *mut Ext2Filesystem {
    // Now always use block device - ignore image pointer
    ext2_new_from_block_device()
}

/// Legacy: kept for API compatibility but now uses block device
#[no_mangle]
pub extern "C" fn ext2_new(_image: *const u8, _size: usize) -> *mut Ext2Filesystem {
    // Now always use block device - ignore image pointer
    ext2_new_from_block_device()
}

/// Create a new ext2 filesystem from block device
fn ext2_new_from_block_device() -> *mut Ext2Filesystem {
    mod_info!(b"ext2: initializing from block device");
    
    // Find rootfs block device
    let device_index = unsafe { kmod_blk_find_rootfs() };
    if device_index < 0 {
        mod_error!(b"ext2: no block device found");
        return core::ptr::null_mut();
    }
    let device_index = device_index as usize;
    
    // Read superblock from block device
    let mut sb_buf = [0u8; SUPERBLOCK_SIZE];
    let result = unsafe {
        kmod_blk_read_bytes(device_index, SUPERBLOCK_OFFSET as u64, sb_buf.as_mut_ptr(), SUPERBLOCK_SIZE)
    };
    if result < 0 {
        mod_error!(b"ext2: failed to read superblock from block device");
        return core::ptr::null_mut();
    }

    // Parse superblock
    let superblock = match Superblock::parse(&sb_buf) {
        Ok(sb) => sb,
        Err(_) => {
            mod_error!(b"ext2: failed to parse superblock");
            return core::ptr::null_mut();
        }
    };

    if superblock.magic != EXT2_SUPER_MAGIC {
        mod_error!(b"ext2: bad magic number");
        return core::ptr::null_mut();
    }

    let block_size = 1024usize << superblock.log_block_size;
    let inode_size = if superblock.rev_level >= 1 && superblock.inode_size != 0 {
        superblock.inode_size as usize
    } else {
        128
    };

    if inode_size > SUPERBLOCK_SIZE {
        mod_error!(b"ext2: unsupported inode size");
        return core::ptr::null_mut();
    }

    let total_groups =
        (superblock.blocks_count + superblock.blocks_per_group - 1) / superblock.blocks_per_group;
    
    // Calculate total size
    let total_size = superblock.blocks_count as u64 * block_size as u64;

    let fs = Ext2Filesystem {
        block_device_index: device_index,
        total_size,
        block_size,
        inode_size,
        inodes_per_group: superblock.inodes_per_group,
        blocks_per_group: superblock.blocks_per_group,
        total_groups,
        first_data_block: superblock.first_data_block,
        sb_inodes_count: superblock.inodes_count,
        sb_blocks_count: superblock.blocks_count,
        sb_magic: superblock.magic,
        sb_rev_level: superblock.rev_level,
    };

    mod_info!(b"ext2: filesystem initialized from block device");

    // Store in global instance
    unsafe {
        EXT2_FS_INSTANCE = Some(fs);
        EXT2_FS_INSTANCE.as_mut().map(|f| f as *mut Ext2Filesystem).unwrap_or(core::ptr::null_mut())
    }
}

/// Lookup a file by path
#[no_mangle]
pub extern "C" fn ext2_lookup(
    fs: *const Ext2Filesystem,
    path: *const u8,
    path_len: usize,
    out_ref: *mut FileRef,
) -> i32 {
    // Debug: log entry
    let debug_msg = b"ext2_lookup called";
    unsafe { kmod_log_info(debug_msg.as_ptr(), debug_msg.len()); }
    
    if fs.is_null() {
        let msg = b"ext2_lookup: fs is null";
        unsafe { kmod_log_error(msg.as_ptr(), msg.len()); }
        return -1;
    }
    if path.is_null() {
        let msg = b"ext2_lookup: path is null";
        unsafe { kmod_log_error(msg.as_ptr(), msg.len()); }
        return -1;
    }
    if out_ref.is_null() {
        let msg = b"ext2_lookup: out_ref is null";
        unsafe { kmod_log_error(msg.as_ptr(), msg.len()); }
        return -1;
    }

    let fs = unsafe { &*fs };
    let path_bytes = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => {
            let msg = b"ext2_lookup: invalid UTF-8";
            unsafe { kmod_log_error(msg.as_ptr(), msg.len()); }
            return -1;
        }
    };

    match fs.lookup_internal(path_str) {
        Some(file_ref) => {
            unsafe { *out_ref = file_ref };
            0
        }
        None => {
            // Debug: log failure
            let msg = b"ext2_lookup: lookup_internal returned None";
            unsafe { kmod_log_warn(msg.as_ptr(), msg.len()); }
            -1
        }
    }
}

/// Read from a file
#[no_mangle]
pub extern "C" fn ext2_read(
    fs: *const Ext2Filesystem,
    inode: u32,
    offset: usize,
    buf: *mut u8,
    buf_len: usize,
) -> isize {
    if fs.is_null() || buf.is_null() {
        return -1;
    }

    let fs = unsafe { &*fs };
    let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf, buf_len) };
    
    fs.read_file_internal(inode, offset, buf_slice) as isize
}

/// Get file metadata
#[no_mangle]
pub extern "C" fn ext2_metadata(file_ref: *const FileRef, out_meta: *mut Metadata) -> i32 {
    if file_ref.is_null() || out_meta.is_null() {
        return -1;
    }

    let file_ref = unsafe { &*file_ref };
    let file_type = match file_ref.mode & 0o170000 {
        0o040000 => FileType::Directory,
        0o100000 => FileType::Regular,
        0o120000 => FileType::Symlink,
        0o020000 => FileType::Character,
        0o060000 => FileType::Block,
        0o010000 => FileType::Fifo,
        0o140000 => FileType::Socket,
        _ => FileType::Unknown,
    };

    let meta = Metadata {
        mode: file_ref.mode,
        uid: file_ref.uid as u32,
        gid: file_ref.gid as u32,
        size: file_ref.size,
        mtime: file_ref.mtime,
        file_type,
        nlink: file_ref.nlink,
        blocks: file_ref.blocks,
    };

    unsafe { *out_meta = meta };
    0
}

/// Internal write function
fn ext2_write_internal(
    fs: *const Ext2Filesystem,
    inode_num: u32,
    offset: usize,
    data: &[u8],
) -> Result<usize, Ext2Error> {
    if fs.is_null() || data.is_empty() {
        return Ok(0);
    }

    let fs = unsafe { &*fs };
    
    // Load the inode
    let inode = fs.load_inode(inode_num)?;
    
    // Check if it's a regular file
    if !inode.is_regular_file() {
        mod_warn!(b"ext2_write: not a regular file");
        return Err(Ext2Error::InvalidInode);
    }
    
    let file_size = inode.size() as usize;
    let block_size = fs.block_size;
    
    // Calculate the end position of the write
    let write_end = offset + data.len();
    
    // Determine how much we can actually write
    // For existing blocks: can write within already-allocated blocks
    // Calculate maximum writable size based on allocated blocks
    let max_block_index = if file_size == 0 {
        0
    } else {
        (file_size + block_size - 1) / block_size
    };
    let max_writable_offset = max_block_index * block_size;
    
    // If offset is beyond allocated blocks, we can't write (no block allocation yet)
    if offset >= max_writable_offset && file_size > 0 {
        mod_warn!(b"ext2_write: offset beyond allocated blocks");
        return Ok(0);
    }
    
    // Calculate actual write length - limited by allocated blocks
    let actual_write_len = if write_end > max_writable_offset {
        if max_writable_offset > offset {
            max_writable_offset - offset
        } else {
            data.len().min(block_size) // For empty files, try to write at least one block
        }
    } else {
        data.len()
    };
    
    if actual_write_len == 0 {
        return Ok(0);
    }
    
    let mut written = 0usize;
    let mut current_offset = offset;
    
    while written < actual_write_len {
        let block_index = current_offset / block_size;
        let within_block = current_offset % block_size;
        
        // Get the block number for this index
        let block_number = match fs.block_number(&inode, block_index) {
            Some(bn) if bn != 0 => bn,
            _ => {
                mod_warn!(b"ext2_write: no block allocated at index");
                break;
            }
        };
        
        // Calculate how much to write in this block
        let available_in_block = block_size - within_block;
        let remaining = actual_write_len - written;
        let to_write = cmp::min(available_in_block, remaining);
        
        // Write to the block
        match fs.write_block(block_number, within_block, &data[written..written + to_write]) {
            Ok(_) => {
                written += to_write;
                current_offset += to_write;
            }
            Err(e) => {
                mod_error!(b"ext2_write: write_block failed");
                return Err(e);
            }
        }
    }
    
    // Update file size if we extended beyond the original size
    let new_size = offset + written;
    if new_size > file_size {
        if let Err(_) = fs.update_inode_size(inode_num, new_size as u64) {
            mod_warn!(b"ext2_write: failed to update inode size");
            // Continue anyway - data was written
        }
    }
    
    mod_info!(b"ext2_write: successfully wrote bytes");
    Ok(written)
}

// ============================================================================
// Internal implementation
// ============================================================================

impl Ext2Filesystem {
    /// Read bytes from block device at given offset
    fn read_bytes(&self, offset: u64, buf: &mut [u8]) -> bool {
        if buf.is_empty() {
            return true;
        }
        let result = unsafe {
            kmod_blk_read_bytes(self.block_device_index, offset, buf.as_mut_ptr(), buf.len())
        };
        if result < 0 {
            mod_error!(b"ext2: block device read failed");
            return false;
        }
        true
    }
    
    /// Write bytes to block device at given offset
    fn write_bytes(&self, offset: u64, buf: &[u8]) -> bool {
        if buf.is_empty() {
            return true;
        }
        let result = unsafe {
            kmod_blk_write_bytes(self.block_device_index, offset, buf.as_ptr(), buf.len())
        };
        if result < 0 {
            mod_error!(b"ext2: block device write failed");
            return false;
        }
        true
    }

    fn lookup_internal(&self, path: &str) -> Option<FileRef> {
        let trimmed = path.trim_matches('/');
        let mut inode_number = 2u32; // root inode

        if trimmed.is_empty() {
            return self.file_ref_from_inode(inode_number);
        }

        // Debug: check magic number by reading from block device
        let magic_offset = 1024 + 56;
        let mut magic_buf = [0u8; 2];
        if !self.read_bytes(magic_offset as u64, &mut magic_buf) {
            mod_error!(b"lookup_internal: failed to read magic");
            return None;
        }
        let magic = u16::from_le_bytes(magic_buf);
        if magic != 0xEF53 {
            mod_error!(b"lookup_internal: bad ext2 magic!");
            return None;
        }

        let mut inode = match self.load_inode(inode_number) {
            Ok(i) => i,
            Err(_) => {
                let msg = b"lookup_internal: failed to load root inode";
                unsafe { kmod_log_error(msg.as_ptr(), msg.len()); }
                return None;
            }
        };

        for segment in trimmed.split('/') {
            if segment.is_empty() {
                continue;
            }
            let next_inode = match self.find_in_directory(&inode, segment) {
                Some(n) => n,
                None => {
                    // Path segment not found - this is normal for non-existent paths
                    // Don't log as warning to avoid noise during font directory scanning
                    return None;
                }
            };
            inode_number = next_inode;
            inode = match self.load_inode(inode_number) {
                Ok(i) => i,
                Err(_) => {
                    let msg = b"lookup_internal: failed to load inode";
                    unsafe { kmod_log_error(msg.as_ptr(), msg.len()); }
                    return None;
                }
            };
        }

        self.file_ref_from_inode(inode_number)
    }

    fn file_ref_from_inode(&self, inode: u32) -> Option<FileRef> {
        let node = self.load_inode(inode).ok()?;
        Some(FileRef {
            fs: self as *const Ext2Filesystem,
            inode,
            size: node.size(),
            mode: node.mode,
            blocks: node.blocks(),
            uid: node.uid,
            gid: node.gid,
            mtime: node.mtime as u64,
            nlink: node.links_count as u32,
        })
    }

    fn load_inode(&self, inode: u32) -> Result<Inode, Ext2Error> {
        if inode == 0 {
            return Err(Ext2Error::InodeOutOfBounds);
        }

        let inode_index = inode - 1;
        let group = inode_index / self.inodes_per_group;
        if group >= self.total_groups {
            return Err(Ext2Error::InodeOutOfBounds);
        }
        let index_in_group = inode_index % self.inodes_per_group;
        let desc = self.group_descriptor(group)?;
        let inode_table_block = desc.inode_table_block;
        let inode_table_offset = inode_table_block as usize * self.block_size;
        let inode_offset = inode_table_offset + index_in_group as usize * self.inode_size;

        // Read inode data from block device
        let mut inode_buf = [0u8; 256]; // Max inode size
        let read_size = self.inode_size.min(256);
        if !self.read_bytes(inode_offset as u64, &mut inode_buf[..read_size]) {
            return Err(Ext2Error::ImageTooSmall);
        }
        
        Inode::parse(&inode_buf[..read_size])
    }

    fn group_descriptor(&self, group: u32) -> Result<GroupDescriptor, Ext2Error> {
        let desc_size = 32usize;
        let superblock_block = if self.block_size == 1024 { 1 } else { 0 };
        let table_block = superblock_block + 1;
        let table_offset = table_block * self.block_size;
        let offset = table_offset + group as usize * desc_size;

        // Read group descriptor from block device
        let mut data = [0u8; 32];
        if !self.read_bytes(offset as u64, &mut data) {
            return Err(Ext2Error::InvalidGroupDescriptor);
        }

        Ok(GroupDescriptor {
            block_bitmap: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            inode_bitmap: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            inode_table_block: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            free_blocks_count: u16::from_le_bytes([data[12], data[13]]),
            free_inodes_count: u16::from_le_bytes([data[14], data[15]]),
            used_dirs_count: u16::from_le_bytes([data[16], data[17]]),
        })
    }
    
    /// Get offset in image for a group descriptor
    fn group_descriptor_offset(&self, group: u32) -> usize {
        let desc_size = 32usize;
        let superblock_block = if self.block_size == 1024 { 1 } else { 0 };
        let table_block = superblock_block + 1;
        let table_offset = table_block * self.block_size;
        table_offset + group as usize * desc_size
    }

    /// Read a block into provided buffer, returns slice of the buffer on success
    fn read_block_to_buf<'a>(&self, block_number: u32, buf: &'a mut [u8]) -> Option<&'a [u8]> {
        if block_number == 0 {
            return None;
        }
        if buf.len() < self.block_size {
            return None;
        }
        let offset = block_number as u64 * self.block_size as u64;
        
        if !self.read_bytes(offset, &mut buf[..self.block_size]) {
            mod_warn!(b"read_block_to_buf: block read failed");
            return None;
        }
        Some(&buf[..self.block_size])
    }

    /// Write data to a block at a specific offset within the block
    fn write_block(&self, block_number: u32, offset_in_block: usize, data: &[u8]) -> Result<(), Ext2Error> {
        if block_number == 0 {
            return Err(Ext2Error::InvalidBlockNumber);
        }
        
        if offset_in_block + data.len() > self.block_size {
            return Err(Ext2Error::InvalidBlockNumber);
        }
        
        let block_offset = block_number as u64 * self.block_size as u64;
        let write_offset = block_offset + offset_in_block as u64;
        
        if write_offset + data.len() as u64 > self.total_size {
            return Err(Ext2Error::ImageTooSmall);
        }
        
        // Write to block device
        if !self.write_bytes(write_offset, data) {
            return Err(Ext2Error::ImageTooSmall);
        }
        
        Ok(())
    }

    /// Allocate a free inode from the filesystem
    /// Returns the inode number (1-based) on success
    fn allocate_inode(&self) -> Result<u32, Ext2Error> {
        let mut block_buf = [0u8; 4096]; // Max block size
        
        for group in 0..self.total_groups {
            let desc = self.group_descriptor(group)?;
            if desc.free_inodes_count == 0 {
                continue;
            }
            
            // Read the inode bitmap
            let bitmap_block = desc.inode_bitmap;
            let bitmap = match self.read_block_to_buf(bitmap_block, &mut block_buf) {
                Some(b) => b,
                None => continue,
            };
            
            // Find a free bit in the bitmap
            for byte_idx in 0..bitmap.len() {
                let byte = bitmap[byte_idx];
                if byte == 0xFF {
                    continue; // All bits set
                }
                
                for bit in 0..8 {
                    if (byte & (1 << bit)) == 0 {
                        // Found a free inode
                        let inode_in_group = (byte_idx * 8 + bit) as u32;
                        if inode_in_group >= self.inodes_per_group {
                            break;
                        }
                        
                        // Set the bit in bitmap and write back
                        let new_byte = byte | (1 << bit);
                        let bitmap_offset = bitmap_block as u64 * self.block_size as u64 + byte_idx as u64;
                        if !self.write_bytes(bitmap_offset, &[new_byte]) {
                            return Err(Ext2Error::ImageTooSmall);
                        }
                        
                        // Update free count in group descriptor
                        let gd_offset = self.group_descriptor_offset(group) as u64;
                        let new_free = desc.free_inodes_count - 1;
                        let new_free_bytes = new_free.to_le_bytes();
                        if !self.write_bytes(gd_offset + 14, &new_free_bytes) {
                            return Err(Ext2Error::ImageTooSmall);
                        }
                        
                        // Calculate actual inode number (1-based)
                        let inode_num = group * self.inodes_per_group + inode_in_group + 1;
                        return Ok(inode_num);
                    }
                }
            }
        }
        
        Err(Ext2Error::NoSpaceLeft)
    }
    
    /// Allocate a free block from the filesystem
    /// Returns the block number on success
    fn allocate_block(&self) -> Result<u32, Ext2Error> {
        let mut block_buf = [0u8; 4096]; // Max block size
        
        for group in 0..self.total_groups {
            let desc = self.group_descriptor(group)?;
            if desc.free_blocks_count == 0 {
                continue;
            }
            
            // Read the block bitmap
            let bitmap_block = desc.block_bitmap;
            let bitmap = match self.read_block_to_buf(bitmap_block, &mut block_buf) {
                Some(b) => b,
                None => continue,
            };
            
            // Find a free bit in the bitmap
            for byte_idx in 0..bitmap.len() {
                let byte = bitmap[byte_idx];
                if byte == 0xFF {
                    continue; // All bits set
                }
                
                for bit in 0..8 {
                    if (byte & (1 << bit)) == 0 {
                        // Found a free block
                        let block_in_group = (byte_idx * 8 + bit) as u32;
                        if block_in_group >= self.blocks_per_group {
                            break;
                        }
                        
                        // Set the bit in bitmap and write back
                        let new_byte = byte | (1 << bit);
                        let bitmap_offset = bitmap_block as u64 * self.block_size as u64 + byte_idx as u64;
                        if !self.write_bytes(bitmap_offset, &[new_byte]) {
                            return Err(Ext2Error::ImageTooSmall);
                        }
                        
                        // Update free count in group descriptor
                        let gd_offset = self.group_descriptor_offset(group) as u64;
                        let new_free = desc.free_blocks_count - 1;
                        let new_free_bytes = new_free.to_le_bytes();
                        if !self.write_bytes(gd_offset + 12, &new_free_bytes) {
                            return Err(Ext2Error::ImageTooSmall);
                        }
                        
                        // Calculate actual block number
                        let block_num = group * self.blocks_per_group + block_in_group + self.first_data_block;
                        
                        // Zero out the newly allocated block
                        let zero_buf = [0u8; 4096];
                        let block_offset = block_num as u64 * self.block_size as u64;
                        if !self.write_bytes(block_offset, &zero_buf[..self.block_size]) {
                            return Err(Ext2Error::ImageTooSmall);
                        }
                        
                        return Ok(block_num);
                    }
                }
            }
        }
        
        Err(Ext2Error::NoSpaceLeft)
    }
    
    /// Write a new inode to disk
    fn write_inode(&self, inode_num: u32, inode: &Inode) -> Result<(), Ext2Error> {
        if inode_num == 0 {
            return Err(Ext2Error::InodeOutOfBounds);
        }

        let inode_index = inode_num - 1;
        let group = inode_index / self.inodes_per_group;
        if group >= self.total_groups {
            return Err(Ext2Error::InodeOutOfBounds);
        }
        let index_in_group = inode_index % self.inodes_per_group;
        let desc = self.group_descriptor(group)?;
        let inode_table_block = desc.inode_table_block;
        let inode_table_offset = inode_table_block as usize * self.block_size;
        let inode_offset = inode_table_offset + index_in_group as usize * self.inode_size;

        // Serialize inode to bytes (128 bytes minimum)
        let mut buf = [0u8; 128];
        buf[0..2].copy_from_slice(&inode.mode.to_le_bytes());
        buf[2..4].copy_from_slice(&inode.uid.to_le_bytes());
        buf[4..8].copy_from_slice(&inode.size_lo.to_le_bytes());
        buf[8..12].copy_from_slice(&inode.atime.to_le_bytes());
        buf[12..16].copy_from_slice(&inode.ctime.to_le_bytes());
        buf[16..20].copy_from_slice(&inode.mtime.to_le_bytes());
        buf[20..24].copy_from_slice(&inode.dtime.to_le_bytes());
        buf[24..26].copy_from_slice(&inode.gid.to_le_bytes());
        buf[26..28].copy_from_slice(&inode.links_count.to_le_bytes());
        buf[28..32].copy_from_slice(&inode.blocks_lo.to_le_bytes());
        buf[32..36].copy_from_slice(&inode.flags.to_le_bytes());
        // Skip osd1 (offset 36-40)
        for i in 0..15 {
            let start = 40 + i * 4;
            buf[start..start + 4].copy_from_slice(&inode.block[i].to_le_bytes());
        }
        // Offset 100-104: generation
        buf[104..108].copy_from_slice(&inode.file_acl.to_le_bytes());
        buf[108..112].copy_from_slice(&inode.size_high.to_le_bytes());

        // Write inode to block device
        if !self.write_bytes(inode_offset as u64, &buf) {
            return Err(Ext2Error::ImageTooSmall);
        }

        Ok(())
    }
    
    /// Add a directory entry to a directory inode
    fn add_dir_entry(&self, dir_inode_num: u32, name: &str, new_inode: u32, file_type: u8) -> Result<(), Ext2Error> {
        let dir_inode = self.load_inode(dir_inode_num)?;
        
        // Entry size: 8 bytes header + name (4-byte aligned)
        let name_len = name.len();
        let rec_len = ((8 + name_len + 3) / 4) * 4; // 4-byte aligned
        
        // Iterate through directory blocks to find space
        let block_size = self.block_size;
        let num_blocks = (dir_inode.size() as usize + block_size - 1) / block_size;
        
        let mut block_buf = [0u8; 4096]; // Max block size
        
        for block_idx in 0..num_blocks {
            let block_num = match self.block_number(&dir_inode, block_idx) {
                Some(bn) if bn != 0 => bn,
                _ => continue,
            };
            
            let block_offset = block_num as u64 * block_size as u64;
            
            // Read block from device
            if !self.read_bytes(block_offset, &mut block_buf[..block_size]) {
                continue;
            }
            
            // Scan through directory entries in this block
            let mut offset = 0usize;
            while offset + 8 <= block_size {
                let entry_inode = u32::from_le_bytes([
                    block_buf[offset],
                    block_buf[offset + 1],
                    block_buf[offset + 2],
                    block_buf[offset + 3],
                ]);
                let entry_rec_len = u16::from_le_bytes([
                    block_buf[offset + 4],
                    block_buf[offset + 5],
                ]) as usize;
                let entry_name_len = block_buf[offset + 6] as usize;
                
                if entry_rec_len == 0 || entry_rec_len < 8 {
                    break;
                }
                
                // Calculate the actual size needed for the existing entry
                let actual_len = if entry_inode == 0 {
                    8 // Empty entry
                } else {
                    ((8 + entry_name_len + 3) / 4) * 4
                };
                
                // Check if there's room after this entry for our new one
                let free_space = entry_rec_len - actual_len;
                if free_space >= rec_len {
                    // Split this entry
                    let new_entry_offset = offset + actual_len;
                    
                    // Update current entry's rec_len in buffer
                    let new_cur_rec_len = actual_len as u16;
                    block_buf[offset + 4..offset + 6].copy_from_slice(&new_cur_rec_len.to_le_bytes());
                    
                    // Write new entry to buffer
                    let new_rec_len = free_space as u16;
                    block_buf[new_entry_offset..new_entry_offset + 4].copy_from_slice(&new_inode.to_le_bytes());
                    block_buf[new_entry_offset + 4..new_entry_offset + 6].copy_from_slice(&new_rec_len.to_le_bytes());
                    block_buf[new_entry_offset + 6] = name_len as u8;
                    block_buf[new_entry_offset + 7] = file_type;
                    block_buf[new_entry_offset + 8..new_entry_offset + 8 + name_len].copy_from_slice(name.as_bytes());
                    
                    // Write block back to device
                    if !self.write_bytes(block_offset, &block_buf[..block_size]) {
                        return Err(Ext2Error::ImageTooSmall);
                    }
                    
                    return Ok(());
                }
                
                offset += entry_rec_len;
            }
        }
        
        // Need to allocate a new block for the directory
        let new_block = self.allocate_block()?;
        
        // Add block to directory inode
        let next_block_idx = num_blocks;
        if next_block_idx < EXT2_NDIR_BLOCKS {
            // Update inode's direct block pointer
            let inode_index = dir_inode_num - 1;
            let group = inode_index / self.inodes_per_group;
            let index_in_group = inode_index % self.inodes_per_group;
            let desc = self.group_descriptor(group)?;
            let inode_table_offset = desc.inode_table_block as usize * self.block_size;
            let inode_offset = inode_table_offset + index_in_group as usize * self.inode_size;
            let block_ptr_offset = (inode_offset + 40 + next_block_idx * 4) as u64;
            
            // Write block pointer to inode
            if !self.write_bytes(block_ptr_offset, &new_block.to_le_bytes()) {
                return Err(Ext2Error::ImageTooSmall);
            }
            
            // Update directory size
            let new_size = ((next_block_idx + 1) * block_size) as u64;
            self.update_inode_size(dir_inode_num, new_size)?;
            
            // Write the new entry at the start of the new block
            let block_offset = new_block as u64 * block_size as u64;
            let remaining = block_size as u16;
            
            // Prepare new directory entry
            let mut entry_buf = [0u8; 4096];
            entry_buf[0..4].copy_from_slice(&new_inode.to_le_bytes());
            entry_buf[4..6].copy_from_slice(&remaining.to_le_bytes());
            entry_buf[6] = name_len as u8;
            entry_buf[7] = file_type;
            entry_buf[8..8 + name_len].copy_from_slice(name.as_bytes());
            
            if !self.write_bytes(block_offset, &entry_buf[..block_size]) {
                return Err(Ext2Error::ImageTooSmall);
            }
            
            return Ok(());
        }
        
        // Indirect blocks not implemented for directory extension
        Err(Ext2Error::NoSpaceLeft)
    }
    
    /// Create a new file in the filesystem
    pub fn create_file(&self, path: &str, mode: u16) -> Result<u32, Ext2Error> {
        // Split path into directory and filename
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Err(Ext2Error::InvalidInode);
        }
        
        let (parent_path, filename) = match path.rfind('/') {
            Some(pos) => (&path[..pos], &path[pos + 1..]),
            None => ("", path),
        };
        
        if filename.is_empty() || filename.len() > 255 {
            return Err(Ext2Error::InvalidInode);
        }
        
        // Find parent directory
        let parent_inode_num = if parent_path.is_empty() {
            2 // Root directory
        } else {
            match self.lookup_internal(parent_path) {
                Some(file_ref) => file_ref.inode,
                None => return Err(Ext2Error::InvalidInode),
            }
        };
        
        // Check if file already exists
        let parent_inode = self.load_inode(parent_inode_num)?;
        if self.find_in_directory(&parent_inode, filename).is_some() {
            // File already exists - that's okay, return success
            mod_info!(b"create_file: file already exists");
            return Ok(0);
        }
        
        // Allocate a new inode
        let new_inode_num = self.allocate_inode()?;
        
        // Allocate an initial block for the file (optional, but allows immediate writes)
        let initial_block = self.allocate_block()?;
        
        // Get current timestamp (use 0 for now since we don't have time syscalls in module)
        let timestamp = 0u32;
        
        // Create the inode
        let file_mode = 0o100000 | (mode & 0o7777); // Regular file + permissions
        let new_inode = Inode {
            mode: file_mode,
            uid: 0,
            size_lo: 0,
            atime: timestamp,
            ctime: timestamp,
            mtime: timestamp,
            dtime: 0,
            gid: 0,
            links_count: 1,
            blocks_lo: (self.block_size / 512) as u32, // blocks in 512-byte units
            flags: 0,
            block: {
                let mut blocks = [0u32; 15];
                blocks[0] = initial_block;
                blocks
            },
            file_acl: 0,
            size_high: 0,
        };
        
        // Write the inode
        self.write_inode(new_inode_num, &new_inode)?;
        
        // Add directory entry
        // File type 1 = regular file
        self.add_dir_entry(parent_inode_num, filename, new_inode_num, 1)?;
        
        mod_info!(b"create_file: file created successfully");
        Ok(new_inode_num)
    }

    /// Update the size field in an inode
    fn update_inode_size(&self, inode_num: u32, new_size: u64) -> Result<(), Ext2Error> {
        if inode_num == 0 {
            return Err(Ext2Error::InodeOutOfBounds);
        }

        let inode_index = inode_num - 1;
        let group = inode_index / self.inodes_per_group;
        if group >= self.total_groups {
            return Err(Ext2Error::InodeOutOfBounds);
        }
        let index_in_group = inode_index % self.inodes_per_group;
        let desc = self.group_descriptor(group)?;
        let inode_table_block = desc.inode_table_block;
        let inode_table_offset = inode_table_block as usize * self.block_size;
        let inode_offset = inode_table_offset + index_in_group as usize * self.inode_size;

        // Write the low 32 bits of size at offset 4 in the inode
        let size_lo = (new_size & 0xFFFFFFFF) as u32;
        let size_lo_bytes = size_lo.to_le_bytes();
        
        // Write the high 32 bits of size at offset 108 in the inode (for large files)
        let size_hi = ((new_size >> 32) & 0xFFFFFFFF) as u32;
        let size_hi_bytes = size_hi.to_le_bytes();

        // Write size_lo at inode+4
        if !self.write_bytes((inode_offset + 4) as u64, &size_lo_bytes) {
            return Err(Ext2Error::ImageTooSmall);
        }
        
        // Write size_hi at inode+108 (for ext2 revision 1+)
        if self.sb_rev_level >= 1 {
            if !self.write_bytes((inode_offset + 108) as u64, &size_hi_bytes) {
                return Err(Ext2Error::ImageTooSmall);
            }
        }

        Ok(())
    }

    fn find_in_directory(&self, inode: &Inode, target: &str) -> Option<u32> {
        let mut found = None;
        self.for_each_dir_entry(inode, |name, inode_num, _file_type| {
            if name == target {
                found = Some(inode_num);
            }
        });
        found
    }

    fn for_each_dir_entry<F>(&self, inode: &Inode, mut cb: F)
    where
        F: FnMut(&str, u32, u8),
    {
        let block_size = self.block_size;
        let mut block_count = 0u32;
        let mut block_buf = [0u8; 4096]; // Max block size
        
        for &block in inode.block.iter().take(EXT2_NDIR_BLOCKS) {
            if block == 0 {
                continue;
            }
            block_count += 1;
            
            if let Some(data) = self.read_block_to_buf(block, &mut block_buf) {
                let mut offset = 0usize;
                let mut entry_count = 0u32;
                
                while offset + 8 <= block_size {
                    let entry_inode = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    let rec_len = u16::from_le_bytes([data[offset + 4], data[offset + 5]]) as usize;
                    if rec_len == 0 {
                        break;
                    }
                    let name_len = data[offset + 6] as usize;
                    let file_type = data[offset + 7];
                    if offset + rec_len > block_size || offset + 8 + name_len > block_size {
                        break;
                    }
                    if entry_inode != 0 && name_len > 0 {
                        entry_count += 1;
                        if let Ok(name) =
                            core::str::from_utf8(&data[offset + 8..offset + 8 + name_len])
                        {
                            cb(name, entry_inode, file_type);
                        }
                    }
                    offset += rec_len;
                }
                
                // Debug: log if no entries found in block
                if entry_count == 0 {
                    let msg = b"for_each_dir_entry: no valid entries in block";
                    unsafe { kmod_log_warn(msg.as_ptr(), msg.len()); }
                }
            } else {
                // Debug: log if read_block failed
                let msg = b"for_each_dir_entry: read_block returned None";
                unsafe { kmod_log_warn(msg.as_ptr(), msg.len()); }
            }
        }
        
        // Debug: log total blocks processed
        if block_count == 0 {
            let msg = b"for_each_dir_entry: no blocks in directory inode!";
            unsafe { kmod_log_warn(msg.as_ptr(), msg.len()); }
        }
    }

    fn read_file_internal(&self, inode_num: u32, offset: usize, buf: &mut [u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }

        let inode = match self.load_inode(inode_num) {
            Ok(inode) => inode,
            Err(_) => {
                mod_error!(b"read_file_internal: load_inode failed");
                return 0;
            }
        };
        
        if !inode.is_regular_file() {
            mod_error!(b"read_file_internal: not a regular file");
            return 0;
        }

        let file_size = inode.size() as usize;
        if offset >= file_size {
            return 0;
        }

        let mut remaining = cmp::min(buf.len(), file_size - offset);
        let mut written = 0usize;
        let block_size = self.block_size;
        let mut current_offset = offset;
        let mut block_buf = [0u8; 4096]; // Max block size

        while remaining > 0 {
            let block_index = current_offset / block_size;
            let within_block = current_offset % block_size;
            let Some(block_number) = self.block_number(&inode, block_index) else {
                mod_error!(b"read_file_internal: block_number returned None");
                break;
            };
            if block_number == 0 {
                break;
            }
            
            let Some(block) = self.read_block_to_buf(block_number, &mut block_buf) else {
                break;
            };
            
            let available = cmp::min(block_size - within_block, remaining);
            buf[written..written + available]
                .copy_from_slice(&block[within_block..within_block + available]);
            
            written += available;
            remaining -= available;
            current_offset += available;
        }
        
        written
    }

    fn block_number(&self, inode: &Inode, index: usize) -> Option<u32> {
        let pointers_per_block = self.block_size / EXT2_BLOCK_POINTER_SIZE;

        // Direct blocks (0-11)
        if index < EXT2_NDIR_BLOCKS {
            return Some(inode.block[index]);
        }

        let ind_index = index - EXT2_NDIR_BLOCKS;
        let mut block_buf = [0u8; 4096]; // Max block size

        // Single indirect block (12): covers pointers_per_block entries
        if ind_index < pointers_per_block {
            let indirect_block = inode.block[EXT2_IND_BLOCK];
            if indirect_block == 0 {
                return None;
            }

            let raw = self.read_block_to_buf(indirect_block, &mut block_buf)?;
            let offset = ind_index * EXT2_BLOCK_POINTER_SIZE;
            if offset + EXT2_BLOCK_POINTER_SIZE > raw.len() {
                return None;
            }

            return Some(u32::from_le_bytes([
                raw[offset],
                raw[offset + 1],
                raw[offset + 2],
                raw[offset + 3],
            ]));
        }

        // Double indirect block (13): covers pointers_per_block^2 entries
        let dind_index = ind_index - pointers_per_block;
        let dind_capacity = pointers_per_block * pointers_per_block;
        
        if dind_index < dind_capacity {
            const EXT2_DIND_BLOCK: usize = 13;
            let double_indirect_block = inode.block[EXT2_DIND_BLOCK];
            if double_indirect_block == 0 {
                return None;
            }

            // First level: which indirect block?
            let first_level_index = dind_index / pointers_per_block;
            // Second level: which pointer within that indirect block?
            let second_level_index = dind_index % pointers_per_block;

            // Read the double indirect block to get the indirect block pointer
            let mut dind_buf = [0u8; 4096];
            let dind_raw = self.read_block_to_buf(double_indirect_block, &mut dind_buf)?;
            let first_offset = first_level_index * EXT2_BLOCK_POINTER_SIZE;
            if first_offset + EXT2_BLOCK_POINTER_SIZE > dind_raw.len() {
                return None;
            }

            let indirect_block = u32::from_le_bytes([
                dind_raw[first_offset],
                dind_raw[first_offset + 1],
                dind_raw[first_offset + 2],
                dind_raw[first_offset + 3],
            ]);

            if indirect_block == 0 {
                return None;
            }

            // Read the indirect block to get the data block pointer
            let mut ind_buf = [0u8; 4096];
            let ind_raw = self.read_block_to_buf(indirect_block, &mut ind_buf)?;
            let second_offset = second_level_index * EXT2_BLOCK_POINTER_SIZE;
            if second_offset + EXT2_BLOCK_POINTER_SIZE > ind_raw.len() {
                return None;
            }

            return Some(u32::from_le_bytes([
                ind_raw[second_offset],
                ind_raw[second_offset + 1],
                ind_raw[second_offset + 2],
                ind_raw[second_offset + 3],
            ]));
        }

        // Triple indirect block (14) not implemented yet
        // Would cover pointers_per_block^3 entries
        None
    }
}

impl Superblock {
    fn parse(raw: &[u8]) -> Result<Self, Ext2Error> {
        if raw.len() < 92 {
            return Err(Ext2Error::ImageTooSmall);
        }

        Ok(Self {
            inodes_count: u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
            blocks_count: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            first_data_block: u32::from_le_bytes([raw[20], raw[21], raw[22], raw[23]]),
            log_block_size: u32::from_le_bytes([raw[24], raw[25], raw[26], raw[27]]),
            blocks_per_group: u32::from_le_bytes([raw[32], raw[33], raw[34], raw[35]]),
            inodes_per_group: u32::from_le_bytes([raw[40], raw[41], raw[42], raw[43]]),
            magic: u16::from_le_bytes([raw[56], raw[57]]),
            rev_level: u32::from_le_bytes([raw[76], raw[77], raw[78], raw[79]]),
            first_ino: u32::from_le_bytes([raw[84], raw[85], raw[86], raw[87]]),
            inode_size: u16::from_le_bytes([raw[88], raw[89]]),
            mtime: u32::from_le_bytes([raw[44], raw[45], raw[46], raw[47]]),
        })
    }
}

impl Inode {
    fn parse(raw: &[u8]) -> Result<Self, Ext2Error> {
        if raw.len() < 128 {
            return Err(Ext2Error::ImageTooSmall);
        }

        let mut block = [0u32; 15];
        for i in 0..15 {
            let start = 40 + i * 4;
            block[i] =
                u32::from_le_bytes([raw[start], raw[start + 1], raw[start + 2], raw[start + 3]]);
        }

        Ok(Self {
            mode: u16::from_le_bytes([raw[0], raw[1]]),
            uid: u16::from_le_bytes([raw[2], raw[3]]),
            size_lo: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            atime: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
            ctime: u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]),
            mtime: u32::from_le_bytes([raw[16], raw[17], raw[18], raw[19]]),
            dtime: u32::from_le_bytes([raw[20], raw[21], raw[22], raw[23]]),
            gid: u16::from_le_bytes([raw[24], raw[25]]),
            links_count: u16::from_le_bytes([raw[26], raw[27]]),
            blocks_lo: u32::from_le_bytes([raw[28], raw[29], raw[30], raw[31]]),
            flags: u32::from_le_bytes([raw[32], raw[33], raw[34], raw[35]]),
            block,
            file_acl: u32::from_le_bytes([raw[104], raw[105], raw[106], raw[107]]),
            size_high: u32::from_le_bytes([raw[108], raw[109], raw[110], raw[111]]),
        })
    }

    fn size(&self) -> u64 {
        ((self.size_high as u64) << 32) | self.size_lo as u64
    }

    fn blocks(&self) -> u64 {
        self.blocks_lo as u64
    }

    fn is_regular_file(&self) -> bool {
        (self.mode & 0o170000) == 0o100000
    }
}

// ============================================================================
// Panic handler for module
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
