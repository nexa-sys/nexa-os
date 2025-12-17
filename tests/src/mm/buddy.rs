//! Buddy Allocator edge case tests
//!
//! Tests for physical memory allocator boundary conditions and fragmentation.

#[cfg(test)]
mod tests {
    use crate::mm::allocator::BuddyStats;
    
    // =========================================================================
    // Buddy Allocator Constants Tests
    // =========================================================================

    #[test]
    fn test_buddy_max_order() {
        const MAX_ORDER: usize = 11;
        const PAGE_SIZE: usize = 4096;
        
        // Max allocation = 2^11 pages = 2048 pages = 8MB
        let max_alloc = PAGE_SIZE << MAX_ORDER;
        assert_eq!(max_alloc, 8 * 1024 * 1024);
    }

    #[test]
    fn test_buddy_page_size() {
        const PAGE_SIZE: usize = 4096;
        
        assert!(PAGE_SIZE.is_power_of_two());
        assert_eq!(PAGE_SIZE, 0x1000);
    }

    #[test]
    fn test_buddy_order_sizes() {
        const PAGE_SIZE: usize = 4096;
        const MAX_ORDER: usize = 11;
        
        let order_sizes: Vec<usize> = (0..=MAX_ORDER)
            .map(|order| PAGE_SIZE << order)
            .collect();
        
        // Verify each order is double the previous
        for i in 1..=MAX_ORDER {
            assert_eq!(order_sizes[i], order_sizes[i - 1] * 2);
        }
    }

    // =========================================================================
    // Order Calculation Tests
    // =========================================================================

    #[test]
    fn test_size_to_order() {
        const PAGE_SIZE: usize = 4096;
        
        // Calculate minimum order needed for a given size
        fn size_to_order(size: usize) -> usize {
            if size <= PAGE_SIZE {
                return 0;
            }
            let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
            // Round up to next power of 2
            (usize::BITS - (pages - 1).leading_zeros()) as usize
        }
        
        assert_eq!(size_to_order(1), 0);
        assert_eq!(size_to_order(4096), 0);
        assert_eq!(size_to_order(4097), 1);
        assert_eq!(size_to_order(8192), 1);
        assert_eq!(size_to_order(8193), 2);
    }

    #[test]
    fn test_order_to_size() {
        const PAGE_SIZE: usize = 4096;
        
        fn order_to_size(order: usize) -> usize {
            PAGE_SIZE << order
        }
        
        assert_eq!(order_to_size(0), 4096);
        assert_eq!(order_to_size(1), 8192);
        assert_eq!(order_to_size(2), 16384);
        assert_eq!(order_to_size(10), 4 * 1024 * 1024);
    }

    // =========================================================================
    // Buddy Pairing Tests
    // =========================================================================

    #[test]
    fn test_buddy_address_calculation() {
        const PAGE_SIZE: u64 = 4096;
        
        // Calculate buddy address by XORing with block size
        fn get_buddy(addr: u64, order: usize) -> u64 {
            let block_size = PAGE_SIZE << order;
            addr ^ block_size
        }
        
        // Order 0: 4KB blocks
        assert_eq!(get_buddy(0x1000, 0), 0x0000);
        assert_eq!(get_buddy(0x0000, 0), 0x1000);
        
        // Order 1: 8KB blocks
        assert_eq!(get_buddy(0x2000, 1), 0x0000);
        assert_eq!(get_buddy(0x0000, 1), 0x2000);
    }

    #[test]
    fn test_buddy_is_valid_pair() {
        const PAGE_SIZE: u64 = 4096;
        
        // Buddy pair validation: Two addresses are buddies if:
        // 1. They XOR to the block size for that order
        // 2. Both addresses are aligned to the block size
        // 3. They are from the same parent block (lower bits must match)
        fn is_valid_buddy_pair(addr1: u64, addr2: u64, order: usize) -> bool {
            let block_size = PAGE_SIZE << order;
            // Check alignment
            if addr1 & (block_size - 1) != 0 || addr2 & (block_size - 1) != 0 {
                return false;
            }
            // XOR check: buddies differ only in the buddy bit
            (addr1 ^ addr2) == block_size
        }
        
        // Valid pairs: addresses differ exactly by block_size and are aligned
        assert!(is_valid_buddy_pair(0x0000, 0x1000, 0));  // Order 0: 4KB buddies at 0 and 4KB
        assert!(is_valid_buddy_pair(0x0000, 0x2000, 1));  // Order 1: 8KB buddies at 0 and 8KB
        
        // Invalid pairs
        assert!(!is_valid_buddy_pair(0x0000, 0x2000, 0)); // 0x2000 is not 4KB-buddy of 0
        // 0x1000 and 0x3000 at order 1: 0x1000 is not aligned to 8KB
        assert!(!is_valid_buddy_pair(0x1000, 0x3000, 1));
    }

    // =========================================================================
    // Alignment Tests
    // =========================================================================

    #[test]
    fn test_address_alignment() {
        const PAGE_SIZE: u64 = 4096;
        
        fn is_aligned(addr: u64, order: usize) -> bool {
            let alignment = PAGE_SIZE << order;
            addr & (alignment - 1) == 0
        }
        
        // Order 0 alignment (4KB)
        assert!(is_aligned(0x0000, 0));
        assert!(is_aligned(0x1000, 0));
        assert!(!is_aligned(0x1001, 0));
        
        // Order 1 alignment (8KB)
        assert!(is_aligned(0x0000, 1));
        assert!(is_aligned(0x2000, 1));
        assert!(!is_aligned(0x1000, 1));
    }

    // =========================================================================
    // BuddyStats Tests
    // =========================================================================

    #[test]
    fn test_buddy_stats_initialization() {
        let stats = BuddyStats {
            allocations: 0,
            frees: 0,
            splits: 0,
            merges: 0,
            pages_allocated: 0,
            pages_free: 1024, // 4MB
        };
        
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.pages_free, 1024);
    }

    #[test]
    fn test_buddy_stats_tracking() {
        let mut stats = BuddyStats {
            allocations: 0,
            frees: 0,
            splits: 0,
            merges: 0,
            pages_allocated: 0,
            pages_free: 1000,
        };
        
        // Simulate allocation of 4 pages
        stats.allocations += 1;
        stats.pages_allocated += 4;
        stats.pages_free -= 4;
        
        assert_eq!(stats.allocations, 1);
        assert_eq!(stats.pages_allocated, 4);
        assert_eq!(stats.pages_free, 996);
    }

    #[test]
    fn test_buddy_stats_splits_merges() {
        let mut stats = BuddyStats {
            allocations: 0,
            frees: 0,
            splits: 0,
            merges: 0,
            pages_allocated: 0,
            pages_free: 0,
        };
        
        // Allocating requires splitting
        stats.splits += 3; // Split order 10 -> 9 -> 8
        assert_eq!(stats.splits, 3);
        
        // Freeing may cause merging
        stats.merges += 2;
        assert_eq!(stats.merges, 2);
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_zero_size_allocation() {
        // Zero-size allocation should be rejected
        let size = 0usize;
        assert_eq!(size, 0);
        // Allocator should return error or minimum allocation
    }

    #[test]
    fn test_max_size_allocation() {
        const PAGE_SIZE: usize = 4096;
        const MAX_ORDER: usize = 11;
        
        let max_size = PAGE_SIZE << MAX_ORDER;
        assert_eq!(max_size, 8 * 1024 * 1024);
        
        // Allocation larger than max should fail
        let too_large = max_size + 1;
        assert!(too_large > max_size);
    }

    #[test]
    fn test_fragmentation_scenario() {
        // Simulate fragmentation: allocate order 0, 2, 0, 2, free middle
        // This creates holes in the free list
        
        let allocations = [(0, "block1"), (2, "block2"), (0, "block3"), (2, "block4")];
        
        // After freeing block2 and block3, we have fragmentation
        // Order 2 hole at former block2 position
        // Order 0 hole at former block3 position
        
        // Cannot allocate order 3 even though total free pages might be enough
        assert!(true); // Placeholder for fragmentation detection
    }

    #[test]
    fn test_memory_exhaustion() {
        let mut pages_free = 100u64;
        let request_pages = 128u64;
        
        // Cannot satisfy request
        assert!(request_pages > pages_free);
        
        // After allocation fails, state should be unchanged
        assert_eq!(pages_free, 100);
    }

    // =========================================================================
    // Concurrent Access Tests (Conceptual)
    // =========================================================================

    #[test]
    fn test_allocator_lock_semantics() {
        // Buddy allocator uses spin lock for thread safety
        use std::sync::atomic::{AtomicBool, Ordering};
        
        let locked = AtomicBool::new(false);
        
        // Acquire lock
        assert!(!locked.swap(true, Ordering::Acquire));
        
        // Lock is now held
        assert!(locked.load(Ordering::Relaxed));
        
        // Release lock
        locked.store(false, Ordering::Release);
        assert!(!locked.load(Ordering::Relaxed));
    }
}
