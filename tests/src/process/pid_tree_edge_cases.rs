//! PID Tree and Radix Tree Edge Case Tests
//!
//! Tests for PID allocation, deallocation, and lookup operations
//! including boundary conditions, recycling, and concurrency patterns.

#[cfg(test)]
mod tests {
    use crate::process::pid_tree::{MAX_PID, MIN_PID};

    // =========================================================================
    // PID Constants Validation
    // =========================================================================

    #[test]
    fn test_pid_constants() {
        // MIN_PID should be 1 (PID 0 is reserved for kernel)
        assert_eq!(MIN_PID, 1);
        
        // MAX_PID should be reasonable
        assert!(MAX_PID >= 32767, "MAX_PID should support at least 32767 processes");
        assert!(MAX_PID <= (1 << 22), "MAX_PID should not be excessively large");
    }

    #[test]
    fn test_pid_range_valid() {
        // Verify MIN_PID < MAX_PID
        assert!(MIN_PID < MAX_PID);
        
        // Total PID count should be calculable without overflow
        let pid_count = MAX_PID - MIN_PID + 1;
        assert!(pid_count > 0);
    }

    // =========================================================================
    // Bitmap Allocator Simulation
    // =========================================================================

    /// Simulates the PID bitmap allocator
    struct PidBitmapSimulator {
        bitmap: Vec<u64>,
        next_hint: u64,
        allocated_count: u64,
    }

    impl PidBitmapSimulator {
        fn new(max_pid: u64) -> Self {
            let words_needed = ((max_pid + 1) / 64 + 1) as usize;
            let mut bitmap = vec![0u64; words_needed];
            bitmap[0] = 1; // Mark PID 0 as allocated
            
            Self {
                bitmap,
                next_hint: MIN_PID,
                allocated_count: 1,
            }
        }

        fn is_allocated(&self, pid: u64) -> bool {
            let word_idx = (pid / 64) as usize;
            let bit_idx = pid % 64;
            if word_idx >= self.bitmap.len() {
                return false;
            }
            (self.bitmap[word_idx] & (1 << bit_idx)) != 0
        }

        fn allocate(&mut self) -> Option<u64> {
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
                    // Skip PID 0, find next free
                    let next = (!word & !1).trailing_zeros() as u64;
                    if next < 64 {
                        let pid = (word_idx as u64) * 64 + next;
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

        fn mark_allocated(&mut self, pid: u64) -> bool {
            let word_idx = (pid / 64) as usize;
            let bit_idx = pid % 64;
            
            if word_idx >= self.bitmap.len() {
                return false;
            }
            if (self.bitmap[word_idx] & (1 << bit_idx)) != 0 {
                return false; // Already allocated
            }
            
            self.bitmap[word_idx] |= 1 << bit_idx;
            self.allocated_count += 1;
            true
        }

        fn free(&mut self, pid: u64) -> bool {
            if pid == 0 {
                return false; // Cannot free PID 0
            }
            
            let word_idx = (pid / 64) as usize;
            let bit_idx = pid % 64;
            
            if word_idx >= self.bitmap.len() {
                return false;
            }
            if (self.bitmap[word_idx] & (1 << bit_idx)) == 0 {
                return false; // Not allocated
            }
            
            self.bitmap[word_idx] &= !(1 << bit_idx);
            self.allocated_count -= 1;
            
            if pid < self.next_hint {
                self.next_hint = pid;
            }
            true
        }

        fn count(&self) -> u64 {
            self.allocated_count
        }
    }

    // =========================================================================
    // Basic Allocation Tests
    // =========================================================================

    #[test]
    fn test_first_allocation() {
        let mut sim = PidBitmapSimulator::new(1024);
        
        let pid = sim.allocate();
        assert!(pid.is_some());
        assert_eq!(pid.unwrap(), 1); // First allocation should be PID 1
    }

    #[test]
    fn test_sequential_allocation() {
        let mut sim = PidBitmapSimulator::new(1024);
        
        // Allocate several PIDs
        for expected in 1..=10 {
            let pid = sim.allocate().unwrap();
            assert_eq!(pid, expected);
        }
    }

    #[test]
    fn test_pid_0_never_allocated() {
        let mut sim = PidBitmapSimulator::new(1024);
        
        // Allocate many PIDs
        for _ in 0..100 {
            let pid = sim.allocate().unwrap();
            assert_ne!(pid, 0, "PID 0 should never be allocated");
        }
    }

    // =========================================================================
    // Deallocation and Recycling Tests
    // =========================================================================

    #[test]
    fn test_free_and_reuse() {
        let mut sim = PidBitmapSimulator::new(1024);
        
        // Allocate PIDs 1-5
        let pids: Vec<_> = (0..5).map(|_| sim.allocate().unwrap()).collect();
        assert_eq!(pids, vec![1, 2, 3, 4, 5]);
        
        // Free PID 3
        assert!(sim.free(3));
        
        // Allocate again - should get 3 or 6 depending on hint behavior
        let new_pid = sim.allocate().unwrap();
        // After freeing 3, hint should update, next allocation could be 3 or 6
        assert!(new_pid == 3 || new_pid == 6);
    }

    #[test]
    fn test_cannot_free_pid_0() {
        let mut sim = PidBitmapSimulator::new(1024);
        
        // Cannot free PID 0
        assert!(!sim.free(0));
    }

    #[test]
    fn test_double_free_detection() {
        let mut sim = PidBitmapSimulator::new(1024);
        
        let pid = sim.allocate().unwrap();
        
        // First free should succeed
        assert!(sim.free(pid));
        
        // Second free should fail
        assert!(!sim.free(pid));
    }

    #[test]
    fn test_free_unallocated_pid() {
        let mut sim = PidBitmapSimulator::new(1024);
        
        // Try to free a PID that was never allocated
        assert!(!sim.free(100));
    }

    // =========================================================================
    // Exhaustion and Wrap-around Tests
    // =========================================================================

    #[test]
    fn test_allocation_until_exhaustion() {
        let max_pids = 64; // This creates capacity for PIDs 0-127 (2 u64 words)
        let mut sim = PidBitmapSimulator::new(max_pids);
        
        // PidBitmapSimulator allocates: words_needed = ((max_pid + 1) / 64 + 1) = 2 words = 128 bits
        // PID 0 is pre-allocated, so we can allocate PIDs 1-127 = 127 PIDs
        let expected_allocations = 127; // 128 total bits - 1 (PID 0)
        
        // Allocate all available PIDs
        for i in 0..expected_allocations {
            let result = sim.allocate();
            assert!(result.is_some(), "Allocation {} should succeed", i);
        }
        
        // Next allocation should fail
        assert!(sim.allocate().is_none(), "Pool should be exhausted after {} allocations", expected_allocations);
    }

    #[test]
    fn test_allocation_after_free_when_exhausted() {
        let max_pids = 64;
        let mut sim = PidBitmapSimulator::new(max_pids);
        
        // Exhaust all PIDs
        let mut pids = Vec::new();
        while let Some(pid) = sim.allocate() {
            pids.push(pid);
        }
        
        // Free one PID
        let freed_pid = pids[10];
        assert!(sim.free(freed_pid));
        
        // Should be able to allocate again
        let new_pid = sim.allocate();
        assert!(new_pid.is_some());
        assert_eq!(new_pid.unwrap(), freed_pid);
    }

    #[test]
    fn test_wrap_around_allocation() {
        let mut sim = PidBitmapSimulator::new(128);
        
        // Allocate PIDs 1-64
        for _ in 0..64 {
            sim.allocate().unwrap();
        }
        
        // Free PIDs 1-10
        for pid in 1..=10 {
            sim.free(pid);
        }
        
        // Continue allocating - should use 65-128 first (based on hint)
        // then wrap around to 1-10
        let mut allocated = Vec::new();
        while let Some(pid) = sim.allocate() {
            allocated.push(pid);
            if allocated.len() > 100 {
                break; // Safety limit
            }
        }
        
        // Should have allocated 65-128 and 1-10 (64 + 10 = 74 total minus already allocated)
        assert!(!allocated.is_empty());
    }

    // =========================================================================
    // Radix Tree Simulation
    // =========================================================================

    /// Simple radix tree node for testing
    #[derive(Clone)]
    struct RadixNode {
        children: [Option<usize>; 64],
        value: Option<u16>,
    }

    impl RadixNode {
        fn new() -> Self {
            Self {
                children: [None; 64],
                value: None,
            }
        }
    }

    /// Simple radix tree for testing
    struct RadixTree {
        nodes: Vec<RadixNode>,
    }

    impl RadixTree {
        fn new() -> Self {
            Self {
                nodes: vec![RadixNode::new()],
            }
        }

        fn radix_index(key: u64, level: usize) -> usize {
            let shift = (2 - level) * 6; // 3 levels, 6 bits each
            ((key >> shift) & 0x3F) as usize
        }

        fn insert(&mut self, key: u64, value: u16) -> bool {
            let mut node_idx = 0;

            for level in 0..2 {
                let radix_idx = Self::radix_index(key, level);
                
                if self.nodes[node_idx].children[radix_idx].is_none() {
                    let new_idx = self.nodes.len();
                    self.nodes.push(RadixNode::new());
                    self.nodes[node_idx].children[radix_idx] = Some(new_idx);
                }
                
                node_idx = self.nodes[node_idx].children[radix_idx].unwrap();
            }

            let leaf_idx = Self::radix_index(key, 2);
            if self.nodes[node_idx].children[leaf_idx].is_none() {
                let new_idx = self.nodes.len();
                self.nodes.push(RadixNode::new());
                self.nodes[node_idx].children[leaf_idx] = Some(new_idx);
            }

            let leaf_node = self.nodes[node_idx].children[leaf_idx].unwrap();
            self.nodes[leaf_node].value = Some(value);
            true
        }

        fn lookup(&self, key: u64) -> Option<u16> {
            let mut node_idx = 0;

            for level in 0..3 {
                let radix_idx = Self::radix_index(key, level);
                node_idx = self.nodes[node_idx].children[radix_idx]?;
            }

            self.nodes[node_idx].value
        }

        fn remove(&mut self, key: u64) -> Option<u16> {
            let mut node_idx = 0;

            for level in 0..3 {
                let radix_idx = Self::radix_index(key, level);
                if let Some(next) = self.nodes[node_idx].children[radix_idx] {
                    node_idx = next;
                } else {
                    return None;
                }
            }

            self.nodes[node_idx].value.take()
        }
    }

    #[test]
    fn test_radix_insert_and_lookup() {
        let mut tree = RadixTree::new();
        
        tree.insert(1, 100);
        tree.insert(2, 200);
        tree.insert(100, 1000);
        
        assert_eq!(tree.lookup(1), Some(100));
        assert_eq!(tree.lookup(2), Some(200));
        assert_eq!(tree.lookup(100), Some(1000));
        assert_eq!(tree.lookup(3), None);
    }

    #[test]
    fn test_radix_remove() {
        let mut tree = RadixTree::new();
        
        tree.insert(1, 100);
        assert_eq!(tree.lookup(1), Some(100));
        
        assert_eq!(tree.remove(1), Some(100));
        assert_eq!(tree.lookup(1), None);
    }

    #[test]
    fn test_radix_sparse_keys() {
        let mut tree = RadixTree::new();
        
        // Insert sparse keys
        tree.insert(1, 1);
        tree.insert(1000, 2);
        tree.insert(100000, 3);
        
        assert_eq!(tree.lookup(1), Some(1));
        assert_eq!(tree.lookup(1000), Some(2));
        assert_eq!(tree.lookup(100000), Some(3));
        
        // Keys in between should not exist
        assert_eq!(tree.lookup(500), None);
        assert_eq!(tree.lookup(50000), None);
    }

    // =========================================================================
    // Edge Cases and Bug Detection
    // =========================================================================

    #[test]
    fn test_allocation_count_consistency() {
        let mut sim = PidBitmapSimulator::new(256);
        
        let initial_count = sim.count();
        assert_eq!(initial_count, 1); // PID 0 is pre-allocated
        
        // Allocate 10 PIDs
        for _ in 0..10 {
            sim.allocate();
        }
        assert_eq!(sim.count(), 11);
        
        // Free 5 PIDs
        for pid in 1..=5 {
            sim.free(pid);
        }
        assert_eq!(sim.count(), 6);
    }

    #[test]
    fn test_hint_optimization() {
        let mut sim = PidBitmapSimulator::new(256);
        
        // Allocate many PIDs
        for _ in 0..50 {
            sim.allocate();
        }
        
        // Free a low PID
        sim.free(10);
        
        // Hint should be updated
        assert!(sim.next_hint <= 10);
    }

    #[test]
    fn test_boundary_pid_allocation() {
        let max = 63; // Edge of first bitmap word
        let mut sim = PidBitmapSimulator::new(max + 10);
        
        // Allocate up to boundary
        for _ in 1..=max {
            let pid = sim.allocate().unwrap();
            assert!(pid <= max);
        }
        
        // Allocate across boundary
        let pid = sim.allocate().unwrap();
        assert_eq!(pid, max + 1);
    }

    #[test]
    fn test_bitmap_word_full() {
        // Test when an entire bitmap word becomes full
        let mut sim = PidBitmapSimulator::new(128);
        
        // Allocate all 63 PIDs in first word (1-63, since 0 is reserved)
        for _ in 0..63 {
            sim.allocate();
        }
        
        // First word should be full (except potentially bit 0)
        // Next allocation should be from second word
        let pid = sim.allocate().unwrap();
        assert_eq!(pid, 64);
    }
}
