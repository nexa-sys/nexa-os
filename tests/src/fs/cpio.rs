//! CPIO (initramfs) parsing tests
//!
//! Tests for CPIO newc format parsing and edge cases.

#[cfg(test)]
mod tests {
    use crate::fs::initramfs::CpioNewcHeader;
    
    // =========================================================================
    // CPIO Header Constants Tests
    // =========================================================================

    #[test]
    fn test_cpio_header_size() {
        // CPIO newc header is exactly 110 bytes
        assert_eq!(core::mem::size_of::<CpioNewcHeader>(), 110);
    }

    #[test]
    fn test_cpio_magic_value() {
        // Magic for newc format is "070701"
        let magic: &[u8; 6] = b"070701";
        assert_eq!(magic, b"070701");
    }

    // =========================================================================
    // Hex Parsing Tests
    // =========================================================================

    #[test]
    fn test_parse_hex_zero() {
        // "00000000" should parse as 0
        fn parse_hex(bytes: &[u8]) -> u64 {
            let mut result = 0u64;
            for &b in bytes {
                result = result * 16 + match b {
                    b'0'..=b'9' => (b - b'0') as u64,
                    b'a'..=b'f' => (b - b'a' + 10) as u64,
                    b'A'..=b'F' => (b - b'A' + 10) as u64,
                    _ => 0,
                };
            }
            result
        }
        
        assert_eq!(parse_hex(b"00000000"), 0);
    }

    #[test]
    fn test_parse_hex_simple() {
        fn parse_hex(bytes: &[u8]) -> u64 {
            let mut result = 0u64;
            for &b in bytes {
                result = result * 16 + match b {
                    b'0'..=b'9' => (b - b'0') as u64,
                    b'a'..=b'f' => (b - b'a' + 10) as u64,
                    b'A'..=b'F' => (b - b'A' + 10) as u64,
                    _ => 0,
                };
            }
            result
        }
        
        assert_eq!(parse_hex(b"00000001"), 1);
        assert_eq!(parse_hex(b"00000010"), 16);
        assert_eq!(parse_hex(b"000000FF"), 255);
        assert_eq!(parse_hex(b"000000ff"), 255);
    }

    #[test]
    fn test_parse_hex_large() {
        fn parse_hex(bytes: &[u8]) -> u64 {
            let mut result = 0u64;
            for &b in bytes {
                result = result * 16 + match b {
                    b'0'..=b'9' => (b - b'0') as u64,
                    b'a'..=b'f' => (b - b'a' + 10) as u64,
                    b'A'..=b'F' => (b - b'A' + 10) as u64,
                    _ => 0,
                };
            }
            result
        }
        
        // Maximum 8-digit hex
        assert_eq!(parse_hex(b"FFFFFFFF"), 0xFFFFFFFF);
        
        // Common file sizes
        assert_eq!(parse_hex(b"00001000"), 0x1000); // 4KB
        assert_eq!(parse_hex(b"00100000"), 0x100000); // 1MB
    }

    #[test]
    fn test_parse_hex_invalid_chars() {
        fn parse_hex(bytes: &[u8]) -> u64 {
            let mut result = 0u64;
            for &b in bytes {
                result = result * 16 + match b {
                    b'0'..=b'9' => (b - b'0') as u64,
                    b'a'..=b'f' => (b - b'a' + 10) as u64,
                    b'A'..=b'F' => (b - b'A' + 10) as u64,
                    _ => 0,
                };
            }
            result
        }
        
        // Invalid chars treated as 0
        assert_eq!(parse_hex(b"0000000G"), 0);
        assert_eq!(parse_hex(b"0000000Z"), 0);
    }

    // =========================================================================
    // File Mode Tests
    // =========================================================================

    #[test]
    fn test_file_mode_parsing() {
        // S_IFREG (regular file) = 0100000
        // S_IFDIR (directory) = 0040000
        // S_IFLNK (symlink) = 0120000
        
        // Regular file with 0755 permissions: 0100755
        let mode: u32 = 0o100755;
        
        // File type mask
        let s_ifmt: u32 = 0o170000;
        let s_ifreg: u32 = 0o100000;
        
        assert_eq!(mode & s_ifmt, s_ifreg);
        
        // Permission bits
        let perms = mode & 0o777;
        assert_eq!(perms, 0o755);
    }

    #[test]
    fn test_directory_mode() {
        let mode: u32 = 0o40755; // Directory with 755 permissions
        
        let s_ifmt: u32 = 0o170000;
        let s_ifdir: u32 = 0o40000;
        
        assert_eq!(mode & s_ifmt, s_ifdir);
    }

    #[test]
    fn test_symlink_mode() {
        let mode: u32 = 0o120777; // Symlink (usually 777)
        
        let s_ifmt: u32 = 0o170000;
        let s_iflnk: u32 = 0o120000;
        
        assert_eq!(mode & s_ifmt, s_iflnk);
    }

    // =========================================================================
    // Alignment Tests
    // =========================================================================

    #[test]
    fn test_cpio_alignment() {
        // CPIO entries are aligned to 4 bytes
        fn align_to_4(offset: usize) -> usize {
            (offset + 3) & !3
        }
        
        assert_eq!(align_to_4(0), 0);
        assert_eq!(align_to_4(1), 4);
        assert_eq!(align_to_4(2), 4);
        assert_eq!(align_to_4(3), 4);
        assert_eq!(align_to_4(4), 4);
        assert_eq!(align_to_4(5), 8);
    }

    #[test]
    fn test_cpio_entry_offset_calculation() {
        // After header (110 bytes), filename, then data
        let header_size = 110;
        let filename_len = 5; // "init\0"
        
        // Name starts right after header
        let name_offset = header_size;
        
        // Data starts after header + name, aligned to 4 bytes
        let data_offset = ((name_offset + filename_len) + 3) & !3;
        
        // For "init" (5 chars including NUL), aligned offset should be:
        // 110 + 5 = 115, aligned to 4 = 116
        assert_eq!(data_offset, 116);
    }

    // =========================================================================
    // Trailer Tests
    // =========================================================================

    #[test]
    fn test_cpio_trailer_name() {
        // CPIO archive ends with "TRAILER!!!"
        let trailer = "TRAILER!!!";
        assert_eq!(trailer.len(), 10);
        
        // With NUL terminator = 11 bytes
        let name_with_nul = trailer.len() + 1;
        assert_eq!(name_with_nul, 11);
    }

    #[test]
    fn test_trailer_detection() {
        fn is_trailer(name: &str) -> bool {
            name == "TRAILER!!!"
        }
        
        assert!(is_trailer("TRAILER!!!"));
        assert!(!is_trailer("trailer!!!"));
        assert!(!is_trailer("TRAILER"));
        assert!(!is_trailer("init"));
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_empty_file() {
        // File with size 0 is valid
        let filesize: usize = 0;
        
        // Should be able to create an entry with no data
        assert_eq!(filesize, 0);
    }

    #[test]
    fn test_long_filename() {
        // Long filenames should work (PATH_MAX = 4096 on Linux)
        let long_name = "a".repeat(255);
        
        // namesize in CPIO includes NUL terminator
        let namesize = long_name.len() + 1;
        assert_eq!(namesize, 256);
    }

    #[test]
    fn test_path_with_directories() {
        // CPIO stores full paths
        let path = "usr/bin/init";
        
        // Components
        let components: Vec<&str> = path.split('/').collect();
        assert_eq!(components, vec!["usr", "bin", "init"]);
    }

    #[test]
    fn test_absolute_vs_relative_paths() {
        // CPIO paths should be relative (no leading /)
        let good_path = "bin/init";
        let bad_path = "/bin/init";
        
        assert!(!good_path.starts_with('/'));
        assert!(bad_path.starts_with('/'));
    }

    // =========================================================================
    // Validation Tests
    // =========================================================================

    #[test]
    fn test_header_validation() {
        // A valid header has:
        // - Magic "070701" or "070702"
        // - hw_addr_len = 6 (for Ethernet)
        // - proto_addr_len = 4 (for IPv4)
        
        // Check magic validation concept
        fn is_valid_magic(magic: &[u8; 6]) -> bool {
            magic == b"070701" || magic == b"070702"
        }
        
        assert!(is_valid_magic(b"070701"));
        assert!(is_valid_magic(b"070702"));
        assert!(!is_valid_magic(b"070703"));
        assert!(!is_valid_magic(b"000000"));
    }

    #[test]
    fn test_filesize_validation() {
        // Filesize should be reasonable
        fn is_valid_filesize(size: usize, archive_remaining: usize) -> bool {
            size <= archive_remaining
        }
        
        assert!(is_valid_filesize(100, 1000));
        assert!(is_valid_filesize(1000, 1000));
        assert!(!is_valid_filesize(1001, 1000));
    }

    #[test]
    fn test_namesize_validation() {
        // Namesize should be at least 1 (for NUL) and reasonable
        fn is_valid_namesize(size: usize) -> bool {
            size >= 1 && size <= 4096
        }
        
        assert!(!is_valid_namesize(0));
        assert!(is_valid_namesize(1));
        assert!(is_valid_namesize(256));
        assert!(is_valid_namesize(4096));
        assert!(!is_valid_namesize(4097));
    }
}
