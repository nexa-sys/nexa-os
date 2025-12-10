//! Symbol Resolution - Symbol lookup and binding
//!
//! This module provides symbol lookup functionality using both ELF hash
//! and GNU hash tables, as well as linear search fallback.

use super::elf::*;
use super::rtld::*;

// ============================================================================
// Symbol Lookup Result
// ============================================================================

/// Result of a symbol lookup
#[derive(Debug, Clone, Copy)]
pub struct SymbolResult {
    /// Address of the symbol
    pub addr: u64,
    /// Size of the symbol
    pub size: u64,
    /// Symbol binding (STB_LOCAL, STB_GLOBAL, STB_WEAK)
    pub binding: u8,
    /// Symbol type (STT_FUNC, STT_OBJECT, etc.)
    pub symbol_type: u8,
    /// Index of library containing the symbol
    pub lib_index: usize,
    /// Symbol table index
    pub sym_index: u32,
}

impl SymbolResult {
    pub fn new() -> Self {
        Self {
            addr: 0,
            size: 0,
            binding: STB_LOCAL,
            symbol_type: STT_NOTYPE,
            lib_index: 0,
            sym_index: 0,
        }
    }

    pub fn is_defined(&self) -> bool {
        self.addr != 0
    }

    pub fn is_weak(&self) -> bool {
        self.binding == STB_WEAK
    }
}

// ============================================================================
// Symbol Lookup Options
// ============================================================================

/// Options for symbol lookup
#[derive(Debug, Clone, Copy)]
pub struct LookupOptions {
    /// Start searching from this library index (for RTLD_NEXT)
    pub start_index: usize,
    /// Skip undefined symbols
    pub skip_undefined: bool,
    /// Search in global scope only
    pub global_only: bool,
    /// Include weak symbols in results
    pub include_weak: bool,
}

impl Default for LookupOptions {
    fn default() -> Self {
        Self {
            start_index: 0,
            skip_undefined: true,
            global_only: false,
            include_weak: true,
        }
    }
}

// ============================================================================
// ELF Hash Table Lookup
// ============================================================================

/// Look up a symbol using ELF hash table
///
/// # Safety
/// The caller must ensure that the library's hash table and symbol table
/// are valid and properly initialized.
pub unsafe fn lookup_elf_hash(
    lib: &LoadedLibrary,
    name: &[u8],
    hash: u32,
) -> Option<(u32, &'static Elf64Sym)> {
    if lib.dyn_info.hash == 0 {
        return None;
    }

    let hash_addr = (lib.dyn_info.hash as i64 + lib.load_bias) as *const u32;
    let nbucket = *hash_addr;
    let _nchain = *hash_addr.add(1);

    if nbucket == 0 {
        return None;
    }

    let buckets = hash_addr.add(2);
    let chains = buckets.add(nbucket as usize);

    let symtab = lib.symtab();
    let strtab = lib.strtab();

    if symtab.is_null() || strtab.is_null() {
        return None;
    }

    let mut idx = *buckets.add((hash % nbucket) as usize);

    while idx != 0 {
        let sym = &*symtab.add(idx as usize);
        let sym_name = get_string(strtab, sym.st_name);

        if name_matches(sym_name, name) {
            return Some((idx, sym));
        }

        idx = *chains.add(idx as usize);

        // Safety: prevent infinite loops
        if idx > 0x100000 {
            break;
        }
    }

    None
}

// ============================================================================
// GNU Hash Table Lookup
// ============================================================================

/// Look up a symbol using GNU hash table
///
/// # Safety
/// The caller must ensure that the library's GNU hash table and symbol table
/// are valid and properly initialized.
pub unsafe fn lookup_gnu_hash(
    lib: &LoadedLibrary,
    name: &[u8],
    hash: u32,
) -> Option<(u32, &'static Elf64Sym)> {
    if lib.dyn_info.gnu_hash == 0 {
        return None;
    }

    let hash_addr = (lib.dyn_info.gnu_hash as i64 + lib.load_bias) as *const u32;

    let nbuckets = *hash_addr;
    let symoffset = *hash_addr.add(1);
    let bloom_size = *hash_addr.add(2);
    let bloom_shift = *hash_addr.add(3);

    if nbuckets == 0 || bloom_size == 0 {
        return None;
    }

    // Bloom filter (64-bit entries)
    let bloom = hash_addr.add(4) as *const u64;

    // Check bloom filter first
    let word_idx = ((hash / 64) % bloom_size) as usize;
    let bloom_word = *bloom.add(word_idx);
    let h1 = hash as u64;
    let h2 = (hash >> bloom_shift) as u64;
    let mask = (1u64 << (h1 % 64)) | (1u64 << (h2 % 64));

    if (bloom_word & mask) != mask {
        return None; // Definitely not in table
    }

    // Find bucket and chain arrays
    let buckets = (bloom.add(bloom_size as usize)) as *const u32;
    let chains = buckets.add(nbuckets as usize);

    let symtab = lib.symtab();
    let strtab = lib.strtab();

    if symtab.is_null() || strtab.is_null() {
        return None;
    }

    // Get the bucket
    let bucket = *buckets.add((hash % nbuckets) as usize);
    if bucket == 0 {
        return None;
    }

    // Walk the chain
    let mut idx = bucket;
    loop {
        let chain_idx = idx - symoffset;
        let chain_hash = *chains.add(chain_idx as usize);

        // Compare hashes (low bit indicates end of chain)
        if (hash | 1) == (chain_hash | 1) {
            let sym = &*symtab.add(idx as usize);
            let sym_name = get_string(strtab, sym.st_name);

            if name_matches(sym_name, name) {
                return Some((idx, sym));
            }
        }

        // Check if this is the end of the chain
        if (chain_hash & 1) != 0 {
            break;
        }

        idx += 1;

        // Safety: prevent infinite loops
        if idx > 0x100000 {
            break;
        }
    }

    None
}

// ============================================================================
// Linear Symbol Table Search (Fallback)
// ============================================================================

/// Linear search through symbol table (fallback when no hash table)
///
/// # Safety
/// The caller must ensure that the library's symbol table is valid.
pub unsafe fn lookup_linear(
    lib: &LoadedLibrary,
    name: &[u8],
    max_symbols: usize,
) -> Option<(u32, &'static Elf64Sym)> {
    let symtab = lib.symtab();
    let strtab = lib.strtab();

    if symtab.is_null() || strtab.is_null() {
        return None;
    }

    let syment = if lib.dyn_info.syment != 0 {
        lib.dyn_info.syment as usize
    } else {
        core::mem::size_of::<Elf64Sym>()
    };

    // Estimate max symbols from hash table if available
    let max_count = if lib.dyn_info.hash != 0 {
        let hash_addr = (lib.dyn_info.hash as i64 + lib.load_bias) as *const u32;
        let nchain = *hash_addr.add(1);
        nchain as usize
    } else {
        max_symbols
    };

    for i in 1..max_count {
        let sym = &*symtab.add(i);
        if sym.st_name == 0 {
            continue;
        }

        let sym_name = get_string(strtab, sym.st_name);
        if name_matches(sym_name, name) {
            return Some((i as u32, sym));
        }
    }

    None
}

// ============================================================================
// Symbol Lookup in Single Library
// ============================================================================

/// Look up a symbol in a specific library
///
/// # Safety
/// The caller must ensure the library is valid and initialized.
pub unsafe fn lookup_symbol_in_lib(lib: &LoadedLibrary, name: &[u8]) -> Option<SymbolResult> {
    // Calculate hashes
    let elf_h = elf_hash(name);
    let gnu_h = gnu_hash(name);

    // Try GNU hash first (faster)
    let result = lookup_gnu_hash(lib, name, gnu_h)
        .or_else(|| lookup_elf_hash(lib, name, elf_h))
        .or_else(|| lookup_linear(lib, name, 10000));

    result.map(|(sym_idx, sym)| {
        let addr = if sym.is_defined() {
            (sym.st_value as i64 + lib.load_bias) as u64
        } else {
            0
        };

        SymbolResult {
            addr,
            size: sym.st_size,
            binding: sym.binding(),
            symbol_type: sym.symbol_type(),
            lib_index: lib.index,
            sym_index: sym_idx,
        }
    })
}

// ============================================================================
// Global Symbol Lookup
// ============================================================================

/// Look up a symbol across all loaded libraries
///
/// This implements the standard symbol lookup order:
/// 1. Search the main executable
/// 2. Search libraries in load order
/// 3. Prefer global symbols over weak symbols
///
/// # Safety
/// The caller must ensure proper synchronization with the library manager.
pub unsafe fn lookup_symbol(name: &[u8], options: &LookupOptions) -> Option<SymbolResult> {
    let mgr = get_library_manager();

    let mut weak_result: Option<SymbolResult> = None;

    for i in options.start_index..mgr.count {
        let lib = &mgr.libraries[i];

        if lib.is_free() || lib.state == LibraryState::Loading {
            continue;
        }

        if options.global_only && (lib.flags & RTLD_LOCAL) != 0 {
            continue;
        }

        if let Some(result) = lookup_symbol_in_lib(lib, name) {
            if options.skip_undefined && !result.is_defined() {
                continue;
            }

            if result.binding == STB_GLOBAL {
                return Some(result);
            }

            if result.is_weak() && options.include_weak && weak_result.is_none() {
                weak_result = Some(result);
            }
        }
    }

    weak_result
}

/// Look up a symbol by name (string)
///
/// # Safety
/// Same as lookup_symbol
pub unsafe fn lookup_symbol_by_name(name: &str) -> Option<SymbolResult> {
    lookup_symbol(name.as_bytes(), &LookupOptions::default())
}

// ============================================================================
// Symbol Lookup for Relocation
// ============================================================================

/// Look up a symbol for relocation purposes
///
/// This is used during relocation processing and follows the binding rules:
/// - Global symbols are preferred
/// - Weak symbols are used if no global is found
/// - Undefined weak symbols resolve to 0
///
/// # Safety
/// The caller must ensure proper synchronization with the library manager.
pub unsafe fn lookup_symbol_for_reloc(
    requesting_lib: usize,
    name: &[u8],
    sym: &Elf64Sym,
) -> SymbolResult {
    let mgr = get_library_manager();

    // First, check if this is a local symbol
    if sym.binding() == STB_LOCAL {
        let lib = &mgr.libraries[requesting_lib];
        return SymbolResult {
            addr: (sym.st_value as i64 + lib.load_bias) as u64,
            size: sym.st_size,
            binding: sym.binding(),
            symbol_type: sym.symbol_type(),
            lib_index: requesting_lib,
            sym_index: 0,
        };
    }

    // Search for the symbol globally
    let options = LookupOptions {
        start_index: 0,
        skip_undefined: true,
        global_only: false,
        include_weak: true,
    };

    if let Some(result) = lookup_symbol(name, &options) {
        return result;
    }

    // Undefined weak symbols resolve to 0
    if sym.is_weak() {
        return SymbolResult {
            addr: 0,
            size: 0,
            binding: STB_WEAK,
            symbol_type: sym.symbol_type(),
            lib_index: requesting_lib,
            sym_index: 0,
        };
    }

    // Undefined symbol - return failure
    SymbolResult::new()
}

// ============================================================================
// Symbol Iteration
// ============================================================================

/// Iterator over symbols in a library
pub struct SymbolIterator<'a> {
    lib: &'a LoadedLibrary,
    index: usize,
    max_index: usize,
}

impl<'a> SymbolIterator<'a> {
    /// Create a new symbol iterator for a library
    ///
    /// # Safety
    /// The caller must ensure the library is valid.
    pub unsafe fn new(lib: &'a LoadedLibrary) -> Self {
        let max_index = if lib.dyn_info.hash != 0 {
            let hash_addr = (lib.dyn_info.hash as i64 + lib.load_bias) as *const u32;
            (*hash_addr.add(1)) as usize // nchain
        } else {
            0
        };

        Self {
            lib,
            index: 1, // Skip symbol 0 (undefined)
            max_index,
        }
    }
}

impl<'a> Iterator for SymbolIterator<'a> {
    type Item = (u32, &'a Elf64Sym, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.max_index {
            return None;
        }

        let symtab = self.lib.symtab();
        let strtab = self.lib.strtab();

        if symtab.is_null() || strtab.is_null() {
            return None;
        }

        unsafe {
            let sym = &*symtab.add(self.index);
            let name = get_string(strtab, sym.st_name);
            let idx = self.index as u32;
            self.index += 1;
            Some((idx, sym, name))
        }
    }
}

// ============================================================================
// Find Symbol containing Address
// ============================================================================

/// Find the symbol containing a given address
///
/// # Safety
/// The caller must ensure the library is valid.
pub unsafe fn find_symbol_by_addr(
    lib: &LoadedLibrary,
    addr: u64,
) -> Option<(u32, &'static Elf64Sym)> {
    let symtab = lib.symtab();
    let strtab = lib.strtab();

    if symtab.is_null() || strtab.is_null() {
        return None;
    }

    let max_symbols = if lib.dyn_info.hash != 0 {
        let hash_addr = (lib.dyn_info.hash as i64 + lib.load_bias) as *const u32;
        (*hash_addr.add(1)) as usize
    } else {
        return None;
    };

    let mut best_match: Option<(u32, &'static Elf64Sym)> = None;
    let mut best_distance: u64 = u64::MAX;

    for i in 1..max_symbols {
        let sym = &*symtab.add(i);

        // Skip undefined symbols
        if !sym.is_defined() {
            continue;
        }

        // Skip symbols without addresses
        if sym.st_value == 0 {
            continue;
        }

        let sym_addr = (sym.st_value as i64 + lib.load_bias) as u64;

        // Check if address is within this symbol's range
        if addr >= sym_addr && addr < sym_addr.saturating_add(sym.st_size.max(1)) {
            let distance = addr - sym_addr;
            if distance < best_distance {
                best_distance = distance;
                best_match = Some((i as u32, sym));
            }
        }
    }

    best_match
}

// ============================================================================
// Versioned Symbol Lookup
// ============================================================================

/// Look up a versioned symbol
///
/// Symbol versioning allows multiple versions of a symbol to coexist.
/// This is an advanced feature used by glibc.
///
/// # Safety
/// The caller must ensure proper synchronization.
pub unsafe fn lookup_versioned_symbol(name: &[u8], version: Option<&[u8]>) -> Option<SymbolResult> {
    // For now, ignore version and do normal lookup
    // Full versioning support would require parsing DT_VERSYM, DT_VERNEED, etc.
    lookup_symbol(name, &LookupOptions::default())
}
