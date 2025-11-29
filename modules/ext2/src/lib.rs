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

// NOTE: This module avoids using core library functions that would introduce
// symbol dependencies. Instead, we use kernel-provided APIs or local implementations.

// ============================================================================
// Helper functions - replacements for core library functions
// ============================================================================

/// Local min function to avoid core::cmp dependency
#[inline(always)]
const fn min_usize(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

/// Compare two byte slices for equality (replaces slice == slice)
#[inline(always)]
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ============================================================================
// Rust panic/intrinsics support - call kernel's kmod_panic
// These functions are required by Rust's core library for bounds checking,
// division by zero, etc. We provide them to avoid undefined symbol errors.
// ============================================================================

extern "C" {
    fn kmod_panic(msg: *const u8, len: usize) -> !;
}

/// Helper to call kernel panic with a static message
#[inline(always)]
fn do_panic(msg: &[u8]) -> ! {
    unsafe { kmod_panic(msg.as_ptr(), msg.len()) }
}

// Bounds check panic - called by Rust compiler for array/slice indexing
#[no_mangle]
#[export_name = "_ZN4core9panicking18panic_bounds_check17h423523315369be23E"]
pub extern "C" fn panic_bounds_check(_: usize, _: usize) -> ! {
    do_panic(b"index out of bounds")
}

// Slice index bounds check failure - exact symbol name from compiler
#[no_mangle]
#[export_name = "_ZN4core5slice5index16slice_index_fail17ha90d967cc5a41c62E"]
pub extern "C" fn slice_index_fail(_: usize, _: usize) -> ! {
    do_panic(b"slice index out of bounds")
}

// More slice failures with common hash suffixes
#[no_mangle]
pub extern "C" fn _ZN4core5slice5index24slice_end_index_len_fail17h3f46tried62E(_: usize, _: usize) -> ! {
    do_panic(b"slice end index out of bounds")
}

#[no_mangle]
pub extern "C" fn _ZN4core5slice5index26slice_start_index_len_fail17h3f46tried62E(_: usize, _: usize) -> ! {
    do_panic(b"slice start index out of bounds")
}

#[no_mangle]
pub extern "C" fn _ZN4core5slice5index22slice_index_order_fail17h3f46tried62E(_: usize, _: usize) -> ! {
    do_panic(b"slice index order invalid")
}

// Division by zero and overflow panics
#[no_mangle]
pub extern "C" fn _ZN4core9panicking11panic_const23panic_const_div_by_zero17hc2227f4d6d9d4d9cE() -> ! {
    do_panic(b"attempt to divide by zero")
}

#[no_mangle]
pub extern "C" fn _ZN4core9panicking11panic_const26panic_const_rem_by_zero17hc2227f4d6d9d4d9cE() -> ! {
    do_panic(b"attempt to calculate remainder with zero divisor")
}

#[no_mangle]
pub extern "C" fn _ZN4core9panicking11panic_const28panic_const_add_overflow17hc2227f4d6d9d4d9cE() -> ! {
    do_panic(b"attempt to add with overflow")
}

#[no_mangle]
pub extern "C" fn _ZN4core9panicking11panic_const28panic_const_sub_overflow17hc2227f4d6d9d4d9cE() -> ! {
    do_panic(b"attempt to subtract with overflow")
}

#[no_mangle]
pub extern "C" fn _ZN4core9panicking11panic_const28panic_const_mul_overflow17hc2227f4d6d9d4d9cE() -> ! {
    do_panic(b"attempt to multiply with overflow")
}

// Generic panic entry points (catch-all for any mangled panic symbols)
#[no_mangle]
pub extern "C" fn rust_begin_unwind(_: &core::panic::PanicInfo) -> ! {
    do_panic(b"panic in kernel module")
}

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
    fn kmod_dealloc(ptr: *mut u8, size: usize, align: usize);
    fn kmod_register_fs(name: *const u8, name_len: usize, init_fn: usize, lookup_fn: usize) -> i32;
    fn kmod_unregister_fs(name: *const u8, name_len: usize) -> i32;
    fn kmod_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
    fn kmod_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8;
    // New VFS driver registration API
    fn register_fs_driver(name: *const u8, name_len: usize, ops: *const FsOps) -> i32;
    fn unregister_fs_driver(name: *const u8, name_len: usize) -> i32;
}

// ============================================================================
// VFS Driver Operations Structure (must match kernel's FsOps)
// ============================================================================

/// Filesystem driver operations - C ABI compatible
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsOps {
    /// Initialize filesystem from image data, returns opaque fs handle
    pub init: Option<unsafe extern "C" fn(image: *const u8, size: usize) -> *mut core::ffi::c_void>,
    /// Lookup a file by path, fills DynamicFileRef structure, returns 0 on success
    pub lookup: Option<unsafe extern "C" fn(
        fs: *mut core::ffi::c_void,
        path: *const u8,
        path_len: usize,
        out_ref: *mut DynamicFileRef,
    ) -> i32>,
    /// Read file data at offset
    pub read: Option<unsafe extern "C" fn(
        fs: *mut core::ffi::c_void,
        inode: u32,
        offset: usize,
        buf: *mut u8,
        buf_len: usize,
    ) -> isize>,
    /// List directory entries
    pub readdir: Option<unsafe extern "C" fn(
        fs: *mut core::ffi::c_void,
        path: *const u8,
        path_len: usize,
        callback: unsafe extern "C" fn(*const u8, usize, *const DynamicFileRef, *mut core::ffi::c_void),
        user_data: *mut core::ffi::c_void,
    ) -> i32>,
    /// Get filesystem name
    pub name: Option<unsafe extern "C" fn() -> *const u8>,
}

/// Dynamic file reference - C ABI compatible (must match kernel's DynamicFileRef)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DynamicFileRef {
    /// Filesystem handle (opaque pointer)
    pub fs_handle: *mut core::ffi::c_void,
    /// Inode number
    pub inode: u32,
    /// File size in bytes
    pub size: u64,
    /// File mode (permissions and type)
    pub mode: u16,
    /// Number of 512-byte blocks
    pub blocks: u64,
    /// Last modification time (Unix timestamp)
    pub mtime: u64,
    /// Number of hard links
    pub nlink: u32,
    /// User ID
    pub uid: u16,
    /// Group ID
    pub gid: u16,
    /// Filesystem driver index (for looking up ops)
    pub driver_idx: u8,
    /// Padding for alignment
    pub _pad: [u8; 5],
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
        
        // Create FsOps structure for the new VFS driver API
        let ops = FsOps {
            init: Some(ext2_fs_init_wrapper),
            lookup: Some(ext2_lookup_wrapper),
            read: Some(ext2_read_wrapper),
            readdir: None, // TODO: implement readdir
            name: Some(ext2_name),
        };
        
        // Register with the kernel's VFS using new driver API
        let name = b"ext2";
        let result = register_fs_driver(
            name.as_ptr(),
            name.len(),
            &ops as *const FsOps,
        );
        
        if result < 0 {
            // Fall back to legacy registration
            mod_warn!(b"ext2 module: new API failed, trying legacy...");
            let legacy_result = kmod_register_fs(
                name.as_ptr(),
                name.len(),
                ext2_fs_init as usize,
                ext2_lookup as usize,
            );
            if legacy_result != 0 {
                mod_error!(b"ext2 module: failed to register filesystem");
                return -1;
            }
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
        
        // Unregister from VFS
        let name = b"ext2";
        kmod_unregister_fs(name.as_ptr(), name.len());
        
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
// New VFS Driver API Wrappers
// ============================================================================

/// Wrapper for init function compatible with FsOps
unsafe extern "C" fn ext2_fs_init_wrapper(image: *const u8, size: usize) -> *mut core::ffi::c_void {
    ext2_fs_init(image, size) as *mut core::ffi::c_void
}

/// Wrapper for lookup function compatible with FsOps
unsafe extern "C" fn ext2_lookup_wrapper(
    fs: *mut core::ffi::c_void,
    path: *const u8,
    path_len: usize,
    out_ref: *mut DynamicFileRef,
) -> i32 {
    if fs.is_null() || path.is_null() || out_ref.is_null() {
        return -1;
    }

    let fs_ptr = fs as *const Ext2Filesystem;
    let mut file_ref = FileRef {
        fs: fs_ptr,
        inode: 0,
        size: 0,
        mode: 0,
        blocks: 0,
        mtime: 0,
        nlink: 0,
        uid: 0,
        gid: 0,
    };

    let result = ext2_lookup(fs_ptr, path, path_len, &mut file_ref as *mut FileRef);
    
    if result == 0 {
        // Convert FileRef to DynamicFileRef
        (*out_ref) = DynamicFileRef {
            fs_handle: fs,
            inode: file_ref.inode,
            size: file_ref.size,
            mode: file_ref.mode,
            blocks: file_ref.blocks,
            mtime: file_ref.mtime,
            nlink: file_ref.nlink,
            uid: file_ref.uid,
            gid: file_ref.gid,
            driver_idx: 0, // Will be set by kernel
            _pad: [0; 5],
        };
        0
    } else {
        -1
    }
}

/// Wrapper for read function compatible with FsOps
unsafe extern "C" fn ext2_read_wrapper(
    fs: *mut core::ffi::c_void,
    inode: u32,
    offset: usize,
    buf: *mut u8,
    buf_len: usize,
) -> isize {
    ext2_read(fs as *const Ext2Filesystem, inode, offset, buf, buf_len)
}

/// Get filesystem name
unsafe extern "C" fn ext2_name() -> *const u8 {
    b"ext2\0".as_ptr()
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
    // Use raw bytes directly instead of converting to str
    // This avoids core::str::from_utf8 symbol dependency

    match fs.lookup_internal_bytes(path_bytes) {
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

    /// Lookup using raw bytes instead of &str to avoid core::str dependencies
    fn lookup_internal_bytes(&self, path: &[u8]) -> Option<FileRef> {
        // Trim leading/trailing slashes manually
        let mut start = 0;
        let mut end = path.len();
        while start < end && path[start] == b'/' {
            start += 1;
        }
        while end > start && path[end - 1] == b'/' {
            end -= 1;
        }
        let trimmed = &path[start..end];
        
        let mut inode_number = 2u32; // root inode

        if trimmed.is_empty() {
            return self.file_ref_from_inode(inode_number);
        }

        let mut inode = self.load_inode(inode_number).ok()?;

        // Manual path segment iteration (split by '/')
        let mut seg_start = 0;
        let mut i = 0;
        while i <= trimmed.len() {
            if i == trimmed.len() || trimmed[i] == b'/' {
                if i > seg_start {
                    let segment = &trimmed[seg_start..i];
                    let next_inode = self.find_in_directory_bytes(&inode, segment)?;
                    inode_number = next_inode;
                    inode = self.load_inode(inode_number).ok()?;
                }
                seg_start = i + 1;
            }
            i += 1;
        }

        self.file_ref_from_inode(inode_number)
    }
    
    /// Legacy lookup for &str (converts to bytes)
    fn lookup_internal(&self, path: &str) -> Option<FileRef> {
        self.lookup_internal_bytes(path.as_bytes())
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
        self.find_in_directory_bytes(inode, target.as_bytes())
    }
    
    /// Find directory entry by raw bytes (avoids str conversion)
    fn find_in_directory_bytes(&self, inode: &Inode, target: &[u8]) -> Option<u32> {
        let mut found = None;
        self.for_each_dir_entry_bytes(inode, |name_bytes, inode_num, _file_type| {
            if bytes_eq(name_bytes, target) {
                found = Some(inode_num);
            }
        });
        found
    }

    /// Iterate directory entries using raw bytes (avoids core::str::from_utf8)
    fn for_each_dir_entry_bytes<F>(&self, inode: &Inode, mut cb: F)
    where
        F: FnMut(&[u8], u32, u8),
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
                        // Use raw bytes directly - no UTF-8 conversion needed
                        let name_bytes = &data[offset + 8..offset + 8 + name_len];
                        cb(name_bytes, entry_inode, file_type);
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

        let mut remaining = min_usize(buf.len(), file_size - offset);
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
            let available = min_usize(block_size - within_block, remaining);
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
// Panic handler for module - calls kernel's kmod_panic (wraps kpanic! macro)
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // Simple panic message - avoid using Display trait which requires core library symbols
    unsafe { kmod_panic(b"ext2 module panic".as_ptr(), 17) }
}
