//! Kernel Symbol Table for Module Support
//!
//! This module exports kernel APIs that loadable modules can use.
//! Similar to Linux's EXPORT_SYMBOL mechanism.
//!
//! # Memory Management
//!
//! The symbol table uses heap allocation for the string table to avoid
//! bloating the kernel binary with large static buffers. Symbol entries
//! are stored in a Vec for dynamic growth.

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr;
use spin::Mutex;

/// Symbol table entry (heap-allocated)
#[derive(Debug, Clone)]
pub struct KernelSymbol {
    /// Symbol name (heap-allocated string)
    pub name: String,
    /// Symbol address
    pub address: u64,
    /// Symbol type
    pub sym_type: SymbolType,
    /// Symbol visibility
    pub visibility: SymbolVisibility,
}

/// Symbol types
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolType {
    /// Function symbol
    Function = 1,
    /// Data symbol
    Data = 2,
    /// Weak symbol (can be overridden)
    Weak = 3,
}

/// Symbol visibility
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolVisibility {
    /// Symbol is globally visible to all modules
    Global = 0,
    /// Symbol is only visible to modules with explicit dependency
    Protected = 1,
    /// Symbol is internal (not exported)
    Hidden = 2,
}

/// Initial capacity for symbol table
const INITIAL_SYMBOL_CAPACITY: usize = 64;

/// Maximum symbols (soft limit, can grow)
const MAX_SYMBOLS_SOFT_LIMIT: usize = 512;

/// Kernel symbol registry (heap-allocated)
pub struct SymbolTable {
    /// Symbols stored in heap-allocated Vec
    symbols: Vec<KernelSymbol>,
    /// Whether the table has been initialized
    initialized: bool,
}

impl SymbolTable {
    /// Create an uninitialized symbol table
    /// Must call init() before use to allocate heap memory
    const fn new_uninit() -> Self {
        Self {
            symbols: Vec::new(),
            initialized: false,
        }
    }

    /// Initialize the symbol table with heap allocation
    pub fn init(&mut self) {
        if self.initialized {
            return;
        }
        self.symbols = Vec::with_capacity(INITIAL_SYMBOL_CAPACITY);
        self.initialized = true;
    }

    /// Register a new symbol with default global visibility
    pub fn register(&mut self, name: &str, address: u64, sym_type: SymbolType) -> bool {
        self.register_with_visibility(name, address, sym_type, SymbolVisibility::Global)
    }

    /// Register a symbol with specified visibility
    pub fn register_with_visibility(
        &mut self,
        name: &str,
        address: u64,
        sym_type: SymbolType,
        visibility: SymbolVisibility,
    ) -> bool {
        if !self.initialized {
            return false;
        }

        // Check soft limit but allow growth if needed
        if self.symbols.len() >= MAX_SYMBOLS_SOFT_LIMIT {
            crate::kwarn!(
                "Symbol table approaching limit: {} symbols",
                self.symbols.len()
            );
        }

        // Check for duplicate
        if self.symbols.iter().any(|s| s.name == name) {
            crate::kwarn!("Duplicate symbol registration: {}", name);
            return false;
        }

        // Add symbol entry (heap-allocated)
        self.symbols.push(KernelSymbol {
            name: String::from(name),
            address,
            sym_type,
            visibility,
        });

        true
    }

    /// Lookup a symbol by name
    pub fn lookup(&self, name: &str) -> Option<u64> {
        self.symbols
            .iter()
            .find(|sym| sym.name == name && sym.visibility != SymbolVisibility::Hidden)
            .map(|sym| sym.address)
    }

    /// Lookup a symbol and return full info
    pub fn lookup_full(&self, name: &str) -> Option<&KernelSymbol> {
        self.symbols
            .iter()
            .find(|sym| sym.name == name && sym.visibility != SymbolVisibility::Hidden)
    }

    /// Get all registered symbols
    pub fn iter(&self) -> impl Iterator<Item = (&str, u64)> + '_ {
        self.symbols
            .iter()
            .filter(|sym| sym.visibility != SymbolVisibility::Hidden)
            .map(|sym| (sym.name.as_str(), sym.address))
    }

    /// Get symbol count
    pub fn count(&self) -> usize {
        self.symbols.len()
    }

    /// Check if initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Unregister a symbol by name
    pub fn unregister(&mut self, name: &str) -> bool {
        if let Some(pos) = self.symbols.iter().position(|sym| sym.name == name) {
            self.symbols.swap_remove(pos);
            true
        } else {
            false
        }
    }

    /// Get memory usage statistics
    pub fn memory_usage(&self) -> SymbolTableStats {
        let string_bytes: usize = self.symbols.iter().map(|s| s.name.len()).sum();
        let entry_bytes = self.symbols.len() * core::mem::size_of::<KernelSymbol>();
        SymbolTableStats {
            symbol_count: self.symbols.len(),
            string_bytes,
            entry_bytes,
            total_bytes: string_bytes + entry_bytes,
        }
    }
}

/// Symbol table memory statistics
#[derive(Debug, Clone, Copy)]
pub struct SymbolTableStats {
    pub symbol_count: usize,
    pub string_bytes: usize,
    pub entry_bytes: usize,
    pub total_bytes: usize,
}

/// Global kernel symbol table (protected by Mutex)
static KERNEL_SYMBOLS: Mutex<SymbolTable> = Mutex::new(SymbolTable::new_uninit());

/// Initialize the kernel symbol table with exported APIs
pub fn init() {
    // Initialize the symbol table (allocates heap memory)
    {
        let mut table = KERNEL_SYMBOLS.lock();
        table.init();
    }

    // Register logging functions
    register_symbol(
        "kmod_log_info",
        kmod_log_info as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_log_error",
        kmod_log_error as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_log_warn",
        kmod_log_warn as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_log_debug",
        kmod_log_debug as *const () as u64,
        SymbolType::Function,
    );

    // Register memory allocation functions
    register_symbol(
        "kmod_alloc",
        kmod_alloc as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_dealloc",
        kmod_dealloc as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_realloc",
        kmod_realloc as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_zalloc",
        kmod_zalloc as *const () as u64,
        SymbolType::Function,
    );

    // Register filesystem functions
    register_symbol(
        "kmod_register_fs",
        kmod_register_fs as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_unregister_fs",
        kmod_unregister_fs as *const () as u64,
        SymbolType::Function,
    );

    // Register spinlock functions
    register_symbol(
        "kmod_spinlock_init",
        kmod_spinlock_init as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_spinlock_lock",
        kmod_spinlock_lock as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_spinlock_unlock",
        kmod_spinlock_unlock as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_spinlock_trylock",
        kmod_spinlock_trylock as *const () as u64,
        SymbolType::Function,
    );

    // Register memory operations
    register_symbol(
        "kmod_memcpy",
        kmod_memcpy as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_memset",
        kmod_memset as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_memcmp",
        kmod_memcmp as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_memmove",
        kmod_memmove as *const () as u64,
        SymbolType::Function,
    );

    // Register string operations
    register_symbol(
        "kmod_strlen",
        kmod_strlen as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_strcmp",
        kmod_strcmp as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_strncmp",
        kmod_strncmp as *const () as u64,
        SymbolType::Function,
    );

    // Register misc kernel APIs
    register_symbol(
        "kmod_get_ticks",
        kmod_get_ticks as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_yield",
        kmod_yield as *const () as u64,
        SymbolType::Function,
    );

    // Register taint/license APIs
    register_symbol(
        "kmod_add_taint",
        kmod_add_taint as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_get_taint",
        kmod_get_taint as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_is_tainted",
        kmod_is_tainted as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_set_license",
        kmod_set_license as *const () as u64,
        SymbolType::Function,
    );

    // Register I/O port access functions (for VirtIO-PCI and legacy devices)
    register_symbol(
        "kmod_inb",
        kmod_inb as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_inw",
        kmod_inw as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_inl",
        crate::net::modular::kmod_inl as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_outb",
        kmod_outb as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_outw",
        kmod_outw as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_outl",
        crate::net::modular::kmod_outl as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_fence",
        crate::net::modular::kmod_fence as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_spin_hint",
        crate::net::modular::kmod_spin_hint as *const () as u64,
        SymbolType::Function,
    );

    // Register swap module API
    register_symbol(
        "kmod_swap_register",
        crate::mm::swap::kmod_swap_register as *const () as u64,
        SymbolType::Function,
    );
    register_symbol(
        "kmod_swap_unregister",
        crate::mm::swap::kmod_swap_unregister as *const () as u64,
        SymbolType::Function,
    );

    crate::kinfo!(
        "Kernel symbol table initialized with {} symbols",
        symbol_count()
    );
}

/// Register a kernel symbol
pub fn register_symbol(name: &str, address: u64, sym_type: SymbolType) -> bool {
    KERNEL_SYMBOLS.lock().register(name, address, sym_type)
}

/// Register a symbol with visibility
pub fn register_symbol_with_visibility(
    name: &str,
    address: u64,
    sym_type: SymbolType,
    visibility: SymbolVisibility,
) -> bool {
    KERNEL_SYMBOLS
        .lock()
        .register_with_visibility(name, address, sym_type, visibility)
}

/// Lookup a kernel symbol by name
pub fn lookup_symbol(name: &str) -> Option<u64> {
    KERNEL_SYMBOLS.lock().lookup(name)
}

/// Lookup a kernel symbol with full info
pub fn lookup_symbol_full(name: &str) -> Option<KernelSymbol> {
    KERNEL_SYMBOLS.lock().lookup_full(name).cloned()
}

/// Get the number of registered symbols
pub fn symbol_count() -> usize {
    KERNEL_SYMBOLS.lock().count()
}

/// Unregister a kernel symbol
pub fn unregister_symbol(name: &str) -> bool {
    KERNEL_SYMBOLS.lock().unregister(name)
}

/// List all exported symbols (returns owned data to avoid lifetime issues)
pub fn list_symbols() -> Vec<(String, u64)> {
    KERNEL_SYMBOLS
        .lock()
        .iter()
        .map(|(name, addr)| (String::from(name), addr))
        .collect()
}

/// Get symbol table memory usage statistics
pub fn get_symbol_stats() -> SymbolTableStats {
    KERNEL_SYMBOLS.lock().memory_usage()
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
pub extern "C" fn kmod_dealloc(ptr: *mut u8, _size: usize, _align: usize) {
    use crate::mm::allocator::kfree;

    if ptr.is_null() {
        return;
    }

    // The original pointer is stored just before the aligned address
    unsafe {
        let ptr_storage = (ptr as usize - 8) as *const *mut u8;
        let raw_ptr = *ptr_storage;
        kfree(raw_ptr);
    }
}

/// Reallocate memory for a module
#[no_mangle]
pub extern "C" fn kmod_realloc(
    ptr: *mut u8,
    old_size: usize,
    new_size: usize,
    align: usize,
) -> *mut u8 {
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
        unsafe {
            *lock = 0;
        }
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
        while atomic
            .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
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
// Additional Kernel APIs
// ============================================================================

/// Allocate zeroed memory for a module with proper alignment
/// 
/// Note: The kernel's global allocator doesn't respect alignment requirements,
/// so we handle alignment manually by over-allocating and storing the original
/// pointer before the aligned address.
#[no_mangle]
pub extern "C" fn kmod_zalloc(size: usize, align: usize) -> *mut u8 {
    use crate::mm::allocator::kalloc;
    
    if size == 0 {
        return ptr::null_mut();
    }

    let align = if align == 0 || align < 8 { 8 } else { align.next_power_of_two() };
    
    // Allocate extra space for alignment and to store the original pointer
    // We need:
    // - size bytes for the actual data
    // - up to (align - 1) bytes for alignment padding
    // - 8 bytes to store the original pointer before the aligned address
    let total_size = size + align - 1 + 8;
    
    let raw_ptr = match kalloc(total_size) {
        Some(p) => p,
        None => return ptr::null_mut(),
    };
    
    // Calculate aligned address, leaving room for the original pointer storage
    let raw_addr = raw_ptr as usize;
    let aligned_addr = (raw_addr + 8 + align - 1) & !(align - 1);
    
    // Store the original pointer just before the aligned address
    unsafe {
        let ptr_storage = (aligned_addr - 8) as *mut *mut u8;
        *ptr_storage = raw_ptr;
        
        // Zero the memory
        ptr::write_bytes(aligned_addr as *mut u8, 0, size);
    }
    
    aligned_addr as *mut u8
}

/// Try to acquire a spinlock without blocking
/// Returns 0 if lock acquired, -1 if lock was already held
#[no_mangle]
pub extern "C" fn kmod_spinlock_trylock(lock: *mut u64) -> i32 {
    if lock.is_null() {
        return -1;
    }

    use core::sync::atomic::{AtomicU64, Ordering};
    unsafe {
        let atomic = &*(lock as *const AtomicU64);
        if atomic
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            0
        } else {
            -1
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

/// Get string length (null-terminated)
#[no_mangle]
pub extern "C" fn kmod_strlen(s: *const u8) -> usize {
    if s.is_null() {
        return 0;
    }

    let mut len = 0;
    unsafe {
        while *s.add(len) != 0 {
            len += 1;
        }
    }
    len
}

/// Compare two null-terminated strings
#[no_mangle]
pub extern "C" fn kmod_strcmp(s1: *const u8, s2: *const u8) -> i32 {
    if s1.is_null() || s2.is_null() {
        return if s1.is_null() && s2.is_null() { 0 } else { -1 };
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

/// Compare two strings up to n bytes
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

/// Get current kernel tick count (for timing)
#[no_mangle]
pub extern "C" fn kmod_get_ticks() -> u64 {
    // Read TSC for timing
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Yield to scheduler (give up CPU time slice)
#[no_mangle]
pub extern "C" fn kmod_yield() {
    // Use pause instruction to hint the CPU we're in a spin-wait
    core::hint::spin_loop();
}

// ============================================================================
// Taint and License APIs for Modules
// ============================================================================

/// Add a taint flag from a module
///
/// Taint flags (same as Linux):
/// - 1: Proprietary module (P)
/// - 2: Forced load (F)
/// - 16384: Unsigned module (E)
/// - 32768: Out-of-tree module (O)
#[no_mangle]
pub extern "C" fn kmod_add_taint(flag: u32) {
    use super::{add_taint, TaintFlag};

    // Map integer flags to TaintFlag enum
    let taint = match flag {
        1 => Some(TaintFlag::ProprietaryModule),
        2 => Some(TaintFlag::ForcedLoad),
        4 => Some(TaintFlag::Smp),
        8 => Some(TaintFlag::ForcedUnload),
        16 => Some(TaintFlag::MachineCheck),
        32 => Some(TaintFlag::BadPage),
        64 => Some(TaintFlag::UserRequest),
        128 => Some(TaintFlag::Die),
        256 => Some(TaintFlag::OverriddenAcpiTable),
        512 => Some(TaintFlag::Warn),
        1024 => Some(TaintFlag::LivePatch),
        2048 => Some(TaintFlag::UnsupportedHardware),
        4096 => Some(TaintFlag::Softlockup),
        8192 => Some(TaintFlag::FirmwareBug),
        16384 => Some(TaintFlag::UnsignedModule),
        32768 => Some(TaintFlag::OutOfTreeModule),
        65536 => Some(TaintFlag::StagingDriver),
        131072 => Some(TaintFlag::RandomizeTampered),
        262144 => Some(TaintFlag::Aux),
        _ => None,
    };

    if let Some(t) = taint {
        add_taint(t);
    }
}

/// Get current kernel taint value
#[no_mangle]
pub extern "C" fn kmod_get_taint() -> u32 {
    super::get_taint()
}

/// Check if a specific taint flag is set
#[no_mangle]
pub extern "C" fn kmod_is_tainted(flag: u32) -> bool {
    (super::get_taint() & flag) != 0
}

/// Set module license and check for tainting
///
/// Known GPL-compatible licenses that don't taint:
/// - "GPL", "GPL v2", "GPL v2+", "Dual MIT/GPL", "Dual BSD/GPL"
///
/// Returns 1 if license taints kernel, 0 otherwise
#[no_mangle]
pub extern "C" fn kmod_set_license(license: *const u8, len: usize) -> i32 {
    use super::{add_taint, LicenseType, TaintFlag};

    if license.is_null() || len == 0 {
        // Unknown license taints kernel
        add_taint(TaintFlag::ProprietaryModule);
        return 1;
    }

    let license_str = unsafe {
        let slice = core::slice::from_raw_parts(license, len);
        core::str::from_utf8(slice).unwrap_or("Unknown")
    };

    let license_type = LicenseType::from_string(license_str);

    if !license_type.is_gpl_compatible() {
        add_taint(TaintFlag::ProprietaryModule);
        crate::kinfo!(
            "Module license '{}' is not GPL-compatible, tainting kernel",
            license_str
        );
        1
    } else {
        crate::kinfo!("Module license '{}' is GPL-compatible", license_str);
        0
    }
}

// ============================================================================
// I/O Port Access Functions (for legacy hardware and VirtIO-PCI)
// ============================================================================

/// Read a byte from an I/O port
#[no_mangle]
pub extern "C" fn kmod_inb(port: u16) -> u8 {
    crate::safety::inb(port)
}

/// Read a word (16-bit) from an I/O port
#[no_mangle]
pub extern "C" fn kmod_inw(port: u16) -> u16 {
    crate::safety::inw(port)
}

// kmod_inl is defined in src/net/modular.rs

/// Write a byte to an I/O port
#[no_mangle]
pub extern "C" fn kmod_outb(port: u16, value: u8) {
    crate::safety::outb(port, value);
}

/// Write a word (16-bit) to an I/O port
#[no_mangle]
pub extern "C" fn kmod_outw(port: u16, value: u16) {
    crate::safety::outw(port, value);
}

// kmod_outl, kmod_fence and kmod_spin_hint are defined in src/net/modular.rs