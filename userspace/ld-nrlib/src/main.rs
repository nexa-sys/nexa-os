//! NexaOS Dynamic Linker / Loader (ld-nrlib)
//!
//! This is the standalone dynamic linker for NexaOS, similar to:
//! - ld-linux.so on glibc systems
//! - ld-musl-x86_64.so on musl systems
//!
//! When the kernel loads a dynamically linked executable, it reads the
//! PT_INTERP segment which points to this interpreter. The kernel then
//! loads this interpreter and transfers control to its entry point.
//!
//! This interpreter then:
//! 1. Parses the auxiliary vector to find the main executable
//! 2. Loads all required shared libraries (DT_NEEDED)
//! 3. Performs relocations
//! 4. Calls initialization functions
//! 5. Transfers control to the main executable's entry point

#![no_std]
#![no_main]

use core::arch::asm;
use core::arch::naked_asm;

// ============================================================================
// Constants
// ============================================================================

/// Auxiliary vector types
const AT_NULL: u64 = 0;
const AT_IGNORE: u64 = 1;
const AT_EXECFD: u64 = 2;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_BASE: u64 = 7;
const AT_FLAGS: u64 = 8;
const AT_ENTRY: u64 = 9;
const AT_NOTELF: u64 = 10;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_PLATFORM: u64 = 15;
const AT_HWCAP: u64 = 16;
const AT_CLKTCK: u64 = 17;
const AT_SECURE: u64 = 23;
const AT_BASE_PLATFORM: u64 = 24;
const AT_RANDOM: u64 = 25;
const AT_HWCAP2: u64 = 26;
const AT_EXECFN: u64 = 31;
const AT_SYSINFO_EHDR: u64 = 33;

/// ELF Constants
const PT_NULL: u32 = 0;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;
const PT_SHLIB: u32 = 5;
const PT_PHDR: u32 = 6;
const PT_TLS: u32 = 7;

const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;
const DT_PLTRELSZ: i64 = 2;
const DT_PLTGOT: i64 = 3;
const DT_HASH: i64 = 4;
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_RELAENT: i64 = 9;
const DT_STRSZ: i64 = 10;
const DT_SYMENT: i64 = 11;
const DT_INIT: i64 = 12;
const DT_FINI: i64 = 13;
const DT_SONAME: i64 = 14;
const DT_RPATH: i64 = 15;
const DT_SYMBOLIC: i64 = 16;
const DT_REL: i64 = 17;
const DT_RELSZ: i64 = 18;
const DT_RELENT: i64 = 19;
const DT_PLTREL: i64 = 20;
const DT_DEBUG: i64 = 21;
const DT_TEXTREL: i64 = 22;
const DT_JMPREL: i64 = 23;
const DT_BIND_NOW: i64 = 24;
const DT_INIT_ARRAY: i64 = 25;
const DT_FINI_ARRAY: i64 = 26;
const DT_INIT_ARRAYSZ: i64 = 27;
const DT_FINI_ARRAYSZ: i64 = 28;
const DT_RUNPATH: i64 = 29;
const DT_FLAGS: i64 = 30;
const DT_FLAGS_1: i64 = 0x6ffffffb;
const DT_PREINIT_ARRAY: i64 = 32;
const DT_PREINIT_ARRAYSZ: i64 = 33;
const DT_GNU_HASH: i64 = 0x6ffffef5;
const DT_VERSYM: i64 = 0x6ffffff0;
const DT_RELACOUNT: i64 = 0x6ffffff9;
const DT_RELCOUNT: i64 = 0x6ffffffa;
const DT_VERNEED: i64 = 0x6ffffffe;
const DT_VERNEEDNUM: i64 = 0x6fffffff;

/// DT_FLAGS values
const DF_ORIGIN: u64 = 0x0001;
const DF_SYMBOLIC: u64 = 0x0002;
const DF_TEXTREL: u64 = 0x0004;
const DF_BIND_NOW: u64 = 0x0008;
const DF_STATIC_TLS: u64 = 0x0010;

/// DT_FLAGS_1 values
const DF_1_NOW: u64 = 0x00000001;
const DF_1_PIE: u64 = 0x08000000;

/// Relocation types
const R_X86_64_NONE: u32 = 0;
const R_X86_64_64: u32 = 1;
const R_X86_64_PC32: u32 = 2;
const R_X86_64_GOT32: u32 = 3;
const R_X86_64_PLT32: u32 = 4;
const R_X86_64_COPY: u32 = 5;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;
const R_X86_64_GOTPCREL: u32 = 9;
const R_X86_64_32: u32 = 10;
const R_X86_64_32S: u32 = 11;
const R_X86_64_DTPMOD64: u32 = 16;
const R_X86_64_DTPOFF64: u32 = 17;
const R_X86_64_TPOFF64: u32 = 18;
const R_X86_64_TLSGD: u32 = 19;
const R_X86_64_TLSLD: u32 = 20;
const R_X86_64_DTPOFF32: u32 = 21;
const R_X86_64_GOTTPOFF: u32 = 22;
const R_X86_64_TPOFF32: u32 = 23;
const R_X86_64_IRELATIVE: u32 = 37;

// System call numbers
const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;
const SYS_MMAP: u64 = 9;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_READ: u64 = 0;
const SYS_FSTAT: u64 = 5;
const SYS_LSEEK: u64 = 8;
const SYS_MUNMAP: u64 = 11;

// mmap constants
const PROT_READ: u64 = 0x1;
const PROT_WRITE: u64 = 0x2;
const PROT_EXEC: u64 = 0x4;
const MAP_PRIVATE: u64 = 0x02;
const MAP_FIXED: u64 = 0x10;
const MAP_ANONYMOUS: u64 = 0x20;

// ============================================================================
// ELF Structures
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Dyn {
    d_tag: i64,
    d_val: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Sym {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Rela {
    r_offset: u64,
    r_info: u64,
    r_addend: i64,
}

// ============================================================================
// Auxiliary Vector Entry
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
struct AuxEntry {
    a_type: u64,
    a_val: u64,
}

// ============================================================================
// System Call Wrappers
// ============================================================================

#[inline]
unsafe fn syscall1(nr: u64, a1: u64) -> u64 {
    let ret: u64;
    asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline]
unsafe fn syscall6(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> u64 {
    let ret: u64;
    asm!(
        "syscall",
        in("rax") nr,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        in("r9") a6,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

unsafe fn write(fd: i32, buf: *const u8, len: usize) -> isize {
    syscall3(SYS_WRITE, fd as u64, buf as u64, len as u64) as isize
}

unsafe fn exit(code: i32) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {
        asm!("ud2", options(noreturn));
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

unsafe fn print(msg: &[u8]) {
    write(2, msg.as_ptr(), msg.len());
}

unsafe fn print_str(s: &str) {
    print(s.as_bytes());
}

unsafe fn print_hex(val: u64) {
    let mut buf = [0u8; 18]; // "0x" + 16 hex digits
    buf[0] = b'0';
    buf[1] = b'x';
    let hex_chars = b"0123456789abcdef";
    for i in 0..16 {
        let nibble = ((val >> (60 - i * 4)) & 0xf) as usize;
        buf[2 + i] = hex_chars[nibble];
    }
    print(&buf);
}

/// Copy memory
unsafe fn memcpy(dest: *mut u8, src: *const u8, n: usize) {
    for i in 0..n {
        *dest.add(i) = *src.add(i);
    }
}

/// Zero memory
unsafe fn memset(dest: *mut u8, val: u8, n: usize) {
    for i in 0..n {
        *dest.add(i) = val;
    }
}

/// Get length of C string (not including null terminator)
unsafe fn cstr_len(s: *const u8) -> usize {
    let mut len = 0;
    while *s.add(len) != 0 {
        len += 1;
        if len > 256 {
            break;
        }
    }
    len
}

/// Compare two library names (handles libc.so -> libnrlib.so mapping)
unsafe fn is_same_library_name(a: *const u8, b: *const u8) -> bool {
    let mut i = 0;
    loop {
        let ca = *a.add(i);
        let cb = *b.add(i);
        if ca == 0 && cb == 0 {
            return true;
        }
        if ca != cb {
            return false;
        }
        i += 1;
        if i > 256 {
            return false;
        }
    }
}

/// Check if name starts with prefix
unsafe fn starts_with(name: *const u8, prefix: &[u8]) -> bool {
    for (i, &c) in prefix.iter().enumerate() {
        if c == 0 {
            return true;
        }
        if *name.add(i) != c {
            return false;
        }
    }
    true
}

/// Check if library name is a libc variant that should map to libnrlib
/// Supports musl, glibc, and other libc variants
unsafe fn is_libc_library(name: *const u8) -> bool {
    // Check for libnrlib.so (already loaded)
    if is_same_library_name(name, b"libnrlib.so\0".as_ptr()) {
        return true;
    }
    
    // Check for glibc variants
    if is_same_library_name(name, b"libc.so\0".as_ptr()) ||
       is_same_library_name(name, b"libc.so.6\0".as_ptr()) ||
       starts_with(name, b"libc.so.") {
        return true;
    }
    
    // Check for musl variants (ld-musl-*.so.1)
    if starts_with(name, b"ld-musl-") {
        return true;
    }
    if is_same_library_name(name, b"ld-musl-x86_64.so.1\0".as_ptr()) {
        return true;
    }
    
    // Check for libpthread (merged into libc in musl)
    if is_same_library_name(name, b"libpthread.so.0\0".as_ptr()) ||
       is_same_library_name(name, b"libpthread.so\0".as_ptr()) ||
       starts_with(name, b"libpthread.so.") {
        return true;
    }
    
    // Check for libdl (merged into libc in musl)
    if is_same_library_name(name, b"libdl.so.2\0".as_ptr()) ||
       is_same_library_name(name, b"libdl.so\0".as_ptr()) ||
       starts_with(name, b"libdl.so.") {
        return true;
    }
    
    // Check for librt (merged into libc in musl)
    if is_same_library_name(name, b"librt.so.1\0".as_ptr()) ||
       is_same_library_name(name, b"librt.so\0".as_ptr()) ||
       starts_with(name, b"librt.so.") {
        return true;
    }
    
    // Check for libm (provided by libnrlib)
    if is_same_library_name(name, b"libm.so.6\0".as_ptr()) ||
       is_same_library_name(name, b"libm.so\0".as_ptr()) ||
       starts_with(name, b"libm.so.") {
        return true;
    }
    
    // Check for libcrypt
    if starts_with(name, b"libcrypt.so") {
        return true;
    }
    
    // Check for libresolv
    if starts_with(name, b"libresolv.so") {
        return true;
    }
    
    // Check for libutil
    if starts_with(name, b"libutil.so") {
        return true;
    }
    
    false
}

/// Map library name to NexaOS equivalent
/// Returns the original name if no mapping exists
fn map_library_name(name: &[u8]) -> [u8; 64] {
    let mut result = [0u8; 64];
    
    // Default: copy original name
    for (i, &c) in name.iter().enumerate() {
        if i >= 63 || c == 0 {
            break;
        }
        result[i] = c;
    }
    
    result
}

// ============================================================================
// Dynamic Linker State
// ============================================================================

/// Maximum number of loaded libraries
const MAX_LIBS: usize = 16;

/// Library search paths
// Library search paths as fixed-size byte arrays to avoid relocation issues
const LIB_PATH_1: &[u8; 8] = b"/lib64\0\0";
const LIB_PATH_2: &[u8; 6] = b"/lib\0\0";
const LIB_PATH_3: &[u8; 12] = b"/usr/lib64\0\0";
const LIB_PATH_4: &[u8; 10] = b"/usr/lib\0\0";

/// Information parsed from auxiliary vector
#[derive(Clone, Copy)]
struct AuxInfo {
    at_phdr: u64,     // Program headers address
    at_phent: u64,    // Program header entry size
    at_phnum: u64,    // Number of program headers
    at_pagesz: u64,   // Page size
    at_base: u64,     // Interpreter base address
    at_entry: u64,    // Main executable entry point
    at_execfn: u64,   // Executable filename
    at_random: u64,   // Address of random bytes
    at_secure: u64,   // Secure mode flag
    at_uid: u64,      // Real UID
    at_euid: u64,     // Effective UID
    at_gid: u64,      // Real GID
    at_egid: u64,     // Effective GID
    at_hwcap: u64,    // Hardware capabilities
    at_hwcap2: u64,   // Hardware capabilities 2
    at_clktck: u64,   // Clock ticks per second
    at_sysinfo_ehdr: u64, // vDSO address
}

impl AuxInfo {
    const fn new() -> Self {
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

/// Dynamic section information
#[derive(Clone, Copy)]
struct DynInfo {
    strtab: u64,
    symtab: u64,
    strsz: u64,
    syment: u64,
    rela: u64,
    relasz: u64,
    relaent: u64,
    relacount: u64,    // DT_RELACOUNT - count of relative relocations
    jmprel: u64,
    pltrelsz: u64,
    pltrel: u64,       // DT_PLTREL - type of PLT relocations
    init: u64,
    fini: u64,
    init_array: u64,
    init_arraysz: u64,
    fini_array: u64,
    fini_arraysz: u64,
    preinit_array: u64,
    preinit_arraysz: u64,
    hash: u64,
    gnu_hash: u64,
    flags: u64,        // DT_FLAGS
    flags_1: u64,      // DT_FLAGS_1
    // Version information
    versym: u64,       // DT_VERSYM
    verneed: u64,      // DT_VERNEED
    verneednum: u64,   // DT_VERNEEDNUM
    // TLS information
    tls_modid: u64,    // TLS module ID (assigned at runtime)
    needed: [u64; 16],  // DT_NEEDED offsets into strtab
    needed_count: usize,
}

impl DynInfo {
    const fn new() -> Self {
        Self {
            strtab: 0,
            symtab: 0,
            strsz: 0,
            syment: 24,  // sizeof(Elf64_Sym)
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

/// Loaded library information
#[derive(Clone, Copy)]
struct LoadedLib {
    /// Base address where library is loaded
    base_addr: u64,
    /// Load bias (runtime - link time address)
    load_bias: i64,
    /// Dynamic section info
    dyn_info: DynInfo,
    /// Is this entry valid
    valid: bool,
}

impl LoadedLib {
    const fn new() -> Self {
        Self {
            base_addr: 0,
            load_bias: 0,
            dyn_info: DynInfo::new(),
            valid: false,
        }
    }
}

/// Global symbol table for all loaded libraries
struct GlobalSymbolTable {
    /// Loaded libraries (index 0 = main executable, 1+ = shared libs)
    libs: [LoadedLib; MAX_LIBS],
    /// Number of loaded libraries
    lib_count: usize,
}

impl GlobalSymbolTable {
    const fn new() -> Self {
        Self {
            libs: [LoadedLib::new(); MAX_LIBS],
            lib_count: 0,
        }
    }
}

/// Global symbol table instance
static mut GLOBAL_SYMTAB: GlobalSymbolTable = GlobalSymbolTable::new();

// ============================================================================
// Global Auxiliary Vector Storage (for getauxval support)
// ============================================================================

/// Global auxiliary vector storage
static mut AUXV_STORAGE: AuxInfo = AuxInfo::new();
static mut AUXV_INITIALIZED: bool = false;

/// Store auxiliary vector globally
unsafe fn store_auxv(aux_info: &AuxInfo) {
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

// ============================================================================
// TLS (Thread-Local Storage) Support
// ============================================================================

/// TLS module information
#[derive(Clone, Copy)]
struct TlsModule {
    /// Module ID (1-based, 0 means unused)
    id: u64,
    /// TLS image address in memory
    image: u64,
    /// TLS image size (file size)
    filesz: u64,
    /// TLS memory size (including BSS)
    memsz: u64,
    /// TLS alignment
    align: u64,
    /// Offset in static TLS block
    offset: i64,
}

impl TlsModule {
    const fn new() -> Self {
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

/// Maximum TLS modules
const MAX_TLS_MODULES: usize = 16;

/// Global TLS state
struct TlsState {
    /// TLS modules
    modules: [TlsModule; MAX_TLS_MODULES],
    /// Number of TLS modules
    count: usize,
    /// Total static TLS size
    total_size: usize,
    /// TLS module ID counter
    next_id: u64,
}

impl TlsState {
    const fn new() -> Self {
        Self {
            modules: [TlsModule::new(); MAX_TLS_MODULES],
            count: 0,
            total_size: 0,
            next_id: 1,
        }
    }
}

static mut TLS_STATE: TlsState = TlsState::new();

/// Register a TLS module (called during library loading)
unsafe fn register_tls_module(image: u64, filesz: u64, memsz: u64, align: u64) -> u64 {
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

/// __tls_get_addr - TLS access function (musl/glibc compatible)
/// Used for accessing thread-local variables in dynamically loaded libraries
#[repr(C)]
pub struct TlsIndex {
    ti_module: u64,  // Module ID
    ti_offset: u64,  // Offset within module
}

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

// ============================================================================
// ELF Loading Functions
// ============================================================================

/// Page size
const PAGE_SIZE: u64 = 4096;

/// Align down to page boundary
fn page_align_down(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}

/// Align up to page boundary  
fn page_align_up(addr: u64) -> u64 {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

/// Open a file and return file descriptor
/// NexaOS sys_open now takes (path_ptr, flags, mode) - standard POSIX interface
unsafe fn open_file(path: *const u8) -> i64 {
    // Pass path pointer, flags=O_RDONLY(0), mode=0
    // Kernel reads null-terminated string from path pointer
    syscall3(SYS_OPEN, path as u64, 0, 0) as i64
}

/// Close a file descriptor
unsafe fn close_file(fd: i32) {
    syscall1(SYS_CLOSE, fd as u64);
}

/// Read from file
unsafe fn read_bytes(fd: i32, buf: *mut u8, len: usize) -> isize {
    syscall3(SYS_READ, fd as u64, buf as u64, len as u64) as isize
}

/// Seek in file
unsafe fn lseek(fd: i32, offset: i64, whence: i32) -> i64 {
    syscall3(SYS_LSEEK, fd as u64, offset as u64, whence as u64) as i64
}

/// Load a shared library from path
/// Returns (base_addr, load_bias, dyn_info) or (0, 0, DynInfo::new()) on failure
unsafe fn load_shared_library(path: *const u8) -> (u64, i64, DynInfo) {
    let fd = open_file(path);
    if fd < 0 {
        return (0, 0, DynInfo::new());
    }
    
    // Read ELF header
    let mut ehdr_buf = [0u8; 64];
    let bytes_read = read_bytes(fd as i32, ehdr_buf.as_mut_ptr(), 64);
    if bytes_read < 64 {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    let ehdr = &*(ehdr_buf.as_ptr() as *const Elf64Ehdr);
    
    // Validate ELF magic
    if ehdr.e_ident[0] != 0x7f || ehdr.e_ident[1] != b'E' || 
       ehdr.e_ident[2] != b'L' || ehdr.e_ident[3] != b'F' {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    // Must be shared object (ET_DYN = 3)
    if ehdr.e_type != 3 {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    // Read program headers
    let phdr_size = (ehdr.e_phentsize as usize) * (ehdr.e_phnum as usize);
    if phdr_size > 2048 {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    lseek(fd as i32, ehdr.e_phoff as i64, 0); // SEEK_SET
    let mut phdr_buf = [0u8; 2048];
    let bytes_read = read_bytes(fd as i32, phdr_buf.as_mut_ptr(), phdr_size);
    if bytes_read < phdr_size as isize {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    let phdrs = core::slice::from_raw_parts(
        phdr_buf.as_ptr() as *const Elf64Phdr,
        ehdr.e_phnum as usize,
    );
    
    // Find extent of loadable segments and TLS segment
    let mut load_addr_min: u64 = u64::MAX;
    let mut load_addr_max: u64 = 0;
    let mut dyn_vaddr: u64 = 0;
    let mut tls_phdr: Option<Elf64Phdr> = None;
    
    for phdr in phdrs {
        if phdr.p_type == PT_LOAD {
            let seg_start = page_align_down(phdr.p_vaddr);
            let seg_end = page_align_up(phdr.p_vaddr + phdr.p_memsz);
            if seg_start < load_addr_min {
                load_addr_min = seg_start;
            }
            if seg_end > load_addr_max {
                load_addr_max = seg_end;
            }
        }
        if phdr.p_type == PT_DYNAMIC {
            dyn_vaddr = phdr.p_vaddr;
        }
        if phdr.p_type == PT_TLS {
            tls_phdr = Some(*phdr);
        }
    }
    
    if load_addr_min == u64::MAX {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    let total_size = load_addr_max - load_addr_min;
    
    // Allocate memory for the library
    // For MAP_ANONYMOUS, fd should be -1 (passed as u64 representation of -1)
    let base_addr = syscall6(
        SYS_MMAP,
        0,                          // addr
        total_size,                 // length
        PROT_READ | PROT_WRITE | PROT_EXEC, // prot
        MAP_PRIVATE | MAP_ANONYMOUS, // flags
        (-1i64) as u64,             // fd = -1 for anonymous mapping
        0,                          // offset
    );
    
    // Check for mmap failure
    if base_addr >= 0xFFFF_FFFF_FFFF_F000 || base_addr == 0 {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    let load_bias = base_addr as i64 - load_addr_min as i64;
    
    // Load each PT_LOAD segment
    for phdr in phdrs {
        if phdr.p_type != PT_LOAD {
            continue;
        }
        
        if phdr.p_filesz > 0 {
            // Seek to segment in file
            lseek(fd as i32, phdr.p_offset as i64, 0);
            
            // Read segment data
            let dest_addr = (phdr.p_vaddr as i64 + load_bias) as *mut u8;
            let mut total_read: u64 = 0;
            while total_read < phdr.p_filesz {
                let to_read = core::cmp::min(phdr.p_filesz - total_read, 4096) as usize;
                let read = read_bytes(fd as i32, dest_addr.add(total_read as usize), to_read);
                if read <= 0 {
                    break;
                }
                total_read += read as u64;
            }
        }
        
        // Zero BSS (memsz > filesz)
        if phdr.p_memsz > phdr.p_filesz {
            let bss_start = ((phdr.p_vaddr + phdr.p_filesz) as i64 + load_bias) as *mut u8;
            let bss_size = (phdr.p_memsz - phdr.p_filesz) as usize;
            memset(bss_start, 0, bss_size);
        }
    }
    
    close_file(fd as i32);
    
    // Parse dynamic section
    let mut dyn_info = DynInfo::new();
    if dyn_vaddr != 0 {
        let dyn_addr = (dyn_vaddr as i64 + load_bias) as u64;
        parse_dynamic_section(dyn_addr, load_bias, &mut dyn_info);
    }
    
    // Register TLS module if PT_TLS segment exists
    if let Some(tls) = tls_phdr {
        let tls_image = (tls.p_vaddr as i64 + load_bias) as u64;
        let tls_mod_id = register_tls_module(
            tls_image,
            tls.p_filesz,
            tls.p_memsz,
            tls.p_align,
        );
        dyn_info.tls_modid = tls_mod_id;
    }
    
    (base_addr, load_bias, dyn_info)
}

/// Parse dynamic section and fill DynInfo
unsafe fn parse_dynamic_section(dyn_addr: u64, load_bias: i64, dyn_info: &mut DynInfo) {
    let mut dyn_ptr = dyn_addr as *const Elf64Dyn;
    
    loop {
        let entry = *dyn_ptr;
        if entry.d_tag == DT_NULL {
            break;
        }
        match entry.d_tag {
            DT_STRTAB => dyn_info.strtab = (entry.d_val as i64 + load_bias) as u64,
            DT_SYMTAB => dyn_info.symtab = (entry.d_val as i64 + load_bias) as u64,
            DT_STRSZ => dyn_info.strsz = entry.d_val,
            DT_SYMENT => dyn_info.syment = entry.d_val,
            DT_RELA => dyn_info.rela = (entry.d_val as i64 + load_bias) as u64,
            DT_RELASZ => dyn_info.relasz = entry.d_val,
            DT_RELAENT => dyn_info.relaent = entry.d_val,
            DT_RELACOUNT => dyn_info.relacount = entry.d_val,
            DT_JMPREL => dyn_info.jmprel = (entry.d_val as i64 + load_bias) as u64,
            DT_PLTRELSZ => dyn_info.pltrelsz = entry.d_val,
            DT_PLTREL => dyn_info.pltrel = entry.d_val,
            DT_INIT => dyn_info.init = (entry.d_val as i64 + load_bias) as u64,
            DT_FINI => dyn_info.fini = (entry.d_val as i64 + load_bias) as u64,
            DT_INIT_ARRAY => dyn_info.init_array = (entry.d_val as i64 + load_bias) as u64,
            DT_INIT_ARRAYSZ => dyn_info.init_arraysz = entry.d_val,
            DT_FINI_ARRAY => dyn_info.fini_array = (entry.d_val as i64 + load_bias) as u64,
            DT_FINI_ARRAYSZ => dyn_info.fini_arraysz = entry.d_val,
            DT_PREINIT_ARRAY => dyn_info.preinit_array = (entry.d_val as i64 + load_bias) as u64,
            DT_PREINIT_ARRAYSZ => dyn_info.preinit_arraysz = entry.d_val,
            DT_HASH => dyn_info.hash = (entry.d_val as i64 + load_bias) as u64,
            DT_GNU_HASH => dyn_info.gnu_hash = (entry.d_val as i64 + load_bias) as u64,
            DT_FLAGS => dyn_info.flags = entry.d_val,
            DT_FLAGS_1 => dyn_info.flags_1 = entry.d_val,
            DT_VERSYM => dyn_info.versym = (entry.d_val as i64 + load_bias) as u64,
            DT_VERNEED => dyn_info.verneed = (entry.d_val as i64 + load_bias) as u64,
            DT_VERNEEDNUM => dyn_info.verneednum = entry.d_val,
            DT_NEEDED => {
                if dyn_info.needed_count < 16 {
                    dyn_info.needed[dyn_info.needed_count] = entry.d_val;
                    dyn_info.needed_count += 1;
                }
            }
            _ => {}
        }
        dyn_ptr = dyn_ptr.add(1);
    }
}

/// Search for a library in standard paths
unsafe fn search_library(name: &[u8]) -> Option<[u8; 256]> {
    let mut path_buf = [0u8; 256];
    
    // Use stack-local array to avoid global pointer relocation issues
    let search_paths: [&[u8]; 4] = [
        LIB_PATH_1.as_slice(),
        LIB_PATH_2.as_slice(),
        LIB_PATH_3.as_slice(),
        LIB_PATH_4.as_slice(),
    ];
    
    let mut path_idx = 0usize;
    while path_idx < 4 {
        let search_path = search_paths[path_idx];
        // Build path: search_path + "/" + name
        let mut pos = 0;
        
        // Copy search path (without null terminator)
        let mut i = 0;
        while i < search_path.len() && search_path[i] != 0 {
            if pos < 255 {
                path_buf[pos] = search_path[i];
                pos += 1;
            }
            i += 1;
        }
        
        // Add separator
        if pos < 255 {
            path_buf[pos] = b'/';
            pos += 1;
        }
        
        // Copy name
        for &c in name {
            if c == 0 {
                break;
            }
            if pos < 255 {
                path_buf[pos] = c;
                pos += 1;
            }
        }
        
        // Null terminate
        path_buf[pos] = 0;
        
        // Try to open to check if it exists
        let fd = open_file(path_buf.as_ptr());
        if fd >= 0 {
            close_file(fd as i32);
            return Some(path_buf);
        }
        
        path_idx += 1;
    }
    
    None
}

/// Get symbol count from hash table
unsafe fn get_symbol_count(dyn_info: &DynInfo) -> usize {
    if dyn_info.hash != 0 {
        // ELF hash: nbucket at offset 0, nchain at offset 4
        let nchain = *((dyn_info.hash + 4) as *const u32);
        return nchain as usize;
    }
    // Fallback
    256
}

// ============================================================================
// GNU Hash Table Support (for faster symbol lookup)
// ============================================================================

/// GNU hash function
fn gnu_hash(name: &[u8]) -> u32 {
    let mut h: u32 = 5381;
    for &c in name {
        if c == 0 {
            break;
        }
        h = h.wrapping_mul(33).wrapping_add(c as u32);
    }
    h
}

/// ELF SYSV hash function
fn elf_hash(name: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in name {
        if c == 0 {
            break;
        }
        h = (h << 4).wrapping_add(c as u32);
        let g = h & 0xf0000000;
        if g != 0 {
            h ^= g >> 24;
        }
        h &= !g;
    }
    h
}

/// Lookup symbol using GNU hash table (much faster than linear search)
/// Returns symbol value (with load_bias applied) or 0 if not found
unsafe fn lookup_symbol_gnu_hash(lib: &LoadedLib, name: &[u8]) -> u64 {
    let dyn_info = &lib.dyn_info;
    
    if dyn_info.gnu_hash == 0 || dyn_info.symtab == 0 || dyn_info.strtab == 0 {
        return 0;
    }
    
    let gnu_hash_addr = dyn_info.gnu_hash;
    let symtab = dyn_info.symtab;
    let strtab = dyn_info.strtab;
    let syment = if dyn_info.syment == 0 { 24 } else { dyn_info.syment };
    
    // GNU hash table layout:
    // uint32_t nbuckets
    // uint32_t symoffset
    // uint32_t bloom_size
    // uint32_t bloom_shift
    // uint64_t bloom[bloom_size]  (for 64-bit)
    // uint32_t buckets[nbuckets]
    // uint32_t chains[]
    
    let nbuckets = *(gnu_hash_addr as *const u32);
    let symoffset = *((gnu_hash_addr + 4) as *const u32);
    let bloom_size = *((gnu_hash_addr + 8) as *const u32);
    let bloom_shift = *((gnu_hash_addr + 12) as *const u32);
    
    if nbuckets == 0 {
        return 0;
    }
    
    let h1 = gnu_hash(name);
    let h2 = h1 >> bloom_shift;
    
    // Check bloom filter first (64-bit)
    let bloom = (gnu_hash_addr + 16) as *const u64;
    let bloom_word = *bloom.add((h1 as usize / 64) % bloom_size as usize);
    let mask = (1u64 << (h1 % 64)) | (1u64 << (h2 % 64));
    
    if (bloom_word & mask) != mask {
        // Symbol definitely not present
        return 0;
    }
    
    // Calculate bucket and chain offsets
    let buckets = (gnu_hash_addr + 16 + (bloom_size as u64 * 8)) as *const u32;
    let chains = buckets.add(nbuckets as usize);
    
    let bucket = h1 % nbuckets;
    let mut sym_idx = *buckets.add(bucket as usize);
    
    if sym_idx == 0 {
        return 0;
    }
    
    // Search the chain
    loop {
        let chain_idx = sym_idx - symoffset;
        let chain_val = *chains.add(chain_idx as usize);
        
        // Check if hash matches (ignoring LSB which marks end of chain)
        if (chain_val | 1) == (h1 | 1) {
            // Hash matches, verify name
            let sym = &*((symtab + (sym_idx as u64) * syment) as *const Elf64Sym);
            let sym_name_ptr = (strtab + sym.st_name as u64) as *const u8;
            
            // Compare names
            let mut j = 0;
            let mut match_found = true;
            while j < name.len() {
                let c = *sym_name_ptr.add(j);
                if c != name[j] {
                    match_found = false;
                    break;
                }
                j += 1;
            }
            
            if match_found && *sym_name_ptr.add(j) == 0 {
                // Found it!
                if sym.st_value != 0 || sym.st_shndx != 0 {
                    return (sym.st_value as i64 + lib.load_bias) as u64;
                }
            }
        }
        
        // Check end of chain (LSB set)
        if (chain_val & 1) != 0 {
            break;
        }
        
        sym_idx += 1;
    }
    
    0
}

/// Lookup symbol using ELF SYSV hash table
unsafe fn lookup_symbol_elf_hash(lib: &LoadedLib, name: &[u8]) -> u64 {
    let dyn_info = &lib.dyn_info;
    
    if dyn_info.hash == 0 || dyn_info.symtab == 0 || dyn_info.strtab == 0 {
        return 0;
    }
    
    let hash_addr = dyn_info.hash;
    let symtab = dyn_info.symtab;
    let strtab = dyn_info.strtab;
    let syment = if dyn_info.syment == 0 { 24 } else { dyn_info.syment };
    
    // ELF hash table layout:
    // uint32_t nbucket
    // uint32_t nchain
    // uint32_t bucket[nbucket]
    // uint32_t chain[nchain]
    
    let nbucket = *(hash_addr as *const u32);
    let _nchain = *((hash_addr + 4) as *const u32);
    
    if nbucket == 0 {
        return 0;
    }
    
    let h = elf_hash(name);
    let bucket = (hash_addr + 8) as *const u32;
    let chain = bucket.add(nbucket as usize);
    
    let mut sym_idx = *bucket.add((h % nbucket) as usize);
    
    while sym_idx != 0 {
        let sym = &*((symtab + (sym_idx as u64) * syment) as *const Elf64Sym);
        let sym_name_ptr = (strtab + sym.st_name as u64) as *const u8;
        
        // Compare names
        let mut j = 0;
        let mut match_found = true;
        while j < name.len() {
            let c = *sym_name_ptr.add(j);
            if c != name[j] {
                match_found = false;
                break;
            }
            j += 1;
        }
        
        if match_found && *sym_name_ptr.add(j) == 0 {
            if sym.st_value != 0 || sym.st_shndx != 0 {
                return (sym.st_value as i64 + lib.load_bias) as u64;
            }
        }
        
        sym_idx = *chain.add(sym_idx as usize);
    }
    
    0
}

/// Lookup a symbol by name in a single library
/// Uses GNU hash if available, falls back to ELF hash or linear search
/// Returns symbol value (with load_bias applied) or 0 if not found
unsafe fn lookup_symbol_in_lib(lib: &LoadedLib, name: &[u8]) -> u64 {
    if !lib.valid {
        return 0;
    }
    
    let dyn_info = &lib.dyn_info;
    if dyn_info.symtab == 0 || dyn_info.strtab == 0 {
        return 0;
    }
    
    // Try GNU hash first (fastest)
    if dyn_info.gnu_hash != 0 {
        let result = lookup_symbol_gnu_hash(lib, name);
        if result != 0 {
            return result;
        }
    }
    
    // Try ELF SYSV hash
    if dyn_info.hash != 0 {
        let result = lookup_symbol_elf_hash(lib, name);
        if result != 0 {
            return result;
        }
    }
    
    // Fallback to linear search
    let sym_count = get_symbol_count(dyn_info);
    let syment = if dyn_info.syment == 0 { 24 } else { dyn_info.syment };
    
    // Linear search through symbol table
    for i in 0..sym_count {
        let sym = &*((dyn_info.symtab + (i as u64) * syment) as *const Elf64Sym);
        
        // Skip undefined symbols and symbols with st_name == 0
        if sym.st_name == 0 || sym.st_shndx == 0 {
            continue;
        }
        
        // Get symbol name from string table
        let sym_name_ptr = (dyn_info.strtab + sym.st_name as u64) as *const u8;
        
        // Compare names - name is a slice without null terminator
        let mut j = 0;
        let mut match_found = true;
        while j < name.len() {
            let c = *sym_name_ptr.add(j);
            let target = name[j];
            if c != target {
                match_found = false;
                break;
            }
            j += 1;
        }
        
        // Check that symbol name ends at the same position (sym_name[j] should be 0)
        if match_found {
            let c = *sym_name_ptr.add(j);
            if c != 0 {
                // Symbol name is longer, not a match
                match_found = false;
            }
        }
        
        if match_found && sym.st_value != 0 {
            // Found it!
            return (sym.st_value as i64 + lib.load_bias) as u64;
        }
    }
    
    0
}

/// Global symbol lookup - search all loaded libraries
/// Search order: main executable first, then libraries in load order
unsafe fn global_symbol_lookup(name: &[u8]) -> u64 {
    for i in 0..GLOBAL_SYMTAB.lib_count {
        let addr = lookup_symbol_in_lib(&GLOBAL_SYMTAB.libs[i], name);
        if addr != 0 {
            return addr;
        }
    }
    0
}

/// Get symbol name from symbol table by index
unsafe fn get_symbol_name(dyn_info: &DynInfo, sym_idx: u32) -> *const u8 {
    if dyn_info.symtab == 0 || dyn_info.strtab == 0 {
        return core::ptr::null();
    }
    
    let syment = if dyn_info.syment == 0 { 24 } else { dyn_info.syment };
    let sym = &*((dyn_info.symtab + (sym_idx as u64) * syment) as *const Elf64Sym);
    
    if sym.st_name == 0 {
        return core::ptr::null();
    }
    
    (dyn_info.strtab + sym.st_name as u64) as *const u8
}

// ============================================================================
// Entry Point
// ============================================================================

/// Raw entry point - receives stack pointer from kernel
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    naked_asm!(
        // The kernel passes:
        // - argc at [rsp]
        // - argv at [rsp + 8]
        // - envp after argv (NULL terminated)
        // - auxv after envp (NULL terminated)
        "mov rdi, rsp",           // Pass stack pointer as first argument
        "and rsp, -16",           // Align stack to 16 bytes
        "xor rbp, rbp",           // Clear frame pointer
        "call {ld_main}",
        "ud2",                    // Should never return
        ld_main = sym ld_main,
    );
}

/// Also provide _start_c for compatibility
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn _start_c() -> ! {
    naked_asm!(
        "jmp _start",
    );
}

/// Main dynamic linker entry point
#[no_mangle]
unsafe extern "C" fn ld_main(stack_ptr: *const u64) -> ! {
    // Parse the stack to get argc, argv, envp, auxv
    let argc = *stack_ptr as usize;
    let argv = stack_ptr.add(1) as *const *const u8;
    
    // Skip past argv (argc+1 entries including NULL terminator)
    let mut ptr = argv.add(argc + 1) as *const *const u8;
    
    // Skip past envp (until NULL)
    while !(*ptr).is_null() {
        ptr = ptr.add(1);
    }
    ptr = ptr.add(1); // Skip NULL terminator
    
    // Now ptr points to auxv
    let auxv = ptr as *const AuxEntry;
    
    // Parse auxiliary vector
    let mut aux_info = AuxInfo::new();
    let mut aux_ptr = auxv;
    loop {
        let entry = *aux_ptr;
        if entry.a_type == AT_NULL {
            break;
        }
        match entry.a_type {
            AT_PHDR => aux_info.at_phdr = entry.a_val,
            AT_PHENT => aux_info.at_phent = entry.a_val,
            AT_PHNUM => aux_info.at_phnum = entry.a_val,
            AT_PAGESZ => aux_info.at_pagesz = entry.a_val,
            AT_BASE => aux_info.at_base = entry.a_val,
            AT_ENTRY => aux_info.at_entry = entry.a_val,
            AT_EXECFN => aux_info.at_execfn = entry.a_val,
            AT_RANDOM => aux_info.at_random = entry.a_val,
            AT_SECURE => aux_info.at_secure = entry.a_val,
            AT_UID => aux_info.at_uid = entry.a_val,
            AT_EUID => aux_info.at_euid = entry.a_val,
            AT_GID => aux_info.at_gid = entry.a_val,
            AT_EGID => aux_info.at_egid = entry.a_val,
            AT_HWCAP => aux_info.at_hwcap = entry.a_val,
            AT_HWCAP2 => aux_info.at_hwcap2 = entry.a_val,
            AT_CLKTCK => aux_info.at_clktck = entry.a_val,
            AT_SYSINFO_EHDR => aux_info.at_sysinfo_ehdr = entry.a_val,
            _ => {}
        }
        aux_ptr = aux_ptr.add(1);
    }
    
    // Store auxv globally for getauxval support
    store_auxv(&aux_info);

    // Find the dynamic section of the main executable
    if aux_info.at_phdr == 0 || aux_info.at_phnum == 0 {
        print_str("[ld-nrlib] ERROR: Invalid auxv\n");
        exit(127);
    }

    // Scan program headers to find PT_DYNAMIC
    let mut dyn_addr: u64 = 0;
    let mut load_bias: i64 = 0;
    let mut first_load_vaddr: u64 = u64::MAX;
    
    let phdrs = core::slice::from_raw_parts(
        aux_info.at_phdr as *const Elf64Phdr,
        aux_info.at_phnum as usize,
    );
    
    // Calculate load bias from first PT_LOAD segment
    for phdr in phdrs {
        if phdr.p_type == PT_LOAD && phdr.p_vaddr < first_load_vaddr {
            first_load_vaddr = phdr.p_vaddr;
        }
        if phdr.p_type == PT_DYNAMIC {
            dyn_addr = phdr.p_vaddr;
        }
    }

    // For PIE executables loaded at AT_PHDR location
    if first_load_vaddr != u64::MAX {
        // The PHDR is at its loaded address, first LOAD vaddr tells us link-time base
        // load_bias = actual_load_addr - link_time_vaddr
        // But we need to figure out actual load address from AT_PHDR
        for phdr in phdrs {
            if phdr.p_type == PT_PHDR {
                load_bias = (aux_info.at_phdr as i64) - (phdr.p_vaddr as i64);
                break;
            }
        }
    }

    // Parse main executable's dynamic section first
    let mut main_dyn_info = DynInfo::new();
    if dyn_addr != 0 {
        dyn_addr = (dyn_addr as i64 + load_bias) as u64;
        
        // Parse dynamic section (including DT_NEEDED)
        parse_dynamic_section(dyn_addr, load_bias, &mut main_dyn_info);
        
        // Store main executable info in global symbol table (index 0)
        let main_lib = &mut GLOBAL_SYMTAB.libs[0];
        main_lib.base_addr = (first_load_vaddr as i64 + load_bias) as u64;
        main_lib.load_bias = load_bias;
        main_lib.dyn_info = main_dyn_info;
        main_lib.valid = true;
        GLOBAL_SYMTAB.lib_count = 1;
        
        // ================================================================
        // Step 1: Load libnrlib.so first (always needed)
        // ================================================================
        
        // Try to load libnrlib.so
        let libnrlib_path = b"/lib64/libnrlib.so\0";
        let (lib_base, lib_bias, lib_dyn_info) = load_shared_library(libnrlib_path.as_ptr());
        
        if lib_base != 0 {
            // Register libnrlib.so in global symbol table
            let lib_idx = GLOBAL_SYMTAB.lib_count;
            if lib_idx < MAX_LIBS {
                let lib = &mut GLOBAL_SYMTAB.libs[lib_idx];
                lib.base_addr = lib_base;
                lib.load_bias = lib_bias;
                lib.dyn_info = lib_dyn_info;
                lib.valid = true;
                GLOBAL_SYMTAB.lib_count = lib_idx + 1;
                
                // Process libnrlib.so's RELATIVE relocations first
                if lib_dyn_info.rela != 0 && lib_dyn_info.relasz > 0 {
                    process_rela(lib_dyn_info.rela, lib_dyn_info.relasz, lib_dyn_info.relaent, lib_bias);
                }
                
                // Call libnrlib.so init functions
                if lib_dyn_info.init != 0 {
                    let init_fn: extern "C" fn() = core::mem::transmute(lib_dyn_info.init);
                    init_fn();
                }
                
                if lib_dyn_info.init_array != 0 && lib_dyn_info.init_arraysz > 0 {
                    let count = lib_dyn_info.init_arraysz / 8;
                    let array = lib_dyn_info.init_array as *const u64;
                    for i in 0..count {
                        let fn_ptr = *array.add(i as usize);
                        if fn_ptr != 0 && fn_ptr != u64::MAX {
                            let init_fn: extern "C" fn() = core::mem::transmute(fn_ptr);
                            init_fn();
                        }
                    }
                }
            }
        }
        
        // ================================================================
        // Step 2: Load other DT_NEEDED libraries
        // ================================================================
        if main_dyn_info.needed_count > 0 {
            for i in 0..main_dyn_info.needed_count {
                let name_offset = main_dyn_info.needed[i];
                let name_ptr = (main_dyn_info.strtab + name_offset) as *const u8;
                
                // Skip libraries that map to libnrlib.so (musl-compatible mappings)
                if is_libc_library(name_ptr) {
                    continue;
                }
                
                // Try to find and load the library
                let name_len = cstr_len(name_ptr);
                let name_slice = core::slice::from_raw_parts(name_ptr, name_len);
                
                // Try mapped name first, then original name
                let mapped_name = map_library_name(name_slice);
                
                if let Some(path) = search_library(&mapped_name) {
                    let (lib_base, lib_bias, lib_dyn_info) = load_shared_library(path.as_ptr());
                    if lib_base != 0 {
                        let lib_idx = GLOBAL_SYMTAB.lib_count;
                        if lib_idx < MAX_LIBS {
                            let lib = &mut GLOBAL_SYMTAB.libs[lib_idx];
                            lib.base_addr = lib_base;
                            lib.load_bias = lib_bias;
                            lib.dyn_info = lib_dyn_info;
                            lib.valid = true;
                            GLOBAL_SYMTAB.lib_count = lib_idx + 1;
                            
                            // Process library's relocations
                            if lib_dyn_info.rela != 0 && lib_dyn_info.relasz > 0 {
                                process_rela(lib_dyn_info.rela, lib_dyn_info.relasz, lib_dyn_info.relaent, lib_bias);
                            }
                        }
                    }
                } else if let Some(path) = search_library(name_slice) {
                    let (lib_base, lib_bias, lib_dyn_info) = load_shared_library(path.as_ptr());
                    if lib_base != 0 {
                        let lib_idx = GLOBAL_SYMTAB.lib_count;
                        if lib_idx < MAX_LIBS {
                            let lib = &mut GLOBAL_SYMTAB.libs[lib_idx];
                            lib.base_addr = lib_base;
                            lib.load_bias = lib_bias;
                            lib.dyn_info = lib_dyn_info;
                            lib.valid = true;
                            GLOBAL_SYMTAB.lib_count = lib_idx + 1;
                            
                            // Process library's relocations
                            if lib_dyn_info.rela != 0 && lib_dyn_info.relasz > 0 {
                                process_rela(lib_dyn_info.rela, lib_dyn_info.relasz, lib_dyn_info.relaent, lib_bias);
                            }
                        }
                    }
                }
            }
        }
        
        // ================================================================
        // Step 3: Now process main executable's relocations with symbol lookup
        // ================================================================
        
        // Process RELA relocations with full symbol lookup
        if main_dyn_info.rela != 0 && main_dyn_info.relasz > 0 {
            process_rela_with_symtab(main_dyn_info.rela, main_dyn_info.relasz, main_dyn_info.relaent, load_bias, &main_dyn_info);
        }
        
        // Process PLT/JMPREL relocations with full symbol lookup
        if main_dyn_info.jmprel != 0 && main_dyn_info.pltrelsz > 0 {
            process_rela_with_symtab(main_dyn_info.jmprel, main_dyn_info.pltrelsz, 24, load_bias, &main_dyn_info);
        }
        
        // ================================================================
        // Step 4: Process library relocations that need main executable symbols
        // ================================================================
        for i in 1..GLOBAL_SYMTAB.lib_count {
            let lib = &GLOBAL_SYMTAB.libs[i];
            if !lib.valid {
                continue;
            }
            
            // Process RELA relocations (includes GLOB_DAT) with symbol lookup
            if lib.dyn_info.rela != 0 && lib.dyn_info.relasz > 0 {
                process_rela_with_symtab(lib.dyn_info.rela, lib.dyn_info.relasz, lib.dyn_info.relaent, lib.load_bias, &lib.dyn_info);
            }
            
            // Process PLT/JMPREL relocations
            if lib.dyn_info.jmprel != 0 && lib.dyn_info.pltrelsz > 0 {
                process_rela_with_symtab(lib.dyn_info.jmprel, lib.dyn_info.pltrelsz, 24, lib.load_bias, &lib.dyn_info);
            }
        }

        // ================================================================
        // Step 5: Call preinit_array for main executable (before init)
        // Note: preinit_array is only for main executable, not shared libs
        // ================================================================
        if main_dyn_info.preinit_array != 0 && main_dyn_info.preinit_arraysz > 0 {
            let count = main_dyn_info.preinit_arraysz / 8;
            let array = main_dyn_info.preinit_array as *const u64;
            for i in 0..count {
                let fn_ptr = *array.add(i as usize);
                if fn_ptr != 0 && fn_ptr != u64::MAX {
                    let preinit_fn: extern "C" fn() = core::mem::transmute(fn_ptr);
                    preinit_fn();
                }
            }
        }

        // ================================================================
        // Step 6: Call init functions for main executable
        // ================================================================
        if main_dyn_info.init != 0 {
            let init_fn: extern "C" fn() = core::mem::transmute(main_dyn_info.init);
            init_fn();
        }

        if main_dyn_info.init_array != 0 && main_dyn_info.init_arraysz > 0 {
            let count = main_dyn_info.init_arraysz / 8;
            let array = main_dyn_info.init_array as *const u64;
            for i in 0..count {
                let fn_ptr = *array.add(i as usize);
                if fn_ptr != 0 && fn_ptr != u64::MAX {
                    let init_fn: extern "C" fn() = core::mem::transmute(fn_ptr);
                    init_fn();
                }
            }
        }
    }

    // Transfer control to the main executable
    let mut entry = aux_info.at_entry;
    
    // For PIE executables, AT_ENTRY might be 0 (relative entry point)
    // In this case, we need to read e_entry from ELF header and apply load_bias
    if entry == 0 {
        // The ELF header is at the start of the first PT_LOAD segment
        let elf_header_addr = (first_load_vaddr as i64 + load_bias) as u64;
        
        // Read e_entry from offset 24 in ELF header
        let e_entry = *((elf_header_addr + 24) as *const u64);
        
        if e_entry != 0 {
            // Apply load_bias to get actual entry point
            entry = (e_entry as i64 + load_bias) as u64;
        } else {
            // e_entry is also 0, need to find entry point via symbol lookup
            // First, try finding _start in main executable
            if dyn_addr != 0 {
                let start_addr = find_symbol_by_name(dyn_addr, load_bias, b"_start\0");
                if start_addr != 0 {
                    entry = start_addr;
                }
            }
            
            // If not found, search in loaded libraries (global symbol table)
            if entry == 0 {
                // Try _start from any loaded library (typically libnrlib.so)
                let start_addr = global_symbol_lookup(b"_start");
                if start_addr != 0 {
                    entry = start_addr;
                } else {
                    // Try __nexa_get_start_addr to get _start address indirectly
                    let get_start_fn = global_symbol_lookup(b"__nexa_get_start_addr");
                    if get_start_fn != 0 {
                        // Call the function to get _start address
                        let get_start: extern "C" fn() -> usize = core::mem::transmute(get_start_fn);
                        let start_addr = get_start() as u64;
                        if start_addr != 0 {
                            entry = start_addr;
                        }
                    }
                }
                
                // Try __nexa_crt_start (NexaOS nrlib entry point) as fallback
                if entry == 0 {
                    let crt_addr = global_symbol_lookup(b"__nexa_crt_start");
                    if crt_addr != 0 {
                        entry = crt_addr;
                    } else {
                        // Last resort: look for main and call it directly
                        let main_addr = global_symbol_lookup(b"main");
                        if main_addr == 0 && dyn_addr != 0 {
                            let main_addr = find_symbol_by_name(dyn_addr, load_bias, b"main\0");
                            if main_addr != 0 {
                                call_main_directly(main_addr, stack_ptr);
                            }
                        } else if main_addr != 0 {
                            call_main_directly(main_addr, stack_ptr);
                        }
                    }
                }
            }
        }
    }
    
    if entry == 0 {
        print_str("[ld-nrlib] ERROR: No entry point\n");
        exit(127);
    }

    // Jump to the entry point with the original stack
    // The main executable expects the same stack layout as if it was started directly
    jump_to_entry(entry, stack_ptr);
}

/// Find a symbol by name in the dynamic symbol table
/// Returns the symbol's virtual address (with load_bias applied), or 0 if not found
unsafe fn find_symbol_by_name(dyn_addr: u64, load_bias: i64, name: &[u8]) -> u64 {
    let mut strtab: u64 = 0;
    let mut symtab: u64 = 0;
    let mut hash: u64 = 0;
    let mut gnu_hash: u64 = 0;
    
    // Parse dynamic section to find STRTAB, SYMTAB, and HASH/GNU_HASH
    let mut dyn_ptr = dyn_addr as *const Elf64Dyn;
    loop {
        let entry = *dyn_ptr;
        if entry.d_tag == DT_NULL {
            break;
        }
        match entry.d_tag {
            DT_STRTAB => strtab = (entry.d_val as i64 + load_bias) as u64,
            DT_SYMTAB => symtab = (entry.d_val as i64 + load_bias) as u64,
            DT_HASH => hash = (entry.d_val as i64 + load_bias) as u64,
            DT_GNU_HASH => gnu_hash = (entry.d_val as i64 + load_bias) as u64,
            _ => {}
        }
        dyn_ptr = dyn_ptr.add(1);
    }
    
    if symtab == 0 || strtab == 0 {
        return 0;
    }
    
    // Use DT_HASH to determine symbol count if available
    let sym_count = if hash != 0 {
        // ELF hash table: nchain is at offset 4 (u32)
        *((hash + 4) as *const u32) as usize
    } else if gnu_hash != 0 {
        // For GNU hash, we need to scan - use a reasonable max
        256
    } else {
        256 // Fallback
    };
    
    // Linear search through symbol table
    for i in 0..sym_count {
        let sym = &*((symtab + i as u64 * 24) as *const Elf64Sym);
        if sym.st_name == 0 {
            continue;
        }
        
        // Get symbol name from string table
        let sym_name_ptr = (strtab + sym.st_name as u64) as *const u8;
        
        // Compare names (name is null-terminated)
        let mut j = 0;
        let mut match_found = true;
        while j < name.len() {
            let c = *sym_name_ptr.add(j);
            if c != name[j] {
                match_found = false;
                break;
            }
            if c == 0 {
                break;
            }
            j += 1;
        }
        
        if match_found && j == name.len() - 1 {
            // Found it! Return address with load_bias
            if sym.st_value != 0 {
                return (sym.st_value as i64 + load_bias) as u64;
            }
        }
    }
    
    0
}

/// Call main() directly with C calling convention (argc in rdi, argv in rsi)
/// Used when no proper _start or CRT entry point is available
#[inline(never)]
unsafe fn call_main_directly(main_addr: u64, stack_ptr: *const u64) -> ! {
    // Parse argc and argv from stack
    let argc = *stack_ptr as i32;
    let argv = stack_ptr.add(1) as *const *const u8;
    
    // Call main with C calling convention
    let main_fn: extern "C" fn(i32, *const *const u8) -> i32 = core::mem::transmute(main_addr);
    let ret = main_fn(argc, argv);
    
    // Exit with main's return value
    exit(ret);
}

/// Process RELA relocations with symbol lookup
unsafe fn process_rela_with_symtab(rela_addr: u64, relasz: u64, relaent: u64, load_bias: i64, dyn_info: &DynInfo) {
    let entry_size = if relaent == 0 { 24 } else { relaent };
    let count = relasz / entry_size;
    
    for i in 0..count {
        let rela = &*((rela_addr + i * entry_size) as *const Elf64Rela);
        let rel_type = (rela.r_info & 0xffffffff) as u32;
        let sym_idx = (rela.r_info >> 32) as u32;
        
        let target = (rela.r_offset as i64 + load_bias) as *mut u64;
        
        match rel_type {
            R_X86_64_RELATIVE => {
                // R_X86_64_RELATIVE: *target = load_bias + addend
                *target = (load_bias + rela.r_addend) as u64;
            }
            R_X86_64_64 => {
                // R_X86_64_64: *target = symbol + addend
                if sym_idx != 0 {
                    let sym_name = get_symbol_name(dyn_info, sym_idx);
                    if !sym_name.is_null() {
                        let sym_addr = global_symbol_lookup_cstr(sym_name);
                        if sym_addr != 0 {
                            *target = (sym_addr as i64 + rela.r_addend) as u64;
                        }
                    }
                } else {
                    *target = (load_bias + rela.r_addend) as u64;
                }
            }
            R_X86_64_32 | R_X86_64_32S => {
                // R_X86_64_32/32S: 32-bit relocations
                if sym_idx != 0 {
                    let sym_name = get_symbol_name(dyn_info, sym_idx);
                    if !sym_name.is_null() {
                        let sym_addr = global_symbol_lookup_cstr(sym_name);
                        if sym_addr != 0 {
                            let val = (sym_addr as i64 + rela.r_addend) as u32;
                            *(target as *mut u32) = val;
                        }
                    }
                } else {
                    let val = (load_bias + rela.r_addend) as u32;
                    *(target as *mut u32) = val;
                }
            }
            R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                // R_X86_64_GLOB_DAT/JUMP_SLOT: *target = symbol address
                if sym_idx != 0 {
                    let sym_name = get_symbol_name(dyn_info, sym_idx);
                    if !sym_name.is_null() {
                        let sym_addr = global_symbol_lookup_cstr(sym_name);
                        if sym_addr != 0 {
                            *target = sym_addr;
                        }
                    }
                }
            }
            R_X86_64_COPY => {
                // R_X86_64_COPY: copy data from shared library
                // This is used for initialized data
                if sym_idx != 0 {
                    let sym_name = get_symbol_name(dyn_info, sym_idx);
                    if !sym_name.is_null() {
                        let sym_addr = global_symbol_lookup_cstr(sym_name);
                        if sym_addr != 0 {
                            // Get symbol size from symtab
                            let syment = if dyn_info.syment == 0 { 24 } else { dyn_info.syment };
                            let sym = &*((dyn_info.symtab + (sym_idx as u64) * syment) as *const Elf64Sym);
                            let size = sym.st_size as usize;
                            if size > 0 {
                                memcpy(target as *mut u8, sym_addr as *const u8, size);
                            }
                        }
                    }
                }
            }
            R_X86_64_IRELATIVE => {
                // R_X86_64_IRELATIVE: call resolver function to get symbol address
                // *target = resolver()(void)
                let resolver_addr = (load_bias + rela.r_addend) as u64;
                if resolver_addr != 0 {
                    let resolver: extern "C" fn() -> u64 = core::mem::transmute(resolver_addr);
                    *target = resolver();
                }
            }
            R_X86_64_DTPMOD64 => {
                // R_X86_64_DTPMOD64: TLS module ID
                // For static TLS (single module), this is typically 1
                if sym_idx != 0 {
                    // Get TLS module ID for the symbol's defining module
                    *target = dyn_info.tls_modid;
                } else {
                    // Local TLS: use current module's ID
                    *target = dyn_info.tls_modid;
                }
                if *target == 0 {
                    *target = 1; // Default TLS module ID for main executable
                }
            }
            R_X86_64_DTPOFF64 => {
                // R_X86_64_DTPOFF64: TLS offset within module
                if sym_idx != 0 {
                    let syment = if dyn_info.syment == 0 { 24 } else { dyn_info.syment };
                    let sym = &*((dyn_info.symtab + (sym_idx as u64) * syment) as *const Elf64Sym);
                    *target = (sym.st_value as i64 + rela.r_addend) as u64;
                } else {
                    *target = rela.r_addend as u64;
                }
            }
            R_X86_64_TPOFF64 => {
                // R_X86_64_TPOFF64: TLS offset from thread pointer
                // For x86_64 variant II TLS model, offset is negative from TP
                if sym_idx != 0 {
                    let syment = if dyn_info.syment == 0 { 24 } else { dyn_info.syment };
                    let sym = &*((dyn_info.symtab + (sym_idx as u64) * syment) as *const Elf64Sym);
                    // Static TLS offset - symbol value is offset within TLS block
                    *target = (sym.st_value as i64 + rela.r_addend) as u64;
                } else {
                    *target = rela.r_addend as u64;
                }
            }
            R_X86_64_TPOFF32 => {
                // R_X86_64_TPOFF32: 32-bit TLS offset from thread pointer
                if sym_idx != 0 {
                    let syment = if dyn_info.syment == 0 { 24 } else { dyn_info.syment };
                    let sym = &*((dyn_info.symtab + (sym_idx as u64) * syment) as *const Elf64Sym);
                    let val = (sym.st_value as i64 + rela.r_addend) as i32;
                    *(target as *mut i32) = val;
                } else {
                    *(target as *mut i32) = rela.r_addend as i32;
                }
            }
            R_X86_64_PC32 | R_X86_64_PLT32 => {
                // PC-relative 32-bit relocations
                if sym_idx != 0 {
                    let sym_name = get_symbol_name(dyn_info, sym_idx);
                    if !sym_name.is_null() {
                        let sym_addr = global_symbol_lookup_cstr(sym_name);
                        if sym_addr != 0 {
                            let pc = target as u64;
                            let val = ((sym_addr as i64 + rela.r_addend) - pc as i64) as i32;
                            *(target as *mut i32) = val;
                        }
                    }
                }
            }
            R_X86_64_GOTPCREL => {
                // GOT-relative PC-relative relocation
                // Usually handled by PLT, but we need to handle it for direct GOT access
                if sym_idx != 0 {
                    let sym_name = get_symbol_name(dyn_info, sym_idx);
                    if !sym_name.is_null() {
                        let sym_addr = global_symbol_lookup_cstr(sym_name);
                        if sym_addr != 0 {
                            // For GOTPCREL, we store the symbol address at target
                            // and the relocation computes the PC-relative offset to it
                            *target = sym_addr;
                        }
                    }
                }
            }
            R_X86_64_NONE => {}
            _ => {
                // Unknown relocation type - ignore
            }
        }
    }
}

/// Global symbol lookup using C string (null-terminated)
unsafe fn global_symbol_lookup_cstr(name: *const u8) -> u64 {
    // Find length
    let mut len = 0;
    let mut p = name;
    while *p != 0 && len < 256 {
        len += 1;
        p = p.add(1);
    }
    
    // Create slice including null terminator
    let name_slice = core::slice::from_raw_parts(name, len);
    global_symbol_lookup(name_slice)
}

/// Process RELA relocations (legacy - without full symbol lookup)
unsafe fn process_rela(rela_addr: u64, relasz: u64, relaent: u64, load_bias: i64) {
    let entry_size = if relaent == 0 { 24 } else { relaent };
    let count = relasz / entry_size;
    
    for i in 0..count {
        let rela = &*((rela_addr + i * entry_size) as *const Elf64Rela);
        let rel_type = (rela.r_info & 0xffffffff) as u32;
        let _sym_idx = (rela.r_info >> 32) as u32;
        
        let target = (rela.r_offset as i64 + load_bias) as *mut u64;
        
        match rel_type {
            R_X86_64_RELATIVE => {
                // R_X86_64_RELATIVE: *target = load_bias + addend
                *target = (load_bias + rela.r_addend) as u64;
            }
            R_X86_64_64 => {
                // R_X86_64_64: *target = symbol + addend
                // For now, just apply load bias (assumes local symbol)
                *target = (load_bias + rela.r_addend) as u64;
            }
            R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                // These need symbol resolution - handled by process_rela_with_symtab
            }
            R_X86_64_NONE => {}
            _ => {
                // Unknown relocation type - ignore for now
            }
        }
    }
}

/// Jump to the entry point of the main executable
/// For _start (assembly): reads argc/argv from [rsp]
/// For __nexa_crt_start (C function): receives stack_ptr in rdi
#[inline(never)]
unsafe fn jump_to_entry(entry: u64, stack_ptr: *const u64) -> ! {
    // CRITICAL: We must use explicit register assignments to prevent the compiler
    // from allocating entry/stack to registers that get cleared later.
    // - r14: entry point address (preserved across register clearing)
    // - r15: stack pointer (preserved across register clearing)
    asm!(
        // First, save our inputs to callee-saved registers before clearing anything
        "mov r14, {entry}",       // Save entry point to r14
        "mov r15, {stack}",       // Save stack pointer to r15
        // Now set up the stack
        "mov rsp, r15",           // Restore original stack pointer
        "xor rbp, rbp",           // Clear frame pointer
        // Clear general purpose registers (but not r14/r15 yet)
        "xor rax, rax",
        "xor rbx, rbx",
        "xor rcx, rcx",
        "xor rdx, rdx",
        "xor rsi, rsi",
        "mov rdi, rsp",           // Pass stack pointer as first argument (for C entry)
        "xor r8, r8",
        "xor r9, r9",
        "xor r10, r10",
        "xor r11, r11",
        "xor r12, r12",
        "xor r13, r13",
        // Jump to entry point (r14 still holds the address)
        "jmp r14",
        stack = in(reg) stack_ptr,
        entry = in(reg) entry,
        options(noreturn)
    );
}

// ============================================================================
// musl/glibc Compatibility Symbols
// ============================================================================

/// Global program name (musl compatibility)
#[no_mangle]
pub static mut __progname: *const u8 = b"unknown\0".as_ptr();

/// Full program name (musl compatibility)
#[no_mangle]
pub static mut __progname_full: *const u8 = b"unknown\0".as_ptr();

/// Program invocation name (glibc compatibility)
#[no_mangle]
pub static mut program_invocation_name: *mut u8 = core::ptr::null_mut();

/// Program invocation short name (glibc compatibility)
#[no_mangle]
pub static mut program_invocation_short_name: *mut u8 = core::ptr::null_mut();

/// DSO handle for atexit (musl/glibc compatibility)
/// Using usize instead of *mut u8 to avoid Sync issues
#[no_mangle]
pub static __dso_handle: usize = 0;

/// __libc_start_main - glibc/musl entry point
/// This is called by _start in glibc-compiled programs
/// 
/// Arguments:
/// - main: pointer to main function
/// - argc: argument count  
/// - argv: argument vector
/// - init: pointer to init function (may be NULL)
/// - fini: pointer to fini function (may be NULL)
/// - rtld_fini: pointer to rtld fini function (may be NULL)
/// - stack_end: end of stack
#[no_mangle]
pub unsafe extern "C" fn __libc_start_main(
    main: extern "C" fn(i32, *const *const u8, *const *const u8) -> i32,
    argc: i32,
    argv: *const *const u8,
    init: Option<extern "C" fn()>,
    fini: Option<extern "C" fn()>,
    rtld_fini: Option<extern "C" fn()>,
    stack_end: *mut u8,
) -> i32 {
    let _ = stack_end; // unused
    let _ = rtld_fini; // unused - we handle fini ourselves
    
    // Set up program name
    if argc > 0 && !argv.is_null() {
        let arg0 = *argv;
        if !arg0.is_null() {
            __progname_full = arg0;
            program_invocation_name = arg0 as *mut u8;
            
            // Find short name (after last '/')
            let mut short_name = arg0;
            let mut p = arg0;
            while *p != 0 {
                if *p == b'/' {
                    short_name = p.add(1);
                }
                p = p.add(1);
            }
            __progname = short_name;
            program_invocation_short_name = short_name as *mut u8;
        }
    }
    
    // Call init function if provided
    if let Some(init_fn) = init {
        init_fn();
    }
    
    // Calculate envp (after argv + NULL terminator)
    let envp = argv.add(argc as usize + 1);
    
    // Call main
    let ret = main(argc, argv, envp);
    
    // Call fini function if provided
    if let Some(fini_fn) = fini {
        fini_fn();
    }
    
    // Exit with return value
    exit(ret)
}

/// __libc_csu_init - called by __libc_start_main to run constructors
#[no_mangle]
pub unsafe extern "C" fn __libc_csu_init() {
    // Empty - init_array is handled by the dynamic linker
}

/// __libc_csu_fini - called to run destructors
#[no_mangle]
pub unsafe extern "C" fn __libc_csu_fini() {
    // Empty - fini_array is handled by the dynamic linker
}

/// __cxa_atexit - register a destructor function
/// For now, we just ignore these since we handle fini_array
#[no_mangle]
pub unsafe extern "C" fn __cxa_atexit(
    _func: Option<extern "C" fn(*mut u8)>,
    _arg: *mut u8,
    _dso_handle: *mut u8,
) -> i32 {
    0 // Success
}

/// __cxa_finalize - run registered destructors
#[no_mangle]
pub unsafe extern "C" fn __cxa_finalize(_dso_handle: *mut u8) {
    // Empty - destructor handling is simplified for now
}

/// atexit - register an exit handler
#[no_mangle]
pub unsafe extern "C" fn atexit(_func: Option<extern "C" fn()>) -> i32 {
    0 // Success - we don't track these for now
}

/// __stack_chk_guard - stack canary value
#[no_mangle]
pub static __stack_chk_guard: u64 = 0x00000aff0a0d0000;

/// __stack_chk_fail - called when stack smashing is detected
#[no_mangle]
pub unsafe extern "C" fn __stack_chk_fail() -> ! {
    print_str("[ld-nrlib] *** stack smashing detected ***\n");
    exit(127)
}

/// abort - abnormal program termination
#[no_mangle]
pub unsafe extern "C" fn abort() -> ! {
    print_str("[ld-nrlib] abort() called\n");
    exit(134) // 128 + SIGABRT (6)
}

/// environ - environment pointer (musl/glibc compatibility)
#[no_mangle]
pub static mut environ: *mut *mut u8 = core::ptr::null_mut();

/// __environ - environment pointer (glibc compatibility alias)
#[no_mangle]
pub static mut __environ: *mut *mut u8 = core::ptr::null_mut();

/// _environ - environment pointer (alternative alias)
#[no_mangle]
pub static mut _environ: *mut *mut u8 = core::ptr::null_mut();

/// __libc_single_threaded - indicate single-threaded mode
/// musl uses this to optimize locking
#[no_mangle]
pub static __libc_single_threaded: u8 = 1;

/// errno location - thread-local errno
/// For now, use a global since we're single-threaded
static mut ERRNO_VAL: i32 = 0;

#[no_mangle]
pub unsafe extern "C" fn __errno_location() -> *mut i32 {
    &mut ERRNO_VAL as *mut i32
}

/// ___errno - alternative errno accessor
#[no_mangle]
pub unsafe extern "C" fn ___errno() -> *mut i32 {
    &mut ERRNO_VAL as *mut i32
}

/// dl_iterate_phdr callback info structure
#[repr(C)]
pub struct DlPhdrInfo {
    pub dlpi_addr: u64,       // Base address of object
    pub dlpi_name: *const u8, // Null-terminated name
    pub dlpi_phdr: *const Elf64Phdr, // Pointer to program headers
    pub dlpi_phnum: u16,      // Number of program headers
    // Additional fields for newer versions
    pub dlpi_adds: u64,       // Number of loads
    pub dlpi_subs: u64,       // Number of unloads
    pub dlpi_tls_modid: usize, // TLS module ID
    pub dlpi_tls_data: *mut u8, // TLS data address
}

/// dl_iterate_phdr - iterate over loaded shared objects
/// This is used by exception handling, stack unwinding, and debugging
#[no_mangle]
pub unsafe extern "C" fn dl_iterate_phdr(
    callback: extern "C" fn(*mut DlPhdrInfo, usize, *mut u8) -> i32,
    data: *mut u8,
) -> i32 {
    // Iterate over all loaded libraries
    for i in 0..GLOBAL_SYMTAB.lib_count {
        let lib = &GLOBAL_SYMTAB.libs[i];
        if !lib.valid {
            continue;
        }
        
        // We don't store PHDR info, so we skip this for now
        // A full implementation would store and provide PHDR info
        let mut info = DlPhdrInfo {
            dlpi_addr: lib.base_addr,
            dlpi_name: if i == 0 { b"\0".as_ptr() } else { b"lib\0".as_ptr() },
            dlpi_phdr: core::ptr::null(),
            dlpi_phnum: 0,
            dlpi_adds: GLOBAL_SYMTAB.lib_count as u64,
            dlpi_subs: 0,
            dlpi_tls_modid: lib.dyn_info.tls_modid as usize,
            dlpi_tls_data: core::ptr::null_mut(),
        };
        
        let ret = callback(&mut info, core::mem::size_of::<DlPhdrInfo>(), data);
        if ret != 0 {
            return ret;
        }
    }
    0
}

/// dlsym - look up symbol by name (simplified)
#[no_mangle]
pub unsafe extern "C" fn dlsym(_handle: *mut u8, name: *const u8) -> *mut u8 {
    if name.is_null() {
        return core::ptr::null_mut();
    }
    
    // Convert name to slice
    let name_len = cstr_len(name);
    let name_slice = core::slice::from_raw_parts(name, name_len);
    
    // Look up in global symbol table
    let addr = global_symbol_lookup(name_slice);
    addr as *mut u8
}

/// dlopen - open a shared library (stub)
#[no_mangle]
pub unsafe extern "C" fn dlopen(_filename: *const u8, _flags: i32) -> *mut u8 {
    // Stub - return pseudo-handle for RTLD_DEFAULT behavior
    core::ptr::null_mut()
}

/// dlclose - close a shared library (stub)
#[no_mangle]
pub unsafe extern "C" fn dlclose(_handle: *mut u8) -> i32 {
    0 // Success
}

/// dlerror - get last error message (stub)
static mut DLERROR_MSG: [u8; 64] = [0; 64];

#[no_mangle]
pub unsafe extern "C" fn dlerror() -> *const u8 {
    core::ptr::null() // No error
}

/// __libc_current_sigrtmin - get minimum real-time signal number
#[no_mangle]
pub unsafe extern "C" fn __libc_current_sigrtmin() -> i32 {
    34 // SIGRTMIN on Linux
}

/// __libc_current_sigrtmax - get maximum real-time signal number
#[no_mangle]
pub unsafe extern "C" fn __libc_current_sigrtmax() -> i32 {
    64 // SIGRTMAX on Linux
}

/// __register_atfork - register fork handlers (stub)
#[no_mangle]
pub unsafe extern "C" fn __register_atfork(
    _prepare: Option<extern "C" fn()>,
    _parent: Option<extern "C" fn()>,
    _child: Option<extern "C" fn()>,
    _dso_handle: *mut u8,
) -> i32 {
    0 // Success
}

/// pthread_atfork - register fork handlers (stub, alias)
#[no_mangle]
pub unsafe extern "C" fn pthread_atfork(
    _prepare: Option<extern "C" fn()>,
    _parent: Option<extern "C" fn()>,
    _child: Option<extern "C" fn()>,
) -> i32 {
    0 // Success
}

/// _exit - immediate program termination
#[no_mangle]
pub unsafe extern "C" fn _exit(status: i32) -> ! {
    exit(status)
}

/// _Exit - immediate program termination (C99)
#[no_mangle]
pub unsafe extern "C" fn _Exit(status: i32) -> ! {
    exit(status)
}

// ============================================================================
// Panic Handler
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        print_str("[ld-nrlib] PANIC!\n");
        exit(127);
    }
}
