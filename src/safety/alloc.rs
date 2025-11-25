//! Memory allocation safety wrappers.
//!
//! This module provides tracked allocation/deallocation operations
//! with additional safety checks.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;

/// Allocate memory with the given layout.
///
/// Returns None if allocation fails.
///
/// # Safety
/// This is a wrapper around the global allocator. The returned memory
/// is uninitialized and must be properly initialized before use.
#[inline]
pub unsafe fn allocate(layout: Layout) -> Option<NonNull<u8>> {
    extern crate alloc;
    let ptr = alloc::alloc::alloc(layout);
    NonNull::new(ptr)
}

/// Allocate zeroed memory with the given layout.
///
/// Returns None if allocation fails.
///
/// # Safety
/// This is a wrapper around the global allocator.
#[inline]
pub unsafe fn allocate_zeroed(layout: Layout) -> Option<NonNull<u8>> {
    extern crate alloc;
    let ptr = alloc::alloc::alloc_zeroed(layout);
    NonNull::new(ptr)
}

/// Deallocate memory.
///
/// # Safety
/// - `ptr` must have been allocated by this allocator with the same layout
/// - `ptr` must not be used after deallocation
#[inline]
pub unsafe fn deallocate(ptr: NonNull<u8>, layout: Layout) {
    extern crate alloc;
    alloc::alloc::dealloc(ptr.as_ptr(), layout);
}

/// Reallocate memory with a new size.
///
/// # Safety
/// - `ptr` must have been allocated by this allocator with `old_layout`
/// - `new_size` must be greater than zero
/// - On success, the old pointer is invalidated
#[inline]
pub unsafe fn reallocate(
    ptr: NonNull<u8>,
    old_layout: Layout,
    new_size: usize,
) -> Option<NonNull<u8>> {
    extern crate alloc;
    let new_ptr = alloc::alloc::realloc(ptr.as_ptr(), old_layout, new_size);
    NonNull::new(new_ptr)
}

/// A wrapper for tracking allocations.
///
/// This can be used to ensure paired alloc/dealloc calls.
pub struct TrackedAllocation {
    ptr: NonNull<u8>,
    layout: Layout,
}

impl TrackedAllocation {
    /// Allocate tracked memory.
    ///
    /// # Safety
    /// The returned memory is uninitialized.
    pub unsafe fn new(layout: Layout) -> Option<Self> {
        let ptr = allocate(layout)?;
        Some(Self { ptr, layout })
    }

    /// Allocate tracked zeroed memory.
    pub unsafe fn new_zeroed(layout: Layout) -> Option<Self> {
        let ptr = allocate_zeroed(layout)?;
        Some(Self { ptr, layout })
    }

    /// Get the pointer.
    #[inline]
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Get the pointer as a specific type.
    ///
    /// # Safety
    /// The caller must ensure proper alignment and initialization.
    #[inline]
    pub unsafe fn as_ptr_typed<T>(&self) -> *mut T {
        self.ptr.as_ptr() as *mut T
    }

    /// Get the layout.
    #[inline]
    pub const fn layout(&self) -> Layout {
        self.layout
    }

    /// Consume the allocation and return the raw pointer.
    ///
    /// After calling this, the caller is responsible for deallocation.
    #[inline]
    pub fn leak(self) -> (*mut u8, Layout) {
        let ptr = self.ptr.as_ptr();
        let layout = self.layout;
        core::mem::forget(self);
        (ptr, layout)
    }
}

impl Drop for TrackedAllocation {
    fn drop(&mut self) {
        // SAFETY: We own this allocation and it hasn't been leaked
        unsafe {
            deallocate(self.ptr, self.layout);
        }
    }
}

/// Create a layout for a type.
#[inline]
pub const fn layout_of<T>() -> Layout {
    // SAFETY: Layout::new is always valid for any sized type
    Layout::new::<T>()
}

/// Create a layout for an array of items.
#[inline]
pub fn layout_array<T>(count: usize) -> Option<Layout> {
    Layout::array::<T>(count).ok()
}

/// Stack allocation helper - allocate on the kernel stack with specific alignment.
///
/// This is useful for temporary buffers that need specific alignment.
#[macro_export]
macro_rules! stack_alloc {
    ($size:expr, $align:expr) => {{
        #[repr(C, align($align))]
        struct AlignedBuffer {
            data: [u8; $size],
        }
        AlignedBuffer { data: [0u8; $size] }
    }};
}
