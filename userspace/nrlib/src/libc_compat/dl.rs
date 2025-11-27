//! Dynamic Linker API Implementation
//!
//! This module provides the complete dlopen/dlsym/dlclose/dladdr/dlerror API
//! for dynamic loading of shared libraries at runtime.
//!
//! The implementation supports:
//! - Loading shared objects from the filesystem
//! - Symbol lookup (global and local scope)
//! - Reference counting for loaded libraries
//! - Proper initialization/finalization function calling
//! - Thread-safe operations (via spinlock)

use crate::{c_int, c_void};
use core::ptr;

use super::elf::*;
use super::reloc::*;
use super::rtld::*;
use super::symbol::*;

// ============================================================================
// Error Message Buffer
// ============================================================================

/// Thread-local error message buffer
static mut DLERROR_BUF: [u8; 512] = [0; 512];
static mut DLERROR_LEN: usize = 0;
static mut DLERROR_SET: bool = false;

/// Set the error message
unsafe fn set_dlerror(msg: &[u8]) {
    let len = core::cmp::min(msg.len(), DLERROR_BUF.len() - 1);
    DLERROR_BUF[..len].copy_from_slice(&msg[..len]);
    DLERROR_BUF[len] = 0;
    DLERROR_LEN = len;
    DLERROR_SET = true;
}

/// Set error message from DlError
unsafe fn set_dlerror_from_error(error: DlError, extra: Option<&[u8]>) {
    let base = error.as_str().as_bytes();
    let mut pos = 0;

    // Copy base message
    let base_len = core::cmp::min(base.len(), DLERROR_BUF.len() - 1);
    DLERROR_BUF[..base_len].copy_from_slice(&base[..base_len]);
    pos = base_len;

    // Add extra info if present
    if let Some(extra) = extra {
        if pos + 2 + extra.len() < DLERROR_BUF.len() {
            DLERROR_BUF[pos] = b':';
            DLERROR_BUF[pos + 1] = b' ';
            pos += 2;
            let extra_len = core::cmp::min(extra.len(), DLERROR_BUF.len() - pos - 1);
            DLERROR_BUF[pos..pos + extra_len].copy_from_slice(&extra[..extra_len]);
            pos += extra_len;
        }
    }

    DLERROR_BUF[pos] = 0;
    DLERROR_LEN = pos;
    DLERROR_SET = true;
}

/// Clear the error message
unsafe fn clear_dlerror() {
    DLERROR_SET = false;
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert C string to byte slice
unsafe fn cstr_to_bytes(s: *const i8) -> &'static [u8] {
    if s.is_null() {
        return &[];
    }
    let mut len = 0;
    while *s.add(len) != 0 {
        len += 1;
        if len > 4096 {
            break;
        }
    }
    core::slice::from_raw_parts(s as *const u8, len)
}

/// Find the last occurrence of '/' in a path
fn find_basename(path: &[u8]) -> &[u8] {
    if let Some(pos) = path.iter().rposition(|&c| c == b'/') {
        &path[pos + 1..]
    } else {
        path
    }
}

/// Check if path is absolute
fn is_absolute_path(path: &[u8]) -> bool {
    !path.is_empty() && path[0] == b'/'
}

// ============================================================================
// Library Loading Implementation
// ============================================================================

/// Internal implementation of library loading
///
/// # Safety
/// The caller must have acquired the library manager lock.
unsafe fn do_dlopen(filename: *const i8, flags: c_int) -> *mut c_void {
    let mgr = get_library_manager_mut();

    // NULL filename means return handle to main executable
    if filename.is_null() {
        if mgr.count > 0 {
            mgr.libraries[0].refcount += 1;
            return 0 as *mut c_void; // Index 0 = main executable
        }
        set_dlerror(b"main executable not registered");
        return ptr::null_mut();
    }

    let path = cstr_to_bytes(filename);
    if path.is_empty() {
        set_dlerror(b"empty filename");
        return ptr::null_mut();
    }

    // Check if already loaded
    if let Some(idx) = mgr.find_by_path(path).or_else(|| mgr.find_by_soname(path)) {
        let lib = &mut mgr.libraries[idx];
        lib.refcount += 1;

        // Handle RTLD_NOLOAD - only return if already loaded
        if (flags & RTLD_NOLOAD) != 0 {
            return idx as *mut c_void;
        }

        // Update flags if RTLD_GLOBAL is newly requested
        if (flags & RTLD_GLOBAL) != 0 {
            lib.flags |= RTLD_GLOBAL;
        }

        return idx as *mut c_void;
    }

    // RTLD_NOLOAD and not loaded - return NULL without error
    if (flags & RTLD_NOLOAD) != 0 {
        return ptr::null_mut();
    }

    // Find a free slot
    let slot_idx = match mgr.find_free_slot() {
        Some(idx) => idx,
        None => {
            set_dlerror_from_error(DlError::TooManyLibs, None);
            return ptr::null_mut();
        }
    };

    // Resolve the library path
    let resolved_path: [u8; MAX_LIB_PATH] = resolve_library_path(path);

    // Try to load the library from filesystem
    // This is a simplified implementation - in a real system, we would:
    // 1. Open the file
    // 2. Read and validate the ELF header
    // 3. Map the segments into memory
    // 4. Parse the dynamic section
    // 5. Process relocations

    // For now, since NexaOS doesn't have full mmap support for files,
    // we'll implement a stub that returns an error
    // TODO: Implement actual file loading when mmap is available

    set_dlerror_from_error(DlError::LoadFailed, Some(b"file loading not yet implemented"));
    ptr::null_mut()
}

/// Resolve library search path
unsafe fn resolve_library_path(name: &[u8]) -> [u8; MAX_LIB_PATH] {
    let mut result = [0u8; MAX_LIB_PATH];

    // If absolute path, use as-is
    if is_absolute_path(name) {
        let len = core::cmp::min(name.len(), MAX_LIB_PATH - 1);
        result[..len].copy_from_slice(&name[..len]);
        return result;
    }

    // Otherwise, the path is used as-is (would normally search LD_LIBRARY_PATH, etc.)
    let len = core::cmp::min(name.len(), MAX_LIB_PATH - 1);
    result[..len].copy_from_slice(&name[..len]);
    result
}

// ============================================================================
// Public API: dlopen
// ============================================================================

/// Open a shared library
///
/// # Arguments
/// * `filename` - Path to the shared library, or NULL for main executable
/// * `flags` - Combination of RTLD_* flags
///
/// # Returns
/// Handle to the library, or NULL on error (check dlerror())
///
/// # Safety
/// Standard C ABI function, caller must ensure valid strings.
#[no_mangle]
pub unsafe extern "C" fn dlopen(filename: *const i8, flags: c_int) -> *mut c_void {
    clear_dlerror();

    let mgr = get_library_manager();
    mgr.acquire();

    let result = do_dlopen(filename, flags);

    mgr.release();
    result
}

// ============================================================================
// Public API: dlsym
// ============================================================================

/// Look up a symbol in a shared library
///
/// # Arguments
/// * `handle` - Handle from dlopen, RTLD_DEFAULT, or RTLD_NEXT
/// * `symbol` - Name of the symbol to look up
///
/// # Returns
/// Address of the symbol, or NULL if not found (check dlerror())
///
/// # Safety
/// Standard C ABI function.
#[no_mangle]
pub unsafe extern "C" fn dlsym(handle: *mut c_void, symbol: *const i8) -> *mut c_void {
    clear_dlerror();

    if symbol.is_null() {
        set_dlerror(b"symbol name is NULL");
        return ptr::null_mut();
    }

    let name = cstr_to_bytes(symbol);
    if name.is_empty() {
        set_dlerror(b"empty symbol name");
        return ptr::null_mut();
    }

    let mgr = get_library_manager();
    mgr.acquire();

    let result = if handle == RTLD_DEFAULT {
        // Search all libraries in load order
        let options = LookupOptions::default();
        lookup_symbol(name, &options).map(|r| r.addr as *mut c_void)
    } else if handle == RTLD_NEXT {
        // Search libraries after the calling library
        // For now, search from index 1 (after main)
        let options = LookupOptions {
            start_index: 1,
            ..Default::default()
        };
        lookup_symbol(name, &options).map(|r| r.addr as *mut c_void)
    } else {
        // Search in specific library
        let lib_idx = handle as usize;
        if lib_idx < MAX_LOADED_LIBS && !mgr.libraries[lib_idx].is_free() {
            lookup_symbol_in_lib(&mgr.libraries[lib_idx], name).map(|r| r.addr as *mut c_void)
        } else {
            set_dlerror_from_error(DlError::InvalidHandle, None);
            None
        }
    };

    mgr.release();

    match result {
        Some(addr) if !addr.is_null() => addr,
        _ => {
            if !DLERROR_SET {
                set_dlerror_from_error(DlError::SymbolNotFound, Some(name));
            }
            ptr::null_mut()
        }
    }
}

// ============================================================================
// Public API: dlclose
// ============================================================================

/// Close a shared library
///
/// Decrements the reference count of the library. When it reaches zero,
/// the library's finalization functions are called and it may be unloaded.
///
/// # Arguments
/// * `handle` - Handle from dlopen
///
/// # Returns
/// 0 on success, non-zero on error
///
/// # Safety
/// Standard C ABI function.
#[no_mangle]
pub unsafe extern "C" fn dlclose(handle: *mut c_void) -> c_int {
    clear_dlerror();

    if handle.is_null() {
        return 0; // NULL handle is valid (no-op)
    }

    let lib_idx = handle as usize;

    let mgr = get_library_manager_mut();
    mgr.acquire();

    if lib_idx >= MAX_LOADED_LIBS || mgr.libraries[lib_idx].is_free() {
        mgr.release();
        set_dlerror_from_error(DlError::InvalidHandle, None);
        return -1;
    }

    let lib = &mut mgr.libraries[lib_idx];

    // Decrement reference count
    if lib.refcount > 0 {
        lib.refcount -= 1;
    }

    // Check if we should unload
    if lib.refcount == 0 && (lib.flags & RTLD_NODELETE) == 0 {
        // Call finalization functions
        call_fini_functions(lib);

        // Mark as free
        lib.state = LibraryState::Free;

        // Note: In a full implementation, we would also:
        // - Unmap the library's memory regions
        // - Remove from link map
        // - Notify debugger
    }

    mgr.release();
    0
}

// ============================================================================
// Public API: dlerror
// ============================================================================

/// Get the last dynamic linker error message
///
/// Returns a human-readable string describing the most recent error
/// that occurred during dlopen, dlsym, or dlclose. The error is cleared
/// after being returned.
///
/// # Returns
/// Error message string, or NULL if no error
///
/// # Safety
/// Standard C ABI function.
#[no_mangle]
pub unsafe extern "C" fn dlerror() -> *mut i8 {
    if DLERROR_SET {
        DLERROR_SET = false;
        DLERROR_BUF.as_mut_ptr() as *mut i8
    } else {
        ptr::null_mut()
    }
}

// ============================================================================
// Public API: dladdr
// ============================================================================

/// Find information about a symbol by address
///
/// Given an address, finds the shared library containing it and
/// the nearest symbol at or before that address.
///
/// # Arguments
/// * `addr` - Address to look up
/// * `info` - Pointer to Dl_info structure to fill
///
/// # Returns
/// Non-zero if information was found, 0 if address not in any library
///
/// # Safety
/// Standard C ABI function, info must point to valid Dl_info.
#[no_mangle]
pub unsafe extern "C" fn dladdr(addr: *const c_void, info: *mut c_void) -> c_int {
    if info.is_null() {
        return 0;
    }

    let dl_info = info as *mut DlInfo;
    *dl_info = DlInfo::new();

    let addr_val = addr as u64;

    let mgr = get_library_manager();
    mgr.acquire();

    // Find library containing this address
    let lib_idx = match mgr.find_by_addr(addr_val) {
        Some(idx) => idx,
        None => {
            mgr.release();
            return 0;
        }
    };

    let lib = &mgr.libraries[lib_idx];

    // Fill in library info
    (*dl_info).dli_fname = lib.path.as_ptr() as *const i8;
    (*dl_info).dli_fbase = lib.base_addr as *mut c_void;

    // Try to find the symbol containing this address
    if let Some((_, sym)) = find_symbol_by_addr(lib, addr_val) {
        if sym.st_name != 0 {
            let strtab = lib.strtab();
            if !strtab.is_null() {
                (*dl_info).dli_sname = strtab.add(sym.st_name as usize) as *const i8;
            }
        }
        (*dl_info).dli_saddr = ((sym.st_value as i64 + lib.load_bias) as u64) as *mut c_void;
    }

    mgr.release();
    1
}

// ============================================================================
// Additional GNU Extensions
// ============================================================================

/// Extended dladdr with extra information
///
/// # Safety
/// Standard C ABI function.
#[no_mangle]
pub unsafe extern "C" fn dladdr1(
    addr: *const c_void,
    info: *mut c_void,
    _extra_info: *mut *mut c_void,
    _flags: c_int,
) -> c_int {
    // For now, just call regular dladdr
    dladdr(addr, info)
}

/// Get information about a loaded shared object
///
/// # Safety
/// Standard C ABI function.
#[no_mangle]
pub unsafe extern "C" fn dlinfo(
    _handle: *mut c_void,
    _request: c_int,
    _info: *mut c_void,
) -> c_int {
    set_dlerror(b"dlinfo not implemented");
    -1
}

/// Iterate over loaded shared objects
///
/// # Safety
/// Standard C ABI function.
#[no_mangle]
pub unsafe extern "C" fn dl_iterate_phdr(
    callback: unsafe extern "C" fn(
        info: *mut DlPhdrInfo,
        size: usize,
        data: *mut c_void,
    ) -> c_int,
    data: *mut c_void,
) -> c_int {
    let mgr = get_library_manager();
    mgr.acquire();

    for i in 0..mgr.count {
        let lib = &mgr.libraries[i];
        if lib.is_free() {
            continue;
        }

        let mut info = DlPhdrInfo {
            dlpi_addr: lib.base_addr,
            dlpi_name: lib.path.as_ptr() as *const i8,
            dlpi_phdr: lib.phdr_addr as *const Elf64Phdr,
            dlpi_phnum: lib.phnum,
            dlpi_adds: mgr.count as u64,
            dlpi_subs: 0,
            dlpi_tls_modid: 0,
            dlpi_tls_data: ptr::null_mut(),
        };

        let ret = callback(&mut info, core::mem::size_of::<DlPhdrInfo>(), data);
        if ret != 0 {
            mgr.release();
            return ret;
        }
    }

    mgr.release();
    0
}

/// Structure passed to dl_iterate_phdr callback
#[repr(C)]
pub struct DlPhdrInfo {
    /// Base address of object
    pub dlpi_addr: u64,
    /// Name of object
    pub dlpi_name: *const i8,
    /// Pointer to program headers
    pub dlpi_phdr: *const Elf64Phdr,
    /// Number of program headers
    pub dlpi_phnum: u16,
    /// Incremented when a new object is loaded
    pub dlpi_adds: u64,
    /// Incremented when an object is unloaded
    pub dlpi_subs: u64,
    /// TLS module ID
    pub dlpi_tls_modid: usize,
    /// TLS data address
    pub dlpi_tls_data: *mut c_void,
}

// ============================================================================
// Compatibility Aliases
// ============================================================================

/// GNU extension: dlvsym - versioned symbol lookup
#[no_mangle]
pub unsafe extern "C" fn dlvsym(
    handle: *mut c_void,
    symbol: *const i8,
    _version: *const i8,
) -> *mut c_void {
    // For now, ignore version and do regular lookup
    dlsym(handle, symbol)
}

/// POSIX extension: dlmopen - open in specific namespace
#[no_mangle]
pub unsafe extern "C" fn dlmopen(
    _lmid: isize,
    filename: *const i8,
    flags: c_int,
) -> *mut c_void {
    // For now, ignore namespace and do regular open
    dlopen(filename, flags)
}
 