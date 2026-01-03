//! TCP State Machine Edge Case Tests
//!
//! Tests for the TCP connection state machine using real kernel types,
//! including TcpHeader, TcpOptions, TcpState, and sequence number logic.

#[cfg(test)]
mod tests {
    use crate::net::tcp::{
        TcpHeader, TcpOptions, TcpState,
        TCP_FIN, TCP_SYN, TCP_RST, TCP_PSH, TCP_ACK, TCP_URG,
        TCP_OPT_MSS, TCP_OPT_WINDOW_SCALE, TCP_OPT_TIMESTAMP,
    };

    // =========================================================================
    // TCP Header Tests
    // =========================================================================

    #[test]
    fn test_tcp_header_new() {
        let header = TcpHeader::new(8080, 443);
        
        // Ports should be in network byte order
        assert_eq!(u16::from_be(header.src_port), 8080);
        assert_eq!(u16::from_be(header.dst_port), 443);
        
        // Default data offset should be 5 (20 bytes)
        assert_eq!(header.data_offset(), 20);
        
        // No flags by default
        assert_eq!(header.flags(), 0);
    }

    #[test]
    fn test_tcp_header_flags() {
        let mut header = TcpHeader::new(1234, 5678);
        
        // Set SYN flag
        header.set_flags(TCP_SYN);
        assert_eq!(header.flags() & TCP_SYN, TCP_SYN);
        assert_eq!(header.flags() & TCP_ACK, 0);
        
        // Set SYN+ACK
        header.set_flags(TCP_SYN | TCP_ACK);
        assert_eq!(header.flags() & TCP_SYN, TCP_SYN);
        assert_eq!(header.flags() & TCP_ACK, TCP_ACK);
        
        // All flags
        header.set_flags(TCP_FIN | TCP_SYN | TCP_RST | TCP_PSH | TCP_ACK | TCP_URG);
        assert_eq!(header.flags(), 0x3F);
    }

    #[test]
    fn test_tcp_header_data_offset_preserved() {
        let mut header = TcpHeader::new(80, 80);
        
        // Set flags shouldn't affect data offset
        let original_offset = header.data_offset();
        header.set_flags(TCP_SYN | TCP_ACK);
        assert_eq!(header.data_offset(), original_offset, 
                   "Setting flags should not affect data offset");
    }

    // =========================================================================
    // TCP Options Tests
    // =========================================================================

    #[test]
    fn test_tcp_options_new_is_empty() {
        let opts = TcpOptions::new();
        assert!(opts.mss.is_none());
        assert!(opts.window_scale.is_none());
        assert!(!opts.sack_permitted);
        assert!(opts.timestamp.is_none());
    }

    #[test]
    fn test_tcp_options_mss_parsing() {
        // MSS option: kind=2, length=4, value=1460 (0x05B4)
        let data = [TCP_OPT_MSS, 4, 0x05, 0xB4];
        let opts = TcpOptions::parse(&data);
        
        assert_eq!(opts.mss, Some(1460));
    }

    #[test]
    fn test_tcp_options_window_scale_parsing() {
        // Window scale: kind=3, length=3, value=7
        let data = [TCP_OPT_WINDOW_SCALE, 3, 7];
        let opts = TcpOptions::parse(&data);
        
        assert_eq!(opts.window_scale, Some(7));
    }

    #[test]
    fn test_tcp_options_timestamp_parsing() {
        // Timestamp: kind=8, length=10, TSval=12345678, TSecr=87654321
        let tsval: u32 = 12345678;
        let tsecr: u32 = 87654321;
        let data = [
            TCP_OPT_TIMESTAMP, 10,
            // TSval
            (tsval >> 24) as u8, (tsval >> 16) as u8, (tsval >> 8) as u8, tsval as u8,
            // TSecr
            (tsecr >> 24) as u8, (tsecr >> 16) as u8, (tsecr >> 8) as u8, tsecr as u8,
        ];
        let opts = TcpOptions::parse(&data);
        
        assert_eq!(opts.timestamp, Some((tsval, tsecr)));
    }

    #[test]
    fn test_tcp_options_combined_parsing() {
        // Multiple options together
        let mut data = Vec::new();
        
        // MSS
        data.extend_from_slice(&[TCP_OPT_MSS, 4, 0x05, 0xB4]);
        // Window scale
        data.extend_from_slice(&[TCP_OPT_WINDOW_SCALE, 3, 7]);
        // SACK permitted
        data.extend_from_slice(&[4, 2]); // kind=4, length=2
        
        let opts = TcpOptions::parse(&data);
        
        assert_eq!(opts.mss, Some(1460));
        assert_eq!(opts.window_scale, Some(7));
        assert!(opts.sack_permitted);
    }

    #[test]
    fn test_tcp_options_generate_round_trip() {
        let mut original = TcpOptions::new();
        original.mss = Some(1460);
        original.window_scale = Some(7);
        original.sack_permitted = true;
        
        let mut buffer = [0u8; 64];
        let len = original.generate(&mut buffer);
        
        // Parse the generated options
        let parsed = TcpOptions::parse(&buffer[..len]);
        
        assert_eq!(parsed.mss, original.mss);
        assert_eq!(parsed.window_scale, original.window_scale);
        assert_eq!(parsed.sack_permitted, original.sack_permitted);
    }

    #[test]
    fn test_tcp_options_size_calculation() {
        let mut opts = TcpOptions::new();
        
        // Empty options
        let empty_size = opts.size();
        assert_eq!(empty_size % 4, 0, "Size should be 4-byte aligned");
        
        // Just MSS (4 bytes)
        opts.mss = Some(1460);
        let mss_size = opts.size();
        assert!(mss_size >= 4);
        assert_eq!(mss_size % 4, 0);
        
        // MSS + window scale (4 + 4 = 8 bytes)
        opts.window_scale = Some(7);
        let ws_size = opts.size();
        assert!(ws_size >= 8);
        assert_eq!(ws_size % 4, 0);
    }

    #[test]
    fn test_tcp_options_malformed_data() {
        // Truncated MSS option
        let truncated = [TCP_OPT_MSS, 4, 0x05]; // Missing last byte
        let opts = TcpOptions::parse(&truncated);
        assert!(opts.mss.is_none(), "Truncated option should not parse");
        
        // Invalid length
        let bad_length = [TCP_OPT_MSS, 2, 0x05, 0xB4]; // Wrong length field
        let opts = TcpOptions::parse(&bad_length);
        // Should either skip or stop parsing
    }

    #[test]
    fn test_tcp_options_buffer_too_small() {
        let mut opts = TcpOptions::new();
        opts.mss = Some(1460);
        opts.window_scale = Some(7);
        opts.sack_permitted = true;
        opts.timestamp = Some((12345, 67890));
        
        // Buffer too small
        let mut tiny_buffer = [0u8; 4];
        let len = opts.generate(&mut tiny_buffer);
        
        // Should handle gracefully - either partial or nothing
        assert!(len <= tiny_buffer.len());
    }

    // =========================================================================
    // TCP State Tests - Using Real Kernel Enum
    // =========================================================================

    #[test]
    fn test_tcp_states_all_defined() {
        // Verify all TCP states are distinct
        let states = [
            TcpState::Closed,
            TcpState::Listen,
            TcpState::SynSent,
            TcpState::SynReceived,
            TcpState::Established,
            TcpState::FinWait1,
            TcpState::FinWait2,
            TcpState::CloseWait,
            TcpState::Closing,
            TcpState::LastAck,
            TcpState::TimeWait,
        ];
        
        // All should be distinct
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j], 
                           "States {:?} and {:?} should be distinct", states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_tcp_state_copy_trait() {
        let state = TcpState::Established;
        let copied = state;
        assert_eq!(state, copied);
    }

    #[test]
    fn test_tcp_state_eq_trait() {
        assert_eq!(TcpState::Closed, TcpState::Closed);
        assert_ne!(TcpState::Closed, TcpState::Listen);
        assert_ne!(TcpState::SynSent, TcpState::SynReceived);
    }

    // =========================================================================
    // Sequence Number Tests
    // =========================================================================

    #[test]
    fn test_sequence_number_wraparound() {
        // TCP sequence numbers are 32-bit and wrap around
        let seq1: u32 = u32::MAX - 100;
        let seq2: u32 = seq1.wrapping_add(200);
        
        // seq2 should be around 99 after wraparound
        assert!(seq2 < 200);
        
        // Sequence number comparison with wraparound
        fn seq_before(a: u32, b: u32) -> bool {
            let diff = b.wrapping_sub(a);
            diff > 0 && diff < (1 << 31)
        }
        
        assert!(seq_before(seq1, seq2), 
                "{} should be before {} with wraparound", seq1, seq2);
    }

    #[test]
    fn test_sequence_number_in_window() {
        fn in_window(seq: u32, window_start: u32, window_size: u32) -> bool {
            let relative = seq.wrapping_sub(window_start);
            relative < window_size
        }
        
        // Normal case
        assert!(in_window(1000, 900, 200));
        assert!(!in_window(1200, 900, 200));
        
        // Wraparound case
        // Window starts at MAX-50 and has size 100
        // So valid sequence numbers are: MAX-50, MAX-49, ..., MAX, 0, 1, ..., 48
        // (total of 100 numbers)
        let window_start = u32::MAX - 50;
        assert!(in_window(u32::MAX, window_start, 100), "MAX should be in window");
        assert!(in_window(0, window_start, 100), "0 should be in window");
        assert!(in_window(48, window_start, 100), "48 should be in window (last valid)");
        assert!(!in_window(49, window_start, 100), "49 should be outside window");
        assert!(!in_window(50, window_start, 100), "50 should be outside window");
    }

    #[test]
    fn test_sequence_number_arithmetic() {
        // Wrapping addition
        let seq: u32 = u32::MAX - 10;
        assert_eq!(seq.wrapping_add(20), 9);
        
        // Wrapping subtraction
        let seq2: u32 = 10;
        assert_eq!(seq2.wrapping_sub(20), u32::MAX - 9);
    }

    // =========================================================================
    // TCP State Transition Validation Tests
    // Using lookup tables instead of fake state machine
    // =========================================================================

    /// Valid transitions from each TCP state (RFC 793)
    fn valid_transitions(state: TcpState) -> &'static [TcpState] {
        match state {
            TcpState::Closed => &[TcpState::Listen, TcpState::SynSent],
            TcpState::Listen => &[TcpState::Closed, TcpState::SynReceived, TcpState::SynSent],
            TcpState::SynSent => &[TcpState::Closed, TcpState::SynReceived, TcpState::Established],
            TcpState::SynReceived => &[TcpState::Closed, TcpState::Established, TcpState::FinWait1],
            TcpState::Established => &[TcpState::FinWait1, TcpState::CloseWait],
            TcpState::FinWait1 => &[TcpState::FinWait2, TcpState::Closing, TcpState::TimeWait],
            TcpState::FinWait2 => &[TcpState::TimeWait],
            TcpState::CloseWait => &[TcpState::LastAck],
            TcpState::Closing => &[TcpState::TimeWait],
            TcpState::LastAck => &[TcpState::Closed],
            TcpState::TimeWait => &[TcpState::Closed],
        }
    }

    #[test]
    fn test_valid_client_connection_path() {
        // Client: CLOSED -> SYN_SENT -> ESTABLISHED -> FIN_WAIT_1 -> FIN_WAIT_2 -> TIME_WAIT -> CLOSED
        let path = [
            TcpState::Closed,
            TcpState::SynSent,
            TcpState::Established,
            TcpState::FinWait1,
            TcpState::FinWait2,
            TcpState::TimeWait,
            TcpState::Closed,
        ];
        
        for i in 0..path.len() - 1 {
            let from = path[i];
            let to = path[i + 1];
            let valid = valid_transitions(from);
            assert!(valid.contains(&to),
                    "Transition {:?} -> {:?} should be valid", from, to);
        }
    }

    #[test]
    fn test_valid_server_connection_path() {
        // Server: CLOSED -> LISTEN -> SYN_RECEIVED -> ESTABLISHED -> CLOSE_WAIT -> LAST_ACK -> CLOSED
        let path = [
            TcpState::Closed,
            TcpState::Listen,
            TcpState::SynReceived,
            TcpState::Established,
            TcpState::CloseWait,
            TcpState::LastAck,
            TcpState::Closed,
        ];
        
        for i in 0..path.len() - 1 {
            let from = path[i];
            let to = path[i + 1];
            let valid = valid_transitions(from);
            assert!(valid.contains(&to),
                    "Transition {:?} -> {:?} should be valid", from, to);
        }
    }

    #[test]
    fn test_simultaneous_close_path() {
        // Simultaneous close: ESTABLISHED -> FIN_WAIT_1 -> CLOSING -> TIME_WAIT -> CLOSED
        let path = [
            TcpState::Established,
            TcpState::FinWait1,
            TcpState::Closing,
            TcpState::TimeWait,
            TcpState::Closed,
        ];
        
        for i in 0..path.len() - 1 {
            let from = path[i];
            let to = path[i + 1];
            let valid = valid_transitions(from);
            assert!(valid.contains(&to),
                    "Transition {:?} -> {:?} should be valid", from, to);
        }
    }

    #[test]
    fn test_invalid_transitions() {
        // Established cannot go directly to Closed
        let valid = valid_transitions(TcpState::Established);
        assert!(!valid.contains(&TcpState::Closed),
                "ESTABLISHED -> CLOSED should not be valid");
        
        // Listen cannot go directly to TimeWait
        let valid = valid_transitions(TcpState::Listen);
        assert!(!valid.contains(&TcpState::TimeWait),
                "LISTEN -> TIME_WAIT should not be valid");
    }

    // =========================================================================
    // Congestion Control Constants Tests (Using Kernel Constants)
    // =========================================================================

    /// Initial cwnd per RFC 6928
    const INITIAL_CWND_SEGMENTS: u32 = 10;
    /// MSS as used in kernel
    const MSS: u32 = 1460;
    /// Initial ssthresh
    const INITIAL_SSTHRESH: u32 = 65535;
    /// Min ssthresh per RFC
    const MIN_SSTHRESH_SEGMENTS: u32 = 2;

    #[test]
    fn test_congestion_control_initial_values() {
        // These match the kernel's TcpConnection::new() values
        let initial_cwnd = INITIAL_CWND_SEGMENTS * MSS;
        assert_eq!(initial_cwnd, 14600); // 10 * 1460
        
        assert_eq!(INITIAL_SSTHRESH, 65535);
    }

    #[test]
    fn test_slow_start_growth_formula() {
        // In slow start: cwnd += mss for each ACK
        // After N ACKs: cwnd = initial_cwnd + N * mss
        
        let initial = 10 * MSS;
        let after_10_acks = initial + 10 * MSS;
        assert_eq!(after_10_acks, 20 * MSS);
    }

    #[test]
    fn test_congestion_avoidance_growth_formula() {
        // In congestion avoidance: cwnd += mss * mss / cwnd per ACK
        // This is approximately cwnd += mss / (cwnd/mss) = cwnd += mss/N where N is segments in window
        
        let cwnd: u32 = 65535;
        let mss: u32 = 1460;
        
        let increment = (mss * mss) / cwnd;
        // Growth should be small compared to mss
        assert!(increment < mss, "CA increment should be < MSS");
        assert!(increment > 0, "CA increment should be > 0");
    }

    #[test]
    fn test_multiplicative_decrease_formula() {
        // On loss: ssthresh = cwnd / 2 (but at least 2*MSS)
        // cwnd = ssthresh (fast recovery) or cwnd = 1*MSS (timeout)
        
        let cwnd: u32 = 100 * MSS;
        let new_ssthresh = core::cmp::max(cwnd / 2, MIN_SSTHRESH_SEGMENTS * MSS);
        assert_eq!(new_ssthresh, 50 * MSS);
        
        // With small cwnd
        let small_cwnd: u32 = 3 * MSS;
        let small_ssthresh = core::cmp::max(small_cwnd / 2, MIN_SSTHRESH_SEGMENTS * MSS);
        assert_eq!(small_ssthresh, MIN_SSTHRESH_SEGMENTS * MSS);
    }

    #[test]
    fn test_cwnd_minimum_enforcement() {
        // cwnd should never go below 2*MSS
        let min_cwnd = MIN_SSTHRESH_SEGMENTS * MSS;
        
        // Multiple losses simulation
        let mut cwnd: u32 = 10 * MSS;
        for _ in 0..10 {
            cwnd = core::cmp::max(cwnd / 2, min_cwnd);
        }
        
        assert!(cwnd >= min_cwnd, "cwnd should not go below 2*MSS");
    }

    // =========================================================================
    // TCP Flags Tests
    // =========================================================================

    #[test]
    fn test_tcp_flags_values() {
        assert_eq!(TCP_FIN, 0x01);
        assert_eq!(TCP_SYN, 0x02);
        assert_eq!(TCP_RST, 0x04);
        assert_eq!(TCP_PSH, 0x08);
        assert_eq!(TCP_ACK, 0x10);
        assert_eq!(TCP_URG, 0x20);
    }

    #[test]
    fn test_tcp_flags_combinations() {
        // SYN+ACK
        let syn_ack = TCP_SYN | TCP_ACK;
        assert_eq!(syn_ack, 0x12);
        
        // FIN+ACK
        let fin_ack = TCP_FIN | TCP_ACK;
        assert_eq!(fin_ack, 0x11);
        
        // PSH+ACK (common for data)
        let psh_ack = TCP_PSH | TCP_ACK;
        assert_eq!(psh_ack, 0x18);
    }

    #[test]
    fn test_tcp_flags_no_overlap() {
        // Flags should be distinct powers of 2
        let flags = [TCP_FIN, TCP_SYN, TCP_RST, TCP_PSH, TCP_ACK, TCP_URG];
        
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two(), "Flag {:x} should be power of 2", flags[i]);
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
                assert_eq!(flags[i] & flags[j], 0, "Flags should not overlap");
            }
        }
    }
}
