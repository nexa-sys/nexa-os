//! Fixed-size vector implementation
//!
//! A Vec-like container with a fixed maximum capacity,
//! suitable for no_std environments where heap allocation is not available.

/// A fixed-capacity vector that doesn't require heap allocation
pub struct FixedVec<T, const N: usize> {
    data: [Option<T>; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> Default for FixedVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const N: usize> FixedVec<T, N> {
    /// Create a new empty FixedVec
    pub fn new() -> Self {
        Self {
            data: [None; N],
            len: 0,
        }
    }

    /// Push an item, returns false if full
    pub fn push(&mut self, item: T) -> bool {
        if self.len >= N {
            return false;
        }
        self.data[self.len] = Some(item);
        self.len += 1;
        true
    }

    /// Pop an item from the end
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        self.data[self.len].take()
    }

    /// Get an item by index
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        self.data[index].as_ref()
    }

    /// Get a mutable reference by index
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        self.data[index].as_mut()
    }

    /// Get the length
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Check if full
    pub fn is_full(&self) -> bool {
        self.len >= N
    }

    /// Get capacity
    pub fn capacity(&self) -> usize {
        N
    }

    /// Clear all items
    pub fn clear(&mut self) {
        for i in 0..self.len {
            self.data[i] = None;
        }
        self.len = 0;
    }

    /// Iterate over items
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data[..self.len].iter().filter_map(|x| x.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_vec_basic() {
        let mut vec: FixedVec<i32, 5> = FixedVec::new();
        
        assert!(vec.is_empty());
        assert!(!vec.is_full());
        assert_eq!(vec.capacity(), 5);
        
        vec.push(1);
        vec.push(2);
        vec.push(3);
        
        assert_eq!(vec.len(), 3);
        assert!(!vec.is_empty());
    }

    #[test]
    fn test_fixed_vec_full() {
        let mut vec: FixedVec<i32, 3> = FixedVec::new();
        
        assert!(vec.push(1));
        assert!(vec.push(2));
        assert!(vec.push(3));
        
        assert!(vec.is_full());
        assert!(!vec.push(4)); // Should fail
    }

    #[test]
    fn test_fixed_vec_pop() {
        let mut vec: FixedVec<i32, 5> = FixedVec::new();
        
        vec.push(1);
        vec.push(2);
        vec.push(3);
        
        assert_eq!(vec.pop(), Some(3));
        assert_eq!(vec.pop(), Some(2));
        assert_eq!(vec.pop(), Some(1));
        assert_eq!(vec.pop(), None);
    }

    #[test]
    fn test_fixed_vec_get() {
        let mut vec: FixedVec<i32, 5> = FixedVec::new();
        
        vec.push(10);
        vec.push(20);
        vec.push(30);
        
        assert_eq!(vec.get(0), Some(&10));
        assert_eq!(vec.get(1), Some(&20));
        assert_eq!(vec.get(2), Some(&30));
        assert_eq!(vec.get(3), None);
    }

    #[test]
    fn test_fixed_vec_get_mut() {
        let mut vec: FixedVec<i32, 5> = FixedVec::new();
        
        vec.push(10);
        vec.push(20);
        
        if let Some(val) = vec.get_mut(0) {
            *val = 100;
        }
        
        assert_eq!(vec.get(0), Some(&100));
    }

    #[test]
    fn test_fixed_vec_clear() {
        let mut vec: FixedVec<i32, 5> = FixedVec::new();
        
        vec.push(1);
        vec.push(2);
        vec.push(3);
        
        vec.clear();
        
        assert!(vec.is_empty());
        assert_eq!(vec.len(), 0);
    }

    #[test]
    fn test_fixed_vec_iter() {
        let mut vec: FixedVec<i32, 5> = FixedVec::new();
        
        vec.push(1);
        vec.push(2);
        vec.push(3);
        
        let collected: Vec<i32> = vec.iter().copied().collect();
        assert_eq!(collected, vec![1, 2, 3]);
    }
}
