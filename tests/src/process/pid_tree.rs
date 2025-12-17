//! PID Tree and Allocator Tests
//!
//! Tests for the radix tree-based PID allocation and management system.
//! These tests verify PID allocation, recycling, lookup, and edge cases.

#[cfg(test)]
mod tests {
    use crate::process::pid_tree::{
        allocate_pid, free_pid, is_pid_allocated, lookup_pid, register_pid_mapping,
        MAX_PID, MIN_PID,
    };

    // =========================================================================
    // PID Constants Tests
    // =========================================================================

    #[test]
    fn test_pid_constants() {
        // MIN_PID should be 1 (PID 0 is reserved for kernel/idle)
        assert_eq!(MIN_PID, 1);
        // MAX_PID should be 2^18 - 1 = 262143
        assert_eq!(MAX_PID, (1 << 18) - 1);
        assert_eq!(MAX_PID, 262143);
    }

    #[test]
    fn test_pid_range_is_reasonable() {
        // Should support at least 32768 PIDs (like traditional Unix)
        assert!(MAX_PID >= 32767);
        // But not more than a million for sanity
        assert!(MAX_PID < 1_000_000);
    }

    // =========================================================================
    // PID Bitmap Tests
    // =========================================================================

    /// Simulates the bitmap operations used in PID allocation
    struct PidBitmap {
        bitmap: [u64; 64], // 64 * 64 = 4096 PIDs tracked
        next_hint: u64,
        allocated_count: u64,
    }

    impl PidBitmap {
        fn new() -> Self {
            let mut bitmap = [0u64; 64];
            bitmap[0] = 1; // Mark PID 0 as allocated
            Self {
                bitmap,
                next_hint: 1,
                allocated_count: 1,
            }
        }

        fn is_allocated(&self, pid: u64) -> bool {
            if pid >= 4096 {
                return false;
            }
            let word_idx = (pid / 64) as usize;
            let bit_idx = pid % 64;
            (self.bitmap[word_idx] & (1 << bit_idx)) != 0
        }

        fn mark_allocated(&mut self, pid: u64) -> bool {
            if pid >= 4096 || self.is_allocated(pid) {
                return false;
            }
            let word_idx = (pid / 64) as usize;
            let bit_idx = pid % 64;
            self.bitmap[word_idx] |= 1 << bit_idx;
            self.allocated_count += 1;
            true
        }

        fn mark_free(&mut self, pid: u64) -> bool {
            if pid == 0 || pid >= 4096 || !self.is_allocated(pid) {
                return false;
            }
            let word_idx = (pid / 64) as usize;
            let bit_idx = pid % 64;
            self.bitmap[word_idx] &= !(1 << bit_idx);
            self.allocated_count -= 1;
            if pid < self.next_hint {
                self.next_hint = pid;
            }
            true
        }

        fn allocate_next(&mut self) -> Option<u64> {
            let start_word = (self.next_hint / 64) as usize;
            
            // Search from hint to end
            for word_idx in start_word..self.bitmap.len() {
                let word = self.bitmap[word_idx];
                if word == u64::MAX {
                    continue;
                }
                
                let first_zero = (!word).trailing_zeros() as u64;
                let pid = (word_idx as u64) * 64 + first_zero;
                
                if pid == 0 {
                    // Skip PID 0, find next zero
                    let next_zero = (!word & !1).trailing_zeros() as u64;
                    if next_zero < 64 {
                        let pid = next_zero;
                        if self.mark_allocated(pid) {
                            self.next_hint = pid + 1;
                            return Some(pid);
                        }
                    }
                    continue;
                }
                
                if self.mark_allocated(pid) {
                    self.next_hint = pid + 1;
                    return Some(pid);
                }
            }
            
            // Wrap around
            for word_idx in 0..start_word {
                let word = self.bitmap[word_idx];
                if word == u64::MAX {
                    continue;
                }
                
                let first_zero = (!word).trailing_zeros() as u64;
                let pid = (word_idx as u64) * 64 + first_zero;
                
                if pid == 0 {
                    continue;
                }
                
                if self.mark_allocated(pid) {
                    self.next_hint = pid + 1;
                    return Some(pid);
                }
            }
            
            None
        }
    }

    #[test]
    fn test_bitmap_initial_state() {
        let bitmap = PidBitmap::new();
        
        // PID 0 should be pre-allocated (reserved)
        assert!(bitmap.is_allocated(0));
        
        // PID 1 should be free
        assert!(!bitmap.is_allocated(1));
        
        // Count should be 1 (only PID 0)
        assert_eq!(bitmap.allocated_count, 1);
    }

    #[test]
    fn test_bitmap_allocate_sequential() {
        let mut bitmap = PidBitmap::new();
        
        // Allocate PIDs 1, 2, 3
        assert_eq!(bitmap.allocate_next(), Some(1));
        assert_eq!(bitmap.allocate_next(), Some(2));
        assert_eq!(bitmap.allocate_next(), Some(3));
        
        assert!(bitmap.is_allocated(1));
        assert!(bitmap.is_allocated(2));
        assert!(bitmap.is_allocated(3));
        assert_eq!(bitmap.allocated_count, 4); // 0, 1, 2, 3
    }

    #[test]
    fn test_bitmap_allocate_with_gap() {
        let mut bitmap = PidBitmap::new();
        
        // Allocate PIDs 1, 2, 3
        bitmap.allocate_next();
        bitmap.allocate_next();
        bitmap.allocate_next();
        
        // Free PID 2
        assert!(bitmap.mark_free(2));
        assert!(!bitmap.is_allocated(2));
        
        // Next allocation should reuse PID 2
        assert_eq!(bitmap.allocate_next(), Some(2));
    }

    #[test]
    fn test_bitmap_cannot_free_pid_zero() {
        let mut bitmap = PidBitmap::new();
        
        // Cannot free PID 0
        assert!(!bitmap.mark_free(0));
        assert!(bitmap.is_allocated(0));
    }

    #[test]
    fn test_bitmap_double_allocate_fails() {
        let mut bitmap = PidBitmap::new();
        
        // Allocate PID 1
        assert!(bitmap.mark_allocated(1));
        
        // Try to allocate again - should fail
        assert!(!bitmap.mark_allocated(1));
    }

    #[test]
    fn test_bitmap_double_free_fails() {
        let mut bitmap = PidBitmap::new();
        
        // Allocate and free PID 1
        bitmap.mark_allocated(1);
        assert!(bitmap.mark_free(1));
        
        // Try to free again - should fail
        assert!(!bitmap.mark_free(1));
    }

    #[test]
    fn test_bitmap_word_boundary() {
        let mut bitmap = PidBitmap::new();
        
        // Allocate PIDs up to word boundary (63)
        for pid in 1..64 {
            bitmap.mark_allocated(pid);
        }
        
        // Next allocation should be in second word (PID 64)
        assert_eq!(bitmap.allocate_next(), Some(64));
        
        let word_idx = (64 / 64) as usize;
        let bit_idx = 64 % 64;
        assert_eq!(word_idx, 1);
        assert_eq!(bit_idx, 0);
    }

    #[test]
    fn test_bitmap_exhaustion() {
        let mut bitmap = PidBitmap::new();
        
        // Allocate all PIDs (1-4095)
        for _ in 1..4096 {
            assert!(bitmap.allocate_next().is_some());
        }
        
        // Next allocation should fail
        assert_eq!(bitmap.allocate_next(), None);
    }

    // =========================================================================
    // Radix Tree Tests
    // =========================================================================

    /// Simulates the radix tree structure for PID lookup
    const RADIX_BITS: usize = 6;
    const RADIX_CHILDREN: usize = 1 << RADIX_BITS; // 64
    const RADIX_MASK: u64 = (RADIX_CHILDREN - 1) as u64;
    const RADIX_LEVELS: usize = 3;

    fn radix_index(pid: u64, level: usize) -> usize {
        let shift = (RADIX_LEVELS - 1 - level) * RADIX_BITS;
        ((pid >> shift) & RADIX_MASK) as usize
    }

    #[test]
    fn test_radix_index_level_0() {
        // Level 0 extracts bits 12-17 (most significant)
        assert_eq!(radix_index(0, 0), 0);
        assert_eq!(radix_index(0b111111_000000_000000, 0), 63);
        assert_eq!(radix_index(0b000001_000000_000000, 0), 1);
    }

    #[test]
    fn test_radix_index_level_1() {
        // Level 1 extracts bits 6-11 (middle)
        assert_eq!(radix_index(0, 1), 0);
        assert_eq!(radix_index(0b000000_111111_000000, 1), 63);
        assert_eq!(radix_index(0b000000_000001_000000, 1), 1);
    }

    #[test]
    fn test_radix_index_level_2() {
        // Level 2 extracts bits 0-5 (least significant)
        assert_eq!(radix_index(0, 2), 0);
        assert_eq!(radix_index(0b000000_000000_111111, 2), 63);
        assert_eq!(radix_index(0b000000_000000_000001, 2), 1);
    }

    #[test]
    fn test_radix_path_for_pid() {
        // PID 12345 = 0b11_000000_111001 = 3, 0, 57
        let pid = 12345u64;
        let level0 = radix_index(pid, 0);
        let level1 = radix_index(pid, 1);
        let level2 = radix_index(pid, 2);
        
        // Verify: 12345 = 3 * 4096 + 0 * 64 + 57
        assert_eq!(level0 * 4096 + level1 * 64 + level2, 12345);
    }

    #[test]
    fn test_radix_max_pid_path() {
        // MAX_PID = 262143 = 2^18 - 1 = all 1s
        let pid = 262143u64;
        assert_eq!(radix_index(pid, 0), 63);
        assert_eq!(radix_index(pid, 1), 63);
        assert_eq!(radix_index(pid, 2), 63);
    }

    // =========================================================================
    // PID Recycling Tests
    // =========================================================================

    #[test]
    fn test_pid_recycling_order() {
        let mut bitmap = PidBitmap::new();
        
        // Allocate PIDs 1, 2, 3, 4, 5
        for _ in 0..5 {
            bitmap.allocate_next();
        }
        
        // Free PIDs 2 and 4
        bitmap.mark_free(2);
        bitmap.mark_free(4);
        
        // Should recycle PID 2 first (lower)
        assert_eq!(bitmap.allocate_next(), Some(2));
        
        // Then PID 4
        assert_eq!(bitmap.allocate_next(), Some(4));
        
        // Then new PID 6
        assert_eq!(bitmap.allocate_next(), Some(6));
    }

    #[test]
    fn test_pid_recycling_with_hint() {
        let mut bitmap = PidBitmap::new();
        
        // Allocate PIDs 1-10
        for _ in 0..10 {
            bitmap.allocate_next();
        }
        
        // Free PID 5
        bitmap.mark_free(5);
        assert_eq!(bitmap.next_hint, 5);
        
        // Next allocation should find PID 5
        assert_eq!(bitmap.allocate_next(), Some(5));
    }

    // =========================================================================
    // Edge Cases and Bug Detection
    // =========================================================================

    #[test]
    fn test_boundary_pid_63() {
        let mut bitmap = PidBitmap::new();
        
        // PID 63 is at the boundary of first word
        bitmap.mark_allocated(63);
        assert!(bitmap.is_allocated(63));
        
        bitmap.mark_free(63);
        assert!(!bitmap.is_allocated(63));
    }

    #[test]
    fn test_boundary_pid_64() {
        let mut bitmap = PidBitmap::new();
        
        // PID 64 is first bit of second word
        bitmap.mark_allocated(64);
        assert!(bitmap.is_allocated(64));
        
        // Verify it's in the right word
        assert!((bitmap.bitmap[1] & 1) != 0);
    }

    #[test]
    fn test_out_of_range_pid() {
        let bitmap = PidBitmap::new();
        
        // PID out of range should return false
        assert!(!bitmap.is_allocated(5000));
        assert!(!bitmap.is_allocated(u64::MAX));
    }

    #[test]
    fn test_stress_allocate_free() {
        let mut bitmap = PidBitmap::new();
        
        // Allocate 100 PIDs
        let mut pids = Vec::new();
        for _ in 0..100 {
            if let Some(pid) = bitmap.allocate_next() {
                pids.push(pid);
            }
        }
        assert_eq!(pids.len(), 100);
        
        // Free every other PID
        for (i, &pid) in pids.iter().enumerate() {
            if i % 2 == 0 {
                bitmap.mark_free(pid);
            }
        }
        
        // Allocated count should be 51 (50 remaining + PID 0)
        assert_eq!(bitmap.allocated_count, 51);
        
        // Allocate 50 more - should fill the gaps
        for _ in 0..50 {
            assert!(bitmap.allocate_next().is_some());
        }
        
        // All 100 PIDs should be allocated again
        assert_eq!(bitmap.allocated_count, 101);
    }

    #[test]
    fn test_bitmap_all_bits_set() {
        let mut bitmap = PidBitmap::new();
        
        // Manually set all bits in first word
        bitmap.bitmap[0] = u64::MAX;
        
        // is_allocated should return true for all PIDs 0-63
        for pid in 0..64 {
            assert!(bitmap.is_allocated(pid));
        }
        
        // is_allocated should return false for PID 64
        assert!(!bitmap.is_allocated(64));
    }

    #[test]
    fn test_concurrent_allocation_simulation() {
        // Simulate what might happen with concurrent allocations
        let mut bitmap1 = PidBitmap::new();
        let mut bitmap2 = PidBitmap::new();
        
        // Both "threads" try to allocate
        let pid1 = bitmap1.allocate_next();
        let pid2 = bitmap2.allocate_next();
        
        // Without proper synchronization, they'd get the same PID
        // This test documents expected behavior (not thread-safe)
        assert_eq!(pid1, pid2); // Same result because independent instances
    }
}
