//! Ring buffer implementation
//!
//! Used for pipes, TTY buffers, network packet queues, etc.

use std::collections::VecDeque;

/// A fixed-capacity ring buffer
pub struct RingBuffer<T> {
    buffer: VecDeque<T>,
    capacity: usize,
}

impl<T> RingBuffer<T> {
    /// Create a new ring buffer with the given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push an item to the buffer, returns false if full
    pub fn push(&mut self, item: T) -> bool {
        if self.buffer.len() >= self.capacity {
            return false;
        }
        self.buffer.push_back(item);
        true
    }

    /// Pop an item from the buffer
    pub fn pop(&mut self) -> Option<T> {
        self.buffer.pop_front()
    }

    /// Peek at the front item without removing it
    pub fn peek(&self) -> Option<&T> {
        self.buffer.front()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Check if the buffer is full
    pub fn is_full(&self) -> bool {
        self.buffer.len() >= self.capacity
    }

    /// Get the number of items in the buffer
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Get the capacity of the buffer
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get available space
    pub fn available(&self) -> usize {
        self.capacity - self.buffer.len()
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl<T: Clone> RingBuffer<T> {
    /// Read multiple items without consuming them
    pub fn peek_many(&self, count: usize) -> Vec<T> {
        self.buffer.iter().take(count).cloned().collect()
    }

    /// Write multiple items, returns count of items written
    pub fn write(&mut self, items: &[T]) -> usize {
        let available = self.available();
        let to_write = items.len().min(available);
        for item in items.iter().take(to_write) {
            self.buffer.push_back(item.clone());
        }
        to_write
    }

    /// Read multiple items
    pub fn read(&mut self, count: usize) -> Vec<T> {
        let to_read = count.min(self.len());
        let mut result = Vec::with_capacity(to_read);
        for _ in 0..to_read {
            if let Some(item) = self.buffer.pop_front() {
                result.push(item);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_basic() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(5);
        
        assert!(rb.is_empty());
        assert!(!rb.is_full());
        assert_eq!(rb.capacity(), 5);
        
        assert!(rb.push(1));
        assert!(rb.push(2));
        assert!(rb.push(3));
        
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.available(), 2);
    }

    #[test]
    fn test_ring_buffer_full() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        
        assert!(rb.push(1));
        assert!(rb.push(2));
        assert!(rb.push(3));
        
        assert!(rb.is_full());
        assert!(!rb.push(4)); // Should fail
        
        assert_eq!(rb.pop(), Some(1));
        assert!(!rb.is_full());
        assert!(rb.push(4)); // Now it should work
    }

    #[test]
    fn test_ring_buffer_fifo() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(10);
        
        for i in 0..5 {
            rb.push(i);
        }
        
        for i in 0..5 {
            assert_eq!(rb.pop(), Some(i));
        }
        
        assert!(rb.is_empty());
    }

    #[test]
    fn test_ring_buffer_peek() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(5);
        
        rb.push(42);
        assert_eq!(rb.peek(), Some(&42));
        assert_eq!(rb.len(), 1); // Not consumed
        
        assert_eq!(rb.pop(), Some(42));
        assert_eq!(rb.peek(), None);
    }

    #[test]
    fn test_ring_buffer_write_read() {
        let mut rb: RingBuffer<u8> = RingBuffer::new(10);
        
        let data = vec![1, 2, 3, 4, 5];
        assert_eq!(rb.write(&data), 5);
        assert_eq!(rb.len(), 5);
        
        let read = rb.read(3);
        assert_eq!(read, vec![1, 2, 3]);
        assert_eq!(rb.len(), 2);
        
        let remaining = rb.read(10);
        assert_eq!(remaining, vec![4, 5]);
    }

    #[test]
    fn test_ring_buffer_clear() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(5);
        
        rb.push(1);
        rb.push(2);
        rb.push(3);
        
        rb.clear();
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
    }
}
