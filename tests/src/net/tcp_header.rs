//! TCP Header and Options Tests
//!
//! Tests for TCP header structure and option parsing.

#[cfg(test)]
mod tests {
    use crate::net::tcp::{
        TcpHeader, TcpOptions,
        TCP_FIN, TCP_SYN, TCP_RST, TCP_PSH, TCP_ACK, TCP_URG,
        TCP_OPT_END, TCP_OPT_NOP, TCP_OPT_MSS, TCP_OPT_WINDOW_SCALE,
        TCP_OPT_SACK_PERMITTED, TCP_OPT_SACK, TCP_OPT_TIMESTAMP,
    };

    // =========================================================================
    // TCP Flag Constants Tests
    // =========================================================================

    #[test]
    fn test_tcp_fin_flag() {
        assert_eq!(TCP_FIN, 0x01);
    }

    #[test]
    fn test_tcp_syn_flag() {
        assert_eq!(TCP_SYN, 0x02);
    }

    #[test]
    fn test_tcp_rst_flag() {
        assert_eq!(TCP_RST, 0x04);
    }

    #[test]
    fn test_tcp_psh_flag() {
        assert_eq!(TCP_PSH, 0x08);
    }

    #[test]
    fn test_tcp_ack_flag() {
        assert_eq!(TCP_ACK, 0x10);
    }

    #[test]
    fn test_tcp_urg_flag() {
        assert_eq!(TCP_URG, 0x20);
    }

    #[test]
    fn test_tcp_flags_unique() {
        let flags = [TCP_FIN, TCP_SYN, TCP_RST, TCP_PSH, TCP_ACK, TCP_URG];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_tcp_flags_power_of_two() {
        // Each flag should be a power of 2
        assert!(TCP_FIN.is_power_of_two());
        assert!(TCP_SYN.is_power_of_two());
        assert!(TCP_RST.is_power_of_two());
        assert!(TCP_PSH.is_power_of_two());
        assert!(TCP_ACK.is_power_of_two());
        assert!(TCP_URG.is_power_of_two());
    }

    #[test]
    fn test_tcp_syn_ack_combination() {
        let syn_ack = TCP_SYN | TCP_ACK;
        assert_eq!(syn_ack, 0x12);
    }

    #[test]
    fn test_tcp_fin_ack_combination() {
        let fin_ack = TCP_FIN | TCP_ACK;
        assert_eq!(fin_ack, 0x11);
    }

    // =========================================================================
    // TCP Option Kind Constants Tests
    // =========================================================================

    #[test]
    fn test_tcp_opt_end() {
        assert_eq!(TCP_OPT_END, 0);
    }

    #[test]
    fn test_tcp_opt_nop() {
        assert_eq!(TCP_OPT_NOP, 1);
    }

    #[test]
    fn test_tcp_opt_mss() {
        assert_eq!(TCP_OPT_MSS, 2);
    }

    #[test]
    fn test_tcp_opt_window_scale() {
        assert_eq!(TCP_OPT_WINDOW_SCALE, 3);
    }

    #[test]
    fn test_tcp_opt_sack_permitted() {
        assert_eq!(TCP_OPT_SACK_PERMITTED, 4);
    }

    #[test]
    fn test_tcp_opt_sack() {
        assert_eq!(TCP_OPT_SACK, 5);
    }

    #[test]
    fn test_tcp_opt_timestamp() {
        assert_eq!(TCP_OPT_TIMESTAMP, 8);
    }

    // =========================================================================
    // TcpHeader Structure Tests
    // =========================================================================

    #[test]
    fn test_tcp_header_size() {
        let size = core::mem::size_of::<TcpHeader>();
        // TCP header minimum is 20 bytes (5 32-bit words)
        assert_eq!(size, 20);
    }

    #[test]
    fn test_tcp_header_new() {
        let header = TcpHeader::new(8080, 80);
        // Ports are stored in big-endian
        assert_eq!(u16::from_be(header.src_port), 8080);
        assert_eq!(u16::from_be(header.dst_port), 80);
    }

    #[test]
    fn test_tcp_header_data_offset() {
        let header = TcpHeader::new(0, 0);
        // Default data offset is 5 words = 20 bytes
        assert_eq!(header.data_offset(), 20);
    }

    #[test]
    fn test_tcp_header_initial_flags() {
        let header = TcpHeader::new(0, 0);
        // Initial flags should be 0
        assert_eq!(header.flags(), 0);
    }

    #[test]
    fn test_tcp_header_set_flags() {
        let mut header = TcpHeader::new(0, 0);
        header.set_flags(TCP_SYN);
        assert_eq!(header.flags(), TCP_SYN);
    }

    #[test]
    fn test_tcp_header_set_multiple_flags() {
        let mut header = TcpHeader::new(0, 0);
        header.set_flags(TCP_SYN | TCP_ACK);
        let flags = header.flags();
        assert!(flags & TCP_SYN != 0);
        assert!(flags & TCP_ACK != 0);
    }

    // =========================================================================
    // TcpOptions Structure Tests
    // =========================================================================

    #[test]
    fn test_tcp_options_new() {
        let opts = TcpOptions::new();
        assert!(opts.mss.is_none());
        assert!(opts.window_scale.is_none());
        assert!(!opts.sack_permitted);
        assert!(opts.timestamp.is_none());
    }

    #[test]
    fn test_tcp_options_clone() {
        let opts1 = TcpOptions {
            mss: Some(1460),
            window_scale: Some(7),
            sack_permitted: true,
            timestamp: Some((12345, 0)),
        };
        let opts2 = opts1.clone();
        assert_eq!(opts1.mss, opts2.mss);
        assert_eq!(opts1.window_scale, opts2.window_scale);
    }

    #[test]
    fn test_tcp_options_copy() {
        let opts1 = TcpOptions::new();
        let opts2 = opts1;
        assert!(opts2.mss.is_none());
    }

    #[test]
    fn test_tcp_options_parse_empty() {
        let data: [u8; 0] = [];
        let opts = TcpOptions::parse(&data);
        assert!(opts.mss.is_none());
    }

    #[test]
    fn test_tcp_options_parse_mss() {
        // MSS option: kind=2, length=4, MSS=1460 (0x05B4)
        let data = [TCP_OPT_MSS, 4, 0x05, 0xB4];
        let opts = TcpOptions::parse(&data);
        assert_eq!(opts.mss, Some(1460));
    }

    #[test]
    fn test_tcp_options_parse_window_scale() {
        // Window scale option: kind=3, length=3, scale=7
        let data = [TCP_OPT_WINDOW_SCALE, 3, 7];
        let opts = TcpOptions::parse(&data);
        assert_eq!(opts.window_scale, Some(7));
    }

    #[test]
    fn test_tcp_options_parse_end() {
        let data = [TCP_OPT_END];
        let opts = TcpOptions::parse(&data);
        assert!(opts.mss.is_none());
    }

    #[test]
    fn test_tcp_options_parse_nop_padding() {
        // NOP followed by MSS
        let data = [TCP_OPT_NOP, TCP_OPT_MSS, 4, 0x05, 0xDC]; // MSS=1500
        let opts = TcpOptions::parse(&data);
        assert_eq!(opts.mss, Some(1500));
    }

    // =========================================================================
    // TCP Port and Sequence Tests
    // =========================================================================

    #[test]
    fn test_well_known_ports() {
        const HTTP: u16 = 80;
        const HTTPS: u16 = 443;
        const SSH: u16 = 22;
        const FTP: u16 = 21;
        const DNS: u16 = 53;

        assert!(HTTP < 1024);
        assert!(HTTPS < 1024);
        assert!(SSH < 1024);
        assert!(FTP < 1024);
        assert!(DNS < 1024);
    }

    #[test]
    fn test_ephemeral_port_range() {
        // Linux ephemeral port range: 32768-60999
        const EPHEMERAL_START: u16 = 32768;
        const EPHEMERAL_END: u16 = 60999;

        assert!(EPHEMERAL_START > 1024);
        assert!(EPHEMERAL_END < 65535);
    }

    #[test]
    fn test_sequence_wrap_around() {
        // TCP sequence numbers wrap around at 2^32
        let seq: u32 = 0xFFFFFFFF;
        let next_seq = seq.wrapping_add(1);
        assert_eq!(next_seq, 0);
    }

    #[test]
    fn test_sequence_comparison() {
        // TCP uses modular arithmetic for sequence comparison
        fn seq_lt(a: u32, b: u32) -> bool {
            (a.wrapping_sub(b) as i32) < 0
        }

        assert!(seq_lt(0, 1));
        assert!(seq_lt(0xFFFFFFFF, 0)); // Wrap around
        assert!(!seq_lt(1, 0));
    }
}
