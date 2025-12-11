//! ELF structure definitions for the NexaOS dynamic linker

// ============================================================================
// ELF Header
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
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

// ============================================================================
// ELF Program Header
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
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

// ============================================================================
// ELF Dynamic Entry
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Dyn {
    pub d_tag: i64,
    pub d_val: u64,
}

// ============================================================================
// ELF Symbol
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Sym {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

// ============================================================================
// ELF Relocation with Addend
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}

// ============================================================================
// Auxiliary Vector Entry
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AuxEntry {
    pub a_type: u64,
    pub a_val: u64,
}
