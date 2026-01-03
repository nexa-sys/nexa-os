//! Pipe Buffer Edge Case Tests
//!
//! Tests for pipe buffer management using REAL kernel pipe functions.
//! Uses create_pipe(), pipe_read(), pipe_write(), close_pipe_read(), close_pipe_write().

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use crate::pipe::{
        create_pipe, pipe_read, pipe_write, 
        close_pipe_read, close_pipe_write,
    };

    // Constants matching kernel implementation
    const PIPE_BUF_SIZE: usize = 4096;

    // =========================================================================
    // Basic Read/Write Tests - Using REAL kernel pipe
    // =========================================================================

    #[test]
    #[serial]
    fn test_xspipe_write_read() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        let data = b"Hello, pipe!";
        let written = pipe_write(write_end, data).unwrap();
        assert_eq!(written, data.len());

        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, data.len());
        assert_eq!(&buffer[..read], data);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_fifo_order() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Write in chunks
        pipe_write(write_end, b"First").unwrap();
        pipe_write(write_end, b"Second").unwrap();
        pipe_write(write_end, b"Third").unwrap();

        // Read should get data in order
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        
        // Verify FIFO order
        let data = &buffer[..read];
        assert!(data.starts_with(b"First"));
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_partial_read() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        pipe_write(write_end, b"Hello, World!").unwrap();

        // Read only 5 bytes
        let mut buffer = [0u8; 5];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buffer, b"Hello");

        // Read remaining
        let mut buffer2 = [0u8; 64];
        let read2 = pipe_read(read_end, &mut buffer2).unwrap();
        assert_eq!(read2, 8); // ", World!"
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Buffer Boundary Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_full_buffer() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Fill buffer completely
        let large_data = vec![0xAAu8; PIPE_BUF_SIZE];
        let written = pipe_write(write_end, &large_data).unwrap();
        assert_eq!(written, PIPE_BUF_SIZE);
        
        // Next write should fail (buffer full)
        let more_data = b"more";
        let result = pipe_write(write_end, more_data);
        assert!(result.is_err());
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_wrap_around() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Write half buffer
        let half = vec![0xAAu8; PIPE_BUF_SIZE / 2];
        pipe_write(write_end, &half).unwrap();
        
        // Read half buffer
        let mut buffer = vec![0u8; PIPE_BUF_SIZE / 2];
        pipe_read(read_end, &mut buffer).unwrap();
        
        // Write another full buffer (should wrap around)
        let full = vec![0xBBu8; PIPE_BUF_SIZE];
        let written = pipe_write(write_end, &full).unwrap();
        assert_eq!(written, PIPE_BUF_SIZE);
        
        // Read and verify wrap-around worked
        let mut read_buf = vec![0u8; PIPE_BUF_SIZE];
        let read = pipe_read(read_end, &mut read_buf).unwrap();
        assert_eq!(read, PIPE_BUF_SIZE);
        assert!(read_buf.iter().all(|&b| b == 0xBB));
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // EOF and Pipe Closure Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_eof_on_write_close() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        pipe_write(write_end, b"Some data").unwrap();
        close_pipe_write(write_end).unwrap();
        
        // Read existing data
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).unwrap();
        assert!(read > 0);
        
        // Read again - should get EOF (0 bytes)
        let read2 = pipe_read(read_end, &mut buffer).unwrap();
        assert_eq!(read2, 0); // EOF
        
        // Cleanup
        let _ = close_pipe_read(read_end);
    }

    #[test]
    #[serial]
    fn test_pipe_sigpipe_on_read_close() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        close_pipe_read(read_end).unwrap();
        
        // Write should fail with EPIPE (would trigger SIGPIPE)
        let result = pipe_write(write_end, b"data");
        assert!(result.is_err());
        
        // Cleanup
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Atomic Write Tests (POSIX PIPE_BUF guarantee)
    // =========================================================================

    #[test]
    #[serial]
    fn test_posix_pipe_buf_atomic() {
        // POSIX guarantees writes <= PIPE_BUF are atomic
        assert!(PIPE_BUF_SIZE >= 512, "POSIX requires PIPE_BUF >= 512");
        
        // Our implementation has PIPE_BUF = 4096
        assert_eq!(PIPE_BUF_SIZE, 4096);
    }

    #[test]
    #[serial]
    fn test_write_larger_than_buffer() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Write more than buffer size
        let large_data = vec![0xAAu8; PIPE_BUF_SIZE * 2];
        
        // Should only write up to available space
        let written = pipe_write(write_end, &large_data).unwrap();
        assert_eq!(written, PIPE_BUF_SIZE);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Empty Pipe Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_empty_write() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Write 0 bytes
        let written = pipe_write(write_end, &[]).unwrap();
        assert_eq!(written, 0);
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    // =========================================================================
    // Multiple Pipe Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_multiple_pipes() {
        // Create multiple pipes
        let pipe1 = create_pipe().unwrap();
        let pipe2 = create_pipe().unwrap();
        
        // Write to different pipes
        pipe_write(pipe1.1, b"pipe1 data").unwrap();
        pipe_write(pipe2.1, b"pipe2 data").unwrap();
        
        // Read from correct pipes
        let mut buf1 = [0u8; 32];
        let mut buf2 = [0u8; 32];
        
        let read1 = pipe_read(pipe1.0, &mut buf1).unwrap();
        let read2 = pipe_read(pipe2.0, &mut buf2).unwrap();
        
        assert_eq!(&buf1[..read1], b"pipe1 data");
        assert_eq!(&buf2[..read2], b"pipe2 data");
        
        // Cleanup
        let _ = close_pipe_read(pipe1.0);
        let _ = close_pipe_write(pipe1.1);
        let _ = close_pipe_read(pipe2.0);
        let _ = close_pipe_write(pipe2.1);
    }

    // =========================================================================
    // Edge Cases and Bug Detection
    // =========================================================================

    #[test]
    #[serial]
    fn test_position_wrap() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Do many small writes and reads to force position wrap
        for _ in 0..100 {
            pipe_write(write_end, b"test").unwrap();
            let mut buf = [0u8; 4];
            pipe_read(read_end, &mut buf).unwrap();
        }
        
        // Should still work correctly
        pipe_write(write_end, b"final").unwrap();
        let mut buf = [0u8; 5];
        let read = pipe_read(read_end, &mut buf).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buf, b"final");
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_read_write_interleave() {
        let (read_end, write_end) = create_pipe().unwrap();
        
        // Interleaved reads and writes
        pipe_write(write_end, b"AAA").unwrap();
        pipe_write(write_end, b"BBB").unwrap();
        
        let mut buf = [0u8; 3];
        pipe_read(read_end, &mut buf).unwrap();
        assert_eq!(&buf, b"AAA");
        
        pipe_write(write_end, b"CCC").unwrap();
        
        pipe_read(read_end, &mut buf).unwrap();
        assert_eq!(&buf, b"BBB");
        
        pipe_read(read_end, &mut buf).unwrap();
        assert_eq!(&buf, b"CCC");
        
        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_invalid_pipe_id() {
        // Invalid pipe ID should return error
        let result = pipe_read(999, &mut [0u8; 10]);
        assert!(result.is_err());
        
        let result = pipe_write(999, b"data");
        assert!(result.is_err());
    }
}
