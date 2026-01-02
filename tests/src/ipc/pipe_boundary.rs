//! Pipe Edge Case and Boundary Tests
//!
//! Tests for pipe implementation edge cases, buffer management, and error handling:
//! - Buffer full/empty conditions
//! - Partial reads/writes
//! - Closed pipe behavior (EPIPE, EOF)
//! - Concurrent access scenarios
//! - Maximum pipe count limits

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use crate::ipc::pipe::{
        create_pipe, pipe_read, pipe_write, close_pipe_read, close_pipe_write,
    };

    // =========================================================================
    // Pipe Creation Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_create_pipe_success() {
        let result = create_pipe();
        assert!(result.is_ok(), "First pipe creation should succeed");
        
        let (read_end, write_end) = result.unwrap();
        // Clean up
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_create_multiple_pipes() {
        let mut pipes = Vec::new();
        
        // Create several pipes
        for i in 0..8 {
            let result = create_pipe();
            if let Ok((r, w)) = result {
                pipes.push((r, w));
            } else {
                // Expected if we hit the limit
                break;
            }
        }
        
        // Clean up
        for (r, w) in pipes {
            let _ = close_pipe_read(r);
            let _ = close_pipe_write(w);
        }
    }

    // =========================================================================
    // Invalid Pipe ID Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_read_invalid_pipe_id() {
        let mut buffer = [0u8; 64];
        
        // Very large pipe ID should fail
        let result = pipe_read(9999, &mut buffer);
        assert!(result.is_err(), "Reading from invalid pipe ID should fail");
    }

    #[test]
    #[serial]
    fn test_write_invalid_pipe_id() {
        let data = [0u8; 64];
        
        let result = pipe_write(9999, &data);
        assert!(result.is_err(), "Writing to invalid pipe ID should fail");
    }

    #[test]
    #[serial]
    fn test_close_invalid_pipe_id() {
        let result = close_pipe_read(9999);
        assert!(result.is_err(), "Closing invalid pipe ID should fail");
        
        let result = close_pipe_write(9999);
        assert!(result.is_err(), "Closing invalid pipe ID should fail");
    }

    // =========================================================================
    // Empty Buffer Read Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_read_from_empty_pipe() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        let mut buffer = [0u8; 64];
        let result = pipe_read(read_end, &mut buffer);
        
        // Should return 0 bytes (empty) or block, not error
        assert!(result.is_ok(), "Reading from empty pipe should not error");
        assert_eq!(result.unwrap(), 0, "Empty pipe read should return 0 bytes");
        
        // Clean up
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_read_with_zero_buffer() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        // Write some data first
        let _ = pipe_write(write_end, b"test");
        
        // Read with zero-length buffer
        let mut buffer = [0u8; 0];
        let result = pipe_read(read_end, &mut buffer);
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0, "Zero-length read should return 0");
        
        // Clean up
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Write Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_write_and_read() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        let data = b"Hello, pipe!";
        let written = pipe_write(write_end, data).expect("write failed");
        assert_eq!(written, data.len());
        
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).expect("read failed");
        assert_eq!(read, data.len());
        assert_eq!(&buffer[..read], data);
        
        // Clean up
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_write_zero_bytes() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        let result = pipe_write(write_end, &[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
        
        // Clean up
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Closed Pipe End Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_write_to_closed_read_end() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        // Close read end
        close_pipe_read(read_end).expect("close failed");
        
        // Write should fail with EPIPE
        let result = pipe_write(write_end, b"test");
        assert!(result.is_err(), "Write to pipe with closed read end should fail");
        
        // Clean up
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_read_from_closed_write_end() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        // Write some data
        pipe_write(write_end, b"data").expect("write failed");
        
        // Close write end
        close_pipe_write(write_end).expect("close failed");
        
        // Read should succeed and return data
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).expect("read failed");
        assert_eq!(read, 4);
        
        // Next read should return EOF (0)
        let read = pipe_read(read_end, &mut buffer).expect("read failed");
        assert_eq!(read, 0, "Read from closed write end with empty buffer should return EOF");
        
        // Clean up
        let _ = close_pipe_read(read_end);
    }

    #[test]
    #[serial]
    fn test_double_close() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        // Close once
        close_pipe_read(read_end).expect("first close should succeed");
        
        // Close again - should fail gracefully
        let result = close_pipe_read(read_end);
        assert!(result.is_err(), "Double close should fail");
        
        // Clean up
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Buffer Boundary Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_partial_read() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        // Write 100 bytes
        let data = [0xABu8; 100];
        pipe_write(write_end, &data).expect("write failed");
        
        // Read only 10 bytes
        let mut buffer = [0u8; 10];
        let read = pipe_read(read_end, &mut buffer).expect("read failed");
        assert_eq!(read, 10);
        assert_eq!(buffer, [0xABu8; 10]);
        
        // Read remaining
        let mut buffer2 = [0u8; 100];
        let read2 = pipe_read(read_end, &mut buffer2).expect("read failed");
        assert_eq!(read2, 90, "Should read remaining 90 bytes");
        
        // Clean up
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_multiple_small_writes() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        // Multiple small writes
        for i in 0..10 {
            let data = [i as u8; 10];
            pipe_write(write_end, &data).expect("write failed");
        }
        
        // Single large read
        let mut buffer = [0u8; 100];
        let read = pipe_read(read_end, &mut buffer).expect("read failed");
        assert_eq!(read, 100);
        
        // Verify data integrity
        for i in 0..10 {
            for j in 0..10 {
                assert_eq!(buffer[i * 10 + j], i as u8);
            }
        }
        
        // Clean up
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Close Order Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_close_write_then_read() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        close_pipe_write(write_end).expect("close write should succeed");
        close_pipe_read(read_end).expect("close read should succeed");
    }

    #[test]
    #[serial]
    fn test_close_read_then_write() {
        let (read_end, write_end) = create_pipe().expect("pipe creation failed");
        
        close_pipe_read(read_end).expect("close read should succeed");
        close_pipe_write(write_end).expect("close write should succeed");
    }

    // =========================================================================
    // Pipe Reuse Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_slot_reuse() {
        // Create and close a pipe
        let (read_end, write_end) = create_pipe().expect("first pipe creation failed");
        close_pipe_read(read_end).expect("close failed");
        close_pipe_write(write_end).expect("close failed");
        
        // Create another pipe - should reuse the slot
        let result = create_pipe();
        assert!(result.is_ok(), "Pipe slot should be reusable after close");
        
        let (r2, w2) = result.unwrap();
        let _ = close_pipe_read(r2);
        let _ = close_pipe_write(w2);
    }
}
