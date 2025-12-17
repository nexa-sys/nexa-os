//! Memory Management Tests
//!
//! Tests for memory allocation, paging, and virtual memory.
//! This module includes:
//! - Buddy allocator edge cases and statistics
//! - Slab allocator and object caching
//! - Memory layout validation
//! - Virtual address mapping
//! - Page table operations
//! - Safety utilities (layout_of, layout_array)
//! - VMA (Virtual Memory Area) management

mod allocator;
mod brk_edge_cases;
mod buddy;
mod buddy_edge_cases;
mod comprehensive;
mod paging_edge_cases;
mod safety;
mod slab;
mod vma;
mod vma_advanced;
