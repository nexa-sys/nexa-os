use core::marker::PhantomData;
use core::mem;
use core::ptr;
use core::slice;

/// Errors raised when reading raw byte buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawAccessError {
    /// Requested range extends past the backing slice.
    OutOfBounds {
        offset: usize,
        size: usize,
        len: usize,
    },
    /// Offset math overflowed.
    Overflow,
}

/// Thin wrapper providing checked access to unaligned data.
#[derive(Clone, Copy)]
pub struct RawReader<'a> {
    data: *const u8,
    len: usize,
    _marker: PhantomData<&'a [u8]>,
}

impl<'a> RawReader<'a> {
    pub const fn new(data: &'a [u8]) -> Self {
        Self {
            data: data.as_ptr(),
            len: data.len(),
            _marker: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn read<T>(&self, offset: usize) -> Result<T, RawAccessError>
    where
        T: Copy,
    {
        self.ensure(offset, mem::size_of::<T>())?;
        unsafe {
            let ptr = self.data.add(offset) as *const T;
            Ok(ptr::read_unaligned(ptr))
        }
    }

    pub fn u8(&self, offset: usize) -> Result<u8, RawAccessError> {
        self.read::<u8>(offset)
    }

    pub fn u16(&self, offset: usize) -> Result<u16, RawAccessError> {
        self.read::<u16>(offset)
    }

    pub fn u32(&self, offset: usize) -> Result<u32, RawAccessError> {
        self.read::<u32>(offset)
    }

    pub fn u64(&self, offset: usize) -> Result<u64, RawAccessError> {
        self.read::<u64>(offset)
    }

    pub fn bytes(&self, offset: usize, len: usize) -> Result<&'a [u8], RawAccessError> {
        if len == 0 {
            return Ok(&[]);
        }
        self.ensure(offset, len)?;
        unsafe {
            let ptr = self.data.add(offset);
            Ok(slice::from_raw_parts(ptr, len))
        }
    }

    pub fn slice<T>(&self, offset: usize, count: usize) -> Result<&'a [T], RawAccessError>
    where
        T: Copy,
    {
        if count == 0 {
            return Ok(&[]);
        }
        let size = count
            .checked_mul(mem::size_of::<T>())
            .ok_or(RawAccessError::Overflow)?;
        self.ensure(offset, size)?;
        unsafe {
            let ptr = self.data.add(offset) as *const T;
            Ok(slice::from_raw_parts(ptr, count))
        }
    }

    fn ensure(&self, offset: usize, size: usize) -> Result<(), RawAccessError> {
        let end = offset.checked_add(size).ok_or(RawAccessError::Overflow)?;
        if end > self.len {
            Err(RawAccessError::OutOfBounds {
                offset,
                size,
                len: self.len,
            })
        } else {
            Ok(())
        }
    }
}

/// Create a `'static` slice from a raw pointer and length.
///
/// # Safety
/// This function encapsulates the unsafe operation of creating a slice from raw parts.
/// The caller must ensure:
/// - `ptr` is valid for reads for `len` bytes
/// - `ptr` points to memory that lives for the `'static` lifetime
/// - The memory is properly initialized
#[inline]
pub fn static_slice_from_raw_parts<T>(ptr: *const T, len: usize) -> &'static [T] {
    // SAFETY: Caller guarantees pointer validity and lifetime requirements
    unsafe { slice::from_raw_parts(ptr, len) }
}

/// Wrapper for accessing a static mutable buffer with bounds checking.
///
/// This provides a safe interface for reading from and writing to static mutable
/// buffers commonly used in kernel code for caching purposes.
pub struct StaticBufferAccessor<const N: usize> {
    ptr: *mut u8,
}

impl<const N: usize> StaticBufferAccessor<N> {
    /// Create a new accessor from a raw pointer to a buffer.
    ///
    /// # Safety
    /// The caller must ensure:
    /// - `ptr` points to a valid buffer of at least N bytes
    /// - Exclusive access to the buffer for the accessor's lifetime
    /// - The buffer remains valid for the accessor's lifetime
    #[inline]
    pub const unsafe fn from_raw_ptr(ptr: *mut [u8; N]) -> Self {
        Self {
            ptr: ptr as *mut u8,
        }
    }

    /// Get a mutable slice of the buffer up to `len` bytes.
    /// Returns `None` if `len` exceeds buffer capacity.
    #[inline]
    pub fn slice_mut(&mut self, len: usize) -> Option<&mut [u8]> {
        if len <= N {
            // SAFETY: We have exclusive access (guaranteed by caller), len <= N
            Some(unsafe { slice::from_raw_parts_mut(self.ptr, len) })
        } else {
            None
        }
    }

    /// Get a static reference to the first `len` bytes of the buffer.
    ///
    /// # Safety
    /// The caller must ensure the buffer content remains valid and unchanged
    /// for the returned slice's lifetime.
    #[inline]
    pub unsafe fn as_static_slice(&self, len: usize) -> Option<&'static [u8]> {
        if len <= N {
            Some(static_slice_from_raw_parts(self.ptr as *const u8, len))
        } else {
            None
        }
    }

    /// Get the buffer capacity.
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }
}
