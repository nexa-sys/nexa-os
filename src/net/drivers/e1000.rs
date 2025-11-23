use core::{cmp, ptr};

use crate::uefi_compat::NetworkDescriptor;

use super::NetError;

// PCI Configuration Space offsets
const PCI_COMMAND: u32 = 0x04;
const PCI_COMMAND_BUS_MASTER: u16 = 0x04;  // Bit 2: Bus Master Enable
const PCI_COMMAND_MEMORY: u16 = 0x02;      // Bit 1: Memory Space Enable

const RX_DESC_COUNT: usize = 64;
const TX_DESC_COUNT: usize = 64;
const RX_BUFFER_SIZE: usize = 2048;
const TX_BUFFER_SIZE: usize = 2048;

const REG_CTRL: u32 = 0x0000;
const REG_STATUS: u32 = 0x0008;
const REG_CTRL_EXT: u32 = 0x0018;
const REG_IMS: u32 = 0x00D0;
const REG_IMC: u32 = 0x00D8;
const REG_RCTL: u32 = 0x0100;
const REG_TCTL: u32 = 0x0400;
const REG_TIPG: u32 = 0x0410;
const REG_RDBAL: u32 = 0x2800;
const REG_RDBAH: u32 = 0x2804;
const REG_RDLEN: u32 = 0x2808;
const REG_RDH: u32 = 0x2810;
const REG_RDT: u32 = 0x2818;
const REG_TDBAL: u32 = 0x3800;
const REG_TDBAH: u32 = 0x3804;
const REG_TDLEN: u32 = 0x3808;
const REG_TDH: u32 = 0x3810;
const REG_TDT: u32 = 0x3818;
const REG_ICR: u32 = 0x00C0;
const REG_RAL0: u32 = 0x5400;
const REG_RAH0: u32 = 0x5404;

const CTRL_RST: u32 = 1 << 26;
const CTRL_FRCSPD: u32 = 1 << 11;
const CTRL_FRCDPX: u32 = 1 << 12;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_ASDE: u32 = 1 << 5;

const RCTL_EN: u32 = 1 << 1;
const RCTL_UPE: u32 = 1 << 3;  // Unicast Promiscuous Enable
const RCTL_MPE: u32 = 1 << 4;  // Multicast Promiscuous Enable
const RCTL_BAM: u32 = 1 << 15;
const RCTL_BSIZE_2048: u32 = 0b00 << 16;
const RCTL_BSEX: u32 = 1 << 25;  // Buffer Size Extension (0 for BSIZE compatibility)
const RCTL_SECRC: u32 = 1 << 26;
const RCTL_LBM_NONE: u32 = 0b00 << 6;

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT_SHIFT: u32 = 4;
const TCTL_COLD_SHIFT: u32 = 12;

const RX_STATUS_DD: u8 = 1 << 0;
const TX_CMD_EOP: u8 = 1 << 0;
const TX_CMD_IFCS: u8 = 1 << 1;
const TX_CMD_RS: u8 = 1 << 3;
const TX_STATUS_DD: u8 = 1 << 0;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct RxDescriptor {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

impl RxDescriptor {
    const fn new() -> Self {
        Self {
            addr: 0,
            length: 0,
            checksum: 0,
            status: 0,
            errors: 0,
            special: 0,
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct TxDescriptor {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

impl TxDescriptor {
    const fn new() -> Self {
        Self {
            addr: 0,
            length: 0,
            cso: 0,
            cmd: 0,
            status: TX_STATUS_DD,
            css: 0,
            special: 0,
        }
    }
}

#[repr(align(16))]
#[derive(Clone, Copy)]
struct Buffer<const N: usize>([u8; N]);

impl<const N: usize> Buffer<N> {
    const fn new() -> Self {
        Self([0u8; N])
    }

    fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }
}

pub struct E1000 {
    index: usize,
    base: *mut u8,
    mac: [u8; 6],
    pci_segment: u16,
    pci_bus: u8,
    pci_device: u8,
    pci_function: u8,
    rx_desc: [RxDescriptor; RX_DESC_COUNT],
    tx_desc: [TxDescriptor; TX_DESC_COUNT],
    rx_buffers: [Buffer<RX_BUFFER_SIZE>; RX_DESC_COUNT],
    tx_buffers: [Buffer<TX_BUFFER_SIZE>; TX_DESC_COUNT],
    rx_index: usize,
    rx_tail: usize,
    tx_index: usize,
    link_up: bool,
}

impl E1000 {
    pub fn new(index: usize, descriptor: NetworkDescriptor) -> Result<Self, NetError> {
        if descriptor.mmio_base == 0 {
            return Err(NetError::InvalidDescriptor);
        }

        let mut mac = [0u8; 6];
        let mac_len = descriptor.info.mac_len.min(6) as usize;
        if mac_len >= 6 {
            mac.copy_from_slice(&descriptor.info.mac_address[..6]);
        } else {
            mac[..mac_len].copy_from_slice(&descriptor.info.mac_address[..mac_len]);
        }

        Ok(Self {
            index,
            base: descriptor.mmio_base as *mut u8,
            mac,
            pci_segment: descriptor.info.pci_segment,
            pci_bus: descriptor.info.pci_bus,
            pci_device: descriptor.info.pci_device,
            pci_function: descriptor.info.pci_function,
            rx_desc: core::array::from_fn(|_| RxDescriptor::new()),
            tx_desc: core::array::from_fn(|_| TxDescriptor::new()),
            rx_buffers: core::array::from_fn(|_| Buffer::new()),
            tx_buffers: core::array::from_fn(|_| Buffer::new()),
            rx_index: 0,
            rx_tail: RX_DESC_COUNT - 1,
            tx_index: 0,
            link_up: false,
        })
    }

    pub fn init(&mut self) -> Result<(), NetError> {
        crate::serial::_print(format_args!(
            "[e1000::init] Starting initialization for device {}, MMIO base={:#x}\n",
            self.index, self.base as u64
        ));
        
        // CRITICAL: Enable PCI Bus Master for DMA operations
        self.enable_pci_bus_master();
        
        self.reset();
        
        // Verify device is accessible
        let status = self.read_reg(REG_STATUS);
        crate::serial::_print(format_args!(
            "[e1000::init] Device status after reset: {:#x}\n",
            status
        ));
        
        self.program_mac();
        self.init_rx();
        self.init_tx();
        self.enable_interrupts();
        
        // Final status check
        let final_status = self.read_reg(REG_STATUS);
        let rctl = self.read_reg(REG_RCTL);
        let tctl = self.read_reg(REG_TCTL);
        let rdh = self.read_reg(REG_RDH);
        let rdt = self.read_reg(REG_RDT);
        let rdlen = self.read_reg(REG_RDLEN);
        let rdbal = self.read_reg(REG_RDBAL);
        let rdbah = self.read_reg(REG_RDBAH);
        
        crate::serial::_print(format_args!(
            "[e1000::init] Final state - STATUS={:#x}, RCTL={:#x}, TCTL={:#x}\n",
            final_status, rctl, tctl
        ));
        crate::serial::_print(format_args!(
            "[e1000::init] RX Ring - RDBAL={:#x}, RDBAH={:#x}, RDLEN={}, RDH={}, RDT={}\n",
            rdbal, rdbah, rdlen, rdh, rdt
        ));
        
        crate::kinfo!("e1000[{}]: initialized", self.index);
        Ok(())
    }

    /// Update DMA descriptor base addresses after the driver has been moved in memory.
    /// This must be called after moving the driver to ensure hardware uses correct addresses.
    pub fn update_dma_addresses(&mut self) {
        // Update RX descriptor base
        let rdba = self.rx_desc.as_ptr() as u64;
        self.write_reg(REG_RDBAL, (rdba & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_RDBAH, (rdba >> 32) as u32);
        
        // Update TX descriptor base
        let tdba = self.tx_desc.as_ptr() as u64;
        self.write_reg(REG_TDBAL, (tdba & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_TDBAH, (tdba >> 32) as u32);
        
        // Update all RX descriptor buffer addresses
        for (idx, desc) in self.rx_desc.iter_mut().enumerate() {
            let buf_addr = self.rx_buffers[idx].as_ptr() as u64;
            desc.addr = buf_addr;
            if idx < 3 {
                crate::serial::_print(format_args!(
                    "[update_dma] desc[{}].addr={:#x} (buffer@{:#x})\n",
                    idx, desc.addr, buf_addr
                ));
            }
        }
        
        // Reset RDT to indicate all descriptors are available
        self.rx_tail = RX_DESC_COUNT - 1;
        self.write_reg(REG_RDT, self.rx_tail as u32);
        
        crate::serial::_print(format_args!(
            "[e1000::update_dma_addresses] Updated addresses - RDBA={:#x}, TDBA={:#x}, RDT={}\n",
            rdba, tdba, self.rx_tail
        ));
    }

    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), NetError> {
        if frame.len() > TX_BUFFER_SIZE {
            return Err(NetError::BufferTooSmall);
        }

        let slot = self.tx_index;
        if (self.tx_desc[slot].status & TX_STATUS_DD) == 0 {
            return Err(NetError::TxBusy);
        }

        // Check link status before transmit
        let status = self.read_reg(REG_STATUS);
        let link_up = (status & 0x2) != 0;
        
        // Get buffer address BEFORE modifying descriptor
        let buf_addr = self.tx_buffers[slot].as_ptr() as u64;
        
        self.tx_desc[slot].status = 0;
        self.tx_buffers[slot].0[..frame.len()].copy_from_slice(frame);
        self.tx_desc[slot].addr = buf_addr;
        self.tx_desc[slot].length = frame.len() as u16;
        self.tx_desc[slot].cmd = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;

        // Ensure descriptor writes are visible to DMA before updating TDT
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

        let new_tdt = (self.tx_index + 1) % TX_DESC_COUNT;
        
        // Debug: Log every TX packet with descriptor pointer
        let desc_ptr = &self.tx_desc[slot] as *const TxDescriptor as usize;
        crate::serial::_print(format_args!(
            "[e1000::transmit] Sending {} bytes, slot={}, buf_addr={:#x}, desc_ptr={:#x}, TDT: {} -> {}, link_up={}, STATUS={:#x}\n",
            frame.len(), slot, buf_addr, desc_ptr, self.tx_index, new_tdt, link_up, status
        ));
        
        self.tx_index = new_tdt;
        self.write_reg(REG_TDT, self.tx_index as u32);
        
        // Verify TDT was written and check TCTL
        let actual_tdt = self.read_reg(REG_TDT);
        let tctl = self.read_reg(REG_TCTL);
        
        // Read descriptor back with volatile to see what E1000 sees
        let desc_addr_readback = unsafe { 
            core::ptr::read_volatile(&self.tx_desc[slot].addr as *const u64)
        };
        
        crate::serial::_print(format_args!(
            "[e1000::transmit] Verified TDT={}, TCTL={:#x}, desc[{}].addr={:#x} (readback={:#x})\n", 
            actual_tdt, tctl, slot, self.tx_desc[slot].addr, desc_addr_readback
        ));
        
        // Wait a bit and check if descriptor was processed
        for _ in 0..1000 { core::hint::spin_loop(); }
        let desc_status = unsafe {
            core::ptr::read_volatile(&self.tx_desc[slot].status as *const u8)
        };
        crate::serial::_print(format_args!(
            "[e1000::transmit] After spin: desc[{}].status={:#x}, DD={}\n",
            slot, desc_status, (desc_status & TX_STATUS_DD) != 0
        ));
        
        Ok(())
    }

    pub fn drain_rx(&mut self, scratch: &mut [u8]) -> Option<usize> {
        if scratch.len() < RX_BUFFER_SIZE {
            // Ensure we never overflow the caller buffer
        }
        
        // Debug: Print hardware state periodically (before borrowing desc)
        static DEBUG_COUNTER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        let count = DEBUG_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        
        if count % 100 == 1 {
            let rdh = self.read_reg(REG_RDH);
            let rdt = self.read_reg(REG_RDT);
            let rctl = self.read_reg(REG_RCTL);
            let desc_status = self.rx_desc[self.rx_index].status;
            crate::serial::_print(format_args!(
                "[e1000::drain_rx] RX state: index={}, RDH={}, RDT={}, RCTL={:#x}, desc.status={:#x}, DD={}\n",
                self.rx_index, rdh, rdt, rctl, desc_status, (desc_status & RX_STATUS_DD) != 0
            ));
        }
        
        let desc = &mut self.rx_desc[self.rx_index];
        if (desc.status & RX_STATUS_DD) == 0 {
            return None;
        }

        // Ensure descriptor status read completes before reading buffer data
        core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);

        crate::serial::_print(format_args!(
            "[e1000::drain_rx] *** PACKET RECEIVED! index={}, len={} ***\n",
            self.rx_index, desc.length
        ));

        let packet_len = cmp::min(desc.length as usize, scratch.len());
        scratch[..packet_len]
            .copy_from_slice(&self.rx_buffers[self.rx_index].0[..packet_len]);
        
        // DEBUG: Dump first 32 bytes to see ethernet header
        if packet_len >= 14 {
            crate::serial::_print(format_args!(
                "[e1000::drain_rx] Header dump: [{:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}]\n",
                scratch[0], scratch[1], scratch[2], scratch[3], scratch[4], scratch[5],
                scratch[6], scratch[7], scratch[8], scratch[9], scratch[10], scratch[11],
                scratch[12], scratch[13]
            ));
        }
        
        // Clear descriptor status BEFORE updating RDT
        desc.status = 0;
        desc.length = 0;
        
        // Ensure descriptor modifications are visible before updating tail pointer
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

        self.rx_tail = self.rx_index;
        self.rx_index = (self.rx_index + 1) % RX_DESC_COUNT;
        self.write_reg(REG_RDT, self.rx_tail as u32);
        Some(packet_len)
    }

    pub fn maintenance(&mut self) -> Result<(), NetError> {
        let status = self.read_reg(REG_STATUS);
        let link_bit = (status & (1 << 1)) != 0;
        if link_bit != self.link_up {
            self.link_up = link_bit;
            if self.link_up {
                crate::kinfo!("e1000[{}]: link up", self.index);
            } else {
                crate::kwarn!("e1000[{}]: link down", self.index);
            }
        }
        Ok(())
    }

    pub fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn reset(&mut self) {
        self.write_reg(REG_IMC, 0xFFFF_FFFF);
        self.write_reg(REG_CTRL, CTRL_RST);
        while (self.read_reg(REG_CTRL) & CTRL_RST) != 0 {
            core::hint::spin_loop();
        }
        self.write_reg(REG_CTRL, CTRL_SLU | CTRL_ASDE | CTRL_FRCSPD | CTRL_FRCDPX);
    }

    fn program_mac(&mut self) {
        let low = u32::from_le_bytes([self.mac[2], self.mac[3], self.mac[4], self.mac[5]]);
        let high = u32::from_le_bytes([self.mac[0], self.mac[1], 0, 0]) | (1 << 31);
        
        crate::serial::_print(format_args!(
            "[e1000::program_mac] Setting MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, RAL0={:#x}, RAH0={:#x}\n",
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
            low, high
        ));
        
        self.write_reg(REG_RAL0, low);
        self.write_reg(REG_RAH0, high);
        
        // Verify
        let ral_read = self.read_reg(REG_RAL0);
        let rah_read = self.read_reg(REG_RAH0);
        crate::serial::_print(format_args!(
            "[e1000::program_mac] Readback: RAL0={:#x}, RAH0={:#x}\n",
            ral_read, rah_read
        ));
    }

    fn init_rx(&mut self) {
        // Verify descriptor alignment (must be 16-byte aligned for E1000)
        let desc_addr = self.rx_desc.as_ptr() as u64;
        if desc_addr & 0xF != 0 {
            crate::kwarn!("[e1000::init_rx] WARNING: RX descriptor base {:#x} is not 16-byte aligned!", desc_addr);
        }
        
        for (idx, desc) in self.rx_desc.iter_mut().enumerate() {
            let buf_addr = self.rx_buffers[idx].as_ptr() as u64;
            desc.addr = buf_addr;
            desc.status = 0;
            desc.length = 0;
            desc.checksum = 0;
            desc.errors = 0;
            desc.special = 0;
            
            if idx == 0 {
                crate::serial::_print(format_args!(
                    "[e1000::init_rx] desc[0].addr={:#x}, buf alignment={}\n",
                    desc.addr, buf_addr & 0xF
                ));
            }
        }

        let rdba = desc_addr;
        crate::serial::_print(format_args!(
            "[e1000::init_rx] RX descriptor base={:#x} (alignment={}), count={}\n",
            rdba, rdba & 0xF, RX_DESC_COUNT
        ));
        
        self.write_reg(REG_RDBAL, (rdba & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_RDBAH, (rdba >> 32) as u32);
        self.write_reg(
            REG_RDLEN,
            (RX_DESC_COUNT * core::mem::size_of::<RxDescriptor>()) as u32,
        );
        self.write_reg(REG_RDH, 0);
        self.rx_index = 0;
        self.rx_tail = RX_DESC_COUNT - 1;
        self.write_reg(REG_RDT, self.rx_tail as u32);

        // Enable promiscuous mode temporarily for debugging DHCP issues
        // Production note: Remove UPE/MPE after DHCP is working
        let rctl = RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SECRC | RCTL_BSIZE_2048 | RCTL_LBM_NONE;
        crate::serial::_print(format_args!(
            "[e1000::init_rx] Setting RCTL={:#x} (EN={}, UPE={}, MPE={}, BAM={}), RDT={}\n",
            rctl, (rctl & RCTL_EN) != 0, (rctl & RCTL_UPE) != 0, 
            (rctl & RCTL_MPE) != 0, (rctl & RCTL_BAM) != 0, self.rx_tail
        ));
        self.write_reg(REG_RCTL, rctl);
        
        // Verify configuration
        let rctl_read = self.read_reg(REG_RCTL);
        if rctl_read != rctl {
            crate::kwarn!("[e1000::init_rx] RCTL mismatch! Wrote {:#x}, read back {:#x}", rctl, rctl_read);
        } else {
            crate::serial::_print(format_args!(
                "[e1000::init_rx] RCTL verified={:#x}\n",
                rctl_read
            ));
        }
    }

    fn init_tx(&mut self) {
        for desc in self.tx_desc.iter_mut() {
            *desc = TxDescriptor::new();
            // Set DD (Descriptor Done) flag so first transmit doesn't fail
            desc.status = TX_STATUS_DD;
        }

        let tx_desc_addr = self.tx_desc.as_ptr() as u64;
        let tx_buf0_addr = self.tx_buffers[0].as_ptr() as u64;
        let tdbal = (tx_desc_addr & 0xFFFF_FFFF) as u32;
        let tdbah = (tx_desc_addr >> 32) as u32;
        
        self.write_reg(REG_TDBAL, tdbal);
        self.write_reg(REG_TDBAH, tdbah);
        self.write_reg(
            REG_TDLEN,
            (TX_DESC_COUNT * core::mem::size_of::<TxDescriptor>()) as u32,
        );
        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);
        self.tx_index = 0;

        let mut tctl = TCTL_EN | TCTL_PSP;
        tctl |= (0x10 << TCTL_CT_SHIFT) | (0x40 << TCTL_COLD_SHIFT);
        self.write_reg(REG_TCTL, tctl);
        self.write_reg(REG_TIPG, 0x0060200A);
        
        crate::serial::_print(format_args!(
            "[e1000::init_tx] TX ring: desc_addr={:#x}, buf[0]_addr={:#x}, TDBAL={:#x}, TDBAH={:#x}, TDLEN={}, TCTL={:#x}\n",
            tx_desc_addr, tx_buf0_addr, tdbal, tdbah, TX_DESC_COUNT * core::mem::size_of::<TxDescriptor>(), tctl
        ));
    }

    fn enable_interrupts(&mut self) {
        self.write_reg(REG_IMC, 0xFFFF_FFFF);
        self.read_reg(REG_ICR);
        self.write_reg(REG_IMS, 0x1F6DC);
    }

    fn write_reg(&mut self, offset: u32, value: u32) {
        unsafe {
            ptr::write_volatile(self.base.add(offset as usize) as *mut u32, value);
        }
    }

    fn read_reg(&self, offset: u32) -> u32 {
        unsafe { ptr::read_volatile(self.base.add(offset as usize) as *const u32) }
    }

    /// Enable PCI Bus Master - CRITICAL for DMA operations
    fn enable_pci_bus_master(&mut self) {
        crate::serial::_print(format_args!(
            "[e1000::enable_pci_bus_master] PCI {:04x}:{:02x}:{:02x}.{} - Enabling Bus Master\n",
            self.pci_segment, self.pci_bus, self.pci_device, self.pci_function
        ));

        // Read current PCI command register
        let mut command = self.pci_read_config_word(PCI_COMMAND);
        crate::serial::_print(format_args!(
            "[e1000::enable_pci_bus_master] Current PCI_COMMAND={:#x}\n",
            command
        ));

        // Enable Bus Master and Memory Space
        command |= PCI_COMMAND_BUS_MASTER | PCI_COMMAND_MEMORY;
        self.pci_write_config_word(PCI_COMMAND, command);

        // Verify
        let command_verify = self.pci_read_config_word(PCI_COMMAND);
        crate::serial::_print(format_args!(
            "[e1000::enable_pci_bus_master] New PCI_COMMAND={:#x} (BM={}, MEM={})\n",
            command_verify,
            (command_verify & PCI_COMMAND_BUS_MASTER) != 0,
            (command_verify & PCI_COMMAND_MEMORY) != 0
        ));

        if (command_verify & PCI_COMMAND_BUS_MASTER) == 0 {
            crate::kwarn!("[e1000] CRITICAL: Failed to enable PCI Bus Master! DMA will not work!");
        }
    }

    /// Read 16-bit value from PCI configuration space
    fn pci_read_config_word(&self, offset: u32) -> u16 {
        let address = 0x80000000u32
            | ((self.pci_bus as u32) << 16)
            | ((self.pci_device as u32) << 11)
            | ((self.pci_function as u32) << 8)
            | (offset & 0xFC);

        unsafe {
            // Write address to CONFIG_ADDRESS (0xCF8)
            ptr::write_volatile(0xCF8 as *mut u32, address);
            // Read from CONFIG_DATA (0xCFC), adjust for offset
            let data = ptr::read_volatile(0xCFC as *const u32);
            ((data >> ((offset & 2) * 8)) & 0xFFFF) as u16
        }
    }

    /// Write 16-bit value to PCI configuration space
    fn pci_write_config_word(&mut self, offset: u32, value: u16) {
        let address = 0x80000000u32
            | ((self.pci_bus as u32) << 16)
            | ((self.pci_device as u32) << 11)
            | ((self.pci_function as u32) << 8)
            | (offset & 0xFC);

        unsafe {
            // Write address to CONFIG_ADDRESS (0xCF8)
            ptr::write_volatile(0xCF8 as *mut u32, address);
            // Read-modify-write to CONFIG_DATA (0xCFC)
            let shift = (offset & 2) * 8;
            let mut data = ptr::read_volatile(0xCFC as *const u32);
            data = (data & !(0xFFFF << shift)) | ((value as u32) << shift);
            ptr::write_volatile(0xCFC as *mut u32, data);
        }
    }
}

unsafe impl Send for E1000 {}
