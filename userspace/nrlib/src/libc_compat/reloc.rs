//! Relocation Processing
//!
//! This module handles ELF relocations for dynamically loaded shared objects.
//! It supports x86_64 relocation types needed for dynamic linking.

use super::elf::*;
use super::rtld::*;
use super::symbol::*;
use core::ptr;

// ============================================================================
// Relocation Error
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocError {
    /// Success
    Success,
    /// Unknown relocation type
    UnknownType(u32),
    /// Symbol not found
    SymbolNotFound,
    /// Invalid address
    InvalidAddress,
    /// Write failed
    WriteFailed,
    /// Overflow in address calculation
    Overflow,
}

// ============================================================================
// Relocation Processing
// ============================================================================

/// Process a single RELA relocation
///
/// # Safety
/// The caller must ensure:
/// - lib points to a valid, loaded library
/// - Memory at the relocation offset is writable
/// - Symbol table and string table are valid
pub unsafe fn process_rela(
    lib: &LoadedLibrary,
    rela: &Elf64Rela,
    bind_now: bool,
) -> Result<(), RelocError> {
    let reloc_type = rela.reloc_type();
    let sym_idx = rela.symbol();

    // Calculate target address (where to apply relocation)
    let target_addr = (rela.r_offset as i64 + lib.load_bias) as u64;

    // Get symbol if needed
    let (sym_addr, _sym_size) = if sym_idx != 0 {
        // Look up the symbol
        let symtab = lib.symtab();
        let strtab = lib.strtab();

        if symtab.is_null() || strtab.is_null() {
            return Err(RelocError::SymbolNotFound);
        }

        let sym = &*symtab.add(sym_idx as usize);
        let sym_name = get_string(strtab, sym.st_name);

        let result = lookup_symbol_for_reloc(lib.index, sym_name, sym);

        if !result.is_defined() && sym.binding() != STB_WEAK {
            // Check if it's a weak symbol that can be 0
            if reloc_type != R_X86_64_JUMP_SLOT || bind_now {
                // For lazy binding, we defer undefined symbols
                return Err(RelocError::SymbolNotFound);
            }
        }

        (result.addr, result.size)
    } else {
        (0u64, 0u64)
    };

    // Apply the relocation based on type
    match reloc_type {
        R_X86_64_NONE => {
            // No relocation needed
        }

        R_X86_64_64 => {
            // S + A
            let value = sym_addr.wrapping_add(rela.r_addend as u64);
            ptr::write_volatile(target_addr as *mut u64, value);
        }

        R_X86_64_PC32 => {
            // S + A - P
            let value = (sym_addr as i64)
                .wrapping_add(rela.r_addend)
                .wrapping_sub(target_addr as i64);
            if value < i32::MIN as i64 || value > i32::MAX as i64 {
                return Err(RelocError::Overflow);
            }
            ptr::write_volatile(target_addr as *mut i32, value as i32);
        }

        R_X86_64_GLOB_DAT => {
            // S
            // Used for GOT entries
            ptr::write_volatile(target_addr as *mut u64, sym_addr);
        }

        R_X86_64_JUMP_SLOT => {
            // S
            // Used for PLT entries (function calls)
            if bind_now || sym_addr != 0 {
                ptr::write_volatile(target_addr as *mut u64, sym_addr);
            }
            // For lazy binding with undefined symbol, keep original PLT stub
        }

        R_X86_64_RELATIVE => {
            // B + A
            // Base address + addend (no symbol involved)
            let value = (lib.base_addr as i64).wrapping_add(rela.r_addend) as u64;
            ptr::write_volatile(target_addr as *mut u64, value);
        }

        R_X86_64_32 => {
            // S + A (truncated to 32 bits)
            let value = sym_addr.wrapping_add(rela.r_addend as u64);
            if value > u32::MAX as u64 {
                return Err(RelocError::Overflow);
            }
            ptr::write_volatile(target_addr as *mut u32, value as u32);
        }

        R_X86_64_32S => {
            // S + A (truncated to 32 bits, sign extended)
            let value = (sym_addr as i64).wrapping_add(rela.r_addend);
            if value < i32::MIN as i64 || value > i32::MAX as i64 {
                return Err(RelocError::Overflow);
            }
            ptr::write_volatile(target_addr as *mut i32, value as i32);
        }

        R_X86_64_COPY => {
            // Copy symbol contents from shared object
            if sym_addr != 0 {
                let size = _sym_size as usize;
                if size > 0 {
                    ptr::copy_nonoverlapping(sym_addr as *const u8, target_addr as *mut u8, size);
                }
            }
        }

        R_X86_64_DTPMOD64 => {
            // TLS module ID - for now just use 1 (main module)
            ptr::write_volatile(target_addr as *mut u64, 1);
        }

        R_X86_64_DTPOFF64 => {
            // TLS offset within module
            let value = sym_addr.wrapping_add(rela.r_addend as u64);
            ptr::write_volatile(target_addr as *mut u64, value);
        }

        R_X86_64_TPOFF64 => {
            // TLS offset from thread pointer
            // For static TLS model
            let value = (sym_addr as i64).wrapping_add(rela.r_addend);
            ptr::write_volatile(target_addr as *mut i64, value);
        }

        R_X86_64_IRELATIVE => {
            // Indirect function (GNU extension)
            // B + A gives address of resolver function
            let resolver_addr = (lib.base_addr as i64).wrapping_add(rela.r_addend) as u64;
            // Call the resolver to get actual function address
            let resolver: unsafe extern "C" fn() -> u64 = core::mem::transmute(resolver_addr);
            let resolved_addr = resolver();
            ptr::write_volatile(target_addr as *mut u64, resolved_addr);
        }

        _ => {
            // Unknown relocation type
            return Err(RelocError::UnknownType(reloc_type));
        }
    }

    Ok(())
}

/// Process a single REL relocation (without explicit addend)
///
/// # Safety
/// Same requirements as process_rela
pub unsafe fn process_rel(
    lib: &LoadedLibrary,
    rel: &Elf64Rel,
    bind_now: bool,
) -> Result<(), RelocError> {
    // REL relocations use implicit addend stored at target location
    let target_addr = (rel.r_offset as i64 + lib.load_bias) as u64;
    let implicit_addend = ptr::read_volatile(target_addr as *const i64);

    // Convert to RELA and process
    let rela = Elf64Rela {
        r_offset: rel.r_offset,
        r_info: rel.r_info,
        r_addend: implicit_addend,
    };

    process_rela(lib, &rela, bind_now)
}

// ============================================================================
// Process All Relocations for a Library
// ============================================================================

/// Process all relocations for a loaded library
///
/// This processes:
/// 1. DT_RELA relocations (with explicit addends)
/// 2. DT_REL relocations (with implicit addends)
/// 3. DT_JMPREL (PLT) relocations
///
/// # Safety
/// The caller must ensure the library is properly loaded and mapped.
pub unsafe fn process_relocations(lib: &LoadedLibrary) -> Result<(), RelocError> {
    let dyn_info = &lib.dyn_info;
    let bind_now = dyn_info.bind_now();

    // Process DT_RELA relocations
    if dyn_info.rela != 0 && dyn_info.relasz > 0 {
        let rela_addr = (dyn_info.rela as i64 + lib.load_bias) as *const Elf64Rela;
        let entry_size = if dyn_info.relaent != 0 {
            dyn_info.relaent as usize
        } else {
            core::mem::size_of::<Elf64Rela>()
        };
        let count = (dyn_info.relasz as usize) / entry_size;

        // Process relative relocations first (optimization)
        // These don't need symbol lookup
        if dyn_info.relacount > 0 {
            for i in 0..dyn_info.relacount as usize {
                let rela = &*rela_addr.add(i);
                if rela.reloc_type() == R_X86_64_RELATIVE {
                    process_rela(lib, rela, bind_now)?;
                }
            }
        }

        // Process remaining relocations
        let start = dyn_info.relacount as usize;
        for i in start..count {
            let rela = &*rela_addr.add(i);
            process_rela(lib, rela, bind_now)?;
        }
    }

    // Process DT_REL relocations (less common on x86_64)
    if dyn_info.rel != 0 && dyn_info.relsz > 0 {
        let rel_addr = (dyn_info.rel as i64 + lib.load_bias) as *const Elf64Rel;
        let entry_size = if dyn_info.relent != 0 {
            dyn_info.relent as usize
        } else {
            core::mem::size_of::<Elf64Rel>()
        };
        let count = (dyn_info.relsz as usize) / entry_size;

        for i in 0..count {
            let rel = &*rel_addr.add(i);
            process_rel(lib, rel, bind_now)?;
        }
    }

    // Process PLT relocations (DT_JMPREL)
    if dyn_info.jmprel != 0 && dyn_info.pltrelsz > 0 {
        let pltrel_addr = (dyn_info.jmprel as i64 + lib.load_bias) as *const u8;

        // Check if using RELA or REL format
        if dyn_info.pltrel == DT_RELA as u64 {
            let rela_addr = pltrel_addr as *const Elf64Rela;
            let count = (dyn_info.pltrelsz as usize) / core::mem::size_of::<Elf64Rela>();

            for i in 0..count {
                let rela = &*rela_addr.add(i);
                process_rela(lib, rela, bind_now)?;
            }
        } else {
            let rel_addr = pltrel_addr as *const Elf64Rel;
            let count = (dyn_info.pltrelsz as usize) / core::mem::size_of::<Elf64Rel>();

            for i in 0..count {
                let rel = &*rel_addr.add(i);
                process_rel(lib, rel, bind_now)?;
            }
        }
    }

    Ok(())
}

// ============================================================================
// Lazy PLT Resolution
// ============================================================================

/// Resolve a single PLT entry lazily
///
/// This is called from the PLT stub when a function is called for the first time.
/// The PLT stub pushes the relocation index and library handle, then jumps here.
///
/// # Safety
/// This function is called from assembly PLT stubs and must be careful about
/// register preservation and calling convention.
#[no_mangle]
pub unsafe extern "C" fn _dl_runtime_resolve(
    lib_handle: *mut core::ffi::c_void,
    reloc_index: u64,
) -> u64 {
    let mgr = get_library_manager();
    let lib_idx = lib_handle as usize;

    if lib_idx >= MAX_LOADED_LIBS {
        return 0;
    }

    let lib = &mgr.libraries[lib_idx];
    if lib.is_free() {
        return 0;
    }

    let dyn_info = &lib.dyn_info;

    // Get the relocation entry
    let pltrel_addr = (dyn_info.jmprel as i64 + lib.load_bias) as *const u8;

    let (sym_idx, target_addr) = if dyn_info.pltrel == DT_RELA as u64 {
        let rela = &*(pltrel_addr as *const Elf64Rela).add(reloc_index as usize);
        (rela.symbol(), (rela.r_offset as i64 + lib.load_bias) as u64)
    } else {
        let rel = &*(pltrel_addr as *const Elf64Rel).add(reloc_index as usize);
        (rel.symbol(), (rel.r_offset as i64 + lib.load_bias) as u64)
    };

    // Look up the symbol
    let symtab = lib.symtab();
    let strtab = lib.strtab();

    if symtab.is_null() || strtab.is_null() {
        return 0;
    }

    let sym = &*symtab.add(sym_idx as usize);
    let sym_name = get_string(strtab, sym.st_name);

    let result = lookup_symbol_for_reloc(lib.index, sym_name, sym);

    if result.is_defined() {
        // Write resolved address to GOT entry
        ptr::write_volatile(target_addr as *mut u64, result.addr);
        result.addr
    } else {
        0
    }
}

// ============================================================================
// PLT/GOT Initialization
// ============================================================================

/// Initialize the PLT/GOT for lazy binding
///
/// This sets up the GOT entries to point to the PLT stub and our resolver.
///
/// # Safety
/// The caller must ensure the library is properly loaded.
pub unsafe fn init_plt_got(lib: &mut LoadedLibrary) {
    if lib.dyn_info.pltgot == 0 {
        return;
    }

    let got_addr = (lib.dyn_info.pltgot as i64 + lib.load_bias) as *mut u64;

    // GOT[0] = address of dynamic section (optional, for debugger)
    // GOT[1] = library handle (for lazy binding)
    // GOT[2] = address of resolver function

    // Store library handle in GOT[1]
    *got_addr.add(1) = lib.index as u64;

    // Store resolver address in GOT[2]
    *got_addr.add(2) = _dl_runtime_resolve as u64;
}

// ============================================================================
// Init/Fini Functions
// ============================================================================

/// Call library initialization functions
///
/// This calls:
/// 1. DT_INIT function (if present)
/// 2. DT_INIT_ARRAY functions (if present)
///
/// # Safety
/// The caller must ensure all relocations have been processed and
/// dependencies are initialized.
pub unsafe fn call_init_functions(lib: &mut LoadedLibrary) {
    if lib.init_called {
        return;
    }

    let dyn_info = &lib.dyn_info;

    // Call DT_INIT
    if dyn_info.init != 0 {
        let init_fn: unsafe extern "C" fn() =
            core::mem::transmute((dyn_info.init as i64 + lib.load_bias) as u64);
        init_fn();
    }

    // Call DT_INIT_ARRAY
    if dyn_info.init_array != 0 && dyn_info.init_arraysz > 0 {
        let init_array =
            (dyn_info.init_array as i64 + lib.load_bias) as *const unsafe extern "C" fn();
        let count = (dyn_info.init_arraysz as usize) / core::mem::size_of::<usize>();

        for i in 0..count {
            let init_fn = *init_array.add(i);
            // Skip sentinel values (-1)
            if init_fn as usize != usize::MAX {
                init_fn();
            }
        }
    }

    lib.init_called = true;
}

/// Call library finalization functions
///
/// This calls:
/// 1. DT_FINI_ARRAY functions (in reverse order)
/// 2. DT_FINI function
///
/// # Safety
/// The caller must ensure no code is still executing from this library.
pub unsafe fn call_fini_functions(lib: &LoadedLibrary) {
    if !lib.init_called {
        return;
    }

    let dyn_info = &lib.dyn_info;

    // Call DT_FINI_ARRAY in reverse order
    if dyn_info.fini_array != 0 && dyn_info.fini_arraysz > 0 {
        let fini_array =
            (dyn_info.fini_array as i64 + lib.load_bias) as *const unsafe extern "C" fn();
        let count = (dyn_info.fini_arraysz as usize) / core::mem::size_of::<usize>();

        for i in (0..count).rev() {
            let fini_fn = *fini_array.add(i);
            // Skip sentinel values (-1)
            if fini_fn as usize != usize::MAX {
                fini_fn();
            }
        }
    }

    // Call DT_FINI
    if dyn_info.fini != 0 {
        let fini_fn: unsafe extern "C" fn() =
            core::mem::transmute((dyn_info.fini as i64 + lib.load_bias) as u64);
        fini_fn();
    }
}
