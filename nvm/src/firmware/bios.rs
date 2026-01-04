//! Legacy BIOS Implementation for NVM
//!
//! Provides a minimal BIOS implementation for legacy boot support.
//! This is similar to SeaBIOS but integrated into NVM.
//!
//! ## Memory Map (Real Mode)
//!
//! ```text
//! 0x00000 - 0x003FF   Interrupt Vector Table (IVT)
//! 0x00400 - 0x004FF   BIOS Data Area (BDA)
//! 0x00500 - 0x07BFF   Free conventional memory
//! 0x07C00 - 0x07DFF   Boot sector load address
//! 0x07E00 - 0x9FFFF   Free conventional memory
//! 0xA0000 - 0xBFFFF   Video memory
//! 0xC0000 - 0xC7FFF   Video BIOS ROM
//! 0xC8000 - 0xEFFFF   Option ROMs
//! 0xF0000 - 0xFFFFF   System BIOS ROM
//! ```

use super::{Firmware, FirmwareType, FirmwareLoadResult, FirmwareError, FirmwareResult, ServiceRegisters};
use crate::memory::PhysAddr;
use std::collections::HashMap;

/// BIOS configuration
#[derive(Debug, Clone)]
pub struct BiosConfig {
    /// Memory size in KB (for INT 12h)
    pub memory_kb: u32,
    /// Extended memory in KB (for INT 15h E801h)
    pub extended_memory_kb: u32,
    /// Number of hard disks
    pub num_hard_disks: u8,
    /// Number of floppy drives
    pub num_floppies: u8,
    /// Boot device order (0x00=floppy, 0x80=hdd, 0x81=cdrom)
    pub boot_order: Vec<u8>,
    /// Enable serial console
    pub serial_enabled: bool,
    /// COM port base address
    pub com_port: u16,
}

impl Default for BiosConfig {
    fn default() -> Self {
        Self {
            memory_kb: 640,
            extended_memory_kb: 0,
            num_hard_disks: 1,
            num_floppies: 0,
            boot_order: vec![0x80, 0x00],  // HDD first, then floppy
            serial_enabled: true,
            com_port: 0x3F8,
        }
    }
}

/// BIOS Data Area structure
#[derive(Debug, Clone, Default)]
pub struct BiosDataArea {
    /// COM port addresses (4 ports)
    pub com_ports: [u16; 4],
    /// LPT port addresses (4 ports)
    pub lpt_ports: [u16; 4],
    /// Equipment flags
    pub equipment_flags: u16,
    /// Memory size in KB
    pub memory_size_kb: u16,
    /// Keyboard buffer head
    pub kbd_buffer_head: u16,
    /// Keyboard buffer tail
    pub kbd_buffer_tail: u16,
    /// Video mode
    pub video_mode: u8,
    /// Screen columns
    pub screen_cols: u16,
    /// Screen rows (minus 1)
    pub screen_rows: u8,
    /// Cursor position
    pub cursor_pos: [(u8, u8); 8],
    /// Active page
    pub active_page: u8,
    /// Timer tick count
    pub timer_ticks: u32,
    /// Keyboard flags
    pub kbd_flags: u8,
}

impl BiosDataArea {
    /// Serialize BDA to bytes (at offset 0x400)
    pub fn to_bytes(&self) -> [u8; 256] {
        let mut data = [0u8; 256];
        
        // COM ports at 0x400-0x407
        for (i, &port) in self.com_ports.iter().enumerate() {
            let offset = i * 2;
            data[offset] = port as u8;
            data[offset + 1] = (port >> 8) as u8;
        }
        
        // LPT ports at 0x408-0x40F
        for (i, &port) in self.lpt_ports.iter().enumerate() {
            let offset = 8 + i * 2;
            data[offset] = port as u8;
            data[offset + 1] = (port >> 8) as u8;
        }
        
        // Equipment flags at 0x410
        data[0x10] = self.equipment_flags as u8;
        data[0x11] = (self.equipment_flags >> 8) as u8;
        
        // Memory size at 0x413
        data[0x13] = self.memory_size_kb as u8;
        data[0x14] = (self.memory_size_kb >> 8) as u8;
        
        // Video mode at 0x449
        data[0x49] = self.video_mode;
        
        // Screen columns at 0x44A
        data[0x4A] = self.screen_cols as u8;
        data[0x4B] = (self.screen_cols >> 8) as u8;
        
        // Timer ticks at 0x46C
        data[0x6C] = (self.timer_ticks & 0xFF) as u8;
        data[0x6D] = ((self.timer_ticks >> 8) & 0xFF) as u8;
        data[0x6E] = ((self.timer_ticks >> 16) & 0xFF) as u8;
        data[0x6F] = ((self.timer_ticks >> 24) & 0xFF) as u8;
        
        data
    }
}

/// BIOS services handler
pub struct BiosServices {
    /// Disk parameter tables
    disk_params: HashMap<u8, DiskParameters>,
    /// Current video mode
    video_mode: u8,
    /// Cursor position (row, col)
    cursor_pos: (u8, u8),
    /// Boot sector data (512 bytes)
    boot_sector: Option<[u8; 512]>,
}

#[derive(Debug, Clone)]
pub struct DiskParameters {
    pub cylinders: u16,
    pub heads: u8,
    pub sectors_per_track: u8,
    pub bytes_per_sector: u16,
    pub total_sectors: u32,
}

impl Default for BiosServices {
    fn default() -> Self {
        Self::new()
    }
}

impl BiosServices {
    pub fn new() -> Self {
        let mut disk_params = HashMap::new();
        
        // Default HDD parameters (80h)
        disk_params.insert(0x80, DiskParameters {
            cylinders: 1024,
            heads: 16,
            sectors_per_track: 63,
            bytes_per_sector: 512,
            total_sectors: 1024 * 16 * 63,
        });
        
        Self {
            disk_params,
            video_mode: 0x03,  // 80x25 text mode
            cursor_pos: (0, 0),
            boot_sector: None,
        }
    }
    
    /// Set boot sector data
    pub fn set_boot_sector(&mut self, data: &[u8]) {
        if data.len() >= 512 {
            let mut sector = [0u8; 512];
            sector.copy_from_slice(&data[..512]);
            self.boot_sector = Some(sector);
        }
    }
    
    /// Add disk
    pub fn add_disk(&mut self, drive: u8, params: DiskParameters) {
        self.disk_params.insert(drive, params);
    }
    
    /// Handle INT 10h - Video services
    pub fn handle_int10(&mut self, regs: &mut ServiceRegisters, memory: &mut [u8]) {
        let ah = ((regs.rax >> 8) & 0xFF) as u8;
        let al = (regs.rax & 0xFF) as u8;
        
        match ah {
            0x00 => {
                // Set video mode
                self.video_mode = al;
                regs.rax = (regs.rax & 0xFFFF0000) | (al as u64);
            }
            0x01 => {
                // Set cursor shape - ignore
            }
            0x02 => {
                // Set cursor position
                let row = ((regs.rdx >> 8) & 0xFF) as u8;
                let col = (regs.rdx & 0xFF) as u8;
                self.cursor_pos = (row, col);
            }
            0x03 => {
                // Get cursor position
                regs.rcx = 0x0607;  // Default cursor shape
                regs.rdx = ((self.cursor_pos.0 as u64) << 8) | (self.cursor_pos.1 as u64);
            }
            0x06 => {
                // Scroll window up
                let lines = al;
                let attr = ((regs.rbx >> 8) & 0xFF) as u8;
                let top = ((regs.rcx >> 8) & 0xFF) as usize;
                let left = (regs.rcx & 0xFF) as usize;
                let bottom = ((regs.rdx >> 8) & 0xFF) as usize;
                let right = (regs.rdx & 0xFF) as usize;
                
                // Scroll VGA text buffer
                self.scroll_up(memory, lines as usize, top, left, bottom, right, attr);
            }
            0x07 => {
                // Scroll window down - similar to 06h
            }
            0x09 | 0x0A => {
                // Write character at cursor
                let ch = al;
                let attr = (regs.rbx & 0xFF) as u8;
                let count = regs.rcx as usize;
                
                let vga_base = 0xB8000;
                let offset = (self.cursor_pos.0 as usize * 80 + self.cursor_pos.1 as usize) * 2;
                
                for i in 0..count {
                    if vga_base + offset + i * 2 + 1 < memory.len() {
                        memory[vga_base + offset + i * 2] = ch;
                        if ah == 0x09 {
                            memory[vga_base + offset + i * 2 + 1] = attr;
                        }
                    }
                }
            }
            0x0E => {
                // Teletype output
                let ch = al;
                self.teletype_output(memory, ch);
            }
            0x0F => {
                // Get video mode
                regs.rax = ((80u64) << 8) | (self.video_mode as u64);
                regs.rbx = (regs.rbx & 0xFFFFFF00) | 0; // Page 0
            }
            0x12 => {
                // Video subsystem configuration
                let bl = (regs.rbx & 0xFF) as u8;
                match bl {
                    0x10 => {
                        // Get EGA info
                        regs.rbx = 0x0003;  // 256K EGA, color
                        regs.rcx = 0x0009;  // Feature bits
                    }
                    _ => {}
                }
            }
            0x1A => {
                // Get/Set display combination code
                let al = (regs.rax & 0xFF) as u8;
                if al == 0x00 {
                    regs.rax = 0x001A;  // Function supported
                    regs.rbx = 0x0008;  // VGA with color analog display
                }
            }
            _ => {
                // Unknown function - set carry flag
                regs.rflags |= 1;
            }
        }
    }
    
    fn scroll_up(&self, memory: &mut [u8], lines: usize, top: usize, left: usize, 
                  bottom: usize, right: usize, attr: u8) {
        let vga_base = 0xB8000;
        let width = right - left + 1;
        
        if lines == 0 {
            // Clear window
            for row in top..=bottom {
                for col in left..=right {
                    let offset = vga_base + (row * 80 + col) * 2;
                    if offset + 1 < memory.len() {
                        memory[offset] = b' ';
                        memory[offset + 1] = attr;
                    }
                }
            }
        } else {
            // Scroll up
            for row in top..=bottom.saturating_sub(lines) {
                for col in left..=right {
                    let src = vga_base + ((row + lines) * 80 + col) * 2;
                    let dst = vga_base + (row * 80 + col) * 2;
                    if src + 1 < memory.len() && dst + 1 < memory.len() {
                        memory[dst] = memory[src];
                        memory[dst + 1] = memory[src + 1];
                    }
                }
            }
            // Clear new lines
            for row in (bottom + 1 - lines)..=bottom {
                for col in left..=right {
                    let offset = vga_base + (row * 80 + col) * 2;
                    if offset + 1 < memory.len() {
                        memory[offset] = b' ';
                        memory[offset + 1] = attr;
                    }
                }
            }
        }
    }
    
    fn teletype_output(&mut self, memory: &mut [u8], ch: u8) {
        match ch {
            0x07 => {} // Bell - ignore
            0x08 => {
                // Backspace
                if self.cursor_pos.1 > 0 {
                    self.cursor_pos.1 -= 1;
                }
            }
            0x09 => {
                // Tab
                self.cursor_pos.1 = ((self.cursor_pos.1 / 8) + 1) * 8;
                if self.cursor_pos.1 >= 80 {
                    self.cursor_pos.1 = 0;
                    self.cursor_pos.0 += 1;
                }
            }
            0x0A => {
                // Line feed
                self.cursor_pos.0 += 1;
            }
            0x0D => {
                // Carriage return
                self.cursor_pos.1 = 0;
            }
            _ => {
                // Print character
                let vga_base = 0xB8000;
                let offset = (self.cursor_pos.0 as usize * 80 + self.cursor_pos.1 as usize) * 2;
                if vga_base + offset + 1 < memory.len() {
                    memory[vga_base + offset] = ch;
                    memory[vga_base + offset + 1] = 0x07; // Light gray on black
                }
                self.cursor_pos.1 += 1;
                if self.cursor_pos.1 >= 80 {
                    self.cursor_pos.1 = 0;
                    self.cursor_pos.0 += 1;
                }
            }
        }
        
        // Handle scroll
        if self.cursor_pos.0 >= 25 {
            self.scroll_up(memory, 1, 0, 0, 24, 79, 0x07);
            self.cursor_pos.0 = 24;
        }
    }
    
    /// Handle INT 13h - Disk services
    pub fn handle_int13(&mut self, regs: &mut ServiceRegisters, memory: &mut [u8]) {
        let ah = ((regs.rax >> 8) & 0xFF) as u8;
        let dl = (regs.rdx & 0xFF) as u8;
        
        match ah {
            0x00 => {
                // Reset disk system
                regs.rax &= 0xFFFF00FF;  // AH = 0 (success)
                regs.rflags &= !1;  // Clear CF
            }
            0x02 => {
                // Read sectors
                let sectors = (regs.rax & 0xFF) as usize;
                let cylinder = ((regs.rcx >> 8) & 0xFF) as u16 | (((regs.rcx >> 6) & 0x03) as u16) << 8;
                let sector = (regs.rcx & 0x3F) as u8;
                let head = ((regs.rdx >> 8) & 0xFF) as u8;
                let buffer = ((regs.rsi & 0xFFFF) as usize) + (((regs.rdi >> 16) & 0xFFFF) as usize * 16);
                
                if let Some(params) = self.disk_params.get(&dl) {
                    // Calculate LBA
                    let lba = ((cylinder as u32) * (params.heads as u32) + (head as u32)) 
                              * (params.sectors_per_track as u32) + (sector as u32) - 1;
                    
                    // For boot sector (LBA 0), use stored boot sector
                    if lba == 0 && sectors >= 1 {
                        if let Some(ref boot) = self.boot_sector {
                            let dest_end = buffer + 512;
                            if dest_end <= memory.len() {
                                memory[buffer..dest_end].copy_from_slice(boot);
                                regs.rax = (regs.rax & 0xFFFF00FF) | 1;  // 1 sector read, AH=0
                                regs.rflags &= !1;  // Clear CF
                                return;
                            }
                        }
                    }
                    
                    // Return success with 0 sectors for now
                    regs.rax = (regs.rax & 0xFFFF00FF) | (sectors as u64);
                    regs.rflags &= !1;
                } else {
                    // Drive not found
                    regs.rax = (regs.rax & 0xFFFF00FF) | 0x0100;  // AH = 01 (bad command)
                    regs.rflags |= 1;  // Set CF
                }
            }
            0x08 => {
                // Get drive parameters
                if let Some(params) = self.disk_params.get(&dl) {
                    regs.rax = 0;  // AH = 0 (success)
                    regs.rbx = 0;  // Drive type
                    let cyl = (params.cylinders - 1) as u64;
                    regs.rcx = ((cyl & 0xFF) << 8) 
                              | (((cyl >> 8) & 0x03) << 6)
                              | (params.sectors_per_track as u64 & 0x3F);
                    regs.rdx = ((params.heads - 1) as u64) << 8 
                              | (if dl >= 0x80 { self.disk_params.len() as u64 } else { 0 });
                    regs.rflags &= !1;
                } else {
                    regs.rax = 0x0100;  // AH = 01 (bad command)
                    regs.rflags |= 1;
                }
            }
            0x15 => {
                // Get disk type
                if dl >= 0x80 && self.disk_params.contains_key(&dl) {
                    regs.rax = (regs.rax & 0xFFFF00FF) | 0x0300;  // AH = 03 (HDD)
                    if let Some(params) = self.disk_params.get(&dl) {
                        let sectors = params.total_sectors as u64;
                        regs.rcx = (sectors >> 16) & 0xFFFF;
                        regs.rdx = sectors & 0xFFFF;
                    }
                    regs.rflags &= !1;
                } else {
                    regs.rax = 0;  // No drive
                    regs.rflags |= 1;
                }
            }
            0x41 => {
                // Check extensions present
                if (regs.rbx & 0xFFFF) == 0x55AA {
                    regs.rax = (regs.rax & 0xFFFF00FF) | 0x0100;  // Version 1.0
                    regs.rbx = (regs.rbx & 0xFFFF0000) | 0xAA55;
                    regs.rcx = 0x0001;  // Extended disk access supported
                    regs.rflags &= !1;
                } else {
                    regs.rflags |= 1;
                }
            }
            _ => {
                // Unknown function
                regs.rax = (regs.rax & 0xFFFF00FF) | 0x0100;
                regs.rflags |= 1;
            }
        }
    }
    
    /// Handle INT 15h - System services
    pub fn handle_int15(&self, regs: &mut ServiceRegisters, config: &BiosConfig) {
        let ah = ((regs.rax >> 8) & 0xFF) as u8;
        let ax = (regs.rax & 0xFFFF) as u16;
        
        match ax {
            0xE820 => {
                // Get memory map
                let continuation = regs.rbx as u32;
                let buffer = regs.rdi as usize;
                
                // Simplified memory map entries
                let entries = [
                    // Base, Length, Type (1=usable, 2=reserved, 3=ACPI reclaim)
                    (0x0000_0000u64, 0x0009_FC00u64, 1u32),  // Conventional memory
                    (0x0009_FC00u64, 0x0000_0400u64, 2u32),  // Extended BIOS data
                    (0x000F_0000u64, 0x0001_0000u64, 2u32),  // BIOS ROM
                    (0x0010_0000u64, (config.extended_memory_kb as u64) * 1024, 1u32),  // Extended memory
                    (0xFEC0_0000u64, 0x0001_0000u64, 2u32),  // I/O APIC
                    (0xFEE0_0000u64, 0x0001_0000u64, 2u32),  // Local APIC
                    (0xFFFC_0000u64, 0x0004_0000u64, 2u32),  // High BIOS
                ];
                
                if (continuation as usize) < entries.len() {
                    let (base, length, mem_type) = entries[continuation as usize];
                    // Write E820 entry (20 bytes)
                    // This would write to guest memory at 'buffer' offset
                    let _ = (buffer, base, length, mem_type);
                    
                    regs.rax = (regs.rax & 0xFFFF0000) | 0x534D4150;  // "SMAP"
                    regs.rcx = 20;
                    regs.rbx = if continuation as usize + 1 < entries.len() {
                        continuation as u64 + 1
                    } else {
                        0  // End of list
                    };
                    regs.rflags &= !1;  // Clear CF
                } else {
                    regs.rflags |= 1;  // Set CF (end of list)
                }
            }
            0xE801 => {
                // Get extended memory size
                let ext_kb = config.extended_memory_kb.min(0xFFFF) as u64;
                regs.rax = ext_kb.min(0x3C00);  // 1-16MB in KB
                regs.rbx = ext_kb.saturating_sub(0x3C00) / 64;  // >16MB in 64KB blocks
                regs.rcx = regs.rax;
                regs.rdx = regs.rbx;
                regs.rflags &= !1;
            }
            0x2400 => {
                // Disable A20 gate
                regs.rflags &= !1;
            }
            0x2401 => {
                // Enable A20 gate
                regs.rflags &= !1;
            }
            0x2403 => {
                // Query A20 gate status
                regs.rax = (regs.rax & 0xFFFF00FF) | 0x0100;  // A20 supported
                regs.rbx = 0x0003;  // Keyboard controller + Fast A20
                regs.rflags &= !1;
            }
            _ => {
                match ah {
                    0x88 => {
                        // Get extended memory size (legacy)
                        regs.rax = (config.extended_memory_kb.min(0xFFFF)) as u64;
                        regs.rflags &= !1;
                    }
                    0xC0 => {
                        // Get configuration
                        regs.rax = (regs.rax & 0xFFFF00FF);  // AH = 0
                        regs.rflags &= !1;
                    }
                    _ => {
                        // Unknown function
                        regs.rax = (regs.rax & 0xFFFF00FF) | 0x8600;  // AH = 86 (unsupported)
                        regs.rflags |= 1;
                    }
                }
            }
        }
    }
    
    /// Handle INT 16h - Keyboard services
    pub fn handle_int16(&self, regs: &mut ServiceRegisters) {
        let ah = ((regs.rax >> 8) & 0xFF) as u8;
        
        match ah {
            0x00 | 0x10 => {
                // Wait for keypress - return no key for now
                regs.rax = 0;
            }
            0x01 | 0x11 => {
                // Check for keypress
                regs.rflags |= 0x40;  // Set ZF (no key available)
            }
            0x02 | 0x12 => {
                // Get shift flags
                regs.rax = (regs.rax & 0xFFFFFF00);  // No shift keys pressed
            }
            _ => {}
        }
    }
    
    /// Handle INT 12h - Get conventional memory size
    pub fn handle_int12(&self, regs: &mut ServiceRegisters, config: &BiosConfig) {
        regs.rax = config.memory_kb as u64;
    }
}

/// Legacy BIOS firmware implementation
pub struct Bios {
    config: BiosConfig,
    services: BiosServices,
    bda: BiosDataArea,
    version: String,
}

impl Bios {
    pub fn new(config: BiosConfig) -> Self {
        let mut bda = BiosDataArea::default();
        bda.memory_size_kb = config.memory_kb as u16;
        bda.video_mode = 0x03;
        bda.screen_cols = 80;
        bda.screen_rows = 24;
        
        if config.serial_enabled {
            bda.com_ports[0] = config.com_port;
        }
        
        Self {
            config,
            services: BiosServices::new(),
            bda,
            version: String::from("NexaBIOS 1.0"),
        }
    }
    
    /// Set boot sector to load
    pub fn set_boot_sector(&mut self, data: &[u8]) {
        self.services.set_boot_sector(data);
    }
    
    /// Add disk drive
    pub fn add_disk(&mut self, drive: u8, params: DiskParameters) {
        self.services.add_disk(drive, params);
    }
    
    /// Initialize IVT (Interrupt Vector Table) in memory
    fn init_ivt(&self, memory: &mut [u8]) {
        // Each IVT entry is 4 bytes: offset (2) + segment (2)
        // Point all vectors to a default handler in high memory
        // The handler is at F000:FF53 (typical BIOS default)
        
        for i in 0..256 {
            let offset = i * 4;
            if offset + 3 < memory.len() && offset + 3 < 0x400 {
                memory[offset] = 0x53;      // Offset low
                memory[offset + 1] = 0xFF;  // Offset high
                memory[offset + 2] = 0x00;  // Segment low
                memory[offset + 3] = 0xF0;  // Segment high
            }
        }
        
        // Set specific vectors
        // INT 10h - Video at F000:F065
        self.set_ivt_entry(memory, 0x10, 0xF065, 0xF000);
        // INT 13h - Disk at F000:EC59
        self.set_ivt_entry(memory, 0x13, 0xEC59, 0xF000);
        // INT 15h - System at F000:F859
        self.set_ivt_entry(memory, 0x15, 0xF859, 0xF000);
        // INT 16h - Keyboard at F000:E82E
        self.set_ivt_entry(memory, 0x16, 0xE82E, 0xF000);
        // INT 12h - Memory at F000:F841
        self.set_ivt_entry(memory, 0x12, 0xF841, 0xF000);
        // INT 19h - Bootstrap at F000:E6F2
        self.set_ivt_entry(memory, 0x19, 0xE6F2, 0xF000);
    }
    
    fn set_ivt_entry(&self, memory: &mut [u8], vector: usize, offset: u16, segment: u16) {
        let addr = vector * 4;
        if addr + 3 < memory.len() {
            memory[addr] = offset as u8;
            memory[addr + 1] = (offset >> 8) as u8;
            memory[addr + 2] = segment as u8;
            memory[addr + 3] = (segment >> 8) as u8;
        }
    }
    
    /// Initialize BIOS ROM area with minimal code
    fn init_bios_rom(&self, memory: &mut [u8]) {
        let bios_base = 0xF0000;
        
        // Clear BIOS area
        if bios_base + 0x10000 <= memory.len() {
            for i in 0..0x10000 {
                memory[bios_base + i] = 0;
            }
        }
        
        // Write BIOS signature at end (FFFF0)
        let reset_vec = 0xFFFF0;
        if reset_vec + 5 < memory.len() {
            // JMP F000:E05B (typical BIOS entry point)
            memory[reset_vec] = 0xEA;      // Far JMP
            memory[reset_vec + 1] = 0x5B;  // Offset low
            memory[reset_vec + 2] = 0xE0;  // Offset high
            memory[reset_vec + 3] = 0x00;  // Segment low
            memory[reset_vec + 4] = 0xF0;  // Segment high
        }
        
        // BIOS date at FFFF5
        let date_offset = 0xFFFF5;
        if date_offset + 8 < memory.len() {
            let date = b"01/04/26";  // MM/DD/YY
            memory[date_offset..date_offset + 8].copy_from_slice(date);
        }
        
        // System model byte at FFFFE
        if 0xFFFFE < memory.len() {
            memory[0xFFFFE] = 0xFC;  // AT compatible
        }
        
        // BIOS entry point at F000:E05B - POST initialization including PIC
        let entry = 0xFE05B;
        if entry + 80 < memory.len() {
            // Real x86 BIOS POST code: initialize hardware via OUT instructions
            let code: &[u8] = &[
                0xFA,             // CLI - disable interrupts during init
                0xFC,             // CLD - clear direction flag
                
                // ========== Initialize segment registers ==========
                0x31, 0xC0,       // XOR AX, AX
                0x8E, 0xD8,       // MOV DS, AX
                0x8E, 0xC0,       // MOV ES, AX
                0x8E, 0xD0,       // MOV SS, AX
                0xBC, 0x00, 0x7C, // MOV SP, 7C00h
                
                // ========== Initialize 8259 PIC (Master) ==========
                // ICW1: edge triggered, cascade, ICW4 needed
                0xB0, 0x11,       // MOV AL, 11h
                0xE6, 0x20,       // OUT 20h, AL
                // ICW2: vector base 08h (IRQ0 = INT 08h)
                0xB0, 0x08,       // MOV AL, 08h
                0xE6, 0x21,       // OUT 21h, AL
                // ICW3: slave on IRQ2
                0xB0, 0x04,       // MOV AL, 04h
                0xE6, 0x21,       // OUT 21h, AL
                // ICW4: 8086 mode, normal EOI
                0xB0, 0x01,       // MOV AL, 01h
                0xE6, 0x21,       // OUT 21h, AL
                
                // ========== Initialize 8259 PIC (Slave) ==========
                // ICW1: edge triggered, cascade, ICW4 needed
                0xB0, 0x11,       // MOV AL, 11h
                0xE6, 0xA0,       // OUT A0h, AL
                // ICW2: vector base 70h (IRQ8 = INT 70h)
                0xB0, 0x70,       // MOV AL, 70h
                0xE6, 0xA1,       // OUT A1h, AL
                // ICW3: cascade identity (IRQ2)
                0xB0, 0x02,       // MOV AL, 02h
                0xE6, 0xA1,       // OUT A1h, AL
                // ICW4: 8086 mode, normal EOI
                0xB0, 0x01,       // MOV AL, 01h
                0xE6, 0xA1,       // OUT A1h, AL
                
                // ========== Unmask all IRQs ==========
                // Master PIC: unmask all (keyboard IRQ1, timer IRQ0, etc)
                0xB0, 0x00,       // MOV AL, 00h
                0xE6, 0x21,       // OUT 21h, AL
                // Slave PIC: unmask all
                0xB0, 0x00,       // MOV AL, 00h
                0xE6, 0xA1,       // OUT A1h, AL
                
                // ========== Enable interrupts and boot ==========
                0xFB,             // STI - enable interrupts
                // Try to boot from first device
                0xCD, 0x19,       // INT 19h (Bootstrap)
                0xF4,             // HLT (if boot fails)
                0xEB, 0xFD,       // JMP -3 (loop on HLT)
            ];
            memory[entry..entry + code.len()].copy_from_slice(code);
        }
        
        // INT 19h handler - load boot sector
        let int19_handler = 0xFE6F2;
        if int19_handler + 32 < memory.len() {
            let code: &[u8] = &[
                0xB8, 0x01, 0x02, // MOV AX, 0201h (read 1 sector)
                0xBB, 0x00, 0x7C, // MOV BX, 7C00h (destination)
                0xB1, 0x01,       // MOV CL, 1 (sector 1)
                0xB5, 0x00,       // MOV CH, 0 (cylinder 0)
                0xB6, 0x00,       // MOV DH, 0 (head 0)
                0xB2, 0x80,       // MOV DL, 80h (first HDD)
                0xCD, 0x13,       // INT 13h
                0x72, 0x08,       // JC fail
                0x81, 0x3E, 0xFE, 0x7D, 0x55, 0xAA, // CMP [7DFE], AA55h
                0x75, 0x01,       // JNE fail
                0xEA, 0x00, 0x7C, 0x00, 0x00, // JMP 0000:7C00
                0xF4,             // HLT (fail)
                0xEB, 0xFD,       // JMP -3
            ];
            memory[int19_handler..int19_handler + code.len()].copy_from_slice(code);
        }
    }
    
    /// Initialize ACPI tables for modern OS support
    fn init_acpi_tables(&self, memory: &mut [u8]) {
        // ACPI RSDP at E0000 (EBDA or ROM area)
        let rsdp_addr = 0xE0000usize;
        if rsdp_addr + 36 >= memory.len() {
            return;
        }
        
        // RSDP signature "RSD PTR "
        let rsdp: &[u8] = &[
            b'R', b'S', b'D', b' ', b'P', b'T', b'R', b' ', // Signature
            0x00,  // Checksum (to be calculated)
            b'N', b'E', b'X', b'A', b' ', b' ', // OEM ID
            0x02,  // Revision (ACPI 2.0)
            0x00, 0x00, 0x00, 0x00, // RSDT Address (will set below)
            0x24, 0x00, 0x00, 0x00, // Length (36 bytes for RSDP 2.0)
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // XSDT Address
            0x00, // Extended checksum
            0x00, 0x00, 0x00, // Reserved
        ];
        memory[rsdp_addr..rsdp_addr + rsdp.len()].copy_from_slice(rsdp);
        
        // Calculate checksum for first 20 bytes
        let mut sum: u8 = 0;
        for i in 0..20 {
            sum = sum.wrapping_add(memory[rsdp_addr + i]);
        }
        memory[rsdp_addr + 8] = (256 - sum as u16) as u8;
    }
    
    /// Initialize MP (MultiProcessor) tables for SMP support
    fn init_mp_tables(&self, memory: &mut [u8]) {
        // MP Floating Pointer Structure at 0x9FC00 (end of conventional memory)
        let mp_addr = 0x9FC00usize;
        if mp_addr + 16 >= memory.len() {
            return;
        }
        
        // MP floating pointer signature "_MP_"
        let mp_fps: &[u8] = &[
            b'_', b'M', b'P', b'_',  // Signature
            0x00, 0x00, 0x00, 0x00,  // Physical pointer (0 = default config)
            0x10,                    // Length (16 bytes)
            0x04,                    // Spec revision 1.4
            0x00,                    // Checksum (calculated below)
            0x00,                    // MP feature byte 1 (0 = MP table present)
            0x00, 0x00, 0x00, 0x00,  // MP feature bytes 2-5
        ];
        memory[mp_addr..mp_addr + mp_fps.len()].copy_from_slice(mp_fps);
        
        // Calculate checksum
        let mut sum: u8 = 0;
        for i in 0..16 {
            sum = sum.wrapping_add(memory[mp_addr + i]);
        }
        memory[mp_addr + 10] = (256 - sum as u16) as u8;
    }
}

impl Firmware for Bios {
    fn firmware_type(&self) -> FirmwareType {
        FirmwareType::Bios
    }
    
    fn load(&mut self, memory: &mut [u8]) -> FirmwareResult<FirmwareLoadResult> {
        if memory.len() < 0x100000 {
            return Err(FirmwareError::InvalidMemory(
                "Need at least 1MB of memory for BIOS".to_string()
            ));
        }
        
        // =========== Phase 1: Initialize low memory structures ===========
        
        // Initialize IVT at 0x0000
        self.init_ivt(memory);
        
        // Initialize BDA at 0x0400
        let bda_bytes = self.bda.to_bytes();
        memory[0x400..0x500].copy_from_slice(&bda_bytes);
        
        // =========== Phase 2: Initialize EBDA (Extended BIOS Data Area) ===========
        // EBDA typically at top of conventional memory (9FC00-9FFFF)
        let ebda_segment = 0x9FC0u16;
        // Write EBDA segment to BDA at offset 0x0E
        memory[0x40E] = (ebda_segment & 0xFF) as u8;
        memory[0x40F] = ((ebda_segment >> 8) & 0xFF) as u8;
        
        // =========== Phase 3: Initialize BIOS ROM area ===========
        self.init_bios_rom(memory);
        
        // =========== Phase 4: Initialize video memory ===========
        // Clear VGA text buffer at 0xB8000
        let vga_base = 0xB8000;
        if vga_base + 0x1000 < memory.len() {
            for i in 0..(80 * 25) {
                memory[vga_base + i * 2] = b' ';
                memory[vga_base + i * 2 + 1] = 0x07;  // Light gray on black
            }
            // Display POST message
            let post_msg = b"NexaBIOS v1.0 - POST Complete";
            for (i, &ch) in post_msg.iter().enumerate() {
                memory[vga_base + i * 2] = ch;
                memory[vga_base + i * 2 + 1] = 0x0F;  // Bright white
            }
        }
        
        // =========== Phase 5: Setup ACPI tables (if space available) ===========
        self.init_acpi_tables(memory);
        
        // =========== Phase 6: Setup MP tables for SMP ===========
        self.init_mp_tables(memory);
        
        // Entry point is at FFFF:0000 (reset vector)
        // Which points to F000:E05B
        Ok(FirmwareLoadResult {
            entry_point: 0xFFFF0,  // Reset vector  
            stack_pointer: 0x7C00, // Traditional stack location
            code_segment: 0xF000,
            real_mode: true,
        })
    }
    
    fn handle_service(&mut self, memory: &mut [u8], regs: &mut ServiceRegisters) -> FirmwareResult<()> {
        // Determine which interrupt based on CS:IP
        // For simplicity, we use the vector number passed in a reserved register
        let int_num = ((regs.r11 >> 8) & 0xFF) as u8;
        
        match int_num {
            0x10 => self.services.handle_int10(regs, memory),
            0x12 => self.services.handle_int12(regs, &self.config),
            0x13 => self.services.handle_int13(regs, memory),
            0x15 => self.services.handle_int15(regs, &self.config),
            0x16 => self.services.handle_int16(regs),
            _ => {
                // Unknown interrupt
                regs.rflags |= 1;  // Set carry flag
            }
        }
        
        Ok(())
    }
    
    fn reset(&mut self) {
        self.services = BiosServices::new();
        self.bda = BiosDataArea::default();
        self.bda.memory_size_kb = self.config.memory_kb as u16;
    }
    
    fn version(&self) -> &str {
        &self.version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_bios_creation() {
        let bios = Bios::new(BiosConfig::default());
        assert_eq!(bios.firmware_type(), FirmwareType::Bios);
        assert!(bios.version().contains("NexaBIOS"));
    }
    
    #[test]
    fn test_bios_load() {
        let mut bios = Bios::new(BiosConfig {
            memory_kb: 640,
            extended_memory_kb: 16384,
            ..Default::default()
        });
        
        let mut memory = vec![0u8; 16 * 1024 * 1024];  // 16MB
        let result = bios.load(&mut memory).unwrap();
        
        assert!(result.real_mode);
        assert_eq!(result.code_segment, 0xF000);
        
        // Check reset vector
        assert_eq!(memory[0xFFFF0], 0xEA);  // Far JMP
    }
}
