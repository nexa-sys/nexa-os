//! Network packet header casting abstractions.
//!
//! This module provides safe patterns for interpreting byte buffers
//! as network protocol headers.

use core::mem::size_of;

/// Trait for types that can be safely cast from raw bytes.
///
/// # Safety
/// Implementors must ensure:
/// - The type has no padding bytes that could contain uninitialized data
/// - The type has no invalid bit patterns
/// - The type is repr(C) or repr(packed)
pub unsafe trait FromBytes: Sized + Copy {
    /// The minimum size needed to contain this header.
    const SIZE: usize = size_of::<Self>();
}

/// Cast a byte slice to a header reference.
///
/// Returns None if the buffer is too small.
///
/// # Safety
/// The buffer must be properly aligned for type T.
/// For network headers, this usually isn't an issue since they're
/// typically accessed at arbitrary offsets.
#[inline]
pub unsafe fn cast_header<T: FromBytes>(buffer: &[u8]) -> Option<&T> {
    if buffer.len() < T::SIZE {
        return None;
    }
    Some(&*(buffer.as_ptr() as *const T))
}

/// Cast a mutable byte slice to a mutable header reference.
///
/// # Safety
/// Same requirements as cast_header.
#[inline]
pub unsafe fn cast_header_mut<T: FromBytes>(buffer: &mut [u8]) -> Option<&mut T> {
    if buffer.len() < T::SIZE {
        return None;
    }
    Some(&mut *(buffer.as_mut_ptr() as *mut T))
}

/// Read a header by copying from unaligned memory.
///
/// This is safer than cast_header when alignment isn't guaranteed.
#[inline]
pub fn read_header_unaligned<T: FromBytes>(buffer: &[u8]) -> Option<T> {
    if buffer.len() < T::SIZE {
        return None;
    }
    // SAFETY: We've verified the buffer is large enough
    Some(unsafe { core::ptr::read_unaligned(buffer.as_ptr() as *const T) })
}

/// Write a header to a buffer (unaligned).
#[inline]
pub fn write_header_unaligned<T: FromBytes>(buffer: &mut [u8], header: &T) -> bool {
    if buffer.len() < T::SIZE {
        return false;
    }
    // SAFETY: We've verified the buffer is large enough
    unsafe {
        core::ptr::write_unaligned(buffer.as_mut_ptr() as *mut T, *header);
    }
    true
}

/// Get header bytes from a value.
#[inline]
pub fn header_as_bytes<T: FromBytes>(header: &T) -> &[u8] {
    // SAFETY: Any FromBytes type can be viewed as bytes
    unsafe { core::slice::from_raw_parts(header as *const T as *const u8, T::SIZE) }
}

/// A wrapper for safely working with packet buffers.
pub struct PacketBuffer<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> PacketBuffer<'a> {
    /// Create a new packet buffer.
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    /// Create a packet buffer starting at an offset.
    pub fn with_offset(data: &'a [u8], offset: usize) -> Option<Self> {
        if offset > data.len() {
            return None;
        }
        Some(Self { data, offset })
    }

    /// Get the remaining data.
    #[inline]
    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.offset..]
    }

    /// Get the remaining length.
    #[inline]
    pub fn remaining_len(&self) -> usize {
        self.data.len() - self.offset
    }

    /// Read a header and advance the offset.
    ///
    /// # Safety
    /// The buffer must be properly aligned for type T.
    pub unsafe fn read_header<T: FromBytes>(&mut self) -> Option<&'a T> {
        if self.remaining_len() < T::SIZE {
            return None;
        }
        let ptr = self.data.as_ptr().add(self.offset) as *const T;
        self.offset += T::SIZE;
        Some(&*ptr)
    }

    /// Read bytes and advance the offset.
    pub fn read_bytes(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.remaining_len() < len {
            return None;
        }
        let start = self.offset;
        self.offset += len;
        Some(&self.data[start..self.offset])
    }

    /// Skip bytes.
    pub fn skip(&mut self, len: usize) -> bool {
        if self.remaining_len() < len {
            return false;
        }
        self.offset += len;
        true
    }

    /// Get the current offset.
    #[inline]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Reset to a specific offset.
    pub fn seek(&mut self, offset: usize) -> bool {
        if offset > self.data.len() {
            return false;
        }
        self.offset = offset;
        true
    }
}

/// Mutable packet buffer.
pub struct PacketBufferMut<'a> {
    data: &'a mut [u8],
    offset: usize,
}

impl<'a> PacketBufferMut<'a> {
    /// Create a new mutable packet buffer.
    pub fn new(data: &'a mut [u8]) -> Self {
        Self { data, offset: 0 }
    }

    /// Get remaining length.
    #[inline]
    pub fn remaining_len(&self) -> usize {
        self.data.len() - self.offset
    }

    /// Write a header and advance the offset.
    pub fn write_header<T: FromBytes>(&mut self, header: &T) -> bool {
        if self.remaining_len() < T::SIZE {
            return false;
        }
        // SAFETY: We've verified space is available
        unsafe {
            core::ptr::write_unaligned(
                self.data.as_mut_ptr().add(self.offset) as *mut T,
                *header,
            );
        }
        self.offset += T::SIZE;
        true
    }

    /// Write bytes and advance the offset.
    pub fn write_bytes(&mut self, bytes: &[u8]) -> bool {
        if self.remaining_len() < bytes.len() {
            return false;
        }
        let start = self.offset;
        self.data[start..start + bytes.len()].copy_from_slice(bytes);
        self.offset += bytes.len();
        true
    }

    /// Get a mutable reference to the remaining buffer.
    pub fn remaining_mut(&mut self) -> &mut [u8] {
        &mut self.data[self.offset..]
    }

    /// Get the current offset.
    #[inline]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Get the underlying buffer.
    pub fn into_inner(self) -> &'a mut [u8] {
        self.data
    }

    /// Get the written portion.
    pub fn written(&self) -> &[u8] {
        &self.data[..self.offset]
    }
}
