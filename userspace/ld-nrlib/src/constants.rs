//! Constants for the NexaOS dynamic linker

// ============================================================================
// Auxiliary Vector Types
// ============================================================================

pub const AT_NULL: u64 = 0;
pub const AT_IGNORE: u64 = 1;
pub const AT_EXECFD: u64 = 2;
pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_BASE: u64 = 7;
pub const AT_FLAGS: u64 = 8;
pub const AT_ENTRY: u64 = 9;
pub const AT_NOTELF: u64 = 10;
pub const AT_UID: u64 = 11;
pub const AT_EUID: u64 = 12;
pub const AT_GID: u64 = 13;
pub const AT_EGID: u64 = 14;
pub const AT_PLATFORM: u64 = 15;
pub const AT_HWCAP: u64 = 16;
pub const AT_CLKTCK: u64 = 17;
pub const AT_SECURE: u64 = 23;
pub const AT_BASE_PLATFORM: u64 = 24;
pub const AT_RANDOM: u64 = 25;
pub const AT_HWCAP2: u64 = 26;
pub const AT_EXECFN: u64 = 31;
pub const AT_SYSINFO_EHDR: u64 = 33;

// ============================================================================
// ELF Program Header Types
// ============================================================================

pub const PT_NULL: u32 = 0;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_SHLIB: u32 = 5;
pub const PT_PHDR: u32 = 6;
pub const PT_TLS: u32 = 7;

// ============================================================================
// ELF Dynamic Section Tags
// ============================================================================

pub const DT_NULL: i64 = 0;
pub const DT_NEEDED: i64 = 1;
pub const DT_PLTRELSZ: i64 = 2;
pub const DT_PLTGOT: i64 = 3;
pub const DT_HASH: i64 = 4;
pub const DT_STRTAB: i64 = 5;
pub const DT_SYMTAB: i64 = 6;
pub const DT_RELA: i64 = 7;
pub const DT_RELASZ: i64 = 8;
pub const DT_RELAENT: i64 = 9;
pub const DT_STRSZ: i64 = 10;
pub const DT_SYMENT: i64 = 11;
pub const DT_INIT: i64 = 12;
pub const DT_FINI: i64 = 13;
pub const DT_SONAME: i64 = 14;
pub const DT_RPATH: i64 = 15;
pub const DT_SYMBOLIC: i64 = 16;
pub const DT_REL: i64 = 17;
pub const DT_RELSZ: i64 = 18;
pub const DT_RELENT: i64 = 19;
pub const DT_PLTREL: i64 = 20;
pub const DT_DEBUG: i64 = 21;
pub const DT_TEXTREL: i64 = 22;
pub const DT_JMPREL: i64 = 23;
pub const DT_BIND_NOW: i64 = 24;
pub const DT_INIT_ARRAY: i64 = 25;
pub const DT_FINI_ARRAY: i64 = 26;
pub const DT_INIT_ARRAYSZ: i64 = 27;
pub const DT_FINI_ARRAYSZ: i64 = 28;
pub const DT_RUNPATH: i64 = 29;
pub const DT_FLAGS: i64 = 30;
pub const DT_FLAGS_1: i64 = 0x6ffffffb;
pub const DT_PREINIT_ARRAY: i64 = 32;
pub const DT_PREINIT_ARRAYSZ: i64 = 33;
pub const DT_GNU_HASH: i64 = 0x6ffffef5;
pub const DT_VERSYM: i64 = 0x6ffffff0;
pub const DT_RELACOUNT: i64 = 0x6ffffff9;
pub const DT_RELCOUNT: i64 = 0x6ffffffa;
pub const DT_VERNEED: i64 = 0x6ffffffe;
pub const DT_VERNEEDNUM: i64 = 0x6fffffff;

// ============================================================================
// DT_FLAGS Values
// ============================================================================

#[allow(dead_code)]
pub const DF_ORIGIN: u64 = 0x0001;
#[allow(dead_code)]
pub const DF_SYMBOLIC: u64 = 0x0002;
#[allow(dead_code)]
pub const DF_TEXTREL: u64 = 0x0004;
#[allow(dead_code)]
pub const DF_BIND_NOW: u64 = 0x0008;
#[allow(dead_code)]
pub const DF_STATIC_TLS: u64 = 0x0010;

// ============================================================================
// DT_FLAGS_1 Values
// ============================================================================

#[allow(dead_code)]
pub const DF_1_NOW: u64 = 0x00000001;
#[allow(dead_code)]
pub const DF_1_PIE: u64 = 0x08000000;

// ============================================================================
// Relocation Types (x86_64)
// ============================================================================

pub const R_X86_64_NONE: u32 = 0;
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_PC32: u32 = 2;
#[allow(dead_code)]
pub const R_X86_64_GOT32: u32 = 3;
pub const R_X86_64_PLT32: u32 = 4;
pub const R_X86_64_COPY: u32 = 5;
pub const R_X86_64_GLOB_DAT: u32 = 6;
pub const R_X86_64_JUMP_SLOT: u32 = 7;
pub const R_X86_64_RELATIVE: u32 = 8;
pub const R_X86_64_GOTPCREL: u32 = 9;
pub const R_X86_64_32: u32 = 10;
pub const R_X86_64_32S: u32 = 11;
pub const R_X86_64_DTPMOD64: u32 = 16;
pub const R_X86_64_DTPOFF64: u32 = 17;
pub const R_X86_64_TPOFF64: u32 = 18;
#[allow(dead_code)]
pub const R_X86_64_TLSGD: u32 = 19;
#[allow(dead_code)]
pub const R_X86_64_TLSLD: u32 = 20;
#[allow(dead_code)]
pub const R_X86_64_DTPOFF32: u32 = 21;
#[allow(dead_code)]
pub const R_X86_64_GOTTPOFF: u32 = 22;
pub const R_X86_64_TPOFF32: u32 = 23;
pub const R_X86_64_IRELATIVE: u32 = 37;

// ============================================================================
// System Call Numbers
// ============================================================================

pub const SYS_WRITE: u64 = 1;
pub const SYS_EXIT: u64 = 60;
pub const SYS_MMAP: u64 = 9;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_READ: u64 = 0;
#[allow(dead_code)]
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;
#[allow(dead_code)]
pub const SYS_MUNMAP: u64 = 11;

// ============================================================================
// mmap Constants
// ============================================================================

pub const PROT_READ: u64 = 0x1;
pub const PROT_WRITE: u64 = 0x2;
pub const PROT_EXEC: u64 = 0x4;
pub const MAP_PRIVATE: u64 = 0x02;
#[allow(dead_code)]
pub const MAP_FIXED: u64 = 0x10;
pub const MAP_ANONYMOUS: u64 = 0x20;

// ============================================================================
// Dynamic Linker Limits
// ============================================================================

/// Maximum number of loaded libraries
pub const MAX_LIBS: usize = 16;

/// Maximum TLS modules
pub const MAX_TLS_MODULES: usize = 16;

/// Page size
pub const PAGE_SIZE: u64 = 4096;

// ============================================================================
// Library Search Paths
// ============================================================================

pub const LIB_PATH_1: &[u8; 8] = b"/lib64\0\0";
pub const LIB_PATH_2: &[u8; 6] = b"/lib\0\0";
pub const LIB_PATH_3: &[u8; 12] = b"/usr/lib64\0\0";
pub const LIB_PATH_4: &[u8; 10] = b"/usr/lib\0\0";
