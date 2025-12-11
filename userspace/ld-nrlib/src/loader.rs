//! ELF loader functions for the NexaOS dynamic linker

use crate::constants::*;
use crate::elf::{Elf64Dyn, Elf64Ehdr, Elf64Phdr};
use crate::helpers::{cstr_len, is_libc_library, map_library_name, memset, page_align_down, page_align_up};
use crate::reloc::process_rela;
use crate::state::{DynInfo, GLOBAL_SYMTAB};
use crate::syscall::{close_file, lseek, mmap, open_file, read_bytes};
use crate::tls::register_tls_module;

// ============================================================================
// Dynamic Section Parsing
// ============================================================================

/// Parse dynamic section and fill DynInfo
pub unsafe fn parse_dynamic_section(dyn_addr: u64, load_bias: i64, dyn_info: &mut DynInfo) {
    let mut dyn_ptr = dyn_addr as *const Elf64Dyn;

    loop {
        let entry = *dyn_ptr;
        if entry.d_tag == DT_NULL {
            break;
        }
        match entry.d_tag {
            DT_STRTAB => dyn_info.strtab = (entry.d_val as i64 + load_bias) as u64,
            DT_SYMTAB => dyn_info.symtab = (entry.d_val as i64 + load_bias) as u64,
            DT_STRSZ => dyn_info.strsz = entry.d_val,
            DT_SYMENT => dyn_info.syment = entry.d_val,
            DT_RELA => dyn_info.rela = (entry.d_val as i64 + load_bias) as u64,
            DT_RELASZ => dyn_info.relasz = entry.d_val,
            DT_RELAENT => dyn_info.relaent = entry.d_val,
            DT_RELACOUNT => dyn_info.relacount = entry.d_val,
            DT_JMPREL => dyn_info.jmprel = (entry.d_val as i64 + load_bias) as u64,
            DT_PLTRELSZ => dyn_info.pltrelsz = entry.d_val,
            DT_PLTREL => dyn_info.pltrel = entry.d_val,
            DT_INIT => dyn_info.init = (entry.d_val as i64 + load_bias) as u64,
            DT_FINI => dyn_info.fini = (entry.d_val as i64 + load_bias) as u64,
            DT_INIT_ARRAY => dyn_info.init_array = (entry.d_val as i64 + load_bias) as u64,
            DT_INIT_ARRAYSZ => dyn_info.init_arraysz = entry.d_val,
            DT_FINI_ARRAY => dyn_info.fini_array = (entry.d_val as i64 + load_bias) as u64,
            DT_FINI_ARRAYSZ => dyn_info.fini_arraysz = entry.d_val,
            DT_PREINIT_ARRAY => dyn_info.preinit_array = (entry.d_val as i64 + load_bias) as u64,
            DT_PREINIT_ARRAYSZ => dyn_info.preinit_arraysz = entry.d_val,
            DT_HASH => dyn_info.hash = (entry.d_val as i64 + load_bias) as u64,
            DT_GNU_HASH => dyn_info.gnu_hash = (entry.d_val as i64 + load_bias) as u64,
            DT_FLAGS => dyn_info.flags = entry.d_val,
            DT_FLAGS_1 => dyn_info.flags_1 = entry.d_val,
            DT_VERSYM => dyn_info.versym = (entry.d_val as i64 + load_bias) as u64,
            DT_VERNEED => dyn_info.verneed = (entry.d_val as i64 + load_bias) as u64,
            DT_VERNEEDNUM => dyn_info.verneednum = entry.d_val,
            DT_NEEDED => {
                if dyn_info.needed_count < 16 {
                    dyn_info.needed[dyn_info.needed_count] = entry.d_val;
                    dyn_info.needed_count += 1;
                }
            }
            _ => {}
        }
        dyn_ptr = dyn_ptr.add(1);
    }
}

// ============================================================================
// Library Search
// ============================================================================

/// Search for a library in standard paths
pub unsafe fn search_library(name: &[u8]) -> Option<[u8; 256]> {
    let mut path_buf = [0u8; 256];

    // Use stack-local array to avoid global pointer relocation issues
    let search_paths: [&[u8]; 4] = [
        LIB_PATH_1.as_slice(),
        LIB_PATH_2.as_slice(),
        LIB_PATH_3.as_slice(),
        LIB_PATH_4.as_slice(),
    ];

    let mut path_idx = 0usize;
    while path_idx < 4 {
        let search_path = search_paths[path_idx];
        // Build path: search_path + "/" + name
        let mut pos = 0;

        // Copy search path (without null terminator)
        let mut i = 0;
        while i < search_path.len() && search_path[i] != 0 {
            if pos < 255 {
                path_buf[pos] = search_path[i];
                pos += 1;
            }
            i += 1;
        }

        // Add separator
        if pos < 255 {
            path_buf[pos] = b'/';
            pos += 1;
        }

        // Copy name
        for &c in name {
            if c == 0 {
                break;
            }
            if pos < 255 {
                path_buf[pos] = c;
                pos += 1;
            }
        }

        // Null terminate
        path_buf[pos] = 0;

        // Try to open to check if it exists
        let fd = open_file(path_buf.as_ptr());
        if fd >= 0 {
            close_file(fd as i32);
            return Some(path_buf);
        }

        path_idx += 1;
    }

    None
}

// ============================================================================
// Shared Library Loading
// ============================================================================

/// Load a shared library from path
/// Returns (base_addr, load_bias, dyn_info) or (0, 0, DynInfo::new()) on failure
pub unsafe fn load_shared_library(path: *const u8) -> (u64, i64, DynInfo) {
    let fd = open_file(path);
    if fd < 0 {
        return (0, 0, DynInfo::new());
    }

    // Read ELF header
    let mut ehdr_buf = [0u8; 64];

    let bytes_read = read_bytes(fd as i32, ehdr_buf.as_mut_ptr(), 64);

    if bytes_read < 64 {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }

    let ehdr = &*(ehdr_buf.as_ptr() as *const Elf64Ehdr);

    // Validate ELF magic
    if ehdr.e_ident[0] != 0x7f
        || ehdr.e_ident[1] != b'E'
        || ehdr.e_ident[2] != b'L'
        || ehdr.e_ident[3] != b'F'
    {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }

    // Must be shared object (ET_DYN = 3)
    if ehdr.e_type != 3 {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }

    // Read program headers
    let phdr_size = (ehdr.e_phentsize as usize) * (ehdr.e_phnum as usize);
    if phdr_size > 2048 {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }

    lseek(fd as i32, ehdr.e_phoff as i64, 0); // SEEK_SET
    let mut phdr_buf = [0u8; 2048];
    let bytes_read = read_bytes(fd as i32, phdr_buf.as_mut_ptr(), phdr_size);
    if bytes_read < phdr_size as isize {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }

    let phdrs =
        core::slice::from_raw_parts(phdr_buf.as_ptr() as *const Elf64Phdr, ehdr.e_phnum as usize);

    // Find extent of loadable segments and TLS segment
    let mut load_addr_min: u64 = u64::MAX;
    let mut load_addr_max: u64 = 0;
    let mut dyn_vaddr: u64 = 0;
    let mut tls_phdr: Option<Elf64Phdr> = None;

    for phdr in phdrs {
        if phdr.p_type == PT_LOAD {
            let seg_start = page_align_down(phdr.p_vaddr);
            let seg_end = page_align_up(phdr.p_vaddr + phdr.p_memsz);
            if seg_start < load_addr_min {
                load_addr_min = seg_start;
            }
            if seg_end > load_addr_max {
                load_addr_max = seg_end;
            }
        }
        if phdr.p_type == PT_DYNAMIC {
            dyn_vaddr = phdr.p_vaddr;
        }
        if phdr.p_type == PT_TLS {
            tls_phdr = Some(*phdr);
        }
    }

    if load_addr_min == u64::MAX {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }

    let total_size = load_addr_max - load_addr_min;

    // Allocate memory for the library
    let base_addr = mmap(
        0,                                  // addr
        total_size,                         // length
        PROT_READ | PROT_WRITE | PROT_EXEC, // prot
        MAP_PRIVATE | MAP_ANONYMOUS,        // flags
        -1,                                 // fd = -1 for anonymous mapping
        0,                                  // offset
    );

    // Check for mmap failure
    if base_addr >= 0xFFFF_FFFF_FFFF_F000 || base_addr == 0 {
        close_file(fd as i32);
        return (0, 0, DynInfo::new());
    }

    let load_bias = base_addr as i64 - load_addr_min as i64;

    // Load each PT_LOAD segment
    for phdr in phdrs {
        if phdr.p_type != PT_LOAD {
            continue;
        }

        if phdr.p_filesz > 0 {
            // Seek to segment in file
            lseek(fd as i32, phdr.p_offset as i64, 0);

            // Read segment data
            let dest_addr = (phdr.p_vaddr as i64 + load_bias) as *mut u8;
            let mut total_read: u64 = 0;
            while total_read < phdr.p_filesz {
                let to_read = core::cmp::min(phdr.p_filesz - total_read, 4096) as usize;
                let read = read_bytes(fd as i32, dest_addr.add(total_read as usize), to_read);
                if read <= 0 {
                    break;
                }
                total_read += read as u64;
            }
        }

        // Zero BSS (memsz > filesz)
        if phdr.p_memsz > phdr.p_filesz {
            let bss_start = ((phdr.p_vaddr + phdr.p_filesz) as i64 + load_bias) as *mut u8;
            let bss_size = (phdr.p_memsz - phdr.p_filesz) as usize;
            memset(bss_start, 0, bss_size);
        }
    }

    close_file(fd as i32);

    // Parse dynamic section
    let mut dyn_info = DynInfo::new();
    if dyn_vaddr != 0 {
        let dyn_addr = (dyn_vaddr as i64 + load_bias) as u64;
        parse_dynamic_section(dyn_addr, load_bias, &mut dyn_info);
    }

    // Register TLS module if PT_TLS segment exists
    if let Some(tls) = tls_phdr {
        let tls_image = (tls.p_vaddr as i64 + load_bias) as u64;
        let tls_mod_id = register_tls_module(tls_image, tls.p_filesz, tls.p_memsz, tls.p_align);
        dyn_info.tls_modid = tls_mod_id;
    }

    (base_addr, load_bias, dyn_info)
}

// ============================================================================
// Recursive Library Loading
// ============================================================================

/// Load a library and recursively load its dependencies
/// Returns true if library was loaded successfully (or already loaded)
pub unsafe fn load_library_recursive(name: &[u8]) -> bool {
    // Skip libc-like libraries that map to libnrlib.so
    if name.len() > 0 {
        // Convert to null-terminated for is_libc_library check
        let mut name_buf = [0u8; 128];
        let copy_len = core::cmp::min(name.len(), 127);
        for i in 0..copy_len {
            name_buf[i] = name[i];
        }
        name_buf[copy_len] = 0;

        if is_libc_library(name_buf.as_ptr()) {
            return true;
        }
    }

    // Try mapped name first, then original name
    let mapped_name = map_library_name(name);

    let path = if let Some(p) = search_library(&mapped_name) {
        p
    } else if let Some(p) = search_library(name) {
        p
    } else {
        return false;
    };

    // Load the library
    let (lib_base, lib_bias, lib_dyn_info) = load_shared_library(path.as_ptr());
    if lib_base == 0 {
        return false;
    }

    // Register in global symbol table
    let lib_idx = GLOBAL_SYMTAB.lib_count;
    if lib_idx >= MAX_LIBS {
        return false;
    }

    let lib = &mut GLOBAL_SYMTAB.libs[lib_idx];
    lib.base_addr = lib_base;
    lib.load_bias = lib_bias;
    lib.dyn_info = lib_dyn_info;
    lib.valid = true;
    GLOBAL_SYMTAB.lib_count = lib_idx + 1;

    // Process library's RELATIVE relocations first
    if lib_dyn_info.rela != 0 && lib_dyn_info.relasz > 0 {
        process_rela(
            lib_dyn_info.rela,
            lib_dyn_info.relasz,
            lib_dyn_info.relaent,
            lib_bias,
        );
    }

    // Recursively load this library's dependencies
    if lib_dyn_info.needed_count > 0 {
        for i in 0..lib_dyn_info.needed_count {
            let dep_name_offset = lib_dyn_info.needed[i];
            let dep_name_ptr = (lib_dyn_info.strtab + dep_name_offset) as *const u8;
            let dep_name_len = cstr_len(dep_name_ptr);
            let dep_name_slice = core::slice::from_raw_parts(dep_name_ptr, dep_name_len);

            // Recursively load dependency
            load_library_recursive(dep_name_slice);
        }
    }

    true
}
