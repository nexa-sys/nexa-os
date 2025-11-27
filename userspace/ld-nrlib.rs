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

// ============================================================================
// Dynamic Linker State
// ============================================================================

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
struct DynInfo {
    strtab: u64,
    symtab: u64,
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
}

impl DynInfo {
    const fn new() -> Self {
        Self {
            strtab: 0,
            symtab: 0,
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
        }
    }
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
    if aux_info.at_phdr == 0 || aux_info.at_phnum == 0 {
        print_str("[ld-nrlib] ERROR: No program headers from kernel\n");
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

    if dyn_addr != 0 {
        dyn_addr = (dyn_addr as i64 + load_bias) as u64;
        print_str("[ld-nrlib] PT_DYNAMIC at: ");
        print_hex(dyn_addr);
        print_str("\n");
        
        // Parse dynamic section
        let mut dyn_info = DynInfo::new();
        let mut dyn_ptr = dyn_addr as *const Elf64Dyn;
        
        loop {
            let entry = *dyn_ptr;
            if entry.d_tag == DT_NULL {
                break;
            }
            match entry.d_tag {
                DT_STRTAB => dyn_info.strtab = (entry.d_val as i64 + load_bias) as u64,
                DT_SYMTAB => dyn_info.symtab = (entry.d_val as i64 + load_bias) as u64,
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
                _ => {}
            }
            dyn_ptr = dyn_ptr.add(1);
        }

        // Process relocations
        if dyn_info.rela != 0 && dyn_info.relasz > 0 {
            print_str("[ld-nrlib] Processing RELA relocations...\n");
            process_rela(dyn_info.rela, dyn_info.relasz, dyn_info.relaent, load_bias);
        }
        
        if dyn_info.jmprel != 0 && dyn_info.pltrelsz > 0 {
            print_str("[ld-nrlib] Processing PLT relocations...\n");
            process_rela(dyn_info.jmprel, dyn_info.pltrelsz, 24, load_bias);
        }

        // Call init functions
        if dyn_info.init != 0 {
            print_str("[ld-nrlib] Calling DT_INIT at ");
            print_hex(dyn_info.init);
            print_str("\n");
            let init_fn: extern "C" fn() = core::mem::transmute(dyn_info.init);
            init_fn();
        }

        if dyn_info.init_array != 0 && dyn_info.init_arraysz > 0 {
            print_str("[ld-nrlib] Calling DT_INIT_ARRAY...\n");
            let count = dyn_info.init_arraysz / 8;
            let array = dyn_info.init_array as *const u64;
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
    let entry = aux_info.at_entry;
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

/// Process RELA relocations
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
                // These need symbol resolution - for now skip
                // TODO: Implement proper symbol resolution
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
