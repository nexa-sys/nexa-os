//! Socketpair Tests
//!
//! Tests for bidirectional socketpair communication using real kernel functions.

#[cfg(test)]
mod tests {
    use crate::pipe::{
        close_socketpair_end, create_socketpair, socketpair_has_data, socketpair_read,
        socketpair_write,
    };

    // =========================================================================
    // Socketpair Basic Operations - Using Real Kernel Functions
    // =========================================================================

    #[test]
    fn test_socketpair_creation() {
        // Create a real socketpair using kernel function
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Verify the socketpair is usable by checking if it has data (should be false initially)
        let has_data_0 = socketpair_has_data(pair_id, 0).expect("Failed to check end 0");
        let has_data_1 = socketpair_has_data(pair_id, 1).expect("Failed to check end 1");

        assert!(!has_data_0, "End 0 should have no data initially");
        assert!(!has_data_1, "End 1 should have no data initially");

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_bidirectional_write_read() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Write from end 0, should be readable from end 1
        let data = b"hello from end 0";
        let written = socketpair_write(pair_id, 0, data).expect("Write from end 0 failed");
        assert_eq!(written, data.len());

        // Verify end 1 has data to read
        let has_data = socketpair_has_data(pair_id, 1).expect("Check failed");
        assert!(has_data, "End 1 should have data after end 0 writes");

        // Read from end 1
        let mut buf = [0u8; 32];
        let read = socketpair_read(pair_id, 1, &mut buf).expect("Read from end 1 failed");
        assert_eq!(read, data.len());
        assert_eq!(&buf[..read], data);

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_reverse_direction() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Write from end 1, should be readable from end 0
        let data = b"hello from end 1";
        let written = socketpair_write(pair_id, 1, data).expect("Write from end 1 failed");
        assert_eq!(written, data.len());

        // Verify end 0 has data to read
        let has_data = socketpair_has_data(pair_id, 0).expect("Check failed");
        assert!(has_data, "End 0 should have data after end 1 writes");

        // Read from end 0
        let mut buf = [0u8; 32];
        let read = socketpair_read(pair_id, 0, &mut buf).expect("Read from end 0 failed");
        assert_eq!(read, data.len());
        assert_eq!(&buf[..read], data);

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_buffer_independence() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Write different data from each end simultaneously
        let data_0 = b"from0";
        let data_1 = b"from1";

        socketpair_write(pair_id, 0, data_0).expect("Write from end 0 failed");
        socketpair_write(pair_id, 1, data_1).expect("Write from end 1 failed");

        // Both ends should have data available
        assert!(socketpair_has_data(pair_id, 0).unwrap());
        assert!(socketpair_has_data(pair_id, 1).unwrap());

        // Each end reads the data written by the other
        let mut buf = [0u8; 16];

        let read_0 = socketpair_read(pair_id, 0, &mut buf).expect("Read at end 0 failed");
        assert_eq!(&buf[..read_0], data_1);

        let read_1 = socketpair_read(pair_id, 1, &mut buf).expect("Read at end 1 failed");
        assert_eq!(&buf[..read_1], data_0);

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_half_close_end_0() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Close end 0
        close_socketpair_end(pair_id, 0).expect("Failed to close end 0");

        // End 0 cannot write anymore
        let result = socketpair_write(pair_id, 0, b"test");
        assert!(result.is_err(), "Closed end 0 should not be able to write");

        // End 1 writing to closed peer should fail (SIGPIPE equivalent)
        let result = socketpair_write(pair_id, 1, b"test");
        assert!(result.is_err(), "Writing to closed peer should fail");

        // Cleanup remaining end
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_half_close_end_1() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Close end 1
        close_socketpair_end(pair_id, 1).expect("Failed to close end 1");

        // End 1 cannot write anymore
        let result = socketpair_write(pair_id, 1, b"test");
        assert!(result.is_err(), "Closed end 1 should not be able to write");

        // End 0 writing to closed peer should fail
        let result = socketpair_write(pair_id, 0, b"test");
        assert!(result.is_err(), "Writing to closed peer should fail");

        // Cleanup remaining end
        let _ = close_socketpair_end(pair_id, 0);
    }

    #[test]
    fn test_socketpair_full_close() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Close both ends
        close_socketpair_end(pair_id, 0).expect("Failed to close end 0");
        close_socketpair_end(pair_id, 1).expect("Failed to close end 1");

        // Neither end can write or read
        assert!(socketpair_write(pair_id, 0, b"test").is_err());
        assert!(socketpair_write(pair_id, 1, b"test").is_err());

        let mut buf = [0u8; 10];
        assert!(socketpair_read(pair_id, 0, &mut buf).is_err());
        assert!(socketpair_read(pair_id, 1, &mut buf).is_err());
    }

    #[test]
    fn test_socketpair_multiple_writes() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Multiple writes from same end accumulate in buffer
        socketpair_write(pair_id, 0, b"hello ").expect("First write failed");
        socketpair_write(pair_id, 0, b"world").expect("Second write failed");

        // Read should get all accumulated data
        let mut buf = [0u8; 32];
        let read = socketpair_read(pair_id, 1, &mut buf).expect("Read failed");
        assert_eq!(&buf[..read], b"hello world");

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_partial_read() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Write some data
        socketpair_write(pair_id, 0, b"abcdefghij").expect("Write failed");

        // Read only 5 bytes
        let mut buf = [0u8; 5];
        let read = socketpair_read(pair_id, 1, &mut buf).expect("Read failed");
        assert_eq!(read, 5);
        assert_eq!(&buf[..5], b"abcde");

        // Read remaining 5 bytes
        let read = socketpair_read(pair_id, 1, &mut buf).expect("Second read failed");
        assert_eq!(read, 5);
        assert_eq!(&buf[..5], b"fghij");

        // No more data
        assert!(!socketpair_has_data(pair_id, 1).unwrap());

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_empty_read() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // No data written yet
        assert!(!socketpair_has_data(pair_id, 0).unwrap());
        assert!(!socketpair_has_data(pair_id, 1).unwrap());

        // Reading from empty buffer returns 0 bytes
        let mut buf = [0u8; 10];
        let read = socketpair_read(pair_id, 0, &mut buf).expect("Read failed");
        assert_eq!(read, 0);

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_concurrent_usage() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Write from both ends in quick succession
        socketpair_write(pair_id, 0, b"ping").expect("Ping failed");
        socketpair_write(pair_id, 1, b"pong").expect("Pong failed");

        let mut buf = [0u8; 10];

        // End 0 receives "pong"
        let read = socketpair_read(pair_id, 0, &mut buf).expect("Read 0 failed");
        assert_eq!(&buf[..read], b"pong");

        // End 1 receives "ping"
        let read = socketpair_read(pair_id, 1, &mut buf).expect("Read 1 failed");
        assert_eq!(&buf[..read], b"ping");

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_invalid_id() {
        // Test with invalid socketpair ID
        let invalid_id = 9999;

        let result = socketpair_write(invalid_id, 0, b"test");
        assert!(result.is_err());

        let mut buf = [0u8; 10];
        let result = socketpair_read(invalid_id, 0, &mut buf);
        assert!(result.is_err());

        let result = socketpair_has_data(invalid_id, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_socketpairs() {
        // Create multiple socketpairs
        let pair1 = create_socketpair().expect("Failed to create pair 1");
        let pair2 = create_socketpair().expect("Failed to create pair 2");

        // Write different data to each
        socketpair_write(pair1, 0, b"pair1").expect("Write to pair1 failed");
        socketpair_write(pair2, 0, b"pair2").expect("Write to pair2 failed");

        let mut buf = [0u8; 10];

        // Verify data is isolated between pairs
        let read = socketpair_read(pair1, 1, &mut buf).expect("Read pair1 failed");
        assert_eq!(&buf[..read], b"pair1");

        let read = socketpair_read(pair2, 1, &mut buf).expect("Read pair2 failed");
        assert_eq!(&buf[..read], b"pair2");

        // Cleanup
        let _ = close_socketpair_end(pair1, 0);
        let _ = close_socketpair_end(pair1, 1);
        let _ = close_socketpair_end(pair2, 0);
        let _ = close_socketpair_end(pair2, 1);
    }

    #[test]
    fn test_socketpair_binary_data() {
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Write binary data including null bytes
        let binary_data: [u8; 10] = [0x00, 0xFF, 0x01, 0xFE, 0x02, 0xFD, 0x03, 0xFC, 0x04, 0xFB];
        socketpair_write(pair_id, 0, &binary_data).expect("Binary write failed");

        // Read and verify
        let mut buf = [0u8; 10];
        let read = socketpair_read(pair_id, 1, &mut buf).expect("Binary read failed");
        assert_eq!(read, 10);
        assert_eq!(buf, binary_data);

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }

    #[test]
    fn test_socketpair_echo_pattern() {
        // Common IPC pattern: send request, receive response
        let pair_id = create_socketpair().expect("Failed to create socketpair");

        // Process A (end 0) sends request
        socketpair_write(pair_id, 0, b"REQUEST").expect("Request failed");

        // Process B (end 1) receives and responds
        let mut buf = [0u8; 16];
        let read = socketpair_read(pair_id, 1, &mut buf).expect("Read request failed");
        assert_eq!(&buf[..read], b"REQUEST");

        socketpair_write(pair_id, 1, b"RESPONSE").expect("Response failed");

        // Process A receives response
        let read = socketpair_read(pair_id, 0, &mut buf).expect("Read response failed");
        assert_eq!(&buf[..read], b"RESPONSE");

        // Cleanup
        let _ = close_socketpair_end(pair_id, 0);
        let _ = close_socketpair_end(pair_id, 1);
    }
}
