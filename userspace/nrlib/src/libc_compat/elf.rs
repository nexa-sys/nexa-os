//! ELF format definitions and dynamic segment parsing
//!
//! This module provides ELF64 structure definitions and parsing utilities
//! needed for dynamic linking support in userspace.

// ============================================================================
// ELF Constants
// ============================================================================

/// ELF magic number: 0x7F 'E' 'L' 'F'
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// ELF class constants
pub const ELFCLASS64: u8 = 2;

/// ELF data encoding
pub const ELFDATA2LSB: u8 = 1; // Little endian

/// ELF machine type
pub const EM_X86_64: u16 = 62;

// ============================================================================
// ELF Type Constants
// ============================================================================

pub const ET_NONE: u16 = 0;
pub const ET_REL: u16 = 1;
pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;
pub const ET_CORE: u16 = 4;

// ============================================================================
// Program Header Type Constants
// ============================================================================

pub const PT_NULL: u32 = 0;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_SHLIB: u32 = 5;
pub const PT_PHDR: u32 = 6;
pub const PT_TLS: u32 = 7;
pub const PT_GNU_EH_FRAME: u32 = 0x6474e550;
pub const PT_GNU_STACK: u32 = 0x6474e551;
pub const PT_GNU_RELRO: u32 = 0x6474e552;

// ============================================================================
// Program Header Flags
// ============================================================================

pub const PF_X: u32 = 0x1; // Execute
pub const PF_W: u32 = 0x2; // Write
pub const PF_R: u32 = 0x4; // Read

// ============================================================================
// Section Header Type Constants
// ============================================================================

pub const SHT_NULL: u32 = 0;
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_HASH: u32 = 5;
pub const SHT_DYNAMIC: u32 = 6;
pub const SHT_NOTE: u32 = 7;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_REL: u32 = 9;
pub const SHT_DYNSYM: u32 = 11;
pub const SHT_GNU_HASH: u32 = 0x6ffffff6;

// ============================================================================
// Dynamic Section Tag Constants
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
pub const DT_GNU_HASH: i64 = 0x6ffffef5;
pub const DT_VERSYM: i64 = 0x6ffffff0;
pub const DT_RELACOUNT: i64 = 0x6ffffff9;
pub const DT_RELCOUNT: i64 = 0x6ffffffa;
pub const DT_FLAGS_1: i64 = 0x6ffffffb;
pub const DT_VERNEED: i64 = 0x6ffffffe;
pub const DT_VERNEEDNUM: i64 = 0x6fffffff;

// DT_FLAGS values
pub const DF_ORIGIN: u64 = 0x0001;
pub const DF_SYMBOLIC: u64 = 0x0002;
pub const DF_TEXTREL: u64 = 0x0004;
pub const DF_BIND_NOW: u64 = 0x0008;
pub const DF_STATIC_TLS: u64 = 0x0010;

// DT_FLAGS_1 values
pub const DF_1_NOW: u64 = 0x00000001;
pub const DF_1_GLOBAL: u64 = 0x00000002;
pub const DF_1_GROUP: u64 = 0x00000004;
pub const DF_1_NODELETE: u64 = 0x00000008;
pub const DF_1_LOADFLTR: u64 = 0x00000010;
pub const DF_1_INITFIRST: u64 = 0x00000020;
pub const DF_1_NOOPEN: u64 = 0x00000040;
pub const DF_1_ORIGIN: u64 = 0x00000080;
pub const DF_1_DIRECT: u64 = 0x00000100;
pub const DF_1_TRANS: u64 = 0x00000200;
pub const DF_1_INTERPOSE: u64 = 0x00000400;
pub const DF_1_NODEFLIB: u64 = 0x00000800;
pub const DF_1_NODUMP: u64 = 0x00001000;
pub const DF_1_CONFALT: u64 = 0x00002000;
pub const DF_1_ENDFILTEE: u64 = 0x00004000;
pub const DF_1_DISPRELDNE: u64 = 0x00008000;
pub const DF_1_DISPRELPND: u64 = 0x00010000;
pub const DF_1_PIE: u64 = 0x08000000;

// ============================================================================
// Relocation Type Constants (x86_64)
// ============================================================================

pub const R_X86_64_NONE: u32 = 0;
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_PC32: u32 = 2;
pub const R_X86_64_GOT32: u32 = 3;
pub const R_X86_64_PLT32: u32 = 4;
pub const R_X86_64_COPY: u32 = 5;
pub const R_X86_64_GLOB_DAT: u32 = 6;
pub const R_X86_64_JUMP_SLOT: u32 = 7;
pub const R_X86_64_RELATIVE: u32 = 8;
pub const R_X86_64_GOTPCREL: u32 = 9;
pub const R_X86_64_32: u32 = 10;
pub const R_X86_64_32S: u32 = 11;
pub const R_X86_64_16: u32 = 12;
pub const R_X86_64_PC16: u32 = 13;
pub const R_X86_64_8: u32 = 14;
pub const R_X86_64_PC8: u32 = 15;
pub const R_X86_64_DTPMOD64: u32 = 16;
pub const R_X86_64_DTPOFF64: u32 = 17;
pub const R_X86_64_TPOFF64: u32 = 18;
pub const R_X86_64_TLSGD: u32 = 19;
pub const R_X86_64_TLSLD: u32 = 20;
pub const R_X86_64_DTPOFF32: u32 = 21;
pub const R_X86_64_GOTTPOFF: u32 = 22;
pub const R_X86_64_TPOFF32: u32 = 23;
pub const R_X86_64_IRELATIVE: u32 = 37;

// ============================================================================
// Symbol Binding Constants
// ============================================================================

pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;

// ============================================================================
// Symbol Type Constants
// ============================================================================

pub const STT_NOTYPE: u8 = 0;
pub const STT_OBJECT: u8 = 1;
pub const STT_FUNC: u8 = 2;
pub const STT_SECTION: u8 = 3;
pub const STT_FILE: u8 = 4;
pub const STT_COMMON: u8 = 5;
pub const STT_TLS: u8 = 6;
pub const STT_GNU_IFUNC: u8 = 10;

// ============================================================================
// Symbol Visibility Constants
// ============================================================================

pub const STV_DEFAULT: u8 = 0;
pub const STV_INTERNAL: u8 = 1;
pub const STV_HIDDEN: u8 = 2;
pub const STV_PROTECTED: u8 = 3;

// ============================================================================
// Special Section Indices
// ============================================================================

pub const SHN_UNDEF: u16 = 0;
pub const SHN_ABS: u16 = 0xfff1;
pub const SHN_COMMON: u16 = 0xfff2;

// ============================================================================
// ELF64 Header Structure
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Ehdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

impl Elf64Ehdr {
    /// Check if this is a valid ELF64 header
    pub fn is_valid(&self) -> bool {
        self.e_ident[0..4] == ELF_MAGIC
            && self.e_ident[4] == ELFCLASS64
            && self.e_ident[5] == ELFDATA2LSB
            && self.e_machine == EM_X86_64
    }

    /// Check if this is a shared object or PIE
    pub fn is_shared(&self) -> bool {
        self.e_type == ET_DYN
    }

    /// Check if this is an executable
    pub fn is_executable(&self) -> bool {
        self.e_type == ET_EXEC
    }
}

// ============================================================================
// ELF64 Program Header Structure
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

impl Elf64Phdr {
    /// Check if this is a PT_DYNAMIC segment
    pub fn is_dynamic(&self) -> bool {
        self.p_type == PT_DYNAMIC
    }

    /// Check if this is a PT_LOAD segment
    pub fn is_load(&self) -> bool {
        self.p_type == PT_LOAD
    }

    /// Check if this segment is readable
    pub fn is_readable(&self) -> bool {
        (self.p_flags & PF_R) != 0
    }

    /// Check if this segment is writable
    pub fn is_writable(&self) -> bool {
        (self.p_flags & PF_W) != 0
    }

    /// Check if this segment is executable
    pub fn is_executable(&self) -> bool {
        (self.p_flags & PF_X) != 0
    }
}

// ============================================================================
// ELF64 Section Header Structure
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Shdr {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u64,
    pub sh_entsize: u64,
}

// ============================================================================
// ELF64 Dynamic Entry Structure
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Dyn {
    pub d_tag: i64,
    pub d_val: u64, // Can be d_val or d_ptr (union in C)
}

impl Elf64Dyn {
    /// Get the value/pointer as an address
    pub fn d_ptr(&self) -> u64 {
        self.d_val
    }
}

// ============================================================================
// ELF64 Symbol Structure
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Sym {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

impl Elf64Sym {
    /// Get symbol binding (STB_LOCAL, STB_GLOBAL, STB_WEAK)
    pub fn binding(&self) -> u8 {
        self.st_info >> 4
    }

    /// Get symbol type (STT_NOTYPE, STT_FUNC, etc.)
    pub fn symbol_type(&self) -> u8 {
        self.st_info & 0xf
    }

    /// Get symbol visibility
    pub fn visibility(&self) -> u8 {
        self.st_other & 0x3
    }

    /// Check if symbol is defined
    pub fn is_defined(&self) -> bool {
        self.st_shndx != SHN_UNDEF
    }

    /// Check if symbol is weak
    pub fn is_weak(&self) -> bool {
        self.binding() == STB_WEAK
    }

    /// Check if symbol is global
    pub fn is_global(&self) -> bool {
        self.binding() == STB_GLOBAL
    }

    /// Check if symbol is a function
    pub fn is_function(&self) -> bool {
        self.symbol_type() == STT_FUNC
    }

    /// Check if symbol is an object (data)
    pub fn is_object(&self) -> bool {
        self.symbol_type() == STT_OBJECT
    }
}

// ============================================================================
// ELF64 Relocation Structures
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Rel {
    pub r_offset: u64,
    pub r_info: u64,
}

impl Elf64Rel {
    /// Get symbol index from r_info
    pub fn symbol(&self) -> u32 {
        (self.r_info >> 32) as u32
    }

    /// Get relocation type from r_info
    pub fn reloc_type(&self) -> u32 {
        (self.r_info & 0xffffffff) as u32
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}

impl Elf64Rela {
    /// Get symbol index from r_info
    pub fn symbol(&self) -> u32 {
        (self.r_info >> 32) as u32
    }

    /// Get relocation type from r_info
    pub fn reloc_type(&self) -> u32 {
        (self.r_info & 0xffffffff) as u32
    }
}

// ============================================================================
// GNU Hash Table Structure
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GnuHashHeader {
    pub nbuckets: u32,
    pub symoffset: u32,
    pub bloom_size: u32,
    pub bloom_shift: u32,
}

// ============================================================================
// ELF Hash Table Structure
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ElfHashHeader {
    pub nbucket: u32,
    pub nchain: u32,
}

// ============================================================================
// Dynamic Section Info (parsed from PT_DYNAMIC)
// ============================================================================

/// Parsed information from the dynamic section
#[derive(Debug, Clone, Copy, Default)]
pub struct DynamicInfo {
    /// Address of string table (.dynstr)
    pub strtab: u64,
    /// Size of string table
    pub strsz: u64,
    /// Address of symbol table (.dynsym)
    pub symtab: u64,
    /// Size of each symbol entry
    pub syment: u64,
    /// Address of RELA relocations
    pub rela: u64,
    /// Total size of RELA relocations
    pub relasz: u64,
    /// Size of each RELA entry
    pub relaent: u64,
    /// Address of REL relocations
    pub rel: u64,
    /// Total size of REL relocations
    pub relsz: u64,
    /// Size of each REL entry
    pub relent: u64,
    /// Address of PLT relocations (JMPREL)
    pub jmprel: u64,
    /// Size of PLT relocations
    pub pltrelsz: u64,
    /// Type of PLT relocations (DT_REL or DT_RELA)
    pub pltrel: u64,
    /// Address of PLT/GOT
    pub pltgot: u64,
    /// Address of init function
    pub init: u64,
    /// Address of fini function
    pub fini: u64,
    /// Address of init function array
    pub init_array: u64,
    /// Size of init function array
    pub init_arraysz: u64,
    /// Address of fini function array
    pub fini_array: u64,
    /// Size of fini function array
    pub fini_arraysz: u64,
    /// Address of ELF hash table
    pub hash: u64,
    /// Address of GNU hash table
    pub gnu_hash: u64,
    /// DT_FLAGS value
    pub flags: u64,
    /// DT_FLAGS_1 value
    pub flags_1: u64,
    /// Number of RELA relocations (relative)
    pub relacount: u64,
    /// Number of REL relocations (relative)
    pub relcount: u64,
    /// Symbol versioning table address
    pub versym: u64,
    /// Version needed section address
    pub verneed: u64,
    /// Number of version needed entries
    pub verneednum: u64,
    /// SONAME string offset in strtab
    pub soname: u64,
    /// RPATH string offset in strtab
    pub rpath: u64,
    /// RUNPATH string offset in strtab
    pub runpath: u64,
}

impl DynamicInfo {
    /// Parse dynamic section from memory
    ///
    /// # Safety
    /// The caller must ensure that `base` points to valid memory containing
    /// a properly formatted dynamic section, and `count` is the correct number
    /// of entries.
    pub unsafe fn parse(base: *const Elf64Dyn, count: usize) -> Self {
        let mut info = DynamicInfo::default();

        for i in 0..count {
            let dyn_entry = &*base.add(i);

            if dyn_entry.d_tag == DT_NULL {
                break;
            }

            match dyn_entry.d_tag {
                DT_STRTAB => info.strtab = dyn_entry.d_val,
                DT_STRSZ => info.strsz = dyn_entry.d_val,
                DT_SYMTAB => info.symtab = dyn_entry.d_val,
                DT_SYMENT => info.syment = dyn_entry.d_val,
                DT_RELA => info.rela = dyn_entry.d_val,
                DT_RELASZ => info.relasz = dyn_entry.d_val,
                DT_RELAENT => info.relaent = dyn_entry.d_val,
                DT_REL => info.rel = dyn_entry.d_val,
                DT_RELSZ => info.relsz = dyn_entry.d_val,
                DT_RELENT => info.relent = dyn_entry.d_val,
                DT_JMPREL => info.jmprel = dyn_entry.d_val,
                DT_PLTRELSZ => info.pltrelsz = dyn_entry.d_val,
                DT_PLTREL => info.pltrel = dyn_entry.d_val,
                DT_PLTGOT => info.pltgot = dyn_entry.d_val,
                DT_INIT => info.init = dyn_entry.d_val,
                DT_FINI => info.fini = dyn_entry.d_val,
                DT_INIT_ARRAY => info.init_array = dyn_entry.d_val,
                DT_INIT_ARRAYSZ => info.init_arraysz = dyn_entry.d_val,
                DT_FINI_ARRAY => info.fini_array = dyn_entry.d_val,
                DT_FINI_ARRAYSZ => info.fini_arraysz = dyn_entry.d_val,
                DT_HASH => info.hash = dyn_entry.d_val,
                DT_GNU_HASH => info.gnu_hash = dyn_entry.d_val,
                DT_FLAGS => info.flags = dyn_entry.d_val,
                DT_FLAGS_1 => info.flags_1 = dyn_entry.d_val,
                DT_RELACOUNT => info.relacount = dyn_entry.d_val,
                DT_RELCOUNT => info.relcount = dyn_entry.d_val,
                DT_VERSYM => info.versym = dyn_entry.d_val,
                DT_VERNEED => info.verneed = dyn_entry.d_val,
                DT_VERNEEDNUM => info.verneednum = dyn_entry.d_val,
                DT_SONAME => info.soname = dyn_entry.d_val,
                DT_RPATH => info.rpath = dyn_entry.d_val,
                DT_RUNPATH => info.runpath = dyn_entry.d_val,
                _ => {}
            }
        }

        info
    }

    /// Check if BIND_NOW is set (eager binding)
    pub fn bind_now(&self) -> bool {
        (self.flags & DF_BIND_NOW) != 0 || (self.flags_1 & DF_1_NOW) != 0
    }

    /// Check if this is a PIE
    pub fn is_pie(&self) -> bool {
        (self.flags_1 & DF_1_PIE) != 0
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Calculate ELF hash value for a symbol name
pub fn elf_hash(name: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &byte in name {
        if byte == 0 {
            break;
        }
        h = (h << 4).wrapping_add(byte as u32);
        let g = h & 0xf0000000;
        if g != 0 {
            h ^= g >> 24;
        }
        h &= !g;
    }
    h
}

/// Calculate GNU hash value for a symbol name
pub fn gnu_hash(name: &[u8]) -> u32 {
    let mut h: u32 = 5381;
    for &byte in name {
        if byte == 0 {
            break;
        }
        h = h.wrapping_mul(33).wrapping_add(byte as u32);
    }
    h
}

/// Get a null-terminated string from a string table
///
/// # Safety
/// The caller must ensure that `strtab` points to valid memory and
/// `offset` is within bounds of the string table.
pub unsafe fn get_string(strtab: *const u8, offset: u32) -> &'static [u8] {
    let start = strtab.add(offset as usize);
    let mut len = 0;
    while *start.add(len) != 0 {
        len += 1;
        // Safety limit to prevent infinite loops
        if len > 4096 {
            break;
        }
    }
    core::slice::from_raw_parts(start, len)
}

/// Compare a symbol name with a target name
pub fn name_matches(name: &[u8], target: &[u8]) -> bool {
    if name.len() != target.len() {
        return false;
    }
    for i in 0..name.len() {
        if name[i] != target[i] {
            return false;
        }
    }
    true
}

/// Convert a byte slice to a str if it's valid UTF-8
pub fn bytes_to_str(bytes: &[u8]) -> Option<&str> {
    core::str::from_utf8(bytes).ok()
}

// ============================================================================
// ELF Auxiliary Vector Types (for passing info to dynamic linker)
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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Auxv {
    pub a_type: u64,
    pub a_val: u64,
}
