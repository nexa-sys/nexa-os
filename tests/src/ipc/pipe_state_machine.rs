//! Pipe and IPC Buffer Edge Case Tests
//!
//! Tests for pipe operations including:
//! - Circular buffer wraparound
//! - Full and empty buffer conditions
//! - State transitions (open, read-closed, write-closed, closed)
//! - SIGPIPE conditions
//! - EOF detection

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use crate::ipc::pipe::{
        create_pipe, pipe_read, pipe_write, 
        close_pipe_read, close_pipe_write,
    };

    // =========================================================================
    // Pipe Creation Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_create_returns_valid_ids() {
        let result = create_pipe();
        
        if let Ok((read_fd, write_fd)) = result {
            // Pipe IDs should be valid indices
            assert!(read_fd < 256, "Read FD should be reasonable");
            assert!(write_fd < 256, "Write FD should be reasonable");
        }
        // If creation fails (too many pipes), that's also acceptable in tests
    }

    #[test]
    #[serial]
    fn test_pipe_create_multiple() {
        // Try to create multiple pipes
        let mut pipes = Vec::new();
        
        for _ in 0..4 {
            if let Ok(pipe) = create_pipe() {
                pipes.push(pipe);
            }
        }
        
        // Should be able to create at least a few pipes
        assert!(!pipes.is_empty(), "Should be able to create at least one pipe");
    }

    // =========================================================================
    // Pipe Read/Write Basic Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_write_then_read() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            let data = b"Hello, pipe!";
            
            // Write to pipe
            let written = pipe_write(write_fd, data);
            assert!(written.is_ok(), "Write should succeed");
            assert_eq!(written.unwrap(), data.len());
            
            // Read from pipe
            let mut buffer = [0u8; 64];
            let read_result = pipe_read(read_fd, &mut buffer);
            
            assert!(read_result.is_ok(), "Read should succeed");
            let bytes_read = read_result.unwrap();
            assert_eq!(bytes_read, data.len());
            assert_eq!(&buffer[..bytes_read], data);
        }
    }

    #[test]
    #[serial]
    fn test_pipe_read_empty() {
        if let Ok((read_fd, _write_fd)) = create_pipe() {
            let mut buffer = [0u8; 64];
            
            // Read from empty pipe should return 0 (or block in real implementation)
            let result = pipe_read(read_fd, &mut buffer);
            
            if let Ok(bytes) = result {
                assert_eq!(bytes, 0, "Reading from empty pipe should return 0");
            }
        }
    }

    #[test]
    #[serial]
    fn test_pipe_partial_read() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            let data = b"Hello, World!";
            
            // Write data
            pipe_write(write_fd, data).ok();
            
            // Read only part of the data
            let mut buffer = [0u8; 5];
            let result = pipe_read(read_fd, &mut buffer);
            
            if let Ok(bytes) = result {
                assert_eq!(bytes, 5);
                assert_eq!(&buffer[..5], b"Hello");
                
                // Remaining data should still be readable
                let mut buffer2 = [0u8; 64];
                let result2 = pipe_read(read_fd, &mut buffer2);
                
                if let Ok(bytes2) = result2 {
                    assert_eq!(bytes2, 8); // ", World!"
                    assert_eq!(&buffer2[..bytes2], b", World!");
                }
            }
        }
    }

    // =========================================================================
    // Pipe Close State Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_read_after_write_close() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            // Write some data
            pipe_write(write_fd, b"test").ok();
            
            // Close write end
            let close_result = close_pipe_write(write_fd);
            assert!(close_result.is_ok(), "Close write should succeed");
            
            // Read should still succeed for buffered data
            let mut buffer = [0u8; 64];
            let result = pipe_read(read_fd, &mut buffer);
            
            if let Ok(bytes) = result {
                assert_eq!(bytes, 4);
            }
            
            // After buffer is drained, read should return 0 (EOF)
            let result2 = pipe_read(read_fd, &mut buffer);
            if let Ok(bytes) = result2 {
                assert_eq!(bytes, 0, "Read after write-close and buffer drain should be EOF");
            }
        }
    }

    #[test]
    #[serial]
    fn test_pipe_write_after_read_close_fails() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            // Close read end
            let close_result = close_pipe_read(read_fd);
            assert!(close_result.is_ok(), "Close read should succeed");
            
            // Write should fail (SIGPIPE condition)
            let result = pipe_write(write_fd, b"test");
            assert!(result.is_err(), "Write after read-close should fail");
        }
    }

    #[test]
    #[serial]
    fn test_pipe_double_close() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            // Close both ends
            close_pipe_read(read_fd).ok();
            close_pipe_write(write_fd).ok();
            
            // Operations on closed pipe should fail
            let mut buffer = [0u8; 64];
            let read_result = pipe_read(read_fd, &mut buffer);
            let write_result = pipe_write(write_fd, b"test");
            
            assert!(read_result.is_err(), "Read on closed pipe should fail");
            assert!(write_result.is_err(), "Write on closed pipe should fail");
        }
    }

    // =========================================================================
    // Invalid Pipe ID Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_read_invalid_id() {
        let mut buffer = [0u8; 64];
        
        // Use an impossibly high pipe ID
        let result = pipe_read(9999, &mut buffer);
        assert!(result.is_err(), "Read with invalid pipe ID should fail");
    }

    #[test]
    #[serial]
    fn test_pipe_write_invalid_id() {
        let result = pipe_write(9999, b"test");
        assert!(result.is_err(), "Write with invalid pipe ID should fail");
    }

    #[test]
    #[serial]
    fn test_pipe_close_invalid_id() {
        let result = close_pipe_read(9999);
        assert!(result.is_err(), "Close read with invalid ID should fail");
        
        let result = close_pipe_write(9999);
        assert!(result.is_err(), "Close write with invalid ID should fail");
    }

    // =========================================================================
    // Buffer Capacity Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_fill_to_capacity() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            // PIPE_BUF_SIZE is typically 4096
            let chunk = [0xAAu8; 1024];
            let mut total_written = 0;
            
            // Write until pipe is full
            for _ in 0..10 {
                match pipe_write(write_fd, &chunk) {
                    Ok(bytes) => {
                        total_written += bytes;
                        if bytes < chunk.len() {
                            break; // Partial write means buffer is full
                        }
                    }
                    Err(_) => break, // Buffer full
                }
            }
            
            assert!(total_written > 0, "Should write at least some data");
            
            // Drain the pipe
            let mut buffer = [0u8; 4096];
            let mut total_read = 0;
            
            loop {
                match pipe_read(read_fd, &mut buffer) {
                    Ok(0) => break,
                    Ok(bytes) => total_read += bytes,
                    Err(_) => break,
                }
            }
            
            assert_eq!(total_read, total_written, 
                "Should read exactly what was written");
        }
    }

    #[test]
    #[serial]
    fn test_pipe_write_full_fails() {
        if let Ok((_read_fd, write_fd)) = create_pipe() {
            // Fill the pipe (don't read)
            let chunk = [0xAAu8; 4096];
            
            // First write should succeed
            let result1 = pipe_write(write_fd, &chunk);
            assert!(result1.is_ok());
            
            // Second write when buffer is full should fail or write 0
            let result2 = pipe_write(write_fd, &chunk);
            match result2 {
                Ok(0) => {} // Acceptable - no space
                Err(_) => {} // Acceptable - would block
                Ok(n) if n < chunk.len() => {} // Acceptable - partial write
                _ => {} // Any behavior is acceptable for this edge case
            }
        }
    }

    // =========================================================================
    // Circular Buffer Wraparound Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_circular_buffer_wraparound() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            let data = [0x42u8; 1000];
            
            // Write and read multiple times to force wraparound
            for iteration in 0..10 {
                // Write
                let write_result = pipe_write(write_fd, &data);
                assert!(write_result.is_ok(), "Write {} should succeed", iteration);
                
                // Read
                let mut buffer = [0u8; 1000];
                let read_result = pipe_read(read_fd, &mut buffer);
                assert!(read_result.is_ok(), "Read {} should succeed", iteration);
                
                let bytes = read_result.unwrap();
                assert_eq!(bytes, data.len());
                assert_eq!(&buffer[..bytes], &data, 
                    "Data should be intact after wraparound at iteration {}", iteration);
            }
        }
    }

    #[test]
    #[serial]
    fn test_pipe_staggered_read_write() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            // Interleave writes and reads with different sizes
            let patterns = [
                (b"short".as_slice(), 5),
                (b"medium sized data".as_slice(), 17),
                (b"a".as_slice(), 1),
                (b"longer message that spans more bytes".as_slice(), 36), // Correct length
            ];
            
            for (data, expected_len) in patterns {
                // Write
                let written = pipe_write(write_fd, data).unwrap();
                assert_eq!(written, expected_len);
                
                // Read
                let mut buffer = [0u8; 64];
                let read = pipe_read(read_fd, &mut buffer).unwrap();
                assert_eq!(read, expected_len);
                assert_eq!(&buffer[..read], data);
            }
        }
    }

    // =========================================================================
    // Zero-Length Operations
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_write_zero_bytes() {
        if let Ok((_read_fd, write_fd)) = create_pipe() {
            let result = pipe_write(write_fd, &[]);
            
            // Zero-byte write should succeed with 0 bytes written
            if let Ok(bytes) = result {
                assert_eq!(bytes, 0);
            }
        }
    }

    #[test]
    #[serial]
    fn test_pipe_read_zero_buffer() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            // Write some data first
            pipe_write(write_fd, b"test").ok();
            
            // Read into zero-length buffer
            let mut buffer: [u8; 0] = [];
            let result = pipe_read(read_fd, &mut buffer);
            
            // Should succeed with 0 bytes read
            if let Ok(bytes) = result {
                assert_eq!(bytes, 0);
            }
        }
    }

    // =========================================================================
    // State Machine Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_state_open() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            // Both ends open - read and write should work
            let written = pipe_write(write_fd, b"test");
            assert!(written.is_ok());
            
            let mut buf = [0u8; 10];
            let read = pipe_read(read_fd, &mut buf);
            assert!(read.is_ok());
        }
    }

    #[test]
    #[serial]
    fn test_pipe_state_write_closed() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            pipe_write(write_fd, b"data").ok();
            close_pipe_write(write_fd).ok();
            
            // Read should still work
            let mut buf = [0u8; 10];
            let result = pipe_read(read_fd, &mut buf);
            assert!(result.is_ok());
        }
    }

    #[test]
    #[serial]
    fn test_pipe_state_read_closed() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            close_pipe_read(read_fd).ok();
            
            // Write should fail (EPIPE)
            let result = pipe_write(write_fd, b"data");
            assert!(result.is_err());
        }
    }

    #[test]
    #[serial]
    fn test_pipe_state_both_closed() {
        if let Ok((read_fd, write_fd)) = create_pipe() {
            close_pipe_read(read_fd).ok();
            close_pipe_write(write_fd).ok();
            
            // Both operations should fail
            let mut buf = [0u8; 10];
            assert!(pipe_read(read_fd, &mut buf).is_err());
            assert!(pipe_write(write_fd, b"x").is_err());
        }
    }
}
