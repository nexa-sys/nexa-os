//! Static variable access abstractions.
//!
//! This module provides safe patterns for accessing static mutable state,
//! which is common in OS kernel code but requires careful handling.

use core::cell::UnsafeCell;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

/// A wrapper for static mutable variables that provides controlled access.
///
/// This type ensures that access to the underlying data goes through
/// explicit unsafe blocks with documented safety requirements.
pub struct StaticMut<T> {
    data: UnsafeCell<T>,
    initialized: AtomicBool,
}

// SAFETY: StaticMut provides its own synchronization guarantees
unsafe impl<T: Send> Sync for StaticMut<T> {}
unsafe impl<T: Send> Send for StaticMut<T> {}

impl<T> StaticMut<T> {
    /// Create a new StaticMut with the given initial value.
    pub const fn new(value: T) -> Self {
        Self {
            data: UnsafeCell::new(value),
            initialized: AtomicBool::new(true),
        }
    }

    /// Create an uninitialized StaticMut.
    ///
    /// Must be initialized before use.
    pub const fn uninit() -> Self
    where
        T: Copy,
    {
        Self {
            data: UnsafeCell::new(unsafe { core::mem::zeroed() }),
            initialized: AtomicBool::new(false),
        }
    }

    /// Initialize the value.
    ///
    /// # Safety
    /// Must not be called concurrently with any access to the data.
    pub unsafe fn init(&self, value: T) {
        *self.data.get() = value;
        self.initialized.store(true, Ordering::Release);
    }

    /// Check if initialized.
    #[inline]
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    /// Get a raw pointer to the data.
    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.data.get()
    }

    /// Get an immutable reference.
    ///
    /// # Safety
    /// - No mutable references must exist
    /// - The value must be initialized
    #[inline]
    pub unsafe fn get(&self) -> &T {
        &*self.data.get()
    }

    /// Get a mutable reference.
    ///
    /// # Safety
    /// - No other references (mutable or immutable) must exist
    /// - The value must be initialized
    #[inline]
    pub unsafe fn get_mut(&self) -> &mut T {
        &mut *self.data.get()
    }

    /// Execute a closure with an immutable reference.
    ///
    /// # Safety
    /// - No mutable references must exist during the closure execution
    /// - The value must be initialized
    #[inline]
    pub unsafe fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        f(&*self.data.get())
    }

    /// Execute a closure with a mutable reference.
    ///
    /// # Safety
    /// - No other references must exist during the closure execution
    /// - The value must be initialized
    #[inline]
    pub unsafe fn with_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        f(&mut *self.data.get())
    }
}

/// A static array with tracked length.
///
/// Useful for static buffers that grow during initialization.
pub struct StaticArray<T, const N: usize> {
    data: UnsafeCell<[T; N]>,
    len: core::sync::atomic::AtomicUsize,
}

unsafe impl<T: Send, const N: usize> Sync for StaticArray<T, N> {}
unsafe impl<T: Send, const N: usize> Send for StaticArray<T, N> {}

impl<T: Copy + Default, const N: usize> StaticArray<T, N> {
    /// Create a new empty StaticArray.
    pub const fn new() -> Self
    where
        T: Copy,
    {
        Self {
            data: UnsafeCell::new(unsafe { core::mem::zeroed() }),
            len: core::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Get the current length.
    #[inline]
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// Check if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the capacity.
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Push an item.
    ///
    /// # Safety
    /// Must not be called concurrently.
    pub unsafe fn push(&self, value: T) -> bool {
        let len = self.len.load(Ordering::Acquire);
        if len >= N {
            return false;
        }
        (*self.data.get())[len] = value;
        self.len.store(len + 1, Ordering::Release);
        true
    }

    /// Get an item by index.
    ///
    /// # Safety
    /// Must not be called while push is in progress.
    pub unsafe fn get(&self, index: usize) -> Option<T> {
        if index >= self.len() {
            None
        } else {
            Some((*self.data.get())[index])
        }
    }

    /// Get a slice of the current items.
    ///
    /// # Safety
    /// Must not be called while push is in progress.
    pub unsafe fn as_slice(&self) -> &[T] {
        let len = self.len();
        core::slice::from_raw_parts((*self.data.get()).as_ptr(), len)
    }

    /// Get the raw data pointer.
    #[inline]
    pub fn as_ptr(&self) -> *const T {
        // SAFETY: UnsafeCell::get returns a valid pointer
        unsafe { (*self.data.get()).as_ptr() }
    }
}

/// Helper macro to safely access a static mut variable.
///
/// This macro generates code that uses addr_of_mut! to get a pointer
/// without creating a reference, avoiding the static_mut_refs lint.
#[macro_export]
macro_rules! static_mut_ptr {
    ($static:expr) => {
        core::ptr::addr_of_mut!($static)
    };
}

/// Helper macro to read from a static mut variable.
#[macro_export]
macro_rules! static_mut_read {
    ($static:expr) => {
        unsafe { core::ptr::read(core::ptr::addr_of!($static)) }
    };
}

/// Helper macro to write to a static mut variable.
#[macro_export]
macro_rules! static_mut_write {
    ($static:expr, $value:expr) => {
        unsafe { core::ptr::write(core::ptr::addr_of_mut!($static), $value) }
    };
}
