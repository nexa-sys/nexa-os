//! Pipe Edge Case Tests
//!
//! Tests for POSIX pipe implementation edge cases and potential bugs.
//! These tests call real kernel pipe functions.

#[cfg(test)]
mod tests {
    use crate::pipe::{close_pipe_read, close_pipe_write, create_pipe, pipe_read, pipe_write};
    use serial_test::serial;

    // =========================================================================
    // Basic Pipe Operations - Using Real Kernel Functions
    // =========================================================================

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

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_large_write_read() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write a larger amount of data
        let data = vec![0xABu8; 2048];
        let written = pipe_write(write_end, &data).expect("Write failed");
        assert!(written > 0, "Should write some bytes");

        // Read all data back
        let mut buffer = vec![0u8; 4096];
        let read = pipe_read(read_end, &mut buffer).expect("Read failed");
        assert_eq!(read, written);
        assert!(buffer[..read].iter().all(|&b| b == 0xAB));

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_circular_buffer_wraparound() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Fill and drain multiple times to force wraparound
        for iteration in 0u8..5 {
            let data = vec![iteration; 1000];
            let written = pipe_write(write_end, &data).expect("Write failed");
            assert_eq!(written, 1000);

            let mut buffer = [0u8; 1000];
            let read = pipe_read(read_end, &mut buffer).expect("Read failed");
            assert_eq!(read, 1000);
            assert!(
                buffer.iter().all(|&b| b == iteration),
                "Data corruption after wraparound iteration {}",
                iteration
            );
        }

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_empty_read() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Read from empty pipe should return 0 (non-blocking) or block
        let mut buffer = [0u8; 64];
        let read = pipe_read(read_end, &mut buffer).expect("Read failed");
        assert_eq!(read, 0, "Empty pipe read should return 0");

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_zero_length_write() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write zero bytes
        let written = pipe_write(write_end, &[]).expect("Zero write failed");
        assert_eq!(written, 0, "Zero-length write should succeed with 0");

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
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
        assert!(
            result.is_err(),
            "Write to pipe with closed read end should fail"
        );

        // Cleanup remaining end
        let _ = close_pipe_write(write_end);
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

        // Cleanup
        let _ = close_pipe_read(read_end);
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
    fn test_pipe_double_close_read() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // First close should succeed
        close_pipe_read(read_end).expect("First close failed");

        // Second close should fail
        let result = close_pipe_read(read_end);
        assert!(result.is_err(), "Double close should fail");

        // Cleanup write end
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_double_close_write() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // First close should succeed
        close_pipe_write(write_end).expect("First close failed");

        // Second close should fail
        let result = close_pipe_write(write_end);
        assert!(result.is_err(), "Double close should fail");

        // Cleanup read end
        let _ = close_pipe_read(read_end);
    }

    // =========================================================================
    // Pipe Resource Management Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_multiple_create_cleanup() {
        // Create several pipes and clean them up properly
        const TEST_PIPES: usize = 8;
        let mut pipes = Vec::new();

        // Create pipes
        for i in 0..TEST_PIPES {
            match create_pipe() {
                Ok(pipe) => pipes.push(pipe),
                Err(e) => {
                    // Resource exhaustion is acceptable
                    eprintln!("Pipe creation limited at {}: {}", i, e);
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

    #[test]
    #[serial]
    fn test_pipe_reuse_after_close() {
        // Create and close a pipe
        let (read_end, write_end) = create_pipe().expect("Failed to create first pipe");
        close_pipe_read(read_end).expect("Close failed");
        close_pipe_write(write_end).expect("Close failed");

        // Create another pipe - should work
        let (read_end2, write_end2) = create_pipe().expect("Failed to create second pipe");

        // Use the new pipe
        pipe_write(write_end2, b"test").expect("Write failed");
        let mut buf = [0u8; 10];
        let read = pipe_read(read_end2, &mut buf).expect("Read failed");
        assert_eq!(&buf[..read], b"test");

        // Cleanup
        let _ = close_pipe_read(read_end2);
        let _ = close_pipe_write(write_end2);
    }

    // =========================================================================
    // Pipe Data Integrity Tests
    // =========================================================================

    #[test]
    #[serial]
    fn test_pipe_data_integrity_checksum() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write data with predictable pattern
        let data: Vec<u8> = (0..256).map(|i| (i & 0xFF) as u8).collect();
        let mut sent_checksum: u32 = 0;
        for &b in &data {
            sent_checksum = sent_checksum.wrapping_add(b as u32);
        }

        let written = pipe_write(write_end, &data).expect("Write failed");
        assert_eq!(written, 256);

        // Read and verify checksum
        let mut buffer = [0u8; 256];
        let read = pipe_read(read_end, &mut buffer).expect("Read failed");
        assert_eq!(read, 256);

        let mut recv_checksum: u32 = 0;
        for &b in &buffer[..read] {
            recv_checksum = recv_checksum.wrapping_add(b as u32);
        }

        assert_eq!(sent_checksum, recv_checksum, "Data corruption detected");

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_sequential_read_preserves_order() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write numbered bytes
        for i in 0u8..10 {
            pipe_write(write_end, &[i]).expect("Write failed");
        }

        // Read and verify order preserved
        let mut buffer = [0u8; 1];
        for expected in 0u8..10 {
            let read = pipe_read(read_end, &mut buffer).expect("Read failed");
            assert_eq!(read, 1);
            assert_eq!(
                buffer[0], expected,
                "Order not preserved at position {}",
                expected
            );
        }

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_binary_data() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write binary data including null bytes and high bytes
        let binary_data: [u8; 8] = [0x00, 0xFF, 0x01, 0xFE, 0x02, 0xFD, 0x03, 0xFC];
        pipe_write(write_end, &binary_data).expect("Binary write failed");

        // Read and verify
        let mut buffer = [0u8; 8];
        let read = pipe_read(read_end, &mut buffer).expect("Binary read failed");
        assert_eq!(read, 8);
        assert_eq!(buffer, binary_data);

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_multiple_write_single_read() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Multiple small writes
        pipe_write(write_end, b"Hello").expect("Write 1 failed");
        pipe_write(write_end, b" ").expect("Write 2 failed");
        pipe_write(write_end, b"World").expect("Write 3 failed");

        // Single read should get all data
        let mut buffer = [0u8; 32];
        let read = pipe_read(read_end, &mut buffer).expect("Read failed");
        assert_eq!(&buffer[..read], b"Hello World");

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }

    #[test]
    #[serial]
    fn test_pipe_partial_read() {
        let (read_end, write_end) = create_pipe().expect("Failed to create pipe");

        // Write some data
        pipe_write(write_end, b"abcdefghij").expect("Write failed");

        // Read only 5 bytes
        let mut buffer = [0u8; 5];
        let read = pipe_read(read_end, &mut buffer).expect("First read failed");
        assert_eq!(read, 5);
        assert_eq!(&buffer[..5], b"abcde");

        // Read remaining 5 bytes
        let read = pipe_read(read_end, &mut buffer).expect("Second read failed");
        assert_eq!(read, 5);
        assert_eq!(&buffer[..5], b"fghij");

        // Cleanup
        let _ = close_pipe_read(read_end);
        let _ = close_pipe_write(write_end);
    }
}
