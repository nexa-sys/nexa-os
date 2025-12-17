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

mod allocator;
mod buddy;
mod comprehensive;
mod safety;
mod slab;
