mod e1000;

use crate::{bootinfo, uefi_compat::NetworkDescriptor};

pub use e1000::E1000;

#[derive(Debug)]
pub enum NetError {
    UnsupportedDevice,
    DeviceMissing,
    RxExhausted,
    TxBusy,
    InvalidDescriptor,
    HardwareFault,
    BufferTooSmall,
    AddressInUse,
    TooManyConnections,
    InvalidSocket,
    InvalidDevice,
    ArpCacheMiss,
    RxQueueFull,
    RxQueueEmpty,
    ChecksumFailed,
    InvalidPacket,
    OperationNotSupported,
}

pub enum DriverInstance {
    E1000(e1000::E1000),
}

impl DriverInstance {
    pub fn new(index: usize, descriptor: NetworkDescriptor) -> Result<Self, NetError> {
        match detect_kind(&descriptor) {
            Some(DriverKind::E1000) => Ok(Self::E1000(e1000::E1000::new(index, descriptor)?)),
            None => Err(NetError::UnsupportedDevice),
        }
    }

    pub fn init(&mut self) -> Result<(), NetError> {
        match self {
            DriverInstance::E1000(dev) => dev.init(),
        }
    }

    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), NetError> {
        match self {
            DriverInstance::E1000(dev) => dev.transmit(frame),
        }
    }

    pub fn drain_rx(&mut self, scratch: &mut [u8]) -> Option<usize> {
        match self {
            DriverInstance::E1000(dev) => dev.drain_rx(scratch),
        }
    }

    pub fn maintenance(&mut self) -> Result<(), NetError> {
        match self {
            DriverInstance::E1000(dev) => dev.maintenance(),
        }
    }

    pub fn mac_address(&self) -> [u8; 6] {
        match self {
            DriverInstance::E1000(dev) => dev.mac_address(),
        }
    }
}

enum DriverKind {
    E1000,
}

fn detect_kind(descriptor: &NetworkDescriptor) -> Option<DriverKind> {
    let pci = bootinfo::pci_device_by_location(
        descriptor.info.pci_segment,
        descriptor.info.pci_bus,
        descriptor.info.pci_device,
        descriptor.info.pci_function,
    )?;

    if pci.vendor_id == 0x8086 {
        match pci.device_id {
            0x100e | 0x100f | 0x150e | 0x153a | 0x10d3 => return Some(DriverKind::E1000),
            _ => {}
        }
    }

    None
}
