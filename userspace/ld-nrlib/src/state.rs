//! Dynamic section information and global symbol table

use crate::constants::MAX_LIBS;

// ============================================================================
// Dynamic Section Information
// ============================================================================

#[derive(Clone, Copy)]
pub struct DynInfo {
    pub strtab: u64,
    pub symtab: u64,
    pub strsz: u64,
    pub syment: u64,
    pub rela: u64,
    pub relasz: u64,
    pub relaent: u64,
    pub relacount: u64, // DT_RELACOUNT - count of relative relocations
    pub jmprel: u64,
    pub pltrelsz: u64,
    pub pltrel: u64, // DT_PLTREL - type of PLT relocations
    pub init: u64,
    pub fini: u64,
    pub init_array: u64,
    pub init_arraysz: u64,
    pub fini_array: u64,
    pub fini_arraysz: u64,
    pub preinit_array: u64,
    pub preinit_arraysz: u64,
    pub hash: u64,
    pub gnu_hash: u64,
    pub flags: u64,   // DT_FLAGS
    pub flags_1: u64, // DT_FLAGS_1
    // Version information
    pub versym: u64,     // DT_VERSYM
    pub verneed: u64,    // DT_VERNEED
    pub verneednum: u64, // DT_VERNEEDNUM
    // TLS information
    pub tls_modid: u64,    // TLS module ID (assigned at runtime)
    pub needed: [u64; 16], // DT_NEEDED offsets into strtab
    pub needed_count: usize,
}

impl DynInfo {
    pub const fn new() -> Self {
        Self {
            strtab: 0,
            symtab: 0,
            strsz: 0,
            syment: 24, // sizeof(Elf64_Sym)
            rela: 0,
            relasz: 0,
            relaent: 0,
            relacount: 0,
            jmprel: 0,
            pltrelsz: 0,
            pltrel: 0,
            init: 0,
            fini: 0,
            init_array: 0,
            init_arraysz: 0,
            fini_array: 0,
            fini_arraysz: 0,
            preinit_array: 0,
            preinit_arraysz: 0,
            hash: 0,
            gnu_hash: 0,
            flags: 0,
            flags_1: 0,
            versym: 0,
            verneed: 0,
            verneednum: 0,
            tls_modid: 0,
            needed: [0; 16],
            needed_count: 0,
        }
    }
}

// ============================================================================
// Loaded Library Information
// ============================================================================

#[derive(Clone, Copy)]
pub struct LoadedLib {
    /// Base address where library is loaded
    pub base_addr: u64,
    /// Load bias (runtime - link time address)
    pub load_bias: i64,
    /// Dynamic section info
    pub dyn_info: DynInfo,
    /// Is this entry valid
    pub valid: bool,
}

impl LoadedLib {
    pub const fn new() -> Self {
        Self {
            base_addr: 0,
            load_bias: 0,
            dyn_info: DynInfo::new(),
            valid: false,
        }
    }
}

// ============================================================================
// Global Symbol Table
// ============================================================================

pub struct GlobalSymbolTable {
    /// Loaded libraries (index 0 = main executable, 1+ = shared libs)
    pub libs: [LoadedLib; MAX_LIBS],
    /// Number of loaded libraries
    pub lib_count: usize,
}

impl GlobalSymbolTable {
    pub const fn new() -> Self {
        Self {
            libs: [LoadedLib::new(); MAX_LIBS],
            lib_count: 0,
        }
    }
}

/// Global symbol table instance
pub static mut GLOBAL_SYMTAB: GlobalSymbolTable = GlobalSymbolTable::new();
