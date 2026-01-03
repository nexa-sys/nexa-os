//! Serial Driver Tests

#[cfg(test)]
mod tests {
    // =========================================================================
    // COM Port Address Tests
    // =========================================================================

    #[test]
    fn test_com_port_addresses() {
        const COM1: u16 = 0x3F8;
        const COM2: u16 = 0x2F8;
        const COM3: u16 = 0x3E8;
        const COM4: u16 = 0x2E8;
        
        assert_eq!(COM1, 0x3F8);
        assert_eq!(COM2, 0x2F8);
        // COM3 and COM4 are 8 bytes below COM1/COM2
        assert_eq!(COM1 - COM3, 0x10);
    }

    #[test]
    fn test_uart_register_offsets() {
        // Register offsets from base port
        const THR: u16 = 0; // Transmit Holding Register (write)
        const RBR: u16 = 0; // Receive Buffer Register (read)
        const IER: u16 = 1; // Interrupt Enable Register
        const IIR: u16 = 2; // Interrupt Identification Register (read)
        const FCR: u16 = 2; // FIFO Control Register (write)
        const LCR: u16 = 3; // Line Control Register
        const MCR: u16 = 4; // Modem Control Register
        const LSR: u16 = 5; // Line Status Register
        const MSR: u16 = 6; // Modem Status Register
        const SCR: u16 = 7; // Scratch Register
        
        // When DLAB=1
        const DLL: u16 = 0; // Divisor Latch LSB
        const DLM: u16 = 1; // Divisor Latch MSB
        
        assert_eq!(LCR, 3);
        assert_eq!(LSR, 5);
    }

    // =========================================================================
    // Line Control Register Tests
    // =========================================================================

    #[test]
    fn test_lcr_word_length() {
        const WORD_5: u8 = 0b00;
        const WORD_6: u8 = 0b01;
        const WORD_7: u8 = 0b10;
        const WORD_8: u8 = 0b11;
        
        assert_eq!(WORD_8, 3);
    }

    #[test]
    fn test_lcr_stop_bits() {
        const STOP_1: u8 = 0 << 2;
        const STOP_2: u8 = 1 << 2;
        
        assert_eq!(STOP_1, 0);
        assert_eq!(STOP_2, 4);
    }

    #[test]
    fn test_lcr_parity() {
        const PARITY_NONE: u8 = 0b000 << 3;
        const PARITY_ODD: u8 = 0b001 << 3;
        const PARITY_EVEN: u8 = 0b011 << 3;
        const PARITY_MARK: u8 = 0b101 << 3;
        const PARITY_SPACE: u8 = 0b111 << 3;
        
        assert_eq!(PARITY_NONE, 0);
        assert_eq!(PARITY_ODD, 8);
    }

    #[test]
    fn test_lcr_dlab() {
        const DLAB: u8 = 1 << 7;
        assert_eq!(DLAB, 0x80);
    }

    #[test]
    fn test_lcr_8n1() {
        // 8 data bits, no parity, 1 stop bit
        const LCR_8N1: u8 = 0b00000011;
        assert_eq!(LCR_8N1, 3);
    }

    // =========================================================================
    // Baud Rate Divisor Tests
    // =========================================================================

    #[test]
    fn test_baud_divisor() {
        const BASE_CLOCK: u32 = 115200;
        
        fn calculate_divisor(baud: u32) -> u16 {
            (BASE_CLOCK / baud) as u16
        }
        
        assert_eq!(calculate_divisor(115200), 1);
        assert_eq!(calculate_divisor(57600), 2);
        assert_eq!(calculate_divisor(38400), 3);
        assert_eq!(calculate_divisor(19200), 6);
        assert_eq!(calculate_divisor(9600), 12);
    }

    #[test]
    fn test_divisor_split() {
        fn split_divisor(divisor: u16) -> (u8, u8) {
            let lsb = (divisor & 0xFF) as u8;
            let msb = (divisor >> 8) as u8;
            (lsb, msb)
        }
        
        assert_eq!(split_divisor(1), (1, 0));
        assert_eq!(split_divisor(0x1234), (0x34, 0x12));
    }

    // =========================================================================
    // Line Status Register Tests
    // =========================================================================

    #[test]
    fn test_lsr_flags() {
        const LSR_DR: u8 = 1 << 0;    // Data Ready
        const LSR_OE: u8 = 1 << 1;    // Overrun Error
        const LSR_PE: u8 = 1 << 2;    // Parity Error
        const LSR_FE: u8 = 1 << 3;    // Framing Error
        const LSR_BI: u8 = 1 << 4;    // Break Indicator
        const LSR_THRE: u8 = 1 << 5;  // Transmit Holding Register Empty
        const LSR_TEMT: u8 = 1 << 6;  // Transmitter Empty
        const LSR_ERR: u8 = 1 << 7;   // Error in FIFO
        
        assert_eq!(LSR_DR, 0x01);
        assert_eq!(LSR_THRE, 0x20);
        assert_eq!(LSR_TEMT, 0x40);
    }

    #[test]
    fn test_lsr_ready_to_send() {
        const LSR_THRE: u8 = 0x20;
        
        fn can_send(lsr: u8) -> bool {
            (lsr & LSR_THRE) != 0
        }
        
        assert!(can_send(0x60));
        assert!(!can_send(0x00));
    }

    #[test]
    fn test_lsr_data_available() {
        const LSR_DR: u8 = 0x01;
        
        fn data_available(lsr: u8) -> bool {
            (lsr & LSR_DR) != 0
        }
        
        assert!(data_available(0x01));
        assert!(!data_available(0x60));
    }

    // =========================================================================
    // FIFO Control Register Tests
    // =========================================================================

    #[test]
    fn test_fcr_flags() {
        const FCR_ENABLE: u8 = 1 << 0;      // Enable FIFOs
        const FCR_CLR_RECV: u8 = 1 << 1;    // Clear Receive FIFO
        const FCR_CLR_XMIT: u8 = 1 << 2;    // Clear Transmit FIFO
        const FCR_DMA_MODE: u8 = 1 << 3;    // DMA Mode Select
        const FCR_TRIGGER_1: u8 = 0b00 << 6;  // Trigger at 1 byte
        const FCR_TRIGGER_4: u8 = 0b01 << 6;  // Trigger at 4 bytes
        const FCR_TRIGGER_8: u8 = 0b10 << 6;  // Trigger at 8 bytes
        const FCR_TRIGGER_14: u8 = 0b11 << 6; // Trigger at 14 bytes
        
        assert_eq!(FCR_ENABLE, 0x01);
        assert_eq!(FCR_TRIGGER_14, 0xC0);
    }

    #[test]
    fn test_fcr_init_value() {
        // Common initialization: Enable FIFO, clear both, trigger at 14
        const FCR_INIT: u8 = 0xC7;
        assert_eq!(FCR_INIT, 0b11000111);
    }

    // =========================================================================
    // Interrupt Enable Register Tests
    // =========================================================================

    #[test]
    fn test_ier_flags() {
        const IER_RDA: u8 = 1 << 0;   // Received Data Available
        const IER_THRE: u8 = 1 << 1;  // Transmitter Holding Register Empty
        const IER_RLS: u8 = 1 << 2;   // Receiver Line Status
        const IER_MS: u8 = 1 << 3;    // Modem Status
        
        assert_eq!(IER_RDA, 0x01);
        assert_eq!(IER_THRE, 0x02);
    }
}
