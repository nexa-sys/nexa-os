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
const DT_GNU_HASH: i64 = 0x6ffffef5;

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

// ============================================================================
// Dynamic Linker State
// ============================================================================

/// Maximum number of loaded libraries
const MAX_LIBS: usize = 16;

/// Library search paths
const LIB_SEARCH_PATHS: &[&[u8]] = &[
    b"/lib64\0",
    b"/lib\0",
    b"/usr/lib64\0",
    b"/usr/lib\0",
];

/// Information parsed from auxiliary vector
struct AuxInfo {
    at_phdr: u64,     // Program headers address
    at_phent: u64,    // Program header entry size
    at_phnum: u64,    // Number of program headers
    at_pagesz: u64,   // Page size
    at_base: u64,     // Interpreter base address
    at_entry: u64,    // Main executable entry point
    at_execfn: u64,   // Executable filename
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
    jmprel: u64,
    pltrelsz: u64,
    init: u64,
    fini: u64,
    init_array: u64,
    init_arraysz: u64,
    fini_array: u64,
    fini_arraysz: u64,
    hash: u64,
    gnu_hash: u64,
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
            jmprel: 0,
            pltrelsz: 0,
            init: 0,
            fini: 0,
            init_array: 0,
            init_arraysz: 0,
            fini_array: 0,
            fini_arraysz: 0,
            hash: 0,
            gnu_hash: 0,
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
/// NexaOS sys_open takes (path_ptr, length) not (path, flags, mode)
unsafe fn open_file(path: *const u8) -> i64 {
    // Calculate path length (null-terminated string)
    let mut len = 0;
    while *path.add(len) != 0 {
        len += 1;
        if len > 4096 {
            break;
        }
    }
    syscall3(SYS_OPEN, path as u64, len as u64, 0) as i64
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
    print_str("[ld-nrlib] Loading shared library: ");
    // Print path
    let mut p = path;
    while *p != 0 {
        let buf = [*p];
        print(&buf);
        p = p.add(1);
    }
    print_str("\n");
    
    let fd = open_file(path);
    if fd < 0 {
        print_str("[ld-nrlib] Failed to open library\n");
        return (0, 0, DynInfo::new());
    }
    
    // Read ELF header
    let mut ehdr_buf = [0u8; 64];
    let bytes_read = read_bytes(fd as i32, ehdr_buf.as_mut_ptr(), 64);
    if bytes_read < 64 {
        print_str("[ld-nrlib] Failed to read ELF header\n");
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    let ehdr = &*(ehdr_buf.as_ptr() as *const Elf64Ehdr);
    
    // Validate ELF magic
    if ehdr.e_ident[0] != 0x7f || ehdr.e_ident[1] != b'E' || 
       ehdr.e_ident[2] != b'L' || ehdr.e_ident[3] != b'F' {
        print_str("[ld-nrlib] Invalid ELF magic\n");
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    // Must be shared object (ET_DYN = 3)
    if ehdr.e_type != 3 {
        print_str("[ld-nrlib] Not a shared object\n");
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    // Read program headers
    let phdr_size = (ehdr.e_phentsize as usize) * (ehdr.e_phnum as usize);
    if phdr_size > 2048 {
        print_str("[ld-nrlib] Program headers too large\n");
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    lseek(fd as i32, ehdr.e_phoff as i64, 0); // SEEK_SET
    let mut phdr_buf = [0u8; 2048];
    let bytes_read = read_bytes(fd as i32, phdr_buf.as_mut_ptr(), phdr_size);
    if bytes_read < phdr_size as isize {
        print_str("[ld-nrlib] Failed to read program headers\n");
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    let phdrs = core::slice::from_raw_parts(
        phdr_buf.as_ptr() as *const Elf64Phdr,
        ehdr.e_phnum as usize,
    );
    
    // Find extent of loadable segments
    let mut load_addr_min: u64 = u64::MAX;
    let mut load_addr_max: u64 = 0;
    let mut dyn_vaddr: u64 = 0;
    
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
    }
    
    if load_addr_min == u64::MAX {
        print_str("[ld-nrlib] No loadable segments\n");
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    let total_size = load_addr_max - load_addr_min;
    
    print_str("[ld-nrlib] Allocating ");
    print_hex(total_size);
    print_str(" bytes for library\n");
    
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
    
    print_str("[ld-nrlib] mmap returned: ");
    print_hex(base_addr);
    print_str("\n");
    
    // Check for mmap failure - MAP_FAILED is u64::MAX (0xFFFF_FFFF_FFFF_FFFF)
    // But also handle if kernel returns a small negative errno value cast to u64
    // Values above 0xFFFF_FFFF_FFFF_F000 are likely error codes
    if base_addr >= 0xFFFF_FFFF_FFFF_F000 {
        print_str("[ld-nrlib] mmap failed (error code)\n");
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    if base_addr == 0 {
        print_str("[ld-nrlib] mmap returned NULL\n");
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }
    
    print_str("[ld-nrlib] Library base: ");
    print_hex(base_addr);
    print_str("\n");
    
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
    
    print_str("[ld-nrlib] Library loaded successfully\n");
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
            DT_JMPREL => dyn_info.jmprel = (entry.d_val as i64 + load_bias) as u64,
            DT_PLTRELSZ => dyn_info.pltrelsz = entry.d_val,
            DT_INIT => dyn_info.init = (entry.d_val as i64 + load_bias) as u64,
            DT_FINI => dyn_info.fini = (entry.d_val as i64 + load_bias) as u64,
            DT_INIT_ARRAY => dyn_info.init_array = (entry.d_val as i64 + load_bias) as u64,
            DT_INIT_ARRAYSZ => dyn_info.init_arraysz = entry.d_val,
            DT_FINI_ARRAY => dyn_info.fini_array = (entry.d_val as i64 + load_bias) as u64,
            DT_FINI_ARRAYSZ => dyn_info.fini_arraysz = entry.d_val,
            DT_HASH => dyn_info.hash = (entry.d_val as i64 + load_bias) as u64,
            DT_GNU_HASH => dyn_info.gnu_hash = (entry.d_val as i64 + load_bias) as u64,
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
    
    for search_path in LIB_SEARCH_PATHS {
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

/// Lookup a symbol by name in a single library
/// Returns symbol value (with load_bias applied) or 0 if not found
unsafe fn lookup_symbol_in_lib(lib: &LoadedLib, name: &[u8]) -> u64 {
    if !lib.valid {
        return 0;
    }
    
    let dyn_info = &lib.dyn_info;
    if dyn_info.symtab == 0 || dyn_info.strtab == 0 {
        return 0;
    }
    
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
        
        // Compare names
        let mut j = 0;
        let mut match_found = true;
        while j < name.len() {
            let c = *sym_name_ptr.add(j);
            let target = name[j];
            if target == 0 {
                // Check if sym_name also ends here
                if c != 0 {
                    match_found = false;
                }
                break;
            }
            if c != target {
                match_found = false;
                break;
            }
            j += 1;
        }
        
        // Also check that symbol name ends at j
        if match_found {
            let c = *sym_name_ptr.add(j);
            if c != 0 && j < name.len() && name[j] != 0 {
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
    print_str("[ld-nrlib] Dynamic linker starting...\n");

    // Debug: print stack pointer
    print_str("[ld-nrlib] stack_ptr=");
    print_hex(stack_ptr as u64);
    print_str("\n");

    // Parse the stack to get argc, argv, envp, auxv
    let argc = *stack_ptr as usize;
    
    // Debug: print argc
    print_str("[ld-nrlib] argc=");
    print_hex(argc as u64);
    print_str("\n");
    
    let argv = stack_ptr.add(1) as *const *const u8;
    
    // Skip past argv (argc+1 entries including NULL terminator)
    let mut ptr = argv.add(argc + 1) as *const *const u8;
    
    // Debug: print ptr after argv
    print_str("[ld-nrlib] ptr after argv=");
    print_hex(ptr as u64);
    print_str("\n");
    
    // Skip past envp (until NULL)
    while !(*ptr).is_null() {
        ptr = ptr.add(1);
    }
    ptr = ptr.add(1); // Skip NULL terminator
    
    // Debug: print auxv pointer
    print_str("[ld-nrlib] auxv ptr=");
    print_hex(ptr as u64);
    print_str("\n");
    
    // Now ptr points to auxv
    let auxv = ptr as *const AuxEntry;
    
    // Debug: dump entire auxv array
    print_str("[ld-nrlib] === DUMPING AUXV ===\n");
    let mut dump_ptr = auxv;
    for i in 0..20 {
        let entry = *dump_ptr;
        print_str("[ld-nrlib] auxv[");
        print_hex(i as u64);
        print_str("].type=");
        print_hex(entry.a_type);
        print_str(" val=");
        print_hex(entry.a_val);
        print_str("\n");
        if entry.a_type == AT_NULL {
            break;
        }
        dump_ptr = dump_ptr.add(1);
    }
    print_str("[ld-nrlib] === END AUXV DUMP ===\n");
    
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
            _ => {}
        }
        aux_ptr = aux_ptr.add(1);
    }

    print_str("[ld-nrlib] AT_PHDR:  ");
    print_hex(aux_info.at_phdr);
    print_str("\n");
    print_str("[ld-nrlib] AT_PHNUM: ");
    print_hex(aux_info.at_phnum);
    print_str("\n");
    print_str("[ld-nrlib] AT_ENTRY: ");
    print_hex(aux_info.at_entry);
    print_str("\n");
    print_str("[ld-nrlib] AT_BASE:  ");
    print_hex(aux_info.at_base);
    print_str("\n");

    // Find the dynamic section of the main executable
    if aux_info.at_phdr == 0 {
        print_str("[ld-nrlib] ERROR: AT_PHDR is 0! Kernel did not pass program headers.\n");
        exit(127);
    }
    if aux_info.at_phnum == 0 {
        print_str("[ld-nrlib] ERROR: AT_PHNUM is 0!\n");
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
    
    print_str("[ld-nrlib] Load bias: ");
    print_hex(load_bias as u64);
    print_str("\n");

    // Parse main executable's dynamic section first
    let mut main_dyn_info = DynInfo::new();
    if dyn_addr != 0 {
        dyn_addr = (dyn_addr as i64 + load_bias) as u64;
        print_str("[ld-nrlib] PT_DYNAMIC at: ");
        print_hex(dyn_addr);
        print_str("\n");
        
        // Parse dynamic section (including DT_NEEDED)
        parse_dynamic_section(dyn_addr, load_bias, &mut main_dyn_info);
        
        // Store main executable info in global symbol table (index 0)
        let main_lib = &mut GLOBAL_SYMTAB.libs[0];
        main_lib.base_addr = (first_load_vaddr as i64 + load_bias) as u64;
        main_lib.load_bias = load_bias;
        main_lib.dyn_info = main_dyn_info;
        main_lib.valid = true;
        GLOBAL_SYMTAB.lib_count = 1;
        
        print_str("[ld-nrlib] Main executable registered in symbol table\n");
        
        // ================================================================
        // Step 1: Load libnrlib.so first (always needed)
        // ================================================================
        print_str("[ld-nrlib] === Loading libnrlib.so ===\n");
        
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
                
                print_str("[ld-nrlib] libnrlib.so registered (index ");
                print_hex(lib_idx as u64);
                print_str(")\n");
                
                // Process libnrlib.so's RELATIVE relocations first
                if lib_dyn_info.rela != 0 && lib_dyn_info.relasz > 0 {
                    print_str("[ld-nrlib] Processing libnrlib.so RELA relocations...\n");
                    process_rela(lib_dyn_info.rela, lib_dyn_info.relasz, lib_dyn_info.relaent, lib_bias);
                }
                
                // Call libnrlib.so init functions
                if lib_dyn_info.init != 0 {
                    print_str("[ld-nrlib] Calling libnrlib.so DT_INIT\n");
                    let init_fn: extern "C" fn() = core::mem::transmute(lib_dyn_info.init);
                    init_fn();
                }
                
                if lib_dyn_info.init_array != 0 && lib_dyn_info.init_arraysz > 0 {
                    print_str("[ld-nrlib] Calling libnrlib.so DT_INIT_ARRAY...\n");
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
        } else {
            print_str("[ld-nrlib] Warning: Could not load libnrlib.so\n");
        }
        
        // ================================================================
        // Step 2: Load other DT_NEEDED libraries
        // ================================================================
        if main_dyn_info.needed_count > 0 {
            print_str("[ld-nrlib] === Loading DT_NEEDED libraries ===\n");
            
            for i in 0..main_dyn_info.needed_count {
                let name_offset = main_dyn_info.needed[i];
                let name_ptr = (main_dyn_info.strtab + name_offset) as *const u8;
                
                print_str("[ld-nrlib] DT_NEEDED: ");
                let mut p = name_ptr;
                while *p != 0 {
                    let buf = [*p];
                    print(&buf);
                    p = p.add(1);
                }
                print_str("\n");
                
                // Skip if this is libnrlib.so (already loaded)
                if is_same_library_name(name_ptr, b"libnrlib.so\0".as_ptr()) ||
                   is_same_library_name(name_ptr, b"libc.so\0".as_ptr()) ||
                   is_same_library_name(name_ptr, b"libc.so.6\0".as_ptr()) {
                    print_str("[ld-nrlib] (already loaded as libnrlib.so)\n");
                    continue;
                }
                
                // Try to find and load the library
                let name_len = cstr_len(name_ptr);
                let name_slice = core::slice::from_raw_parts(name_ptr, name_len);
                
                if let Some(path) = search_library(name_slice) {
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
                } else {
                    print_str("[ld-nrlib] Warning: library not found\n");
                }
            }
        }
        
        // ================================================================
        // Step 3: Now process main executable's relocations with symbol lookup
        // ================================================================
        print_str("[ld-nrlib] === Processing main executable relocations ===\n");
        print_str("[ld-nrlib] Total libraries in symbol table: ");
        print_hex(GLOBAL_SYMTAB.lib_count as u64);
        print_str("\n");
        
        // Process RELA relocations with full symbol lookup
        if main_dyn_info.rela != 0 && main_dyn_info.relasz > 0 {
            print_str("[ld-nrlib] Processing main RELA relocations...\n");
            process_rela_with_symtab(main_dyn_info.rela, main_dyn_info.relasz, main_dyn_info.relaent, load_bias, &main_dyn_info);
        }
        
        // Process PLT/JMPREL relocations with full symbol lookup
        if main_dyn_info.jmprel != 0 && main_dyn_info.pltrelsz > 0 {
            print_str("[ld-nrlib] Processing main PLT relocations...\n");
            process_rela_with_symtab(main_dyn_info.jmprel, main_dyn_info.pltrelsz, 24, load_bias, &main_dyn_info);
        }
        
        // ================================================================
        // Step 4: Process library relocations that need main executable symbols
        // ================================================================
        print_str("[ld-nrlib] === Processing library GLOB_DAT/JUMP_SLOT relocations ===\n");
        for i in 1..GLOBAL_SYMTAB.lib_count {
            let lib = &GLOBAL_SYMTAB.libs[i];
            if !lib.valid {
                continue;
            }
            
            if lib.dyn_info.jmprel != 0 && lib.dyn_info.pltrelsz > 0 {
                print_str("[ld-nrlib] Processing library ");
                print_hex(i as u64);
                print_str(" PLT relocations...\n");
                process_rela_with_symtab(lib.dyn_info.jmprel, lib.dyn_info.pltrelsz, 24, lib.load_bias, &lib.dyn_info);
            }
        }

        // ================================================================
        // Step 5: Call init functions for main executable
        // ================================================================
        if main_dyn_info.init != 0 {
            print_str("[ld-nrlib] Calling DT_INIT at ");
            print_hex(main_dyn_info.init);
            print_str("\n");
            let init_fn: extern "C" fn() = core::mem::transmute(main_dyn_info.init);
            init_fn();
        }

        if main_dyn_info.init_array != 0 && main_dyn_info.init_arraysz > 0 {
            print_str("[ld-nrlib] Calling DT_INIT_ARRAY...\n");
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
        print_str("[ld-nrlib] AT_ENTRY is 0, reading e_entry from ELF header...\n");
        
        // The ELF header is at the start of the first PT_LOAD segment
        // For PIE, first_load_vaddr is typically 0x400000 or similar
        // The actual loaded address is first_load_vaddr + load_bias
        let elf_header_addr = (first_load_vaddr as i64 + load_bias) as u64;
        
        // Read e_entry from offset 24 in ELF header
        let e_entry = *((elf_header_addr + 24) as *const u64);
        
        print_str("[ld-nrlib] ELF e_entry: ");
        print_hex(e_entry);
        print_str("\n");
        
        if e_entry != 0 {
            // Apply load_bias to get actual entry point
            entry = (e_entry as i64 + load_bias) as u64;
        } else {
            // e_entry is also 0, need to find entry point via symbol lookup
            // Standard order: _start -> __libc_start_main -> main
            print_str("[ld-nrlib] e_entry is 0, looking for entry symbols...\n");
            
            if dyn_addr != 0 {
                // Try _start first (standard C runtime entry point)
                let start_addr = find_symbol_by_name(dyn_addr, load_bias, b"_start\0");
                if start_addr != 0 {
                    print_str("[ld-nrlib] Found _start at: ");
                    print_hex(start_addr);
                    print_str("\n");
                    entry = start_addr;
                } else {
                    // Try __nexa_crt_start (NexaOS nrlib entry point)
                    let crt_addr = find_symbol_by_name(dyn_addr, load_bias, b"__nexa_crt_start\0");
                    if crt_addr != 0 {
                        print_str("[ld-nrlib] Found __nexa_crt_start at: ");
                        print_hex(crt_addr);
                        print_str("\n");
                        entry = crt_addr;
                    } else {
                        // Last resort: look for main and call it directly
                        // This works for simple C programs but NOT for Rust std programs
                        let main_addr = find_symbol_by_name(dyn_addr, load_bias, b"main\0");
                        if main_addr != 0 {
                            print_str("[ld-nrlib] Found main at: ");
                            print_hex(main_addr);
                            print_str("\n");
                            // Call main directly with C calling convention
                            call_main_directly(main_addr, stack_ptr);
                        }
                    }
                }
            }
        }
        
        print_str("[ld-nrlib] Calculated entry: ");
        print_hex(entry);
        print_str("\n");
    }
    
    if entry == 0 {
        print_str("[ld-nrlib] ERROR: No entry point\n");
        exit(127);
    }

    print_str("[ld-nrlib] Jumping to entry point: ");
    print_hex(entry);
    print_str("\n");

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
    
    print_str("[ld-nrlib] Calling main(");
    print_hex(argc as u64);
    print_str(", ");
    print_hex(argv as u64);
    print_str(")\n");
    
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
            R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                // R_X86_64_GLOB_DAT/JUMP_SLOT: *target = symbol address
                if sym_idx != 0 {
                    let sym_name = get_symbol_name(dyn_info, sym_idx);
                    if !sym_name.is_null() {
                        let sym_addr = global_symbol_lookup_cstr(sym_name);
                        if sym_addr != 0 {
                            *target = sym_addr;
                        } else {
                            // Symbol not found - print warning
                            print_str("[ld-nrlib] Warning: unresolved symbol: ");
                            let mut p = sym_name;
                            while *p != 0 {
                                let buf = [*p];
                                print(&buf);
                                p = p.add(1);
                            }
                            print_str("\n");
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
            R_X86_64_NONE => {}
            _ => {
                // Unknown relocation type - print debug info
                print_str("[ld-nrlib] Unknown reloc type: ");
                print_hex(rel_type as u64);
                print_str("\n");
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
#[inline(never)]
unsafe fn jump_to_entry(entry: u64, stack_ptr: *const u64) -> ! {
    asm!(
        "mov rsp, {stack}",       // Restore original stack pointer
        "xor rbp, rbp",           // Clear frame pointer
        "xor rax, rax",           // Clear registers
        "xor rbx, rbx",
        "xor rcx, rcx",
        "xor rdx, rdx",
        "xor rsi, rsi",
        "xor rdi, rdi",
        "xor r8, r8",
        "xor r9, r9",
        "xor r10, r10",
        "xor r11, r11",
        "xor r12, r12",
        "xor r13, r13",
        "xor r14, r14",
        "xor r15, r15",
        "jmp {entry}",            // Jump to entry point
        stack = in(reg) stack_ptr,
        entry = in(reg) entry,
        options(noreturn)
    );
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
