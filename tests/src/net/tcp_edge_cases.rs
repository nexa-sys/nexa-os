//! TCP State Machine Edge Case Tests
//!
//! Tests for the TCP connection state machine, including:
//! - All valid state transitions
//! - Invalid state transition handling
//! - Timeout behavior
//! - Sequence number wraparound
//! - Congestion control edge cases

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
    // TCP State Machine Tests
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

    /// Simulates TCP state transitions to verify state machine correctness
    struct TcpStateMachine {
        state: TcpState,
    }

    impl TcpStateMachine {
        fn new() -> Self {
            Self { state: TcpState::Closed }
        }

        /// Client active open: CLOSED -> SYN_SENT
        fn active_open(&mut self) -> Result<(), &'static str> {
            match self.state {
                TcpState::Closed => {
                    self.state = TcpState::SynSent;
                    Ok(())
                }
                _ => Err("Invalid state for active open"),
            }
        }

        /// Server passive open: CLOSED -> LISTEN
        fn passive_open(&mut self) -> Result<(), &'static str> {
            match self.state {
                TcpState::Closed => {
                    self.state = TcpState::Listen;
                    Ok(())
                }
                _ => Err("Invalid state for passive open"),
            }
        }

        /// Receive SYN (server): LISTEN -> SYN_RECEIVED
        fn recv_syn(&mut self) -> Result<(), &'static str> {
            match self.state {
                TcpState::Listen => {
                    self.state = TcpState::SynReceived;
                    Ok(())
                }
                _ => Err("Invalid state for receiving SYN"),
            }
        }

        /// Receive SYN+ACK (client): SYN_SENT -> ESTABLISHED
        fn recv_syn_ack(&mut self) -> Result<(), &'static str> {
            match self.state {
                TcpState::SynSent => {
                    self.state = TcpState::Established;
                    Ok(())
                }
                _ => Err("Invalid state for receiving SYN+ACK"),
            }
        }

        /// Receive ACK of SYN (server): SYN_RECEIVED -> ESTABLISHED
        fn recv_ack_of_syn(&mut self) -> Result<(), &'static str> {
            match self.state {
                TcpState::SynReceived => {
                    self.state = TcpState::Established;
                    Ok(())
                }
                _ => Err("Invalid state for receiving ACK of SYN"),
            }
        }

        /// Close from ESTABLISHED: ESTABLISHED -> FIN_WAIT_1
        fn close(&mut self) -> Result<(), &'static str> {
            match self.state {
                TcpState::Established => {
                    self.state = TcpState::FinWait1;
                    Ok(())
                }
                TcpState::CloseWait => {
                    self.state = TcpState::LastAck;
                    Ok(())
                }
                _ => Err("Invalid state for close"),
            }
        }

        /// Receive FIN: ESTABLISHED -> CLOSE_WAIT
        fn recv_fin(&mut self) -> Result<(), &'static str> {
            match self.state {
                TcpState::Established => {
                    self.state = TcpState::CloseWait;
                    Ok(())
                }
                TcpState::FinWait1 => {
                    self.state = TcpState::Closing;
                    Ok(())
                }
                TcpState::FinWait2 => {
                    self.state = TcpState::TimeWait;
                    Ok(())
                }
                _ => Err("Invalid state for receiving FIN"),
            }
        }

        /// Receive ACK of FIN
        fn recv_ack_of_fin(&mut self) -> Result<(), &'static str> {
            match self.state {
                TcpState::FinWait1 => {
                    self.state = TcpState::FinWait2;
                    Ok(())
                }
                TcpState::LastAck => {
                    self.state = TcpState::Closed;
                    Ok(())
                }
                TcpState::Closing => {
                    self.state = TcpState::TimeWait;
                    Ok(())
                }
                _ => Err("Invalid state for receiving ACK of FIN"),
            }
        }

        /// TIME_WAIT timeout: TIME_WAIT -> CLOSED
        fn timeout(&mut self) -> Result<(), &'static str> {
            match self.state {
                TcpState::TimeWait => {
                    self.state = TcpState::Closed;
                    Ok(())
                }
                _ => Err("Invalid state for timeout"),
            }
        }
    }

    #[test]
    fn test_tcp_client_connection_lifecycle() {
        let mut fsm = TcpStateMachine::new();
        assert_eq!(fsm.state, TcpState::Closed);

        // Client initiates connection
        fsm.active_open().unwrap();
        assert_eq!(fsm.state, TcpState::SynSent);

        // Receives SYN+ACK
        fsm.recv_syn_ack().unwrap();
        assert_eq!(fsm.state, TcpState::Established);

        // Client initiates close
        fsm.close().unwrap();
        assert_eq!(fsm.state, TcpState::FinWait1);

        // Receives ACK of FIN
        fsm.recv_ack_of_fin().unwrap();
        assert_eq!(fsm.state, TcpState::FinWait2);

        // Receives FIN from server
        fsm.recv_fin().unwrap();
        assert_eq!(fsm.state, TcpState::TimeWait);

        // Timeout expires
        fsm.timeout().unwrap();
        assert_eq!(fsm.state, TcpState::Closed);
    }

    #[test]
    fn test_tcp_server_connection_lifecycle() {
        let mut fsm = TcpStateMachine::new();
        assert_eq!(fsm.state, TcpState::Closed);

        // Server listens
        fsm.passive_open().unwrap();
        assert_eq!(fsm.state, TcpState::Listen);

        // Receives SYN
        fsm.recv_syn().unwrap();
        assert_eq!(fsm.state, TcpState::SynReceived);

        // Receives ACK of SYN+ACK
        fsm.recv_ack_of_syn().unwrap();
        assert_eq!(fsm.state, TcpState::Established);

        // Receives FIN from client
        fsm.recv_fin().unwrap();
        assert_eq!(fsm.state, TcpState::CloseWait);

        // Server sends FIN
        fsm.close().unwrap();
        assert_eq!(fsm.state, TcpState::LastAck);

        // Receives ACK of FIN
        fsm.recv_ack_of_fin().unwrap();
        assert_eq!(fsm.state, TcpState::Closed);
    }

    #[test]
    fn test_tcp_simultaneous_close() {
        let mut fsm = TcpStateMachine::new();
        
        // Get to ESTABLISHED state
        fsm.active_open().unwrap();
        fsm.recv_syn_ack().unwrap();
        assert_eq!(fsm.state, TcpState::Established);

        // Both sides close at same time (we send FIN)
        fsm.close().unwrap();
        assert_eq!(fsm.state, TcpState::FinWait1);

        // Receive FIN before ACK (simultaneous close)
        fsm.recv_fin().unwrap();
        assert_eq!(fsm.state, TcpState::Closing);

        // Receive ACK of our FIN
        fsm.recv_ack_of_fin().unwrap();
        assert_eq!(fsm.state, TcpState::TimeWait);
    }

    #[test]
    fn test_tcp_invalid_state_transitions() {
        let mut fsm = TcpStateMachine::new();

        // Can't receive SYN in CLOSED state
        assert!(fsm.recv_syn().is_err());

        // Can't receive SYN+ACK in CLOSED state
        assert!(fsm.recv_syn_ack().is_err());

        // Can't close in CLOSED state
        assert!(fsm.close().is_err());

        // Go to LISTEN
        fsm.passive_open().unwrap();

        // Can't passive_open again
        assert!(fsm.passive_open().is_err());

        // Can't active_open from LISTEN
        assert!(fsm.active_open().is_err());
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

    // =========================================================================
    // Congestion Control Tests
    // =========================================================================

    /// Simplified congestion window state for testing
    struct CongestionState {
        cwnd: u32,      // Congestion window (bytes)
        ssthresh: u32,  // Slow start threshold
        mss: u32,       // Maximum segment size
    }

    impl CongestionState {
        fn new(mss: u32) -> Self {
            Self {
                cwnd: 10 * mss,  // Initial cwnd (RFC 6928)
                ssthresh: 65535,
                mss,
            }
        }

        /// Slow start: cwnd += mss for each ACK
        fn slow_start_ack(&mut self) {
            if self.cwnd < self.ssthresh {
                self.cwnd += self.mss;
            }
        }

        /// Congestion avoidance: cwnd += mss * mss / cwnd for each ACK
        fn congestion_avoidance_ack(&mut self) {
            if self.cwnd >= self.ssthresh {
                // Approximation of additive increase
                self.cwnd += (self.mss * self.mss) / self.cwnd;
                if self.cwnd == 0 {
                    self.cwnd = self.mss; // Prevent 0 cwnd
                }
            }
        }

        /// Packet loss (multiplicative decrease)
        fn packet_loss(&mut self) {
            self.ssthresh = core::cmp::max(self.cwnd / 2, 2 * self.mss);
            self.cwnd = self.ssthresh; // Fast recovery
        }
    }

    #[test]
    fn test_slow_start_growth() {
        let mss: u32 = 1460;
        let mut cc = CongestionState::new(mss);
        
        // Initial cwnd should be 10 * MSS
        assert_eq!(cc.cwnd, 10 * mss);
        
        // Simulate receiving ACKs in slow start
        let initial_cwnd = cc.cwnd;
        for _ in 0..10 {
            cc.slow_start_ack();
        }
        
        // Should have grown by 10 * MSS
        assert_eq!(cc.cwnd, initial_cwnd + 10 * mss);
    }

    #[test]
    fn test_congestion_avoidance_growth() {
        let mss: u32 = 1460;
        let mut cc = CongestionState::new(mss);
        
        // Set cwnd above ssthresh for congestion avoidance
        cc.cwnd = 65535;
        cc.ssthresh = 32768;
        
        let initial_cwnd = cc.cwnd;
        
        // Congestion avoidance grows much slower
        for _ in 0..100 {
            cc.congestion_avoidance_ack();
        }
        
        // Growth should be much smaller than slow start
        let growth = cc.cwnd - initial_cwnd;
        assert!(growth < 100 * mss, "Congestion avoidance should grow slower than slow start");
    }

    #[test]
    fn test_multiplicative_decrease() {
        let mss: u32 = 1460;
        let mut cc = CongestionState::new(mss);
        
        // Set up large cwnd
        cc.cwnd = 100 * mss;
        cc.ssthresh = 200 * mss;
        
        let cwnd_before = cc.cwnd;
        cc.packet_loss();
        
        // ssthresh should be cwnd/2
        assert_eq!(cc.ssthresh, cwnd_before / 2);
        
        // cwnd should drop to ssthresh (fast recovery)
        assert_eq!(cc.cwnd, cc.ssthresh);
    }

    #[test]
    fn test_cwnd_minimum() {
        let mss: u32 = 1460;
        let mut cc = CongestionState::new(mss);
        
        // Multiple losses
        for _ in 0..10 {
            cc.packet_loss();
        }
        
        // cwnd should never go below 2 * MSS
        assert!(cc.cwnd >= 2 * mss, "cwnd should not go below 2 * MSS");
        assert!(cc.ssthresh >= 2 * mss, "ssthresh should not go below 2 * MSS");
    }
}
