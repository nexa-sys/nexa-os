use core::{cmp, ptr};

use crate::uefi_compat::NetworkDescriptor;

use super::NetError;

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
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const RCTL_BSIZE_2048: u32 = 0b00 << 16;
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
        self.reset();
        self.program_mac();
        self.init_rx();
        self.init_tx();
        self.enable_interrupts();
        crate::kinfo!("e1000[{}]: initialized", self.index);
        Ok(())
    }

    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), NetError> {
        if frame.len() > TX_BUFFER_SIZE {
            return Err(NetError::BufferTooSmall);
        }

        let slot = self.tx_index;
        if (self.tx_desc[slot].status & TX_STATUS_DD) == 0 {
            return Err(NetError::TxBusy);
        }

        self.tx_desc[slot].status = 0;
        self.tx_buffers[slot].0[..frame.len()].copy_from_slice(frame);
        self.tx_desc[slot].addr = self.tx_buffers[slot].as_ptr() as u64;
        self.tx_desc[slot].length = frame.len() as u16;
        self.tx_desc[slot].cmd = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;

        self.tx_index = (self.tx_index + 1) % TX_DESC_COUNT;
        self.write_reg(REG_TDT, self.tx_index as u32);
        Ok(())
    }

    pub fn drain_rx(&mut self, scratch: &mut [u8]) -> Option<usize> {
        if scratch.len() < RX_BUFFER_SIZE {
            // Ensure we never overflow the caller buffer
        }
        let desc = &mut self.rx_desc[self.rx_index];
        if (desc.status & RX_STATUS_DD) == 0 {
            return None;
        }

        let packet_len = cmp::min(desc.length as usize, scratch.len());
        scratch[..packet_len]
            .copy_from_slice(&self.rx_buffers[self.rx_index].0[..packet_len]);
        desc.status = 0;

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
        self.write_reg(REG_RAL0, low);
        self.write_reg(REG_RAH0, high);
    }

    fn init_rx(&mut self) {
        for (idx, desc) in self.rx_desc.iter_mut().enumerate() {
            desc.addr = self.rx_buffers[idx].as_ptr() as u64;
            desc.status = 0;
        }

        self.write_reg(REG_RDBAL, (self.rx_desc.as_ptr() as u64 & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_RDBAH, (self.rx_desc.as_ptr() as u64 >> 32) as u32);
        self.write_reg(
            REG_RDLEN,
            (RX_DESC_COUNT * core::mem::size_of::<RxDescriptor>()) as u32,
        );
        self.write_reg(REG_RDH, 0);
        self.rx_index = 0;
        self.rx_tail = RX_DESC_COUNT - 1;
        self.write_reg(REG_RDT, self.rx_tail as u32);

        let rctl = RCTL_EN | RCTL_BAM | RCTL_SECRC | RCTL_BSIZE_2048 | RCTL_LBM_NONE;
        self.write_reg(REG_RCTL, rctl);
    }

    fn init_tx(&mut self) {
        for desc in self.tx_desc.iter_mut() {
            *desc = TxDescriptor::new();
        }

        self.write_reg(REG_TDBAL, (self.tx_desc.as_ptr() as u64 & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_TDBAH, (self.tx_desc.as_ptr() as u64 >> 32) as u32);
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
}

unsafe impl Send for E1000 {}
