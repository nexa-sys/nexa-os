//! Pointer and slice conversion abstractions.
//!
//! This module provides safe wrappers for converting raw pointers to references
//! and creating slices from raw parts.

use core::slice;

/// Convert a raw pointer to a reference.
///
/// # Safety
/// The caller must ensure:
/// - The pointer is non-null and properly aligned
/// - The pointed-to memory is valid for the returned reference's lifetime
/// - No mutable references to the same memory exist
#[inline]
pub unsafe fn ptr_to_ref<'a, T>(ptr: *const T) -> &'a T {
    &*ptr
}

/// Convert a raw pointer to a mutable reference.
///
/// # Safety
/// The caller must ensure:
/// - The pointer is non-null and properly aligned
/// - The pointed-to memory is valid for the returned reference's lifetime
/// - No other references (mutable or immutable) to the same memory exist
#[inline]
pub unsafe fn ptr_to_mut<'a, T>(ptr: *mut T) -> &'a mut T {
    &mut *ptr
}

/// Create a slice from raw pointer and length.
///
/// # Safety
/// The caller must ensure:
/// - `ptr` is valid for reads for `len * size_of::<T>()` bytes
/// - `ptr` is properly aligned
/// - The memory is initialized
/// - The total size doesn't exceed isize::MAX
#[inline]
pub unsafe fn slice_from_ptr<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    slice::from_raw_parts(ptr, len)
}

/// Create a mutable slice from raw pointer and length.
///
/// # Safety
/// The caller must ensure:
/// - `ptr` is valid for reads and writes for `len * size_of::<T>()` bytes
/// - `ptr` is properly aligned
/// - The memory is initialized
/// - No other references to the same memory exist
/// - The total size doesn't exceed isize::MAX
#[inline]
pub unsafe fn slice_from_ptr_mut<'a, T>(ptr: *mut T, len: usize) -> &'a mut [T] {
    slice::from_raw_parts_mut(ptr, len)
}

/// Create a static slice from raw pointer and length.
///
/// # Safety
/// Same as `slice_from_ptr`, plus:
/// - The memory must have 'static lifetime
#[inline]
pub unsafe fn static_slice<T>(ptr: *const T, len: usize) -> &'static [T] {
    slice::from_raw_parts(ptr, len)
}

/// Create a static mutable slice from raw pointer and length.
///
/// # Safety
/// Same as `slice_from_ptr_mut`, plus:
/// - The memory must have 'static lifetime
#[inline]
pub unsafe fn static_slice_mut<T>(ptr: *mut T, len: usize) -> &'static mut [T] {
    slice::from_raw_parts_mut(ptr, len)
}

/// Wrapper for safely creating slices from user-space pointers.
///
/// This provides bounds checking and validation for syscall handlers.
pub struct UserSlice<'a, T> {
    ptr: *const T,
    len: usize,
    _marker: core::marker::PhantomData<&'a T>,
}

impl<'a, T> UserSlice<'a, T> {
    /// Create a new UserSlice after validating the pointer.
    ///
    /// Returns None if the pointer is null or the length would overflow.
    pub fn new(ptr: *const T, len: usize) -> Option<Self> {
        if ptr.is_null() {
            return None;
        }
        // Check for overflow
        let size = len.checked_mul(core::mem::size_of::<T>())?;
        let end = (ptr as usize).checked_add(size)?;
        // Basic sanity check - user space should be below kernel space
        // This is a simplified check; real implementation should verify page tables
        if end > 0x0000_8000_0000_0000 {
            return None;
        }
        Some(Self {
            ptr,
            len,
            _marker: core::marker::PhantomData,
        })
    }

    /// Get the slice.
    ///
    /// # Safety
    /// The caller must ensure the user-space memory is actually mapped and accessible.
    #[inline]
    pub unsafe fn as_slice(&self) -> &'a [T] {
        slice::from_raw_parts(self.ptr, self.len)
    }

    /// Get the length.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Check if empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Wrapper for safely creating mutable slices from user-space pointers.
pub struct UserSliceMut<'a, T> {
    ptr: *mut T,
    len: usize,
    _marker: core::marker::PhantomData<&'a mut T>,
}

impl<'a, T> UserSliceMut<'a, T> {
    /// Create a new UserSliceMut after validating the pointer.
    pub fn new(ptr: *mut T, len: usize) -> Option<Self> {
        if ptr.is_null() {
            return None;
        }
        let size = len.checked_mul(core::mem::size_of::<T>())?;
        let end = (ptr as usize).checked_add(size)?;
        if end > 0x0000_8000_0000_0000 {
            return None;
        }
        Some(Self {
            ptr,
            len,
            _marker: core::marker::PhantomData,
        })
    }

    /// Get the mutable slice.
    ///
    /// # Safety
    /// The caller must ensure the user-space memory is actually mapped and writable.
    #[inline]
    pub unsafe fn as_slice_mut(&mut self) -> &'a mut [T] {
        slice::from_raw_parts_mut(self.ptr, self.len)
    }

    /// Get the length.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Check if empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Copy data to user space buffer.
///
/// # Safety
/// The destination pointer must be valid and writable in user space.
#[inline]
pub unsafe fn copy_to_user<T: Copy>(dst: *mut T, src: &T) {
    core::ptr::write(dst, *src);
}

/// Copy data from user space buffer.
///
/// # Safety
/// The source pointer must be valid and readable in user space.
#[inline]
pub unsafe fn copy_from_user<T: Copy>(src: *const T) -> T {
    core::ptr::read(src)
}

/// Copy a slice to user space.
///
/// # Safety
/// The destination must be valid for `data.len()` elements.
#[inline]
pub unsafe fn copy_slice_to_user<T: Copy>(dst: *mut T, data: &[T]) {
    core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
}
