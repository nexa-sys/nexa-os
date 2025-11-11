use core::cell::UnsafeCell;
use core::slice;
use core::str;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Errors for `StaticArena` operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaError {
    /// Requested region does not fit into the backing buffer.
    OutOfSpace,
    /// Offset math overflowed.
    Overflow,
}

/// Fixed-size bump arena for storing boot-time data with `'static` lifetime.
pub struct StaticArena<const N: usize> {
    buffer: UnsafeCell<[u8; N]>,
    offset: AtomicUsize,
}

unsafe impl<const N: usize> Sync for StaticArena<N> {}

impl<const N: usize> StaticArena<N> {
    pub const fn new() -> Self {
        Self {
            buffer: UnsafeCell::new([0; N]),
            offset: AtomicUsize::new(0),
        }
    }

    /// Store raw bytes and get a `'static` view into the arena.
    pub fn store_bytes(&'static self, bytes: &[u8]) -> Result<&'static [u8], ArenaError> {
        let len = bytes.len();
        if len == 0 {
            return Ok(&[]);
        }

        let (start, end) = self.reserve(len)?;

        unsafe {
            let buffer = &mut *self.buffer.get();
            buffer[start..end].copy_from_slice(bytes);
            let ptr = buffer.as_ptr().add(start);
            Ok(slice::from_raw_parts(ptr, len))
        }
    }

    /// Store a UTF-8 string and get a `'static` reference.
    pub fn store_str(&'static self, value: &str) -> Result<&'static str, ArenaError> {
        let bytes = self.store_bytes(value.as_bytes())?;
        Ok(unsafe { str::from_utf8_unchecked(bytes) })
    }

    fn reserve(&self, len: usize) -> Result<(usize, usize), ArenaError> {
        let mut current = self.offset.load(Ordering::Acquire);
        loop {
            let end = current.checked_add(len).ok_or(ArenaError::Overflow)?;

            if end > N {
                return Err(ArenaError::OutOfSpace);
            }

            match self
                .offset
                .compare_exchange(current, end, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Ok((current, end)),
                Err(next) => current = next,
            }
        }
    }
}
