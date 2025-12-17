//! Pipe Buffer Edge Case Tests
//!
//! Tests for pipe buffer management, boundary conditions, and SIGPIPE handling.

#[cfg(test)]
mod tests {
    // Constants matching pipe implementation
    const PIPE_BUF_SIZE: usize = 4096;
    const MAX_PIPES: usize = 16;

    // =========================================================================
    // Pipe Buffer Simulation
    // =========================================================================

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum PipeState {
        Open,
        ReadClosed,
        WriteClosed,
        Closed,
    }

    struct PipeBuffer {
        data: [u8; PIPE_BUF_SIZE],
        read_pos: usize,
        write_pos: usize,
        count: usize,
        state: PipeState,
    }

    impl PipeBuffer {
        fn new() -> Self {
            Self {
                data: [0; PIPE_BUF_SIZE],
                read_pos: 0,
                write_pos: 0,
                count: 0,
                state: PipeState::Open,
            }
        }

        fn is_empty(&self) -> bool {
            self.count == 0
        }

        fn is_full(&self) -> bool {
            self.count >= PIPE_BUF_SIZE
        }

        fn available_space(&self) -> usize {
            PIPE_BUF_SIZE - self.count
        }

        fn available_data(&self) -> usize {
            self.count
        }

        fn write(&mut self, data: &[u8]) -> Result<usize, &'static str> {
            if self.state == PipeState::Closed || self.state == PipeState::ReadClosed {
                return Err("EPIPE: Pipe read end closed");
            }

            if self.is_full() {
                return Err("EAGAIN: Pipe buffer full");
            }

            let available = self.available_space();
            let to_write = core::cmp::min(data.len(), available);

            for i in 0..to_write {
                self.data[self.write_pos] = data[i];
                self.write_pos = (self.write_pos + 1) % PIPE_BUF_SIZE;
            }

            self.count += to_write;
            Ok(to_write)
        }

        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, &'static str> {
            if self.state == PipeState::Closed {
                return Err("EBADF: Pipe is closed");
            }

            if self.is_empty() {
                if self.state == PipeState::WriteClosed {
                    return Ok(0); // EOF
                }
                return Err("EAGAIN: No data available");
            }

            let to_read = core::cmp::min(buffer.len(), self.count);

            for i in 0..to_read {
                buffer[i] = self.data[self.read_pos];
                self.read_pos = (self.read_pos + 1) % PIPE_BUF_SIZE;
            }

            self.count -= to_read;
            Ok(to_read)
        }

        fn close_read(&mut self) {
            match self.state {
                PipeState::Open => self.state = PipeState::ReadClosed,
                PipeState::WriteClosed => self.state = PipeState::Closed,
                _ => {}
            }
        }

        fn close_write(&mut self) {
            match self.state {
                PipeState::Open => self.state = PipeState::WriteClosed,
                PipeState::ReadClosed => self.state = PipeState::Closed,
                _ => {}
            }
        }
    }

    // =========================================================================
    // Basic Read/Write Tests
    // =========================================================================

    #[test]
    fn test_pipe_write_read() {
        let mut pipe = PipeBuffer::new();
        
        let data = b"Hello, pipe!";
        let written = pipe.write(data).unwrap();
        assert_eq!(written, data.len());

        let mut buffer = [0u8; 64];
        let read = pipe.read(&mut buffer).unwrap();
        assert_eq!(read, data.len());
        assert_eq!(&buffer[..read], data);
    }

    #[test]
    fn test_pipe_fifo_order() {
        let mut pipe = PipeBuffer::new();
        
        // Write in chunks
        pipe.write(b"First").unwrap();
        pipe.write(b"Second").unwrap();
        pipe.write(b"Third").unwrap();

        // Read should get data in order
        let mut buffer = [0u8; 64];
        let read = pipe.read(&mut buffer).unwrap();
        
        // Verify FIFO order
        let data = &buffer[..read];
        assert!(data.starts_with(b"First"));
    }

    #[test]
    fn test_pipe_partial_read() {
        let mut pipe = PipeBuffer::new();
        
        pipe.write(b"Hello, World!").unwrap();

        // Read only 5 bytes
        let mut buffer = [0u8; 5];
        let read = pipe.read(&mut buffer).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buffer, b"Hello");

        // Read remaining
        let mut buffer2 = [0u8; 64];
        let read2 = pipe.read(&mut buffer2).unwrap();
        assert_eq!(read2, 8); // ", World!"
    }

    // =========================================================================
    // Buffer Boundary Tests
    // =========================================================================

    #[test]
    fn test_pipe_full_buffer() {
        let mut pipe = PipeBuffer::new();
        
        // Fill buffer completely
        let large_data = vec![0xAAu8; PIPE_BUF_SIZE];
        let written = pipe.write(&large_data).unwrap();
        assert_eq!(written, PIPE_BUF_SIZE);
        
        assert!(pipe.is_full());
        
        // Next write should fail or return 0
        let more_data = b"more";
        let result = pipe.write(more_data);
        assert!(result.is_err() || result.unwrap() == 0);
    }

    #[test]
    fn test_pipe_wrap_around() {
        let mut pipe = PipeBuffer::new();
        
        // Write half buffer
        let half = vec![0xAAu8; PIPE_BUF_SIZE / 2];
        pipe.write(&half).unwrap();
        
        // Read half buffer
        let mut buffer = vec![0u8; PIPE_BUF_SIZE / 2];
        pipe.read(&mut buffer).unwrap();
        
        // Write another full buffer (should wrap around)
        let full = vec![0xBBu8; PIPE_BUF_SIZE];
        let written = pipe.write(&full).unwrap();
        assert_eq!(written, PIPE_BUF_SIZE);
        
        // Read and verify wrap-around worked
        let mut read_buf = vec![0u8; PIPE_BUF_SIZE];
        let read = pipe.read(&mut read_buf).unwrap();
        assert_eq!(read, PIPE_BUF_SIZE);
        assert!(read_buf.iter().all(|&b| b == 0xBB));
    }

    #[test]
    fn test_pipe_boundary_write() {
        let mut pipe = PipeBuffer::new();
        
        // Write exactly PIPE_BUF_SIZE - 1 bytes
        let data = vec![0xAAu8; PIPE_BUF_SIZE - 1];
        pipe.write(&data).unwrap();
        
        // Should have space for exactly 1 more byte
        assert_eq!(pipe.available_space(), 1);
        
        // Write 1 byte
        pipe.write(&[0xBB]).unwrap();
        
        // Now full
        assert!(pipe.is_full());
    }

    // =========================================================================
    // EOF and Pipe Closure Tests
    // =========================================================================

    #[test]
    fn test_pipe_eof_on_write_close() {
        let mut pipe = PipeBuffer::new();
        
        pipe.write(b"Some data").unwrap();
        pipe.close_write();
        
        // Read existing data
        let mut buffer = [0u8; 64];
        let read = pipe.read(&mut buffer).unwrap();
        assert!(read > 0);
        
        // Read again - should get EOF
        let read2 = pipe.read(&mut buffer).unwrap();
        assert_eq!(read2, 0); // EOF
    }

    #[test]
    fn test_pipe_sigpipe_on_read_close() {
        let mut pipe = PipeBuffer::new();
        
        pipe.close_read();
        
        // Write should fail with EPIPE (would trigger SIGPIPE)
        let result = pipe.write(b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_pipe_double_close() {
        let mut pipe = PipeBuffer::new();
        
        pipe.close_read();
        pipe.close_read(); // Should be idempotent
        
        assert!(pipe.state == PipeState::ReadClosed || pipe.state == PipeState::Closed);
    }

    #[test]
    fn test_pipe_close_both_ends() {
        let mut pipe = PipeBuffer::new();
        
        pipe.close_read();
        pipe.close_write();
        
        assert_eq!(pipe.state, PipeState::Closed);
    }

    // =========================================================================
    // Atomic Write Tests (POSIX PIPE_BUF guarantee)
    // =========================================================================

    #[test]
    fn test_posix_pipe_buf_atomic() {
        // POSIX guarantees writes <= PIPE_BUF are atomic
        assert!(PIPE_BUF_SIZE >= 512, "POSIX requires PIPE_BUF >= 512");
        
        // Most systems have PIPE_BUF = 4096
        assert_eq!(PIPE_BUF_SIZE, 4096);
    }

    #[test]
    fn test_write_larger_than_buffer() {
        let mut pipe = PipeBuffer::new();
        
        // Write more than buffer size
        let large_data = vec![0xAAu8; PIPE_BUF_SIZE * 2];
        
        // Should only write up to available space
        let written = pipe.write(&large_data).unwrap();
        assert_eq!(written, PIPE_BUF_SIZE);
    }

    // =========================================================================
    // Empty Pipe Tests
    // =========================================================================

    #[test]
    fn test_read_empty_pipe() {
        let mut pipe = PipeBuffer::new();
        
        // Read from empty pipe should block (or return EAGAIN in non-blocking)
        let mut buffer = [0u8; 64];
        let result = pipe.read(&mut buffer);
        
        // Our simulation returns EAGAIN
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_write() {
        let mut pipe = PipeBuffer::new();
        
        // Write 0 bytes
        let written = pipe.write(&[]).unwrap();
        assert_eq!(written, 0);
        assert!(pipe.is_empty());
    }

    // =========================================================================
    // Multiple Pipe Tests
    // =========================================================================

    #[test]
    fn test_pipe_pool_limit() {
        // Verify we track the limit
        assert_eq!(MAX_PIPES, 16);
        
        // In real implementation, creating more than MAX_PIPES should fail
    }

    // =========================================================================
    // Edge Cases and Bug Detection
    // =========================================================================

    #[test]
    fn test_count_consistency() {
        let mut pipe = PipeBuffer::new();
        
        // Track count through operations
        assert_eq!(pipe.count, 0);
        
        pipe.write(b"1234567890").unwrap(); // 10 bytes
        assert_eq!(pipe.count, 10);
        
        let mut buf = [0u8; 5];
        pipe.read(&mut buf).unwrap();
        assert_eq!(pipe.count, 5);
        
        pipe.write(b"abcde").unwrap(); // 5 more
        assert_eq!(pipe.count, 10);
    }

    #[test]
    fn test_position_wrap() {
        let mut pipe = PipeBuffer::new();
        
        // Do many small writes and reads to force position wrap
        for _ in 0..100 {
            pipe.write(b"test").unwrap();
            let mut buf = [0u8; 4];
            pipe.read(&mut buf).unwrap();
        }
        
        // Should still work correctly
        pipe.write(b"final").unwrap();
        let mut buf = [0u8; 5];
        let read = pipe.read(&mut buf).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buf, b"final");
    }

    #[test]
    fn test_read_write_interleave() {
        let mut pipe = PipeBuffer::new();
        
        // Interleaved reads and writes
        pipe.write(b"AAA").unwrap();
        pipe.write(b"BBB").unwrap();
        
        let mut buf = [0u8; 3];
        pipe.read(&mut buf).unwrap();
        assert_eq!(&buf, b"AAA");
        
        pipe.write(b"CCC").unwrap();
        
        pipe.read(&mut buf).unwrap();
        assert_eq!(&buf, b"BBB");
        
        pipe.read(&mut buf).unwrap();
        assert_eq!(&buf, b"CCC");
    }

    #[test]
    fn test_state_transitions() {
        let mut pipe = PipeBuffer::new();
        
        // Open -> ReadClosed
        assert_eq!(pipe.state, PipeState::Open);
        pipe.close_read();
        assert_eq!(pipe.state, PipeState::ReadClosed);
        
        // Reset
        pipe = PipeBuffer::new();
        
        // Open -> WriteClosed
        pipe.close_write();
        assert_eq!(pipe.state, PipeState::WriteClosed);
        
        // WriteClosed -> Closed
        pipe.close_read();
        assert_eq!(pipe.state, PipeState::Closed);
    }
}
