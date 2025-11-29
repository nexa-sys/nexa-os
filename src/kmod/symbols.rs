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
        // =====================================================================
        // Logging functions
        // =====================================================================
        register_symbol("kmod_log_info", kmod_log_info as *const () as u64, SymbolType::Function);
        register_symbol("kmod_log_error", kmod_log_error as *const () as u64, SymbolType::Function);
        register_symbol("kmod_log_warn", kmod_log_warn as *const () as u64, SymbolType::Function);
        register_symbol("kmod_log_debug", kmod_log_debug as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Memory allocation functions
        // =====================================================================
        register_symbol("kmod_alloc", kmod_alloc as *const () as u64, SymbolType::Function);
        register_symbol("kmod_dealloc", kmod_dealloc as *const () as u64, SymbolType::Function);
        register_symbol("kmod_realloc", kmod_realloc as *const () as u64, SymbolType::Function);
        register_symbol("kmod_alloc_zeroed", kmod_alloc_zeroed as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Filesystem registration
        // =====================================================================
        register_symbol("kmod_register_fs", kmod_register_fs as *const () as u64, SymbolType::Function);
        register_symbol("kmod_unregister_fs", kmod_unregister_fs as *const () as u64, SymbolType::Function);
        register_symbol("register_fs_driver", crate::fs::traits::register_fs_driver as *const () as u64, SymbolType::Function);
        register_symbol("unregister_fs_driver", crate::fs::traits::unregister_fs_driver as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Spinlock functions
        // =====================================================================
        register_symbol("kmod_spinlock_init", kmod_spinlock_init as *const () as u64, SymbolType::Function);
        register_symbol("kmod_spinlock_lock", kmod_spinlock_lock as *const () as u64, SymbolType::Function);
        register_symbol("kmod_spinlock_unlock", kmod_spinlock_unlock as *const () as u64, SymbolType::Function);
        register_symbol("kmod_spinlock_trylock", kmod_spinlock_trylock as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Memory operations (kmod_* names)
        // =====================================================================
        register_symbol("kmod_memcpy", kmod_memcpy as *const () as u64, SymbolType::Function);
        register_symbol("kmod_memset", kmod_memset as *const () as u64, SymbolType::Function);
        register_symbol("kmod_memcmp", kmod_memcmp as *const () as u64, SymbolType::Function);
        register_symbol("kmod_memmove", kmod_memmove as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Standard C memory operations (for Rust compiler-generated calls)
        // =====================================================================
        register_symbol("memcpy", kmod_memcpy as *const () as u64, SymbolType::Function);
        register_symbol("memset", kmod_memset as *const () as u64, SymbolType::Function);
        register_symbol("memcmp", kmod_memcmp as *const () as u64, SymbolType::Function);
        register_symbol("memmove", kmod_memmove as *const () as u64, SymbolType::Function);
        register_symbol("bcopy", kmod_memmove as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // String operations
        // =====================================================================
        register_symbol("kmod_strlen", kmod_strlen as *const () as u64, SymbolType::Function);
        register_symbol("kmod_strcmp", kmod_strcmp as *const () as u64, SymbolType::Function);
        register_symbol("kmod_strncmp", kmod_strncmp as *const () as u64, SymbolType::Function);
        register_symbol("kmod_strcpy", kmod_strcpy as *const () as u64, SymbolType::Function);
        register_symbol("kmod_strncpy", kmod_strncpy as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Device registration
        // =====================================================================
        register_symbol("kmod_register_blkdev", kmod_register_blkdev as *const () as u64, SymbolType::Function);
        register_symbol("kmod_unregister_blkdev", kmod_unregister_blkdev as *const () as u64, SymbolType::Function);
        register_symbol("kmod_register_chrdev", kmod_register_chrdev as *const () as u64, SymbolType::Function);
        register_symbol("kmod_unregister_chrdev", kmod_unregister_chrdev as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Time and scheduling
        // =====================================================================
        register_symbol("kmod_get_jiffies", kmod_get_jiffies as *const () as u64, SymbolType::Function);
        register_symbol("kmod_msleep", kmod_msleep as *const () as u64, SymbolType::Function);
        register_symbol("kmod_udelay", kmod_udelay as *const () as u64, SymbolType::Function);
        register_symbol("kmod_schedule", kmod_schedule as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Interrupt handling
        // =====================================================================
        register_symbol("kmod_request_irq", kmod_request_irq as *const () as u64, SymbolType::Function);
        register_symbol("kmod_free_irq", kmod_free_irq as *const () as u64, SymbolType::Function);
        register_symbol("kmod_disable_irq", kmod_disable_irq as *const () as u64, SymbolType::Function);
        register_symbol("kmod_enable_irq", kmod_enable_irq as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Module dependencies
        // =====================================================================
        register_symbol("kmod_module_get", kmod_module_get as *const () as u64, SymbolType::Function);
        register_symbol("kmod_module_put", kmod_module_put as *const () as u64, SymbolType::Function);
        register_symbol("kmod_try_module_get", kmod_try_module_get as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Printing/formatting
        // =====================================================================
        register_symbol("kmod_printk", kmod_printk as *const () as u64, SymbolType::Function);
        
        // =====================================================================
        // Panic/BUG
        // =====================================================================
        register_symbol("kmod_panic", kmod_panic as *const () as u64, SymbolType::Function);
        register_symbol("kmod_bug", kmod_bug as *const () as u64, SymbolType::Function);
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

// ============================================================================
// Additional Kernel API Functions
// ============================================================================

/// Allocate zeroed memory for a module
#[no_mangle]
pub extern "C" fn kmod_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    use alloc::alloc::{alloc_zeroed, Layout};
    
    if size == 0 {
        return ptr::null_mut();
    }
    
    let align = if align == 0 { 8 } else { align };
    if let Ok(layout) = Layout::from_size_align(size, align) {
        unsafe { alloc_zeroed(layout) }
    } else {
        ptr::null_mut()
    }
}

/// Try to acquire a spinlock (non-blocking)
#[no_mangle]
pub extern "C" fn kmod_spinlock_trylock(lock: *mut u64) -> i32 {
    if lock.is_null() {
        return 0;
    }
    
    use core::sync::atomic::{AtomicU64, Ordering};
    unsafe {
        let atomic = &*(lock as *const AtomicU64);
        if atomic.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            1 // Lock acquired
        } else {
            0 // Lock not acquired
        }
    }
}

/// Memory move (handles overlapping regions)
#[no_mangle]
pub extern "C" fn kmod_memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if dest.is_null() || src.is_null() {
        return dest;
    }
    
    unsafe {
        ptr::copy(src, dest, n);
    }
    dest
}

/// String length
#[no_mangle]
pub extern "C" fn kmod_strlen(s: *const u8) -> usize {
    if s.is_null() {
        return 0;
    }
    
    unsafe {
        let mut len = 0;
        while *s.add(len) != 0 {
            len += 1;
        }
        len
    }
}

/// String compare
#[no_mangle]
pub extern "C" fn kmod_strcmp(s1: *const u8, s2: *const u8) -> i32 {
    if s1.is_null() || s2.is_null() {
        return 0;
    }
    
    unsafe {
        let mut i = 0;
        loop {
            let c1 = *s1.add(i);
            let c2 = *s2.add(i);
            if c1 != c2 || c1 == 0 {
                return (c1 as i32) - (c2 as i32);
            }
            i += 1;
        }
    }
}

/// String compare (limited length)
#[no_mangle]
pub extern "C" fn kmod_strncmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    if s1.is_null() || s2.is_null() || n == 0 {
        return 0;
    }
    
    unsafe {
        for i in 0..n {
            let c1 = *s1.add(i);
            let c2 = *s2.add(i);
            if c1 != c2 || c1 == 0 {
                return (c1 as i32) - (c2 as i32);
            }
        }
        0
    }
}

/// String copy
#[no_mangle]
pub extern "C" fn kmod_strcpy(dest: *mut u8, src: *const u8) -> *mut u8 {
    if dest.is_null() || src.is_null() {
        return dest;
    }
    
    unsafe {
        let mut i = 0;
        loop {
            let c = *src.add(i);
            *dest.add(i) = c;
            if c == 0 {
                break;
            }
            i += 1;
        }
    }
    dest
}

/// String copy (limited length)
#[no_mangle]
pub extern "C" fn kmod_strncpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if dest.is_null() || src.is_null() {
        return dest;
    }
    
    unsafe {
        let mut i = 0;
        while i < n {
            let c = *src.add(i);
            *dest.add(i) = c;
            if c == 0 {
                // Fill rest with zeros
                while i < n {
                    *dest.add(i) = 0;
                    i += 1;
                }
                break;
            }
            i += 1;
        }
    }
    dest
}

// ============================================================================
// Device Registration (stubs for now)
// ============================================================================

/// Register a block device driver
#[no_mangle]
pub extern "C" fn kmod_register_blkdev(major: u32, name: *const u8, name_len: usize) -> i32 {
    if name.is_null() {
        return -1;
    }
    
    unsafe {
        let name_bytes = core::slice::from_raw_parts(name, name_len);
        if let Ok(dev_name) = core::str::from_utf8(name_bytes) {
            crate::kinfo!("Registering block device: {} (major={})", dev_name, major);
            0
        } else {
            -1
        }
    }
}

/// Unregister a block device driver
#[no_mangle]
pub extern "C" fn kmod_unregister_blkdev(major: u32, name: *const u8, name_len: usize) -> i32 {
    if name.is_null() {
        return -1;
    }
    
    unsafe {
        let name_bytes = core::slice::from_raw_parts(name, name_len);
        if let Ok(dev_name) = core::str::from_utf8(name_bytes) {
            crate::kinfo!("Unregistering block device: {} (major={})", dev_name, major);
            0
        } else {
            -1
        }
    }
}

/// Register a character device driver
#[no_mangle]
pub extern "C" fn kmod_register_chrdev(major: u32, name: *const u8, name_len: usize) -> i32 {
    if name.is_null() {
        return -1;
    }
    
    unsafe {
        let name_bytes = core::slice::from_raw_parts(name, name_len);
        if let Ok(dev_name) = core::str::from_utf8(name_bytes) {
            crate::kinfo!("Registering character device: {} (major={})", dev_name, major);
            0
        } else {
            -1
        }
    }
}

/// Unregister a character device driver
#[no_mangle]
pub extern "C" fn kmod_unregister_chrdev(major: u32, name: *const u8, name_len: usize) -> i32 {
    if name.is_null() {
        return -1;
    }
    
    unsafe {
        let name_bytes = core::slice::from_raw_parts(name, name_len);
        if let Ok(dev_name) = core::str::from_utf8(name_bytes) {
            crate::kinfo!("Unregistering character device: {} (major={})", dev_name, major);
            0
        } else {
            -1
        }
    }
}

// ============================================================================
// Time and Scheduling
// ============================================================================

/// Get current jiffies (system tick count)
#[no_mangle]
pub extern "C" fn kmod_get_jiffies() -> u64 {
    // TODO: Implement actual jiffies counter
    // For now, return a placeholder
    0
}

/// Sleep for specified milliseconds
#[no_mangle]
pub extern "C" fn kmod_msleep(ms: u32) {
    // TODO: Implement actual sleep
    // For now, just spin for a rough approximation
    for _ in 0..ms * 1000 {
        core::hint::spin_loop();
    }
}

/// Delay for specified microseconds (busy wait)
#[no_mangle]
pub extern "C" fn kmod_udelay(us: u32) {
    // Simple busy-wait loop
    for _ in 0..us * 10 {
        core::hint::spin_loop();
    }
}

/// Yield CPU to scheduler
#[no_mangle]
pub extern "C" fn kmod_schedule() {
    // TODO: Call actual scheduler yield
    core::hint::spin_loop();
}

// ============================================================================
// Interrupt Handling (stubs)
// ============================================================================

/// Request an IRQ handler
#[no_mangle]
pub extern "C" fn kmod_request_irq(
    irq: u32,
    handler: extern "C" fn(u32, *mut core::ffi::c_void) -> i32,
    flags: u32,
    name: *const u8,
    dev_id: *mut core::ffi::c_void,
) -> i32 {
    let _ = (handler, flags, dev_id);
    
    if name.is_null() {
        crate::kinfo!("Registering IRQ handler: irq={}", irq);
    } else {
        unsafe {
            let name_str = {
                let mut len = 0;
                while *name.add(len) != 0 && len < 64 {
                    len += 1;
                }
                core::str::from_utf8(core::slice::from_raw_parts(name, len)).unwrap_or("?")
            };
            crate::kinfo!("Registering IRQ handler: irq={} name={}", irq, name_str);
        }
    }
    
    // TODO: Actually register IRQ handler
    0
}

/// Free an IRQ handler
#[no_mangle]
pub extern "C" fn kmod_free_irq(irq: u32, dev_id: *mut core::ffi::c_void) {
    let _ = dev_id;
    crate::kinfo!("Freeing IRQ handler: irq={}", irq);
    // TODO: Actually free IRQ handler
}

/// Disable an IRQ
#[no_mangle]
pub extern "C" fn kmod_disable_irq(irq: u32) {
    crate::kdebug!("Disabling IRQ: {}", irq);
    // TODO: Actual implementation
}

/// Enable an IRQ
#[no_mangle]
pub extern "C" fn kmod_enable_irq(irq: u32) {
    crate::kdebug!("Enabling IRQ: {}", irq);
    // TODO: Actual implementation
}

// ============================================================================
// Module Dependency Management
// ============================================================================

/// Increment reference count for this module (called by dependent modules)
#[no_mangle]
pub extern "C" fn kmod_module_get(name: *const u8, name_len: usize) -> i32 {
    if name.is_null() {
        return -1;
    }
    
    unsafe {
        let name_bytes = core::slice::from_raw_parts(name, name_len);
        if let Ok(mod_name) = core::str::from_utf8(name_bytes) {
            match super::module_get(mod_name) {
                Ok(_) => 0,
                Err(_) => -1,
            }
        } else {
            -1
        }
    }
}

/// Decrement reference count for this module
#[no_mangle]
pub extern "C" fn kmod_module_put(name: *const u8, name_len: usize) -> i32 {
    if name.is_null() {
        return -1;
    }
    
    unsafe {
        let name_bytes = core::slice::from_raw_parts(name, name_len);
        if let Ok(mod_name) = core::str::from_utf8(name_bytes) {
            match super::module_put(mod_name) {
                Ok(_) => 0,
                Err(_) => -1,
            }
        } else {
            -1
        }
    }
}

/// Try to get a module reference (fails if module is not running)
#[no_mangle]
pub extern "C" fn kmod_try_module_get(name: *const u8, name_len: usize) -> i32 {
    if name.is_null() {
        return -1;
    }
    
    unsafe {
        let name_bytes = core::slice::from_raw_parts(name, name_len);
        if let Ok(mod_name) = core::str::from_utf8(name_bytes) {
            match super::try_module_get(mod_name) {
                Ok(_) => 0,
                Err(_) => -1,
            }
        } else {
            -1
        }
    }
}

// ============================================================================
// Kernel Printing
// ============================================================================

/// Print a kernel message (like Linux printk)
#[no_mangle]
pub extern "C" fn kmod_printk(level: i32, msg: *const u8, len: usize) {
    if msg.is_null() || len == 0 {
        return;
    }
    
    unsafe {
        let bytes = core::slice::from_raw_parts(msg, len);
        if let Ok(s) = core::str::from_utf8(bytes) {
            match level {
                0 => crate::kfatal!("[kmod] {}", s),  // KERN_EMERG
                1 => crate::kerror!("[kmod] {}", s),   // KERN_ALERT
                2 => crate::kerror!("[kmod] {}", s),   // KERN_CRIT
                3 => crate::kerror!("[kmod] {}", s),   // KERN_ERR
                4 => crate::kwarn!("[kmod] {}", s),    // KERN_WARNING
                5 => crate::kinfo!("[kmod] {}", s),    // KERN_NOTICE
                6 => crate::kinfo!("[kmod] {}", s),    // KERN_INFO
                _ => crate::kdebug!("[kmod] {}", s),   // KERN_DEBUG
            }
        }
    }
}

// ============================================================================
// Panic/Bug Handling
// ============================================================================

/// Kernel panic from module - wraps the kpanic! macro
#[no_mangle]
pub extern "C" fn kmod_panic(msg: *const u8, len: usize) -> ! {
    if !msg.is_null() && len > 0 {
        unsafe {
            let bytes = core::slice::from_raw_parts(msg, len);
            if let Ok(s) = core::str::from_utf8(bytes) {
                crate::kpanic!("Module panic: {}", s);
            } else {
                crate::kpanic!("Module panic: {}", "<invalid utf8>");
            }
        }
    } else {
        crate::kpanic!("Module panic: {}", "(no message)");
    }
}

/// Report a kernel bug from module
#[no_mangle]
pub extern "C" fn kmod_bug(file: *const u8, file_len: usize, line: u32) {
    if !file.is_null() && file_len > 0 {
        unsafe {
            let bytes = core::slice::from_raw_parts(file, file_len);
            if let Ok(s) = core::str::from_utf8(bytes) {
                crate::kerror!("BUG: at {}:{}", s, line);
            }
        }
    } else {
        crate::kerror!("BUG: at line {}", line);
    }
}
