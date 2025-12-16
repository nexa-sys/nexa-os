//! Mock Physical Memory for Testing
//!
//! This module simulates **physical memory hardware** - the underlying
//! resource that the kernel's memory manager operates on.
//!
//! ## What this mocks:
//! - Physical page frames (4KB aligned memory regions)
//! - Memory allocation tracking (simulates limited physical memory)
//!
//! ## What this does NOT mock:
//! - Page tables (kernel implementation)
//! - Virtual memory (kernel implementation)
//! - The kernel's allocator logic (use real kernel code)
//!
//! This allows testing kernel memory management code without needing
//! actual hardware memory management.

use std::alloc::{alloc, dealloc, Layout};
use std::collections::HashMap;
use std::sync::Mutex;

/// A mock page allocator that tracks allocations
pub struct MockPageAllocator {
    allocations: Mutex<HashMap<usize, usize>>, // address -> size
    total_allocated: Mutex<usize>,
    page_size: usize,
}

impl MockPageAllocator {
    pub const DEFAULT_PAGE_SIZE: usize = 4096;

    pub fn new() -> Self {
        Self::with_page_size(Self::DEFAULT_PAGE_SIZE)
    }

    pub fn with_page_size(page_size: usize) -> Self {
        Self {
            allocations: Mutex::new(HashMap::new()),
            total_allocated: Mutex::new(0),
            page_size,
        }
    }

    /// Allocate pages
    pub fn alloc_pages(&self, count: usize) -> Option<*mut u8> {
        let size = count * self.page_size;
        let layout = Layout::from_size_align(size, self.page_size).ok()?;
        
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            return None;
        }
        
        let addr = ptr as usize;
        let mut allocations = self.allocations.lock().unwrap();
        allocations.insert(addr, size);
        
        let mut total = self.total_allocated.lock().unwrap();
        *total += size;
        
        Some(ptr)
    }

    /// Free pages
    pub fn free_pages(&self, ptr: *mut u8, count: usize) {
        let addr = ptr as usize;
        let size = count * self.page_size;
        
        let mut allocations = self.allocations.lock().unwrap();
        if allocations.remove(&addr).is_some() {
            let layout = Layout::from_size_align(size, self.page_size).unwrap();
            unsafe { dealloc(ptr, layout) };
            
            let mut total = self.total_allocated.lock().unwrap();
            *total -= size;
        }
    }

    /// Get total allocated memory
    pub fn total_allocated(&self) -> usize {
        *self.total_allocated.lock().unwrap()
    }

    /// Get number of allocations
    pub fn allocation_count(&self) -> usize {
        self.allocations.lock().unwrap().len()
    }

    /// Check for memory leaks
    pub fn check_leaks(&self) -> bool {
        self.allocation_count() == 0
    }
}

impl Default for MockPageAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MockPageAllocator {
    fn drop(&mut self) {
        // Clean up any remaining allocations
        let allocations = self.allocations.get_mut().unwrap();
        for (&addr, &size) in allocations.iter() {
            let layout = Layout::from_size_align(size, self.page_size).unwrap();
            unsafe { dealloc(addr as *mut u8, layout) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_allocator_basic() {
        let allocator = MockPageAllocator::new();
        
        let ptr = allocator.alloc_pages(1);
        assert!(ptr.is_some());
        assert_eq!(allocator.allocation_count(), 1);
        assert_eq!(allocator.total_allocated(), 4096);
        
        allocator.free_pages(ptr.unwrap(), 1);
        assert_eq!(allocator.allocation_count(), 0);
        assert_eq!(allocator.total_allocated(), 0);
    }

    #[test]
    fn test_mock_allocator_multiple() {
        let allocator = MockPageAllocator::new();
        
        let ptr1 = allocator.alloc_pages(2);
        let ptr2 = allocator.alloc_pages(3);
        
        assert!(ptr1.is_some());
        assert!(ptr2.is_some());
        assert_eq!(allocator.allocation_count(), 2);
        assert_eq!(allocator.total_allocated(), 5 * 4096);
        
        allocator.free_pages(ptr1.unwrap(), 2);
        allocator.free_pages(ptr2.unwrap(), 3);
        
        assert!(allocator.check_leaks());
    }

    #[test]
    fn test_mock_allocator_write_read() {
        let allocator = MockPageAllocator::new();
        
        let ptr = allocator.alloc_pages(1).unwrap();
        
        // Write to the allocated memory
        unsafe {
            for i in 0..4096 {
                *ptr.add(i) = (i % 256) as u8;
            }
            
            // Verify
            for i in 0..4096 {
                assert_eq!(*ptr.add(i), (i % 256) as u8);
            }
        }
        
        allocator.free_pages(ptr, 1);
    }
}
