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

mod auxv;
mod compat;
mod constants;
mod elf;
mod helpers;
mod loader;
mod reloc;
mod state;
mod symbol;
mod syscall;
mod tls;

use auxv::{store_auxv, AuxInfo};
use constants::*;
use elf::{AuxEntry, Elf64Dyn, Elf64Phdr, Elf64Sym};
use helpers::{cstr_len, is_libc_library, print_str};
use loader::{load_library_recursive, load_shared_library, parse_dynamic_section};
use reloc::{process_rela, process_rela_with_symtab};
use state::{DynInfo, GLOBAL_SYMTAB};
use symbol::global_symbol_lookup;
use syscall::exit;

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
    naked_asm!("jmp _start",);
}

// ============================================================================
// Main Dynamic Linker Logic
// ============================================================================

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

        parse_dynamic_section(dyn_addr, load_bias, &mut main_dyn_info);

        // Store main executable info in global symbol table (index 0)
        let main_lib = &mut GLOBAL_SYMTAB.libs[0];
        main_lib.base_addr = (first_load_vaddr as i64 + load_bias) as u64;
        main_lib.load_bias = load_bias;
        main_lib.dyn_info = main_dyn_info;
        main_lib.valid = true;
        GLOBAL_SYMTAB.lib_count = 1;

        // Step 1: Load libnrlib.so first (always needed)
        let libnrlib_path = b"/lib64/libnrlib.so\0";
        let (lib_base, lib_bias, lib_dyn_info) = load_shared_library(libnrlib_path.as_ptr());

        if lib_base != 0 {
            let lib_idx = GLOBAL_SYMTAB.lib_count;
            if lib_idx < MAX_LIBS {
                let lib = &mut GLOBAL_SYMTAB.libs[lib_idx];
                lib.base_addr = lib_base;
                lib.load_bias = lib_bias;
                lib.dyn_info = lib_dyn_info;
                lib.valid = true;
                GLOBAL_SYMTAB.lib_count = lib_idx + 1;

                if lib_dyn_info.rela != 0 && lib_dyn_info.relasz > 0 {
                    process_rela(
                        lib_dyn_info.rela,
                        lib_dyn_info.relasz,
                        lib_dyn_info.relaent,
                        lib_bias,
                    );
                }

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

        // Step 2: Load other DT_NEEDED libraries
        if main_dyn_info.needed_count > 0 {
            for i in 0..main_dyn_info.needed_count {
                let name_offset = main_dyn_info.needed[i];
                let name_ptr = (main_dyn_info.strtab + name_offset) as *const u8;
                let name_len = cstr_len(name_ptr);

                if is_libc_library(name_ptr) {
                    continue;
                }

                let name_slice = core::slice::from_raw_parts(name_ptr, name_len);
                load_library_recursive(name_slice);
            }
        }

        // Step 3: Process main executable's relocations
        if main_dyn_info.rela != 0 && main_dyn_info.relasz > 0 {
            process_rela_with_symtab(
                main_dyn_info.rela,
                main_dyn_info.relasz,
                main_dyn_info.relaent,
                load_bias,
                &main_dyn_info,
            );
        }

        if main_dyn_info.jmprel != 0 && main_dyn_info.pltrelsz > 0 {
            process_rela_with_symtab(
                main_dyn_info.jmprel,
                main_dyn_info.pltrelsz,
                24,
                load_bias,
                &main_dyn_info,
            );
        }

        // Step 4: Process library relocations
        for i in 1..GLOBAL_SYMTAB.lib_count {
            let lib = &GLOBAL_SYMTAB.libs[i];
            if !lib.valid {
                continue;
            }

            if lib.dyn_info.rela != 0 && lib.dyn_info.relasz > 0 {
                process_rela_with_symtab(
                    lib.dyn_info.rela,
                    lib.dyn_info.relasz,
                    lib.dyn_info.relaent,
                    lib.load_bias,
                    &lib.dyn_info,
                );
            }

            if lib.dyn_info.jmprel != 0 && lib.dyn_info.pltrelsz > 0 {
                process_rela_with_symtab(
                    lib.dyn_info.jmprel,
                    lib.dyn_info.pltrelsz,
                    24,
                    lib.load_bias,
                    &lib.dyn_info,
                );
            }
        }

        // Step 5: Call preinit_array
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

        // Step 6: Call init functions
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

    if entry == 0 {
        let elf_header_addr = (first_load_vaddr as i64 + load_bias) as u64;
        let e_entry = *((elf_header_addr + 24) as *const u64);

        if e_entry != 0 {
            entry = (e_entry as i64 + load_bias) as u64;
        } else {
            if dyn_addr != 0 {
                let start_addr = find_symbol_by_name(dyn_addr, load_bias, b"_start\0");
                if start_addr != 0 {
                    entry = start_addr;
                }
            }

            if entry == 0 {
                let start_addr = global_symbol_lookup(b"_start");
                if start_addr != 0 {
                    entry = start_addr;
                } else {
                    let get_start_fn = global_symbol_lookup(b"__nexa_get_start_addr");
                    if get_start_fn != 0 {
                        let get_start: extern "C" fn() -> usize =
                            core::mem::transmute(get_start_fn);
                        let start_addr = get_start() as u64;
                        if start_addr != 0 {
                            entry = start_addr;
                        }
                    }
                }

                if entry == 0 {
                    let crt_addr = global_symbol_lookup(b"__nexa_crt_start");
                    if crt_addr != 0 {
                        entry = crt_addr;
                    } else {
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

    jump_to_entry(entry, stack_ptr);
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Find a symbol by name in the dynamic symbol table
unsafe fn find_symbol_by_name(dyn_addr: u64, load_bias: i64, name: &[u8]) -> u64 {
    let mut strtab: u64 = 0;
    let mut symtab: u64 = 0;
    let mut hash: u64 = 0;
    let mut gnu_hash: u64 = 0;

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

    let sym_count = if hash != 0 {
        *((hash + 4) as *const u32) as usize
    } else if gnu_hash != 0 {
        256
    } else {
        256
    };

    for i in 0..sym_count {
        let sym = &*((symtab + i as u64 * 24) as *const Elf64Sym);
        if sym.st_name == 0 {
            continue;
        }

        let sym_name_ptr = (strtab + sym.st_name as u64) as *const u8;

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
            if sym.st_value != 0 {
                return (sym.st_value as i64 + load_bias) as u64;
            }
        }
    }

    0
}

/// Call main() directly with C calling convention
#[inline(never)]
unsafe fn call_main_directly(main_addr: u64, stack_ptr: *const u64) -> ! {
    let argc = *stack_ptr as i32;
    let argv = stack_ptr.add(1) as *const *const u8;

    let main_fn: extern "C" fn(i32, *const *const u8) -> i32 = core::mem::transmute(main_addr);
    let ret = main_fn(argc, argv);

    exit(ret);
}

/// Jump to the entry point of the main executable
#[inline(never)]
unsafe fn jump_to_entry(entry: u64, stack_ptr: *const u64) -> ! {
    asm!(
        "mov r14, {entry}",
        "mov r15, {stack}",
        "mov rsp, r15",
        "xor rbp, rbp",
        "xor rax, rax",
        "xor rbx, rbx",
        "xor rcx, rcx",
        "xor rdx, rdx",
        "xor rsi, rsi",
        "mov rdi, rsp",
        "xor r8, r8",
        "xor r9, r9",
        "xor r10, r10",
        "xor r11, r11",
        "xor r12, r12",
        "xor r13, r13",
        "jmp r14",
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
