//! ELF Relocatable Object Loader for Kernel Modules
//!
//! This module handles loading ELF relocatable objects (.o files)
//! and shared objects (.so files) as kernel modules.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr;

/// ELF magic number
const ELF_MAGIC: u32 = 0x464C457F;

/// ELF machine type for x86-64
const EM_X86_64: u16 = 0x3E;

/// ELF types
const ET_REL: u16 = 1; // Relocatable
const ET_DYN: u16 = 3; // Shared object

/// Section types
const SHT_NULL: u32 = 0;
const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;
const SHT_NOBITS: u32 = 8;
const SHT_REL: u32 = 9;

/// Section flags
const SHF_WRITE: u64 = 0x1;
const SHF_ALLOC: u64 = 0x2;
const SHF_EXECINSTR: u64 = 0x4;

/// Symbol binding
const STB_LOCAL: u8 = 0;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;

/// Symbol types
const STT_NOTYPE: u8 = 0;
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const STT_SECTION: u8 = 3;

/// x86-64 relocation types
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
const R_X86_64_GOTPCRELX: u32 = 41;
const R_X86_64_REX_GOTPCRELX: u32 = 42;

/// ELF64 Header
#[repr(C, packed)]
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

/// ELF64 Section Header
#[repr(C, packed)]
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

/// ELF64 Symbol
#[repr(C, packed)]
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
    pub fn binding(&self) -> u8 {
        self.st_info >> 4
    }

    pub fn sym_type(&self) -> u8 {
        self.st_info & 0xf
    }
}

/// ELF64 Relocation with Addend
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}

impl Elf64Rela {
    pub fn sym(&self) -> u32 {
        (self.r_info >> 32) as u32
    }

    pub fn rel_type(&self) -> u32 {
        (self.r_info & 0xffffffff) as u32
    }
}

/// Loaded section information
#[derive(Debug, Clone)]
pub struct LoadedSection {
    pub name: String,
    pub base: usize,
    pub size: usize,
    pub section_idx: usize,
}

/// Module loader error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderError {
    InvalidMagic,
    InvalidClass,
    InvalidMachine,
    InvalidType,
    SectionOutOfBounds,
    SymbolNotFound,
    RelocationFailed,
    AllocationFailed,
    InvalidSection,
    UnsupportedRelocation,
}

/// ELF module loader
pub struct ModuleLoader<'a> {
    data: &'a [u8],
    ehdr: Elf64Ehdr,
    sections: Vec<LoadedSection>,
    section_bases: Vec<usize>, // Runtime base address for each section
    total_size: usize,
    module_base: usize,
}

impl<'a> ModuleLoader<'a> {
    /// Create a new module loader from ELF data
    pub fn new(data: &'a [u8]) -> Result<Self, LoaderError> {
        if data.len() < core::mem::size_of::<Elf64Ehdr>() {
            return Err(LoaderError::InvalidMagic);
        }

        // Parse ELF header
        let ehdr = unsafe { ptr::read_unaligned(data.as_ptr() as *const Elf64Ehdr) };

        // Validate magic
        let magic = u32::from_le_bytes([
            ehdr.e_ident[0],
            ehdr.e_ident[1],
            ehdr.e_ident[2],
            ehdr.e_ident[3],
        ]);
        if magic != ELF_MAGIC {
            return Err(LoaderError::InvalidMagic);
        }

        // Check 64-bit
        if ehdr.e_ident[4] != 2 {
            return Err(LoaderError::InvalidClass);
        }

        // Check machine type
        if ehdr.e_machine != EM_X86_64 {
            return Err(LoaderError::InvalidMachine);
        }

        // Check type (relocatable or shared object)
        if ehdr.e_type != ET_REL && ehdr.e_type != ET_DYN {
            return Err(LoaderError::InvalidType);
        }

        Ok(Self {
            data,
            ehdr,
            sections: Vec::new(),
            section_bases: Vec::new(),
            total_size: 0,
            module_base: 0,
        })
    }

    /// Get section header by index
    fn get_section(&self, idx: usize) -> Result<Elf64Shdr, LoaderError> {
        if idx >= self.ehdr.e_shnum as usize {
            return Err(LoaderError::SectionOutOfBounds);
        }

        let offset = self.ehdr.e_shoff as usize + idx * self.ehdr.e_shentsize as usize;
        if offset + core::mem::size_of::<Elf64Shdr>() > self.data.len() {
            return Err(LoaderError::SectionOutOfBounds);
        }

        Ok(unsafe { ptr::read_unaligned(self.data.as_ptr().add(offset) as *const Elf64Shdr) })
    }

    /// Get section data by index
    fn get_section_data(&self, idx: usize) -> Result<&[u8], LoaderError> {
        let shdr = self.get_section(idx)?;
        if shdr.sh_type == SHT_NOBITS {
            return Ok(&[]);
        }

        let start = shdr.sh_offset as usize;
        let end = start + shdr.sh_size as usize;
        if end > self.data.len() {
            return Err(LoaderError::SectionOutOfBounds);
        }

        Ok(&self.data[start..end])
    }

    /// Get section name
    fn get_section_name(&self, shdr: &Elf64Shdr) -> Result<&str, LoaderError> {
        let strtab_shdr = self.get_section(self.ehdr.e_shstrndx as usize)?;
        let strtab_data = &self.data[strtab_shdr.sh_offset as usize..];

        let name_start = shdr.sh_name as usize;
        let name_end = strtab_data[name_start..]
            .iter()
            .position(|&c| c == 0)
            .map(|p| name_start + p)
            .unwrap_or(strtab_data.len());

        core::str::from_utf8(&strtab_data[name_start..name_end])
            .map_err(|_| LoaderError::InvalidSection)
    }

    /// Calculate total memory needed and allocate module space
    pub fn calculate_size(&mut self) -> Result<usize, LoaderError> {
        let mut total = 0usize;
        self.section_bases = vec![0; self.ehdr.e_shnum as usize];

        for i in 0..self.ehdr.e_shnum as usize {
            let shdr = self.get_section(i)?;

            // Only allocate for sections that need to be loaded
            if (shdr.sh_flags & SHF_ALLOC) != 0 {
                // Align section
                let align = if shdr.sh_addralign > 0 {
                    shdr.sh_addralign as usize
                } else {
                    1
                };
                total = (total + align - 1) & !(align - 1);

                self.section_bases[i] = total;
                total += shdr.sh_size as usize;
            }
        }

        self.total_size = total;
        Ok(total)
    }

    /// Allocate memory for the module
    pub fn allocate(&mut self) -> Result<usize, LoaderError> {
        if self.total_size == 0 {
            self.calculate_size()?;
        }

        if self.total_size == 0 {
            return Ok(0);
        }

        // Allocate executable memory for the module
        // In a real kernel, this would use vmalloc with executable permissions
        use alloc::alloc::{alloc_zeroed, Layout};

        let layout = Layout::from_size_align(self.total_size, 4096)
            .map_err(|_| LoaderError::AllocationFailed)?;

        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            return Err(LoaderError::AllocationFailed);
        }

        self.module_base = ptr as usize;

        // Update section bases to absolute addresses
        for base in self.section_bases.iter_mut() {
            if *base != 0 || self.total_size > 0 {
                *base += self.module_base;
            }
        }

        Ok(self.module_base)
    }

    /// Load sections into allocated memory
    pub fn load_sections(&mut self) -> Result<(), LoaderError> {
        for i in 0..self.ehdr.e_shnum as usize {
            let shdr = self.get_section(i)?;

            if (shdr.sh_flags & SHF_ALLOC) == 0 {
                continue;
            }

            let base = self.section_bases[i];
            let name = self
                .get_section_name(&shdr)
                .map(|s| String::from(s))
                .unwrap_or_else(|_| String::from(""));

            if shdr.sh_type == SHT_NOBITS {
                // BSS section - already zeroed during allocation
                self.sections.push(LoadedSection {
                    name,
                    base,
                    size: shdr.sh_size as usize,
                    section_idx: i,
                });
            } else {
                // Copy section data
                let section_data = self.get_section_data(i)?;
                unsafe {
                    ptr::copy_nonoverlapping(
                        section_data.as_ptr(),
                        base as *mut u8,
                        section_data.len(),
                    );
                }

                self.sections.push(LoadedSection {
                    name,
                    base,
                    size: shdr.sh_size as usize,
                    section_idx: i,
                });
            }
        }

        Ok(())
    }

    /// Find symbol table section
    fn find_symtab(&self) -> Result<(usize, usize), LoaderError> {
        for i in 0..self.ehdr.e_shnum as usize {
            let shdr = self.get_section(i)?;
            if shdr.sh_type == SHT_SYMTAB {
                return Ok((i, shdr.sh_link as usize)); // returns (symtab_idx, strtab_idx)
            }
        }
        Err(LoaderError::SymbolNotFound)
    }

    /// Get symbol value (resolving undefined symbols from kernel and other modules)
    fn get_symbol_value(&self, sym: &Elf64Sym, strtab: &[u8]) -> Result<u64, LoaderError> {
        if sym.st_shndx == 0 {
            // Undefined symbol - look up in kernel symbol table first, then in other modules
            let name_start = sym.st_name as usize;
            let name_end = strtab[name_start..]
                .iter()
                .position(|&c| c == 0)
                .map(|p| name_start + p)
                .unwrap_or(strtab.len());

            let name = core::str::from_utf8(&strtab[name_start..name_end])
                .map_err(|_| LoaderError::SymbolNotFound)?;

            // 1. First, look up in kernel symbol table
            if let Some(addr) = super::symbols::lookup_symbol(name) {
                return Ok(addr);
            }

            // 2. Then, look up in other loaded modules' exported symbols
            if let Some((_module_name, addr)) = super::lookup_module_symbol(name) {
                crate::kdebug!(
                    "kmod: resolved symbol '{}' from module '{}'",
                    name,
                    _module_name
                );
                return Ok(addr);
            }

            // Symbol not found anywhere
            crate::kerror!(
                "kmod: undefined symbol not found: '{}' (kernel symbols: {}, checked modules too)",
                name,
                super::symbols::symbol_count()
            );
            Err(LoaderError::SymbolNotFound)
        } else if sym.st_shndx < self.section_bases.len() as u16 {
            // Symbol in a loaded section
            let section_base = self.section_bases[sym.st_shndx as usize];
            Ok(section_base as u64 + sym.st_value)
        } else {
            // Special section index (ABS, etc.)
            Ok(sym.st_value)
        }
    }

    /// Apply relocations
    pub fn apply_relocations(&self) -> Result<(), LoaderError> {
        // Find symbol table
        let (symtab_idx, strtab_idx) = self.find_symtab()?;
        let symtab_data = self.get_section_data(symtab_idx)?;
        let strtab_data = self.get_section_data(strtab_idx)?;

        let sym_size = core::mem::size_of::<Elf64Sym>();
        let sym_count = symtab_data.len() / sym_size;

        // Process each relocation section
        for i in 0..self.ehdr.e_shnum as usize {
            let shdr = self.get_section(i)?;

            if shdr.sh_type != SHT_RELA {
                continue;
            }

            // Get target section
            let target_idx = shdr.sh_info as usize;
            let target_shdr = self.get_section(target_idx)?;

            // Skip if target section wasn't loaded
            if (target_shdr.sh_flags & SHF_ALLOC) == 0 {
                continue;
            }

            let target_base = self.section_bases[target_idx];
            let rela_data = self.get_section_data(i)?;
            let rela_size = core::mem::size_of::<Elf64Rela>();
            let rela_count = rela_data.len() / rela_size;

            for j in 0..rela_count {
                let rela = unsafe {
                    ptr::read_unaligned(rela_data.as_ptr().add(j * rela_size) as *const Elf64Rela)
                };

                let sym_idx = rela.sym() as usize;
                if sym_idx >= sym_count {
                    return Err(LoaderError::SymbolNotFound);
                }

                let sym = unsafe {
                    ptr::read_unaligned(
                        symtab_data.as_ptr().add(sym_idx * sym_size) as *const Elf64Sym
                    )
                };

                // Get symbol value
                let sym_val = self.get_symbol_value(&sym, strtab_data)?;

                // Calculate relocation address
                let rel_addr = target_base + rela.r_offset as usize;
                let addend = rela.r_addend;

                // Apply relocation based on type
                match rela.rel_type() {
                    R_X86_64_NONE => {}

                    R_X86_64_64 => {
                        // S + A (absolute 64-bit)
                        let value = (sym_val as i64 + addend) as u64;
                        unsafe {
                            ptr::write_unaligned(rel_addr as *mut u64, value);
                        }
                    }

                    R_X86_64_PC32 | R_X86_64_PLT32 => {
                        // S + A - P (PC-relative 32-bit)
                        let value = (sym_val as i64 + addend - rel_addr as i64) as i32;
                        unsafe {
                            ptr::write_unaligned(rel_addr as *mut i32, value);
                        }
                    }

                    R_X86_64_32 => {
                        // S + A (absolute 32-bit, zero-extend)
                        let value = (sym_val as i64 + addend) as u32;
                        unsafe {
                            ptr::write_unaligned(rel_addr as *mut u32, value);
                        }
                    }

                    R_X86_64_32S => {
                        // S + A (absolute 32-bit, sign-extend)
                        let value = (sym_val as i64 + addend) as i32;
                        unsafe {
                            ptr::write_unaligned(rel_addr as *mut i32, value);
                        }
                    }

                    R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX => {
                        // G + GOT + A - P (PC-relative GOT entry)
                        // For kernel modules without GOT, treat as PC-relative to symbol
                        // This works because we're doing static linking effectively
                        let value = (sym_val as i64 + addend - rel_addr as i64) as i32;
                        unsafe {
                            ptr::write_unaligned(rel_addr as *mut i32, value);
                        }
                    }

                    R_X86_64_RELATIVE => {
                        // B + A (base + addend, for position-independent code)
                        let value = (self.module_base as i64 + addend) as u64;
                        unsafe {
                            ptr::write_unaligned(rel_addr as *mut u64, value);
                        }
                    }

                    _ => {
                        crate::kwarn!(
                            "Unsupported relocation type: {} at {:#x}",
                            rela.rel_type(),
                            rel_addr
                        );
                        return Err(LoaderError::UnsupportedRelocation);
                    }
                }
            }
        }

        Ok(())
    }

    /// Find a symbol by name and return its address
    pub fn find_symbol(&self, name: &str) -> Result<u64, LoaderError> {
        let (symtab_idx, strtab_idx) = self.find_symtab()?;
        let symtab_data = self.get_section_data(symtab_idx)?;
        let strtab_data = self.get_section_data(strtab_idx)?;

        let sym_size = core::mem::size_of::<Elf64Sym>();
        let sym_count = symtab_data.len() / sym_size;

        for i in 0..sym_count {
            let sym = unsafe {
                ptr::read_unaligned(symtab_data.as_ptr().add(i * sym_size) as *const Elf64Sym)
            };

            // Skip local symbols and undefined symbols
            if sym.binding() == STB_LOCAL || sym.st_shndx == 0 {
                continue;
            }

            let name_start = sym.st_name as usize;
            let name_end = strtab_data[name_start..]
                .iter()
                .position(|&c| c == 0)
                .map(|p| name_start + p)
                .unwrap_or(strtab_data.len());

            let sym_name = core::str::from_utf8(&strtab_data[name_start..name_end])
                .map_err(|_| LoaderError::SymbolNotFound)?;

            if sym_name == name {
                return self.get_symbol_value(&sym, strtab_data);
            }
        }

        Err(LoaderError::SymbolNotFound)
    }

    /// Get module base address
    pub fn base(&self) -> usize {
        self.module_base
    }

    /// Get module size
    pub fn size(&self) -> usize {
        self.total_size
    }

    /// Get loaded sections
    pub fn sections(&self) -> &[LoadedSection] {
        &self.sections
    }

    /// Extract global symbols that can be exported by this module
    /// Returns a list of (name, address, is_function) tuples
    /// Symbols prefixed with "kmod_export_" will be exported for inter-module FFI
    pub fn extract_exportable_symbols(&self) -> Result<Vec<(String, u64, bool)>, LoaderError> {
        let (symtab_idx, strtab_idx) = self.find_symtab()?;
        let symtab_data = self.get_section_data(symtab_idx)?;
        let strtab_data = self.get_section_data(strtab_idx)?;

        let sym_size = core::mem::size_of::<Elf64Sym>();
        let sym_count = symtab_data.len() / sym_size;

        let mut exports = Vec::new();

        for i in 0..sym_count {
            let sym = unsafe {
                ptr::read_unaligned(symtab_data.as_ptr().add(i * sym_size) as *const Elf64Sym)
            };

            // Only export global or weak symbols that are defined (not undefined)
            let binding = sym.binding();
            if (binding != STB_GLOBAL && binding != STB_WEAK) || sym.st_shndx == 0 {
                continue;
            }

            // Skip symbols with no type or section type
            let sym_type = sym.sym_type();
            if sym_type != STT_FUNC && sym_type != STT_OBJECT {
                continue;
            }

            let name_start = sym.st_name as usize;
            let name_end = strtab_data[name_start..]
                .iter()
                .position(|&c| c == 0)
                .map(|p| name_start + p)
                .unwrap_or(strtab_data.len());

            let sym_name = match core::str::from_utf8(&strtab_data[name_start..name_end]) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Check if this symbol is marked for export
            // Convention: symbols starting with "kmod_export_" or "__kmod_export_" are exported
            if sym_name.starts_with("kmod_export_") || sym_name.starts_with("__kmod_export_") {
                if let Ok(addr) = self.get_symbol_value(&sym, strtab_data) {
                    let is_function = sym_type == STT_FUNC;
                    // Strip the prefix for the exported name
                    let export_name = if sym_name.starts_with("__kmod_export_") {
                        &sym_name[14..] // Skip "__kmod_export_"
                    } else {
                        &sym_name[12..] // Skip "kmod_export_"
                    };
                    exports.push((String::from(export_name), addr, is_function));
                }
            }
        }

        Ok(exports)
    }

    /// Extract module dependencies from special symbols
    /// Modules declare dependencies via symbols like "__kmod_depends_ext2" or "__kmod_depends_virtio_common"
    pub fn extract_dependencies(&self) -> Result<Vec<String>, LoaderError> {
        let (symtab_idx, strtab_idx) = self.find_symtab()?;
        let symtab_data = self.get_section_data(symtab_idx)?;
        let strtab_data = self.get_section_data(strtab_idx)?;

        let sym_size = core::mem::size_of::<Elf64Sym>();
        let sym_count = symtab_data.len() / sym_size;

        let mut deps = Vec::new();

        for i in 0..sym_count {
            let sym = unsafe {
                ptr::read_unaligned(symtab_data.as_ptr().add(i * sym_size) as *const Elf64Sym)
            };

            // Look for symbols that declare dependencies
            // These can be undefined symbols or special marker symbols
            let name_start = sym.st_name as usize;
            let name_end = strtab_data[name_start..]
                .iter()
                .position(|&c| c == 0)
                .map(|p| name_start + p)
                .unwrap_or(strtab_data.len());

            let sym_name = match core::str::from_utf8(&strtab_data[name_start..name_end]) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Convention: symbols starting with "__kmod_depends_" declare a dependency
            // E.g., "__kmod_depends_ext2" means this module depends on "ext2"
            if let Some(dep_name) = sym_name.strip_prefix("__kmod_depends_") {
                if !dep_name.is_empty() && !deps.iter().any(|d: &String| d == dep_name) {
                    deps.push(String::from(dep_name));
                }
            }
        }

        Ok(deps)
    }
}

/// Load an ELF module and return its entry points
pub fn load_elf_module(data: &[u8]) -> Result<LoadedModule, LoaderError> {
    let mut loader = ModuleLoader::new(data)?;

    // Calculate memory requirements
    loader.calculate_size()?;

    // Allocate memory
    let base = loader.allocate()?;
    crate::kinfo!(
        "Module allocated at {:#x}, size {} bytes",
        base,
        loader.size()
    );

    // Load sections
    loader.load_sections()?;

    // Apply relocations
    loader.apply_relocations()?;

    // Find module entry points
    let init_fn = loader
        .find_symbol("module_init")
        .or_else(|_| loader.find_symbol("ext2_module_init"))
        .ok();

    let exit_fn = loader
        .find_symbol("module_exit")
        .or_else(|_| loader.find_symbol("ext2_module_exit"))
        .ok();

    // Extract exportable symbols for inter-module FFI
    let exported_symbols = loader.extract_exportable_symbols().unwrap_or_default();
    if !exported_symbols.is_empty() {
        crate::kinfo!(
            "Module exports {} symbols for inter-module FFI",
            exported_symbols.len()
        );
    }

    // Extract module dependencies
    let dependencies = loader.extract_dependencies().unwrap_or_default();
    if !dependencies.is_empty() {
        crate::kinfo!(
            "Module declares {} dependencies: {:?}",
            dependencies.len(),
            dependencies
        );
    }

    Ok(LoadedModule {
        base,
        size: loader.size(),
        init_fn,
        exit_fn,
        exported_symbols,
        dependencies,
    })
}

/// Exported symbol info from loaded module
#[derive(Debug, Clone)]
pub struct ExportedSymbolInfo {
    pub name: String,
    pub address: u64,
    pub is_function: bool,
}

/// Loaded module information
#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub base: usize,
    pub size: usize,
    pub init_fn: Option<u64>,
    pub exit_fn: Option<u64>,
    /// Symbols exported by this module for inter-module FFI
    pub exported_symbols: Vec<(String, u64, bool)>,
    /// Dependencies declared by this module
    pub dependencies: Vec<String>,
}

impl LoadedModule {
    /// Call the module's init function
    pub fn init(&self) -> Result<i32, LoaderError> {
        if let Some(init_addr) = self.init_fn {
            let init: extern "C" fn() -> i32 = unsafe { core::mem::transmute(init_addr) };
            Ok(init())
        } else {
            Ok(0)
        }
    }

    /// Call the module's exit function
    pub fn exit(&self) -> Result<i32, LoaderError> {
        if let Some(exit_addr) = self.exit_fn {
            let exit: extern "C" fn() -> i32 = unsafe { core::mem::transmute(exit_addr) };
            Ok(exit())
        } else {
            Ok(0)
        }
    }
}
