//! Auxiliary vector handling and global storage

use crate::constants::*;

// ============================================================================
// Auxiliary Vector Information
// ============================================================================

#[derive(Clone, Copy)]
pub struct AuxInfo {
    pub at_phdr: u64,         // Program headers address
    pub at_phent: u64,        // Program header entry size
    pub at_phnum: u64,        // Number of program headers
    pub at_pagesz: u64,       // Page size
    pub at_base: u64,         // Interpreter base address
    pub at_entry: u64,        // Main executable entry point
    pub at_execfn: u64,       // Executable filename
    pub at_random: u64,       // Address of random bytes
    pub at_secure: u64,       // Secure mode flag
    pub at_uid: u64,          // Real UID
    pub at_euid: u64,         // Effective UID
    pub at_gid: u64,          // Real GID
    pub at_egid: u64,         // Effective GID
    pub at_hwcap: u64,        // Hardware capabilities
    pub at_hwcap2: u64,       // Hardware capabilities 2
    pub at_clktck: u64,       // Clock ticks per second
    pub at_sysinfo_ehdr: u64, // vDSO address
}

impl AuxInfo {
    pub const fn new() -> Self {
        Self {
            at_phdr: 0,
            at_phent: 0,
            at_phnum: 0,
            at_pagesz: 4096,
            at_base: 0,
            at_entry: 0,
            at_execfn: 0,
            at_random: 0,
            at_secure: 0,
            at_uid: 0,
            at_euid: 0,
            at_gid: 0,
            at_egid: 0,
            at_hwcap: 0,
            at_hwcap2: 0,
            at_clktck: 100,
            at_sysinfo_ehdr: 0,
        }
    }
}

// ============================================================================
// Global Auxiliary Vector Storage (for getauxval support)
// ============================================================================

static mut AUXV_STORAGE: AuxInfo = AuxInfo::new();
static mut AUXV_INITIALIZED: bool = false;

/// Store auxiliary vector globally
pub unsafe fn store_auxv(aux_info: &AuxInfo) {
    AUXV_STORAGE = *aux_info;
    AUXV_INITIALIZED = true;
}

/// getauxval - get auxiliary vector value (musl-compatible)
/// This is exported for applications that need auxv access
#[no_mangle]
pub unsafe extern "C" fn getauxval(type_: u64) -> u64 {
    if !AUXV_INITIALIZED {
        return 0;
    }

    match type_ {
        AT_PHDR => AUXV_STORAGE.at_phdr,
        AT_PHENT => AUXV_STORAGE.at_phent,
        AT_PHNUM => AUXV_STORAGE.at_phnum,
        AT_PAGESZ => AUXV_STORAGE.at_pagesz,
        AT_BASE => AUXV_STORAGE.at_base,
        AT_ENTRY => AUXV_STORAGE.at_entry,
        AT_EXECFN => AUXV_STORAGE.at_execfn,
        AT_RANDOM => AUXV_STORAGE.at_random,
        AT_SECURE => AUXV_STORAGE.at_secure,
        AT_UID => AUXV_STORAGE.at_uid,
        AT_EUID => AUXV_STORAGE.at_euid,
        AT_GID => AUXV_STORAGE.at_gid,
        AT_EGID => AUXV_STORAGE.at_egid,
        AT_HWCAP => AUXV_STORAGE.at_hwcap,
        AT_HWCAP2 => AUXV_STORAGE.at_hwcap2,
        AT_CLKTCK => AUXV_STORAGE.at_clktck,
        AT_SYSINFO_EHDR => AUXV_STORAGE.at_sysinfo_ehdr,
        _ => 0,
    }
}

/// __getauxval - alias for getauxval (glibc compatibility)
#[no_mangle]
pub unsafe extern "C" fn __getauxval(type_: u64) -> u64 {
    getauxval(type_)
}
