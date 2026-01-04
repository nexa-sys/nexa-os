//! 16550 UART (Serial Port) Emulation
//!
//! Emulates a standard 16550 UART for serial I/O.
//! COM1: 0x3F8-0x3FF
//! COM2: 0x2F8-0x2FF

use std::any::Any;
use super::{Device, DeviceId, IoAccess};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Line Status Register bits
pub mod lsr {
    pub const DATA_READY: u8 = 0x01;
    pub const OVERRUN_ERROR: u8 = 0x02;
    pub const PARITY_ERROR: u8 = 0x04;
    pub const FRAMING_ERROR: u8 = 0x08;
    pub const BREAK_INTERRUPT: u8 = 0x10;
    pub const THRE: u8 = 0x20; // Transmitter Holding Register Empty
    pub const TEMT: u8 = 0x40; // Transmitter Empty
    pub const FIFO_ERROR: u8 = 0x80;
}

/// Modem Control Register bits
pub mod mcr {
    pub const DTR: u8 = 0x01;  // Data Terminal Ready
    pub const RTS: u8 = 0x02;  // Request To Send
    pub const OUT1: u8 = 0x04; // Output 1
    pub const OUT2: u8 = 0x08; // Output 2 (enables IRQ in PC)
    pub const LOOP: u8 = 0x10; // Loopback mode
}

/// Interrupt Enable Register bits
pub mod ier {
    pub const RDA: u8 = 0x01;   // Received Data Available
    pub const THRE: u8 = 0x02;  // Transmitter Holding Register Empty
    pub const RLS: u8 = 0x04;   // Receiver Line Status
    pub const MS: u8 = 0x08;    // Modem Status
}

/// FIFO Control Register bits
pub mod fcr {
    pub const ENABLE: u8 = 0x01;
    pub const RX_CLEAR: u8 = 0x02;
    pub const TX_CLEAR: u8 = 0x04;
    pub const DMA_MODE: u8 = 0x08;
    pub const TRIGGER_1: u8 = 0x00;
    pub const TRIGGER_4: u8 = 0x40;
    pub const TRIGGER_8: u8 = 0x80;
    pub const TRIGGER_14: u8 = 0xC0;
}

/// Interrupt Identification Register values
pub mod iir {
    pub const NO_INTERRUPT: u8 = 0x01;
    pub const THRE_INT: u8 = 0x02;
    pub const RDA_INT: u8 = 0x04;
    pub const RLS_INT: u8 = 0x06;
    pub const TIMEOUT_INT: u8 = 0x0C;
    pub const FIFOS_ENABLED: u8 = 0xC0;
}

/// 16550 UART emulation
pub struct Uart16550 {
    /// Device ID (COM1 or COM2)
    id: DeviceId,
    /// Base I/O port
    base_port: u16,
    /// IRQ number
    irq: u8,
    
    /// Receive buffer FIFO
    rx_fifo: VecDeque<u8>,
    /// Transmit buffer FIFO  
    tx_fifo: VecDeque<u8>,
    
    /// Divisor Latch (baud rate)
    divisor: u16,
    /// DLAB (Divisor Latch Access Bit) state
    dlab: bool,
    
    /// Line Control Register
    lcr: u8,
    /// Modem Control Register
    mcr: u8,
    /// Line Status Register
    lsr: u8,
    /// Modem Status Register
    msr: u8,
    /// Interrupt Enable Register
    ier: u8,
    /// FIFO Control Register
    fcr: u8,
    /// Scratch Register
    scratch: u8,
    
    /// FIFO enabled
    fifo_enabled: bool,
    /// FIFO trigger level
    fifo_trigger: usize,
    
    /// Interrupt pending
    irq_pending: bool,
    
    /// Output capture (for testing)
    output: Arc<Mutex<Vec<u8>>>,
    /// Input injection (for testing)
    input: Arc<Mutex<VecDeque<u8>>>,
}

impl Uart16550 {
    pub fn new_com1() -> Self {
        Self::new(DeviceId::UART_COM1, 0x3F8, 4)
    }
    
    pub fn new_com2() -> Self {
        Self::new(DeviceId::UART_COM2, 0x2F8, 3)
    }
    
    pub fn new(id: DeviceId, base_port: u16, irq: u8) -> Self {
        Self {
            id,
            base_port,
            irq,
            rx_fifo: VecDeque::with_capacity(16),
            tx_fifo: VecDeque::with_capacity(16),
            divisor: 1, // 115200 baud default
            dlab: false,
            lcr: 0x03, // 8N1
            mcr: 0,
            lsr: lsr::THRE | lsr::TEMT, // TX empty
            msr: 0,
            ier: 0,
            fcr: 0,
            scratch: 0,
            fifo_enabled: false,
            fifo_trigger: 1,
            irq_pending: false,
            output: Arc::new(Mutex::new(Vec::new())),
            input: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
    
    /// Get output capture (for testing)
    pub fn output(&self) -> Arc<Mutex<Vec<u8>>> {
        Arc::clone(&self.output)
    }
    
    /// Get input injection (for testing)
    pub fn input(&self) -> Arc<Mutex<VecDeque<u8>>> {
        Arc::clone(&self.input)
    }
    
    /// Get all captured output as string
    pub fn output_string(&self) -> String {
        let out = self.output.lock().unwrap();
        String::from_utf8_lossy(&out).to_string()
    }
    
    /// Inject input data (simulates receiving from remote)
    pub fn inject_input(&mut self, data: &[u8]) {
        for &b in data {
            if self.rx_fifo.len() < 16 {
                self.rx_fifo.push_back(b);
            }
        }
        if !self.rx_fifo.is_empty() {
            self.lsr |= lsr::DATA_READY;
            self.check_interrupt();
        }
    }
    
    fn check_interrupt(&mut self) {
        let mut pending = false;
        
        // Check RX interrupt
        if (self.ier & ier::RDA) != 0 && (self.lsr & lsr::DATA_READY) != 0 {
            pending = true;
        }
        
        // Check TX interrupt
        if (self.ier & ier::THRE) != 0 && (self.lsr & lsr::THRE) != 0 {
            pending = true;
        }
        
        // Check Line Status interrupt
        if (self.ier & ier::RLS) != 0 {
            if (self.lsr & (lsr::OVERRUN_ERROR | lsr::PARITY_ERROR | lsr::FRAMING_ERROR)) != 0 {
                pending = true;
            }
        }
        
        // Only trigger if OUT2 is set (enables IRQ in PC architecture)
        if pending && (self.mcr & mcr::OUT2) != 0 {
            self.irq_pending = true;
        }
    }
    
    fn get_iir(&self) -> u8 {
        let base = if self.fifo_enabled { iir::FIFOS_ENABLED } else { 0 };
        
        // Priority: RLS > RDA > THRE > MS
        if (self.ier & ier::RLS) != 0 {
            if (self.lsr & (lsr::OVERRUN_ERROR | lsr::PARITY_ERROR | lsr::FRAMING_ERROR)) != 0 {
                return base | iir::RLS_INT;
            }
        }
        
        if (self.ier & ier::RDA) != 0 && (self.lsr & lsr::DATA_READY) != 0 {
            return base | iir::RDA_INT;
        }
        
        if (self.ier & ier::THRE) != 0 && (self.lsr & lsr::THRE) != 0 {
            return base | iir::THRE_INT;
        }
        
        base | iir::NO_INTERRUPT
    }
    
    fn read_register(&mut self, offset: u16) -> u8 {
        match offset {
            0 => {
                if self.dlab {
                    // Divisor LSB
                    self.divisor as u8
                } else {
                    // Receive Buffer (RBR)
                    if let Some(byte) = self.rx_fifo.pop_front() {
                        if self.rx_fifo.is_empty() {
                            self.lsr &= !lsr::DATA_READY;
                        }
                        byte
                    } else {
                        0
                    }
                }
            }
            1 => {
                if self.dlab {
                    // Divisor MSB
                    (self.divisor >> 8) as u8
                } else {
                    // Interrupt Enable Register
                    self.ier
                }
            }
            2 => {
                // Interrupt Identification Register
                let iir = self.get_iir();
                // Reading IIR clears THRE interrupt
                if (iir & 0x0F) == iir::THRE_INT {
                    self.irq_pending = false;
                }
                iir
            }
            3 => self.lcr,
            4 => self.mcr,
            5 => {
                // Line Status Register
                let status = self.lsr;
                // Clear error bits on read
                self.lsr &= !(lsr::OVERRUN_ERROR | lsr::PARITY_ERROR | 
                             lsr::FRAMING_ERROR | lsr::BREAK_INTERRUPT);
                status
            }
            6 => {
                // Modem Status Register
                let status = self.msr;
                // Clear delta bits on read
                self.msr &= 0xF0;
                status
            }
            7 => self.scratch,
            _ => 0xFF,
        }
    }
    
    fn write_register(&mut self, offset: u16, value: u8) {
        match offset {
            0 => {
                if self.dlab {
                    // Divisor LSB
                    self.divisor = (self.divisor & 0xFF00) | (value as u16);
                } else {
                    // Transmit Holding Register (THR)
                    // In loopback mode, echo to RX
                    if (self.mcr & mcr::LOOP) != 0 {
                        self.rx_fifo.push_back(value);
                        self.lsr |= lsr::DATA_READY;
                    } else {
                        // Write to output capture
                        self.output.lock().unwrap().push(value);
                    }
                    
                    // TX is always ready (instant transmission)
                    self.lsr |= lsr::THRE | lsr::TEMT;
                    self.check_interrupt();
                }
            }
            1 => {
                if self.dlab {
                    // Divisor MSB
                    self.divisor = (self.divisor & 0x00FF) | ((value as u16) << 8);
                } else {
                    // Interrupt Enable Register
                    self.ier = value & 0x0F;
                    self.check_interrupt();
                }
            }
            2 => {
                // FIFO Control Register (FCR)
                self.fcr = value;
                self.fifo_enabled = (value & fcr::ENABLE) != 0;
                
                if (value & fcr::RX_CLEAR) != 0 {
                    self.rx_fifo.clear();
                    self.lsr &= !lsr::DATA_READY;
                }
                if (value & fcr::TX_CLEAR) != 0 {
                    self.tx_fifo.clear();
                }
                
                self.fifo_trigger = match value & 0xC0 {
                    fcr::TRIGGER_1 => 1,
                    fcr::TRIGGER_4 => 4,
                    fcr::TRIGGER_8 => 8,
                    fcr::TRIGGER_14 => 14,
                    _ => 1,
                };
            }
            3 => {
                // Line Control Register
                self.lcr = value;
                self.dlab = (value & 0x80) != 0;
            }
            4 => {
                // Modem Control Register
                self.mcr = value & 0x1F;
                self.check_interrupt();
            }
            5 => {
                // Line Status Register (read-only, but some bits can be set)
            }
            6 => {
                // Modem Status Register (read-only)
            }
            7 => {
                // Scratch Register
                self.scratch = value;
            }
            _ => {}
        }
    }
}

impl Device for Uart16550 {
    fn id(&self) -> DeviceId {
        self.id
    }
    
    fn name(&self) -> &str {
        match self.id {
            DeviceId::UART_COM1 => "16550 UART COM1",
            DeviceId::UART_COM2 => "16550 UART COM2",
            _ => "16550 UART",
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    
    fn reset(&mut self) {
        self.rx_fifo.clear();
        self.tx_fifo.clear();
        self.divisor = 1;
        self.dlab = false;
        self.lcr = 0x03;
        self.mcr = 0;
        self.lsr = lsr::THRE | lsr::TEMT;
        self.msr = 0;
        self.ier = 0;
        self.fcr = 0;
        self.scratch = 0;
        self.fifo_enabled = false;
        self.fifo_trigger = 1;
        self.irq_pending = false;
        self.output.lock().unwrap().clear();
        self.input.lock().unwrap().clear();
    }
    
    fn handles_port(&self, port: u16) -> bool {
        port >= self.base_port && port < self.base_port + 8
    }
    
    fn port_read(&mut self, port: u16, _access: IoAccess) -> u32 {
        // Check for injected input
        {
            let mut input = self.input.lock().unwrap();
            while let Some(b) = input.pop_front() {
                if self.rx_fifo.len() < 16 {
                    self.rx_fifo.push_back(b);
                }
            }
            if !self.rx_fifo.is_empty() {
                self.lsr |= lsr::DATA_READY;
            }
        }
        
        let offset = port - self.base_port;
        self.read_register(offset) as u32
    }
    
    fn port_write(&mut self, port: u16, value: u32, _access: IoAccess) {
        let offset = port - self.base_port;
        self.write_register(offset, value as u8);
    }
    
    fn has_interrupt(&self) -> bool {
        self.irq_pending
    }
    
    fn interrupt_vector(&self) -> Option<u8> {
        if self.irq_pending {
            Some(self.irq)
        } else {
            None
        }
    }
    
    fn ack_interrupt(&mut self) {
        self.irq_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_uart_output() {
        let mut uart = Uart16550::new_com1();
        
        // Write "Hello"
        for &b in b"Hello" {
            uart.port_write(0x3F8, b as u32, IoAccess::Byte);
        }
        
        assert_eq!(uart.output_string(), "Hello");
    }
    
    #[test]
    fn test_uart_loopback() {
        let mut uart = Uart16550::new_com1();
        
        // Enable loopback
        uart.port_write(0x3FC, mcr::LOOP as u32, IoAccess::Byte);
        
        // Write byte
        uart.port_write(0x3F8, 0x42, IoAccess::Byte);
        
        // Should be in RX buffer
        assert!(uart.port_read(0x3FD, IoAccess::Byte) & lsr::DATA_READY as u32 != 0);
        assert_eq!(uart.port_read(0x3F8, IoAccess::Byte), 0x42);
    }
    
    #[test]
    fn test_uart_divisor() {
        let mut uart = Uart16550::new_com1();
        
        // Enable DLAB
        uart.port_write(0x3FB, 0x83, IoAccess::Byte);
        
        // Set divisor to 12 (9600 baud)
        uart.port_write(0x3F8, 12, IoAccess::Byte);
        uart.port_write(0x3F9, 0, IoAccess::Byte);
        
        assert_eq!(uart.divisor, 12);
        
        // Disable DLAB
        uart.port_write(0x3FB, 0x03, IoAccess::Byte);
    }
}
