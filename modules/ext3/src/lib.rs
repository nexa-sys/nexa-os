//! ext3 Filesystem Kernel Module for NexaOS
//!
//! This is a loadable kernel module (.nkm) that provides ext3 filesystem support.
//! ext3 is an extension of ext2 with journaling support.
//!
//! # Module Dependencies
//!
//! This module depends on ext2 module being loaded first, as ext3 is backward
//! compatible with ext2 and reuses most of ext2's functionality.
//!
//! # ext3 Features (over ext2)
//!
//! - Journaling for filesystem consistency
//! - Support for COMPAT_HAS_JOURNAL feature flag
//! - Journal inode (typically inode 8)
//! - Three journaling modes: journal, ordered (default), writeback
//!
//! # Superblock Features
//!
//! ext3 uses the same superblock as ext2 but with additional feature flags:
//! - s_feature_compat & EXT3_FEATURE_COMPAT_HAS_JOURNAL (0x0004)
//! - s_journal_inum: inode number of journal file
//! - s_journal_dev: device number of journal device (0 for internal journal)

#![no_std]
#![allow(dead_code)]

use core::cmp;

// ============================================================================
// Module Metadata
// ============================================================================

/// Module name
pub const MODULE_NAME: &[u8] = b"ext3\0";
/// Module version
pub const MODULE_VERSION: &[u8] = b"1.0.0\0";
/// Module description
pub const MODULE_DESC: &[u8] = b"ext3 filesystem driver for NexaOS (journaling ext2)\0";
/// Module type (1 = Filesystem)
pub const MODULE_TYPE: u8 = 1;
/// Module license
pub const MODULE_LICENSE: &[u8] = b"MIT\0";
/// Module author
pub const MODULE_AUTHOR: &[u8] = b"NexaOS Team\0";
/// Source version
pub const MODULE_SRCVERSION: &[u8] = b"in-tree\0";
/// Module dependencies
pub const MODULE_DEPENDS: &[u8] = b"ext2\0";

// ============================================================================
// Kernel API declarations
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
    
    // ext3 modular API (register with kernel)
    fn kmod_ext3_register(ops: *const Ext3ModuleOps) -> i32;
    fn kmod_ext3_unregister() -> i32;
    
    // Check if ext2 module is loaded (dependency)
    fn kmod_is_module_loaded(name: *const u8, name_len: usize) -> i32;
    
    // Block device API
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

// ============================================================================
// Constants
// ============================================================================

const SUPERBLOCK_OFFSET: usize = 1024;
const SUPERBLOCK_SIZE: usize = 1024;
const EXT2_SUPER_MAGIC: u16 = 0xEF53;

// ext3 feature flags
const EXT3_FEATURE_COMPAT_HAS_JOURNAL: u32 = 0x0004;
const EXT3_FEATURE_INCOMPAT_JOURNAL_DEV: u32 = 0x0008;
const EXT3_FEATURE_INCOMPAT_RECOVER: u32 = 0x0004;

// Journal magic
const JBD_MAGIC_NUMBER: u32 = 0xC03B3998;

// Journal inode (standard location)
const EXT3_JOURNAL_INO: u32 = 8;

// Journaling modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JournalMode {
    /// Full data journaling - all data and metadata journaled
    Journal = 0,
    /// Ordered mode (default) - metadata journaled, data written before metadata commit
    Ordered = 1,
    /// Writeback mode - only metadata journaled, data can be written out of order
    Writeback = 2,
}

// ============================================================================
// Module Operations Table
// ============================================================================

/// Opaque handle type
pub type Ext3Handle = *mut u8;

/// File reference handle
#[repr(C)]
pub struct FileRefHandle {
    pub fs: Ext3Handle,
    pub inode: u32,
    pub size: u64,
    pub mode: u16,
    pub blocks: u64,
    pub mtime: u64,
    pub nlink: u32,
    pub uid: u16,
    pub gid: u16,
}

/// Directory entry callback
pub type DirEntryCallback = extern "C" fn(name: *const u8, name_len: usize, inode: u32, file_type: u8, ctx: *mut u8);

/// Filesystem statistics
#[repr(C)]
#[derive(Default)]
pub struct Ext3Stats {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub mtime: u32,
    // ext3 specific
    pub journal_blocks: u32,
    pub journal_mode: u8,
}

/// Module operations table
#[repr(C)]
pub struct Ext3ModuleOps {
    pub new: Option<extern "C" fn(image: *const u8, size: usize) -> Ext3Handle>,
    pub destroy: Option<extern "C" fn(handle: Ext3Handle)>,
    pub lookup: Option<extern "C" fn(handle: Ext3Handle, path: *const u8, path_len: usize, out: *mut FileRefHandle) -> i32>,
    pub read_at: Option<extern "C" fn(file: *const FileRefHandle, offset: usize, buf: *mut u8, len: usize) -> i32>,
    pub write_at: Option<extern "C" fn(file: *const FileRefHandle, offset: usize, data: *const u8, len: usize) -> i32>,
    pub list_dir: Option<extern "C" fn(handle: Ext3Handle, path: *const u8, path_len: usize, cb: DirEntryCallback, ctx: *mut u8) -> i32>,
    pub get_stats: Option<extern "C" fn(handle: Ext3Handle, stats: *mut Ext3Stats) -> i32>,
    pub set_writable: Option<extern "C" fn(writable: bool)>,
    pub is_writable: Option<extern "C" fn() -> bool>,
    pub create_file: Option<extern "C" fn(handle: Ext3Handle, path: *const u8, path_len: usize, mode: u16) -> i32>,
    // ext3 specific operations
    pub journal_sync: Option<extern "C" fn(handle: Ext3Handle) -> i32>,
    pub set_journal_mode: Option<extern "C" fn(mode: u8) -> i32>,
}

// ============================================================================
// Error types
// ============================================================================

#[derive(Debug, Copy, Clone)]
#[repr(i32)]
pub enum Ext3Error {
    BadMagic = 1,
    ImageTooSmall = 2,
    UnsupportedInodeSize = 3,
    InvalidGroupDescriptor = 4,
    InodeOutOfBounds = 5,
    NoSpaceLeft = 6,
    ReadOnly = 7,
    InvalidInode = 8,
    InvalidBlockNumber = 9,
    // ext3 specific errors
    NoJournal = 20,
    JournalCorrupt = 21,
    JournalRecoveryNeeded = 22,
    DependencyNotLoaded = 30,
}

// ============================================================================
// Internal structures
// ============================================================================

/// ext3 superblock extension (additional fields beyond ext2)
#[derive(Debug, Clone)]
struct Ext3Superblock {
    // ext2 base fields
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
    // ext3 specific fields
    feature_compat: u32,
    feature_incompat: u32,
    feature_ro_compat: u32,
    journal_inum: u32,
    journal_dev: u32,
}

impl Ext3Superblock {
    fn parse(raw: &[u8]) -> Result<Self, Ext3Error> {
        if raw.len() < 256 {
            return Err(Ext3Error::ImageTooSmall);
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
            // ext3 specific fields at offset 0x5C (92), 0x60 (96), 0x64 (100)
            feature_compat: u32::from_le_bytes([raw[92], raw[93], raw[94], raw[95]]),
            feature_incompat: u32::from_le_bytes([raw[96], raw[97], raw[98], raw[99]]),
            feature_ro_compat: u32::from_le_bytes([raw[100], raw[101], raw[102], raw[103]]),
            // Journal fields at offset 0xE0 (224) and 0xE4 (228)
            journal_inum: u32::from_le_bytes([raw[224], raw[225], raw[226], raw[227]]),
            journal_dev: u32::from_le_bytes([raw[228], raw[229], raw[230], raw[231]]),
        })
    }

    fn has_journal(&self) -> bool {
        (self.feature_compat & EXT3_FEATURE_COMPAT_HAS_JOURNAL) != 0
    }

    fn needs_recovery(&self) -> bool {
        (self.feature_incompat & EXT3_FEATURE_INCOMPAT_RECOVER) != 0
    }
}

/// ext3 Filesystem structure
#[repr(C)]
pub struct Ext3Filesystem {
    block_device_index: usize,
    total_size: u64,
    block_size: usize,
    inode_size: usize,
    inodes_per_group: u32,
    blocks_per_group: u32,
    total_groups: u32,
    first_data_block: u32,
    sb_inodes_count: u32,
    sb_blocks_count: u32,
    sb_magic: u16,
    sb_rev_level: u32,
    // ext3 specific
    has_journal: bool,
    journal_inode: u32,
    journal_mode: JournalMode,
    journal_initialized: bool,
}

// ============================================================================
// Module state
// ============================================================================

static mut EXT3_FS_INSTANCE: Option<Ext3Filesystem> = None;
static mut MODULE_INITIALIZED: bool = false;
static mut EXT3_WRITABLE: bool = false;
static mut JOURNAL_MODE: JournalMode = JournalMode::Ordered;

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

// ============================================================================
// Module entry points
// ============================================================================

#[no_mangle]
#[inline(never)]
pub extern "C" fn module_init() -> i32 {
    mod_info!(b"ext3 module: initializing...");
    
    unsafe {
        if MODULE_INITIALIZED {
            mod_warn!(b"ext3 module: already initialized");
            return 0;
        }
        
        // Check that ext2 module is loaded (dependency)
        let ext2_name = b"ext2";
        let ext2_loaded = kmod_is_module_loaded(ext2_name.as_ptr(), ext2_name.len());
        if ext2_loaded == 0 {
            mod_warn!(b"ext3 module: ext2 module not loaded, continuing anyway (ext3 can work standalone)");
            // Don't fail - ext3 can work without ext2 being separately loaded
            // since ext3 includes all ext2 functionality
        }
        
        // Create the operations table
        let ops = Ext3ModuleOps {
            new: Some(ext3_mod_new),
            destroy: Some(ext3_mod_destroy),
            lookup: Some(ext3_mod_lookup),
            read_at: Some(ext3_mod_read_at),
            write_at: Some(ext3_mod_write_at),
            list_dir: Some(ext3_mod_list_dir),
            get_stats: Some(ext3_mod_get_stats),
            set_writable: Some(ext3_mod_set_writable),
            is_writable: Some(ext3_mod_is_writable),
            create_file: Some(ext3_mod_create_file),
            journal_sync: Some(ext3_mod_journal_sync),
            set_journal_mode: Some(ext3_mod_set_journal_mode),
        };
        
        // Register with the kernel
        let result = kmod_ext3_register(&ops);
        
        if result != 0 {
            mod_error!(b"ext3 module: failed to register with kernel");
            return -1;
        }
        
        MODULE_INITIALIZED = true;
    }
    
    mod_info!(b"ext3 module: initialized successfully");
    0
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn module_exit() -> i32 {
    mod_info!(b"ext3 module: unloading...");
    
    unsafe {
        if !MODULE_INITIALIZED {
            return 0;
        }
        
        // Sync journal before unloading
        if let Some(ref fs) = EXT3_FS_INSTANCE {
            if fs.journal_initialized {
                // Flush any pending journal entries
                mod_info!(b"ext3 module: flushing journal...");
            }
        }
        
        kmod_ext3_unregister();
        
        EXT3_FS_INSTANCE = None;
        MODULE_INITIALIZED = false;
    }
    
    mod_info!(b"ext3 module: unloaded");
    0
}

// Legacy entry points
#[no_mangle]
pub extern "C" fn ext3_module_init() -> i32 {
    module_init()
}

#[no_mangle]
pub extern "C" fn ext3_module_exit() -> i32 {
    module_exit()
}

// ============================================================================
// Module Operation Functions
// ============================================================================

extern "C" fn ext3_mod_new(image: *const u8, size: usize) -> Ext3Handle {
    let fs = ext3_new_from_block_device();
    fs as Ext3Handle
}

extern "C" fn ext3_mod_destroy(_handle: Ext3Handle) {
    unsafe {
        if let Some(ref fs) = EXT3_FS_INSTANCE {
            if fs.journal_initialized {
                mod_info!(b"ext3: flushing journal before destroy");
            }
        }
        EXT3_FS_INSTANCE = None;
    }
}

extern "C" fn ext3_mod_lookup(
    handle: Ext3Handle,
    path: *const u8,
    path_len: usize,
    out: *mut FileRefHandle,
) -> i32 {
    if handle.is_null() || path.is_null() || out.is_null() {
        return -1;
    }

    let fs = handle as *const Ext3Filesystem;
    let fs_ref = unsafe { &*fs };
    
    let path_bytes = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    match fs_ref.lookup_internal(path_str) {
        Some(file_ref) => {
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
        None => -1,
    }
}

extern "C" fn ext3_mod_read_at(
    file: *const FileRefHandle,
    offset: usize,
    buf: *mut u8,
    len: usize,
) -> i32 {
    if file.is_null() || buf.is_null() {
        return -1;
    }

    let file_ref = unsafe { &*file };
    let fs = file_ref.fs as *const Ext3Filesystem;
    
    if fs.is_null() {
        return -1;
    }
    
    let fs_ref = unsafe { &*fs };
    let bytes_read = fs_ref.read_file_internal(file_ref.inode, offset, unsafe {
        core::slice::from_raw_parts_mut(buf, len)
    });
    
    bytes_read as i32
}

extern "C" fn ext3_mod_write_at(
    file: *const FileRefHandle,
    offset: usize,
    data: *const u8,
    len: usize,
) -> i32 {
    if file.is_null() || data.is_null() {
        return -1;
    }

    unsafe {
        if !EXT3_WRITABLE {
            mod_warn!(b"ext3: write denied - filesystem is read-only");
            return Ext3Error::ReadOnly as i32;
        }
    }

    let file_ref = unsafe { &*file };
    let fs = file_ref.fs as *const Ext3Filesystem;
    
    if fs.is_null() {
        return -1;
    }

    let fs_ref = unsafe { &*fs };
    let data_slice = unsafe { core::slice::from_raw_parts(data, len) };
    
    // For ext3, writes go through journal
    // In ordered mode, data is written first, then metadata is journaled
    match fs_ref.write_with_journal(file_ref.inode, offset, data_slice) {
        Ok(bytes_written) => bytes_written as i32,
        Err(e) => -(e as i32),
    }
}

extern "C" fn ext3_mod_list_dir(
    handle: Ext3Handle,
    path: *const u8,
    path_len: usize,
    cb: DirEntryCallback,
    ctx: *mut u8,
) -> i32 {
    if handle.is_null() || path.is_null() {
        return -1;
    }

    let fs = unsafe { &*(handle as *const Ext3Filesystem) };
    let path_bytes = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let file_ref = match fs.lookup_internal(path_str) {
        Some(fr) => fr,
        None => return -1,
    };

    if (file_ref.mode & 0o170000) != 0o040000 {
        return -1; // Not a directory
    }

    let inode = match fs.load_inode(file_ref.inode) {
        Ok(i) => i,
        Err(_) => return -1,
    };

    fs.for_each_dir_entry(&inode, |name, entry_inode, file_type| {
        cb(name.as_ptr(), name.len(), entry_inode, file_type, ctx);
    });

    0
}

extern "C" fn ext3_mod_get_stats(handle: Ext3Handle, stats: *mut Ext3Stats) -> i32 {
    if handle.is_null() || stats.is_null() {
        return -1;
    }

    let fs = unsafe { &*(handle as *const Ext3Filesystem) };
    
    unsafe {
        (*stats).inodes_count = fs.sb_inodes_count;
        (*stats).blocks_count = fs.sb_blocks_count;
        (*stats).free_blocks_count = 0;
        (*stats).free_inodes_count = 0;
        (*stats).block_size = fs.block_size as u32;
        (*stats).blocks_per_group = fs.blocks_per_group;
        (*stats).inodes_per_group = fs.inodes_per_group;
        (*stats).mtime = 0;
        (*stats).journal_blocks = 0; // TODO: read from journal superblock
        (*stats).journal_mode = fs.journal_mode as u8;
    }

    0
}

extern "C" fn ext3_mod_set_writable(writable: bool) {
    unsafe {
        EXT3_WRITABLE = writable;
        if writable {
            mod_info!(b"ext3: write mode ENABLED");
        } else {
            mod_info!(b"ext3: write mode DISABLED");
        }
    }
}

extern "C" fn ext3_mod_is_writable() -> bool {
    unsafe { EXT3_WRITABLE }
}

extern "C" fn ext3_mod_create_file(
    handle: Ext3Handle,
    path: *const u8,
    path_len: usize,
    mode: u16,
) -> i32 {
    if handle.is_null() || path.is_null() {
        return -1;
    }
    
    unsafe {
        if !EXT3_WRITABLE {
            return -1;
        }
    }
    
    let fs = unsafe { &*(handle as *const Ext3Filesystem) };
    let path_bytes = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    
    // File creation goes through journal for metadata consistency
    match fs.create_file_journaled(path_str, mode) {
        Ok(inode) => inode as i32,
        Err(_e) => -1,
    }
}

extern "C" fn ext3_mod_journal_sync(handle: Ext3Handle) -> i32 {
    if handle.is_null() {
        return -1;
    }
    
    let fs = unsafe { &*(handle as *const Ext3Filesystem) };
    
    if !fs.journal_initialized {
        return 0; // No journal to sync
    }
    
    // Flush all pending journal transactions
    mod_info!(b"ext3: journal sync requested");
    // TODO: Implement actual journal sync
    0
}

extern "C" fn ext3_mod_set_journal_mode(mode: u8) -> i32 {
    let new_mode = match mode {
        0 => JournalMode::Journal,
        1 => JournalMode::Ordered,
        2 => JournalMode::Writeback,
        _ => return -1,
    };
    
    unsafe {
        JOURNAL_MODE = new_mode;
    }
    
    mod_info!(b"ext3: journal mode changed");
    0
}

// ============================================================================
// Filesystem implementation
// ============================================================================

fn ext3_new_from_block_device() -> *mut Ext3Filesystem {
    mod_info!(b"ext3: initializing from block device");
    
    let device_index = unsafe { kmod_blk_find_rootfs() };
    if device_index < 0 {
        mod_error!(b"ext3: no block device found");
        return core::ptr::null_mut();
    }
    let device_index = device_index as usize;
    
    // Read superblock
    let mut sb_buf = [0u8; SUPERBLOCK_SIZE];
    let result = unsafe {
        kmod_blk_read_bytes(device_index, SUPERBLOCK_OFFSET as u64, sb_buf.as_mut_ptr(), SUPERBLOCK_SIZE)
    };
    if result < 0 {
        mod_error!(b"ext3: failed to read superblock");
        return core::ptr::null_mut();
    }

    let superblock = match Ext3Superblock::parse(&sb_buf) {
        Ok(sb) => sb,
        Err(_) => {
            mod_error!(b"ext3: failed to parse superblock");
            return core::ptr::null_mut();
        }
    };

    if superblock.magic != EXT2_SUPER_MAGIC {
        mod_error!(b"ext3: bad magic number");
        return core::ptr::null_mut();
    }

    // Check for journal feature
    let has_journal = superblock.has_journal();
    if !has_journal {
        mod_warn!(b"ext3: no journal feature flag, treating as ext2-compatible");
    }

    let block_size = 1024usize << superblock.log_block_size;
    let inode_size = if superblock.rev_level >= 1 && superblock.inode_size != 0 {
        superblock.inode_size as usize
    } else {
        128
    };

    if inode_size > SUPERBLOCK_SIZE {
        mod_error!(b"ext3: unsupported inode size");
        return core::ptr::null_mut();
    }

    let total_groups =
        (superblock.blocks_count + superblock.blocks_per_group - 1) / superblock.blocks_per_group;
    let total_size = superblock.blocks_count as u64 * block_size as u64;

    let journal_inode = if has_journal && superblock.journal_inum != 0 {
        superblock.journal_inum
    } else {
        EXT3_JOURNAL_INO
    };

    let fs = Ext3Filesystem {
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
        has_journal,
        journal_inode,
        journal_mode: unsafe { JOURNAL_MODE },
        journal_initialized: false, // Will be initialized on first write
    };

    if has_journal {
        mod_info!(b"ext3: filesystem with journaling initialized");
    } else {
        mod_info!(b"ext3: filesystem initialized (no journal)");
    }

    unsafe {
        EXT3_FS_INSTANCE = Some(fs);
        EXT3_FS_INSTANCE.as_mut().map(|f| f as *mut Ext3Filesystem).unwrap_or(core::ptr::null_mut())
    }
}

// ============================================================================
// Internal structures (shared with ext2)
// ============================================================================

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

impl Inode {
    fn parse(raw: &[u8]) -> Result<Self, Ext3Error> {
        if raw.len() < 128 {
            return Err(Ext3Error::ImageTooSmall);
        }

        let mut block = [0u32; 15];
        for i in 0..15 {
            let start = 40 + i * 4;
            block[i] = u32::from_le_bytes([raw[start], raw[start + 1], raw[start + 2], raw[start + 3]]);
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

    fn is_regular_file(&self) -> bool {
        (self.mode & 0o170000) == 0o100000
    }
}

struct FileRef {
    inode: u32,
    size: u64,
    mode: u16,
    blocks: u64,
    mtime: u64,
    nlink: u32,
    uid: u16,
    gid: u16,
}

const EXT2_NDIR_BLOCKS: usize = 12;
const EXT2_IND_BLOCK: usize = 12;
const EXT2_BLOCK_POINTER_SIZE: usize = 4;

impl Ext3Filesystem {
    fn read_bytes(&self, offset: u64, buf: &mut [u8]) -> bool {
        if buf.is_empty() {
            return true;
        }
        let result = unsafe {
            kmod_blk_read_bytes(self.block_device_index, offset, buf.as_mut_ptr(), buf.len())
        };
        result >= 0
    }
    
    fn write_bytes(&self, offset: u64, buf: &[u8]) -> bool {
        if buf.is_empty() {
            return true;
        }
        let result = unsafe {
            kmod_blk_write_bytes(self.block_device_index, offset, buf.as_ptr(), buf.len())
        };
        result >= 0
    }

    fn lookup_internal(&self, path: &str) -> Option<FileRef> {
        let trimmed = path.trim_matches('/');
        let mut inode_number = 2u32;

        if trimmed.is_empty() {
            return self.file_ref_from_inode(inode_number);
        }

        let mut inode = self.load_inode(inode_number).ok()?;

        for segment in trimmed.split('/') {
            if segment.is_empty() {
                continue;
            }
            inode_number = self.find_in_directory(&inode, segment)?;
            inode = self.load_inode(inode_number).ok()?;
        }

        self.file_ref_from_inode(inode_number)
    }

    fn file_ref_from_inode(&self, inode: u32) -> Option<FileRef> {
        let node = self.load_inode(inode).ok()?;
        Some(FileRef {
            inode,
            size: node.size(),
            mode: node.mode,
            blocks: node.blocks_lo as u64,
            uid: node.uid,
            gid: node.gid,
            mtime: node.mtime as u64,
            nlink: node.links_count as u32,
        })
    }

    fn load_inode(&self, inode: u32) -> Result<Inode, Ext3Error> {
        if inode == 0 {
            return Err(Ext3Error::InodeOutOfBounds);
        }

        let inode_index = inode - 1;
        let group = inode_index / self.inodes_per_group;
        if group >= self.total_groups {
            return Err(Ext3Error::InodeOutOfBounds);
        }
        
        let index_in_group = inode_index % self.inodes_per_group;
        let desc = self.group_descriptor(group)?;
        let inode_table_offset = desc.inode_table_block as usize * self.block_size;
        let inode_offset = inode_table_offset + index_in_group as usize * self.inode_size;

        let mut inode_buf = [0u8; 256];
        let read_size = self.inode_size.min(256);
        if !self.read_bytes(inode_offset as u64, &mut inode_buf[..read_size]) {
            return Err(Ext3Error::ImageTooSmall);
        }
        
        Inode::parse(&inode_buf[..read_size])
    }

    fn group_descriptor(&self, group: u32) -> Result<GroupDescriptor, Ext3Error> {
        let desc_size = 32usize;
        let superblock_block = if self.block_size == 1024 { 1 } else { 0 };
        let table_block = superblock_block + 1;
        let table_offset = table_block * self.block_size;
        let offset = table_offset + group as usize * desc_size;

        let mut data = [0u8; 32];
        if !self.read_bytes(offset as u64, &mut data) {
            return Err(Ext3Error::InvalidGroupDescriptor);
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

    fn read_block_to_buf<'a>(&self, block_number: u32, buf: &'a mut [u8]) -> Option<&'a [u8]> {
        if block_number == 0 || buf.len() < self.block_size {
            return None;
        }
        let offset = block_number as u64 * self.block_size as u64;
        if !self.read_bytes(offset, &mut buf[..self.block_size]) {
            return None;
        }
        Some(&buf[..self.block_size])
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
        let mut block_buf = [0u8; 4096];
        
        for &block in inode.block.iter().take(EXT2_NDIR_BLOCKS) {
            if block == 0 {
                continue;
            }
            
            if let Some(data) = self.read_block_to_buf(block, &mut block_buf) {
                let mut offset = 0usize;
                
                while offset + 8 <= block_size {
                    let entry_inode = u32::from_le_bytes([
                        data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
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
                        if let Ok(name) = core::str::from_utf8(&data[offset + 8..offset + 8 + name_len]) {
                            cb(name, entry_inode, file_type);
                        }
                    }
                    offset += rec_len;
                }
            }
        }
    }

    fn read_file_internal(&self, inode_num: u32, offset: usize, buf: &mut [u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }

        let inode = match self.load_inode(inode_num) {
            Ok(inode) => inode,
            Err(_) => return 0,
        };
        
        if !inode.is_regular_file() {
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
        let mut block_buf = [0u8; 4096];

        while remaining > 0 {
            let block_index = current_offset / block_size;
            let within_block = current_offset % block_size;
            let block_number = match self.block_number(&inode, block_index) {
                Some(bn) if bn != 0 => bn,
                _ => break,
            };
            
            let Some(block) = self.read_block_to_buf(block_number, &mut block_buf) else {
                break;
            };
            
            let available = cmp::min(block_size - within_block, remaining);
            buf[written..written + available].copy_from_slice(&block[within_block..within_block + available]);
            
            written += available;
            remaining -= available;
            current_offset += available;
        }
        
        written
    }

    fn block_number(&self, inode: &Inode, index: usize) -> Option<u32> {
        let pointers_per_block = self.block_size / EXT2_BLOCK_POINTER_SIZE;

        if index < EXT2_NDIR_BLOCKS {
            return Some(inode.block[index]);
        }

        let ind_index = index - EXT2_NDIR_BLOCKS;
        let mut block_buf = [0u8; 4096];

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
                raw[offset], raw[offset + 1], raw[offset + 2], raw[offset + 3],
            ]));
        }

        // Double indirect
        let dind_index = ind_index - pointers_per_block;
        let dind_capacity = pointers_per_block * pointers_per_block;
        
        if dind_index < dind_capacity {
            const EXT2_DIND_BLOCK: usize = 13;
            let double_indirect_block = inode.block[EXT2_DIND_BLOCK];
            if double_indirect_block == 0 {
                return None;
            }

            let first_level_index = dind_index / pointers_per_block;
            let second_level_index = dind_index % pointers_per_block;

            let mut dind_buf = [0u8; 4096];
            let dind_raw = self.read_block_to_buf(double_indirect_block, &mut dind_buf)?;
            let first_offset = first_level_index * EXT2_BLOCK_POINTER_SIZE;
            if first_offset + EXT2_BLOCK_POINTER_SIZE > dind_raw.len() {
                return None;
            }

            let indirect_block = u32::from_le_bytes([
                dind_raw[first_offset], dind_raw[first_offset + 1],
                dind_raw[first_offset + 2], dind_raw[first_offset + 3],
            ]);

            if indirect_block == 0 {
                return None;
            }

            let mut ind_buf = [0u8; 4096];
            let ind_raw = self.read_block_to_buf(indirect_block, &mut ind_buf)?;
            let second_offset = second_level_index * EXT2_BLOCK_POINTER_SIZE;
            if second_offset + EXT2_BLOCK_POINTER_SIZE > ind_raw.len() {
                return None;
            }

            return Some(u32::from_le_bytes([
                ind_raw[second_offset], ind_raw[second_offset + 1],
                ind_raw[second_offset + 2], ind_raw[second_offset + 3],
            ]));
        }

        None
    }

    // ext3 specific: write with journaling
    fn write_with_journal(&self, inode_num: u32, offset: usize, data: &[u8]) -> Result<usize, Ext3Error> {
        // For ordered mode (default): write data first, then journal metadata
        // For now, implement simple write similar to ext2
        
        if data.is_empty() {
            return Ok(0);
        }

        let inode = self.load_inode(inode_num)?;
        
        if !inode.is_regular_file() {
            return Err(Ext3Error::InvalidInode);
        }
        
        let file_size = inode.size() as usize;
        let block_size = self.block_size;
        let max_block_index = if file_size == 0 { 0 } else { (file_size + block_size - 1) / block_size };
        let max_writable_offset = max_block_index * block_size;
        
        if offset >= max_writable_offset && file_size > 0 {
            return Ok(0);
        }
        
        let write_end = offset + data.len();
        let actual_write_len = if write_end > max_writable_offset {
            if max_writable_offset > offset {
                max_writable_offset - offset
            } else {
                data.len().min(block_size)
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
            
            let block_number = match self.block_number(&inode, block_index) {
                Some(bn) if bn != 0 => bn,
                _ => break,
            };
            
            let available_in_block = block_size - within_block;
            let remaining = actual_write_len - written;
            let to_write = cmp::min(available_in_block, remaining);
            
            let block_offset = block_number as u64 * block_size as u64 + within_block as u64;
            if !self.write_bytes(block_offset, &data[written..written + to_write]) {
                return Err(Ext3Error::ImageTooSmall);
            }
            
            written += to_write;
            current_offset += to_write;
        }
        
        // TODO: Journal the metadata update
        
        Ok(written)
    }

    fn create_file_journaled(&self, path: &str, mode: u16) -> Result<u32, Ext3Error> {
        // TODO: Implement journaled file creation
        // For now, just log and return success for existing files
        mod_info!(b"ext3: create_file_journaled called");
        
        // Check if file already exists
        if self.lookup_internal(path).is_some() {
            return Ok(0);
        }
        
        // TODO: Implement actual file creation with journaling
        Err(Ext3Error::ReadOnly)
    }
}

// ============================================================================
// Panic handler
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
