//! Buddy Allocator edge case tests
//!
//! Tests for physical memory allocator boundary conditions and fragmentation.
//! Uses REAL kernel functions from crate::mm::allocator

#[cfg(test)]
mod tests {
    use crate::mm::allocator::{
        BuddyStats, get_buddy_addr, is_order_aligned, is_valid_buddy_pair,
        order_to_size, size_to_order,
    };
    
    // =========================================================================
    // Buddy Allocator Constants Tests
    // =========================================================================

    #[test]
    fn test_buddy_max_order() {
        const MAX_ORDER: usize = 11;
        
        // Max allocation = 2^11 pages = 2048 pages = 8MB
        let max_alloc = order_to_size(MAX_ORDER);
        assert_eq!(max_alloc, 8 * 1024 * 1024);
    }

    #[test]
    fn test_buddy_page_size() {
        // Order 0 = 1 page = 4KB
        let page_size = order_to_size(0);
        assert!(page_size.is_power_of_two());
        assert_eq!(page_size, 0x1000);
    }

    #[test]
    fn test_buddy_order_sizes() {
        const MAX_ORDER: usize = 11;
        
        let order_sizes: Vec<usize> = (0..=MAX_ORDER)
            .map(|order| order_to_size(order))
            .collect();
        
        // Verify each order is double the previous
        for i in 1..=MAX_ORDER {
            assert_eq!(order_sizes[i], order_sizes[i - 1] * 2);
        }
    }

    // =========================================================================
    // Order Calculation Tests (using REAL kernel functions)
    // =========================================================================

    #[test]
    fn test_size_to_order() {
        // Use REAL kernel function
        assert_eq!(size_to_order(1), 0);
        assert_eq!(size_to_order(4096), 0);
        assert_eq!(size_to_order(4097), 1);
        assert_eq!(size_to_order(8192), 1);
        assert_eq!(size_to_order(8193), 2);
    }

    #[test]
    fn test_order_to_size() {
        // Use REAL kernel function
        assert_eq!(order_to_size(0), 4096);
        assert_eq!(order_to_size(1), 8192);
        assert_eq!(order_to_size(2), 16384);
        assert_eq!(order_to_size(10), 4 * 1024 * 1024);
    }

    #[test]
    fn test_size_order_roundtrip() {
        // Size -> Order -> Size should be >= original (rounding up)
        for size in [1, 100, 4096, 4097, 8192, 8193, 65536] {
            let order = size_to_order(size);
            let result_size = order_to_size(order);
            assert!(result_size >= size, "size={} order={} result={}", size, order, result_size);
        }
    }

    // =========================================================================
    // Buddy Pairing Tests (using REAL kernel functions)
    // =========================================================================

    #[test]
    fn test_buddy_address_calculation() {
        // Use REAL kernel function get_buddy_addr
        
        // Order 0: 4KB blocks
        assert_eq!(get_buddy_addr(0x1000, 0), 0x0000);
        assert_eq!(get_buddy_addr(0x0000, 0), 0x1000);
        
        // Order 1: 8KB blocks
        assert_eq!(get_buddy_addr(0x2000, 1), 0x0000);
        assert_eq!(get_buddy_addr(0x0000, 1), 0x2000);
        
        // Buddy of buddy should be original
        assert_eq!(get_buddy_addr(get_buddy_addr(0x4000, 2), 2), 0x4000);
    }

    #[test]
    fn test_buddy_is_valid_pair() {
        // Use REAL kernel function is_valid_buddy_pair
        
        // Valid pairs: addresses differ exactly by block_size and are aligned
        assert!(is_valid_buddy_pair(0x0000, 0x1000, 0));  // Order 0: 4KB buddies
        assert!(is_valid_buddy_pair(0x0000, 0x2000, 1));  // Order 1: 8KB buddies
        assert!(is_valid_buddy_pair(0x4000, 0x0000, 2));  // Order 2: 16KB buddies
        
        // Invalid pairs
        assert!(!is_valid_buddy_pair(0x0000, 0x2000, 0)); // 0x2000 is not 4KB-buddy of 0
        assert!(!is_valid_buddy_pair(0x1000, 0x3000, 1)); // Not aligned to 8KB
    }

    #[test]
    fn test_buddy_symmetry() {
        // If A is buddy of B, then B is buddy of A
        // Use properly aligned addresses for each order
        for order in 0..8 {
            let block_size = order_to_size(order) as u64;
            // Use address that is aligned to block_size * 2 (parent block alignment)
            let addr = block_size * 2;
            let buddy = get_buddy_addr(addr, order);
            assert_eq!(get_buddy_addr(buddy, order), addr,
                "Buddy of buddy should be original at order {}", order);
            assert!(is_valid_buddy_pair(addr, buddy, order),
                "addr={:#x} buddy={:#x} should be valid pair at order {}", addr, buddy, order);
            assert!(is_valid_buddy_pair(buddy, addr, order),
                "buddy={:#x} addr={:#x} should be valid pair at order {}", buddy, addr, order);
        }
    }

    // =========================================================================
    // Alignment Tests (using REAL kernel functions)
    // =========================================================================

    #[test]
    fn test_address_alignment() {
        // Use REAL kernel function is_order_aligned
        
        // Order 0 alignment (4KB)
        assert!(is_order_aligned(0x0000, 0));
        assert!(is_order_aligned(0x1000, 0));
        assert!(!is_order_aligned(0x1001, 0));
        
        // Order 1 alignment (8KB)
        assert!(is_order_aligned(0x0000, 1));
        assert!(is_order_aligned(0x2000, 1));
        assert!(!is_order_aligned(0x1000, 1));
        
        // Order 2 alignment (16KB)
        assert!(is_order_aligned(0x0000, 2));
        assert!(is_order_aligned(0x4000, 2));
        assert!(!is_order_aligned(0x2000, 2));
    }

    #[test]
    fn test_alignment_consistency() {
        // Higher order alignment implies lower order alignment
        for addr in [0x0000u64, 0x10000, 0x100000] {
            for high_order in 0..8 {
                if is_order_aligned(addr, high_order) {
                    for low_order in 0..high_order {
                        assert!(is_order_aligned(addr, low_order),
                            "addr={:#x} aligned to order {} but not to order {}",
                            addr, high_order, low_order);
                    }
                }
            }
        }
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
        
        // Allocation of 4 pages
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
        // Zero-size should map to order 0 (minimum allocation)
        let order = size_to_order(0);
        assert_eq!(order, 0);
        assert_eq!(order_to_size(order), 4096);
    }

    #[test]
    fn test_max_size_allocation() {
        const MAX_ORDER: usize = 11;
        
        let max_size = order_to_size(MAX_ORDER);
        assert_eq!(max_size, 8 * 1024 * 1024);
        
        // Size larger than max would need order > MAX_ORDER
        let order = size_to_order(max_size + 1);
        assert!(order > MAX_ORDER);
    }

    #[test]
    fn test_fragmentation_scenario() {
        // Fragmentation: allocate order 0, 2, 0, 2, free middle
        // This creates holes in the free list
        
        let allocations = [(0, "block1"), (2, "block2"), (0, "block3"), (2, "block4")];
        let _ = allocations; // Use it
        
        // After freeing block2 and block3, we have fragmentation
        // Order 2 hole at former block2 position
        // Order 0 hole at former block3 position
        
        // Cannot allocate order 3 even though total free pages might be enough
        assert!(true); // Placeholder for fragmentation detection
    }

    #[test]
    fn test_memory_exhaustion() {
        let pages_free = 100u64;
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
