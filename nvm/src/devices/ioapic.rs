//! I/O APIC Emulation
//!
//! The I/O APIC handles external interrupts and routes them to Local APICs.
//! MMIO base: 0xFEC0_0000 (default)

use std::any::Any;
use super::{Device, DeviceId, IoAccess};
use crate::memory::PhysAddr;

/// I/O APIC register addresses
pub mod reg {
    pub const IOREGSEL: usize = 0x00;
    pub const IOWIN: usize = 0x10;
}

/// I/O APIC indirect registers
pub mod indirect {
    pub const ID: u8 = 0x00;
    pub const VER: u8 = 0x01;
    pub const ARB: u8 = 0x02;
    pub const REDTBL_BASE: u8 = 0x10;
}

/// Redirection entry bits
pub mod redir {
    pub const VECTOR_MASK: u64 = 0xFF;
    pub const DELIVERY_MODE_MASK: u64 = 0x700;
    pub const DEST_MODE_LOGICAL: u64 = 0x800;
    pub const DELIVERY_PENDING: u64 = 0x1000;
    pub const POLARITY_LOW: u64 = 0x2000;
    pub const REMOTE_IRR: u64 = 0x4000;
    pub const TRIGGER_LEVEL: u64 = 0x8000;
    pub const MASKED: u64 = 0x10000;
    pub const DESTINATION_SHIFT: u64 = 56;
}

/// I/O APIC emulation
pub struct IoApic {
    /// APIC ID
    id: u32,
    /// MMIO base address
    base: PhysAddr,
    /// Current register select
    regsel: u8,
    /// Redirection table entries (24 entries)
    redirection: [u64; 24],
    /// Number of entries
    max_redir: u8,
    /// IRQ state (pending)
    irq_state: u32,
}

impl IoApic {
    pub const DEFAULT_BASE: PhysAddr = 0xFEC0_0000;
    pub const SIZE: usize = 0x20;
    
    pub fn new(id: u32) -> Self {
        let mut ioapic = Self {
            id,
            base: Self::DEFAULT_BASE,
            regsel: 0,
            redirection: [0; 24],
            max_redir: 23,
            irq_state: 0,
        };
        
        // Initialize all entries as masked
        for entry in &mut ioapic.redirection {
            *entry = redir::MASKED;
        }
        
        ioapic
    }
    
    /// Set MMIO base address
    pub fn set_base(&mut self, base: PhysAddr) {
        self.base = base;
    }
    
    /// Raise an IRQ line
    pub fn raise_irq(&mut self, irq: u8) {
        if irq < 24 {
            self.irq_state |= 1 << irq;
        }
    }
    
    /// Lower an IRQ line
    pub fn lower_irq(&mut self, irq: u8) {
        if irq < 24 {
            self.irq_state &= !(1 << irq);
        }
    }
    
    /// Get pending interrupt (destination APIC ID, vector)
    pub fn get_pending(&self) -> Option<(u8, u8)> {
        for irq in 0..24 {
            if (self.irq_state & (1 << irq)) != 0 {
                let entry = self.redirection[irq];
                
                // Check if masked
                if (entry & redir::MASKED) != 0 {
                    continue;
                }
                
                let vector = (entry & redir::VECTOR_MASK) as u8;
                let dest = (entry >> redir::DESTINATION_SHIFT) as u8;
                
                return Some((dest, vector));
            }
        }
        None
    }
    
    fn read_indirect(&self, reg: u8) -> u32 {
        match reg {
            indirect::ID => self.id << 24,
            indirect::VER => ((self.max_redir as u32) << 16) | 0x11, // Version 0x11
            indirect::ARB => self.id << 24,
            r if r >= indirect::REDTBL_BASE && r < indirect::REDTBL_BASE + 48 => {
                let idx = ((r - indirect::REDTBL_BASE) / 2) as usize;
                if idx < 24 {
                    let entry = self.redirection[idx];
                    if r % 2 == 0 {
                        entry as u32 // Low 32 bits
                    } else {
                        (entry >> 32) as u32 // High 32 bits
                    }
                } else {
                    0
                }
            }
            _ => 0,
        }
    }
    
    fn write_indirect(&mut self, reg: u8, value: u32) {
        match reg {
            indirect::ID => {
                self.id = (value >> 24) & 0x0F;
            }
            r if r >= indirect::REDTBL_BASE && r < indirect::REDTBL_BASE + 48 => {
                let idx = ((r - indirect::REDTBL_BASE) / 2) as usize;
                if idx < 24 {
                    if r % 2 == 0 {
                        // Low 32 bits
                        self.redirection[idx] = (self.redirection[idx] & 0xFFFFFFFF00000000)
                            | (value as u64);
                    } else {
                        // High 32 bits
                        self.redirection[idx] = (self.redirection[idx] & 0x00000000FFFFFFFF)
                            | ((value as u64) << 32);
                    }
                }
            }
            _ => {}
        }
    }
}

impl Default for IoApic {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Device for IoApic {
    fn id(&self) -> DeviceId {
        DeviceId::IOAPIC
    }
    
    fn name(&self) -> &str {
        "I/O APIC"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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
        match offset {
            reg::IOREGSEL => self.regsel as u32,
            reg::IOWIN => self.read_indirect(self.regsel),
            _ => 0,
        }
    }
    
    fn mmio_write(&mut self, addr: PhysAddr, value: u32, _access: IoAccess) {
        let offset = (addr - self.base) as usize;
        match offset {
            reg::IOREGSEL => {
                self.regsel = value as u8;
            }
            reg::IOWIN => {
                self.write_indirect(self.regsel, value);
            }
            _ => {}
        }
    }
    
    fn has_interrupt(&self) -> bool {
        self.get_pending().is_some()
    }
    
    fn interrupt_vector(&self) -> Option<u8> {
        self.get_pending().map(|(_, vec)| vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ioapic_basic() {
        let mut ioapic = IoApic::new(0);
        
        // Read ID
        ioapic.mmio_write(IoApic::DEFAULT_BASE, indirect::ID as u32, IoAccess::Dword);
        let id = ioapic.mmio_read(IoApic::DEFAULT_BASE + 0x10, IoAccess::Dword);
        assert_eq!(id >> 24, 0);
        
        // Read version
        ioapic.mmio_write(IoApic::DEFAULT_BASE, indirect::VER as u32, IoAccess::Dword);
        let ver = ioapic.mmio_read(IoApic::DEFAULT_BASE + 0x10, IoAccess::Dword);
        assert_eq!(ver & 0xFF, 0x11);
        assert_eq!((ver >> 16) & 0xFF, 23); // max redir
    }
    
    #[test]
    fn test_ioapic_redirection() {
        let mut ioapic = IoApic::new(0);
        
        // Configure IRQ 1 -> vector 0x21, destination 0
        let entry_low = 0x21u32; // Vector 0x21, unmasked
        let entry_high = 0u32;   // Destination 0
        
        // Write low part (register 0x12)
        ioapic.mmio_write(IoApic::DEFAULT_BASE, 0x12, IoAccess::Dword);
        ioapic.mmio_write(IoApic::DEFAULT_BASE + 0x10, entry_low, IoAccess::Dword);
        
        // Write high part (register 0x13)
        ioapic.mmio_write(IoApic::DEFAULT_BASE, 0x13, IoAccess::Dword);
        ioapic.mmio_write(IoApic::DEFAULT_BASE + 0x10, entry_high, IoAccess::Dword);
        
        // Raise IRQ 1
        ioapic.raise_irq(1);
        
        assert!(ioapic.has_interrupt());
        assert_eq!(ioapic.get_pending(), Some((0, 0x21)));
    }
}
