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
