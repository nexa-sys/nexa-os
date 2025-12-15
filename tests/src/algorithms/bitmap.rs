//! Bitmap operations for resource allocation
//!
//! Used in memory management, PID allocation, etc.

/// A simple bitmap for tracking allocated resources
#[derive(Clone)]
pub struct Bitmap {
    bits: Vec<u64>,
    size: usize,
}

impl Bitmap {
    /// Create a new bitmap with the given number of bits
    pub fn new(size: usize) -> Self {
        let num_words = (size + 63) / 64;
        Self {
            bits: vec![0; num_words],
            size,
        }
    }

    /// Set a bit at the given index
    pub fn set(&mut self, index: usize) -> bool {
        if index >= self.size {
            return false;
        }
        let word = index / 64;
        let bit = index % 64;
        self.bits[word] |= 1 << bit;
        true
    }

    /// Clear a bit at the given index
    pub fn clear(&mut self, index: usize) -> bool {
        if index >= self.size {
            return false;
        }
        let word = index / 64;
        let bit = index % 64;
        self.bits[word] &= !(1 << bit);
        true
    }

    /// Test if a bit is set
    pub fn test(&self, index: usize) -> bool {
        if index >= self.size {
            return false;
        }
        let word = index / 64;
        let bit = index % 64;
        (self.bits[word] & (1 << bit)) != 0
    }

    /// Find the first clear bit and set it, returns the index
    pub fn alloc(&mut self) -> Option<usize> {
        for (word_idx, word) in self.bits.iter_mut().enumerate() {
            if *word != u64::MAX {
                // Find first zero bit
                let bit = (!*word).trailing_zeros() as usize;
                let index = word_idx * 64 + bit;
                if index < self.size {
                    *word |= 1 << bit;
                    return Some(index);
                }
            }
        }
        None
    }

    /// Count the number of set bits
    pub fn count_ones(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Count the number of clear bits
    pub fn count_zeros(&self) -> usize {
        self.size - self.count_ones()
    }

    /// Return the total size
    pub fn size(&self) -> usize {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmap_new() {
        let bm = Bitmap::new(100);
        assert_eq!(bm.size(), 100);
        assert_eq!(bm.count_ones(), 0);
        assert_eq!(bm.count_zeros(), 100);
    }

    #[test]
    fn test_bitmap_set_clear() {
        let mut bm = Bitmap::new(128);
        
        assert!(!bm.test(0));
        assert!(bm.set(0));
        assert!(bm.test(0));
        
        assert!(bm.set(63));
        assert!(bm.test(63));
        
        assert!(bm.set(64));
        assert!(bm.test(64));
        
        assert!(bm.clear(0));
        assert!(!bm.test(0));
        
        // Out of bounds
        assert!(!bm.set(128));
        assert!(!bm.test(128));
    }

    #[test]
    fn test_bitmap_alloc() {
        let mut bm = Bitmap::new(10);
        
        // Allocate all bits
        for i in 0..10 {
            assert_eq!(bm.alloc(), Some(i));
        }
        
        // No more bits available
        assert_eq!(bm.alloc(), None);
        
        // Free one and reallocate
        bm.clear(5);
        assert_eq!(bm.alloc(), Some(5));
    }

    #[test]
    fn test_bitmap_count() {
        let mut bm = Bitmap::new(100);
        
        bm.set(0);
        bm.set(50);
        bm.set(99);
        
        assert_eq!(bm.count_ones(), 3);
        assert_eq!(bm.count_zeros(), 97);
    }

    #[test]
    fn test_bitmap_boundary() {
        // Test at word boundaries
        let mut bm = Bitmap::new(65);
        
        assert!(bm.set(63));
        assert!(bm.set(64));
        assert!(bm.test(63));
        assert!(bm.test(64));
        
        // Bit 65 should be out of bounds
        assert!(!bm.set(65));
    }
}
