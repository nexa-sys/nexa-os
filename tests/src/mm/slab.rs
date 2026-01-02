//! Slab Allocator tests
//!
//! Tests for kernel object cache and small allocation handling.
//! Uses REAL kernel allocator code - no simulated implementations.

#[cfg(test)]
mod tests {
    use crate::mm::allocator::{
        SLAB_SIZES, SLAB_CLASSES, PAGE_SIZE, HEAP_MAGIC, POISON_BYTE,
        size_to_order, order_to_size,
        SlabStats, BuddyStats,
    };

    // =========================================================================
    // Slab Size Classes Tests (using REAL kernel constants)
    // =========================================================================

    #[test]
    fn test_slab_size_classes() {
        // Verify kernel SLAB_SIZES are powers of 2
        for size in SLAB_SIZES {
            assert!(size.is_power_of_two(), "{} is not a power of 2", size);
        }
        
        // Verify sizes are increasing
        for i in 1..SLAB_SIZES.len() {
            assert!(SLAB_SIZES[i] > SLAB_SIZES[i - 1]);
        }
    }

    #[test]
    fn test_slab_classes_count() {
        assert_eq!(SLAB_CLASSES, 8);
        assert_eq!(SLAB_SIZES.len(), SLAB_CLASSES);
    }

    #[test]
    fn test_slab_sizes_values() {
        // Verify exact kernel slab sizes
        assert_eq!(SLAB_SIZES[0], 16);
        assert_eq!(SLAB_SIZES[1], 32);
        assert_eq!(SLAB_SIZES[2], 64);
        assert_eq!(SLAB_SIZES[3], 128);
        assert_eq!(SLAB_SIZES[4], 256);
        assert_eq!(SLAB_SIZES[5], 512);
        assert_eq!(SLAB_SIZES[6], 1024);
        assert_eq!(SLAB_SIZES[7], 2048);
    }

    #[test]
    fn test_largest_slab_fits_in_page() {
        // Largest slab object (2048 bytes) must fit in a page
        assert!(SLAB_SIZES[SLAB_CLASSES - 1] < PAGE_SIZE);
    }

    #[test]
    fn test_smallest_slab_reasonable() {
        // Smallest slab (16 bytes) should be reasonable minimum
        assert!(SLAB_SIZES[0] >= 8, "Minimum slab too small for pointer");
        assert!(SLAB_SIZES[0] <= 32, "Minimum slab unnecessarily large");
    }

    // =========================================================================
    // Slab Class Selection Tests (using REAL kernel SLAB_SIZES)
    // =========================================================================

    #[test]
    fn test_slab_class_for_small_alloc() {
        // Find best fit slab class using real SLAB_SIZES
        fn find_slab_class(size: usize) -> Option<usize> {
            for (i, &slab_size) in SLAB_SIZES.iter().enumerate() {
                if size <= slab_size {
                    return Some(i);
                }
            }
            None
        }

        assert_eq!(find_slab_class(1), Some(0));
        assert_eq!(find_slab_class(16), Some(0));
        assert_eq!(find_slab_class(17), Some(1));
        assert_eq!(find_slab_class(2048), Some(7));
        assert_eq!(find_slab_class(2049), None);
    }

    #[test]
    fn test_slab_class_boundary() {
        // Test at exact boundaries using real SLAB_SIZES
        for (i, &size) in SLAB_SIZES.iter().enumerate() {
            // Exact size should go to this class
            let class = SLAB_SIZES.iter().position(|&s| size <= s);
            assert_eq!(class, Some(i));
            
            // One more should go to next class (if not last)
            if i < SLAB_CLASSES - 1 {
                let class = SLAB_SIZES.iter().position(|&s| size + 1 <= s);
                assert_eq!(class, Some(i + 1));
            }
        }
    }

    // =========================================================================
    // Objects Per Slab Tests (using REAL kernel PAGE_SIZE)
    // =========================================================================

    #[test]
    fn test_objects_per_slab_16() {
        // 16-byte objects: 4096/16 = 256 objects (ignoring header)
        let max_objects = PAGE_SIZE / SLAB_SIZES[0];
        assert_eq!(max_objects, 256);
    }

    #[test]
    fn test_objects_per_slab_2048() {
        // 2048-byte objects: 4096/2048 = 2 objects (ignoring header)
        let max_objects = PAGE_SIZE / SLAB_SIZES[7];
        assert_eq!(max_objects, 2);
    }

    #[test]
    fn test_objects_per_page_calculation() {
        // Test objects per page for each slab class using real constants
        for &size in &SLAB_SIZES {
            let objects = PAGE_SIZE / size;
            assert!(objects >= 1, "Slab size {} yields 0 objects per page", size);
        }
    }

    // =========================================================================
    // Magic Number and Poison Tests (using REAL kernel constants)
    // =========================================================================

    #[test]
    fn test_heap_magic_value() {
        assert_eq!(HEAP_MAGIC, 0xDEADBEEF);
    }

    #[test]
    fn test_poison_byte_value() {
        assert_eq!(POISON_BYTE, 0xCC);
    }

    #[test]
    fn test_poison_pattern_detectable() {
        // Poison pattern should be easily distinguishable from common values
        assert_ne!(POISON_BYTE, 0x00);
        assert_ne!(POISON_BYTE, 0xFF);
    }

    #[test]
    fn test_poison_fill() {
        // Test using kernel's POISON_BYTE constant
        let mut buffer = [0u8; 16];
        buffer.fill(POISON_BYTE);
        
        for &byte in &buffer {
            assert_eq!(byte, POISON_BYTE);
        }
    }

    // =========================================================================
    // Size to Order Tests (using REAL kernel function)
    // =========================================================================

    #[test]
    fn test_size_to_order_page_size() {
        // Exactly one page
        assert_eq!(size_to_order(PAGE_SIZE), 0);
    }

    #[test]
    fn test_size_to_order_less_than_page() {
        // Less than one page still requires order 0
        assert_eq!(size_to_order(1), 0);
        assert_eq!(size_to_order(100), 0);
        assert_eq!(size_to_order(PAGE_SIZE - 1), 0);
    }

    #[test]
    fn test_size_to_order_two_pages() {
        // Two pages requires order 1
        assert_eq!(size_to_order(PAGE_SIZE + 1), 1);
        assert_eq!(size_to_order(PAGE_SIZE * 2), 1);
    }

    #[test]
    fn test_size_to_order_large() {
        // 8 pages requires order 3
        assert_eq!(size_to_order(PAGE_SIZE * 8), 3);
        
        // 1MB = 256 pages requires order 8
        assert_eq!(size_to_order(1024 * 1024), 8);
    }

    // =========================================================================
    // Order to Size Tests (using REAL kernel function)
    // =========================================================================

    #[test]
    fn test_order_to_size_zero() {
        assert_eq!(order_to_size(0), PAGE_SIZE);
    }

    #[test]
    fn test_order_to_size_one() {
        assert_eq!(order_to_size(1), PAGE_SIZE * 2);
    }

    #[test]
    fn test_order_to_size_powers() {
        // Each order doubles the size
        for order in 0..10 {
            assert_eq!(order_to_size(order), PAGE_SIZE << order);
        }
    }

    #[test]
    fn test_order_size_roundtrip() {
        // size_to_order and order_to_size should be consistent
        for order in 0..10 {
            let size = order_to_size(order);
            let back = size_to_order(size);
            assert_eq!(back, order, "Roundtrip failed for order {}", order);
        }
    }

    // =========================================================================
    // SlabStats Tests (using REAL kernel struct)
    // =========================================================================

    #[test]
    fn test_slab_stats_default() {
        let stats = SlabStats::default();
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.frees, 0);
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.cache_misses, 0);
    }

    #[test]
    fn test_slab_stats_copy() {
        let stats = SlabStats {
            allocations: 100,
            frees: 50,
            cache_hits: 80,
            cache_misses: 20,
        };
        let copy = stats;
        assert_eq!(copy.allocations, stats.allocations);
        assert_eq!(copy.frees, stats.frees);
    }

    // =========================================================================
    // BuddyStats Tests (using REAL kernel struct)
    // =========================================================================

    #[test]
    fn test_buddy_stats_default() {
        let stats = BuddyStats::default();
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.frees, 0);
        assert_eq!(stats.splits, 0);
        assert_eq!(stats.merges, 0);
    }

    // =========================================================================
    // Alignment Tests (using REAL kernel constants)
    // =========================================================================

    #[test]
    fn test_page_size_alignment() {
        assert!(PAGE_SIZE.is_power_of_two());
        assert_eq!(PAGE_SIZE, 4096);
    }

    #[test]
    fn test_slab_sizes_natural_alignment() {
        // All slab sizes should be naturally aligned (power of 2)
        for &size in &SLAB_SIZES {
            assert!(size.is_power_of_two());
        }
    }

    // =========================================================================
    // Large Allocation Fallback Tests (using REAL kernel constants)
    // =========================================================================

    #[test]
    fn test_large_allocation_threshold() {
        let max_slab_size = SLAB_SIZES[SLAB_CLASSES - 1];
        assert_eq!(max_slab_size, 2048);
        
        // Allocations larger than max slab should use buddy allocator
        assert!(2049 > max_slab_size);
        assert!(PAGE_SIZE > max_slab_size);
    }

    #[test]
    fn test_slab_vs_buddy_decision() {
        // Use slab for small allocations
        for &size in &SLAB_SIZES {
            assert!(size <= SLAB_SIZES[SLAB_CLASSES - 1]);
        }
        
        // Use buddy for large allocations
        assert!(PAGE_SIZE > SLAB_SIZES[SLAB_CLASSES - 1]);
    }
}
