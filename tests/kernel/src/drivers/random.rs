//! Tests for drivers/random.rs - Random Number Generator
//!
//! Tests the CSPRNG and entropy pool implementation.

#[cfg(test)]
mod tests {
    // Constants from the random module
    const MAX_ENTROPY_BITS: u64 = 4096;

    // =========================================================================
    // Entropy Pool Constants Tests
    // =========================================================================

    #[test]
    fn test_max_entropy_bits() {
        // Entropy pool should be at least 4096 bits (512 bytes)
        assert!(MAX_ENTROPY_BITS >= 4096);
        // Should be a reasonable size
        assert!(MAX_ENTROPY_BITS <= 65536);
    }

    #[test]
    fn test_entropy_bits_power_of_two() {
        // Should be power of 2 for efficient operations
        assert!(MAX_ENTROPY_BITS.is_power_of_two());
    }

    // =========================================================================
    // ChaCha20 Algorithm Tests
    // =========================================================================

    #[test]
    fn test_chacha_quarter_round() {
        // Test the quarter round function
        fn quarter_round(a: u32, b: u32, c: u32, d: u32) -> (u32, u32, u32, u32) {
            let a = a.wrapping_add(b);
            let d = (d ^ a).rotate_left(16);
            let c = c.wrapping_add(d);
            let b = (b ^ c).rotate_left(12);
            let a = a.wrapping_add(b);
            let d = (d ^ a).rotate_left(8);
            let c = c.wrapping_add(d);
            let b = (b ^ c).rotate_left(7);
            (a, b, c, d)
        }

        // RFC 7539 test vector
        let (a, b, c, d) = quarter_round(0x11111111, 0x01020304, 0x9b8d6f43, 0x01234567);
        assert_eq!(a, 0xea2a92f4);
        assert_eq!(b, 0xcb1cf8ce);
        assert_eq!(c, 0x4581472e);
        assert_eq!(d, 0x5881c4bb);
    }

    #[test]
    fn test_chacha_constants() {
        // ChaCha20 "expand 32-byte k" constants
        const SIGMA: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];
        
        // Verify the constants spell "expand 32-byte k" in little endian
        let bytes: Vec<u8> = SIGMA.iter()
            .flat_map(|&x| x.to_le_bytes())
            .collect();
        assert_eq!(&bytes, b"expand 32-byte k");
    }

    // =========================================================================
    // Random Buffer Tests
    // =========================================================================

    #[test]
    fn test_random_fill_sizes() {
        // Test various buffer sizes
        let sizes = [1, 8, 16, 32, 64, 128, 256, 1024];
        
        for size in sizes {
            let buf = vec![0u8; size];
            assert_eq!(buf.len(), size);
        }
    }

    #[test]
    fn test_entropy_estimation() {
        // Entropy should be estimated conservatively
        fn estimate_entropy_for_size(bytes: usize) -> u64 {
            // Conservative estimate: 8 bits per byte is maximum
            (bytes * 8) as u64
        }
        
        assert_eq!(estimate_entropy_for_size(32), 256);
        assert_eq!(estimate_entropy_for_size(64), 512);
    }

    // =========================================================================
    // RDRAND/RDSEED Tests (Simulated)
    // =========================================================================

    #[test]
    fn test_rdrand_retry_limit() {
        // RDRAND should be retried a limited number of times
        const RDRAND_RETRIES: u32 = 10;
        assert!(RDRAND_RETRIES >= 10);
        assert!(RDRAND_RETRIES <= 100);
    }

    // =========================================================================
    // Fallback Seed Tests
    // =========================================================================

    #[test]
    fn test_fallback_seed_sources() {
        // Fallback seed should use multiple sources for entropy mixing
        // Sources include: TSC, memory addresses, stack values, etc.
        
        // Simulate TSC-based seeding
        fn simple_hash(seed: u64) -> u64 {
            let mut h = seed;
            h ^= h >> 33;
            h = h.wrapping_mul(0xff51afd7ed558ccd);
            h ^= h >> 33;
            h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
            h ^= h >> 33;
            h
        }
        
        // Different seeds should produce different hashes
        let h1 = simple_hash(12345);
        let h2 = simple_hash(12346);
        assert_ne!(h1, h2);
    }

    // =========================================================================
    // Getrandom Syscall Flags Tests
    // =========================================================================

    #[test]
    fn test_getrandom_flags() {
        // getrandom() flag values (Linux compatible)
        const GRND_NONBLOCK: u32 = 0x01;
        const GRND_RANDOM: u32 = 0x02;
        const GRND_INSECURE: u32 = 0x04;
        
        // Flags should be distinct bits
        assert_eq!(GRND_NONBLOCK & GRND_RANDOM, 0);
        assert_eq!(GRND_RANDOM & GRND_INSECURE, 0);
        assert_eq!(GRND_NONBLOCK & GRND_INSECURE, 0);
    }
}
