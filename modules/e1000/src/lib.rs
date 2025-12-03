//! Intel E1000 Network Driver Kernel Module for NexaOS
//!
//! This is a loadable kernel module (.nkm) that provides Intel E1000 NIC support.
//! It is loaded from initramfs during boot and dynamically linked to the kernel.
//!
//! # Module Entry Points
//!
//! - `module_init`: Called when module is loaded
//! - `module_exit`: Called when module is unloaded
//!
//! # Kernel API Usage
//!
//! This module uses the kernel's exported symbol table for:
//! - Logging (kmod_log_*)
//! - Memory allocation (kmod_alloc, kmod_dealloc)
//! - Network driver registration (kmod_net_register)
//! - MMIO access (kmod_mmio_read32, kmod_mmio_write32)
//! - PCI config access (kmod_pci_read_config_word, etc.)
//! - I/O port access (kmod_inl, kmod_outl)

#![no_std]
#![allow(dead_code)]

use core::cmp;

// ============================================================================
// Module Metadata
// ============================================================================

/// Module name
pub const MODULE_NAME: &[u8] = b"e1000\0";
/// Module version
pub const MODULE_VERSION: &[u8] = b"1.0.0\0";
/// Module description
pub const MODULE_DESC: &[u8] = b"Intel E1000 Gigabit Ethernet driver for NexaOS\0";
/// Module type (4 = Network)
pub const MODULE_TYPE: u8 = 4;
/// Module license (GPL-compatible, doesn't taint kernel)
pub const MODULE_LICENSE: &[u8] = b"MIT\0";
/// Module author
pub const MODULE_AUTHOR: &[u8] = b"NexaOS Team\0";
/// Source version (in-tree module)
pub const MODULE_SRCVERSION: &[u8] = b"in-tree\0";

// ============================================================================
// Kernel API declarations (resolved at load time from kernel symbol table)
// ============================================================================

extern "C" {
    fn kmod_log_info(msg: *const u8, len: usize);
    fn kmod_log_error(msg: *const u8, len: usize);
    fn kmod_log_warn(msg: *const u8, len: usize);
    fn kmod_log_debug(msg: *const u8, len: usize);
    fn kmod_alloc(size: usize, align: usize) -> *mut u8;
    fn kmod_zalloc(size: usize, align: usize) -> *mut u8;
    fn kmod_dealloc(ptr: *mut u8, size: usize, align: usize);
    fn kmod_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
    fn kmod_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8;
    fn kmod_spinlock_init(lock: *mut u64);
    fn kmod_spinlock_lock(lock: *mut u64);
    fn kmod_spinlock_unlock(lock: *mut u64);
    
    // Network modular API
    fn kmod_net_register(ops: *const NetDriverOps) -> i32;
    fn kmod_net_unregister(name: *const u8, name_len: usize) -> i32;
    
    // I/O helpers
    fn kmod_mmio_read32(addr: u64) -> u32;
    fn kmod_mmio_write32(addr: u64, value: u32);
    fn kmod_pci_read_config_word(bus: u8, device: u8, function: u8, offset: u32) -> u16;
    fn kmod_pci_write_config_word(bus: u8, device: u8, function: u8, offset: u32, value: u16);
    fn kmod_inl(port: u16) -> u32;
    fn kmod_outl(port: u16, value: u32);
    fn kmod_fence();
    fn kmod_spin_hint();
}

// ============================================================================
// Logging helpers
// ============================================================================

macro_rules! mod_info {
    ($msg:expr) => {
        unsafe { kmod_log_info($msg.as_ptr(), $msg.len()) }
    };
}

macro_rules! mod_error {
    ($msg:expr) => {
        unsafe { kmod_log_error($msg.as_ptr(), $msg.len()) }
    };
}

macro_rules! mod_warn {
    ($msg:expr) => {
        unsafe { kmod_log_warn($msg.as_ptr(), $msg.len()) }
    };
}

macro_rules! mod_debug {
    ($msg:expr) => {
        unsafe { kmod_log_debug($msg.as_ptr(), $msg.len()) }
    };
}

// ============================================================================
// FFI Types (must match kernel's net_modular.rs)
// ============================================================================

/// Network device descriptor passed from kernel
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NetDeviceDescriptor {
    pub index: usize,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub pci_segment: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub interrupt_line: u8,
    pub mac_len: u8,
    pub mac_address: [u8; 32],
    pub _reserved: [u8; 5],
}

/// Opaque handle to driver instance
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct NetDriverHandle(pub *mut u8);

/// Function pointer types for module operations
pub type FnNetDriverNew = extern "C" fn(desc: *const NetDeviceDescriptor) -> NetDriverHandle;
pub type FnNetDriverDestroy = extern "C" fn(handle: NetDriverHandle);
pub type FnNetDriverInit = extern "C" fn(handle: NetDriverHandle) -> i32;
pub type FnNetDriverUpdateDma = extern "C" fn(handle: NetDriverHandle);
pub type FnNetDriverTransmit = extern "C" fn(handle: NetDriverHandle, frame: *const u8, len: usize) -> i32;
pub type FnNetDriverDrainRx = extern "C" fn(handle: NetDriverHandle, buf: *mut u8, buf_len: usize) -> i32;
pub type FnNetDriverMaintenance = extern "C" fn(handle: NetDriverHandle) -> i32;
pub type FnNetDriverGetMac = extern "C" fn(handle: NetDriverHandle, mac: *mut u8);
pub type FnNetDriverProbe = extern "C" fn(vendor_id: u16, device_id: u16) -> i32;

/// Module operations table
#[repr(C)]
pub struct NetDriverOps {
    pub name: [u8; 32],
    pub probe: Option<FnNetDriverProbe>,
    pub new: Option<FnNetDriverNew>,
    pub destroy: Option<FnNetDriverDestroy>,
    pub init: Option<FnNetDriverInit>,
    pub update_dma: Option<FnNetDriverUpdateDma>,
    pub transmit: Option<FnNetDriverTransmit>,
    pub drain_rx: Option<FnNetDriverDrainRx>,
    pub maintenance: Option<FnNetDriverMaintenance>,
    pub get_mac: Option<FnNetDriverGetMac>,
}

// ============================================================================
// E1000 Hardware Constants
// ============================================================================

// PCI Configuration Space offsets
const PCI_COMMAND: u32 = 0x04;
const PCI_COMMAND_BUS_MASTER: u16 = 0x04;
const PCI_COMMAND_MEMORY: u16 = 0x02;

const RX_DESC_COUNT: usize = 64;
const TX_DESC_COUNT: usize = 64;
const RX_BUFFER_SIZE: usize = 2048;
const TX_BUFFER_SIZE: usize = 2048;

// E1000 Register Offsets
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

// Control Register Bits
const CTRL_RST: u32 = 1 << 26;
const CTRL_FRCSPD: u32 = 1 << 11;
const CTRL_FRCDPX: u32 = 1 << 12;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_ASDE: u32 = 1 << 5;

// Receive Control Register Bits
const RCTL_EN: u32 = 1 << 1;
const RCTL_UPE: u32 = 1 << 3;
const RCTL_MPE: u32 = 1 << 4;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_BSIZE_2048: u32 = 0b00 << 16;
const RCTL_BSEX: u32 = 1 << 25;
const RCTL_SECRC: u32 = 1 << 26;
const RCTL_LBM_NONE: u32 = 0b00 << 6;

// Transmit Control Register Bits
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT_SHIFT: u32 = 4;
const TCTL_COLD_SHIFT: u32 = 12;

// Descriptor Status Bits
const RX_STATUS_DD: u8 = 1 << 0;
const TX_CMD_EOP: u8 = 1 << 0;
const TX_CMD_IFCS: u8 = 1 << 1;
const TX_CMD_RS: u8 = 1 << 3;
const TX_STATUS_DD: u8 = 1 << 0;

// ============================================================================
// E1000 Descriptor Structures
// ============================================================================

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

// ============================================================================
// E1000 Driver Structure
// ============================================================================

#[repr(C, align(16))]
struct RxBuffer([u8; RX_BUFFER_SIZE]);

#[repr(C, align(16))]
struct TxBuffer([u8; TX_BUFFER_SIZE]);

/// E1000 driver instance
#[repr(C)]
pub struct E1000Driver {
    index: usize,
    base: u64,
    mac: [u8; 6],
    pci_bus: u8,
    pci_device: u8,
    pci_function: u8,
    
    // Descriptor rings (must be 16-byte aligned)
    rx_desc: [RxDescriptor; RX_DESC_COUNT],
    tx_desc: [TxDescriptor; TX_DESC_COUNT],
    
    // Buffers
    rx_buffers: [RxBuffer; RX_DESC_COUNT],
    tx_buffers: [TxBuffer; TX_DESC_COUNT],
    
    // State
    rx_index: usize,
    rx_tail: usize,
    tx_index: usize,
    link_up: bool,
}

impl E1000Driver {
    fn new(desc: &NetDeviceDescriptor) -> Option<*mut Self> {
        if desc.mmio_base == 0 {
            mod_error!(b"e1000: invalid MMIO base\n");
            return None;
        }

        // Allocate driver structure
        let size = core::mem::size_of::<Self>();
        let align = 16; // For descriptor alignment
        
        let ptr = unsafe { kmod_zalloc(size, align) };
        if ptr.is_null() {
            mod_error!(b"e1000: failed to allocate driver\n");
            return None;
        }

        let driver = ptr as *mut Self;
        
        unsafe {
            (*driver).index = desc.index;
            (*driver).base = desc.mmio_base;
            (*driver).pci_bus = desc.pci_bus;
            (*driver).pci_device = desc.pci_device;
            (*driver).pci_function = desc.pci_function;
            
            // Copy MAC address
            let mac_len = desc.mac_len.min(6) as usize;
            if mac_len >= 6 {
                (&mut (*driver).mac)[..6].copy_from_slice(&desc.mac_address[..6]);
            } else {
                (&mut (*driver).mac)[..mac_len].copy_from_slice(&desc.mac_address[..mac_len]);
            }
            
            // Initialize descriptor arrays
            for i in 0..RX_DESC_COUNT {
                (*driver).rx_desc[i] = RxDescriptor::new();
            }
            for i in 0..TX_DESC_COUNT {
                (*driver).tx_desc[i] = TxDescriptor::new();
            }
            
            (*driver).rx_index = 0;
            (*driver).rx_tail = RX_DESC_COUNT - 1;
            (*driver).tx_index = 0;
            (*driver).link_up = false;
        }

        Some(driver)
    }

    fn destroy(ptr: *mut Self) {
        if !ptr.is_null() {
            let size = core::mem::size_of::<Self>();
            unsafe { kmod_dealloc(ptr as *mut u8, size, 16) };
        }
    }

    fn init(&mut self) -> i32 {
        mod_info!(b"e1000: initializing device\n");

        // Enable PCI Bus Master
        self.enable_pci_bus_master();

        // Reset device
        self.reset();

        // Verify device is accessible
        let status = self.read_reg(REG_STATUS);
        if status == 0xFFFFFFFF {
            mod_error!(b"e1000: device not responding\n");
            return -1;
        }

        // Program MAC address
        self.program_mac();

        // Initialize RX and TX
        self.init_rx();
        self.init_tx();

        // Enable interrupts
        self.enable_interrupts();

        mod_info!(b"e1000: initialization complete\n");
        0
    }

    fn update_dma_addresses(&mut self) {
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
            let buf_addr = self.rx_buffers[idx].0.as_ptr() as u64;
            desc.addr = buf_addr;
            desc.status = 0;
        }

        // Reset RX state
        self.rx_index = 0;
        self.rx_tail = RX_DESC_COUNT - 1;
        self.write_reg(REG_RDT, self.rx_tail as u32);
    }

    fn transmit(&mut self, frame: &[u8]) -> i32 {
        if frame.len() > TX_BUFFER_SIZE {
            return -7; // BufferTooSmall
        }

        let slot = self.tx_index;
        if (self.tx_desc[slot].status & TX_STATUS_DD) == 0 {
            return -4; // TxBusy
        }

        // Get buffer address
        let buf_addr = self.tx_buffers[slot].0.as_ptr() as u64;

        // Setup descriptor
        self.tx_desc[slot].status = 0;
        self.tx_buffers[slot].0[..frame.len()].copy_from_slice(frame);
        self.tx_desc[slot].addr = buf_addr;
        self.tx_desc[slot].length = frame.len() as u16;
        self.tx_desc[slot].cmd = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;

        // Memory fence before updating TDT
        unsafe { kmod_fence() };

        // Update tail pointer
        let new_tdt = (self.tx_index + 1) % TX_DESC_COUNT;
        self.tx_index = new_tdt;
        self.write_reg(REG_TDT, self.tx_index as u32);

        0
    }

    fn drain_rx(&mut self, buf: &mut [u8]) -> i32 {
        let desc = &mut self.rx_desc[self.rx_index];
        if (desc.status & RX_STATUS_DD) == 0 {
            return 0; // No packet available
        }

        // Memory fence after reading status
        unsafe { kmod_fence() };

        let packet_len = cmp::min(desc.length as usize, buf.len());
        buf[..packet_len].copy_from_slice(&self.rx_buffers[self.rx_index].0[..packet_len]);

        // Clear descriptor
        desc.status = 0;
        desc.length = 0;

        // Memory fence before updating RDT
        unsafe { kmod_fence() };

        // Update pointers
        let old_index = self.rx_index;
        self.rx_index = (self.rx_index + 1) % RX_DESC_COUNT;
        self.rx_tail = old_index;
        self.write_reg(REG_RDT, self.rx_tail as u32);

        packet_len as i32
    }

    fn maintenance(&mut self) -> i32 {
        let status = self.read_reg(REG_STATUS);
        let link_bit = (status & (1 << 1)) != 0;
        if link_bit != self.link_up {
            self.link_up = link_bit;
            if self.link_up {
                mod_info!(b"e1000: link up\n");
            } else {
                mod_warn!(b"e1000: link down\n");
            }
        }
        0
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    // ========================================================================
    // Private helper methods
    // ========================================================================

    fn reset(&mut self) {
        self.write_reg(REG_IMC, 0xFFFF_FFFF);
        self.write_reg(REG_CTRL, CTRL_RST);
        
        // Wait for reset to complete
        while (self.read_reg(REG_CTRL) & CTRL_RST) != 0 {
            unsafe { kmod_spin_hint() };
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
        // Initialize RX descriptors with buffer addresses
        for (idx, desc) in self.rx_desc.iter_mut().enumerate() {
            let buf_addr = self.rx_buffers[idx].0.as_ptr() as u64;
            desc.addr = buf_addr;
            desc.status = 0;
            desc.length = 0;
        }

        // Program descriptor ring
        let rdba = self.rx_desc.as_ptr() as u64;
        self.write_reg(REG_RDBAL, (rdba & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_RDBAH, (rdba >> 32) as u32);
        self.write_reg(REG_RDLEN, (RX_DESC_COUNT * core::mem::size_of::<RxDescriptor>()) as u32);
        self.write_reg(REG_RDH, 0);
        self.rx_index = 0;
        self.rx_tail = RX_DESC_COUNT - 1;
        self.write_reg(REG_RDT, self.rx_tail as u32);

        // Enable receiver (promiscuous for now)
        let rctl = RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SECRC | RCTL_BSIZE_2048 | RCTL_LBM_NONE;
        self.write_reg(REG_RCTL, rctl);
    }

    fn init_tx(&mut self) {
        // Initialize TX descriptors
        for desc in self.tx_desc.iter_mut() {
            *desc = TxDescriptor::new();
            desc.status = TX_STATUS_DD;
        }

        // Program descriptor ring
        let tdba = self.tx_desc.as_ptr() as u64;
        self.write_reg(REG_TDBAL, (tdba & 0xFFFF_FFFF) as u32);
        self.write_reg(REG_TDBAH, (tdba >> 32) as u32);
        self.write_reg(REG_TDLEN, (TX_DESC_COUNT * core::mem::size_of::<TxDescriptor>()) as u32);
        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);
        self.tx_index = 0;

        // Enable transmitter
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

    fn enable_pci_bus_master(&mut self) {
        let mut command = unsafe { 
            kmod_pci_read_config_word(self.pci_bus, self.pci_device, self.pci_function, PCI_COMMAND)
        };
        command |= PCI_COMMAND_BUS_MASTER | PCI_COMMAND_MEMORY;
        unsafe {
            kmod_pci_write_config_word(self.pci_bus, self.pci_device, self.pci_function, PCI_COMMAND, command);
        }
    }

    fn write_reg(&self, offset: u32, value: u32) {
        unsafe { kmod_mmio_write32(self.base + offset as u64, value) };
    }

    fn read_reg(&self, offset: u32) -> u32 {
        unsafe { kmod_mmio_read32(self.base + offset as u64) }
    }
}

// ============================================================================
// Module Entry Points (FFI callbacks)
// ============================================================================

/// Probe function - check if driver supports device
#[no_mangle]
pub extern "C" fn e1000_probe(vendor_id: u16, device_id: u16) -> i32 {
    // Intel vendor ID
    if vendor_id != 0x8086 {
        return -1;
    }
    
    // Supported E1000 device IDs
    match device_id {
        0x100e | // 82540EM (QEMU default)
        0x100f | // 82545EM
        0x150e | // 82580
        0x153a | // I217
        0x10d3   // 82574
        => 0,
        _ => -1,
    }
}

/// Create new driver instance
#[no_mangle]
pub extern "C" fn e1000_new(desc: *const NetDeviceDescriptor) -> NetDriverHandle {
    if desc.is_null() {
        return NetDriverHandle(core::ptr::null_mut());
    }

    let desc = unsafe { &*desc };
    match E1000Driver::new(desc) {
        Some(ptr) => NetDriverHandle(ptr as *mut u8),
        None => NetDriverHandle(core::ptr::null_mut()),
    }
}

/// Destroy driver instance
#[no_mangle]
pub extern "C" fn e1000_destroy(handle: NetDriverHandle) {
    if !handle.0.is_null() {
        E1000Driver::destroy(handle.0 as *mut E1000Driver);
    }
}

/// Initialize hardware
#[no_mangle]
pub extern "C" fn e1000_init(handle: NetDriverHandle) -> i32 {
    if handle.0.is_null() {
        return -2; // DeviceMissing
    }

    let driver = unsafe { &mut *(handle.0 as *mut E1000Driver) };
    driver.init()
}

/// Update DMA addresses
#[no_mangle]
pub extern "C" fn e1000_update_dma(handle: NetDriverHandle) {
    if !handle.0.is_null() {
        let driver = unsafe { &mut *(handle.0 as *mut E1000Driver) };
        driver.update_dma_addresses();
    }
}

/// Transmit frame
#[no_mangle]
pub extern "C" fn e1000_transmit(handle: NetDriverHandle, frame: *const u8, len: usize) -> i32 {
    if handle.0.is_null() || frame.is_null() {
        return -2;
    }

    let driver = unsafe { &mut *(handle.0 as *mut E1000Driver) };
    let frame_slice = unsafe { core::slice::from_raw_parts(frame, len) };
    driver.transmit(frame_slice)
}

/// Drain RX queue
#[no_mangle]
pub extern "C" fn e1000_drain_rx(handle: NetDriverHandle, buf: *mut u8, buf_len: usize) -> i32 {
    if handle.0.is_null() || buf.is_null() {
        return 0;
    }

    let driver = unsafe { &mut *(handle.0 as *mut E1000Driver) };
    let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf, buf_len) };
    driver.drain_rx(buf_slice)
}

/// Maintenance callback
#[no_mangle]
pub extern "C" fn e1000_maintenance(handle: NetDriverHandle) -> i32 {
    if handle.0.is_null() {
        return -2;
    }

    let driver = unsafe { &mut *(handle.0 as *mut E1000Driver) };
    driver.maintenance()
}

/// Get MAC address
#[no_mangle]
pub extern "C" fn e1000_get_mac(handle: NetDriverHandle, mac: *mut u8) {
    if handle.0.is_null() || mac.is_null() {
        return;
    }

    let driver = unsafe { &*(handle.0 as *mut E1000Driver) };
    let mac_slice = unsafe { core::slice::from_raw_parts_mut(mac, 6) };
    mac_slice.copy_from_slice(&driver.mac_address());
}

// ============================================================================
// Module Init/Exit
// ============================================================================

/// Module entry point table
#[used]
#[no_mangle]
pub static MODULE_ENTRY_POINTS: [unsafe extern "C" fn() -> i32; 2] = [
    module_init_wrapper,
    module_exit_wrapper,
];

#[no_mangle]
unsafe extern "C" fn module_init_wrapper() -> i32 {
    module_init()
}

#[no_mangle]
unsafe extern "C" fn module_exit_wrapper() -> i32 {
    module_exit()
}

/// Module initialization
#[no_mangle]
pub unsafe extern "C" fn module_init() -> i32 {
    mod_info!(b"e1000: loading Intel E1000 network driver\n");

    // Build driver name
    let mut name = [0u8; 32];
    name[..5].copy_from_slice(b"e1000");

    // Create operations table
    let ops = NetDriverOps {
        name,
        probe: Some(e1000_probe),
        new: Some(e1000_new),
        destroy: Some(e1000_destroy),
        init: Some(e1000_init),
        update_dma: Some(e1000_update_dma),
        transmit: Some(e1000_transmit),
        drain_rx: Some(e1000_drain_rx),
        maintenance: Some(e1000_maintenance),
        get_mac: Some(e1000_get_mac),
    };

    let result = kmod_net_register(&ops);
    if result != 0 {
        mod_error!(b"e1000: failed to register driver\n");
        return result;
    }

    mod_info!(b"e1000: driver loaded successfully\n");
    0
}

/// Module cleanup
#[no_mangle]
pub unsafe extern "C" fn module_exit() -> i32 {
    mod_info!(b"e1000: unloading driver\n");
    
    let result = kmod_net_unregister(b"e1000\0".as_ptr(), 5);
    
    mod_info!(b"e1000: driver unloaded\n");
    result
}

// ============================================================================
// Panic Handler (required for no_std)
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe { kmod_spin_hint() };
    }
}
