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
    inode_table_block: u32,
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

#[repr(C)]
pub struct Ext2Filesystem {
    image_base: *const u8,
    image_size: usize,
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
            write_at: None, // Not implemented yet
            list_dir: Some(ext2_mod_list_dir),
            get_stats: Some(ext2_mod_get_stats),
            set_writable: None,
            is_writable: None,
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

// ============================================================================
// Filesystem operations (exported to kernel)
// ============================================================================

/// Initialize ext2 filesystem from disk image
#[no_mangle]
pub extern "C" fn ext2_fs_init(image: *const u8, size: usize) -> *mut Ext2Filesystem {
    ext2_new(image, size)
}

/// Create a new ext2 filesystem from an image
#[no_mangle]
pub extern "C" fn ext2_new(image: *const u8, size: usize) -> *mut Ext2Filesystem {
    if image.is_null() || size < SUPERBLOCK_OFFSET + SUPERBLOCK_SIZE {
        return core::ptr::null_mut();
    }

    let image_slice = unsafe { core::slice::from_raw_parts(image, size) };

    // Parse superblock
    let sb_data = &image_slice[SUPERBLOCK_OFFSET..SUPERBLOCK_OFFSET + SUPERBLOCK_SIZE];
    let superblock = match Superblock::parse(sb_data) {
        Ok(sb) => sb,
        Err(_) => return core::ptr::null_mut(),
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

    let fs = Ext2Filesystem {
        image_base: image,
        image_size: size,
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
    if fs.is_null() || path.is_null() || out_ref.is_null() {
        return -1;
    }

    let fs = unsafe { &*fs };
    let path_bytes = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    match fs.lookup_internal(path_str) {
        Some(file_ref) => {
            unsafe { *out_ref = file_ref };
            0
        }
        None => -1,
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

// ============================================================================
// Internal implementation
// ============================================================================

impl Ext2Filesystem {
    fn image(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.image_base, self.image_size) }
    }

    fn lookup_internal(&self, path: &str) -> Option<FileRef> {
        let trimmed = path.trim_matches('/');
        let mut inode_number = 2u32; // root inode

        if trimmed.is_empty() {
            return self.file_ref_from_inode(inode_number);
        }

        let mut inode = self.load_inode(inode_number).ok()?;

        for segment in trimmed.split('/') {
            if segment.is_empty() {
                continue;
            }
            let next_inode = self.find_in_directory(&inode, segment)?;
            inode_number = next_inode;
            inode = self.load_inode(inode_number).ok()?;
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

        let image = self.image();
        if inode_offset + self.inode_size > image.len() {
            return Err(Ext2Error::ImageTooSmall);
        }

        Inode::parse(&image[inode_offset..inode_offset + self.inode_size])
    }

    fn group_descriptor(&self, group: u32) -> Result<GroupDescriptor, Ext2Error> {
        let desc_size = 32usize;
        let superblock_block = if self.block_size == 1024 { 1 } else { 0 };
        let table_block = superblock_block + 1;
        let table_offset = table_block * self.block_size;
        let offset = table_offset + group as usize * desc_size;

        let image = self.image();
        if offset + desc_size > image.len() {
            return Err(Ext2Error::InvalidGroupDescriptor);
        }

        let data = &image[offset..offset + desc_size];
        Ok(GroupDescriptor {
            inode_table_block: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        })
    }

    fn read_block(&self, block_number: u32) -> Option<&[u8]> {
        if block_number == 0 {
            return None;
        }
        let offset = block_number as usize * self.block_size;
        let image = self.image();
        if offset + self.block_size > image.len() {
            return None;
        }
        Some(&image[offset..offset + self.block_size])
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
        for &block in inode.block.iter().take(EXT2_NDIR_BLOCKS) {
            if block == 0 {
                continue;
            }
            if let Some(data) = self.read_block(block) {
                let mut offset = 0usize;
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
                        if let Ok(name) =
                            core::str::from_utf8(&data[offset + 8..offset + 8 + name_len])
                        {
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

        while remaining > 0 {
            let block_index = current_offset / block_size;
            let within_block = current_offset % block_size;
            let Some(block_number) = self.block_number(&inode, block_index) else {
                break;
            };
            if block_number == 0 {
                break;
            }
            let Some(block) = self.read_block(block_number) else {
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
        if index < EXT2_NDIR_BLOCKS {
            return Some(inode.block[index]);
        }

        let ind_index = index - EXT2_NDIR_BLOCKS;
        let pointers_per_block = self.block_size / EXT2_BLOCK_POINTER_SIZE;
        if ind_index >= pointers_per_block {
            return None;
        }

        let indirect_block = inode.block[EXT2_IND_BLOCK];
        if indirect_block == 0 {
            return None;
        }

        let raw = self.read_block(indirect_block)?;
        let offset = ind_index * EXT2_BLOCK_POINTER_SIZE;
        if offset + EXT2_BLOCK_POINTER_SIZE > raw.len() {
            return None;
        }

        Some(u32::from_le_bytes([
            raw[offset],
            raw[offset + 1],
            raw[offset + 2],
            raw[offset + 3],
        ]))
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
