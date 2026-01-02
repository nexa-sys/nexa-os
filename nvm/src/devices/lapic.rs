//! Local APIC Emulation
//!
//! The Local APIC is the per-CPU interrupt controller in modern x86 systems.
//! MMIO base: 0xFEE0_0000 (configurable via MSR)

use super::{Device, DeviceId, IoAccess};
use crate::memory::PhysAddr;
use std::collections::VecDeque;

/// LAPIC register offsets
pub mod reg {
    pub const ID: usize = 0x020;
    pub const VERSION: usize = 0x030;
    pub const TPR: usize = 0x080;         // Task Priority Register
    pub const APR: usize = 0x090;         // Arbitration Priority Register
    pub const PPR: usize = 0x0A0;         // Processor Priority Register
    pub const EOI: usize = 0x0B0;         // End Of Interrupt
    pub const RRD: usize = 0x0C0;         // Remote Read Register
    pub const LDR: usize = 0x0D0;         // Logical Destination Register
    pub const DFR: usize = 0x0E0;         // Destination Format Register
    pub const SVR: usize = 0x0F0;         // Spurious Interrupt Vector Register
    pub const ISR_BASE: usize = 0x100;    // In-Service Register (8 regs)
    pub const TMR_BASE: usize = 0x180;    // Trigger Mode Register (8 regs)
    pub const IRR_BASE: usize = 0x200;    // Interrupt Request Register (8 regs)
    pub const ESR: usize = 0x280;         // Error Status Register
    pub const ICR_LOW: usize = 0x300;     // Interrupt Command Register (low)
    pub const ICR_HIGH: usize = 0x310;    // Interrupt Command Register (high)
    pub const TIMER_LVT: usize = 0x320;   // Timer Local Vector Table
    pub const THERMAL_LVT: usize = 0x330; // Thermal Local Vector Table
    pub const PERF_LVT: usize = 0x340;    // Performance Counter LVT
    pub const LINT0_LVT: usize = 0x350;   // Local Interrupt 0 LVT
    pub const LINT1_LVT: usize = 0x360;   // Local Interrupt 1 LVT
    pub const ERROR_LVT: usize = 0x370;   // Error LVT
    pub const TIMER_ICR: usize = 0x380;   // Timer Initial Count Register
    pub const TIMER_CCR: usize = 0x390;   // Timer Current Count Register
    pub const TIMER_DCR: usize = 0x3E0;   // Timer Divide Configuration Register
}

/// LVT entry masks
pub mod lvt {
    pub const VECTOR_MASK: u32 = 0xFF;
    pub const DELIVERY_MODE_MASK: u32 = 0x700;
    pub const DELIVERY_MODE_FIXED: u32 = 0x000;
    pub const DELIVERY_MODE_NMI: u32 = 0x400;
    pub const DELIVERY_MODE_EXTINT: u32 = 0x700;
    pub const PENDING: u32 = 0x1000;
    pub const POLARITY_LOW: u32 = 0x2000;
    pub const REMOTE_IRR: u32 = 0x4000;
    pub const TRIGGER_LEVEL: u32 = 0x8000;
    pub const MASKED: u32 = 0x10000;
    pub const TIMER_PERIODIC: u32 = 0x20000;
    pub const TIMER_TSC_DEADLINE: u32 = 0x40000;
}

/// Local APIC emulation
pub struct LocalApic {
    /// APIC ID
    id: u32,
    /// MMIO base address
    base: PhysAddr,
    /// Enabled
    enabled: bool,
    
    /// Task Priority Register
    tpr: u32,
    /// Logical Destination Register
    ldr: u32,
    /// Destination Format Register
    dfr: u32,
    /// Spurious Interrupt Vector Register
    svr: u32,
    
    /// In-Service Register (256 bits)
    isr: [u32; 8],
    /// Trigger Mode Register (256 bits)
    tmr: [u32; 8],
    /// Interrupt Request Register (256 bits)
    irr: [u32; 8],
    
    /// Error Status Register
    esr: u32,
    
    /// Interrupt Command Register
    icr: u64,
    
    /// LVT entries
    timer_lvt: u32,
    thermal_lvt: u32,
    perf_lvt: u32,
    lint0_lvt: u32,
    lint1_lvt: u32,
    error_lvt: u32,
    
    /// Timer
    timer_initial: u32,
    timer_current: u32,
    timer_divide: u32,
    
    /// Pending interrupts queue
    pending: VecDeque<u8>,
    
    /// Cycles since last timer tick
    timer_cycles: u64,
}

impl LocalApic {
    pub const DEFAULT_BASE: PhysAddr = 0xFEE0_0000;
    pub const SIZE: usize = 0x1000;
    
    pub fn new(id: u32) -> Self {
        Self {
            id,
            base: Self::DEFAULT_BASE,
            enabled: true,
            tpr: 0,
            ldr: 0,
            dfr: 0xFFFFFFFF,
            svr: 0xFF,
            isr: [0; 8],
            tmr: [0; 8],
            irr: [0; 8],
            esr: 0,
            icr: 0,
            timer_lvt: lvt::MASKED,
            thermal_lvt: lvt::MASKED,
            perf_lvt: lvt::MASKED,
            lint0_lvt: lvt::MASKED,
            lint1_lvt: lvt::MASKED,
            error_lvt: lvt::MASKED,
            timer_initial: 0,
            timer_current: 0,
            timer_divide: 0,
            pending: VecDeque::new(),
            timer_cycles: 0,
        }
    }
    
    /// Set MMIO base address
    pub fn set_base(&mut self, base: PhysAddr) {
        self.base = base;
    }
    
    /// Inject an interrupt
    pub fn inject_interrupt(&mut self, vector: u8) {
        let idx = (vector / 32) as usize;
        let bit = 1u32 << (vector % 32);
        self.irr[idx] |= bit;
    }
    
    /// Get highest priority pending interrupt
    fn get_pending_interrupt(&self) -> Option<u8> {
        for i in (0..8).rev() {
            let pending = self.irr[i] & !self.isr[i];
            if pending != 0 {
                let bit = 31 - pending.leading_zeros();
                let vector = (i as u32 * 32 + bit) as u8;
                
                // Check if priority is high enough
                let priority = vector >> 4;
                let tpr_priority = (self.tpr >> 4) as u8;
                
                if priority > tpr_priority {
                    return Some(vector);
                }
            }
        }
        None
    }
    
    /// Get divisor from timer divide configuration
    fn get_timer_divisor(&self) -> u32 {
        match self.timer_divide & 0x0B {
            0x00 => 2,
            0x01 => 4,
            0x02 => 8,
            0x03 => 16,
            0x08 => 32,
            0x09 => 64,
            0x0A => 128,
            0x0B => 1,
            _ => 1,
        }
    }
    
    fn read_register(&self, offset: usize) -> u32 {
        match offset {
            reg::ID => self.id << 24,
            reg::VERSION => 0x00050014, // Version 0x14, max LVT entries = 5
            reg::TPR => self.tpr,
            reg::APR => 0, // TODO
            reg::PPR => {
                // Processor Priority = max(TPR, highest ISR vector >> 4)
                let mut highest = 0u8;
                for i in (0..8).rev() {
                    if self.isr[i] != 0 {
                        highest = (i as u8 * 32) + (31 - self.isr[i].leading_zeros()) as u8;
                        break;
                    }
                }
                let isr_priority = highest >> 4;
                let tpr_priority = (self.tpr >> 4) as u8;
                (isr_priority.max(tpr_priority) as u32) << 4
            }
            reg::LDR => self.ldr,
            reg::DFR => self.dfr,
            reg::SVR => self.svr,
            o if (reg::ISR_BASE..reg::ISR_BASE + 0x80).contains(&o) => {
                self.isr[(o - reg::ISR_BASE) / 0x10]
            }
            o if (reg::TMR_BASE..reg::TMR_BASE + 0x80).contains(&o) => {
                self.tmr[(o - reg::TMR_BASE) / 0x10]
            }
            o if (reg::IRR_BASE..reg::IRR_BASE + 0x80).contains(&o) => {
                self.irr[(o - reg::IRR_BASE) / 0x10]
            }
            reg::ESR => self.esr,
            reg::ICR_LOW => self.icr as u32,
            reg::ICR_HIGH => (self.icr >> 32) as u32,
            reg::TIMER_LVT => self.timer_lvt,
            reg::THERMAL_LVT => self.thermal_lvt,
            reg::PERF_LVT => self.perf_lvt,
            reg::LINT0_LVT => self.lint0_lvt,
            reg::LINT1_LVT => self.lint1_lvt,
            reg::ERROR_LVT => self.error_lvt,
            reg::TIMER_ICR => self.timer_initial,
            reg::TIMER_CCR => self.timer_current,
            reg::TIMER_DCR => self.timer_divide,
            _ => 0,
        }
    }
    
    fn write_register(&mut self, offset: usize, value: u32) {
        match offset {
            reg::ID => {
                self.id = (value >> 24) & 0xFF;
            }
            reg::TPR => {
                self.tpr = value & 0xFF;
            }
            reg::LDR => {
                self.ldr = value & 0xFF000000;
            }
            reg::DFR => {
                self.dfr = value | 0x0FFFFFFF;
            }
            reg::SVR => {
                self.svr = value & 0x1FF;
                self.enabled = (value & 0x100) != 0;
            }
            reg::EOI => {
                // Find highest priority in-service interrupt and clear it
                for i in (0..8).rev() {
                    if self.isr[i] != 0 {
                        let bit = 31 - self.isr[i].leading_zeros();
                        self.isr[i] &= !(1 << bit);
                        break;
                    }
                }
            }
            reg::ESR => {
                // Writing clears ESR
                self.esr = 0;
            }
            reg::ICR_LOW => {
                self.icr = (self.icr & 0xFFFFFFFF00000000) | (value as u64);
                // TODO: Handle IPI delivery
            }
            reg::ICR_HIGH => {
                self.icr = (self.icr & 0x00000000FFFFFFFF) | ((value as u64) << 32);
            }
            reg::TIMER_LVT => {
                self.timer_lvt = value;
            }
            reg::THERMAL_LVT => {
                self.thermal_lvt = value;
            }
            reg::PERF_LVT => {
                self.perf_lvt = value;
            }
            reg::LINT0_LVT => {
                self.lint0_lvt = value;
            }
            reg::LINT1_LVT => {
                self.lint1_lvt = value;
            }
            reg::ERROR_LVT => {
                self.error_lvt = value;
            }
            reg::TIMER_ICR => {
                self.timer_initial = value;
                self.timer_current = value;
            }
            reg::TIMER_DCR => {
                self.timer_divide = value & 0x0B;
            }
            _ => {}
        }
    }
}

impl Default for LocalApic {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Device for LocalApic {
    fn id(&self) -> DeviceId {
        DeviceId::LAPIC
    }
    
    fn name(&self) -> &str {
        "Local APIC"
    }
    
    fn reset(&mut self) {
        *self = Self::new(self.id);
    }
    
    fn handles_mmio(&self, addr: PhysAddr) -> bool {
        addr >= self.base && addr < self.base + Self::SIZE as u64
    }
    
    fn mmio_region(&self) -> Option<(PhysAddr, usize)> {
        Some((self.base, Self::SIZE))
    }
    
    fn mmio_read(&mut self, addr: PhysAddr, _access: IoAccess) -> u32 {
        let offset = (addr - self.base) as usize;
        self.read_register(offset)
    }
    
    fn mmio_write(&mut self, addr: PhysAddr, value: u32, _access: IoAccess) {
        let offset = (addr - self.base) as usize;
        self.write_register(offset, value);
    }
    
    fn tick(&mut self, cycles: u64) {
        if !self.enabled || self.timer_initial == 0 {
            return;
        }
        
        if (self.timer_lvt & lvt::MASKED) != 0 {
            return;
        }
        
        self.timer_cycles += cycles;
        let divisor = self.get_timer_divisor() as u64;
        let ticks = self.timer_cycles / divisor;
        self.timer_cycles %= divisor;
        
        if ticks > 0 && self.timer_current > 0 {
            if self.timer_current <= ticks as u32 {
                // Timer expired
                let vector = (self.timer_lvt & lvt::VECTOR_MASK) as u8;
                self.inject_interrupt(vector);
                
                if (self.timer_lvt & lvt::TIMER_PERIODIC) != 0 {
                    self.timer_current = self.timer_initial;
                } else {
                    self.timer_current = 0;
                }
            } else {
                self.timer_current -= ticks as u32;
            }
        }
    }
    
    fn has_interrupt(&self) -> bool {
        self.enabled && self.get_pending_interrupt().is_some()
    }
    
    fn interrupt_vector(&self) -> Option<u8> {
        if self.enabled {
            self.get_pending_interrupt()
        } else {
            None
        }
    }
    
    fn ack_interrupt(&mut self) {
        if let Some(vector) = self.get_pending_interrupt() {
            let idx = (vector / 32) as usize;
            let bit = 1u32 << (vector % 32);
            
            // Move from IRR to ISR
            self.irr[idx] &= !bit;
            self.isr[idx] |= bit;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_lapic_basic() {
        let mut lapic = LocalApic::new(0);
        
        // Read ID
        let id = lapic.read_register(reg::ID);
        assert_eq!(id >> 24, 0);
        
        // Read version
        let ver = lapic.read_register(reg::VERSION);
        assert_eq!(ver & 0xFF, 0x14);
    }
    
    #[test]
    fn test_lapic_interrupt() {
        let mut lapic = LocalApic::new(0);
        
        // Enable APIC
        lapic.write_register(reg::SVR, 0x1FF);
        
        // Inject interrupt
        lapic.inject_interrupt(0x30);
        assert!(lapic.has_interrupt());
        assert_eq!(lapic.interrupt_vector(), Some(0x30));
        
        // Acknowledge
        lapic.ack_interrupt();
        assert!(!lapic.has_interrupt());
        
        // Send EOI
        lapic.write_register(reg::EOI, 0);
    }
}
