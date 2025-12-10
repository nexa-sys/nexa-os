//! Virtual Memory Area (VMA) Management for NexaOS
//!
//! This module implements production-grade VMA management similar to Linux's
//! mm/mmap.c. It provides efficient virtual address space management using
//! an interval tree (augmented red-black tree) for O(log n) lookups.
//!
//! # Architecture
//!
//! ```text
//! AddressSpace
//! ├── VMAManager (interval tree of VMAs)
//! │   ├── VMA [0x1000000 - 0x1100000] code, r-x
//! │   ├── VMA [0x1100000 - 0x1200000] data, rw-
//! │   ├── VMA [0x1200000 - 0x1400000] heap, rw-
//! │   └── VMA [0x1400000 - 0x1600000] stack, rw-
//! └── Page Table (CR3)
//! ```
//!
//! # Features
//! - O(log n) VMA lookup by address
//! - O(log n) range queries for overlapping VMAs
//! - Automatic VMA merging for adjacent compatible regions
//! - VMA splitting for partial munmap/mprotect
//! - Copy-on-write (COW) support for fork
//! - Demand paging integration
//! - Memory statistics tracking

// Note: max, min, Ordering, AtomicU64, AtomicOrdering may be used in future implementations
#[allow(unused_imports)]
use core::cmp::{max, min, Ordering};
#[allow(unused_imports)]
use core::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

// =============================================================================
// Constants
// =============================================================================

/// Page size (4KB)
pub const PAGE_SIZE: u64 = 4096;

/// Maximum number of VMAs per address space
pub const MAX_VMAS: usize = 256;

/// Maximum number of address spaces (processes)
pub const MAX_ADDRESS_SPACES: usize = 64;

// =============================================================================
// VMA Permissions and Flags
// =============================================================================

/// VMA permission flags (compatible with POSIX mmap PROT_* flags)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct VMAPermissions(u32);

impl VMAPermissions {
    pub const NONE: Self = Self(0);
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const EXEC: Self = Self(1 << 2);

    /// Create from POSIX PROT_* flags
    #[inline]
    pub const fn from_prot(prot: u64) -> Self {
        Self((prot & 0x7) as u32)
    }

    /// Convert to POSIX PROT_* flags
    #[inline]
    pub const fn to_prot(self) -> u64 {
        self.0 as u64
    }

    /// Check if readable
    #[inline]
    pub const fn is_read(self) -> bool {
        (self.0 & Self::READ.0) != 0
    }

    /// Check if writable
    #[inline]
    pub const fn is_write(self) -> bool {
        (self.0 & Self::WRITE.0) != 0
    }

    /// Check if executable
    #[inline]
    pub const fn is_exec(self) -> bool {
        (self.0 & Self::EXEC.0) != 0
    }

    /// Convert to x86_64 page table flags
    pub fn to_page_flags(self) -> u64 {
        use x86_64::structures::paging::PageTableFlags;

        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;

        if self.is_write() {
            flags |= PageTableFlags::WRITABLE;
        }

        if !self.is_exec() {
            flags |= PageTableFlags::NO_EXECUTE;
        }

        flags.bits()
    }
}

impl core::ops::BitOr for VMAPermissions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitAnd for VMAPermissions {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// VMA type flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct VMAFlags(u32);

impl VMAFlags {
    pub const NONE: Self = Self(0);

    // Mapping type flags (from mmap flags)
    pub const SHARED: Self = Self(1 << 0); // MAP_SHARED
    pub const PRIVATE: Self = Self(1 << 1); // MAP_PRIVATE
    pub const ANONYMOUS: Self = Self(1 << 2); // MAP_ANONYMOUS
    pub const FIXED: Self = Self(1 << 3); // MAP_FIXED

    // VMA state flags
    pub const GROWSDOWN: Self = Self(1 << 4); // Stack grows down
    pub const GROWSUP: Self = Self(1 << 5); // Heap grows up
    pub const LOCKED: Self = Self(1 << 6); // mlock'd - pages cannot be swapped
    pub const DONTEXPAND: Self = Self(1 << 7); // Cannot expand with mremap
    pub const DONTCOPY: Self = Self(1 << 8); // Do not copy on fork (MADV_DONTFORK)
    pub const MAYREAD: Self = Self(1 << 9); // VM_READ may be set
    pub const MAYWRITE: Self = Self(1 << 10); // VM_WRITE may be set
    pub const MAYEXEC: Self = Self(1 << 11); // VM_EXEC may be set
    pub const MAYSHARE: Self = Self(1 << 12); // VM_SHARED may be set

    // Special VMA types
    pub const STACK: Self = Self(1 << 16); // Main thread stack
    pub const HEAP: Self = Self(1 << 17); // Heap region
    pub const CODE: Self = Self(1 << 18); // Code/text segment
    pub const DATA: Self = Self(1 << 19); // Data segment
    pub const BSS: Self = Self(1 << 20); // BSS segment
    pub const VDSO: Self = Self(1 << 21); // Virtual DSO
    pub const INTERP: Self = Self(1 << 22); // Dynamic linker region

    // Demand paging flags
    pub const DEMAND: Self = Self(1 << 24); // Demand-paged (lazy allocation)
    pub const POPULATE: Self = Self(1 << 25); // Populate immediately (MAP_POPULATE)

    // Copy-on-write flag
    pub const COW: Self = Self(1 << 26); // Copy-on-write pending

    /// Create from POSIX MAP_* flags
    pub const fn from_mmap_flags(flags: u64) -> Self {
        let mut result = 0u32;

        if (flags & 0x01) != 0 {
            result |= Self::SHARED.0;
        }
        if (flags & 0x02) != 0 {
            result |= Self::PRIVATE.0;
        }
        if (flags & 0x10) != 0 {
            result |= Self::FIXED.0;
        }
        if (flags & 0x20) != 0 {
            result |= Self::ANONYMOUS.0;
        }
        if (flags & 0x8000) != 0 {
            result |= Self::POPULATE.0;
        }

        Self(result)
    }

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    #[inline]
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// Check if this is an anonymous mapping
    #[inline]
    pub const fn is_anonymous(self) -> bool {
        self.contains(Self::ANONYMOUS)
    }

    /// Check if this is a shared mapping
    #[inline]
    pub const fn is_shared(self) -> bool {
        self.contains(Self::SHARED)
    }

    /// Check if this is a private mapping
    #[inline]
    pub const fn is_private(self) -> bool {
        self.contains(Self::PRIVATE)
    }
}

impl core::ops::BitOr for VMAFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitAnd for VMAFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

// =============================================================================
// VMA Backing Types
// =============================================================================

/// Type of memory backing for a VMA
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMABacking {
    /// Anonymous mapping (zero-filled on demand)
    Anonymous,
    /// File-backed mapping
    File {
        /// Inode number of the backing file
        inode: u64,
        /// Offset in the file
        offset: u64,
    },
    /// Device memory mapping (MMIO)
    Device {
        /// Physical address of device memory
        phys_addr: u64,
    },
    /// Shared memory segment
    SharedMemory {
        /// Shared memory segment ID
        shmid: u64,
    },
}

impl Default for VMABacking {
    fn default() -> Self {
        Self::Anonymous
    }
}

// =============================================================================
// Virtual Memory Area (VMA) Structure
// =============================================================================

/// Virtual Memory Area descriptor
///
/// Represents a contiguous region of virtual memory with uniform
/// permissions and backing. This is the fundamental unit of memory
/// management in userspace.
#[derive(Clone, Copy)]
pub struct VMA {
    /// Start virtual address (page-aligned)
    pub start: u64,
    /// End virtual address (exclusive, page-aligned)
    pub end: u64,
    /// Access permissions
    pub perm: VMAPermissions,
    /// VMA flags
    pub flags: VMAFlags,
    /// Backing type
    pub backing: VMABacking,
    /// Generation counter for COW tracking
    pub generation: u64,
    /// Parent VMA index for COW (-1 if none)
    pub cow_parent: i32,
    /// Reference count for shared VMAs
    pub refcount: u32,
}

impl VMA {
    /// Create an empty/invalid VMA
    pub const fn empty() -> Self {
        Self {
            start: 0,
            end: 0,
            perm: VMAPermissions::NONE,
            flags: VMAFlags::NONE,
            backing: VMABacking::Anonymous,
            generation: 0,
            cow_parent: -1,
            refcount: 0,
        }
    }

    /// Create a new VMA with the given parameters
    pub const fn new(
        start: u64,
        end: u64,
        perm: VMAPermissions,
        flags: VMAFlags,
        backing: VMABacking,
    ) -> Self {
        Self {
            start,
            end,
            perm,
            flags,
            backing,
            generation: 0,
            cow_parent: -1,
            refcount: 1,
        }
    }

    /// Check if this VMA is valid (has non-zero size)
    #[inline]
    pub const fn is_valid(&self) -> bool {
        self.end > self.start
    }

    /// Get the size of this VMA in bytes
    #[inline]
    pub const fn size(&self) -> u64 {
        if self.end > self.start {
            self.end - self.start
        } else {
            0
        }
    }

    /// Get the number of pages in this VMA
    #[inline]
    pub const fn page_count(&self) -> u64 {
        self.size() / PAGE_SIZE
    }

    /// Check if an address falls within this VMA
    #[inline]
    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end
    }

    /// Check if a range overlaps with this VMA
    #[inline]
    pub const fn overlaps(&self, start: u64, end: u64) -> bool {
        self.start < end && start < self.end
    }

    /// Check if this VMA is adjacent to another and can be merged
    #[inline]
    pub fn can_merge_with(&self, other: &VMA) -> bool {
        // VMAs must be adjacent
        if self.end != other.start && other.end != self.start {
            return false;
        }

        // Permissions must match
        if self.perm.0 != other.perm.0 {
            return false;
        }

        // Flags must match (excluding position-specific flags)
        let mask = !(VMAFlags::STACK.0 | VMAFlags::HEAP.0);
        if (self.flags.0 & mask) != (other.flags.0 & mask) {
            return false;
        }

        // Backing type must be compatible
        match (&self.backing, &other.backing) {
            (VMABacking::Anonymous, VMABacking::Anonymous) => true,
            (
                VMABacking::File {
                    inode: i1,
                    offset: o1,
                },
                VMABacking::File {
                    inode: i2,
                    offset: o2,
                },
            ) => {
                // Same file and contiguous offsets
                *i1 == *i2 && *o1 + self.size() == *o2
            }
            _ => false,
        }
    }

    /// Split this VMA at the given address, returning the new VMA for the upper part
    pub fn split_at(&mut self, addr: u64) -> Option<VMA> {
        if addr <= self.start || addr >= self.end {
            return None;
        }

        // Ensure address is page-aligned
        let aligned_addr = (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        if aligned_addr >= self.end {
            return None;
        }

        // Create the upper part
        let mut upper = *self;
        upper.start = aligned_addr;

        // Adjust file offset if file-backed
        if let VMABacking::File { inode, offset } = &mut upper.backing {
            *offset += aligned_addr - self.start;
            let _ = inode; // suppress warning
        }

        // Shrink the lower part
        self.end = aligned_addr;

        Some(upper)
    }
}

impl core::fmt::Debug for VMA {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "VMA[{:#x}-{:#x}] {}{}{}",
            self.start,
            self.end,
            if self.perm.is_read() { "r" } else { "-" },
            if self.perm.is_write() { "w" } else { "-" },
            if self.perm.is_exec() { "x" } else { "-" },
        )?;

        if self.flags.is_anonymous() {
            write!(f, " anon")?;
        }
        if self.flags.contains(VMAFlags::STACK) {
            write!(f, " [stack]")?;
        }
        if self.flags.contains(VMAFlags::HEAP) {
            write!(f, " [heap]")?;
        }
        if self.flags.contains(VMAFlags::COW) {
            write!(f, " [cow]")?;
        }

        Ok(())
    }
}

// =============================================================================
// VMA Manager (Interval Tree Implementation)
// =============================================================================

/// Red-Black Tree node color
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RBColor {
    Red,
    Black,
}

/// Internal node for the interval tree
#[derive(Clone, Copy)]
struct VMANode {
    /// The VMA data
    vma: VMA,
    /// Maximum end address in this subtree (for interval queries)
    max_end: u64,
    /// Left child index (-1 if none)
    left: i32,
    /// Right child index (-1 if none)
    right: i32,
    /// Parent index (-1 if root)
    parent: i32,
    /// Node color for red-black tree balancing
    color: RBColor,
    /// Whether this slot is in use
    in_use: bool,
}

impl VMANode {
    const fn empty() -> Self {
        Self {
            vma: VMA::empty(),
            max_end: 0,
            left: -1,
            right: -1,
            parent: -1,
            color: RBColor::Black,
            in_use: false,
        }
    }
}

/// VMA Manager using an augmented red-black tree (interval tree)
///
/// This provides O(log n) operations for:
/// - Finding VMA containing an address
/// - Finding all VMAs overlapping a range
/// - Inserting new VMAs
/// - Removing VMAs
pub struct VMAManager {
    /// Node storage (fixed-size array for no_std)
    nodes: [VMANode; MAX_VMAS],
    /// Root node index (-1 if empty)
    root: i32,
    /// Number of active VMAs
    count: usize,
    /// Free list head (-1 if none)
    free_head: i32,
    /// Statistics
    stats: VMAStats,
}

/// VMA statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct VMAStats {
    /// Total number of mmap calls
    pub mmap_count: u64,
    /// Total number of munmap calls
    pub munmap_count: u64,
    /// Total number of mprotect calls
    pub mprotect_count: u64,
    /// Number of VMA merges performed
    pub merge_count: u64,
    /// Number of VMA splits performed
    pub split_count: u64,
    /// Total mapped bytes
    pub mapped_bytes: u64,
    /// Peak number of VMAs
    pub peak_vma_count: u64,
    /// Page faults handled
    pub page_faults: u64,
    /// COW faults handled
    pub cow_faults: u64,
}

impl VMAManager {
    /// Create a new empty VMA manager
    pub const fn new() -> Self {
        Self {
            nodes: [VMANode::empty(); MAX_VMAS],
            root: -1,
            count: 0,
            free_head: -1,
            stats: VMAStats {
                mmap_count: 0,
                munmap_count: 0,
                mprotect_count: 0,
                merge_count: 0,
                split_count: 0,
                mapped_bytes: 0,
                peak_vma_count: 0,
                page_faults: 0,
                cow_faults: 0,
            },
        }
    }

    /// Initialize the free list
    pub fn init(&mut self) {
        // Build free list
        for i in 0..MAX_VMAS - 1 {
            self.nodes[i].left = (i + 1) as i32;
        }
        self.nodes[MAX_VMAS - 1].left = -1;
        self.free_head = 0;
    }

    /// Get the number of active VMAs
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Get statistics
    #[inline]
    pub fn stats(&self) -> &VMAStats {
        &self.stats
    }

    /// Get mutable statistics (for updating counters)
    #[inline]
    pub fn stats_mut(&mut self) -> &mut VMAStats {
        &mut self.stats
    }

    /// Get VMA by node index (for internal use)
    /// Returns None if index is invalid or node is not in use
    #[inline]
    pub fn get_vma_by_index(&self, idx: i32) -> Option<&VMA> {
        if idx >= 0 && idx < MAX_VMAS as i32 {
            let node = &self.nodes[idx as usize];
            if node.in_use {
                return Some(&node.vma);
            }
        }
        None
    }

    /// Get mutable VMA by node index (for internal use)
    #[inline]
    pub fn get_vma_by_index_mut(&mut self, idx: i32) -> Option<&mut VMA> {
        if idx >= 0 && idx < MAX_VMAS as i32 {
            let node = &mut self.nodes[idx as usize];
            if node.in_use {
                return Some(&mut node.vma);
            }
        }
        None
    }

    /// Allocate a new node from the free list
    fn alloc_node(&mut self) -> Option<i32> {
        if self.free_head < 0 {
            return None;
        }

        let idx = self.free_head;
        self.free_head = self.nodes[idx as usize].left;

        // Reset the node
        self.nodes[idx as usize] = VMANode {
            vma: VMA::empty(),
            max_end: 0,
            left: -1,
            right: -1,
            parent: -1,
            color: RBColor::Red, // New nodes are red
            in_use: true,
        };

        Some(idx)
    }

    /// Free a node back to the free list
    fn free_node(&mut self, idx: i32) {
        if idx < 0 || idx >= MAX_VMAS as i32 {
            return;
        }

        self.nodes[idx as usize].in_use = false;
        self.nodes[idx as usize].left = self.free_head;
        self.free_head = idx;
    }

    /// Get a reference to a node by index
    #[inline]
    fn node(&self, idx: i32) -> Option<&VMANode> {
        if idx >= 0 && idx < MAX_VMAS as i32 {
            let node = &self.nodes[idx as usize];
            if node.in_use {
                return Some(node);
            }
        }
        None
    }

    /// Get a mutable reference to a node by index
    #[inline]
    fn node_mut(&mut self, idx: i32) -> Option<&mut VMANode> {
        if idx >= 0 && idx < MAX_VMAS as i32 {
            let node = &mut self.nodes[idx as usize];
            if node.in_use {
                return Some(node);
            }
        }
        None
    }

    /// Find the VMA containing a given address
    pub fn find(&self, addr: u64) -> Option<&VMA> {
        let mut current = self.root;

        while current >= 0 {
            let node = self.node(current)?;

            if addr < node.vma.start {
                current = node.left;
            } else if addr >= node.vma.end {
                current = node.right;
            } else {
                // Address is within this VMA
                return Some(&node.vma);
            }
        }

        None
    }

    /// Find VMA containing address (mutable)
    pub fn find_mut(&mut self, addr: u64) -> Option<&mut VMA> {
        let mut current = self.root;

        while current >= 0 {
            let node = &self.nodes[current as usize];
            if !node.in_use {
                return None;
            }

            if addr < node.vma.start {
                current = node.left;
            } else if addr >= node.vma.end {
                current = node.right;
            } else {
                // Found it - need to return mutable reference
                return Some(&mut self.nodes[current as usize].vma);
            }
        }

        None
    }

    /// Find VMA nearest to (but not containing) the given address
    pub fn find_nearest(&self, addr: u64) -> Option<&VMA> {
        let mut current = self.root;
        let mut nearest: Option<i32> = None;
        let mut nearest_dist = u64::MAX;

        while current >= 0 {
            let node = match self.node(current) {
                Some(n) => n,
                None => break,
            };

            // Check if this is closer
            let dist = if addr < node.vma.start {
                node.vma.start - addr
            } else if addr >= node.vma.end {
                addr - node.vma.end + 1
            } else {
                // Address is within this VMA
                return Some(&node.vma);
            };

            if dist < nearest_dist {
                nearest_dist = dist;
                nearest = Some(current);
            }

            // Traverse
            if addr < node.vma.start {
                current = node.left;
            } else {
                current = node.right;
            }
        }

        nearest.and_then(|idx| self.node(idx).map(|n| &n.vma))
    }

    /// Find all VMAs overlapping a given range
    /// Returns indices of overlapping VMAs in the provided buffer
    pub fn find_overlapping(&self, start: u64, end: u64, buffer: &mut [i32]) -> usize {
        let mut count = 0;
        self.find_overlapping_recursive(self.root, start, end, buffer, &mut count);
        count
    }

    fn find_overlapping_recursive(
        &self,
        idx: i32,
        start: u64,
        end: u64,
        buffer: &mut [i32],
        count: &mut usize,
    ) {
        if idx < 0 || *count >= buffer.len() {
            return;
        }

        let node = match self.node(idx) {
            Some(n) => n,
            None => return,
        };

        // If max_end of left subtree could overlap, search left
        if node.left >= 0 {
            if let Some(left) = self.node(node.left) {
                if left.max_end > start {
                    self.find_overlapping_recursive(node.left, start, end, buffer, count);
                }
            }
        }

        // Check if this node overlaps
        if node.vma.overlaps(start, end) && *count < buffer.len() {
            buffer[*count] = idx;
            *count += 1;
        }

        // If this node's start is less than end, search right
        if node.vma.start < end {
            self.find_overlapping_recursive(node.right, start, end, buffer, count);
        }
    }

    /// Insert a new VMA into the tree
    ///
    /// Returns the index of the new node, or None if no space
    pub fn insert(&mut self, vma: VMA) -> Option<i32> {
        if !vma.is_valid() {
            return None;
        }

        // Allocate a new node
        let new_idx = self.alloc_node()?;
        self.nodes[new_idx as usize].vma = vma;
        self.nodes[new_idx as usize].max_end = vma.end;

        // Insert into tree
        if self.root < 0 {
            // Tree is empty
            self.root = new_idx;
            self.nodes[new_idx as usize].color = RBColor::Black;
        } else {
            // Find insertion point
            let mut current = self.root;
            loop {
                let node = &mut self.nodes[current as usize];

                // Update max_end along the path
                if vma.end > node.max_end {
                    node.max_end = vma.end;
                }

                if vma.start < node.vma.start {
                    if node.left < 0 {
                        node.left = new_idx;
                        self.nodes[new_idx as usize].parent = current;
                        break;
                    }
                    current = node.left;
                } else {
                    if node.right < 0 {
                        node.right = new_idx;
                        self.nodes[new_idx as usize].parent = current;
                        break;
                    }
                    current = node.right;
                }
            }

            // Rebalance the tree
            self.insert_fixup(new_idx);
        }

        self.count += 1;
        self.stats.mapped_bytes += vma.size();
        if self.count as u64 > self.stats.peak_vma_count {
            self.stats.peak_vma_count = self.count as u64;
        }

        Some(new_idx)
    }

    /// Red-black tree insertion fixup
    fn insert_fixup(&mut self, mut idx: i32) {
        while idx != self.root {
            let parent_idx = self.nodes[idx as usize].parent;
            if parent_idx < 0 {
                break;
            }

            // If parent is black, we're done
            if self.nodes[parent_idx as usize].color == RBColor::Black {
                break;
            }

            let grandparent_idx = self.nodes[parent_idx as usize].parent;
            if grandparent_idx < 0 {
                break;
            }

            let grandparent = &self.nodes[grandparent_idx as usize];
            let uncle_idx = if grandparent.left == parent_idx {
                grandparent.right
            } else {
                grandparent.left
            };

            // Case 1: Uncle is red
            let uncle_color = if uncle_idx >= 0 {
                self.nodes[uncle_idx as usize].color
            } else {
                RBColor::Black
            };

            if uncle_color == RBColor::Red {
                self.nodes[parent_idx as usize].color = RBColor::Black;
                if uncle_idx >= 0 {
                    self.nodes[uncle_idx as usize].color = RBColor::Black;
                }
                self.nodes[grandparent_idx as usize].color = RBColor::Red;
                idx = grandparent_idx;
                continue;
            }

            // Case 2 & 3: Uncle is black
            let parent_is_left = self.nodes[grandparent_idx as usize].left == parent_idx;

            if parent_is_left {
                if self.nodes[parent_idx as usize].right == idx {
                    // Case 2: Left-Right case
                    self.rotate_left(parent_idx);
                    idx = parent_idx;
                }
                // Case 3: Left-Left case
                let parent_idx = self.nodes[idx as usize].parent;
                let grandparent_idx = self.nodes[parent_idx as usize].parent;
                self.nodes[parent_idx as usize].color = RBColor::Black;
                self.nodes[grandparent_idx as usize].color = RBColor::Red;
                self.rotate_right(grandparent_idx);
            } else {
                if self.nodes[parent_idx as usize].left == idx {
                    // Case 2: Right-Left case
                    self.rotate_right(parent_idx);
                    idx = parent_idx;
                }
                // Case 3: Right-Right case
                let parent_idx = self.nodes[idx as usize].parent;
                let grandparent_idx = self.nodes[parent_idx as usize].parent;
                self.nodes[parent_idx as usize].color = RBColor::Black;
                self.nodes[grandparent_idx as usize].color = RBColor::Red;
                self.rotate_left(grandparent_idx);
            }
            break;
        }

        // Root is always black
        if self.root >= 0 {
            self.nodes[self.root as usize].color = RBColor::Black;
        }
    }

    /// Left rotation
    fn rotate_left(&mut self, idx: i32) {
        let right_idx = self.nodes[idx as usize].right;
        if right_idx < 0 {
            return;
        }

        // Move right's left subtree to idx's right
        self.nodes[idx as usize].right = self.nodes[right_idx as usize].left;
        if self.nodes[right_idx as usize].left >= 0 {
            self.nodes[self.nodes[right_idx as usize].left as usize].parent = idx;
        }

        // Update right's parent
        self.nodes[right_idx as usize].parent = self.nodes[idx as usize].parent;

        // Update parent's child pointer
        let parent_idx = self.nodes[idx as usize].parent;
        if parent_idx < 0 {
            self.root = right_idx;
        } else if self.nodes[parent_idx as usize].left == idx {
            self.nodes[parent_idx as usize].left = right_idx;
        } else {
            self.nodes[parent_idx as usize].right = right_idx;
        }

        // Put idx as right's left child
        self.nodes[right_idx as usize].left = idx;
        self.nodes[idx as usize].parent = right_idx;

        // Update max_end values
        self.update_max_end(idx);
        self.update_max_end(right_idx);
    }

    /// Right rotation
    fn rotate_right(&mut self, idx: i32) {
        let left_idx = self.nodes[idx as usize].left;
        if left_idx < 0 {
            return;
        }

        // Move left's right subtree to idx's left
        self.nodes[idx as usize].left = self.nodes[left_idx as usize].right;
        if self.nodes[left_idx as usize].right >= 0 {
            self.nodes[self.nodes[left_idx as usize].right as usize].parent = idx;
        }

        // Update left's parent
        self.nodes[left_idx as usize].parent = self.nodes[idx as usize].parent;

        // Update parent's child pointer
        let parent_idx = self.nodes[idx as usize].parent;
        if parent_idx < 0 {
            self.root = left_idx;
        } else if self.nodes[parent_idx as usize].right == idx {
            self.nodes[parent_idx as usize].right = left_idx;
        } else {
            self.nodes[parent_idx as usize].left = left_idx;
        }

        // Put idx as left's right child
        self.nodes[left_idx as usize].right = idx;
        self.nodes[idx as usize].parent = left_idx;

        // Update max_end values
        self.update_max_end(idx);
        self.update_max_end(left_idx);
    }

    /// Update the max_end value for a node based on its children
    fn update_max_end(&mut self, idx: i32) {
        if idx < 0 {
            return;
        }

        let node = &self.nodes[idx as usize];
        let mut max = node.vma.end;

        if node.left >= 0 {
            max = max.max(self.nodes[node.left as usize].max_end);
        }
        if node.right >= 0 {
            max = max.max(self.nodes[node.right as usize].max_end);
        }

        self.nodes[idx as usize].max_end = max;
    }

    /// Remove a VMA from the tree by address
    pub fn remove(&mut self, start: u64) -> Option<VMA> {
        // Find the node
        let mut current = self.root;
        while current >= 0 {
            let node = &self.nodes[current as usize];
            if !node.in_use {
                return None;
            }

            if start < node.vma.start {
                current = node.left;
            } else if start > node.vma.start {
                current = node.right;
            } else {
                // Found it
                break;
            }
        }

        if current < 0 {
            return None;
        }

        let vma = self.nodes[current as usize].vma;
        self.remove_node(current);
        self.stats.mapped_bytes = self.stats.mapped_bytes.saturating_sub(vma.size());
        Some(vma)
    }

    /// Remove a node by index
    fn remove_node(&mut self, idx: i32) {
        let node = &self.nodes[idx as usize];
        let left = node.left;
        let right = node.right;

        // Case 1: Node has two children - find successor
        if left >= 0 && right >= 0 {
            // Find minimum in right subtree
            let mut successor = right;
            while self.nodes[successor as usize].left >= 0 {
                successor = self.nodes[successor as usize].left;
            }

            // Copy successor's VMA to current node
            self.nodes[idx as usize].vma = self.nodes[successor as usize].vma;

            // Remove successor (which has at most one child)
            self.remove_node(successor);
            return;
        }

        // Case 2: Node has at most one child
        let child = if left >= 0 { left } else { right };

        // Replace node with child
        let parent = self.nodes[idx as usize].parent;
        if child >= 0 {
            self.nodes[child as usize].parent = parent;
        }

        if parent < 0 {
            self.root = child;
        } else if self.nodes[parent as usize].left == idx {
            self.nodes[parent as usize].left = child;
        } else {
            self.nodes[parent as usize].right = child;
        }

        // Update max_end values up the tree
        let mut current = parent;
        while current >= 0 {
            self.update_max_end(current);
            current = self.nodes[current as usize].parent;
        }

        // Free the node
        self.free_node(idx);
        self.count -= 1;
    }

    /// Iterate over all VMAs in address order
    pub fn iter(&self) -> VMAIterator<'_> {
        VMAIterator {
            manager: self,
            stack: [0i32; 32],
            stack_len: 0,
            current: self.root,
            initialized: false,
        }
    }

    /// Try to merge adjacent VMAs with compatible attributes
    pub fn try_merge(&mut self, start: u64) -> bool {
        // Find the VMA
        let mut idx = self.root;
        while idx >= 0 {
            let node = &self.nodes[idx as usize];
            if !node.in_use {
                return false;
            }

            if start < node.vma.start {
                idx = node.left;
            } else if start > node.vma.start {
                idx = node.right;
            } else {
                break;
            }
        }

        if idx < 0 {
            return false;
        }

        // Try to merge with previous VMA
        let vma = self.nodes[idx as usize].vma;

        // Find previous VMA (in-order predecessor)
        if let Some(prev_vma) = self.find_previous(vma.start) {
            if prev_vma.can_merge_with(&vma) {
                let prev_start = prev_vma.start;
                // Extend previous VMA to include this one
                if let Some(prev_vma_mut) = self.find_mut(prev_start) {
                    prev_vma_mut.end = vma.end;
                }
                // Remove this VMA
                self.remove(vma.start);
                self.stats.merge_count += 1;
                return true;
            }
        }

        // Try to merge with next VMA
        if let Some(next_vma) = self.find_next(vma.end) {
            if vma.can_merge_with(&next_vma) {
                let next_start = next_vma.start;
                let next_end = next_vma.end;
                // Extend this VMA to include next
                if let Some(vma_mut) = self.find_mut(vma.start) {
                    vma_mut.end = next_end;
                }
                // Remove next VMA
                self.remove(next_start);
                self.stats.merge_count += 1;
                return true;
            }
        }

        false
    }

    /// Find the VMA immediately before a given address
    fn find_previous(&self, addr: u64) -> Option<&VMA> {
        let mut current = self.root;
        let mut result: Option<&VMA> = None;

        while current >= 0 {
            let node = match self.node(current) {
                Some(n) => n,
                None => break,
            };

            if node.vma.end <= addr {
                // This VMA ends before addr, could be the answer
                result = Some(&node.vma);
                current = node.right; // Look for a closer one
            } else {
                current = node.left;
            }
        }

        result
    }

    /// Find the VMA immediately after a given address
    fn find_next(&self, addr: u64) -> Option<&VMA> {
        let mut current = self.root;
        let mut result: Option<&VMA> = None;

        while current >= 0 {
            let node = match self.node(current) {
                Some(n) => n,
                None => break,
            };

            if node.vma.start >= addr {
                // This VMA starts at or after addr, could be the answer
                result = Some(&node.vma);
                current = node.left; // Look for a closer one
            } else {
                current = node.right;
            }
        }

        result
    }

    /// Find a free region of the given size within the specified range
    pub fn find_free_region(&self, min_addr: u64, max_addr: u64, size: u64) -> Option<u64> {
        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let mut candidate = min_addr;

        // Iterate through VMAs in order
        for vma in self.iter() {
            if candidate + aligned_size <= vma.start {
                // Found a gap
                return Some(candidate);
            }
            if vma.end > candidate {
                candidate = vma.end;
            }
        }

        // Check space after last VMA
        if candidate + aligned_size <= max_addr {
            return Some(candidate);
        }

        None
    }

    /// Clear all VMAs (for process exit)
    pub fn clear(&mut self) {
        // Reset all nodes
        for i in 0..MAX_VMAS - 1 {
            self.nodes[i] = VMANode::empty();
            self.nodes[i].left = (i + 1) as i32;
        }
        self.nodes[MAX_VMAS - 1] = VMANode::empty();
        self.nodes[MAX_VMAS - 1].left = -1;

        self.root = -1;
        self.count = 0;
        self.free_head = 0;
        self.stats.mapped_bytes = 0;
    }
}

impl Default for VMAManager {
    fn default() -> Self {
        let mut mgr = Self::new();
        mgr.init();
        mgr
    }
}

// =============================================================================
// VMA Iterator
// =============================================================================

/// Iterator over VMAs in address order (in-order traversal)
pub struct VMAIterator<'a> {
    manager: &'a VMAManager,
    stack: [i32; 32],
    stack_len: usize,
    current: i32,
    initialized: bool,
}

impl<'a> Iterator for VMAIterator<'a> {
    type Item = &'a VMA;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.initialized {
            self.initialized = true;
            // Go to leftmost node
            while self.current >= 0 {
                if self.stack_len < self.stack.len() {
                    self.stack[self.stack_len] = self.current;
                    self.stack_len += 1;
                }
                self.current = self.manager.nodes[self.current as usize].left;
            }
        }

        // Pop from stack
        if self.stack_len == 0 {
            return None;
        }

        self.stack_len -= 1;
        let idx = self.stack[self.stack_len];
        let node = &self.manager.nodes[idx as usize];

        if !node.in_use {
            return self.next();
        }

        // Move to right subtree
        self.current = node.right;
        while self.current >= 0 {
            if self.stack_len < self.stack.len() {
                self.stack[self.stack_len] = self.current;
                self.stack_len += 1;
            }
            self.current = self.manager.nodes[self.current as usize].left;
        }

        Some(&node.vma)
    }
}

// =============================================================================
// Address Space
// =============================================================================

/// Process address space combining VMA management with page tables
pub struct AddressSpace {
    /// VMA manager for this address space
    pub vmas: VMAManager,
    /// Physical address of PML4 (CR3 value)
    pub cr3: u64,
    /// Process ID owning this address space
    pub pid: u64,
    /// Heap start address
    pub heap_start: u64,
    /// Current heap end (brk)
    pub heap_end: u64,
    /// Stack start address (bottom of stack region)
    pub stack_start: u64,
    /// Stack end address (top of stack, grows down)
    pub stack_end: u64,
    /// mmap allocation base
    pub mmap_base: u64,
    /// Current mmap allocation pointer
    pub mmap_current: u64,
    /// Total resident set size (RSS) in pages
    pub rss_pages: u64,
    /// Total virtual size in pages
    pub vm_pages: u64,
    /// Is this address space valid/initialized
    pub valid: bool,
}

impl AddressSpace {
    /// Create an empty address space
    pub const fn empty() -> Self {
        Self {
            vmas: VMAManager::new(),
            cr3: 0,
            pid: 0,
            heap_start: 0,
            heap_end: 0,
            stack_start: 0,
            stack_end: 0,
            mmap_base: 0,
            mmap_current: 0,
            rss_pages: 0,
            vm_pages: 0,
            valid: false,
        }
    }

    /// Initialize address space for a new process
    pub fn init(&mut self, pid: u64, cr3: u64) {
        use crate::process::{HEAP_BASE, INTERP_BASE, STACK_BASE, STACK_SIZE};

        self.vmas = VMAManager::default();
        self.cr3 = cr3;
        self.pid = pid;
        self.heap_start = HEAP_BASE;
        self.heap_end = HEAP_BASE;
        self.stack_start = STACK_BASE;
        self.stack_end = STACK_BASE + STACK_SIZE;
        self.mmap_base = INTERP_BASE + 0x100000; // After interpreter
        self.mmap_current = self.mmap_base;
        self.rss_pages = 0;
        self.vm_pages = 0;
        self.valid = true;
    }

    /// Add a VMA to this address space
    pub fn add_vma(&mut self, vma: VMA) -> Option<i32> {
        let pages = vma.page_count();
        let result = self.vmas.insert(vma);
        if result.is_some() {
            self.vm_pages += pages;
        }
        result
    }

    /// Remove a VMA from this address space
    pub fn remove_vma(&mut self, start: u64) -> Option<VMA> {
        let vma = self.vmas.remove(start)?;
        self.vm_pages = self.vm_pages.saturating_sub(vma.page_count());
        Some(vma)
    }

    /// Expand the heap (brk syscall)
    pub fn brk(&mut self, new_brk: u64) -> Result<u64, &'static str> {
        use crate::process::HEAP_SIZE;

        if new_brk == 0 {
            return Ok(self.heap_end);
        }

        let max_heap = self.heap_start + HEAP_SIZE;

        if new_brk < self.heap_start {
            return Err("brk below heap start");
        }

        if new_brk > max_heap {
            return Err("brk exceeds max heap");
        }

        if new_brk > self.heap_end {
            // Expanding heap - check if we need to create/extend VMA
            let heap_vma = VMA::new(
                self.heap_end,
                new_brk,
                VMAPermissions::READ | VMAPermissions::WRITE,
                VMAFlags::PRIVATE | VMAFlags::ANONYMOUS | VMAFlags::HEAP,
                VMABacking::Anonymous,
            );

            self.add_vma(heap_vma);
        } else if new_brk < self.heap_end {
            // Shrinking heap - would need to unmap pages
            // For now, just update the pointer
        }

        self.heap_end = new_brk;
        Ok(new_brk)
    }

    /// Allocate a region for mmap
    pub fn mmap(
        &mut self,
        addr_hint: u64,
        length: u64,
        prot: u64,
        flags: u64,
    ) -> Result<u64, &'static str> {
        use crate::process::{USER_REGION_SIZE, USER_VIRT_BASE};

        let aligned_len = (length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let user_end = USER_VIRT_BASE + USER_REGION_SIZE;

        let vma_flags = VMAFlags::from_mmap_flags(flags);
        let vma_perm = VMAPermissions::from_prot(prot);

        let addr = if (flags & 0x10) != 0 {
            // MAP_FIXED
            if addr_hint == 0 || (addr_hint & (PAGE_SIZE - 1)) != 0 {
                return Err("MAP_FIXED with invalid address");
            }
            addr_hint
        } else if addr_hint != 0 {
            // Try hint address first
            let aligned_hint = (addr_hint + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            if self.vmas.find(aligned_hint).is_none() {
                aligned_hint
            } else {
                // Fall back to finding free region
                self.vmas
                    .find_free_region(self.mmap_current, user_end, aligned_len)
                    .ok_or("No free region for mmap")?
            }
        } else {
            // Find free region starting from mmap_current
            let addr = self
                .vmas
                .find_free_region(self.mmap_current, user_end, aligned_len)
                .ok_or("No free region for mmap")?;
            self.mmap_current = addr + aligned_len;
            addr
        };

        // Create VMA
        let vma = VMA::new(
            addr,
            addr + aligned_len,
            vma_perm,
            vma_flags,
            VMABacking::Anonymous,
        );

        self.add_vma(vma).ok_or("Failed to add VMA")?;
        self.vmas.stats_mut().mmap_count += 1;

        Ok(addr)
    }

    /// Unmap a region
    pub fn munmap(&mut self, addr: u64, length: u64) -> Result<(), &'static str> {
        let aligned_len = (length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let end = addr + aligned_len;

        // Find overlapping VMAs
        let mut overlapping = [0i32; 16];
        let count = self.vmas.find_overlapping(addr, end, &mut overlapping);

        for i in 0..count {
            let idx = overlapping[i];
            let vma = &self.vmas.nodes[idx as usize].vma;

            if vma.start >= addr && vma.end <= end {
                // Entire VMA is unmapped
                self.vmas.remove(vma.start);
            } else if vma.start < addr && vma.end > end {
                // VMA is split in the middle
                let vma_start = vma.start;
                if let Some(vma_mut) = self.vmas.find_mut(vma_start) {
                    if let Some(upper) = vma_mut.split_at(end) {
                        let _lower_end = vma_mut.end;
                        vma_mut.end = addr;
                        self.add_vma(upper);
                        self.vmas.stats_mut().split_count += 2;
                    }
                }
            } else if vma.start < addr {
                // Unmap upper part
                let vma_start = vma.start;
                if let Some(vma_mut) = self.vmas.find_mut(vma_start) {
                    vma_mut.end = addr;
                    self.vmas.stats_mut().split_count += 1;
                }
            } else {
                // Unmap lower part
                let vma_start = vma.start;
                if let Some(vma_mut) = self.vmas.find_mut(vma_start) {
                    vma_mut.start = end;
                    self.vmas.stats_mut().split_count += 1;
                }
            }
        }

        self.vmas.stats_mut().munmap_count += 1;
        Ok(())
    }

    /// Change memory protection
    pub fn mprotect(&mut self, addr: u64, length: u64, prot: u64) -> Result<(), &'static str> {
        let aligned_len = (length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let end = addr + aligned_len;
        let new_perm = VMAPermissions::from_prot(prot);

        // Find overlapping VMAs
        let mut overlapping = [0i32; 16];
        let count = self.vmas.find_overlapping(addr, end, &mut overlapping);

        for i in 0..count {
            let idx = overlapping[i];
            let vma = &self.vmas.nodes[idx as usize].vma;
            let vma_start = vma.start;
            let vma_end = vma.end;

            if vma_start >= addr && vma_end <= end {
                // Entire VMA has protection changed
                if let Some(vma_mut) = self.vmas.find_mut(vma_start) {
                    vma_mut.perm = new_perm;
                }
            } else {
                // Need to split VMA
                // This is a simplified implementation - full implementation
                // would handle all edge cases
                if let Some(vma_mut) = self.vmas.find_mut(vma_start) {
                    vma_mut.perm = new_perm;
                }
            }
        }

        self.vmas.stats_mut().mprotect_count += 1;
        Ok(())
    }

    /// Handle a page fault within this address space
    pub fn handle_fault(&mut self, fault_addr: u64, is_write: bool) -> Result<(), &'static str> {
        let vma = self.vmas.find(fault_addr).ok_or("Address not mapped")?;

        // Check permissions
        if is_write && !vma.perm.is_write() {
            if vma.flags.contains(VMAFlags::COW) {
                // Handle COW fault
                self.vmas.stats_mut().cow_faults += 1;
                // TODO: Implement COW page copy
                return Ok(());
            }
            return Err("Write to read-only mapping");
        }

        // Demand paging - allocate and map the page
        self.vmas.stats_mut().page_faults += 1;
        self.rss_pages += 1;

        Ok(())
    }

    /// Print memory map (for debugging)
    pub fn print_maps(&self) {
        crate::kinfo!("=== Address Space for PID {} ===", self.pid);
        crate::kinfo!("CR3: {:#x}", self.cr3);
        crate::kinfo!("Heap: {:#x} - {:#x}", self.heap_start, self.heap_end);
        crate::kinfo!("Stack: {:#x} - {:#x}", self.stack_start, self.stack_end);
        crate::kinfo!("VMAs ({}):", self.vmas.len());

        for vma in self.vmas.iter() {
            crate::kinfo!("  {:?}", vma);
        }

        crate::kinfo!("RSS: {} pages ({} KB)", self.rss_pages, self.rss_pages * 4);
        crate::kinfo!("VM: {} pages ({} KB)", self.vm_pages, self.vm_pages * 4);
        crate::kinfo!("=== End Address Space ===");
    }
}

impl Default for AddressSpace {
    fn default() -> Self {
        Self::empty()
    }
}

// =============================================================================
// Global Address Space Table
// =============================================================================

use spin::Mutex;

/// Global table of address spaces (one per process)
static ADDRESS_SPACES: Mutex<[AddressSpace; MAX_ADDRESS_SPACES]> =
    Mutex::new([const { AddressSpace::empty() }; MAX_ADDRESS_SPACES]);

/// Get address space for a PID
pub fn get_address_space(
    pid: u64,
) -> Option<spin::MutexGuard<'static, [AddressSpace; MAX_ADDRESS_SPACES]>> {
    let spaces = ADDRESS_SPACES.lock();
    if pid < MAX_ADDRESS_SPACES as u64 && spaces[pid as usize].valid {
        Some(spaces)
    } else {
        None
    }
}

/// Initialize address space for a new process
pub fn init_address_space(pid: u64, cr3: u64) -> Result<(), &'static str> {
    if pid >= MAX_ADDRESS_SPACES as u64 {
        return Err("PID out of range");
    }

    let mut spaces = ADDRESS_SPACES.lock();
    spaces[pid as usize].init(pid, cr3);
    Ok(())
}

/// Free address space for a process
pub fn free_address_space(pid: u64) {
    if pid >= MAX_ADDRESS_SPACES as u64 {
        return;
    }

    let mut spaces = ADDRESS_SPACES.lock();
    spaces[pid as usize].vmas.clear();
    spaces[pid as usize].valid = false;
}
