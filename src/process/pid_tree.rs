//! Radix tree based PID management
//!
//! This module provides efficient PID allocation, deallocation, and lookup
//! using a radix tree data structure. The radix tree provides O(log N) time
//! complexity for all operations and supports PID recycling.
//!
//! ## Design
//!
//! The radix tree uses a 64-way branching factor (6 bits per level), which
//! means for PIDs up to 2^18 (262144), we need at most 3 levels.
//!
//! Key features:
//! - O(log N) PID allocation with recycling
//! - O(log N) PID lookup
//! - O(log N) PID deallocation
//! - Bitmap-based free PID tracking for fast allocation
//!
//! ## Usage
//!
//! ```rust
//! use crate::process::pid_tree;
//!
//! // Allocate a new PID
//! let pid = pid_tree::allocate_pid();
//!
//! // Register the PID in the radix tree with its process table index
//! pid_tree::register_pid_mapping(pid, process_table_idx);
//!
//! // Look up process table index by PID (O(log N))
//! if let Some(idx) = pid_tree::lookup_pid(pid) {
//!     // Use idx to access process in table
//! }
//!
//! // When process exits, free the PID for reuse
//! pid_tree::free_pid(pid);
//! ```

use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// Maximum supported PID value (2^18 - 1 = 262143)
pub const MAX_PID: u64 = (1 << 18) - 1;

/// Minimum PID value (PID 0 is reserved for kernel/idle)
pub const MIN_PID: u64 = 1;

/// Number of bits per radix tree level
const RADIX_BITS: usize = 6;

/// Number of children per node (2^6 = 64)
const RADIX_CHILDREN: usize = 1 << RADIX_BITS;

/// Mask for extracting radix index
const RADIX_MASK: u64 = (RADIX_CHILDREN - 1) as u64;

/// Number of levels in the tree (18 bits / 6 bits = 3 levels)
const RADIX_LEVELS: usize = 3;

/// PID allocator state
struct PidAllocator {
    /// Bitmap for tracking allocated PIDs (4096 PIDs per u64, total 64 u64s = 4096 PIDs tracked)
    /// Each bit represents whether a PID is allocated (1) or free (0)
    bitmap: [u64; 64],
    /// Next hint for PID allocation (helps find free PIDs faster)
    next_hint: u64,
    /// Number of allocated PIDs
    allocated_count: u64,
}

impl PidAllocator {
    const fn new() -> Self {
        // Initialize with PID 0 marked as allocated (reserved for kernel)
        let mut bitmap = [0u64; 64];
        bitmap[0] = 1; // Mark PID 0 as allocated
        Self {
            bitmap,
            next_hint: MIN_PID,
            allocated_count: 1, // PID 0 is pre-allocated
        }
    }

    /// Check if a PID is allocated
    #[inline]
    fn is_allocated(&self, pid: u64) -> bool {
        if pid > MAX_PID {
            return false;
        }
        let word_idx = (pid / 64) as usize;
        let bit_idx = pid % 64;
        if word_idx >= self.bitmap.len() {
            return false;
        }
        (self.bitmap[word_idx] & (1 << bit_idx)) != 0
    }

    /// Mark a PID as allocated
    #[inline]
    fn mark_allocated(&mut self, pid: u64) -> bool {
        if pid > MAX_PID {
            return false;
        }
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

    /// Mark a PID as free
    #[inline]
    fn mark_free(&mut self, pid: u64) -> bool {
        if pid == 0 || pid > MAX_PID {
            return false; // PID 0 cannot be freed
        }
        let word_idx = (pid / 64) as usize;
        let bit_idx = pid % 64;
        if word_idx >= self.bitmap.len() {
            return false;
        }
        if (self.bitmap[word_idx] & (1 << bit_idx)) == 0 {
            return false; // Already free
        }
        self.bitmap[word_idx] &= !(1 << bit_idx);
        self.allocated_count -= 1;
        // Update hint if freed PID is lower
        if pid < self.next_hint {
            self.next_hint = pid;
        }
        true
    }

    /// Find and allocate the next free PID
    fn allocate_next(&mut self) -> Option<u64> {
        // Start searching from the hint
        let start_word = (self.next_hint / 64) as usize;

        // Search from hint to end
        for word_idx in start_word..self.bitmap.len() {
            let word = self.bitmap[word_idx];
            if word == u64::MAX {
                continue; // All bits set, no free PIDs in this word
            }

            // Find first zero bit
            let first_zero = (!word).trailing_zeros() as u64;
            let pid = (word_idx as u64) * 64 + first_zero;

            // Skip PID 0 if we're at the first word
            if pid == 0 {
                let next_zero = (!word & !1).trailing_zeros() as u64;
                if next_zero < 64 {
                    let pid = next_zero;
                    if pid <= MAX_PID && self.mark_allocated(pid) {
                        self.next_hint = pid + 1;
                        return Some(pid);
                    }
                }
                continue;
            }

            if pid <= MAX_PID && self.mark_allocated(pid) {
                self.next_hint = pid + 1;
                return Some(pid);
            }
        }

        // Wrap around and search from beginning to hint
        for word_idx in 0..start_word {
            let word = self.bitmap[word_idx];
            if word == u64::MAX {
                continue;
            }

            let first_zero = (!word).trailing_zeros() as u64;
            let pid = (word_idx as u64) * 64 + first_zero;

            if pid == 0 {
                let next_zero = (!word & !1).trailing_zeros() as u64;
                if next_zero < 64 {
                    let pid = next_zero;
                    if pid <= MAX_PID && self.mark_allocated(pid) {
                        self.next_hint = pid + 1;
                        return Some(pid);
                    }
                }
                continue;
            }

            if pid <= MAX_PID && self.mark_allocated(pid) {
                self.next_hint = pid + 1;
                return Some(pid);
            }
        }

        None // No free PIDs available
    }

    /// Get the number of allocated PIDs
    #[inline]
    fn count(&self) -> u64 {
        self.allocated_count
    }
}

/// Radix tree node for process lookup
#[derive(Clone, Copy)]
struct RadixNode {
    /// Child node indices (0 means empty)
    children: [u16; RADIX_CHILDREN],
    /// Process table index stored at this node (for leaf nodes)
    /// u16::MAX means no process stored
    process_idx: u16,
}

impl RadixNode {
    const fn empty() -> Self {
        Self {
            children: [0; RADIX_CHILDREN],
            process_idx: u16::MAX,
        }
    }
}

/// Maximum number of radix tree nodes
/// With 64-way branching and 3 levels, we need at most:
/// - 1 root node
/// - 64 level-1 nodes
/// - 64 * 64 = 4096 level-2 nodes
/// But in practice, we'll use far fewer for sparse PID spaces
const MAX_RADIX_NODES: usize = 256;

/// Radix tree for fast PID to process index lookup
struct PidRadixTree {
    /// Node pool
    nodes: [RadixNode; MAX_RADIX_NODES],
    /// Number of allocated nodes (node 0 is always the root)
    node_count: usize,
}

impl PidRadixTree {
    const fn new() -> Self {
        Self {
            nodes: [RadixNode::empty(); MAX_RADIX_NODES],
            node_count: 1, // Node 0 is the root
        }
    }

    /// Allocate a new node
    fn alloc_node(&mut self) -> Option<u16> {
        if self.node_count >= MAX_RADIX_NODES {
            return None;
        }
        let idx = self.node_count;
        self.nodes[idx] = RadixNode::empty();
        self.node_count += 1;
        Some(idx as u16)
    }

    /// Extract radix index at a given level (0 = most significant)
    #[inline]
    fn radix_index(pid: u64, level: usize) -> usize {
        let shift = (RADIX_LEVELS - 1 - level) * RADIX_BITS;
        ((pid >> shift) & RADIX_MASK) as usize
    }

    /// Insert a PID -> process_idx mapping
    fn insert(&mut self, pid: u64, process_idx: u16) -> bool {
        let mut node_idx: usize = 0;

        // Navigate/create path to leaf
        for level in 0..(RADIX_LEVELS - 1) {
            let radix_idx = Self::radix_index(pid, level);
            let child_idx = self.nodes[node_idx].children[radix_idx];

            if child_idx == 0 {
                // Need to create new node
                let Some(new_idx) = self.alloc_node() else {
                    return false; // Out of nodes
                };
                self.nodes[node_idx].children[radix_idx] = new_idx;
                node_idx = new_idx as usize;
            } else {
                node_idx = child_idx as usize;
            }
        }

        // At leaf level - store the process index
        let leaf_radix_idx = Self::radix_index(pid, RADIX_LEVELS - 1);
        let child_idx = self.nodes[node_idx].children[leaf_radix_idx];

        if child_idx == 0 {
            // Create leaf node
            let Some(new_idx) = self.alloc_node() else {
                return false;
            };
            self.nodes[new_idx as usize].process_idx = process_idx;
            self.nodes[node_idx].children[leaf_radix_idx] = new_idx;
        } else {
            // Update existing leaf
            self.nodes[child_idx as usize].process_idx = process_idx;
        }

        true
    }

    /// Look up process index by PID
    fn lookup(&self, pid: u64) -> Option<u16> {
        let mut node_idx: usize = 0;

        // Navigate to leaf
        for level in 0..RADIX_LEVELS {
            let radix_idx = Self::radix_index(pid, level);
            let child_idx = self.nodes[node_idx].children[radix_idx];

            if child_idx == 0 {
                return None; // Path doesn't exist
            }
            node_idx = child_idx as usize;
        }

        // Check if leaf has a process
        let process_idx = self.nodes[node_idx].process_idx;
        if process_idx == u16::MAX {
            None
        } else {
            Some(process_idx)
        }
    }

    /// Remove a PID mapping (marks leaf as empty, doesn't reclaim nodes)
    fn remove(&mut self, pid: u64) -> Option<u16> {
        let mut node_idx: usize = 0;

        // Navigate to leaf
        for level in 0..RADIX_LEVELS {
            let radix_idx = Self::radix_index(pid, level);
            let child_idx = self.nodes[node_idx].children[radix_idx];

            if child_idx == 0 {
                return None;
            }
            node_idx = child_idx as usize;
        }

        // Clear the process index
        let old_idx = self.nodes[node_idx].process_idx;
        if old_idx == u16::MAX {
            None
        } else {
            self.nodes[node_idx].process_idx = u16::MAX;
            Some(old_idx)
        }
    }
}

/// Combined PID manager with both allocation bitmap and lookup tree
struct PidManager {
    allocator: PidAllocator,
    tree: PidRadixTree,
}

impl PidManager {
    const fn new() -> Self {
        Self {
            allocator: PidAllocator::new(),
            tree: PidRadixTree::new(),
        }
    }
}

/// Global PID manager
static PID_MANAGER: Mutex<PidManager> = Mutex::new(PidManager::new());

/// Legacy atomic counter for backward compatibility during transition
static LEGACY_NEXT_PID: AtomicU64 = AtomicU64::new(1);

/// Allocate a new unique PID using the radix tree allocator
/// Returns a PID that can be recycled when the process exits
pub fn allocate_pid() -> u64 {
    let mut manager = PID_MANAGER.lock();
    manager.allocator.allocate_next().unwrap_or_else(|| {
        // Fallback to legacy counter if radix allocator is exhausted
        // This should rarely happen in practice
        crate::kwarn!("PID radix allocator exhausted, using legacy counter");
        LEGACY_NEXT_PID.fetch_add(1, Ordering::SeqCst)
    })
}

/// Free a PID for reuse
/// Should be called when a process is fully cleaned up
pub fn free_pid(pid: u64) {
    if pid == 0 {
        return; // Never free PID 0
    }
    let mut manager = PID_MANAGER.lock();
    manager.allocator.mark_free(pid);
    manager.tree.remove(pid);
}

/// Register a PID -> process table index mapping
/// This allows O(1) lookup of process by PID
pub fn register_pid_mapping(pid: u64, process_table_idx: u16) -> bool {
    let mut manager = PID_MANAGER.lock();
    manager.tree.insert(pid, process_table_idx)
}

/// Update an existing PID mapping to a new process table index
pub fn update_pid_mapping(pid: u64, new_process_table_idx: u16) -> bool {
    let mut manager = PID_MANAGER.lock();
    // Remove old mapping if exists, then insert new one
    manager.tree.remove(pid);
    manager.tree.insert(pid, new_process_table_idx)
}

/// Look up process table index by PID
/// Returns None if PID is not registered
pub fn lookup_pid(pid: u64) -> Option<u16> {
    let manager = PID_MANAGER.lock();
    manager.tree.lookup(pid)
}

/// Remove a PID mapping (but don't free the PID itself)
/// Used when moving process in the table
pub fn unregister_pid_mapping(pid: u64) -> Option<u16> {
    let mut manager = PID_MANAGER.lock();
    manager.tree.remove(pid)
}

/// Check if a PID is currently allocated
pub fn is_pid_allocated(pid: u64) -> bool {
    let manager = PID_MANAGER.lock();
    manager.allocator.is_allocated(pid)
}

/// Get the number of currently allocated PIDs
pub fn allocated_pid_count() -> u64 {
    let manager = PID_MANAGER.lock();
    manager.allocator.count()
}

/// Allocate a specific PID (for special cases like init process)
/// Returns true if successful, false if PID is already allocated
pub fn allocate_specific_pid(pid: u64) -> bool {
    if pid == 0 || pid > MAX_PID {
        return false;
    }
    let mut manager = PID_MANAGER.lock();
    manager.allocator.mark_allocated(pid)
}

/// Get statistics about the PID radix tree
/// Returns (allocated_pids, radix_tree_nodes)
pub fn get_pid_stats() -> (u64, usize) {
    let manager = PID_MANAGER.lock();
    (manager.allocator.count(), manager.tree.node_count)
}
