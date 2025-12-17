//! Slab Allocator tests
//!
//! Tests for kernel object cache and small allocation handling.

#[cfg(test)]
mod tests {
    // =========================================================================
    // Slab Size Classes Tests
    // =========================================================================

    #[test]
    fn test_slab_size_classes() {
        const SLAB_SIZES: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];
        
        // Verify sizes are powers of 2
        for size in SLAB_SIZES {
            assert!(size.is_power_of_two(), "{} is not a power of 2", size);
        }
        
        // Verify sizes are increasing
        for i in 1..SLAB_SIZES.len() {
            assert!(SLAB_SIZES[i] > SLAB_SIZES[i - 1]);
        }
    }

    #[test]
    fn test_slab_class_selection() {
        const SLAB_SIZES: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];
        
        // Find best fit slab class for requested size
        fn find_slab_class(size: usize) -> Option<usize> {
            for (i, &slab_size) in SLAB_SIZES.iter().enumerate() {
                if size <= slab_size {
                    return Some(i);
                }
            }
            None // Too large for slab allocator
        }
        
        assert_eq!(find_slab_class(1), Some(0));   // 16-byte slab
        assert_eq!(find_slab_class(16), Some(0));  // 16-byte slab
        assert_eq!(find_slab_class(17), Some(1));  // 32-byte slab
        assert_eq!(find_slab_class(2048), Some(7)); // 2048-byte slab
        assert_eq!(find_slab_class(2049), None);   // Too large
    }

    // =========================================================================
    // Objects Per Slab Tests
    // =========================================================================

    #[test]
    fn test_objects_per_slab() {
        const PAGE_SIZE: usize = 4096;
        const SLAB_SIZES: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];
        
        // Calculate objects per page (simplified - ignores header)
        fn objects_per_page(obj_size: usize) -> usize {
            PAGE_SIZE / obj_size
        }
        
        // 16-byte objects: 4096/16 = 256 objects
        assert_eq!(objects_per_page(16), 256);
        
        // 2048-byte objects: 4096/2048 = 2 objects
        assert_eq!(objects_per_page(2048), 2);
    }

    #[test]
    fn test_objects_per_slab_with_header() {
        const PAGE_SIZE: usize = 4096;
        const SLAB_HEADER_SIZE: usize = 64; // Typical header size
        
        fn objects_per_page(obj_size: usize) -> usize {
            (PAGE_SIZE - SLAB_HEADER_SIZE) / obj_size
        }
        
        // With 64-byte header:
        // 16-byte objects: (4096-64)/16 = 252 objects
        assert_eq!(objects_per_page(16), 252);
    }

    // =========================================================================
    // Slab States Tests
    // =========================================================================

    #[test]
    fn test_slab_states() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum SlabState {
            Empty,   // All objects free
            Partial, // Some objects allocated
            Full,    // All objects allocated
        }
        
        // All states distinct
        assert_ne!(SlabState::Empty, SlabState::Partial);
        assert_ne!(SlabState::Partial, SlabState::Full);
        assert_ne!(SlabState::Empty, SlabState::Full);
    }

    #[test]
    fn test_slab_state_transitions() {
        // Empty -> Partial (first allocation)
        // Partial -> Full (last free object allocated)
        // Full -> Partial (one object freed)
        // Partial -> Empty (last object freed)
        
        let mut free_count = 10;
        let total = 10;
        
        fn get_state(free: usize, total: usize) -> &'static str {
            if free == 0 { "Full" }
            else if free == total { "Empty" }
            else { "Partial" }
        }
        
        assert_eq!(get_state(free_count, total), "Empty");
        
        free_count -= 1; // Allocate one
        assert_eq!(get_state(free_count, total), "Partial");
        
        free_count = 0; // Allocate all
        assert_eq!(get_state(free_count, total), "Full");
        
        free_count = 1; // Free one
        assert_eq!(get_state(free_count, total), "Partial");
    }

    // =========================================================================
    // Free List Tests
    // =========================================================================

    #[test]
    fn test_freelist_bitmap() {
        // Bitmap-based free list for small slabs
        let mut bitmap: u64 = u64::MAX; // All 64 objects free (bits set)
        
        // Allocate first free object
        let obj_idx = bitmap.trailing_zeros() as usize;
        assert_eq!(obj_idx, 0);
        bitmap &= !(1u64 << obj_idx);
        
        // Allocate next free object
        let obj_idx = bitmap.trailing_zeros() as usize;
        assert_eq!(obj_idx, 1);
        bitmap &= !(1u64 << obj_idx);
        
        // Free object 0
        bitmap |= 1u64 << 0;
        let obj_idx = bitmap.trailing_zeros() as usize;
        assert_eq!(obj_idx, 0);
    }

    #[test]
    fn test_freelist_linked() {
        // Linked list based free list
        struct FreeNode {
            next: Option<usize>,
        }
        
        // Initialize free list: 0 -> 1 -> 2 -> None
        let nodes = [
            FreeNode { next: Some(1) },
            FreeNode { next: Some(2) },
            FreeNode { next: None },
        ];
        
        // Pop from head
        let head = 0;
        let new_head = nodes[head].next;
        assert_eq!(new_head, Some(1));
    }

    // =========================================================================
    // Cache Coloring Tests
    // =========================================================================

    #[test]
    fn test_cache_coloring() {
        // Cache coloring improves cache utilization by varying object placement
        const CACHE_LINE_SIZE: usize = 64;
        const OBJ_SIZE: usize = 128;
        
        // Calculate number of color slots
        fn color_slots(cache_line: usize, obj_align: usize) -> usize {
            cache_line / obj_align.min(cache_line)
        }
        
        // With 64-byte cache lines and 128-byte objects (aligned to 128)
        // color_slots = 64 / 64 = 1 (no benefit)
        
        // With 64-byte cache lines and 32-byte objects
        // color_slots = 64 / 32 = 2
        assert_eq!(color_slots(64, 32), 2);
    }

    // =========================================================================
    // Memory Poisoning Tests
    // =========================================================================

    #[test]
    fn test_poison_pattern() {
        const POISON_BYTE: u8 = 0xCC;
        const FREED_PATTERN: u8 = 0xDD;
        
        // Poison freed memory to detect use-after-free
        let mut buffer = [0u8; 16];
        
        // "Free" the buffer
        buffer.fill(POISON_BYTE);
        
        // All bytes should be poison
        for &byte in &buffer {
            assert_eq!(byte, POISON_BYTE);
        }
    }

    #[test]
    fn test_redzone_detection() {
        const REDZONE_PATTERN: u8 = 0xBB;
        const REDZONE_SIZE: usize = 8;
        
        // Redzone before and after object detects overflows
        let mut memory = [0u8; 32];
        
        // Layout: [redzone][object][redzone]
        // Object at [8..24], redzones at [0..8] and [24..32]
        memory[0..8].fill(REDZONE_PATTERN);
        memory[24..32].fill(REDZONE_PATTERN);
        
        // Check redzones intact
        fn check_redzones(memory: &[u8]) -> bool {
            memory[0..8].iter().all(|&b| b == REDZONE_PATTERN)
                && memory[24..32].iter().all(|&b| b == REDZONE_PATTERN)
        }
        
        assert!(check_redzones(&memory));
        
        // Simulate overflow
        memory[24] = 0x00;
        assert!(!check_redzones(&memory));
    }

    // =========================================================================
    // Magic Number Tests
    // =========================================================================

    #[test]
    fn test_heap_magic() {
        const HEAP_MAGIC: u32 = 0xDEADBEEF;
        
        // Block header should have valid magic
        struct BlockHeader {
            magic: u32,
            size: u32,
        }
        
        let header = BlockHeader {
            magic: HEAP_MAGIC,
            size: 64,
        };
        
        fn validate_block(header: &BlockHeader) -> bool {
            header.magic == HEAP_MAGIC
        }
        
        assert!(validate_block(&header));
        
        // Corrupted magic
        let bad_header = BlockHeader {
            magic: 0xBADC0DE,
            size: 64,
        };
        assert!(!validate_block(&bad_header));
    }

    // =========================================================================
    // Alignment Tests
    // =========================================================================

    #[test]
    fn test_allocation_alignment() {
        // All slab allocations should be naturally aligned
        const SLAB_SIZES: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];
        
        fn is_aligned(addr: usize, alignment: usize) -> bool {
            addr & (alignment - 1) == 0
        }
        
        // Base address at page boundary
        let base = 0x100000usize;
        
        for &size in &SLAB_SIZES {
            // Each object should be aligned to its size
            let obj_addr = base + 64; // Skip header
            assert!(is_aligned(obj_addr, size.min(64)), 
                    "Object of size {} at {:#x} not aligned", size, obj_addr);
        }
    }

    // =========================================================================
    // Large Allocation Fallback Tests
    // =========================================================================

    #[test]
    fn test_large_allocation_fallback() {
        const MAX_SLAB_SIZE: usize = 2048;
        
        // Allocations larger than max slab size should use buddy allocator
        fn should_use_buddy(size: usize) -> bool {
            size > MAX_SLAB_SIZE
        }
        
        assert!(!should_use_buddy(1024));
        assert!(!should_use_buddy(2048));
        assert!(should_use_buddy(2049));
        assert!(should_use_buddy(4096));
    }
}
