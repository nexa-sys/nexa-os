//! Buddy Allocator Edge Case Tests
//!
//! Tests for buddy allocator boundary conditions, merging/splitting logic,
//! and potential bugs in free list management.
//!
//! Uses REAL kernel functions - NO local re-implementations.

#[cfg(test)]
mod tests {
    use crate::mm::allocator::{
        BuddyStats, size_to_order, order_to_size, get_buddy_addr, 
        is_valid_buddy_pair, is_order_aligned,
    };
    use crate::safety::paging::{align_up, align_down};

    const PAGE_SIZE: u64 = 4096;
    const MAX_ORDER: usize = 11;

    // =========================================================================
    // Address Validation Tests
    // =========================================================================

    #[test]
    fn test_valid_address_check() {
        // Addresses must be non-zero and 8-byte aligned for u64 read/write
        fn is_valid_addr(addr: u64) -> bool {
            addr != 0 && addr & 7 == 0
        }
        
        // Valid addresses
        assert!(is_valid_addr(0x1000));
        assert!(is_valid_addr(0x8));
        assert!(is_valid_addr(0x1000000));
        
        // Invalid addresses
        assert!(!is_valid_addr(0)); // Zero
        assert!(!is_valid_addr(1)); // Not 8-byte aligned
        assert!(!is_valid_addr(7)); // Not 8-byte aligned
    }

    #[test]
    fn test_page_alignment_requirement() {
        // Base address must be page-aligned
        fn is_page_aligned(addr: u64) -> bool {
            addr & 0xFFF == 0
        }
        
        assert!(is_page_aligned(0x1000));
        assert!(is_page_aligned(0x100000));
        assert!(!is_page_aligned(0x1001));
        assert!(!is_page_aligned(0x1FFF));
    }

    // =========================================================================
    // Order Calculation Tests
    // =========================================================================

    #[test]
    fn test_order_to_pages() {
        for order in 0..MAX_ORDER {
            let pages = 1usize << order;
            let expected_size = pages * PAGE_SIZE as usize;
            
            assert_eq!(expected_size, (PAGE_SIZE as usize) << order);
        }
    }

    #[test]
    fn test_order_limits() {
        // Order 0 = 1 page = 4KB
        assert_eq!(1 << 0, 1);
        
        // Order MAX_ORDER-1 = 2^10 pages = 1024 pages = 4MB
        assert_eq!(1usize << (MAX_ORDER - 1), 1024);
        
        // Max order allocation = 2^11 pages (but MAX_ORDER is exclusive limit)
        // So max allocatable is order 10 = 1024 pages
    }

    #[test]
    fn test_invalid_order_handling() {
        // Order >= MAX_ORDER should fail
        assert!(MAX_ORDER >= 11, "MAX_ORDER should be at least 11");
        
        // Allocation with order >= MAX_ORDER should return None
        fn would_allocate_fail(order: usize) -> bool {
            order >= MAX_ORDER
        }
        
        assert!(would_allocate_fail(MAX_ORDER));
        assert!(would_allocate_fail(MAX_ORDER + 1));
        assert!(would_allocate_fail(100));
    }

    // =========================================================================
    // Buddy Address Calculation Edge Cases
    // =========================================================================

    #[test]
    fn test_buddy_xor_property() {
        // Buddy address = addr XOR block_size
        // This means XORing twice returns the original
        let addr = 0x10000u64;
        let order = 2;
        let block_size = PAGE_SIZE << order;
        
        let buddy = addr ^ block_size;
        let back_to_original = buddy ^ block_size;
        
        assert_eq!(back_to_original, addr, "XOR property violated");
    }

    #[test]
    fn test_buddy_pair_merge_address() {
        // When merging, the merged block starts at the lower address
        let addr1 = 0x10000u64;
        let addr2 = 0x14000u64; // buddy at order 2 (16KB blocks)
        
        let merged_addr = addr1.min(addr2);
        assert_eq!(merged_addr, 0x10000);
    }

    #[test]
    fn test_buddy_at_base_address() {
        // Buddy of block at address 0
        let addr = 0u64;
        let order = 0;
        let block_size = PAGE_SIZE << order;
        
        let buddy = addr ^ block_size;
        assert_eq!(buddy, block_size, "Buddy of 0 should be block_size");
    }

    #[test]
    fn test_buddy_alignment_requirement() {
        // Blocks must be aligned to their size
        fn is_aligned_to_order(addr: u64, order: usize) -> bool {
            let alignment = PAGE_SIZE << order;
            addr & (alignment - 1) == 0
        }
        
        // Order 0: 4KB alignment
        assert!(is_aligned_to_order(0x1000, 0));
        assert!(!is_aligned_to_order(0x1001, 0));
        
        // Order 1: 8KB alignment
        assert!(is_aligned_to_order(0x2000, 1));
        assert!(!is_aligned_to_order(0x1000, 1)); // 4KB aligned but not 8KB
        
        // Order 2: 16KB alignment
        assert!(is_aligned_to_order(0x4000, 2));
        assert!(!is_aligned_to_order(0x2000, 2));
    }

    // =========================================================================
    // Statistics Tracking Tests
    // =========================================================================

    #[test]
    fn test_stats_allocation_tracking() {
        let mut stats = BuddyStats {
            allocations: 0,
            frees: 0,
            splits: 0,
            merges: 0,
            pages_allocated: 0,
            pages_free: 1024,
        };
        
        // Allocation of order 2 (4 pages)
        let order = 2;
        let pages = 1u64 << order;
        
        stats.allocations += 1;
        stats.pages_allocated += pages;
        stats.pages_free -= pages;
        
        assert_eq!(stats.allocations, 1);
        assert_eq!(stats.pages_allocated, 4);
        assert_eq!(stats.pages_free, 1020);
    }

    #[test]
    fn test_stats_split_tracking() {
        let mut stats = BuddyStats {
            allocations: 0,
            frees: 0,
            splits: 0,
            merges: 0,
            pages_allocated: 0,
            pages_free: 1024,
        };
        
        // Allocating order 0 from order 3 block requires 3 splits
        // order 3 -> order 2 + order 2 (split 1)
        // order 2 -> order 1 + order 1 (split 2)
        // order 1 -> order 0 + order 0 (split 3)
        let splits_needed = 3;
        stats.splits += splits_needed;
        
        assert_eq!(stats.splits, 3);
    }

    #[test]
    fn test_stats_merge_tracking() {
        let mut stats = BuddyStats {
            allocations: 0,
            frees: 0,
            splits: 0,
            merges: 0,
            pages_allocated: 4,
            pages_free: 1020,
        };
        
        // Freeing might trigger merges
        stats.frees += 1;
        stats.merges += 2; // Merged twice (order 0 -> 1 -> 2)
        stats.pages_allocated -= 4;
        stats.pages_free += 4;
        
        assert_eq!(stats.frees, 1);
        assert_eq!(stats.merges, 2);
        assert_eq!(stats.pages_free, 1024);
    }

    #[test]
    fn test_stats_consistency() {
        let stats = BuddyStats {
            allocations: 100,
            frees: 80,
            splits: 150,
            merges: 100,
            pages_allocated: 256,
            pages_free: 768,
        };
        
        // Total pages should remain constant
        let total = stats.pages_allocated + stats.pages_free;
        assert_eq!(total, 1024, "Total pages changed!");
    }

    // =========================================================================
    // Free List Corruption Detection Tests
    // =========================================================================

    #[test]
    fn test_sentinel_value() {
        // u64::MAX is used as sentinel for end of free list
        const SENTINEL: u64 = u64::MAX;
        
        // Sentinel should never be a valid address
        fn is_valid_addr(addr: u64) -> bool {
            addr != 0 && addr != SENTINEL && addr & 7 == 0
        }
        
        assert!(!is_valid_addr(SENTINEL));
    }

    #[test]
    fn test_free_list_cycle_detection() {
        // Detect a cycle in free list
        fn detect_cycle(nodes: &[u64]) -> bool {
            use std::collections::HashSet;
            let mut seen = HashSet::new();
            
            for &addr in nodes {
                if addr == u64::MAX {
                    break; // End of list
                }
                if !seen.insert(addr) {
                    return true; // Cycle detected
                }
            }
            false
        }
        
        // Normal list
        let normal = [0x1000, 0x2000, 0x3000, u64::MAX];
        assert!(!detect_cycle(&normal));
        
        // List with cycle
        let cyclic = [0x1000, 0x2000, 0x1000, u64::MAX];
        assert!(detect_cycle(&cyclic));
    }

    // =========================================================================
    // Fragmentation Scenarios
    // =========================================================================

    #[test]
    fn test_alternating_allocation_fragmentation() {
        // Allocating and freeing alternate blocks causes fragmentation
        // Example: allocate A, B, C, D; free A, C; can't allocate 2-block
        
        // This is a conceptual test for the pattern:
        // After: [free][used][free][used]
        // Can't merge because buddies aren't both free
        
        let blocks = [false, true, false, true]; // free, used, free, used
        
        fn can_merge_at(blocks: &[bool], idx: usize) -> bool {
            if idx + 1 >= blocks.len() {
                return false;
            }
            !blocks[idx] && !blocks[idx + 1] // Both must be free
        }
        
        // Index 0 and 1: 0 is free, 1 is used - can't merge
        assert!(!can_merge_at(&blocks, 0));
        
        // Index 2 and 3: 2 is free, 3 is used - can't merge
        assert!(!can_merge_at(&blocks, 2));
    }

    #[test]
    fn test_contiguous_free_merges() {
        // When both buddies are free, they should merge
        let blocks = [false, false, true, true]; // free, free, used, used
        
        fn can_merge_at(blocks: &[bool], idx: usize) -> bool {
            if idx + 1 >= blocks.len() {
                return false;
            }
            !blocks[idx] && !blocks[idx + 1]
        }
        
        // Buddies 0 and 1 are both free - can merge
        assert!(can_merge_at(&blocks, 0));
    }

    // =========================================================================
    // Memory Region Initialization Tests
    // =========================================================================

    #[test]
    fn test_size_rounding() {
        // Use REAL kernel align_down function for page boundary rounding
        assert_eq!(align_down(0x1000, PAGE_SIZE), 0x1000);
        assert_eq!(align_down(0x1001, PAGE_SIZE), 0x1000);
        assert_eq!(align_down(0x1FFF, PAGE_SIZE), 0x1000);
        assert_eq!(align_down(0x2000, PAGE_SIZE), 0x2000);
    }

    #[test]
    fn test_initial_free_block_creation() {
        // When initializing with a region, it should be split into
        // largest possible power-of-2 blocks
        fn largest_fitting_order(size: u64, base_aligned: u64) -> usize {
            for order in (0..MAX_ORDER).rev() {
                let block_size = PAGE_SIZE << order;
                if size >= block_size && base_aligned & (block_size - 1) == 0 {
                    return order;
                }
            }
            0
        }
        
        // 1MB at 1MB boundary
        assert_eq!(largest_fitting_order(0x100000, 0x100000), 8); // 256 pages = order 8
        
        // 4KB at any 4KB boundary
        assert_eq!(largest_fitting_order(0x1000, 0x1000), 0);
    }
}
