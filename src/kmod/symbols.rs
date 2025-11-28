//! Kernel Symbol Table for Module Support
//!
//! This module exports kernel APIs that loadable modules can use.
//! Similar to Linux's EXPORT_SYMBOL mechanism.

use alloc::vec::Vec;
use core::ptr;

/// Symbol table entry
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KernelSymbol {
    /// Symbol name (null-terminated in the string table)
    pub name_offset: u32,
    /// Symbol address
    pub address: u64,
    /// Symbol type
    pub sym_type: SymbolType,
}

/// Symbol types
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolType {
    Function = 1,
    Data = 2,
}

/// Maximum number of exported symbols
const MAX_SYMBOLS: usize = 256;

/// Kernel symbol registry
pub struct SymbolTable {
    symbols: [Option<KernelSymbol>; MAX_SYMBOLS],
    names: [u8; 8192], // String table for symbol names
    name_offset: usize,
    count: usize,
}

impl SymbolTable {
    const fn new() -> Self {
        const NONE: Option<KernelSymbol> = None;
        Self {
            symbols: [NONE; MAX_SYMBOLS],
            names: [0; 8192],
            name_offset: 0,
            count: 0,
        }
    }

    /// Register a new symbol
    pub fn register(&mut self, name: &str, address: u64, sym_type: SymbolType) -> bool {
        if self.count >= MAX_SYMBOLS {
            return false;
        }

        let name_len = name.len();
        if self.name_offset + name_len + 1 > self.names.len() {
            return false;
        }

        // Store name in string table
        let name_start = self.name_offset;
        self.names[name_start..name_start + name_len].copy_from_slice(name.as_bytes());
        self.names[name_start + name_len] = 0; // null terminator
        self.name_offset += name_len + 1;

        // Add symbol entry
        self.symbols[self.count] = Some(KernelSymbol {
            name_offset: name_start as u32,
            address,
            sym_type,
        });
        self.count += 1;

        true
    }

    /// Lookup a symbol by name
    pub fn lookup(&self, name: &str) -> Option<u64> {
        for i in 0..self.count {
            if let Some(sym) = &self.symbols[i] {
                let sym_name = self.get_name(sym.name_offset as usize);
                if sym_name == name {
                    return Some(sym.address);
                }
            }
        }
        None
    }

    /// Get symbol name from string table
    fn get_name(&self, offset: usize) -> &str {
        let start = offset;
        let mut end = start;
        while end < self.names.len() && self.names[end] != 0 {
            end += 1;
        }
        core::str::from_utf8(&self.names[start..end]).unwrap_or("")
    }

    /// Get all registered symbols
    pub fn iter(&self) -> impl Iterator<Item = (&str, u64)> + '_ {
        (0..self.count).filter_map(move |i| {
            self.symbols[i].as_ref().map(|sym| {
                (self.get_name(sym.name_offset as usize), sym.address)
            })
        })
    }
}

static mut KERNEL_SYMBOLS: SymbolTable = SymbolTable::new();

/// Initialize the kernel symbol table with exported APIs
pub fn init() {
    unsafe {
        // Logging functions
        register_symbol("kmod_log_info", kmod_log_info as *const () as u64, SymbolType::Function);
        register_symbol("kmod_log_error", kmod_log_error as *const () as u64, SymbolType::Function);
        register_symbol("kmod_log_warn", kmod_log_warn as *const () as u64, SymbolType::Function);
        register_symbol("kmod_log_debug", kmod_log_debug as *const () as u64, SymbolType::Function);
        
        // Memory allocation functions
        register_symbol("kmod_alloc", kmod_alloc as *const () as u64, SymbolType::Function);
        register_symbol("kmod_dealloc", kmod_dealloc as *const () as u64, SymbolType::Function);
        register_symbol("kmod_realloc", kmod_realloc as *const () as u64, SymbolType::Function);
        
        // Filesystem registration
        register_symbol("kmod_register_fs", kmod_register_fs as *const () as u64, SymbolType::Function);
        register_symbol("kmod_unregister_fs", kmod_unregister_fs as *const () as u64, SymbolType::Function);
        
        // Spinlock functions
        register_symbol("kmod_spinlock_init", kmod_spinlock_init as *const () as u64, SymbolType::Function);
        register_symbol("kmod_spinlock_lock", kmod_spinlock_lock as *const () as u64, SymbolType::Function);
        register_symbol("kmod_spinlock_unlock", kmod_spinlock_unlock as *const () as u64, SymbolType::Function);
        
        // Memory operations
        register_symbol("kmod_memcpy", kmod_memcpy as *const () as u64, SymbolType::Function);
        register_symbol("kmod_memset", kmod_memset as *const () as u64, SymbolType::Function);
        register_symbol("kmod_memcmp", kmod_memcmp as *const () as u64, SymbolType::Function);
    }

    crate::kinfo!("Kernel symbol table initialized with {} symbols", symbol_count());
}

/// Register a kernel symbol
pub fn register_symbol(name: &str, address: u64, sym_type: SymbolType) -> bool {
    unsafe { KERNEL_SYMBOLS.register(name, address, sym_type) }
}

/// Lookup a kernel symbol by name
pub fn lookup_symbol(name: &str) -> Option<u64> {
    unsafe { KERNEL_SYMBOLS.lookup(name) }
}

/// Get the number of registered symbols
pub fn symbol_count() -> usize {
    unsafe { KERNEL_SYMBOLS.count }
}

/// List all exported symbols
pub fn list_symbols() -> Vec<(&'static str, u64)> {
    unsafe {
        KERNEL_SYMBOLS.iter().collect()
    }
}

// ============================================================================
// Kernel API Functions (exported to modules)
// ============================================================================

/// Log an info message from a module
#[no_mangle]
pub extern "C" fn kmod_log_info(msg: *const u8, len: usize) {
    if msg.is_null() || len == 0 {
        return;
    }
    unsafe {
        let bytes = core::slice::from_raw_parts(msg, len);
        if let Ok(s) = core::str::from_utf8(bytes) {
            crate::kinfo!("[kmod] {}", s);
        }
    }
}

/// Log an error message from a module
#[no_mangle]
pub extern "C" fn kmod_log_error(msg: *const u8, len: usize) {
    if msg.is_null() || len == 0 {
        return;
    }
    unsafe {
        let bytes = core::slice::from_raw_parts(msg, len);
        if let Ok(s) = core::str::from_utf8(bytes) {
            crate::kerror!("[kmod] {}", s);
        }
    }
}

/// Log a warning message from a module
#[no_mangle]
pub extern "C" fn kmod_log_warn(msg: *const u8, len: usize) {
    if msg.is_null() || len == 0 {
        return;
    }
    unsafe {
        let bytes = core::slice::from_raw_parts(msg, len);
        if let Ok(s) = core::str::from_utf8(bytes) {
            crate::kwarn!("[kmod] {}", s);
        }
    }
}

/// Log a debug message from a module
#[no_mangle]
pub extern "C" fn kmod_log_debug(msg: *const u8, len: usize) {
    if msg.is_null() || len == 0 {
        return;
    }
    unsafe {
        let bytes = core::slice::from_raw_parts(msg, len);
        if let Ok(s) = core::str::from_utf8(bytes) {
            crate::kdebug!("[kmod] {}", s);
        }
    }
}

/// Allocate memory for a module
#[no_mangle]
pub extern "C" fn kmod_alloc(size: usize, align: usize) -> *mut u8 {
    use alloc::alloc::{alloc, Layout};
    
    if size == 0 {
        return ptr::null_mut();
    }
    
    let align = if align == 0 { 8 } else { align };
    if let Ok(layout) = Layout::from_size_align(size, align) {
        unsafe { alloc(layout) }
    } else {
        ptr::null_mut()
    }
}

/// Deallocate memory from a module
#[no_mangle]
pub extern "C" fn kmod_dealloc(ptr: *mut u8, size: usize, align: usize) {
    use alloc::alloc::{dealloc, Layout};
    
    if ptr.is_null() || size == 0 {
        return;
    }
    
    let align = if align == 0 { 8 } else { align };
    if let Ok(layout) = Layout::from_size_align(size, align) {
        unsafe { dealloc(ptr, layout) }
    }
}

/// Reallocate memory for a module
#[no_mangle]
pub extern "C" fn kmod_realloc(ptr: *mut u8, old_size: usize, new_size: usize, align: usize) -> *mut u8 {
    use alloc::alloc::{realloc, Layout};
    
    if new_size == 0 {
        kmod_dealloc(ptr, old_size, align);
        return ptr::null_mut();
    }
    
    if ptr.is_null() {
        return kmod_alloc(new_size, align);
    }
    
    let align = if align == 0 { 8 } else { align };
    if let Ok(layout) = Layout::from_size_align(old_size, align) {
        unsafe { realloc(ptr, layout, new_size) }
    } else {
        ptr::null_mut()
    }
}

/// Filesystem registration callback type
pub type FsInitFn = extern "C" fn(*const u8, usize) -> *mut core::ffi::c_void;
pub type FsLookupFn = extern "C" fn(*mut core::ffi::c_void, *const u8, usize) -> i32;

/// Register a filesystem driver
#[no_mangle]
pub extern "C" fn kmod_register_fs(
    name: *const u8,
    name_len: usize,
    init_fn: FsInitFn,
    _lookup_fn: FsLookupFn,
) -> i32 {
    if name.is_null() || name_len == 0 {
        return -1;
    }
    
    unsafe {
        let name_bytes = core::slice::from_raw_parts(name, name_len);
        if let Ok(fs_name) = core::str::from_utf8(name_bytes) {
            crate::kinfo!("Registering filesystem: {}", fs_name);
            // Store the fs driver info for later use
            // The actual registration happens through the VFS layer
            let _ = init_fn;
            0
        } else {
            -1
        }
    }
}

/// Unregister a filesystem driver
#[no_mangle]
pub extern "C" fn kmod_unregister_fs(name: *const u8, name_len: usize) -> i32 {
    if name.is_null() || name_len == 0 {
        return -1;
    }
    
    unsafe {
        let name_bytes = core::slice::from_raw_parts(name, name_len);
        if let Ok(fs_name) = core::str::from_utf8(name_bytes) {
            crate::kinfo!("Unregistering filesystem: {}", fs_name);
            0
        } else {
            -1
        }
    }
}

/// Initialize a spinlock
#[no_mangle]
pub extern "C" fn kmod_spinlock_init(lock: *mut u64) {
    if !lock.is_null() {
        unsafe { *lock = 0; }
    }
}

/// Acquire a spinlock
#[no_mangle]
pub extern "C" fn kmod_spinlock_lock(lock: *mut u64) {
    if lock.is_null() {
        return;
    }
    
    use core::sync::atomic::{AtomicU64, Ordering};
    unsafe {
        let atomic = &*(lock as *const AtomicU64);
        while atomic.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }
}

/// Release a spinlock
#[no_mangle]
pub extern "C" fn kmod_spinlock_unlock(lock: *mut u64) {
    if lock.is_null() {
        return;
    }
    
    use core::sync::atomic::{AtomicU64, Ordering};
    unsafe {
        let atomic = &*(lock as *const AtomicU64);
        atomic.store(0, Ordering::Release);
    }
}

/// Memory copy
#[no_mangle]
pub extern "C" fn kmod_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if dest.is_null() || src.is_null() {
        return dest;
    }
    
    unsafe {
        ptr::copy_nonoverlapping(src, dest, n);
    }
    dest
}

/// Memory set
#[no_mangle]
pub extern "C" fn kmod_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    if dest.is_null() {
        return dest;
    }
    
    unsafe {
        ptr::write_bytes(dest, c as u8, n);
    }
    dest
}

/// Memory compare
#[no_mangle]
pub extern "C" fn kmod_memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    if s1.is_null() || s2.is_null() {
        return 0;
    }
    
    unsafe {
        let slice1 = core::slice::from_raw_parts(s1, n);
        let slice2 = core::slice::from_raw_parts(s2, n);
        
        for (a, b) in slice1.iter().zip(slice2.iter()) {
            if a != b {
                return (*a as i32) - (*b as i32);
            }
        }
        0
    }
}
