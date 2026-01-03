//! ELF Loader Security and Validation Tests
//!
//! Tests for ELF parsing security, bounds checking, and attack prevention.
//! Uses REAL kernel ELF types from security/elf module.

#[cfg(test)]
mod tests {
    use crate::process::{
        USER_VIRT_BASE, HEAP_BASE, STACK_BASE, INTERP_BASE, INTERP_REGION_SIZE,
    };
    // Import REAL kernel ELF types
    use crate::security::elf::{
        ELF_MAGIC, ElfClass, ElfData, ElfType, PhType, 
        Elf64Header, Elf64ProgramHeader, ph_flags,
    };
    use crate::safety::paging::{is_user_address, is_kernel_address};

    // Local constants for compatibility (should match kernel)
    const ELFCLASS64: u8 = ElfClass::Elf64 as u8;
    const ELFCLASS32: u8 = ElfClass::Elf32 as u8;
    const ELFDATA2LSB: u8 = ElfData::LittleEndian as u8;
    const ELFDATA2MSB: u8 = ElfData::BigEndian as u8;
    const ET_EXEC: u16 = ElfType::Executable as u16;
    const ET_DYN: u16 = ElfType::Shared as u16;
    const PT_LOAD: u32 = PhType::Load as u32;
    const PT_INTERP: u32 = PhType::Interp as u32;
    const EM_X86_64: u16 = 62;

    // =========================================================================
    // ELF Header Validation Tests
    // =========================================================================

    #[test]
    fn test_elf_magic_validation() {
        // ELF_MAGIC is u32 = 0x464C457F ('\x7FELF' in little-endian)
        fn validate_magic(data: &[u8]) -> bool {
            if data.len() < 4 {
                return false;
            }
            let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            magic == ELF_MAGIC
        }
        
        let valid_elf = [0x7F, b'E', b'L', b'F', 0x02, 0x01, 0x01, 0x00];
        let invalid_elf = [0x7F, b'E', b'L', b'X', 0x02, 0x01, 0x01, 0x00];
        let too_short = [0x7F, b'E', b'L'];
        
        assert!(validate_magic(&valid_elf));
        assert!(!validate_magic(&invalid_elf));
        assert!(!validate_magic(&too_short));
    }

    #[test]
    fn test_elf_class_validation() {
        fn validate_class(class: u8) -> bool {
            class == ELFCLASS64 // Only support 64-bit
        }
        
        assert!(validate_class(ELFCLASS64));
        assert!(!validate_class(ELFCLASS32)); // 32-bit not supported
        assert!(!validate_class(0)); // Invalid
    }

    #[test]
    fn test_elf_encoding_validation() {
        fn validate_encoding(encoding: u8) -> bool {
            encoding == ELFDATA2LSB // x86_64 is little-endian
        }
        
        assert!(validate_encoding(ELFDATA2LSB));
        assert!(!validate_encoding(ELFDATA2MSB)); // Big-endian not supported
    }

    #[test]
    fn test_elf_type_validation() {
        fn validate_type(elf_type: u16) -> bool {
            elf_type == ET_EXEC || elf_type == ET_DYN
        }
        
        assert!(validate_type(ET_EXEC));
        assert!(validate_type(ET_DYN)); // PIE executable
        assert!(!validate_type(0)); // ET_NONE
        assert!(!validate_type(1)); // ET_REL (relocatable, not executable)
    }

    #[test]
    fn test_elf_machine_validation() {
        fn validate_machine(machine: u16) -> bool {
            machine == EM_X86_64
        }
        
        assert!(validate_machine(EM_X86_64));
        assert!(!validate_machine(3)); // EM_386 (32-bit)
        assert!(!validate_machine(0xB7)); // EM_AARCH64
    }

    // =========================================================================
    // Program Header Validation Tests
    // =========================================================================

    #[test]
    fn test_phdr_count_limit() {
        // Limit number of program headers to prevent DoS
        const MAX_PHDRS: usize = 64;
        
        fn validate_phdr_count(count: u16) -> bool {
            count as usize <= MAX_PHDRS && count > 0
        }
        
        assert!(validate_phdr_count(1));
        assert!(validate_phdr_count(64));
        assert!(!validate_phdr_count(0)); // Need at least one
        assert!(!validate_phdr_count(100)); // Too many
    }

    #[test]
    fn test_phdr_size_validation() {
        // Program header size must be correct for ELF64
        const ELF64_PHDR_SIZE: u16 = 56;
        
        fn validate_phdr_size(size: u16) -> bool {
            size == ELF64_PHDR_SIZE
        }
        
        assert!(validate_phdr_size(56));
        assert!(!validate_phdr_size(32)); // Wrong size
        assert!(!validate_phdr_size(0));
    }

    #[test]
    fn test_load_segment_vaddr_bounds() {
        // LOAD segments must be within user space
        fn validate_vaddr(vaddr: u64, memsz: u64) -> bool {
            let end = match vaddr.checked_add(memsz) {
                Some(e) => e,
                None => return false,
            };
            
            vaddr >= USER_VIRT_BASE && end <= INTERP_BASE + INTERP_REGION_SIZE
        }
        
        assert!(validate_vaddr(USER_VIRT_BASE, 0x1000));
        assert!(validate_vaddr(USER_VIRT_BASE + 0x1000, 0x10000));
        assert!(!validate_vaddr(0, 0x1000), "Below user space");
        assert!(!validate_vaddr(0xFFFF_8000_0000_0000, 0x1000), "Kernel space");
        assert!(!validate_vaddr(u64::MAX, 1), "Would overflow");
    }

    #[test]
    fn test_load_segment_offset_bounds() {
        // File offset + size must not exceed file size
        fn validate_segment_offset(offset: u64, filesz: u64, file_size: u64) -> bool {
            match offset.checked_add(filesz) {
                Some(end) => end <= file_size,
                None => false,
            }
        }
        
        let file_size: u64 = 10000;
        
        assert!(validate_segment_offset(0, 1000, file_size));
        assert!(validate_segment_offset(5000, 5000, file_size));
        assert!(!validate_segment_offset(9000, 2000, file_size), "Exceeds file");
        assert!(!validate_segment_offset(u64::MAX, 1, file_size), "Overflow");
    }

    #[test]
    fn test_memsz_vs_filesz() {
        // memsz >= filesz (extra is .bss zeroed space)
        fn validate_segment_sizes(filesz: u64, memsz: u64) -> bool {
            memsz >= filesz
        }
        
        assert!(validate_segment_sizes(1000, 1000)); // Equal
        assert!(validate_segment_sizes(1000, 2000)); // BSS present
        assert!(!validate_segment_sizes(2000, 1000)); // Invalid
    }

    // =========================================================================
    // Security Validation Tests
    // =========================================================================

    #[test]
    fn test_entry_point_validation() {
        // Entry point must be within loaded segments
        fn validate_entry(entry: u64, load_base: u64, load_size: u64) -> bool {
            entry >= load_base && entry < load_base + load_size
        }
        
        let load_base = USER_VIRT_BASE;
        let load_size: u64 = 0x10000;
        
        assert!(validate_entry(load_base + 0x1000, load_base, load_size));
        assert!(!validate_entry(load_base - 1, load_base, load_size), "Before load");
        assert!(!validate_entry(load_base + load_size, load_base, load_size), "After load");
    }

    #[test]
    fn test_interpreter_path_length() {
        // PT_INTERP path must be reasonable length
        const MAX_INTERP_PATH: usize = 256;
        
        fn validate_interp_length(path: &[u8]) -> bool {
            !path.is_empty() && path.len() <= MAX_INTERP_PATH
        }
        
        assert!(validate_interp_length(b"/lib64/ld-linux-x86-64.so.2"));
        assert!(validate_interp_length(b"/lib64/ld-nrlib-x86_64.so.1"));
        assert!(!validate_interp_length(b""), "Empty path");
        
        let long_path = vec![b'a'; 300];
        assert!(!validate_interp_length(&long_path), "Too long");
    }

    #[test]
    fn test_interpreter_path_null_terminated() {
        // Interpreter path must be null-terminated
        fn is_null_terminated(data: &[u8]) -> bool {
            data.last() == Some(&0)
        }
        
        assert!(is_null_terminated(b"/lib64/ld-linux.so\0"));
        assert!(!is_null_terminated(b"/lib64/ld-linux.so"));
    }

    #[test]
    fn test_no_overlapping_segments() {
        // LOAD segments must not overlap
        fn segments_overlap(
            a_addr: u64, a_size: u64,
            b_addr: u64, b_size: u64
        ) -> bool {
            let a_end = a_addr + a_size;
            let b_end = b_addr + b_size;
            
            !(a_end <= b_addr || b_end <= a_addr)
        }
        
        // Non-overlapping
        assert!(!segments_overlap(0x1000, 0x1000, 0x2000, 0x1000));
        assert!(!segments_overlap(0x2000, 0x1000, 0x1000, 0x1000));
        
        // Overlapping
        assert!(segments_overlap(0x1000, 0x2000, 0x2000, 0x1000));
        assert!(segments_overlap(0x1500, 0x1000, 0x1000, 0x1000));
    }

    #[test]
    fn test_no_kernel_space_mapping() {
        // No segments should map into kernel space
        const KERNEL_START: u64 = 0xFFFF_8000_0000_0000;
        
        fn is_kernel_address(addr: u64) -> bool {
            addr >= KERNEL_START
        }
        
        fn validate_no_kernel(vaddr: u64, memsz: u64) -> bool {
            let end = vaddr.saturating_add(memsz);
            !is_kernel_address(vaddr) && !is_kernel_address(end)
        }
        
        assert!(validate_no_kernel(USER_VIRT_BASE, 0x1000));
        assert!(!validate_no_kernel(KERNEL_START, 0x1000));
        assert!(!validate_no_kernel(KERNEL_START - 0x1000, 0x2000)); // Crosses boundary
    }

    // =========================================================================
    // Segment Permission Tests
    // =========================================================================

    #[test]
    fn test_segment_flags() {
        const PF_X: u32 = 1; // Execute
        const PF_W: u32 = 2; // Write
        const PF_R: u32 = 4; // Read
        
        // Typical segment permissions
        let code_flags: u32 = PF_R | PF_X;        // .text
        let data_flags: u32 = PF_R | PF_W;        // .data
        let rodata_flags: u32 = PF_R;             // .rodata
        
        assert_ne!(code_flags & PF_R, 0);
        assert_ne!(code_flags & PF_X, 0);
        assert_eq!(code_flags & PF_W, 0); // Code not writable
        
        assert_ne!(data_flags & PF_R, 0);
        assert_ne!(data_flags & PF_W, 0);
        assert_eq!(data_flags & PF_X, 0); // Data not executable (W^X)
    }

    #[test]
    fn test_wx_policy() {
        // W^X: Pages should not be both writable AND executable
        const PF_X: u32 = 1;
        const PF_W: u32 = 2;
        
        fn is_wx_safe(flags: u32) -> bool {
            // Allow either W or X, but not both
            !((flags & PF_W) != 0 && (flags & PF_X) != 0)
        }
        
        assert!(is_wx_safe(PF_X)); // Execute-only
        assert!(is_wx_safe(PF_W)); // Write-only
        assert!(is_wx_safe(0));    // No permissions
        assert!(!is_wx_safe(PF_W | PF_X)); // W+X is dangerous
    }

    // =========================================================================
    // Auxiliary Vector Tests
    // =========================================================================

    #[test]
    fn test_auxv_types() {
        const AT_NULL: u64 = 0;
        const AT_PHDR: u64 = 3;
        const AT_PHENT: u64 = 4;
        const AT_PHNUM: u64 = 5;
        const AT_PAGESZ: u64 = 6;
        const AT_BASE: u64 = 7;
        const AT_ENTRY: u64 = 9;
        const AT_UID: u64 = 11;
        const AT_GID: u64 = 13;
        const AT_RANDOM: u64 = 25;
        
        // Verify distinct values
        let types = [AT_NULL, AT_PHDR, AT_PHENT, AT_PHNUM, AT_PAGESZ, 
                    AT_BASE, AT_ENTRY, AT_UID, AT_GID, AT_RANDOM];
        
        for i in 0..types.len() {
            for j in i+1..types.len() {
                assert_ne!(types[i], types[j], "Duplicate auxv type");
            }
        }
    }

    #[test]
    fn test_auxv_termination() {
        // Auxv must end with AT_NULL
        #[derive(Clone, Copy)]
        struct Auxv {
            a_type: u64,
            a_val: u64,
        }
        
        let auxv = [
            Auxv { a_type: 6, a_val: 4096 },   // AT_PAGESZ
            Auxv { a_type: 9, a_val: 0x1000 }, // AT_ENTRY
            Auxv { a_type: 0, a_val: 0 },      // AT_NULL
        ];
        
        assert_eq!(auxv.last().unwrap().a_type, 0);
    }

    // =========================================================================
    // Stack Setup Tests
    // =========================================================================

    #[test]
    fn test_stack_alignment() {
        // x86_64 ABI requires 16-byte stack alignment before call
        fn is_stack_aligned(sp: u64) -> bool {
            sp % 16 == 0
        }
        
        assert!(is_stack_aligned(0x7FFF_FFF0));
        assert!(!is_stack_aligned(0x7FFF_FFF8));
    }

    #[test]
    fn test_argc_argv_layout() {
        // Stack layout: argc, argv[0..argc], NULL, envp[0..], NULL, auxv[0..], AT_NULL
        
        fn validate_stack_layout(stack: &[u64]) -> bool {
            if stack.is_empty() {
                return false;
            }
            
            let argc = stack[0];
            let argv_end_idx = 1 + argc as usize;
            
            // Check NULL terminator after argv
            if argv_end_idx >= stack.len() {
                return false;
            }
            
            stack[argv_end_idx] == 0
        }
        
        // argc=2, argv=["prog", "arg"], NULL
        let valid_stack: Vec<u64> = vec![
            2,          // argc
            0x1000,     // argv[0]
            0x1010,     // argv[1]
            0,          // NULL terminator
        ];
        
        assert!(validate_stack_layout(&valid_stack));
    }

    // =========================================================================
    // Integer Overflow Protection Tests
    // =========================================================================

    #[test]
    fn test_size_multiplication_overflow() {
        // phnum * phentsize could overflow
        fn safe_phdr_table_size(phnum: u16, phentsize: u16) -> Option<usize> {
            (phnum as usize).checked_mul(phentsize as usize)
        }
        
        assert_eq!(safe_phdr_table_size(10, 56), Some(560));
        // Large values that might overflow
        assert!(safe_phdr_table_size(u16::MAX, u16::MAX).is_some()); // Still fits in usize
    }

    #[test]
    fn test_address_addition_overflow() {
        fn safe_address_add(base: u64, offset: u64) -> Option<u64> {
            base.checked_add(offset)
        }
        
        assert_eq!(safe_address_add(0x1000, 0x1000), Some(0x2000));
        assert!(safe_address_add(u64::MAX, 1).is_none());
        assert!(safe_address_add(u64::MAX - 10, 20).is_none());
    }
}
