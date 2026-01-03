//! Pipe Implementation Tests
//!
//! Tests for pipe buffer management and read/write semantics
//! using REAL kernel pipe functions.

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
    fn test_pipe_creation() {
        let result = create_pipe();
        assert!(result.is_ok(), "Pipe creation should succeed");
        
        let (read_end, write_end) = result.unwrap();
        assert!(read_end < 16); // MAX_PIPES
        assert!(write_end < 16);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_basic_io() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        let data = b"Hello";
        let written = pipe_write(write_end, data).unwrap();
        assert_eq!(written, 5);
        
        let mut buffer = [0u8; 32];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buffer[..5], b"Hello");
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Ring Buffer Behavior Tests (via kernel pipe)
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_empty_read() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Empty pipe read returns 0
        let mut buffer = [0u8; 32];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 0);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_fifo_order() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Write multiple chunks
        pipe_write(write_end, b"ABC").unwrap();
        pipe_write(write_end, b"DEF").unwrap();
        
        // Should read in FIFO order
        let mut buffer = [0u8; 32];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"ABCDEF");
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_partial_read() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        pipe_write(write_end, b"Hello World").unwrap();
        
        // Read only first 5 bytes
        let mut buffer = [0u8; 5];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buffer[..read], b"Hello");
        
        // Remaining data should still be there
        let mut buffer = [0u8; 32];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 6);
        assert_eq!(&buffer[..read], b" World");
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_multiple_partial_reads() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        pipe_write(write_end, b"0123456789").unwrap();
        
        // Read in small chunks
        let mut buffer = [0u8; 3];
        
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"012");
        
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"345");
        
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"678");
        
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"9");
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Pipe End State Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_close_write_eof() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        pipe_write(write_end, b"data").unwrap();
        close_pipe_write(write_end).unwrap();
        
        // Read remaining data
        let mut buffer = [0u8; 32];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 4);
        
        // Next read should return EOF (0)
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 0);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
    }

    #[test]
    #[serial]
    fn test_pipe_broken_pipe() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Close read end first
        close_pipe_read(read_end).unwrap();
        
        // Write to pipe with no readers should fail (EPIPE)
        let result = pipe_write(write_end, b"data");
        assert!(result.is_err());
        
        // Cleanup
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_close_both_ends() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        close_pipe_read(read_end).unwrap();
        close_pipe_write(write_end).unwrap();
        
        // Operations on closed pipe should fail
        let mut buffer = [0u8; 32];
        let result = pipe_read(read_end, &mut buffer);
        assert!(result.is_err());
    }

    // =========================================================================
    // Blocking vs Non-Blocking Behavior Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_non_blocking_empty_read() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Our implementation returns 0 for empty pipe (non-blocking behavior)
        let mut buffer = [0u8; 32];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 0);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Pipe Buffer Capacity Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_buffer_fill() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Write data to fill buffer
        let chunk = [0xAAu8; 1024];
        let mut total_written = 0;
        
        // Keep writing until buffer is full or we've written enough
        for _ in 0..4 {
            match pipe_write(write_end, &chunk) {
                Ok(n) => total_written += n,
                Err(_) => break, // Buffer full
            }
        }
        
        assert!(total_written > 0, "Should write some data");
        
        // Read all data back
        let mut total_read = 0;
        let mut buffer = [0u8; 4096];
        
        loop {
            let read = pipe_read(read_end, &mut buffer).unwrap();
            if read == 0 {
                break;
            }
            total_read += read;
        }
        
        assert_eq!(total_read, total_written);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Error Handling Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_invalid_id() {
        let mut buffer = [0u8; 32];
        
        // Invalid pipe ID should fail
        let result = pipe_read(999, &mut buffer);
        assert!(result.is_err());
        
        let result = pipe_write(999, b"data");
        assert!(result.is_err());
    }

    // =========================================================================
    // Atomicity Tests (PIPE_BUF guarantee)
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_atomic_small_write() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Small writes (< PIPE_BUF) should be atomic
        let small_data = [0x55u8; 512];
        let written = pipe_write(write_end, &small_data).unwrap();
        
        // Should write all or nothing (atomic)
        assert_eq!(written, 512);
        
        // Read should get all data
        let mut buffer = [0u8; 1024];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 512);
        assert!(buffer[..512].iter().all(|&b| b == 0x55));
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Multiple Pipes Test
    // =========================================================================

    #[test]
    #[serial]
    fn test_multiple_pipes_independent() {
        let (read1, write1) = create_pipe().unwrap();
        let (read2, write2) = create_pipe().unwrap();
        
        // Write different data to each pipe
        pipe_write(write1, b"pipe1").unwrap();
        pipe_write(write2, b"pipe2").unwrap();
        
        // Read from each pipe
        let mut buffer = [0u8; 32];
        
        let read = pipe_read(read1, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"pipe1");
        
        let read = pipe_read(read2, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"pipe2");
        
        // Cleanup
        let _ = close_pipe_read(read1);
        let _ = close_pipe_write(write1);
        let _ = close_pipe_read(read2);
        let _ = close_pipe_write(write2);
    }
}
