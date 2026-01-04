//! RTC (Real Time Clock) Emulation
//!
//! CMOS RTC at ports 0x70-0x71

use std::any::Any;
use super::{Device, DeviceId, IoAccess};
use std::time::{SystemTime, UNIX_EPOCH};

/// RTC register indices
pub mod reg {
    pub const SECONDS: u8 = 0x00;
    pub const SECONDS_ALARM: u8 = 0x01;
    pub const MINUTES: u8 = 0x02;
    pub const MINUTES_ALARM: u8 = 0x03;
    pub const HOURS: u8 = 0x04;
    pub const HOURS_ALARM: u8 = 0x05;
    pub const DAY_OF_WEEK: u8 = 0x06;
    pub const DAY_OF_MONTH: u8 = 0x07;
    pub const MONTH: u8 = 0x08;
    pub const YEAR: u8 = 0x09;
    pub const STATUS_A: u8 = 0x0A;
    pub const STATUS_B: u8 = 0x0B;
    pub const STATUS_C: u8 = 0x0C;
    pub const STATUS_D: u8 = 0x0D;
    pub const CENTURY: u8 = 0x32;
}

/// CMOS RTC emulation
pub struct Rtc {
    /// Currently selected register
    index: u8,
    /// CMOS RAM (128 bytes)
    cmos: [u8; 128],
    /// Use BCD mode
    bcd_mode: bool,
    /// 24-hour mode
    hour_24: bool,
    /// Interrupt pending
    irq_pending: bool,
}

impl Rtc {
    pub fn new() -> Self {
        let mut rtc = Self {
            index: 0,
            cmos: [0; 128],
            bcd_mode: true,
            hour_24: true,
            irq_pending: false,
        };
        
        // Initialize status registers
        rtc.cmos[reg::STATUS_A as usize] = 0x26; // 32.768kHz, 1024Hz periodic
        rtc.cmos[reg::STATUS_B as usize] = 0x02; // 24-hour mode
        rtc.cmos[reg::STATUS_D as usize] = 0x80; // Valid RAM/time
        
        rtc
    }
    
    fn to_bcd(&self, val: u8) -> u8 {
        if self.bcd_mode {
            ((val / 10) << 4) | (val % 10)
        } else {
            val
        }
    }
    
    fn from_bcd(&self, val: u8) -> u8 {
        if self.bcd_mode {
            ((val >> 4) * 10) + (val & 0x0F)
        } else {
            val
        }
    }
    
    fn get_current_time(&self) -> (u8, u8, u8, u8, u8, u8, u8) {
        // Get current time from host
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Simple conversion (not accounting for leap seconds, etc.)
        let secs = (now % 60) as u8;
        let mins = ((now / 60) % 60) as u8;
        let hours = ((now / 3600) % 24) as u8;
        
        let days = (now / 86400) as u32;
        
        // Calculate year/month/day (simplified)
        let mut y = 1970u32;
        let mut d = days;
        loop {
            let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
                366
            } else {
                365
            };
            if d < days_in_year {
                break;
            }
            d -= days_in_year;
            y += 1;
        }
        
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days_in_month = if leap {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };
        
        let mut m = 0u8;
        for (i, &dim) in days_in_month.iter().enumerate() {
            if d < dim {
                m = (i + 1) as u8;
                break;
            }
            d -= dim;
        }
        
        let day = (d + 1) as u8;
        let dow = ((days + 4) % 7 + 1) as u8; // 1 = Sunday
        let year = (y % 100) as u8;
        let century = (y / 100) as u8;
        
        (secs, mins, hours, dow, day, m, year)
    }
    
    fn read_register(&mut self, reg: u8) -> u8 {
        let (secs, mins, hours, dow, day, month, year) = self.get_current_time();
        
        match reg {
            reg::SECONDS => self.to_bcd(secs),
            reg::MINUTES => self.to_bcd(mins),
            reg::HOURS => {
                if self.hour_24 {
                    self.to_bcd(hours)
                } else {
                    let h = if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours };
                    let pm = if hours >= 12 { 0x80 } else { 0 };
                    self.to_bcd(h) | pm
                }
            }
            reg::DAY_OF_WEEK => dow,
            reg::DAY_OF_MONTH => self.to_bcd(day),
            reg::MONTH => self.to_bcd(month),
            reg::YEAR => self.to_bcd(year),
            reg::STATUS_C => {
                // Reading Status C clears interrupt flags
                let val = self.cmos[reg as usize];
                self.cmos[reg as usize] = 0;
                self.irq_pending = false;
                val
            }
            _ => self.cmos.get(reg as usize).copied().unwrap_or(0),
        }
    }
    
    fn write_register(&mut self, reg: u8, value: u8) {
        match reg {
            reg::STATUS_A => {
                // Bits 0-6 are writable
                self.cmos[reg as usize] = (self.cmos[reg as usize] & 0x80) | (value & 0x7F);
            }
            reg::STATUS_B => {
                self.cmos[reg as usize] = value;
                self.bcd_mode = (value & 0x04) == 0;
                self.hour_24 = (value & 0x02) != 0;
            }
            reg::STATUS_C | reg::STATUS_D => {
                // Read-only
            }
            _ => {
                if (reg as usize) < self.cmos.len() {
                    self.cmos[reg as usize] = value;
                }
            }
        }
    }
}

impl Default for Rtc {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Rtc {
    fn id(&self) -> DeviceId {
        DeviceId::RTC
    }
    
    fn name(&self) -> &str {
        "CMOS RTC"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    
    fn reset(&mut self) {
        *self = Self::new();
    }
    
    fn handles_port(&self, port: u16) -> bool {
        matches!(port, 0x70 | 0x71)
    }
    
    fn port_read(&mut self, port: u16, _access: IoAccess) -> u32 {
        match port {
            0x70 => self.index as u32,
            0x71 => self.read_register(self.index) as u32,
            _ => 0xFF,
        }
    }
    
    fn port_write(&mut self, port: u16, value: u32, _access: IoAccess) {
        match port {
            0x70 => {
                self.index = (value as u8) & 0x7F;
            }
            0x71 => {
                self.write_register(self.index, value as u8);
            }
            _ => {}
        }
    }
    
    fn has_interrupt(&self) -> bool {
        self.irq_pending
    }
    
    fn interrupt_vector(&self) -> Option<u8> {
        if self.irq_pending { Some(8) } else { None }
    }
    
    fn ack_interrupt(&mut self) {
        self.irq_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rtc_basic() {
        let mut rtc = Rtc::new();
        
        // Select seconds register
        rtc.port_write(0x70, reg::SECONDS as u32, IoAccess::Byte);
        let secs = rtc.port_read(0x71, IoAccess::Byte);
        
        // Should be valid BCD (0-59)
        let secs_bin = ((secs >> 4) * 10) + (secs & 0x0F);
        assert!(secs_bin < 60);
    }
    
    #[test]
    fn test_rtc_status() {
        let mut rtc = Rtc::new();
        
        // Read Status D (should have valid bit set)
        rtc.port_write(0x70, reg::STATUS_D as u32, IoAccess::Byte);
        let status_d = rtc.port_read(0x71, IoAccess::Byte);
        assert!(status_d & 0x80 != 0);
    }
}
