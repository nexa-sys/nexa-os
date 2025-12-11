//! Symbol lookup functions for the NexaOS dynamic linker

use crate::elf::Elf64Sym;
use crate::state::{DynInfo, LoadedLib, GLOBAL_SYMTAB};

// ============================================================================
// Hash Functions
// ============================================================================

/// GNU hash function
pub fn gnu_hash(name: &[u8]) -> u32 {
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
pub fn elf_hash(name: &[u8]) -> u32 {
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

// ============================================================================
// Symbol Count
// ============================================================================

/// Get symbol count from hash table
pub unsafe fn get_symbol_count(dyn_info: &DynInfo) -> usize {
    if dyn_info.hash != 0 {
        // ELF hash: nbucket at offset 0, nchain at offset 4
        let nchain = *((dyn_info.hash + 4) as *const u32);
        return nchain as usize;
    }
    // Fallback
    256
}

// ============================================================================
// GNU Hash Table Lookup
// ============================================================================

/// Lookup symbol using GNU hash table (much faster than linear search)
/// Returns symbol value (with load_bias applied) or 0 if not found
pub unsafe fn lookup_symbol_gnu_hash(lib: &LoadedLib, name: &[u8]) -> u64 {
    let dyn_info = &lib.dyn_info;

    if dyn_info.gnu_hash == 0 || dyn_info.symtab == 0 || dyn_info.strtab == 0 {
        return 0;
    }

    let gnu_hash_addr = dyn_info.gnu_hash;
    let symtab = dyn_info.symtab;
    let strtab = dyn_info.strtab;
    let syment = if dyn_info.syment == 0 {
        24
    } else {
        dyn_info.syment
    };

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

// ============================================================================
// ELF SYSV Hash Table Lookup
// ============================================================================

/// Lookup symbol using ELF SYSV hash table
pub unsafe fn lookup_symbol_elf_hash(lib: &LoadedLib, name: &[u8]) -> u64 {
    let dyn_info = &lib.dyn_info;

    if dyn_info.hash == 0 || dyn_info.symtab == 0 || dyn_info.strtab == 0 {
        return 0;
    }

    let hash_addr = dyn_info.hash;
    let symtab = dyn_info.symtab;
    let strtab = dyn_info.strtab;
    let syment = if dyn_info.syment == 0 {
        24
    } else {
        dyn_info.syment
    };

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

// ============================================================================
// Library Symbol Lookup
// ============================================================================

/// Lookup a symbol by name in a single library
/// Uses GNU hash if available, falls back to ELF hash or linear search
/// Returns symbol value (with load_bias applied) or 0 if not found
pub unsafe fn lookup_symbol_in_lib(lib: &LoadedLib, name: &[u8]) -> u64 {
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
    let syment = if dyn_info.syment == 0 {
        24
    } else {
        dyn_info.syment
    };

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

// ============================================================================
// Builtin Symbols (provided by the dynamic linker itself)
// ============================================================================

/// Lookup builtin symbols provided by the dynamic linker
/// These are symbols that must be available to all loaded libraries
unsafe fn lookup_builtin_symbol(name: &[u8]) -> u64 {
    // Compare symbol names (strip null terminator if present)
    let name = if name.last() == Some(&0) {
        &name[..name.len() - 1]
    } else {
        name
    };

    match name {
        b"__tls_get_addr" => crate::tls::__tls_get_addr as u64,
        b"__cxa_thread_atexit_impl" => crate::compat::__cxa_thread_atexit_impl as u64,
        b"__dso_handle" => &crate::compat::__dso_handle as *const _ as u64,
        _ => 0,
    }
}

// ============================================================================
// Global Symbol Lookup
// ============================================================================

/// Global symbol lookup - search all loaded libraries
/// Search order: builtin symbols first, main executable, then libraries in load order
pub unsafe fn global_symbol_lookup(name: &[u8]) -> u64 {
    // First check builtin symbols provided by the dynamic linker
    let builtin = lookup_builtin_symbol(name);
    if builtin != 0 {
        return builtin;
    }

    for i in 0..GLOBAL_SYMTAB.lib_count {
        let addr = lookup_symbol_in_lib(&GLOBAL_SYMTAB.libs[i], name);
        if addr != 0 {
            return addr;
        }
    }
    0
}

/// Global symbol lookup using C string (null-terminated)
pub unsafe fn global_symbol_lookup_cstr(name: *const u8) -> u64 {
    // Find length
    let mut len = 0;
    let mut p = name;
    while *p != 0 && len < 256 {
        len += 1;
        p = p.add(1);
    }

    // Create slice
    let name_slice = core::slice::from_raw_parts(name, len);
    global_symbol_lookup(name_slice)
}

/// Get symbol name from symbol table by index
pub unsafe fn get_symbol_name(dyn_info: &DynInfo, sym_idx: u32) -> *const u8 {
    if dyn_info.symtab == 0 || dyn_info.strtab == 0 {
        return core::ptr::null();
    }

    let syment = if dyn_info.syment == 0 {
        24
    } else {
        dyn_info.syment
    };
    let sym = &*((dyn_info.symtab + (sym_idx as u64) * syment) as *const Elf64Sym);

    if sym.st_name == 0 {
        return core::ptr::null();
    }

    (dyn_info.strtab + sym.st_name as u64) as *const u8
}
