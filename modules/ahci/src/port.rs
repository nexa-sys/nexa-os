//! AHCI Port Operations

use crate::regs::*;
use crate::fis::*;
use crate::{kmod_mmio_read32, kmod_mmio_write32, kmod_zalloc, kmod_dealloc, kmod_fence};
use crate::kmod_virt_to_phys;
use core::ptr;

/// AHCI Port state
#[repr(C)]
pub struct AhciPort {
    pub port_base: u64,       // MMIO base for this port
    pub port_num: u8,
    pub present: bool,
    pub atapi: bool,
    pub total_sectors: u64,
    pub sector_size: u32,
    pub model: [u8; 41],
    
    // DMA memory regions
    pub clb: *mut u8,         // Command List Base (1KB aligned)
    pub fb: *mut u8,          // FIS Base (256B aligned)
    pub ctba: *mut u8,        // Command Table Base (128B aligned)
    pub lock: u64,
}

impl AhciPort {
    /// Read port register
    #[inline]
    pub fn read(&self, off: u64) -> u32 {
        unsafe { kmod_mmio_read32(self.port_base + off) }
    }

    /// Write port register
    #[inline]
    pub fn write(&self, off: u64, val: u32) {
        unsafe { kmod_mmio_write32(self.port_base + off, val) }
    }

    /// Stop command engine
    pub fn stop_cmd(&self) {
        let mut cmd = self.read(PORT_CMD);
        cmd &= !(PORT_CMD_ST | PORT_CMD_FRE);
        self.write(PORT_CMD, cmd);

        // Wait until FR and CR are cleared
        for _ in 0..TIMEOUT {
            let cmd = self.read(PORT_CMD);
            if (cmd & (PORT_CMD_FR | PORT_CMD_CR)) == 0 {
                return;
            }
            core::hint::spin_loop();
        }
    }

    /// Start command engine
    pub fn start_cmd(&self) {
        // Wait for CR to clear
        for _ in 0..TIMEOUT {
            if (self.read(PORT_CMD) & PORT_CMD_CR) == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        let mut cmd = self.read(PORT_CMD);
        cmd |= PORT_CMD_FRE | PORT_CMD_ST;
        self.write(PORT_CMD, cmd);
    }

    /// Wait for command completion
    pub fn wait_complete(&self, slot: u8) -> i32 {
        let mask = 1u32 << slot;
        for _ in 0..TIMEOUT {
            let ci = self.read(PORT_CI);
            if (ci & mask) == 0 {
                // Check for errors
                let tfd = self.read(PORT_TFD);
                if (tfd & PORT_TFD_ERR) != 0 {
                    return -2;
                }
                return 0;
            }
            let is = self.read(PORT_IS);
            if (is & 0x40000000) != 0 { // Task file error
                self.write(PORT_IS, is);
                return -3;
            }
            core::hint::spin_loop();
        }
        -1 // Timeout
    }

    /// Issue a command on slot 0
    pub fn issue_cmd(&self, slot: u8) {
        self.write(PORT_CI, 1u32 << slot);
    }

    /// Check if device is busy
    pub fn is_busy(&self) -> bool {
        let tfd = self.read(PORT_TFD);
        (tfd & (PORT_TFD_BSY | PORT_TFD_DRQ)) != 0
    }

    /// Initialize port memory structures
    pub fn init_memory(&mut self) -> i32 {
        self.stop_cmd();

        // Allocate command list (1KB, 1KB aligned)
        self.clb = unsafe { kmod_zalloc(1024, 1024) };
        if self.clb.is_null() {
            return -1;
        }

        // Allocate FIS buffer (256B, 256B aligned)
        self.fb = unsafe { kmod_zalloc(256, 256) };
        if self.fb.is_null() {
            return -1;
        }

        // Allocate command table (256B per slot, 128B aligned)
        self.ctba = unsafe { kmod_zalloc(256, 128) };
        if self.ctba.is_null() {
            return -1;
        }

        // Set up port registers
        let clb_phys = unsafe { kmod_virt_to_phys(self.clb as u64) };
        let fb_phys = unsafe { kmod_virt_to_phys(self.fb as u64) };
        let ctba_phys = unsafe { kmod_virt_to_phys(self.ctba as u64) };

        self.write(PORT_CLB, clb_phys as u32);
        self.write(PORT_CLBU, (clb_phys >> 32) as u32);
        self.write(PORT_FB, fb_phys as u32);
        self.write(PORT_FBU, (fb_phys >> 32) as u32);

        // Set command header to point to command table
        let hdr = self.clb as *mut CmdHeader;
        unsafe {
            (*hdr).ctba = ctba_phys as u32;
            (*hdr).ctbau = (ctba_phys >> 32) as u32;
        }

        // Clear pending interrupts
        self.write(PORT_IS, 0xFFFFFFFF);
        self.write(PORT_SERR, 0xFFFFFFFF);

        self.start_cmd();
        0
    }

    /// Identify device
    pub fn identify(&mut self) -> i32 {
        // Wait for not busy
        for _ in 0..TIMEOUT {
            if !self.is_busy() { break; }
            core::hint::spin_loop();
        }

        // Allocate identify buffer
        let buf = unsafe { kmod_zalloc(512, 2) };
        if buf.is_null() {
            return -1;
        }
        let buf_phys = unsafe { kmod_virt_to_phys(buf as u64) };

        // Set up command
        let hdr = self.clb as *mut CmdHeader;
        unsafe {
            (*hdr).flags = 5 | (1 << 5); // CFL=5, ATAPI bit
            (*hdr).prdtl = 1;
            (*hdr).prdbc = 0;
        }

        // Set up command table
        let tbl = self.ctba as *mut CmdTable;
        let fis = unsafe { &mut *((*tbl).cfis.as_mut_ptr() as *mut FisRegH2D) };
        *fis = FisRegH2D::new_command(ATA_CMD_IDENTIFY);

        // Set up PRDT
        unsafe {
            (*tbl).prdt[0].dba = buf_phys as u32;
            (*tbl).prdt[0].dbau = (buf_phys >> 32) as u32;
            (*tbl).prdt[0].dbc_i = 511; // 512 bytes - 1
        }

        unsafe { kmod_fence(); }
        self.issue_cmd(0);
        let result = self.wait_complete(0);

        if result == 0 {
            // Parse identify data
            let words = buf as *const u16;
            unsafe {
                // Words 100-103: LBA48 sector count
                self.total_sectors = 
                    (ptr::read_volatile(words.add(100)) as u64) |
                    ((ptr::read_volatile(words.add(101)) as u64) << 16) |
                    ((ptr::read_volatile(words.add(102)) as u64) << 32) |
                    ((ptr::read_volatile(words.add(103)) as u64) << 48);

                if self.total_sectors == 0 {
                    // LBA28 fallback
                    self.total_sectors = 
                        (ptr::read_volatile(words.add(60)) as u64) |
                        ((ptr::read_volatile(words.add(61)) as u64) << 16);
                }

                // Model name (words 27-46)
                for i in 0..20 {
                    let w = ptr::read_volatile(words.add(27 + i));
                    self.model[i * 2] = (w >> 8) as u8;
                    self.model[i * 2 + 1] = (w & 0xFF) as u8;
                }
                self.model[40] = 0;
            }
            self.sector_size = SECTOR_SIZE;
            self.present = true;
        }

        unsafe { kmod_dealloc(buf, 512, 2); }
        result
    }

    /// Read sectors via DMA
    pub fn read_sectors(&mut self, lba: u64, count: u32, buf: *mut u8) -> i32 {
        if count == 0 || count > 128 {
            return -1;
        }

        let buf_phys = unsafe { kmod_virt_to_phys(buf as u64) };
        let bytes = count * self.sector_size;

        // Set up command header
        let hdr = self.clb as *mut CmdHeader;
        unsafe {
            (*hdr).flags = 5; // CFL=5 DWORDs
            (*hdr).prdtl = 1;
            (*hdr).prdbc = 0;
        }

        // Set up FIS
        let tbl = self.ctba as *mut CmdTable;
        let fis = unsafe { &mut *((*tbl).cfis.as_mut_ptr() as *mut FisRegH2D) };
        *fis = FisRegH2D::new_command(ATA_CMD_READ_DMA_EX);
        fis.set_lba(lba, count as u16);

        // Set up PRDT
        unsafe {
            (*tbl).prdt[0].dba = buf_phys as u32;
            (*tbl).prdt[0].dbau = (buf_phys >> 32) as u32;
            (*tbl).prdt[0].dbc_i = bytes - 1;
            kmod_fence();
        }

        self.issue_cmd(0);
        self.wait_complete(0)
    }

    /// Write sectors via DMA
    pub fn write_sectors(&mut self, lba: u64, count: u32, buf: *const u8) -> i32 {
        if count == 0 || count > 128 {
            return -1;
        }

        let buf_phys = unsafe { kmod_virt_to_phys(buf as u64) };
        let bytes = count * self.sector_size;

        let hdr = self.clb as *mut CmdHeader;
        unsafe {
            (*hdr).flags = 5 | (1 << 6); // CFL=5, Write bit
            (*hdr).prdtl = 1;
            (*hdr).prdbc = 0;
        }

        let tbl = self.ctba as *mut CmdTable;
        let fis = unsafe { &mut *((*tbl).cfis.as_mut_ptr() as *mut FisRegH2D) };
        *fis = FisRegH2D::new_command(ATA_CMD_WRITE_DMA_EX);
        fis.set_lba(lba, count as u16);

        unsafe {
            (*tbl).prdt[0].dba = buf_phys as u32;
            (*tbl).prdt[0].dbau = (buf_phys >> 32) as u32;
            (*tbl).prdt[0].dbc_i = bytes - 1;
            kmod_fence();
        }

        self.issue_cmd(0);
        self.wait_complete(0)
    }

    /// Flush cache
    pub fn flush(&mut self) -> i32 {
        let hdr = self.clb as *mut CmdHeader;
        unsafe {
            (*hdr).flags = 5;
            (*hdr).prdtl = 0;
        }

        let tbl = self.ctba as *mut CmdTable;
        let fis = unsafe { &mut *((*tbl).cfis.as_mut_ptr() as *mut FisRegH2D) };
        *fis = FisRegH2D::new_command(ATA_CMD_FLUSH_EX);

        unsafe { kmod_fence(); }
        self.issue_cmd(0);
        self.wait_complete(0)
    }

    /// Cleanup
    pub fn cleanup(&mut self) {
        self.stop_cmd();
        if !self.clb.is_null() {
            unsafe { kmod_dealloc(self.clb, 1024, 1024); }
        }
        if !self.fb.is_null() {
            unsafe { kmod_dealloc(self.fb, 256, 256); }
        }
        if !self.ctba.is_null() {
            unsafe { kmod_dealloc(self.ctba, 256, 128); }
        }
    }
}
