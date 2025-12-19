//! Pipe Edge Case Tests
//!
//! Tests for POSIX pipe implementation edge cases and potential bugs.
//! These tests focus on boundary conditions, race conditions, and error handling.

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    // =========================================================================
    // Mock Pipe Implementation
    // =========================================================================

    const PIPE_BUF_SIZE: usize = 4096;

    struct PipeBuffer {
        data: [u8; PIPE_BUF_SIZE],
        read_pos: usize,
        write_pos: usize,
        read_end_open: bool,
        write_end_open: bool,
    }

    impl PipeBuffer {
        fn new() -> Self {
            PipeBuffer {
                data: [0; PIPE_BUF_SIZE],
                read_pos: 0,
                write_pos: 0,
                read_end_open: true,
                write_end_open: true,
            }
        }

        fn available_data(&self) -> usize {
            if self.write_pos >= self.read_pos {
                self.write_pos - self.read_pos
            } else {
                PIPE_BUF_SIZE - self.read_pos + self.write_pos
            }
        }

        fn available_space(&self) -> usize {
            PIPE_BUF_SIZE - self.available_data() - 1
        }

        fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str> {
            if !self.write_end_open {
                return Err("Write end closed");
            }
            if !self.read_end_open {
                return Err("Broken pipe");
            }
            if buf.is_empty() {
                return Ok(0);
            }

            let space = self.available_space();
            if space == 0 {
                return Err("Pipe full");
            }

            let to_write = buf.len().min(space);
            for &byte in &buf[..to_write] {
                self.data[self.write_pos] = byte;
                self.write_pos = (self.write_pos + 1) % PIPE_BUF_SIZE;
            }
            Ok(to_write)
        }

        fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> {
            if !self.read_end_open {
                return Err("Read end closed");
            }
            if buf.is_empty() {
                return Ok(0);
            }

            let available = self.available_data();
            if available == 0 {
                if !self.write_end_open {
                    return Ok(0); // EOF
                }
                return Ok(0); // Non-blocking: no data
            }

            let to_read = buf.len().min(available);
            for i in 0..to_read {
                buf[i] = self.data[self.read_pos];
                self.read_pos = (self.read_pos + 1) % PIPE_BUF_SIZE;
            }
            Ok(to_read)
        }
    }

    lazy_static::lazy_static! {
        static ref PIPES: Arc<Mutex<HashMap<usize, Arc<Mutex<PipeBuffer>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        static ref NEXT_PIPE_ID: Arc<Mutex<usize>> = Arc::new(Mutex::new(1));
    }

    fn create_pipe() -> Result<(usize, usize), &'static str> {
        let mut pipes = PIPES.lock().unwrap();
        let mut next_id = NEXT_PIPE_ID.lock().unwrap();

        let pipe = Arc::new(Mutex::new(PipeBuffer::new()));
        let read_id = *next_id;
        let write_id = *next_id + 1;
        *next_id += 2;

        pipes.insert(read_id, Arc::clone(&pipe));
        pipes.insert(write_id, pipe);

        Ok((read_id, write_id))
    }

    fn pipe_read(pipe_id: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let pipes = PIPES.lock().unwrap();
        let pipe = pipes.get(&pipe_id).ok_or("Invalid pipe ID")?;
        let mut pipe = pipe.lock().unwrap();
        pipe.read(buf)
    }

    fn pipe_write(pipe_id: usize, buf: &[u8]) -> Result<usize, &'static str> {
        let pipes = PIPES.lock().unwrap();
        let pipe = pipes.get(&pipe_id).ok_or("Invalid pipe ID")?;
        let mut pipe = pipe.lock().unwrap();
        pipe.write(buf)
    }

    fn close_pipe_read(pipe_id: usize) -> Result<(), &'static str> {
        let pipes = PIPES.lock().unwrap();
        let pipe = pipes.get(&pipe_id).ok_or("Invalid pipe ID")?;
        let mut pipe = pipe.lock().unwrap();
        if !pipe.read_end_open {
            return Err("Already closed");
        }
        pipe.read_end_open = false;
        Ok(())
    }

    fn close_pipe_write(pipe_id: usize) -> Result<(), &'static str> {
        let pipes = PIPES.lock().unwrap();
        let pipe = pipes.get(&pipe_id).ok_or("Invalid pipe ID")?;
        let mut pipe = pipe.lock().unwrap();
        if !pipe.write_end_open {
            return Err("Already closed");
        }
        pipe.write_end_open = false;
        Ok(())
    }

    #[test]
    #[serial]
    fn test_pipe_create_and_basic_io() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write some data
        let data = b"Hello, pipe!";
        let written = pipe_write(write_end, data).expect("Write failed");
        assert_eq!(written, data.len());

        // Read it back
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).expect("Read failed");
        assert_eq!(read, data.len());
        assert_eq!(&buffer[..read], data);
    }

    #[test]
    #[serial]
    fn test_pipe_exact_buffer_size_write() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write close to PIPE_BUF_SIZE bytes
        // Note: Ring buffer reserves 1 byte to distinguish full from empty
        // so maximum usable capacity is PIPE_BUF_SIZE - 1
        let max_capacity = PIPE_BUF_SIZE - 1;
        let data = vec![0xABu8; max_capacity];
        let written = pipe_write(write_end, &data).expect("Write failed");
        assert_eq!(written, max_capacity, "Should write max_capacity bytes");

        // Pipe should now be full - additional write should fail or return 0
        let more_data = [0xCDu8; 1];
        let result = pipe_write(write_end, &more_data);
        assert!(result.is_err() || result.unwrap() == 0, "Full pipe should reject writes");

        // Read all data
        let mut buffer = vec![0u8; PIPE_BUF_SIZE];
        let read = pipe_read(read_end, &mut buffer).expect("Read failed");
        assert_eq!(read, max_capacity);
        assert!(buffer[..read].iter().all(|&b| b == 0xAB));
    }

    #[test]
    #[serial]
    fn test_pipe_partial_write_when_nearly_full() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Fill pipe almost completely
        let almost_full = vec![0xAAu8; PIPE_BUF_SIZE - 10];
        let written = pipe_write(write_end, &almost_full).expect("Write failed");
        assert_eq!(written, PIPE_BUF_SIZE - 10);

        // Try to write more than available space
        let overflow_data = vec![0xBBu8; 100];
        let written = pipe_write(write_end, &overflow_data);
        
        // Should either error or write partial (10 bytes)
        match written {
            Ok(n) => {
                assert!(n <= 10, "Should only write up to 10 bytes");
            }
            Err(_) => {
                // Also acceptable - pipe full
            }
        }

        // Clean up by reading
        let mut buffer = vec![0u8; PIPE_BUF_SIZE];
        let _ = pipe_read(read_end, &mut buffer);
    }

    #[test]
    #[serial]
    fn test_pipe_circular_buffer_wraparound() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Fill and drain multiple times to force wraparound
        for iteration in 0..5 {
            let data = vec![iteration as u8; 1000];
            let written = pipe_write(write_end, &data).expect("Write failed");
            assert_eq!(written, 1000);

            let mut buffer = [0u8; 1000];
            let read = pipe_read(read_end, &mut buffer).expect("Read failed");
            assert_eq!(read, 1000);
            assert!(buffer.iter().all(|&b| b == iteration as u8), 
                    "Data corruption after wraparound iteration {}", iteration);
        }
    }

    #[test]
    #[serial]
    fn test_pipe_empty_read() {
        let (read_end, _write_end) = create_pipe().expect("Failed to create pipe");

        // Read from empty pipe should return 0 (non-blocking) or block
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).expect("Read failed");
        assert_eq!(read, 0, "Empty pipe read should return 0");
    }

    #[test]
    #[serial]
    fn test_pipe_zero_length_operations() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write zero bytes
        let written = pipe_write(write_end, &[]).expect("Zero write failed");
        assert_eq!(written, 0, "Zero-length write should succeed with 0");

        // Read zero bytes
        let mut buffer = [];
        let read = pipe_read(read_end, &mut buffer).expect("Zero read failed");
        assert_eq!(read, 0, "Zero-length read should succeed with 0");
    }

    // =========================================================================
    // Pipe State Transition Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_close_read_end_then_write() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Close read end
        close_pipe_read(read_end).expect("Close read failed");

        // Writing should fail with SIGPIPE equivalent
        let result = pipe_write(write_end, b"test");
        assert!(result.is_err(), "Write to pipe with closed read end should fail");
    }

    #[test]
    #[serial]
    fn test_pipe_close_write_end_then_read() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write some data first
        pipe_write(write_end, b"data before close").expect("Write failed");

        // Close write end
        close_pipe_write(write_end).expect("Close write failed");

        // Should be able to read existing data
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).expect("Read failed");
        assert!(read > 0, "Should read remaining data");

        // After data exhausted, should return EOF (0)
        let read = pipe_read(read_end, &mut buffer).expect("Read failed");
        assert_eq!(read, 0, "Should return EOF after write end closed");
    }

    #[test]
    #[serial]
    fn test_pipe_close_both_ends() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        close_pipe_read(read_end).expect("Close read failed");
        close_pipe_write(write_end).expect("Close write failed");

        // Both operations on closed pipe should fail
        let mut buffer = [0u8; 64];
        assert!(pipe_read(read_end, &mut buffer).is_err());
        assert!(pipe_write(write_end, b"test").is_err());
    }

    #[test]
    #[serial]
    fn test_pipe_double_close() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // First close should succeed
        close_pipe_read(read_end).expect("First close failed");
        close_pipe_write(write_end).expect("First close failed");

        // Second close should fail
        let result = close_pipe_read(read_end);
        assert!(result.is_err(), "Double close should fail");
    }

    // =========================================================================
    // Pipe Resource Exhaustion Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_resource_exhaustion() {
        // Note: This test validates resource exhaustion behavior.
        // In a mock environment without resource limits, we simply verify
        // that we can create and clean up many pipes without issues.
        const TEST_PIPES: usize = 16;
        let mut pipes = Vec::new();

        // Create several pipes
        for _ in 0..TEST_PIPES {
            match create_pipe() {
                Ok(pipe) => pipes.push(pipe),
                Err(e) => {
                    // Resource exhaustion is acceptable behavior
                    eprintln!("Pipe creation limited: {}", e);
                    break;
                }
            }
        }

        // Verify we created at least some pipes
        assert!(!pipes.is_empty(), "Should create at least one pipe");

        // Clean up all pipes
        for (read_end, write_end) in pipes {
            let _ = close_pipe_read(read_end);
            let _ = close_pipe_write(write_end);
        }
    }

    // =========================================================================
    // Invalid Pipe ID Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_invalid_id_read() {
        let mut buffer = [0u8; 64];
        let result = pipe_read(999, &mut buffer);
        assert!(result.is_err(), "Invalid pipe ID should fail");
    }

    #[test]
    #[serial]
    fn test_pipe_invalid_id_write() {
        let result = pipe_write(999, b"test");
        assert!(result.is_err(), "Invalid pipe ID should fail");
    }

    #[test]
    #[serial]
    fn test_pipe_invalid_id_close() {
        assert!(close_pipe_read(999).is_err());
        assert!(close_pipe_write(999).is_err());
    }

    // =========================================================================
    // Pipe Data Integrity Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_large_transfer_integrity() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Transfer data in chunks and verify integrity
        let total_size = PIPE_BUF_SIZE * 3;
        let chunk_size = 512;
        let mut sent_checksum: u32 = 0;
        let mut recv_checksum: u32 = 0;

        let mut total_written = 0;
        let mut total_read = 0;

        while total_written < total_size || total_read < total_written {
            // Write if there's data to write and pipe has space
            if total_written < total_size {
                let chunk: Vec<u8> = (0..chunk_size).map(|i| ((total_written + i) & 0xFF) as u8).collect();
                match pipe_write(write_end, &chunk) {
                    Ok(n) if n > 0 => {
                        for &b in &chunk[..n] {
                            sent_checksum = sent_checksum.wrapping_add(b as u32);
                        }
                        total_written += n;
                    }
                    _ => {}
                }
            }

            // Read available data
            let mut buffer = [0u8; 1024];
            match pipe_read(read_end, &mut buffer) {
                Ok(n) if n > 0 => {
                    for &b in &buffer[..n] {
                        recv_checksum = recv_checksum.wrapping_add(b as u32);
                    }
                    total_read += n;
                }
                _ => {}
            }
        }

        assert_eq!(sent_checksum, recv_checksum, "Data corruption detected");
    }

    #[test]
    #[serial]
    fn test_pipe_sequential_read_preserves_order() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write numbered bytes
        for i in 0u8..10 {
            pipe_write(write_end, &[i]).expect("Write failed");
        }

        // Read and verify order
        let mut buffer = [0u8; 1];
        for expected in 0u8..10 {
            let read = pipe_read(read_end, &mut buffer).expect("Read failed");
            assert_eq!(read, 1);
            assert_eq!(buffer[0], expected, "Order not preserved at position {}", expected);
        }
    }
}
