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
use helpers::{cstr_len, print_str, print_hex, print};
use loader::{load_library_recursive, parse_dynamic_section};
use reloc::process_rela_with_symtab;
use state::{DynInfo, GLOBAL_SYMTAB};
use symbol::global_symbol_lookup;
use syscall::exit;

// ============================================================================
// Entry Point
// ============================================================================

/// Early print using inline assembly - no relocations needed
/// Prints a single character to stdout
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn early_putchar(c: u8) {
    let buf: [u8; 1] = [c];
    let ret: i64;
    // Use volatile option to ensure syscall is not optimized away
    core::arch::asm!(
        "syscall",
        in("rax") 1u64, // SYS_WRITE
        in("rdi") 1u64, // stdout (fd 1)
        in("rsi") buf.as_ptr(),
        in("rdx") 1u64,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    // Use ret to prevent optimization
    core::hint::black_box(ret);
}

/// Bootstrap: relocate ourselves before accessing any global data
/// Must be called before any code that uses global variables or string literals
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn bootstrap_self(stack_ptr: *const u64) -> i64 {
    // Use individual chars - byte literals don't need relocation
    early_putchar(b'\n');
    early_putchar(b'[');
    early_putchar(b'B');
    early_putchar(b'S');
    early_putchar(b']');
    
    // Parse stack to find auxv
    let argc = *stack_ptr as usize;
    let argv = stack_ptr.add(1) as *const *const u8;
    let mut ptr = argv.add(argc + 1) as *const *const u8;
    
    // Skip envp - be careful with potential infinite loop
    let mut env_count = 0usize;
    while !(*ptr).is_null() && env_count < 1000 {
        ptr = ptr.add(1);
        env_count += 1;
    }
    ptr = ptr.add(1);
    
    // Now ptr points to auxv - find AT_BASE (our load address)
    let mut ld_base: u64 = 0;
    let auxv = ptr as *const AuxEntry;
    let mut aux_ptr = auxv;
    let mut aux_count = 0usize;
    
    // Parse auxv silently
    loop {
        if aux_count > 100 {
            break;
        }
        let entry = *aux_ptr;
        if entry.a_type == 0 { // AT_NULL
            break;
        }
        if entry.a_type == 7 { // AT_BASE
            ld_base = entry.a_val;
        }
        aux_ptr = aux_ptr.add(1);
        aux_count += 1;
    }
    
    // Print result: B for base found, 0 for no base
    if ld_base == 0 {
        early_putchar(b'0');
        early_putchar(b'\n');
        return 0;
    }
    
    early_putchar(b'B');
    
    // Find our own _DYNAMIC section via program headers embedded in our ELF
    // The ELF header is at ld_base
    let ehdr = ld_base as *const Elf64Ehdr;
    let phoff = (*ehdr).e_phoff;
    let phnum = (*ehdr).e_phnum as usize;
    let phentsize = (*ehdr).e_phentsize as usize;
    
    let mut dyn_vaddr: u64 = 0;
    for i in 0..phnum {
        let phdr = (ld_base + phoff + (i * phentsize) as u64) as *const Elf64Phdr;
        if (*phdr).p_type == PT_DYNAMIC {
            dyn_vaddr = (*phdr).p_vaddr;
            break;
        }
    }
    
    if dyn_vaddr == 0 {
        early_putchar(b'E');
        early_putchar(b'\n');
        return -1;
    }
    
    early_putchar(b'D');
    
    // Process our own relocations
    let dyn_addr = ld_base + dyn_vaddr;
    let mut rela: u64 = 0;
    let mut relasz: u64 = 0;
    
    let mut dyn_ptr = dyn_addr as *const Elf64Dyn;
    loop {
        let entry = *dyn_ptr;
        if entry.d_tag == 0 { // DT_NULL
            break;
        }
        match entry.d_tag {
            7 => rela = entry.d_val,   // DT_RELA
            8 => relasz = entry.d_val, // DT_RELASZ
            _ => {}
        }
        dyn_ptr = dyn_ptr.add(1);
    }
    
    if rela != 0 && relasz != 0 {
        // Process R_X86_64_RELATIVE relocations
        let rela_addr = ld_base + rela;
        let num_rela = relasz / 24; // sizeof(Elf64_Rela) = 24
        
        for i in 0..num_rela {
            let rela_entry = (rela_addr + i * 24) as *const Elf64Rela;
            let r_type = (*rela_entry).r_info & 0xffffffff;
            
            if r_type == 8 { // R_X86_64_RELATIVE
                let r_offset = (*rela_entry).r_offset;
                let r_addend = (*rela_entry).r_addend;
                let target = (ld_base + r_offset) as *mut u64;
                *target = (ld_base as i64 + r_addend) as u64;
            }
        }
        early_putchar(b'R');
        early_putchar(b'\n');
    } else {
        early_putchar(b'N');
        early_putchar(b'\n');
    }
    
    ld_base as i64
}

/// Elf64_Rela structure for relocations
#[repr(C)]
struct Elf64Rela {
    r_offset: u64,
    r_info: u64,
    r_addend: i64,
}

/// Elf64_Ehdr structure
#[repr(C)]
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
        
        // Print '[' using syscall directly (cannot be optimized away)
        "push rsp",               // Save original stack pointer
        "sub rsp, 16",            // Allocate buffer on stack (aligned)
        "mov byte ptr [rsp], 0x5b", // '[' character
        "mov rax, 1",             // SYS_WRITE
        "mov rdi, 1",             // stdout
        "mov rsi, rsp",           // buffer
        "mov rdx, 1",             // length
        "syscall",
        // Print 'S'
        "mov byte ptr [rsp], 0x53", // 'S' character
        "mov rax, 1",
        "mov rdi, 1",
        "mov rsi, rsp",
        "mov rdx, 1",
        "syscall",
        // Print ']'
        "mov byte ptr [rsp], 0x5d", // ']' character
        "mov rax, 1",
        "mov rdi, 1",
        "mov rsi, rsp",
        "mov rdx, 1",
        "syscall",
        "add rsp, 16",            // Restore stack
        "pop rdi",                // Get original stack pointer into rdi (argument for ld_main)
        
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
    // CRITICAL: Bootstrap ourselves first - relocate our own GOT/data
    // This must happen before accessing ANY global variables or string literals
    // Use black_box to prevent optimization
    let ld_base = core::hint::black_box(bootstrap_self(stack_ptr));
    
    // Additional barrier to ensure bootstrap_self is not optimized away
    if ld_base < 0 {
        // Should never happen, but prevents dead code elimination
        syscall::exit(127);
    }
    
    // Now we can safely use print_str and other functions
    print_str("LD1\n");
    
    // Parse the stack to get argc, argv, envp, auxv
    let argc = *stack_ptr as usize;
    print_str("LD_ARGC\n");
    let argv = stack_ptr.add(1) as *const *const u8;

    print_str("LD_ARGV\n");

    // Skip past argv (argc+1 entries including NULL terminator)
    let mut ptr = argv.add(argc + 1) as *const *const u8;

    // Skip past envp (until NULL)
    while !(*ptr).is_null() {
        ptr = ptr.add(1);
    }
    ptr = ptr.add(1); // Skip NULL terminator

    print_str("LD_ENVP\n");

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

    print_str("LD4\n");

    // Store auxv globally for getauxval support
    store_auxv(&aux_info);

    print_str("LD5\n");

    // Find the dynamic section of the main executable
    if aux_info.at_phdr == 0 || aux_info.at_phnum == 0 {
        print_str("[ld-nrlib] ERROR: Invalid auxv\n");
        exit(127);
    }

    print_str("LD6\n");

    // Scan program headers to find PT_DYNAMIC
    let mut dyn_addr: u64 = 0;
    let mut load_bias: i64 = 0;
    let mut first_load_vaddr: u64 = u64::MAX;

    // Debug: print phdr info
    print_str("PHDR=");
    print_hex(aux_info.at_phdr);
    print_str(" PHNUM=");
    print_hex(aux_info.at_phnum);
    print_str("\n");

    let phdrs = core::slice::from_raw_parts(
        aux_info.at_phdr as *const Elf64Phdr,
        aux_info.at_phnum as usize,
    );

    print_str("LD7\n");

    // Calculate load bias from first PT_LOAD segment
    // Also find PT_TLS segment for main executable
    let mut phdr_idx = 0usize;
    for phdr in phdrs {
        // Debug: print each phdr type
        print_str("PT[");
        print_hex(phdr_idx as u64);
        print_str("]=");
        print_hex(phdr.p_type as u64);
        print_str("\n");
        phdr_idx += 1;
        
        if phdr.p_type == PT_LOAD && phdr.p_vaddr < first_load_vaddr {
            first_load_vaddr = phdr.p_vaddr;
        }
        if phdr.p_type == PT_DYNAMIC {
            dyn_addr = phdr.p_vaddr;
            print_str("FOUND_DYN=");
            print_hex(dyn_addr);
            print_str("\n");
        }
    }
    print_str("LOOP_DONE dyn=");
    print_hex(dyn_addr);
    print_str("\n");

    print_str("LD8\n");

    // For PIE executables loaded at AT_PHDR location
    if first_load_vaddr != u64::MAX {
        for phdr in phdrs {
            if phdr.p_type == PT_PHDR {
                load_bias = (aux_info.at_phdr as i64) - (phdr.p_vaddr as i64);
                break;
            }
        }
    }

    print_str("LD9\n");

    // Register TLS for main executable
    for phdr in phdrs {
        if phdr.p_type == PT_TLS {
            let tls_image = (phdr.p_vaddr as i64 + load_bias) as u64;
            tls::register_tls_module(tls_image, phdr.p_filesz, phdr.p_memsz, phdr.p_align);
            break;
        }
    }

    print_str("LDA\n");

    // Debug: print dyn_addr before check
    print_str("DYN_ADDR=");
    print_hex(dyn_addr);
    print_str("\n");
    
    // TEST: Print before DynInfo allocation
    print_str("PRE_ALLOC\n");

    // Use static storage to avoid stack issues
    // Note: This is safe because ld_main only runs once per process
    static mut MAIN_DYN_INFO: DynInfo = DynInfo::new();
    let main_dyn_info = &mut MAIN_DYN_INFO;
    
    // TEST: Print after DynInfo allocation
    print_str("POST_ALLOC\n");
    
    // Force condition evaluation - prevent optimization
    let dyn_nonzero = dyn_addr != 0;
    print_str("NONZERO=");
    print_hex(dyn_nonzero as u64);
    print_str("\n");
    
    // *** TEST: Print unconditionally to verify we get here ***
    print_str("BEFORE_IF\n");
    
    if dyn_nonzero {
        print_str("LDA1\n");
        dyn_addr = (dyn_addr as i64 + load_bias) as u64;
        print_str("LDA2 DYN=");
        print_hex(dyn_addr);
        print_str("\n");

        parse_dynamic_section(dyn_addr, load_bias, main_dyn_info);

        print_str("LDB\n");

        // Store main executable info in global symbol table (index 0)
        let main_lib = &mut GLOBAL_SYMTAB.libs[0];
        main_lib.base_addr = (first_load_vaddr as i64 + load_bias) as u64;
        main_lib.load_bias = load_bias;
        main_lib.dyn_info = *main_dyn_info;
        main_lib.valid = true;
        GLOBAL_SYMTAB.lib_count = 1;

        print_str("LDC\n");

        // Load DT_NEEDED libraries from main executable
        print_str("LDD NEEDED=");
        print_hex(main_dyn_info.needed_count as u64);
        print_str("\n");
        if main_dyn_info.needed_count > 0 {
            for i in 0..main_dyn_info.needed_count {
                let name_offset = main_dyn_info.needed[i];
                let name_ptr = (main_dyn_info.strtab + name_offset) as *const u8;
                let name_len = cstr_len(name_ptr);

                let name_slice = core::slice::from_raw_parts(name_ptr, name_len);
                print_str("LOAD: ");
                print(name_slice);
                print_str("\n");
                load_library_recursive(name_slice);
            }
        }

        // Step 3: Process main executable's relocations
        print_str("LDE RELA\n");
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

        print_str("LDF LIB_RELOCS\n");
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

        print_str("LDG PREINIT\n");
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

        print_str("LDH INIT\n");
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
        print_str("LDI END_IF\n");
    }

    // Transfer control to the main executable
    let mut entry = aux_info.at_entry;

    print_str("[ld-nrlib] at_entry=");
    print_hex(entry);
    print_str(", lib_count=");
    print_hex(GLOBAL_SYMTAB.lib_count as u64);
    print_str("\n");

    if entry == 0 {
        let elf_header_addr = (first_load_vaddr as i64 + load_bias) as u64;
        let e_entry = *((elf_header_addr + 24) as *const u64);
        print_str("[ld-nrlib] e_entry from header=");
        print_hex(e_entry);
        print_str("\n");

        if e_entry != 0 {
            entry = (e_entry as i64 + load_bias) as u64;
        } else {
            if dyn_addr != 0 {
                let start_addr = find_symbol_by_name(dyn_addr, load_bias, b"_start\0");
                print_str("[ld-nrlib] _start from dyn=");
                print_hex(start_addr);
                print_str("\n");
                if start_addr != 0 {
                    entry = start_addr;
                }
            }

            if entry == 0 {
                let start_addr = global_symbol_lookup(b"_start");
                print_str("[ld-nrlib] _start from global=");
                print_hex(start_addr);
                print_str("\n");
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

    // NOTE: TLS setup is done by nrlib's __nrlib_init_main_thread_tls()
    // We don't set up TLS here because nrlib's ThreadControlBlock has the
    // correct layout with tls_data array at offset 0x80 that Rust std expects.

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
