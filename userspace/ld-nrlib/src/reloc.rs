//! Relocation processing for the NexaOS dynamic linker

use crate::constants::*;
use crate::elf::{Elf64Rela, Elf64Sym};
use crate::helpers::memcpy_internal;
use crate::state::DynInfo;
use crate::symbol::{get_symbol_name, global_symbol_lookup_cstr};

// ============================================================================
// Relocation Processing with Symbol Table
// ============================================================================

/// Process RELA relocations with symbol lookup
pub unsafe fn process_rela_with_symtab(
    rela_addr: u64,
    relasz: u64,
    relaent: u64,
    load_bias: i64,
    dyn_info: &DynInfo,
) {
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
                if sym_idx != 0 {
                    let sym_name = get_symbol_name(dyn_info, sym_idx);
                    if !sym_name.is_null() {
                        let sym_addr = global_symbol_lookup_cstr(sym_name);
                        if sym_addr != 0 {
                            // Get symbol size from symtab
                            let syment = if dyn_info.syment == 0 {
                                24
                            } else {
                                dyn_info.syment
                            };
                            let sym = &*((dyn_info.symtab + (sym_idx as u64) * syment)
                                as *const Elf64Sym);
                            let size = sym.st_size as usize;
                            if size > 0 {
                                memcpy_internal(target as *mut u8, sym_addr as *const u8, size);
                            }
                        }
                    }
                }
            }
            R_X86_64_IRELATIVE => {
                // R_X86_64_IRELATIVE: call resolver function to get symbol address
                let resolver_addr = (load_bias + rela.r_addend) as u64;
                if resolver_addr != 0 {
                    let resolver: extern "C" fn() -> u64 = core::mem::transmute(resolver_addr);
                    *target = resolver();
                }
            }
            R_X86_64_DTPMOD64 => {
                // R_X86_64_DTPMOD64: TLS module ID
                if sym_idx != 0 {
                    *target = dyn_info.tls_modid;
                } else {
                    *target = dyn_info.tls_modid;
                }
                if *target == 0 {
                    *target = 1; // Default TLS module ID for main executable
                }
            }
            R_X86_64_DTPOFF64 => {
                // R_X86_64_DTPOFF64: TLS offset within module
                if sym_idx != 0 {
                    let syment = if dyn_info.syment == 0 {
                        24
                    } else {
                        dyn_info.syment
                    };
                    let sym = &*((dyn_info.symtab + (sym_idx as u64) * syment) as *const Elf64Sym);
                    *target = (sym.st_value as i64 + rela.r_addend) as u64;
                } else {
                    *target = rela.r_addend as u64;
                }
            }
            R_X86_64_TPOFF64 => {
                // R_X86_64_TPOFF64: TLS offset from thread pointer
                if sym_idx != 0 {
                    let syment = if dyn_info.syment == 0 {
                        24
                    } else {
                        dyn_info.syment
                    };
                    let sym = &*((dyn_info.symtab + (sym_idx as u64) * syment) as *const Elf64Sym);
                    *target = (sym.st_value as i64 + rela.r_addend) as u64;
                } else {
                    *target = rela.r_addend as u64;
                }
            }
            R_X86_64_TPOFF32 => {
                // R_X86_64_TPOFF32: 32-bit TLS offset from thread pointer
                if sym_idx != 0 {
                    let syment = if dyn_info.syment == 0 {
                        24
                    } else {
                        dyn_info.syment
                    };
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
            R_X86_64_NONE => {}
            _ => {
                // Unknown relocation type - ignore
            }
        }
    }
}

// ============================================================================
// Simple Relocation Processing (without full symbol lookup)
// ============================================================================

/// Process RELA relocations (legacy - without full symbol lookup)
pub unsafe fn process_rela(rela_addr: u64, relasz: u64, relaent: u64, load_bias: i64) {
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
