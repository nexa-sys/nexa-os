//! 8259 PIC (Programmable Interrupt Controller) Emulation
//!
//! The 8259 PIC is the interrupt controller used in legacy x86 systems.
//! Modern systems use APIC, but the PIC is still present for compatibility.
//!
//! Master PIC: ports 0x20-0x21, IRQ 0-7
//! Slave PIC: ports 0xA0-0xA1, IRQ 8-15

use super::{Device, DeviceId, IoAccess};

/// PIC initialization state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitState {
    Normal,
    WaitIcw2,
    WaitIcw3,
    WaitIcw4,
}

/// Single 8259 PIC chip
#[derive(Debug)]
pub struct Pic8259Chip {
    /// Interrupt Request Register - pending interrupts
    irr: u8,
    /// In-Service Register - interrupts being serviced
    isr: u8,
    /// Interrupt Mask Register
    imr: u8,
    /// Vector offset (ICW2)
    vector_offset: u8,
    /// Initialization Command Words
    icw1: u8,
    icw2: u8,
    icw3: u8,
    icw4: u8,
    /// Initialization state machine
    init_state: InitState,
    /// Read ISR on next read (vs IRR)
    read_isr: bool,
    /// Auto EOI mode
    auto_eoi: bool,
    /// Special mask mode
    special_mask: bool,
    /// Priority rotation
    lowest_priority: u8,
}

impl Pic8259Chip {
    pub fn new() -> Self {
        Self {
            irr: 0,
            isr: 0,
            imr: 0xFF, // All interrupts masked
            vector_offset: 0,
            icw1: 0,
            icw2: 0,
            icw3: 0,
            icw4: 0,
            init_state: InitState::Normal,
            read_isr: false,
            auto_eoi: false,
            special_mask: false,
            lowest_priority: 7,
        }
    }
    
    pub fn reset(&mut self) {
        *self = Self::new();
    }
    
    /// Raise an interrupt line (0-7)
    pub fn raise_irq(&mut self, irq: u8) {
        debug_assert!(irq < 8);
        self.irr |= 1 << irq;
    }
    
    /// Lower an interrupt line
    pub fn lower_irq(&mut self, irq: u8) {
        debug_assert!(irq < 8);
        self.irr &= !(1 << irq);
    }
    
    /// Check if any unmasked interrupt is pending
    pub fn has_interrupt(&self) -> bool {
        (self.irr & !self.imr) != 0
    }
    
    /// Get highest priority pending interrupt
    pub fn get_interrupt(&self) -> Option<u8> {
        let pending = self.irr & !self.imr;
        if pending == 0 {
            return None;
        }
        
        // Find highest priority (lowest number, with rotation support)
        for i in 0..8 {
            let irq = (self.lowest_priority + 1 + i) % 8;
            if pending & (1 << irq) != 0 {
                return Some(irq);
            }
        }
        None
    }
    
    /// Acknowledge interrupt (INTA)
    pub fn ack(&mut self) -> Option<u8> {
        if let Some(irq) = self.get_interrupt() {
            self.irr &= !(1 << irq);
            self.isr |= 1 << irq;
            
            if self.auto_eoi {
                self.isr &= !(1 << irq);
            }
            
            Some(self.vector_offset + irq)
        } else {
            // Spurious interrupt
            Some(self.vector_offset + 7)
        }
    }
    
    /// Write to command register (port 0x20 or 0xA0)
    pub fn write_command(&mut self, value: u8) {
        if value & 0x10 != 0 {
            // ICW1
            self.icw1 = value;
            self.init_state = InitState::WaitIcw2;
            self.imr = 0;
            self.isr = 0;
            self.irr = 0;
            self.auto_eoi = false;
            self.read_isr = false;
            self.special_mask = false;
        } else if value & 0x08 != 0 {
            // OCW3
            if value & 0x02 != 0 {
                self.read_isr = (value & 0x01) != 0;
            }
            if value & 0x40 != 0 {
                self.special_mask = (value & 0x20) != 0;
            }
        } else {
            // OCW2
            let cmd = (value >> 5) & 0x07;
            match cmd {
                0b001 => {
                    // Non-specific EOI
                    if let Some(irq) = (0..8).find(|&i| self.isr & (1 << i) != 0) {
                        self.isr &= !(1 << irq);
                    }
                }
                0b011 => {
                    // Specific EOI
                    let irq = value & 0x07;
                    self.isr &= !(1 << irq);
                }
                0b101 => {
                    // Rotate on non-specific EOI
                    if let Some(irq) = (0..8).find(|&i| self.isr & (1 << i) != 0) {
                        self.isr &= !(1 << irq);
                        self.lowest_priority = irq;
                    }
                }
                0b111 => {
                    // Rotate on specific EOI
                    let irq = value & 0x07;
                    self.isr &= !(1 << irq);
                    self.lowest_priority = irq;
                }
                0b110 => {
                    // Set priority
                    self.lowest_priority = value & 0x07;
                }
                0b100 => {
                    // Rotate in automatic EOI mode (set)
                    // TODO: Implement
                }
                0b000 => {
                    // Rotate in automatic EOI mode (clear)
                    // TODO: Implement  
                }
                _ => {}
            }
        }
    }
    
    /// Read command register (port 0x20 or 0xA0)
    pub fn read_command(&self) -> u8 {
        if self.read_isr {
            self.isr
        } else {
            self.irr
        }
    }
    
    /// Write to data register (port 0x21 or 0xA1)
    pub fn write_data(&mut self, value: u8) {
        match self.init_state {
            InitState::Normal => {
                // OCW1 - set IMR
                self.imr = value;
            }
            InitState::WaitIcw2 => {
                self.icw2 = value;
                self.vector_offset = value & 0xF8;
                if self.icw1 & 0x02 != 0 {
                    // Single mode - no ICW3
                    if self.icw1 & 0x01 != 0 {
                        self.init_state = InitState::WaitIcw4;
                    } else {
                        self.init_state = InitState::Normal;
                    }
                } else {
                    self.init_state = InitState::WaitIcw3;
                }
            }
            InitState::WaitIcw3 => {
                self.icw3 = value;
                if self.icw1 & 0x01 != 0 {
                    self.init_state = InitState::WaitIcw4;
                } else {
                    self.init_state = InitState::Normal;
                }
            }
            InitState::WaitIcw4 => {
                self.icw4 = value;
                self.auto_eoi = (value & 0x02) != 0;
                self.init_state = InitState::Normal;
            }
        }
    }
    
    /// Read data register (port 0x21 or 0xA1)
    pub fn read_data(&self) -> u8 {
        self.imr
    }
}

impl Default for Pic8259Chip {
    fn default() -> Self {
        Self::new()
    }
}

/// Full dual 8259 PIC (master + slave)
pub struct Pic8259 {
    master: Pic8259Chip,
    slave: Pic8259Chip,
}

impl Pic8259 {
    pub fn new() -> Self {
        Self {
            master: Pic8259Chip::new(),
            slave: Pic8259Chip::new(),
        }
    }
    
    /// Raise an IRQ (0-15)
    pub fn raise_irq(&mut self, irq: u8) {
        if irq < 8 {
            self.master.raise_irq(irq);
        } else if irq < 16 {
            self.slave.raise_irq(irq - 8);
            // Cascade to master IRQ2
            self.master.raise_irq(2);
        }
    }
    
    /// Lower an IRQ
    pub fn lower_irq(&mut self, irq: u8) {
        if irq < 8 {
            self.master.lower_irq(irq);
        } else if irq < 16 {
            self.slave.lower_irq(irq - 8);
            // Check if slave still has pending
            if !self.slave.has_interrupt() {
                self.master.lower_irq(2);
            }
        }
    }
    
    /// Get pending interrupt vector
    pub fn get_interrupt_vector(&self) -> Option<u8> {
        if !self.master.has_interrupt() {
            return None;
        }
        
        let master_irq = self.master.get_interrupt()?;
        if master_irq == 2 {
            // Cascade - check slave
            self.slave.get_interrupt().map(|sirq| self.slave.vector_offset + sirq)
        } else {
            Some(self.master.vector_offset + master_irq)
        }
    }
}

impl Default for Pic8259 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Pic8259 {
    fn id(&self) -> DeviceId {
        DeviceId::PIC_MASTER
    }
    
    fn name(&self) -> &str {
        "8259 PIC"
    }
    
    fn reset(&mut self) {
        self.master.reset();
        self.slave.reset();
    }
    
    fn handles_port(&self, port: u16) -> bool {
        matches!(port, 0x20 | 0x21 | 0xA0 | 0xA1)
    }
    
    fn port_read(&mut self, port: u16, _access: IoAccess) -> u32 {
        match port {
            0x20 => self.master.read_command() as u32,
            0x21 => self.master.read_data() as u32,
            0xA0 => self.slave.read_command() as u32,
            0xA1 => self.slave.read_data() as u32,
            _ => 0xFF,
        }
    }
    
    fn port_write(&mut self, port: u16, value: u32, _access: IoAccess) {
        let value = value as u8;
        match port {
            0x20 => self.master.write_command(value),
            0x21 => self.master.write_data(value),
            0xA0 => self.slave.write_command(value),
            0xA1 => self.slave.write_data(value),
            _ => {}
        }
    }
    
    fn has_interrupt(&self) -> bool {
        self.master.has_interrupt()
    }
    
    fn interrupt_vector(&self) -> Option<u8> {
        self.get_interrupt_vector()
    }
    
    fn ack_interrupt(&mut self) {
        if let Some(irq) = self.master.get_interrupt() {
            if irq == 2 {
                self.slave.ack();
            }
            self.master.ack();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pic_basic() {
        let mut pic = Pic8259::new();
        
        // Initialize with vectors 0x20 and 0x28
        pic.port_write(0x20, 0x11, IoAccess::Byte); // ICW1
        pic.port_write(0x21, 0x20, IoAccess::Byte); // ICW2 - vector base 0x20
        pic.port_write(0x21, 0x04, IoAccess::Byte); // ICW3 - slave on IRQ2
        pic.port_write(0x21, 0x01, IoAccess::Byte); // ICW4
        
        pic.port_write(0xA0, 0x11, IoAccess::Byte); // ICW1
        pic.port_write(0xA1, 0x28, IoAccess::Byte); // ICW2 - vector base 0x28
        pic.port_write(0xA1, 0x02, IoAccess::Byte); // ICW3 - cascade identity
        pic.port_write(0xA1, 0x01, IoAccess::Byte); // ICW4
        
        // Unmask all
        pic.port_write(0x21, 0x00, IoAccess::Byte);
        pic.port_write(0xA1, 0x00, IoAccess::Byte);
        
        // Raise IRQ 1 (keyboard)
        pic.raise_irq(1);
        assert!(pic.has_interrupt());
        assert_eq!(pic.get_interrupt_vector(), Some(0x21));
        
        // Acknowledge interrupt (simulates CPU INTA)
        pic.ack_interrupt();
        
        // Send EOI
        pic.port_write(0x20, 0x20, IoAccess::Byte);
        assert!(!pic.has_interrupt());
    }
    
    #[test]
    fn test_pic_cascade() {
        let mut pic = Pic8259::new();
        
        // Initialize both PICs
        pic.port_write(0x20, 0x11, IoAccess::Byte);
        pic.port_write(0x21, 0x20, IoAccess::Byte);
        pic.port_write(0x21, 0x04, IoAccess::Byte);
        pic.port_write(0x21, 0x01, IoAccess::Byte);
        
        pic.port_write(0xA0, 0x11, IoAccess::Byte);
        pic.port_write(0xA1, 0x28, IoAccess::Byte);
        pic.port_write(0xA1, 0x02, IoAccess::Byte);
        pic.port_write(0xA1, 0x01, IoAccess::Byte);
        
        pic.port_write(0x21, 0x00, IoAccess::Byte);
        pic.port_write(0xA1, 0x00, IoAccess::Byte);
        
        // Raise IRQ 10 (slave IRQ 2)
        pic.raise_irq(10);
        assert!(pic.has_interrupt());
        assert_eq!(pic.get_interrupt_vector(), Some(0x2A));
    }
}
