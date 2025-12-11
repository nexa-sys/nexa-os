//! TLS (Thread-Local Storage) support for the NexaOS dynamic linker

use core::arch::asm;

use crate::constants::MAX_TLS_MODULES;

// ============================================================================
// TLS Module Information
// ============================================================================

#[derive(Clone, Copy)]
pub struct TlsModule {
    /// Module ID (1-based, 0 means unused)
    pub id: u64,
    /// TLS image address in memory
    pub image: u64,
    /// TLS image size (file size)
    pub filesz: u64,
    /// TLS memory size (including BSS)
    pub memsz: u64,
    /// TLS alignment
    pub align: u64,
    /// Offset in static TLS block
    pub offset: i64,
}

impl TlsModule {
    pub const fn new() -> Self {
        Self {
            id: 0,
            image: 0,
            filesz: 0,
            memsz: 0,
            align: 1,
            offset: 0,
        }
    }
}

// ============================================================================
// Global TLS State
// ============================================================================

pub struct TlsState {
    /// TLS modules
    pub modules: [TlsModule; MAX_TLS_MODULES],
    /// Number of TLS modules
    pub count: usize,
    /// Total static TLS size
    pub total_size: usize,
    /// TLS module ID counter
    pub next_id: u64,
}

impl TlsState {
    pub const fn new() -> Self {
        Self {
            modules: [TlsModule::new(); MAX_TLS_MODULES],
            count: 0,
            total_size: 0,
            next_id: 1,
        }
    }
}

pub static mut TLS_STATE: TlsState = TlsState::new();

// ============================================================================
// TLS Functions
// ============================================================================

/// Register a TLS module (called during library loading)
pub unsafe fn register_tls_module(image: u64, filesz: u64, memsz: u64, align: u64) -> u64 {
    if TLS_STATE.count >= MAX_TLS_MODULES {
        return 0;
    }

    let module_id = TLS_STATE.next_id;
    TLS_STATE.next_id += 1;

    let idx = TLS_STATE.count;
    TLS_STATE.modules[idx] = TlsModule {
        id: module_id,
        image,
        filesz,
        memsz,
        align,
        offset: 0, // Will be calculated during TLS setup
    };
    TLS_STATE.count += 1;

    module_id
}

// ============================================================================
// TLS Access Function (musl/glibc compatible)
// ============================================================================

/// TLS index structure for __tls_get_addr
#[repr(C)]
pub struct TlsIndex {
    pub ti_module: u64, // Module ID
    pub ti_offset: u64, // Offset within module
}

/// __tls_get_addr - TLS access function (musl/glibc compatible)
/// Used for accessing thread-local variables in dynamically loaded libraries
#[no_mangle]
pub unsafe extern "C" fn __tls_get_addr(ti: *const TlsIndex) -> *mut u8 {
    if ti.is_null() {
        return core::ptr::null_mut();
    }

    let module_id = (*ti).ti_module;
    let offset = (*ti).ti_offset;

    // Get thread pointer (FS base on x86_64)
    let tp: u64;
    asm!("mov {}, fs:0", out(reg) tp, options(nostack, preserves_flags, readonly));

    // For static TLS (variant II, x86_64), TLS data is below TP
    // Find the module and calculate address
    for i in 0..TLS_STATE.count {
        if TLS_STATE.modules[i].id == module_id {
            let tls_offset = TLS_STATE.modules[i].offset;
            return (tp as i64 + tls_offset + offset as i64) as *mut u8;
        }
    }

    // Module not found
    core::ptr::null_mut()
}
