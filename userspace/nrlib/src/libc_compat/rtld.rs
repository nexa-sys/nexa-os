//! Dynamic Linker Runtime - Shared Library Manager
//!
//! This module provides the runtime dynamic linker functionality for loading
//! and managing shared libraries (shared objects / .so files).

use crate::c_int;
use crate::libc_compat::elf::*;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of loaded libraries
pub const MAX_LOADED_LIBS: usize = 64;

/// Maximum length of library path
pub const MAX_LIB_PATH: usize = 256;

/// Maximum number of DT_NEEDED dependencies per library
pub const MAX_NEEDED: usize = 32;

/// Default library search paths
pub const DEFAULT_LIB_PATHS: &[&str] = &[
    "/lib64",
    "/lib",
    "/usr/lib64",
    "/usr/lib",
    "/usr/local/lib64",
    "/usr/local/lib",
];

// ============================================================================
// RTLD Flags (dlopen flags)
// ============================================================================

/// Lazy function call binding
pub const RTLD_LAZY: c_int = 0x00001;
/// Immediate function call binding
pub const RTLD_NOW: c_int = 0x00002;
/// Mask for binding time flags
pub const RTLD_BINDING_MASK: c_int = 0x3;
/// Do not delete object when closed
pub const RTLD_NODELETE: c_int = 0x01000;
/// Make symbols available for other libraries
pub const RTLD_GLOBAL: c_int = 0x00100;
/// Symbols are not available to other libraries
pub const RTLD_LOCAL: c_int = 0x00000;
/// Do not load, return handle if already loaded
pub const RTLD_NOLOAD: c_int = 0x00004;
/// Deep binding: use local scope first
pub const RTLD_DEEPBIND: c_int = 0x00008;

/// Special handle: search in default shared objects
pub const RTLD_DEFAULT: *mut core::ffi::c_void = 0 as *mut core::ffi::c_void;
/// Special handle: search after this shared object
pub const RTLD_NEXT: *mut core::ffi::c_void = (-1isize) as *mut core::ffi::c_void;

// ============================================================================
// Error Codes
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum DlError {
    None = 0,
    FileNotFound = -1,
    InvalidElf = -2,
    UnsupportedArch = -3,
    LoadFailed = -4,
    SymbolNotFound = -5,
    TooManyLibs = -6,
    DependencyFailed = -7,
    RelocationFailed = -8,
    InvalidHandle = -9,
    NotLoaded = -10,
}

impl DlError {
    pub fn as_str(&self) -> &'static str {
        match self {
            DlError::None => "no error",
            DlError::FileNotFound => "shared object not found",
            DlError::InvalidElf => "invalid ELF format",
            DlError::UnsupportedArch => "unsupported architecture",
            DlError::LoadFailed => "failed to load shared object",
            DlError::SymbolNotFound => "undefined symbol",
            DlError::TooManyLibs => "too many loaded libraries",
            DlError::DependencyFailed => "failed to load dependency",
            DlError::RelocationFailed => "relocation processing failed",
            DlError::InvalidHandle => "invalid library handle",
            DlError::NotLoaded => "shared object not loaded",
        }
    }
}

// ============================================================================
// Loaded Library Structure
// ============================================================================

/// State of a loaded library
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LibraryState {
    /// Slot is free
    Free = 0,
    /// Library is being loaded (dependencies)
    Loading = 1,
    /// Library is loaded and relocated
    Loaded = 2,
    /// Init functions have been called
    Initialized = 3,
    /// Library is being unloaded
    Unloading = 4,
}

/// Information about a loaded shared library
#[repr(C)]
pub struct LoadedLibrary {
    /// Library state
    pub state: LibraryState,

    /// Library pathname (null-terminated)
    pub path: [u8; MAX_LIB_PATH],
    /// Length of path
    pub path_len: usize,

    /// Base address where library is loaded
    pub base_addr: u64,
    /// Size of loaded memory region
    pub mem_size: u64,

    /// Load bias (difference between runtime and link-time addresses)
    pub load_bias: i64,

    /// Entry point (if any)
    pub entry: u64,

    /// Parsed dynamic section info
    pub dyn_info: DynamicInfo,

    /// Address of dynamic section in memory
    pub dynamic_addr: u64,
    /// Number of dynamic entries
    pub dynamic_count: usize,

    /// Reference count
    pub refcount: u32,

    /// Flags from dlopen
    pub flags: c_int,

    /// SONAME from dynamic section (offset in strtab, 0 if none)
    pub soname_offset: u32,

    /// Index in loaded libraries array (for handle)
    pub index: usize,

    /// Number of DT_NEEDED entries
    pub needed_count: usize,
    /// DT_NEEDED string offsets (in library's strtab)
    pub needed: [u32; MAX_NEEDED],
    /// Indices of loaded dependency libraries
    pub needed_libs: [usize; MAX_NEEDED],

    /// Program headers address (runtime)
    pub phdr_addr: u64,
    /// Number of program headers
    pub phnum: u16,

    /// Init function address (0 if none)
    pub init_fn: u64,
    /// Fini function address (0 if none)
    pub fini_fn: u64,

    /// Whether init has been called
    pub init_called: bool,
}

impl LoadedLibrary {
    /// Create a new empty library slot
    pub const fn new() -> Self {
        Self {
            state: LibraryState::Free,
            path: [0; MAX_LIB_PATH],
            path_len: 0,
            base_addr: 0,
            mem_size: 0,
            load_bias: 0,
            entry: 0,
            dyn_info: DynamicInfo {
                strtab: 0,
                strsz: 0,
                symtab: 0,
                syment: 0,
                rela: 0,
                relasz: 0,
                relaent: 0,
                rel: 0,
                relsz: 0,
                relent: 0,
                jmprel: 0,
                pltrelsz: 0,
                pltrel: 0,
                pltgot: 0,
                init: 0,
                fini: 0,
                init_array: 0,
                init_arraysz: 0,
                fini_array: 0,
                fini_arraysz: 0,
                hash: 0,
                gnu_hash: 0,
                flags: 0,
                flags_1: 0,
                relacount: 0,
                relcount: 0,
                versym: 0,
                verneed: 0,
                verneednum: 0,
                soname: 0,
                rpath: 0,
                runpath: 0,
            },
            dynamic_addr: 0,
            dynamic_count: 0,
            refcount: 0,
            flags: 0,
            soname_offset: 0,
            index: 0,
            needed_count: 0,
            needed: [0; MAX_NEEDED],
            needed_libs: [0; MAX_NEEDED],
            phdr_addr: 0,
            phnum: 0,
            init_fn: 0,
            fini_fn: 0,
            init_called: false,
        }
    }

    /// Check if this slot is free
    pub fn is_free(&self) -> bool {
        self.state == LibraryState::Free
    }

    /// Get the library's SONAME or filename
    pub fn get_name(&self) -> &[u8] {
        &self.path[..self.path_len]
    }

    /// Set the library path
    pub fn set_path(&mut self, path: &[u8]) {
        let len = core::cmp::min(path.len(), MAX_LIB_PATH - 1);
        self.path[..len].copy_from_slice(&path[..len]);
        self.path[len] = 0;
        self.path_len = len;
    }

    /// Get the string table address (with load bias applied)
    pub fn strtab(&self) -> *const u8 {
        if self.dyn_info.strtab == 0 {
            ptr::null()
        } else {
            (self.dyn_info.strtab as i64 + self.load_bias) as *const u8
        }
    }

    /// Get the symbol table address (with load bias applied)
    pub fn symtab(&self) -> *const Elf64Sym {
        if self.dyn_info.symtab == 0 {
            ptr::null()
        } else {
            (self.dyn_info.symtab as i64 + self.load_bias) as *const Elf64Sym
        }
    }

    /// Get a string from this library's string table
    pub unsafe fn get_string(&self, offset: u32) -> &'static [u8] {
        let strtab = self.strtab();
        if strtab.is_null() {
            return &[];
        }
        super::elf::get_string(strtab, offset)
    }

    /// Check if an address falls within this library
    pub fn contains_addr(&self, addr: u64) -> bool {
        addr >= self.base_addr && addr < self.base_addr + self.mem_size
    }
}

// ============================================================================
// Library Manager (Global State)
// ============================================================================

/// Global library manager state
pub struct LibraryManager {
    /// Array of loaded libraries
    pub libraries: [LoadedLibrary; MAX_LOADED_LIBS],
    /// Number of loaded libraries (including main executable)
    pub count: usize,
    /// Lock for thread safety
    pub lock: AtomicU32,
    /// Last error code
    pub last_error: DlError,
    /// Error message buffer
    pub error_msg: [u8; 256],
    /// Error message length
    pub error_len: usize,
}

impl LibraryManager {
    /// Create a new library manager
    pub const fn new() -> Self {
        const EMPTY_LIB: LoadedLibrary = LoadedLibrary::new();
        Self {
            libraries: [EMPTY_LIB; MAX_LOADED_LIBS],
            count: 0,
            lock: AtomicU32::new(0),
            last_error: DlError::None,
            error_msg: [0; 256],
            error_len: 0,
        }
    }

    /// Acquire the lock
    pub fn acquire(&self) {
        while self
            .lock
            .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    /// Release the lock
    pub fn release(&self) {
        self.lock.store(0, Ordering::Release);
    }

    /// Set error message
    pub fn set_error(&mut self, error: DlError, extra: Option<&[u8]>) {
        self.last_error = error;
        let base_msg = error.as_str().as_bytes();
        let mut len = core::cmp::min(base_msg.len(), self.error_msg.len() - 1);
        self.error_msg[..len].copy_from_slice(&base_msg[..len]);

        if let Some(extra) = extra {
            if len + 2 + extra.len() < self.error_msg.len() {
                self.error_msg[len] = b':';
                self.error_msg[len + 1] = b' ';
                len += 2;
                let extra_len = core::cmp::min(extra.len(), self.error_msg.len() - len - 1);
                self.error_msg[len..len + extra_len].copy_from_slice(&extra[..extra_len]);
                len += extra_len;
            }
        }

        self.error_msg[len] = 0;
        self.error_len = len;
    }

    /// Clear error
    pub fn clear_error(&mut self) {
        self.last_error = DlError::None;
        self.error_len = 0;
        self.error_msg[0] = 0;
    }

    /// Get error message
    pub fn get_error(&self) -> Option<&[u8]> {
        if self.last_error == DlError::None {
            None
        } else {
            Some(&self.error_msg[..self.error_len])
        }
    }

    /// Find a free slot in the libraries array
    pub fn find_free_slot(&self) -> Option<usize> {
        for i in 0..MAX_LOADED_LIBS {
            if self.libraries[i].is_free() {
                return Some(i);
            }
        }
        None
    }

    /// Find a library by path
    pub fn find_by_path(&self, path: &[u8]) -> Option<usize> {
        for i in 0..self.count {
            if !self.libraries[i].is_free() {
                let lib_path = self.libraries[i].get_name();
                if lib_path == path {
                    return Some(i);
                }
                // Also check just the filename part
                if let Some(pos) = path.iter().rposition(|&c| c == b'/') {
                    let filename = &path[pos + 1..];
                    if let Some(lib_pos) = lib_path.iter().rposition(|&c| c == b'/') {
                        let lib_filename = &lib_path[lib_pos + 1..];
                        if lib_filename == filename {
                            return Some(i);
                        }
                    } else if lib_path == filename {
                        return Some(i);
                    }
                }
            }
        }
        None
    }

    /// Find a library by SONAME
    pub fn find_by_soname(&self, soname: &[u8]) -> Option<usize> {
        for i in 0..self.count {
            let lib = &self.libraries[i];
            if lib.is_free() {
                continue;
            }

            if lib.soname_offset != 0 && lib.dyn_info.strtab != 0 {
                let lib_soname = unsafe { lib.get_string(lib.soname_offset) };
                if lib_soname == soname {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Find a library containing an address
    pub fn find_by_addr(&self, addr: u64) -> Option<usize> {
        for i in 0..MAX_LOADED_LIBS {
            if !self.libraries[i].is_free() && self.libraries[i].contains_addr(addr) {
                return Some(i);
            }
        }
        None
    }

    /// Get a library by handle (index)
    pub fn get_by_handle(&self, handle: *mut core::ffi::c_void) -> Option<&LoadedLibrary> {
        let index = handle as usize;
        if index < MAX_LOADED_LIBS && !self.libraries[index].is_free() {
            Some(&self.libraries[index])
        } else {
            None
        }
    }

    /// Get a mutable library by handle (index)
    pub fn get_by_handle_mut(
        &mut self,
        handle: *mut core::ffi::c_void,
    ) -> Option<&mut LoadedLibrary> {
        let index = handle as usize;
        if index < MAX_LOADED_LIBS && !self.libraries[index].is_free() {
            Some(&mut self.libraries[index])
        } else {
            None
        }
    }

    /// Register the main executable (index 0)
    pub fn register_main_executable(
        &mut self,
        base_addr: u64,
        phdr_addr: u64,
        phnum: u16,
        entry: u64,
    ) -> Result<usize, DlError> {
        if self.count > 0 {
            // Main already registered
            return Ok(0);
        }

        let lib = &mut self.libraries[0];
        lib.state = LibraryState::Initialized;
        lib.set_path(b"[main]");
        lib.base_addr = base_addr;
        lib.phdr_addr = phdr_addr;
        lib.phnum = phnum;
        lib.entry = entry;
        lib.refcount = 1;
        lib.index = 0;
        lib.init_called = true;

        self.count = 1;
        Ok(0)
    }
}

// ============================================================================
// Global Library Manager Instance
// ============================================================================

/// Global library manager instance
static mut LIBRARY_MANAGER: LibraryManager = LibraryManager::new();

/// Get a reference to the global library manager
///
/// # Safety
/// The caller must ensure proper synchronization when accessing the manager.
pub unsafe fn get_library_manager() -> &'static LibraryManager {
    &LIBRARY_MANAGER
}

/// Get a mutable reference to the global library manager
///
/// # Safety
/// The caller must ensure proper synchronization when accessing the manager.
pub unsafe fn get_library_manager_mut() -> &'static mut LibraryManager {
    &mut LIBRARY_MANAGER
}

// ============================================================================
// Link Map Structure (for debugger support)
// ============================================================================

/// Link map entry for debugger support (GDB)
#[repr(C)]
pub struct LinkMap {
    /// Base address where object is mapped
    pub l_addr: u64,
    /// Absolute pathname of object
    pub l_name: *const i8,
    /// Dynamic section of object
    pub l_ld: *const Elf64Dyn,
    /// Chain of loaded objects
    pub l_next: *mut LinkMap,
    /// Previous in chain
    pub l_prev: *mut LinkMap,
}

/// Debug state for rendezvous with debugger
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtState {
    /// Mapping change is complete
    RtConsistent = 0,
    /// Beginning to add a new object
    RtAdd = 1,
    /// Beginning to remove an object
    RtDelete = 2,
}

/// Rendezvous structure for debugger
#[repr(C)]
pub struct RDebug {
    /// Version number for this protocol
    pub r_version: c_int,
    /// Head of loaded objects list
    pub r_map: *mut LinkMap,
    /// Debugger's notification function
    pub r_brk: unsafe extern "C" fn(),
    /// State of dynamic linker
    pub r_state: RtState,
    /// Base address of dynamic linker
    pub r_ldbase: u64,
}

/// Empty breakpoint function for debugger
#[no_mangle]
pub unsafe extern "C" fn _dl_debug_state() {}

/// Global debug rendezvous structure
#[no_mangle]
pub static mut _r_debug: RDebug = RDebug {
    r_version: 1,
    r_map: ptr::null_mut(),
    r_brk: _dl_debug_state,
    r_state: RtState::RtConsistent,
    r_ldbase: 0,
};

// ============================================================================
// Dl_info Structure
// ============================================================================

/// Information returned by dladdr
#[repr(C)]
pub struct DlInfo {
    /// Pathname of shared object containing address
    pub dli_fname: *const i8,
    /// Base address of shared object
    pub dli_fbase: *mut core::ffi::c_void,
    /// Name of nearest symbol
    pub dli_sname: *const i8,
    /// Address of nearest symbol
    pub dli_saddr: *mut core::ffi::c_void,
}

impl DlInfo {
    pub const fn new() -> Self {
        Self {
            dli_fname: ptr::null(),
            dli_fbase: ptr::null_mut(),
            dli_sname: ptr::null(),
            dli_saddr: ptr::null_mut(),
        }
    }
}

// ============================================================================
// Initialization
// ============================================================================

/// Initialize the dynamic linker runtime
///
/// This should be called early during program startup to set up the
/// library manager with information about the main executable.
///
/// # Safety
/// Must be called before any other dynamic linker functions.
pub unsafe fn rtld_init(base_addr: u64, phdr_addr: u64, phnum: u16, entry: u64) {
    let mgr = get_library_manager_mut();
    let _ = mgr.register_main_executable(base_addr, phdr_addr, phnum, entry);
}

/// Check if the dynamic linker has been initialized
pub fn rtld_is_initialized() -> bool {
    unsafe { get_library_manager().count > 0 }
}
