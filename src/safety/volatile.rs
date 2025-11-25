//! Volatile memory access abstractions.
//!
//! This module provides safe wrappers around volatile read/write operations
//! used for MMIO, hardware registers, and memory-mapped buffers.

use core::ptr;

/// Read a value from a volatile memory location.
///
/// # Safety
/// The pointer must be valid and properly aligned for type T.
/// This function encapsulates the unsafe volatile read operation.
#[inline]
pub unsafe fn volatile_read<T: Copy>(src: *const T) -> T {
    ptr::read_volatile(src)
}

/// Write a value to a volatile memory location.
///
/// # Safety
/// The pointer must be valid and properly aligned for type T.
/// This function encapsulates the unsafe volatile write operation.
#[inline]
pub unsafe fn volatile_write<T: Copy>(dst: *mut T, value: T) {
    ptr::write_volatile(dst, value);
}

/// A wrapper for volatile access to a memory-mapped register or buffer.
///
/// This type ensures all accesses go through volatile operations,
/// preventing the compiler from optimizing away reads/writes.
#[repr(transparent)]
pub struct Volatile<T: Copy> {
    value: T,
}

impl<T: Copy> Volatile<T> {
    /// Create a new Volatile wrapper.
    pub const fn new(value: T) -> Self {
        Self { value }
    }

    /// Read the value using volatile semantics.
    #[inline]
    pub fn read(&self) -> T {
        // SAFETY: self is a valid reference, so the pointer is valid and aligned
        unsafe { ptr::read_volatile(&self.value) }
    }

    /// Write a value using volatile semantics.
    #[inline]
    pub fn write(&mut self, value: T) {
        // SAFETY: self is a valid mutable reference
        unsafe { ptr::write_volatile(&mut self.value, value) }
    }

    /// Get a raw pointer to the underlying value.
    #[inline]
    pub const fn as_ptr(&self) -> *const T {
        &self.value as *const T
    }

    /// Get a raw mutable pointer to the underlying value.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        &mut self.value as *mut T
    }
}

/// MMIO register accessor with base address.
///
/// Provides type-safe access to memory-mapped I/O registers.
pub struct MmioRegion {
    base: *mut u8,
}

impl MmioRegion {
    /// Create a new MMIO region accessor.
    ///
    /// # Safety
    /// The caller must ensure:
    /// - `base` points to valid MMIO memory
    /// - The memory remains mapped for the region's lifetime
    /// - No other code accesses the same region concurrently without synchronization
    #[inline]
    pub const unsafe fn new(base: *mut u8) -> Self {
        Self { base }
    }

    /// Read a 32-bit value from the given offset.
    ///
    /// # Safety
    /// The offset must be within the MMIO region and properly aligned.
    #[inline]
    pub unsafe fn read32(&self, offset: usize) -> u32 {
        let ptr = self.base.add(offset) as *const u32;
        ptr::read_volatile(ptr)
    }

    /// Write a 32-bit value to the given offset.
    ///
    /// # Safety
    /// The offset must be within the MMIO region and properly aligned.
    #[inline]
    pub unsafe fn write32(&self, offset: usize, value: u32) {
        let ptr = self.base.add(offset) as *mut u32;
        ptr::write_volatile(ptr, value);
    }

    /// Read a 64-bit value from the given offset.
    ///
    /// # Safety
    /// The offset must be within the MMIO region and properly aligned.
    #[inline]
    pub unsafe fn read64(&self, offset: usize) -> u64 {
        let ptr = self.base.add(offset) as *const u64;
        ptr::read_volatile(ptr)
    }

    /// Write a 64-bit value to the given offset.
    ///
    /// # Safety
    /// The offset must be within the MMIO region and properly aligned.
    #[inline]
    pub unsafe fn write64(&self, offset: usize, value: u64) {
        let ptr = self.base.add(offset) as *mut u64;
        ptr::write_volatile(ptr, value);
    }

    /// Read a u8 value from the given offset.
    #[inline]
    pub unsafe fn read8(&self, offset: usize) -> u8 {
        let ptr = self.base.add(offset);
        ptr::read_volatile(ptr)
    }

    /// Write a u8 value to the given offset.
    #[inline]
    pub unsafe fn write8(&self, offset: usize, value: u8) {
        let ptr = self.base.add(offset);
        ptr::write_volatile(ptr, value);
    }

    /// Get the base address.
    #[inline]
    pub const fn base(&self) -> *mut u8 {
        self.base
    }
}

/// VGA text buffer character with volatile access.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct VgaChar {
    pub ascii: u8,
    pub color: u8,
}

impl VgaChar {
    pub const fn new(ascii: u8, color: u8) -> Self {
        Self { ascii, color }
    }
}

/// VGA text buffer accessor.
pub struct VgaBuffer {
    base: *mut VgaChar,
    width: usize,
    height: usize,
}

impl VgaBuffer {
    /// Create a new VGA buffer accessor.
    ///
    /// # Safety
    /// The base pointer must point to valid VGA text buffer memory (typically 0xB8000).
    pub const unsafe fn new(base: *mut VgaChar, width: usize, height: usize) -> Self {
        Self {
            base,
            width,
            height,
        }
    }

    /// Read a character at the given position.
    #[inline]
    pub fn read_at(&self, row: usize, col: usize) -> Option<VgaChar> {
        if row >= self.height || col >= self.width {
            return None;
        }
        let offset = row * self.width + col;
        // SAFETY: bounds checked above, base is valid VGA memory
        Some(unsafe { ptr::read_volatile(self.base.add(offset)) })
    }

    /// Write a character at the given position.
    #[inline]
    pub fn write_at(&mut self, row: usize, col: usize, ch: VgaChar) -> bool {
        if row >= self.height || col >= self.width {
            return false;
        }
        let offset = row * self.width + col;
        // SAFETY: bounds checked above, base is valid VGA memory
        unsafe { ptr::write_volatile(self.base.add(offset), ch) };
        true
    }

    /// Get buffer dimensions.
    #[inline]
    pub const fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}
