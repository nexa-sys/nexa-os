//! Mock IPC primitives for testing
//!
//! Simulates pipes, message queues, and shared memory.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Condvar};
use std::time::Duration;

/// Pipe buffer size
pub const PIPE_BUF_SIZE: usize = 4096;

/// A mock pipe for testing
pub struct MockPipe {
    buffer: VecDeque<u8>,
    capacity: usize,
    read_closed: bool,
    write_closed: bool,
}

impl MockPipe {
    pub fn new() -> Self {
        Self::with_capacity(PIPE_BUF_SIZE)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
            read_closed: false,
            write_closed: false,
        }
    }

    /// Write data to pipe
    pub fn write(&mut self, data: &[u8]) -> Result<usize, i32> {
        if self.read_closed {
            return Err(32); // EPIPE
        }

        let available = self.capacity - self.buffer.len();
        if available == 0 {
            return Err(11); // EAGAIN (would block)
        }

        let to_write = data.len().min(available);
        for &byte in &data[..to_write] {
            self.buffer.push_back(byte);
        }

        Ok(to_write)
    }

    /// Read data from pipe
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, i32> {
        if self.buffer.is_empty() {
            if self.write_closed {
                return Ok(0); // EOF
            }
            return Err(11); // EAGAIN
        }

        let to_read = buf.len().min(self.buffer.len());
        for byte in buf.iter_mut().take(to_read) {
            *byte = self.buffer.pop_front().unwrap();
        }

        Ok(to_read)
    }

    /// Close read end
    pub fn close_read(&mut self) {
        self.read_closed = true;
    }

    /// Close write end
    pub fn close_write(&mut self) {
        self.write_closed = true;
    }

    /// Check if pipe has data
    pub fn has_data(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// Check if pipe is full
    pub fn is_full(&self) -> bool {
        self.buffer.len() >= self.capacity
    }

    /// Get available space
    pub fn available_space(&self) -> usize {
        self.capacity - self.buffer.len()
    }

    /// Get buffered data size
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

impl Default for MockPipe {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe pipe with blocking operations
pub struct BlockingPipe {
    inner: Mutex<MockPipe>,
    read_ready: Condvar,
    write_ready: Condvar,
}

impl BlockingPipe {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(MockPipe::new()),
            read_ready: Condvar::new(),
            write_ready: Condvar::new(),
        })
    }

    /// Blocking write
    pub fn write(&self, data: &[u8]) -> Result<usize, i32> {
        let mut pipe = self.inner.lock().unwrap();
        
        while pipe.is_full() && !pipe.read_closed {
            pipe = self.write_ready.wait(pipe).unwrap();
        }

        let result = pipe.write(data);
        drop(pipe);
        self.read_ready.notify_one();
        result
    }

    /// Blocking read
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, i32> {
        let mut pipe = self.inner.lock().unwrap();
        
        while !pipe.has_data() && !pipe.write_closed {
            pipe = self.read_ready.wait(pipe).unwrap();
        }

        let result = pipe.read(buf);
        drop(pipe);
        self.write_ready.notify_one();
        result
    }

    /// Read with timeout
    pub fn read_timeout(&self, buf: &mut [u8], timeout: Duration) -> Result<usize, i32> {
        let mut pipe = self.inner.lock().unwrap();
        
        if !pipe.has_data() && !pipe.write_closed {
            let (p, timeout_result) = self.read_ready.wait_timeout(pipe, timeout).unwrap();
            pipe = p;
            if timeout_result.timed_out() && !pipe.has_data() {
                return Err(11); // EAGAIN
            }
        }

        pipe.read(buf)
    }

    pub fn close_read(&self) {
        let mut pipe = self.inner.lock().unwrap();
        pipe.close_read();
        self.write_ready.notify_all();
    }

    pub fn close_write(&self) {
        let mut pipe = self.inner.lock().unwrap();
        pipe.close_write();
        self.read_ready.notify_all();
    }
}

impl Default for BlockingPipe {
    fn default() -> Self {
        Self {
            inner: Mutex::new(MockPipe::new()),
            read_ready: Condvar::new(),
            write_ready: Condvar::new(),
        }
    }
}

/// Message in a message queue
#[derive(Debug, Clone)]
pub struct Message {
    pub msg_type: i64,
    pub data: Vec<u8>,
}

/// A mock message queue
pub struct MockMessageQueue {
    messages: VecDeque<Message>,
    max_messages: usize,
    max_msg_size: usize,
}

impl MockMessageQueue {
    pub fn new(max_messages: usize, max_msg_size: usize) -> Self {
        Self {
            messages: VecDeque::with_capacity(max_messages),
            max_messages,
            max_msg_size,
        }
    }

    /// Send a message
    pub fn send(&mut self, msg_type: i64, data: &[u8]) -> Result<(), i32> {
        if data.len() > self.max_msg_size {
            return Err(7); // E2BIG
        }

        if self.messages.len() >= self.max_messages {
            return Err(11); // EAGAIN
        }

        self.messages.push_back(Message {
            msg_type,
            data: data.to_vec(),
        });

        Ok(())
    }

    /// Receive a message (msg_type = 0 gets any message)
    pub fn receive(&mut self, msg_type: i64) -> Result<Message, i32> {
        let idx = if msg_type == 0 {
            if self.messages.is_empty() {
                return Err(42); // ENOMSG
            }
            0
        } else if msg_type > 0 {
            // Get first message of exact type
            self.messages.iter()
                .position(|m| m.msg_type == msg_type)
                .ok_or(42)?
        } else {
            // Get first message with type <= |msg_type|
            let abs_type = msg_type.abs();
            self.messages.iter()
                .position(|m| m.msg_type <= abs_type)
                .ok_or(42)?
        };

        Ok(self.messages.remove(idx).unwrap())
    }

    /// Get number of messages in queue
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Check if queue is full
    pub fn is_full(&self) -> bool {
        self.messages.len() >= self.max_messages
    }
}

/// Mock semaphore
pub struct MockSemaphore {
    value: i32,
    max_value: i32,
}

impl MockSemaphore {
    pub fn new(initial: i32) -> Self {
        Self {
            value: initial,
            max_value: i32::MAX,
        }
    }

    pub fn with_max(initial: i32, max: i32) -> Self {
        Self {
            value: initial.min(max),
            max_value: max,
        }
    }

    /// Wait (decrement) - non-blocking
    pub fn try_wait(&mut self) -> bool {
        if self.value > 0 {
            self.value -= 1;
            true
        } else {
            false
        }
    }

    /// Post (increment)
    pub fn post(&mut self) -> bool {
        if self.value < self.max_value {
            self.value += 1;
            true
        } else {
            false // Overflow
        }
    }

    /// Get current value
    pub fn value(&self) -> i32 {
        self.value
    }
}

/// Thread-safe semaphore
pub struct BlockingSemaphore {
    inner: Mutex<MockSemaphore>,
    condvar: Condvar,
}

impl BlockingSemaphore {
    pub fn new(initial: i32) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(MockSemaphore::new(initial)),
            condvar: Condvar::new(),
        })
    }

    /// Blocking wait
    pub fn wait(&self) {
        let mut sem = self.inner.lock().unwrap();
        while !sem.try_wait() {
            sem = self.condvar.wait(sem).unwrap();
        }
    }

    /// Wait with timeout
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let mut sem = self.inner.lock().unwrap();
        if sem.try_wait() {
            return true;
        }

        let (s, timeout_result) = self.condvar.wait_timeout(sem, timeout).unwrap();
        sem = s;
        
        if timeout_result.timed_out() {
            sem.try_wait()
        } else {
            sem.try_wait()
        }
    }

    /// Non-blocking try wait
    pub fn try_wait(&self) -> bool {
        let mut sem = self.inner.lock().unwrap();
        sem.try_wait()
    }

    /// Post
    pub fn post(&self) -> bool {
        let mut sem = self.inner.lock().unwrap();
        let result = sem.post();
        drop(sem);
        self.condvar.notify_one();
        result
    }

    /// Get value
    pub fn value(&self) -> i32 {
        self.inner.lock().unwrap().value()
    }
}

/// Mock shared memory region
pub struct MockSharedMemory {
    data: Vec<u8>,
    size: usize,
}

impl MockSharedMemory {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0; size],
            size,
        }
    }

    /// Get mutable reference to memory
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Get reference to memory
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Get size
    pub fn size(&self) -> usize {
        self.size
    }

    /// Write at offset
    pub fn write_at(&mut self, offset: usize, data: &[u8]) -> Result<usize, i32> {
        if offset >= self.size {
            return Err(14); // EFAULT
        }

        let available = self.size - offset;
        let to_write = data.len().min(available);
        self.data[offset..offset + to_write].copy_from_slice(&data[..to_write]);
        Ok(to_write)
    }

    /// Read at offset
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, i32> {
        if offset >= self.size {
            return Err(14); // EFAULT
        }

        let available = self.size - offset;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&self.data[offset..offset + to_read]);
        Ok(to_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipe_basic() {
        let mut pipe = MockPipe::new();
        
        let written = pipe.write(b"Hello").unwrap();
        assert_eq!(written, 5);
        assert_eq!(pipe.buffered(), 5);
        
        let mut buf = [0u8; 10];
        let read = pipe.read(&mut buf).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buf[..read], b"Hello");
    }

    #[test]
    fn test_pipe_full() {
        let mut pipe = MockPipe::with_capacity(10);
        
        pipe.write(b"0123456789").unwrap();
        assert!(pipe.is_full());
        
        let result = pipe.write(b"x");
        assert_eq!(result, Err(11)); // EAGAIN
    }

    #[test]
    fn test_pipe_empty_read() {
        let mut pipe = MockPipe::new();
        
        let mut buf = [0u8; 10];
        let result = pipe.read(&mut buf);
        assert_eq!(result, Err(11)); // EAGAIN
    }

    #[test]
    fn test_pipe_eof() {
        let mut pipe = MockPipe::new();
        
        pipe.write(b"data").unwrap();
        pipe.close_write();
        
        let mut buf = [0u8; 10];
        let read = pipe.read(&mut buf).unwrap();
        assert_eq!(read, 4);
        
        // Next read returns EOF
        let read = pipe.read(&mut buf).unwrap();
        assert_eq!(read, 0);
    }

    #[test]
    fn test_pipe_broken() {
        let mut pipe = MockPipe::new();
        pipe.close_read();
        
        let result = pipe.write(b"data");
        assert_eq!(result, Err(32)); // EPIPE
    }

    #[test]
    fn test_blocking_pipe() {
        let pipe = BlockingPipe::new();
        let pipe2 = Arc::clone(&pipe);
        
        let writer = std::thread::spawn(move || {
            pipe2.write(b"Hello from thread").unwrap()
        });
        
        let mut buf = [0u8; 20];
        let read = pipe.read(&mut buf).unwrap();
        
        writer.join().unwrap();
        assert_eq!(read, 17);
        assert_eq!(&buf[..read], b"Hello from thread");
    }

    #[test]
    fn test_message_queue_basic() {
        let mut mq = MockMessageQueue::new(10, 256);
        
        mq.send(1, b"Message type 1").unwrap();
        mq.send(2, b"Message type 2").unwrap();
        
        assert_eq!(mq.message_count(), 2);
        
        let msg = mq.receive(0).unwrap();
        assert_eq!(msg.msg_type, 1);
        assert_eq!(msg.data, b"Message type 1");
    }

    #[test]
    fn test_message_queue_by_type() {
        let mut mq = MockMessageQueue::new(10, 256);
        
        mq.send(1, b"Type 1").unwrap();
        mq.send(2, b"Type 2").unwrap();
        mq.send(1, b"Type 1 again").unwrap();
        
        let msg = mq.receive(2).unwrap();
        assert_eq!(msg.msg_type, 2);
        
        let msg = mq.receive(1).unwrap();
        assert_eq!(msg.data, b"Type 1");
    }

    #[test]
    fn test_message_queue_too_large() {
        let mut mq = MockMessageQueue::new(10, 8);
        
        let result = mq.send(1, b"Too large message");
        assert_eq!(result, Err(7)); // E2BIG
    }

    #[test]
    fn test_message_queue_full() {
        let mut mq = MockMessageQueue::new(2, 256);
        
        mq.send(1, b"msg1").unwrap();
        mq.send(1, b"msg2").unwrap();
        
        let result = mq.send(1, b"msg3");
        assert_eq!(result, Err(11)); // EAGAIN
    }

    #[test]
    fn test_semaphore_basic() {
        let mut sem = MockSemaphore::new(2);
        
        assert!(sem.try_wait());
        assert!(sem.try_wait());
        assert!(!sem.try_wait()); // Would block
        
        assert_eq!(sem.value(), 0);
        
        sem.post();
        assert_eq!(sem.value(), 1);
        assert!(sem.try_wait());
    }

    #[test]
    fn test_semaphore_max() {
        let mut sem = MockSemaphore::with_max(0, 2);
        
        assert!(sem.post());
        assert!(sem.post());
        assert!(!sem.post()); // Would overflow
        
        assert_eq!(sem.value(), 2);
    }

    #[test]
    fn test_blocking_semaphore() {
        let sem = BlockingSemaphore::new(0);
        let sem2 = Arc::clone(&sem);
        
        let poster = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            sem2.post();
        });
        
        sem.wait();
        poster.join().unwrap();
        
        assert_eq!(sem.value(), 0);
    }

    #[test]
    fn test_shared_memory() {
        let mut shm = MockSharedMemory::new(4096);
        
        shm.write_at(0, b"Hello").unwrap();
        shm.write_at(100, b"World").unwrap();
        
        let mut buf = [0u8; 10];
        shm.read_at(0, &mut buf).unwrap();
        assert_eq!(&buf[..5], b"Hello");
        
        shm.read_at(100, &mut buf).unwrap();
        assert_eq!(&buf[..5], b"World");
    }

    #[test]
    fn test_shared_memory_bounds() {
        let mut shm = MockSharedMemory::new(100);
        
        let result = shm.write_at(200, b"data");
        assert_eq!(result, Err(14)); // EFAULT
        
        let mut buf = [0u8; 10];
        let result = shm.read_at(200, &mut buf);
        assert_eq!(result, Err(14));
    }

    #[test]
    fn test_shared_memory_partial() {
        let mut shm = MockSharedMemory::new(100);
        
        // Write that extends past end
        let written = shm.write_at(95, b"0123456789").unwrap();
        assert_eq!(written, 5); // Only 5 bytes fit
        
        // Read that extends past end
        let mut buf = [0u8; 20];
        let read = shm.read_at(95, &mut buf).unwrap();
        assert_eq!(read, 5);
    }
}
