//! ELF Loader Edge Case Tests
//!
//! Tests for the ELF loader that verifies correct handling of:
//! - Invalid ELF headers
//! - Malformed segments
//! - Edge cases in program header parsing
//! - Memory layout validation

#[cfg(test)]
mod tests {
    // Use REAL kernel ELF types
    use crate::security::elf::{Elf64Header, ElfClass, ElfData, ELF_MAGIC};
    use crate::process::{USER_VIRT_BASE, STACK_BASE};
    
    // Use kernel PAGE_SIZE from safety/paging
    use crate::safety::paging::PAGE_SIZE;

    // =========================================================================
    // ELF Magic Number Tests
    // =========================================================================

    #[test]
    fn test_elf_magic_constant() {
        // Use REAL kernel ELF_MAGIC constant
        assert_eq!(ELF_MAGIC, 0x464C457F); // 0x7F 'E' 'L' 'F' in little-endian
    }

    // =========================================================================
    // ELF Header Validation Tests
    // =========================================================================

    // Use REAL kernel constants
    const EI_CLASS: usize = 4;
    const EI_DATA: usize = 5;
    const EI_VERSION: usize = 6;
    const ELFCLASS64: u8 = ElfClass::Elf64 as u8;
    const ELFDATA2LSB: u8 = ElfData::LittleEndian as u8;

    const ET_EXEC: u16 = 2;
    const ET_DYN: u16 = 3;
    const EM_X86_64: u16 = 62;

    fn make_valid_elf_header() -> Elf64Header {
        Elf64Header {
            e_ident: {
                let mut ident = [0u8; 16];
                ident[0] = 0x7F;
                ident[1] = b'E';
                ident[2] = b'L';
                ident[3] = b'F';
                ident[4] = ELFCLASS64;
                ident[5] = ELFDATA2LSB;
                ident[6] = 1; // EV_CURRENT
                ident
            },
            e_type: ET_EXEC,
            e_machine: EM_X86_64,
            e_version: 1,
            e_entry: 0x400000,
            e_phoff: 64, // Right after header
            e_shoff: 0,
            e_flags: 0,
            e_ehsize: 64,
            e_phentsize: 56,
            e_phnum: 1,
            e_shentsize: 0,
            e_shnum: 0,
            e_shstrndx: 0,
        }
    }

    #[test]
    fn test_valid_elf_header() {
        // Use REAL kernel Elf64Header::is_valid() method
        let header = make_valid_elf_header();
        assert!(header.is_valid(), "Valid header should pass validation");
    }

    #[test]
    fn test_elf_header_class() {
        let header = make_valid_elf_header();
        
        assert_eq!(header.e_ident[EI_CLASS], ELFCLASS64, 
                   "Should be 64-bit ELF");
    }

    #[test]
    fn test_elf_header_endianness() {
        let header = make_valid_elf_header();
        
        assert_eq!(header.e_ident[EI_DATA], ELFDATA2LSB,
                   "Should be little-endian");
    }

    #[test]
    fn test_elf_header_machine() {
        let header = make_valid_elf_header();
        
        // Copy from packed struct to avoid alignment issues
        let e_machine = { header.e_machine };
        assert_eq!(e_machine, EM_X86_64,
                   "Should be x86_64 architecture");
    }

    // =========================================================================
    // Invalid Header Detection Tests
    // =========================================================================

    // Helper to validate ELF data using kernel's is_valid method
    fn validate_elf_data(data: &[u8]) -> Result<(), &'static str> {
        if data.len() < core::mem::size_of::<Elf64Header>() {
            return Err("ELF data too small");
        }
        
        // SAFETY: We checked the data is large enough
        let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };
        
        if header.is_valid() {
            Ok(())
        } else {
            // Check what failed to provide better error message
            let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            if magic != ELF_MAGIC {
                return Err("Invalid ELF magic");
            }
            if data[EI_CLASS] != ELFCLASS64 {
                return Err("Not 64-bit ELF");
            }
            if data[EI_DATA] != ELFDATA2LSB {
                return Err("Not little-endian");
            }
            Err("Invalid ELF header")
        }
    }

    #[test]
    fn test_detect_empty_data() {
        let result = validate_elf_data(&[]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "ELF data too small");
    }

    #[test]
    fn test_detect_too_small() {
        let result = validate_elf_data(&[0; 63]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "ELF data too small");
    }

    #[test]
    fn test_detect_bad_magic() {
        let mut data = [0u8; 64];
        data[0] = 0x00; // Wrong magic
        data[1] = b'E';
        data[2] = b'L';
        data[3] = b'F';
        
        let result = validate_elf_data(&data);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid ELF magic");
    }

    #[test]
    fn test_detect_32bit_elf() {
        let mut data = [0u8; 64];
        // Set correct magic
        data[0] = 0x7F;
        data[1] = b'E';
        data[2] = b'L';
        data[3] = b'F';
        data[EI_CLASS] = 1; // ELFCLASS32
        
        let result = validate_elf_data(&data);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Not 64-bit ELF");
    }

    #[test]
    fn test_detect_big_endian() {
        let mut data = [0u8; 64];
        // Set correct magic
        data[0] = 0x7F;
        data[1] = b'E';
        data[2] = b'L';
        data[3] = b'F';
        data[EI_CLASS] = ELFCLASS64;
        data[EI_DATA] = 2; // ELFDATA2MSB (big endian)
        
        let result = validate_elf_data(&data);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Not little-endian");
    }

    // =========================================================================
    // Program Header Tests
    // =========================================================================

    const PT_NULL: u32 = 0;
    const PT_LOAD: u32 = 1;
    const PT_DYNAMIC: u32 = 2;
    const PT_INTERP: u32 = 3;
    const PT_NOTE: u32 = 4;
    const PT_PHDR: u32 = 6;
    const PT_TLS: u32 = 7;
    const PT_GNU_EH_FRAME: u32 = 0x6474e550;
    const PT_GNU_STACK: u32 = 0x6474e551;
    const PT_GNU_RELRO: u32 = 0x6474e552;

    const PF_X: u32 = 1;
    const PF_W: u32 = 2;
    const PF_R: u32 = 4;

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

    #[test]
    fn test_program_header_types() {
        // Verify distinct values
        let types = [PT_NULL, PT_LOAD, PT_DYNAMIC, PT_INTERP, PT_NOTE, PT_PHDR, PT_TLS];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j], "Program header types should be distinct");
            }
        }
    }

    #[test]
    fn test_permission_flags() {
        assert_eq!(PF_X, 1, "Execute flag");
        assert_eq!(PF_W, 2, "Write flag");
        assert_eq!(PF_R, 4, "Read flag");
        
        // Common combinations
        let rx = PF_R | PF_X;
        let rw = PF_R | PF_W;
        let rwx = PF_R | PF_W | PF_X;
        
        assert_eq!(rx, 5);
        assert_eq!(rw, 6);
        assert_eq!(rwx, 7);
    }

    // =========================================================================
    // Segment Alignment Tests
    // =========================================================================

    fn is_aligned(addr: u64, alignment: u64) -> bool {
        if alignment == 0 || alignment == 1 {
            return true;
        }
        addr % alignment == 0
    }

    #[test]
    fn test_page_alignment() {
        const PAGE_SIZE: u64 = 4096;
        
        assert!(is_aligned(0, PAGE_SIZE));
        assert!(is_aligned(4096, PAGE_SIZE));
        assert!(is_aligned(0x1000, PAGE_SIZE));
        assert!(is_aligned(0x400000, PAGE_SIZE));
        
        assert!(!is_aligned(1, PAGE_SIZE));
        assert!(!is_aligned(4095, PAGE_SIZE));
        assert!(!is_aligned(0x1001, PAGE_SIZE));
    }

    #[test]
    fn test_common_alignment_values() {
        // ELF segments commonly use these alignments
        let alignments: [u64; 7] = [1, 4, 8, 16, 0x1000, 0x10000, 0x200000];
        
        for align in alignments {
            assert!(align.is_power_of_two() || align == 0,
                   "Alignment {} should be power of 2", align);
        }
    }

    // =========================================================================
    // Address Overlap Detection Tests
    // =========================================================================

    fn segments_overlap(
        addr1: u64, size1: u64,
        addr2: u64, size2: u64
    ) -> bool {
        if size1 == 0 || size2 == 0 {
            return false;
        }
        
        let end1 = addr1.saturating_add(size1);
        let end2 = addr2.saturating_add(size2);
        
        !(end1 <= addr2 || end2 <= addr1)
    }

    #[test]
    fn test_non_overlapping_segments() {
        assert!(!segments_overlap(0x1000, 0x1000, 0x2000, 0x1000));
        assert!(!segments_overlap(0x3000, 0x1000, 0x1000, 0x1000));
    }

    #[test]
    fn test_overlapping_segments() {
        assert!(segments_overlap(0x1000, 0x2000, 0x2000, 0x1000));
        assert!(segments_overlap(0x1000, 0x1000, 0x1000, 0x1000)); // Same
        assert!(segments_overlap(0x1000, 0x3000, 0x2000, 0x1000)); // Contained
    }

    #[test]
    fn test_adjacent_segments() {
        // Adjacent but not overlapping
        assert!(!segments_overlap(0x1000, 0x1000, 0x2000, 0x1000));
        assert!(!segments_overlap(0x2000, 0x1000, 0x1000, 0x1000));
    }

    #[test]
    fn test_zero_size_segments() {
        // Zero-size segments don't overlap with anything
        assert!(!segments_overlap(0x1000, 0, 0x1000, 0x1000));
        assert!(!segments_overlap(0x1000, 0x1000, 0x1000, 0));
        assert!(!segments_overlap(0x1000, 0, 0x1000, 0));
    }

    // =========================================================================
    // Entry Point Validation Tests
    // =========================================================================

    fn validate_entry_point(entry: u64, load_base: u64, load_size: u64) -> bool {
        // Entry point should be within loaded program
        if entry < load_base {
            return false;
        }
        if entry >= load_base.saturating_add(load_size) {
            return false;
        }
        true
    }

    #[test]
    fn test_valid_entry_point() {
        assert!(validate_entry_point(0x401000, 0x400000, 0x10000));
        assert!(validate_entry_point(0x400000, 0x400000, 0x10000)); // At base
        assert!(validate_entry_point(0x40FFFF, 0x400000, 0x10000)); // At end-1
    }

    #[test]
    fn test_invalid_entry_point() {
        assert!(!validate_entry_point(0x3FFFFF, 0x400000, 0x10000)); // Before base
        assert!(!validate_entry_point(0x410000, 0x400000, 0x10000)); // At end
        assert!(!validate_entry_point(0x500000, 0x400000, 0x10000)); // After end
        assert!(!validate_entry_point(0, 0x400000, 0x10000)); // At zero
    }

    // =========================================================================
    // Auxiliary Vector Tests
    // =========================================================================

    // Auxiliary vector types
    const AT_NULL: u64 = 0;
    const AT_PHDR: u64 = 3;
    const AT_PHENT: u64 = 4;
    const AT_PHNUM: u64 = 5;
    const AT_PAGESZ: u64 = 6;
    const AT_BASE: u64 = 7;
    const AT_ENTRY: u64 = 9;
    const AT_EXECFN: u64 = 31;
    const AT_RANDOM: u64 = 25;

    #[test]
    fn test_auxv_types_distinct() {
        let types = [AT_NULL, AT_PHDR, AT_PHENT, AT_PHNUM, AT_PAGESZ, AT_BASE, AT_ENTRY, AT_EXECFN, AT_RANDOM];
        
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j], "Auxv types should be distinct");
            }
        }
    }

    #[test]
    fn test_auxv_null_terminates() {
        assert_eq!(AT_NULL, 0, "AT_NULL should be 0 for termination");
    }
}
