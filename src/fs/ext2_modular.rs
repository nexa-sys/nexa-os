//! Modular ext2 Filesystem Support
//!
//! This module provides the kernel-side interface for the ext2 filesystem driver
//! which is loaded as a kernel module (.nkm) rather than being compiled into the kernel.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────────┐     ┌────────────────┐
//! │   VFS/App   │────▶│  ext2_modular   │────▶│  ext2.nkm      │
//! │   Layer     │     │  (this module)  │     │  (loadable)    │
//! └─────────────┘     └─────────────────┘     └────────────────┘
//!                            │
//!                            ▼
//!                     ┌─────────────────┐
//!                     │  Module Ops     │
//!                     │  (FFI callbacks)│
//!                     └─────────────────┘
//! ```
//!
//! The ext2 module registers its operations through `kmod_ext2_register()` when loaded.
//! The kernel then routes all ext2 operations through these registered callbacks.

#![allow(dead_code)]

use crate::posix::{FileType, Metadata};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr;
use spin::Mutex;

// ============================================================================
// Error Types
// ============================================================================

/// ext2 filesystem errors (matches module's error type)
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(i32)]
pub enum Ext2Error {
    BadMagic = -1,
    ImageTooSmall = -2,
    UnsupportedInodeSize = -3,
    InvalidGroupDescriptor = -4,
    InodeOutOfBounds = -5,
    NoSpaceLeft = -6,
    ReadOnly = -7,
    InvalidInode = -8,
    InvalidBlockNumber = -9,
    ModuleNotLoaded = -100,
    InvalidOperation = -101,
}

impl Ext2Error {
    /// Convert from module return code
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            -1 => Some(Self::BadMagic),
            -2 => Some(Self::ImageTooSmall),
            -3 => Some(Self::UnsupportedInodeSize),
            -4 => Some(Self::InvalidGroupDescriptor),
            -5 => Some(Self::InodeOutOfBounds),
            -6 => Some(Self::NoSpaceLeft),
            -7 => Some(Self::ReadOnly),
            -8 => Some(Self::InvalidInode),
            -9 => Some(Self::InvalidBlockNumber),
            -100 => Some(Self::ModuleNotLoaded),
            -101 => Some(Self::InvalidOperation),
            _ => None,
        }
    }
}

// ============================================================================
// FFI Types for Module Callbacks
// ============================================================================

/// Opaque handle to an ext2 filesystem instance (managed by module)
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Ext2Handle(pub *mut u8);

impl Ext2Handle {
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

// SAFETY: Ext2Handle is just a pointer that the module manages
unsafe impl Send for Ext2Handle {}
unsafe impl Sync for Ext2Handle {}

/// Opaque handle to a file reference
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FileRefHandle {
    /// Filesystem handle
    pub fs: Ext2Handle,
    /// Inode number
    pub inode: u32,
    /// File size
    pub size: u64,
    /// File mode (permissions + type)
    pub mode: u16,
    /// Block count
    pub blocks: u64,
    /// Modification time
    pub mtime: u64,
    /// Link count
    pub nlink: u32,
    /// User ID
    pub uid: u16,
    /// Group ID
    pub gid: u16,
}

impl FileRefHandle {
    pub fn is_valid(&self) -> bool {
        !self.fs.is_null() && self.inode != 0
    }

    /// Read file content at specified offset
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        read_at(self, offset, buf)
    }

    pub fn metadata(&self) -> Metadata {
        let file_type = match self.mode & 0o170000 {
            0o040000 => FileType::Directory,
            0o100000 => FileType::Regular,
            0o120000 => FileType::Symlink,
            0o020000 => FileType::Character,
            0o060000 => FileType::Block,
            0o010000 => FileType::Fifo,
            0o140000 => FileType::Socket,
            other => FileType::Unknown(other as u16),
        };

        Metadata {
            mode: self.mode,
            uid: self.uid as u32,
            gid: self.gid as u32,
            size: self.size,
            mtime: self.mtime,
            file_type,
            nlink: self.nlink,
            blocks: self.blocks,
        }
        .normalize()
    }
}

/// Directory entry callback type
pub type DirEntryCallback =
    extern "C" fn(name: *const u8, name_len: usize, inode: u32, file_type: u8, ctx: *mut u8);

/// Filesystem statistics
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
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

// ============================================================================
// Module Operations Table (registered by ext2.nkm)
// ============================================================================

/// Function pointer types for module operations
pub type FnExt2New = extern "C" fn(image: *const u8, size: usize) -> Ext2Handle;
pub type FnExt2Destroy = extern "C" fn(handle: Ext2Handle);
pub type FnExt2Lookup = extern "C" fn(
    handle: Ext2Handle,
    path: *const u8,
    path_len: usize,
    out: *mut FileRefHandle,
) -> i32;
pub type FnExt2ReadAt =
    extern "C" fn(file: *const FileRefHandle, offset: usize, buf: *mut u8, len: usize) -> i32;
pub type FnExt2WriteAt =
    extern "C" fn(file: *const FileRefHandle, offset: usize, data: *const u8, len: usize) -> i32;
pub type FnExt2ListDir = extern "C" fn(
    handle: Ext2Handle,
    path: *const u8,
    path_len: usize,
    cb: DirEntryCallback,
    ctx: *mut u8,
) -> i32;
pub type FnExt2GetStats = extern "C" fn(handle: Ext2Handle, stats: *mut Ext2Stats) -> i32;
pub type FnExt2SetWritable = extern "C" fn(writable: bool);
pub type FnExt2IsWritable = extern "C" fn() -> bool;
pub type FnExt2CreateFile = extern "C" fn(handle: Ext2Handle, path: *const u8, path_len: usize, mode: u16) -> i32;

/// Module operations table
#[repr(C)]
pub struct Ext2ModuleOps {
    /// Create a new ext2 filesystem from image data
    pub new: Option<FnExt2New>,
    /// Destroy an ext2 filesystem handle
    pub destroy: Option<FnExt2Destroy>,
    /// Lookup a file/directory by path
    pub lookup: Option<FnExt2Lookup>,
    /// Read data from a file at offset
    pub read_at: Option<FnExt2ReadAt>,
    /// Write data to a file at offset
    pub write_at: Option<FnExt2WriteAt>,
    /// List directory entries
    pub list_dir: Option<FnExt2ListDir>,
    /// Get filesystem statistics
    pub get_stats: Option<FnExt2GetStats>,
    /// Set write mode
    pub set_writable: Option<FnExt2SetWritable>,
    /// Check if writable
    pub is_writable: Option<FnExt2IsWritable>,
    /// Create a new file
    pub create_file: Option<FnExt2CreateFile>,
}

impl Ext2ModuleOps {
    const fn empty() -> Self {
        Self {
            new: None,
            destroy: None,
            lookup: None,
            read_at: None,
            write_at: None,
            list_dir: None,
            get_stats: None,
            set_writable: None,
            is_writable: None,
            create_file: None,
        }
    }

    fn is_valid(&self) -> bool {
        self.new.is_some()
            && self.lookup.is_some()
            && self.read_at.is_some()
            && self.list_dir.is_some()
    }
}

// ============================================================================
// Global State
// ============================================================================

/// Registered module operations
static EXT2_OPS: Mutex<Ext2ModuleOps> = Mutex::new(Ext2ModuleOps::empty());

/// Global ext2 filesystem instance (after mounting)
static EXT2_GLOBAL: Mutex<Option<Ext2Handle>> = Mutex::new(None);

/// Global image reference (kept alive for filesystem)
static EXT2_IMAGE: Mutex<Option<&'static [u8]>> = Mutex::new(None);

/// Write mode flag
static EXT2_WRITABLE: Mutex<bool> = Mutex::new(false);

// ============================================================================
// Module Registration API (called by ext2.nkm)
// ============================================================================

/// Register ext2 module operations
/// Called by ext2.nkm during module_init
#[no_mangle]
pub extern "C" fn kmod_ext2_register(ops: *const Ext2ModuleOps) -> i32 {
    if ops.is_null() {
        crate::kerror!("ext2_modular: null ops pointer");
        return -1;
    }

    let ops = unsafe { &*ops };

    // Validate required operations
    if ops.new.is_none() {
        crate::kerror!("ext2_modular: missing 'new' operation");
        return -1;
    }
    if ops.lookup.is_none() {
        crate::kerror!("ext2_modular: missing 'lookup' operation");
        return -1;
    }
    if ops.read_at.is_none() {
        crate::kerror!("ext2_modular: missing 'read_at' operation");
        return -1;
    }

    {
        let mut global_ops = EXT2_OPS.lock();
        global_ops.new = ops.new;
        global_ops.destroy = ops.destroy;
        global_ops.lookup = ops.lookup;
        global_ops.read_at = ops.read_at;
        global_ops.write_at = ops.write_at;
        global_ops.list_dir = ops.list_dir;
        global_ops.get_stats = ops.get_stats;
        global_ops.set_writable = ops.set_writable;
        global_ops.is_writable = ops.is_writable;
        global_ops.create_file = ops.create_file;
    }

    // Also register to the new modular filesystem registry for future ext3/ext4 compatibility
    // This creates a bridge from the old ext2-specific API to the new unified API
    register_to_modular_fs_registry();

    crate::kinfo!("ext2_modular: module registered successfully");
    0
}

/// Unregister ext2 module operations
/// Called by ext2.nkm during module_exit
#[no_mangle]
pub extern "C" fn kmod_ext2_unregister() -> i32 {
    // Destroy any existing filesystem instance
    {
        let mut global = EXT2_GLOBAL.lock();
        if let Some(handle) = global.take() {
            let ops = EXT2_OPS.lock();
            if let Some(destroy) = ops.destroy {
                destroy(handle);
            }
        }
    }

    // Clear image reference
    *EXT2_IMAGE.lock() = None;

    // Clear operations
    *EXT2_OPS.lock() = Ext2ModuleOps::empty();

    crate::kinfo!("ext2_modular: module unregistered");
    0
}

// ============================================================================
// Kernel API (used by VFS and boot stages)
// ============================================================================

/// Check if ext2 module is loaded and ready
pub fn is_module_loaded() -> bool {
    EXT2_OPS.lock().is_valid()
}

/// Create a new ext2 filesystem from image data
pub fn new(image: &'static [u8]) -> Result<(), Ext2Error> {
    let ops = EXT2_OPS.lock();
    let new_fn = ops.new.ok_or(Ext2Error::ModuleNotLoaded)?;
    drop(ops);

    let handle = new_fn(image.as_ptr(), image.len());
    if handle.is_null() {
        return Err(Ext2Error::BadMagic);
    }

    // Store globally
    *EXT2_IMAGE.lock() = Some(image);
    *EXT2_GLOBAL.lock() = Some(handle);

    crate::kinfo!(
        "ext2_modular: filesystem initialized ({} bytes)",
        image.len()
    );
    Ok(())
}

/// Register a global ext2 filesystem (compatibility with old API)
pub fn register_global(image: &'static [u8]) -> Result<(), Ext2Error> {
    new(image)
}

/// Get the global ext2 filesystem handle
pub fn global() -> Option<Ext2Handle> {
    *EXT2_GLOBAL.lock()
}

/// Lookup a file by path
pub fn lookup(path: &str) -> Option<FileRefHandle> {
    crate::kdebug!("ext2_modular::lookup called for: {}", path);

    let ops = EXT2_OPS.lock();
    let lookup_fn = match ops.lookup {
        Some(f) => f,
        None => {
            crate::kwarn!("ext2_modular::lookup: no lookup function registered");
            return None;
        }
    };
    let handle = match *EXT2_GLOBAL.lock() {
        Some(h) => h,
        None => {
            crate::kwarn!("ext2_modular::lookup: no global handle");
            return None;
        }
    };
    drop(ops);

    let mut file_ref = FileRefHandle {
        fs: Ext2Handle(ptr::null_mut()),
        inode: 0,
        size: 0,
        mode: 0,
        blocks: 0,
        mtime: 0,
        nlink: 0,
        uid: 0,
        gid: 0,
    };

    // Copy path to a local buffer in case the caller's buffer is in user space
    const PATH_BUF_SIZE: usize = 256;
    let mut path_buf: [u8; PATH_BUF_SIZE] = [0u8; PATH_BUF_SIZE];
    let path_len = path.len().min(PATH_BUF_SIZE);
    path_buf[..path_len].copy_from_slice(&path.as_bytes()[..path_len]);

    // NOTE: No CR3 switch needed - user page tables include kernel mappings

    let ret = lookup_fn(handle, path_buf.as_ptr(), path_len, &mut file_ref);

    crate::kinfo!(
        "ext2_modular::lookup: ret={}, inode={}, size={}, mode=0o{:o}, uid={}, gid={}, nlink={}",
        ret,
        file_ref.inode,
        file_ref.size,
        file_ref.mode,
        file_ref.uid,
        file_ref.gid,
        file_ref.nlink
    );
    if ret == 0 && file_ref.is_valid() {
        Some(file_ref)
    } else {
        crate::kdebug!("ext2_modular::lookup: failed for path '{}'", path);
        None
    }
}

/// Read data from a file
pub fn read_at(file: &FileRefHandle, offset: usize, buf: &mut [u8]) -> usize {
    let ops = EXT2_OPS.lock();
    let read_fn = match ops.read_at {
        Some(f) => f,
        None => return 0,
    };
    drop(ops);

    // NOTE: No CR3 switch needed - user page tables include kernel mappings
    let ret = read_fn(file, offset, buf.as_mut_ptr(), buf.len());

    if ret >= 0 {
        ret as usize
    } else {
        0
    }
}

/// Write data to a file
pub fn write_at(file: &FileRefHandle, offset: usize, data: &[u8]) -> Result<usize, Ext2Error> {
    if !is_writable() {
        return Err(Ext2Error::ReadOnly);
    }

    let ops = EXT2_OPS.lock();
    let write_fn = ops.write_at.ok_or(Ext2Error::InvalidOperation)?;
    drop(ops);

    let ret = write_fn(file, offset, data.as_ptr(), data.len());
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(Ext2Error::from_code(ret).unwrap_or(Ext2Error::InvalidOperation))
    }
}

/// Create a new file in the filesystem
pub fn create_file(path: &str, mode: u16) -> Result<(), Ext2Error> {
    crate::kinfo!("ext2_modular::create_file: attempting to create '{}'", path);
    
    if !is_writable() {
        crate::kwarn!("ext2_modular::create_file: filesystem is not writable");
        return Err(Ext2Error::ReadOnly);
    }

    let ops = EXT2_OPS.lock();
    let create_fn = match ops.create_file {
        Some(f) => f,
        None => {
            crate::kwarn!("ext2_modular::create_file: create_file operation not registered");
            return Err(Ext2Error::InvalidOperation);
        }
    };
    let handle = match *EXT2_GLOBAL.lock() {
        Some(h) => h,
        None => {
            crate::kwarn!("ext2_modular::create_file: no global ext2 handle");
            return Err(Ext2Error::InvalidOperation);
        }
    };
    drop(ops);

    // Copy path to a buffer with fixed size
    let path_len = path.len().min(255);
    let mut path_buf = [0u8; 256];
    path_buf[..path_len].copy_from_slice(path.as_bytes());

    crate::kinfo!("ext2_modular::create_file: calling module create_file for '{}'", path);
    let ret = create_fn(handle, path_buf.as_ptr(), path_len, mode);
    if ret >= 0 {
        crate::kinfo!("ext2_modular::create_file: created '{}' (inode={})", path, ret);
        Ok(())
    } else {
        crate::kwarn!("ext2_modular::create_file: failed for '{}' (ret={})", path, ret);
        Err(Ext2Error::from_code(ret).unwrap_or(Ext2Error::InvalidOperation))
    }
}

/// List directory entries
pub fn list_directory<F>(path: &str, mut callback: F)
where
    F: FnMut(&str, Metadata),
{
    let ops = EXT2_OPS.lock();
    let list_fn = match ops.list_dir {
        Some(f) => f,
        None => return,
    };
    let handle = match *EXT2_GLOBAL.lock() {
        Some(h) => h,
        None => return,
    };
    drop(ops);

    // DEBUG: Print function pointer value via serial
    crate::serial_println!(
        "EXT2_LIST_DIR: list_fn={:#x}, handle={:#x}",
        list_fn as *const () as u64,
        handle.0 as u64
    );

    // CRITICAL FIX: Copy path to a HEAP buffer BEFORE switching CR3
    // The path may point to user-space memory which is not accessible under kernel CR3
    // Using heap allocation instead of stack to prevent stack overflow
    let path_len = path.len().min(256);
    let mut path_buf: Vec<u8> = Vec::with_capacity(path_len);
    path_buf.extend_from_slice(&path.as_bytes()[..path_len]);

    // Create a closure context with heap-allocated name buffer
    struct CallbackContext<'a, F: FnMut(&str, Metadata)> {
        callback: &'a mut F,
        name_buf: Box<[u8; 256]>, // Heap-allocated buffer to avoid stack overflow
    }

    extern "C" fn dir_entry_trampoline<F: FnMut(&str, Metadata)>(
        name: *const u8,
        name_len: usize,
        _inode: u32,
        file_type: u8,
        ctx: *mut u8,
    ) {
        crate::serial_println!("TRAMPOLINE: entry name_len={}", name_len);

        if name.is_null() || ctx.is_null() {
            return;
        }

        unsafe {
            let ctx = &mut *(ctx as *mut CallbackContext<F>);

            // Copy name to the heap-allocated buffer to avoid stack overflow
            let name_len = name_len.min(255);
            let name_slice = core::slice::from_raw_parts(name, name_len);
            ctx.name_buf[..name_len].copy_from_slice(name_slice);

            // Parse name as UTF-8
            let name_str = match core::str::from_utf8(&ctx.name_buf[..name_len]) {
                Ok(s) => s,
                Err(_) => return,
            };

            // Skip . and ..
            if name_str == "." || name_str == ".." {
                return;
            }

            // Create basic metadata from file_type
            let ft = match file_type {
                2 => FileType::Directory,
                1 => FileType::Regular,
                7 => FileType::Symlink,
                3 => FileType::Character,
                4 => FileType::Block,
                5 => FileType::Fifo,
                6 => FileType::Socket,
                _ => FileType::Unknown(0),
            };

            let meta = Metadata::empty().with_type(ft);

            // NOTE: The callback is kernel code that writes to user-space memory.
            // We stay in kernel CR3 here because:
            // 1. The callback code is in kernel space
            // 2. User memory writes are done via the syscall buffer which is already
            //    accessible from kernel CR3 (kernel has identity mapping of user phys memory)
            // DO NOT switch to user CR3 here - it would cause kernel code to become inaccessible!

            (ctx.callback)(name_str, meta);
        }
    }

    // NOTE: We do NOT switch CR3 here anymore.
    // The user page tables already include kernel mappings (cloned during process creation),
    // so the ext2 module code and data are accessible from the user CR3.
    // Switching to kernel CR3 would make user-space addresses (like the syscall buffer)
    // inaccessible, causing the callback to fail when writing results.

    let mut ctx = CallbackContext {
        callback: &mut callback,
        name_buf: Box::new([0u8; 256]),
    };

    crate::serial_println!(
        "EXT2_LIST_DIR: calling list_fn, trampoline={:#x}, ctx={:#x}",
        dir_entry_trampoline::<F> as *const () as u64,
        &ctx as *const _ as u64
    );

    list_fn(
        handle,
        path_buf.as_ptr(),
        path_len,
        dir_entry_trampoline::<F>,
        &mut ctx as *mut _ as *mut u8,
    );

    crate::serial_println!("EXT2_LIST_DIR: list_fn returned");
}

/// Get filesystem metadata for a path
pub fn metadata_for_path(path: &str) -> Option<Metadata> {
    lookup(path).map(|f| f.metadata())
}

/// Get filesystem statistics
pub fn get_stats() -> Option<Ext2Stats> {
    let ops = EXT2_OPS.lock();
    let get_stats_fn = ops.get_stats?;
    let handle = (*EXT2_GLOBAL.lock())?;
    drop(ops);

    let mut stats = Ext2Stats::default();
    let ret = get_stats_fn(handle, &mut stats);
    if ret == 0 {
        Some(stats)
    } else {
        None
    }
}

/// Enable write mode
pub fn enable_write_mode() {
    *EXT2_WRITABLE.lock() = true;

    let ops = EXT2_OPS.lock();
    if let Some(set_writable) = ops.set_writable {
        set_writable(true);
    }
}

/// Check if write mode is enabled
pub fn is_writable() -> bool {
    *EXT2_WRITABLE.lock()
}

/// Filesystem name
pub fn name() -> &'static str {
    "ext2"
}

// ============================================================================
// VFS FileSystem trait implementation
// ============================================================================

use super::vfs::{FileContent, FileSystem, OpenFile};

/// Wrapper that implements FileSystem trait for the modular ext2
pub struct Ext2ModularFs;

impl Ext2ModularFs {
    pub fn new() -> Option<Self> {
        if is_module_loaded() && global().is_some() {
            Some(Self)
        } else {
            None
        }
    }
}

impl FileSystem for Ext2ModularFs {
    fn name(&self) -> &'static str {
        "ext2"
    }

    fn read(&self, path: &str) -> Option<OpenFile> {
        let file_ref = lookup(path)?;
        Some(OpenFile {
            content: FileContent::Ext2Modular(file_ref),
            metadata: file_ref.metadata(),
        })
    }

    fn metadata(&self, path: &str) -> Option<Metadata> {
        metadata_for_path(path)
    }

    fn list(&self, path: &str, cb: &mut dyn FnMut(&str, Metadata)) {
        list_directory(path, cb);
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<usize, &'static str> {
        if !is_writable() {
            return Err("ext2 filesystem is read-only");
        }

        let file_ref = lookup(path).ok_or("file not found")?;
        write_at(&file_ref, 0, data).map_err(|_| "write failed")
    }

    fn create(&self, path: &str) -> Result<(), &'static str> {
        if !is_writable() {
            return Err("ext2 filesystem is read-only");
        }
        create_file(path, 0o644).map_err(|_| "file creation failed")
    }
}

// ============================================================================
// Symbol Table Registration
// ============================================================================

/// Register ext2 module API symbols
pub fn register_symbols() {
    use crate::kmod::symbols::{register_symbol, SymbolType};

    register_symbol(
        "kmod_ext2_register",
        kmod_ext2_register as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_ext2_unregister",
        kmod_ext2_unregister as *const () as u64,
        SymbolType::Function,
    );

    crate::kinfo!("ext2_modular: kernel symbols registered");
}

// ============================================================================
// Initialization
// ============================================================================

/// Initialize the ext2 modular support
/// Called early in boot, before module loading
pub fn init() {
    register_symbols();
    crate::kinfo!("ext2_modular: subsystem initialized (waiting for module)");
}

// ============================================================================
// Modular Filesystem Registry Integration
// ============================================================================

/// Index of ext2 in the modular filesystem registry (set during registration)
static EXT2_REGISTRY_INDEX: Mutex<Option<u8>> = Mutex::new(None);

/// Bridge callback for ModularFsOps::new_from_image
extern "C" fn ext2_modular_new_from_image(image: *const u8, size: usize) -> *mut u8 {
    // This is called when mounting through the unified API
    // We use the existing ext2 module ops
    let ops = EXT2_OPS.lock();
    if let Some(new_fn) = ops.new {
        new_fn(image, size).0
    } else {
        core::ptr::null_mut()
    }
}

/// Bridge callback for ModularFsOps::destroy
extern "C" fn ext2_modular_destroy(handle: *mut u8) {
    let ops = EXT2_OPS.lock();
    if let Some(destroy_fn) = ops.destroy {
        destroy_fn(Ext2Handle(handle));
    }
}

/// Bridge callback for ModularFsOps::lookup
extern "C" fn ext2_modular_lookup_bridge(
    handle: *mut u8,
    path: *const u8,
    path_len: usize,
    out: *mut super::traits::ModularFileHandle,
) -> i32 {
    if handle.is_null() || path.is_null() || out.is_null() {
        return -1;
    }
    
    let ops = EXT2_OPS.lock();
    let lookup_fn = match ops.lookup {
        Some(f) => f,
        None => return -1,
    };
    drop(ops);
    
    // Create a temporary FileRefHandle for the ext2 lookup
    let mut file_ref = FileRefHandle {
        fs: Ext2Handle(handle),
        inode: 0,
        size: 0,
        mode: 0,
        blocks: 0,
        mtime: 0,
        nlink: 0,
        uid: 0,
        gid: 0,
    };
    
    let ret = lookup_fn(Ext2Handle(handle), path, path_len, &mut file_ref);
    
    if ret == 0 {
        // Convert FileRefHandle to ModularFileHandle
        unsafe {
            (*out).inode = file_ref.inode;
            (*out).size = file_ref.size;
            (*out).mode = file_ref.mode;
            (*out).blocks = file_ref.blocks;
            (*out).mtime = file_ref.mtime;
            (*out).nlink = file_ref.nlink;
            (*out).uid = file_ref.uid;
            (*out).gid = file_ref.gid;
            // fs_index and fs_handle will be set by the caller
        }
    }
    
    ret
}

/// Bridge callback for ModularFsOps::read_at
extern "C" fn ext2_modular_read_at_bridge(
    file: *const super::traits::ModularFileHandle,
    offset: usize,
    buf: *mut u8,
    len: usize,
) -> i32 {
    if file.is_null() || buf.is_null() {
        return -1;
    }
    
    let ops = EXT2_OPS.lock();
    let read_fn = match ops.read_at {
        Some(f) => f,
        None => return -1,
    };
    drop(ops);
    
    // Convert ModularFileHandle back to FileRefHandle
    let file_handle = unsafe { &*file };
    let file_ref = FileRefHandle {
        fs: Ext2Handle(file_handle.fs_handle),
        inode: file_handle.inode,
        size: file_handle.size,
        mode: file_handle.mode,
        blocks: file_handle.blocks,
        mtime: file_handle.mtime,
        nlink: file_handle.nlink,
        uid: file_handle.uid,
        gid: file_handle.gid,
    };
    
    read_fn(&file_ref, offset, buf, len)
}

/// Bridge callback for ModularFsOps::write_at
extern "C" fn ext2_modular_write_at_bridge(
    file: *const super::traits::ModularFileHandle,
    offset: usize,
    data: *const u8,
    len: usize,
) -> i32 {
    if file.is_null() || data.is_null() {
        return -1;
    }
    
    let ops = EXT2_OPS.lock();
    let write_fn = match ops.write_at {
        Some(f) => f,
        None => return -7, // ReadOnly error
    };
    drop(ops);
    
    // Convert ModularFileHandle back to FileRefHandle
    let file_handle = unsafe { &*file };
    let file_ref = FileRefHandle {
        fs: Ext2Handle(file_handle.fs_handle),
        inode: file_handle.inode,
        size: file_handle.size,
        mode: file_handle.mode,
        blocks: file_handle.blocks,
        mtime: file_handle.mtime,
        nlink: file_handle.nlink,
        uid: file_handle.uid,
        gid: file_handle.gid,
    };
    
    write_fn(&file_ref, offset, data, len)
}

/// Bridge callback for ModularFsOps::list_dir
extern "C" fn ext2_modular_list_dir_bridge(
    handle: *mut u8,
    path: *const u8,
    path_len: usize,
    cb: super::traits::ModularDirCallback,
    ctx: *mut u8,
) -> i32 {
    if handle.is_null() || path.is_null() {
        return -1;
    }
    
    let ops = EXT2_OPS.lock();
    let list_fn = match ops.list_dir {
        Some(f) => f,
        None => return -1,
    };
    drop(ops);
    
    // The callback signatures are compatible, so we can cast directly
    list_fn(Ext2Handle(handle), path, path_len, cb, ctx);
    0
}

/// Bridge callback for ModularFsOps::get_stats
extern "C" fn ext2_modular_get_stats_bridge(
    handle: *mut u8,
    stats: *mut super::traits::FsStats,
) -> i32 {
    if handle.is_null() || stats.is_null() {
        return -1;
    }
    
    let ops = EXT2_OPS.lock();
    let get_stats_fn = match ops.get_stats {
        Some(f) => f,
        None => return -1,
    };
    drop(ops);
    
    // Create a temporary Ext2Stats
    let mut ext2_stats = Ext2Stats::default();
    let ret = get_stats_fn(Ext2Handle(handle), &mut ext2_stats);
    
    if ret == 0 {
        // Convert Ext2Stats to FsStats
        unsafe {
            (*stats).total_blocks = ext2_stats.blocks_count as u64;
            (*stats).free_blocks = ext2_stats.free_blocks_count as u64;
            (*stats).avail_blocks = ext2_stats.free_blocks_count as u64;
            (*stats).total_inodes = ext2_stats.inodes_count as u64;
            (*stats).free_inodes = ext2_stats.free_inodes_count as u64;
            (*stats).block_size = ext2_stats.block_size;
            (*stats).name_max = 255;
            (*stats).fs_type = 0xEF53; // EXT2_SUPER_MAGIC
        }
    }
    
    ret
}

/// Bridge callback for ModularFsOps::set_writable
extern "C" fn ext2_modular_set_writable_bridge(writable: bool) {
    let ops = EXT2_OPS.lock();
    if let Some(set_writable_fn) = ops.set_writable {
        set_writable_fn(writable);
    }
    drop(ops);
    *EXT2_WRITABLE.lock() = writable;
}

/// Bridge callback for ModularFsOps::is_writable
extern "C" fn ext2_modular_is_writable_bridge() -> bool {
    *EXT2_WRITABLE.lock()
}

/// Bridge callback for ModularFsOps::create_file
extern "C" fn ext2_modular_create_file_bridge(
    handle: *mut u8,
    path: *const u8,
    path_len: usize,
    mode: u16,
) -> i32 {
    if handle.is_null() || path.is_null() {
        return -1;
    }
    
    let ops = EXT2_OPS.lock();
    let create_fn = match ops.create_file {
        Some(f) => f,
        None => return -7, // ReadOnly error
    };
    drop(ops);
    
    create_fn(Ext2Handle(handle), path, path_len, mode)
}

/// Register ext2 to the modular filesystem registry
/// This bridges the legacy ext2 API to the new unified ModularFsOps interface
fn register_to_modular_fs_registry() {
    use super::traits::{register_modular_fs, ModularFsOps};
    
    let ops = ModularFsOps {
        fs_type: "ext2",
        new_from_image: Some(ext2_modular_new_from_image),
        destroy: Some(ext2_modular_destroy),
        lookup: Some(ext2_modular_lookup_bridge),
        read_at: Some(ext2_modular_read_at_bridge),
        write_at: Some(ext2_modular_write_at_bridge),
        list_dir: Some(ext2_modular_list_dir_bridge),
        get_stats: Some(ext2_modular_get_stats_bridge),
        set_writable: Some(ext2_modular_set_writable_bridge),
        is_writable: Some(ext2_modular_is_writable_bridge),
        create_file: Some(ext2_modular_create_file_bridge),
        mkdir: None,
        unlink: None,
        rmdir: None,
        rename: None,
        sync: None,
    };
    
    if let Some(index) = register_modular_fs(ops) {
        *EXT2_REGISTRY_INDEX.lock() = Some(index);
        crate::kinfo!("ext2_modular: registered to modular fs registry at index {}", index);
    } else {
        crate::kwarn!("ext2_modular: failed to register to modular fs registry");
    }
}

/// Get the ext2 registry index (for use when converting FileRefHandle to ModularFileHandle)
pub fn get_registry_index() -> Option<u8> {
    *EXT2_REGISTRY_INDEX.lock()
}
