//! 8254 PIT (Programmable Interval Timer) Emulation
//!
//! The PIT provides timer services at ports 0x40-0x43.
//! Channel 0: System timer (IRQ 0)
//! Channel 1: Memory refresh (legacy)
//! Channel 2: PC Speaker

use std::any::Any;
use super::{Device, DeviceId, IoAccess};

/// Operating mode for a PIT channel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PitMode {
    /// Mode 0: Interrupt on terminal count
    InterruptOnTerminal,
    /// Mode 1: Hardware re-triggerable one-shot
    HardwareOneShot,
    /// Mode 2: Rate generator
    RateGenerator,
    /// Mode 3: Square wave generator
    SquareWave,
    /// Mode 4: Software triggered strobe
    SoftwareStrobe,
    /// Mode 5: Hardware triggered strobe
    HardwareStrobe,
}

impl From<u8> for PitMode {
    fn from(val: u8) -> Self {
        match val & 0x07 {
            0 => PitMode::InterruptOnTerminal,
            1 => PitMode::HardwareOneShot,
            2 | 6 => PitMode::RateGenerator,
            3 | 7 => PitMode::SquareWave,
            4 => PitMode::SoftwareStrobe,
            5 => PitMode::HardwareStrobe,
            _ => unreachable!(),
        }
    }
}

/// Access mode for reading/writing counter
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    Latch,
    LowByte,
    HighByte,
    LowHigh,
}

impl From<u8> for AccessMode {
    fn from(val: u8) -> Self {
        match (val >> 4) & 0x03 {
            0 => AccessMode::Latch,
            1 => AccessMode::LowByte,
            2 => AccessMode::HighByte,
            3 => AccessMode::LowHigh,
            _ => unreachable!(),
        }
    }
}

/// Single PIT channel state
#[derive(Debug)]
pub struct PitChannel {
    /// Counter reload value
    reload: u16,
    /// Current counter value
    counter: u16,
    /// Latched value (for latch command)
    latched: Option<u16>,
    /// Output state
    output: bool,
    /// Operating mode
    mode: PitMode,
    /// Access mode
    access: AccessMode,
    /// Waiting for high byte
    high_byte_next: bool,
    /// Gate input (only matters for channels 1, 2)
    gate: bool,
    /// Interrupt pending
    irq_pending: bool,
}

impl PitChannel {
    pub fn new() -> Self {
        Self {
            reload: 0,
            counter: 0,
            latched: None,
            output: false,
            mode: PitMode::InterruptOnTerminal,
            access: AccessMode::LowHigh,
            high_byte_next: false,
            gate: true,
            irq_pending: false,
        }
    }
    
    pub fn reset(&mut self) {
        *self = Self::new();
    }
    
    /// Write control word
    pub fn write_control(&mut self, val: u8) {
        self.mode = PitMode::from(val);
        self.access = AccessMode::from(val);
        self.high_byte_next = false;
        
        if matches!(self.access, AccessMode::Latch) {
            self.latched = Some(self.counter);
        }
    }
    
    /// Write counter value
    pub fn write_counter(&mut self, val: u8) {
        match self.access {
            AccessMode::Latch => {}
            AccessMode::LowByte => {
                self.reload = (self.reload & 0xFF00) | (val as u16);
                self.counter = self.reload;
            }
            AccessMode::HighByte => {
                self.reload = (self.reload & 0x00FF) | ((val as u16) << 8);
                self.counter = self.reload;
            }
            AccessMode::LowHigh => {
                if self.high_byte_next {
                    self.reload = (self.reload & 0x00FF) | ((val as u16) << 8);
                    self.counter = self.reload;
                } else {
                    self.reload = (self.reload & 0xFF00) | (val as u16);
                }
                self.high_byte_next = !self.high_byte_next;
            }
        }
    }
    
    /// Read counter value
    pub fn read_counter(&mut self) -> u8 {
        let value = self.latched.unwrap_or(self.counter);
        
        match self.access {
            AccessMode::Latch => {
                let result = if self.high_byte_next {
                    (value >> 8) as u8
                } else {
                    value as u8
                };
                
                if self.high_byte_next {
                    self.latched = None;
                }
                self.high_byte_next = !self.high_byte_next;
                result
            }
            AccessMode::LowByte => value as u8,
            AccessMode::HighByte => (value >> 8) as u8,
            AccessMode::LowHigh => {
                let result = if self.high_byte_next {
                    (value >> 8) as u8
                } else {
                    value as u8
                };
                self.high_byte_next = !self.high_byte_next;
                result
            }
        }
    }
    
    /// Tick the channel (decrement counter)
    /// Returns true if interrupt should fire
    pub fn tick(&mut self) -> bool {
        if !self.gate {
            return false;
        }
        
        match self.mode {
            PitMode::InterruptOnTerminal => {
                if self.counter > 0 {
                    self.counter -= 1;
                    if self.counter == 0 {
                        self.output = true;
                        self.irq_pending = true;
                        return true;
                    }
                }
            }
            PitMode::RateGenerator => {
                if self.counter > 0 {
                    self.counter -= 1;
                    if self.counter == 1 {
                        self.output = false;
                        self.irq_pending = true;
                    } else if self.counter == 0 {
                        self.output = true;
                        self.counter = self.reload;
                        return true;
                    }
                }
            }
            PitMode::SquareWave => {
                if self.counter > 0 {
                    self.counter -= 2;
                    if self.counter == 0 {
                        self.output = !self.output;
                        self.counter = self.reload;
                        if self.output {
                            self.irq_pending = true;
                            return true;
                        }
                    }
                }
            }
            _ => {
                // Simplified handling for other modes
                if self.counter > 0 {
                    self.counter -= 1;
                }
            }
        }
        false
    }
    
    pub fn get_output(&self) -> bool {
        self.output
    }
    
    pub fn has_irq(&self) -> bool {
        self.irq_pending
    }
    
    pub fn ack_irq(&mut self) {
        self.irq_pending = false;
    }
}

impl Default for PitChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// 8254 PIT with 3 channels
pub struct Pit8254 {
    channels: [PitChannel; 3],
    /// Cycles per tick (to slow down counter for realistic timing)
    cycles_per_tick: u64,
    /// Accumulated cycles
    accumulated_cycles: u64,
}

impl Pit8254 {
    /// PIT base frequency: 1.193182 MHz
    pub const BASE_FREQUENCY: u64 = 1193182;
    
    pub fn new() -> Self {
        Self {
            channels: [PitChannel::new(), PitChannel::new(), PitChannel::new()],
            // Assume ~1GHz CPU, tick PIT every ~1000 CPU cycles
            cycles_per_tick: 1000,
            accumulated_cycles: 0,
        }
    }
    
    /// Set CPU frequency for accurate timing
    pub fn set_cpu_frequency(&mut self, freq_hz: u64) {
        self.cycles_per_tick = freq_hz / Self::BASE_FREQUENCY;
    }
}

impl Default for Pit8254 {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Pit8254 {
    fn id(&self) -> DeviceId {
        DeviceId::PIT
    }
    
    fn name(&self) -> &str {
        "8254 PIT"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    
    fn reset(&mut self) {
        for ch in &mut self.channels {
            ch.reset();
        }
        self.accumulated_cycles = 0;
    }
    
    fn handles_port(&self, port: u16) -> bool {
        matches!(port, 0x40..=0x43)
    }
    
    fn port_read(&mut self, port: u16, _access: IoAccess) -> u32 {
        match port {
            0x40 => self.channels[0].read_counter() as u32,
            0x41 => self.channels[1].read_counter() as u32,
            0x42 => self.channels[2].read_counter() as u32,
            0x43 => 0, // Control register is write-only
            _ => 0xFF,
        }
    }
    
    fn port_write(&mut self, port: u16, value: u32, _access: IoAccess) {
        let value = value as u8;
        match port {
            0x40 => self.channels[0].write_counter(value),
            0x41 => self.channels[1].write_counter(value),
            0x42 => self.channels[2].write_counter(value),
            0x43 => {
                // Control word
                let channel = (value >> 6) & 0x03;
                if channel < 3 {
                    self.channels[channel as usize].write_control(value);
                }
                // channel == 3 is read-back command, TODO if needed
            }
            _ => {}
        }
    }
    
    fn tick(&mut self, cycles: u64) {
        self.accumulated_cycles += cycles;
        
        while self.accumulated_cycles >= self.cycles_per_tick {
            self.accumulated_cycles -= self.cycles_per_tick;
            
            for ch in &mut self.channels {
                ch.tick();
            }
        }
    }
    
    fn has_interrupt(&self) -> bool {
        self.channels[0].has_irq()
    }
    
    fn interrupt_vector(&self) -> Option<u8> {
        if self.channels[0].has_irq() {
            Some(0) // IRQ 0
        } else {
            None
        }
    }
    
    fn ack_interrupt(&mut self) {
        self.channels[0].ack_irq();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pit_basic() {
        let mut pit = Pit8254::new();
        
        // Program channel 0 for rate generator at ~100 Hz
        // Divisor = 1193182 / 100 ≈ 11932 = 0x2E9C
        pit.port_write(0x43, 0x34, IoAccess::Byte); // Channel 0, mode 2, binary
        pit.port_write(0x40, 0x9C, IoAccess::Byte); // Low byte
        pit.port_write(0x40, 0x2E, IoAccess::Byte); // High byte
        
        assert_eq!(pit.channels[0].reload, 0x2E9C);
    }
    
    #[test]
    fn test_pit_tick() {
        let mut pit = Pit8254::new();
        pit.cycles_per_tick = 1; // Tick every cycle for testing
        
        // Simple countdown
        pit.port_write(0x43, 0x30, IoAccess::Byte); // Channel 0, mode 0, binary
        pit.port_write(0x40, 10, IoAccess::Byte); // Low byte
        pit.port_write(0x40, 0, IoAccess::Byte);  // High byte
        
        for _ in 0..9 {
            pit.tick(1);
            assert!(!pit.has_interrupt());
        }
        
        pit.tick(1);
        assert!(pit.has_interrupt());
    }
}
