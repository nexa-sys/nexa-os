mod e1000;

use crate::{bootinfo, uefi_compat::NetworkDescriptor};

pub use e1000::E1000;

/// Re-export modular driver support
pub use super::modular;

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
    InvalidState,
    ConnectionClosed,
    WouldBlock,
    NoDevice,
    /// Driver module not loaded
    ModuleNotLoaded,
}

/// Driver instance - can be either built-in or modular
pub enum DriverInstance {
    /// Built-in E1000 driver (fallback when module not loaded)
    E1000(e1000::E1000),
    /// Modular driver (loaded from .nkm module)
    Modular { device_index: usize },
}

impl DriverInstance {
    /// Create a new driver instance for the given device
    /// 
    /// This first checks if a modular driver is available for the device,
    /// and falls back to built-in drivers if not.
    pub fn new(index: usize, descriptor: NetworkDescriptor) -> Result<Self, NetError> {
        // First, try to find a modular driver
        if let Some(pci) = bootinfo::pci_device_by_location(
            descriptor.info.pci_segment,
            descriptor.info.pci_bus,
            descriptor.info.pci_device,
            descriptor.info.pci_function,
        ) {
            if let Some(driver_idx) = modular::find_driver_for_device(pci.vendor_id, pci.device_id) {
                // Create modular driver instance
                let mod_desc = modular::NetDeviceDescriptor {
                    index,
                    mmio_base: descriptor.mmio_base,
                    mmio_length: descriptor.mmio_length,
                    pci_segment: descriptor.info.pci_segment,
                    pci_bus: descriptor.info.pci_bus,
                    pci_device: descriptor.info.pci_device,
                    pci_function: descriptor.info.pci_function,
                    interrupt_line: descriptor.interrupt_line,
                    mac_len: descriptor.info.mac_len,
                    mac_address: descriptor.info.mac_address,
                    _reserved: [0; 5],
                };
                
                if let Err(e) = modular::create_driver_instance(driver_idx, index, &mod_desc) {
                    crate::kwarn!(
                        "net: modular driver failed for device {}: {:?}, falling back to built-in",
                        index, e
                    );
                    // Fall through to built-in driver
                } else {
                    crate::kinfo!("net: using modular driver for device {}", index);
                    return Ok(Self::Modular { device_index: index });
                }
            }
        }
        
        // Fall back to built-in driver detection
        match detect_kind(&descriptor) {
            Some(DriverKind::E1000) => Ok(Self::E1000(e1000::E1000::new(index, descriptor)?)),
            None => Err(NetError::UnsupportedDevice),
        }
    }

    pub fn init(&mut self) -> Result<(), NetError> {
        match self {
            DriverInstance::E1000(dev) => dev.init(),
            DriverInstance::Modular { .. } => {
                // Modular drivers are initialized during create_driver_instance
                Ok(())
            }
        }
    }

    pub fn update_dma_addresses(&mut self) {
        match self {
            DriverInstance::E1000(dev) => dev.update_dma_addresses(),
            DriverInstance::Modular { device_index } => {
                modular::update_dma_addresses(*device_index);
            }
        }
    }

    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), NetError> {
        match self {
            DriverInstance::E1000(dev) => dev.transmit(frame),
            DriverInstance::Modular { device_index } => {
                modular::transmit(*device_index, frame)
                    .map_err(|_| NetError::TxBusy)
            }
        }
    }

    pub fn drain_rx(&mut self, scratch: &mut [u8]) -> Option<usize> {
        match self {
            DriverInstance::E1000(dev) => dev.drain_rx(scratch),
            DriverInstance::Modular { device_index } => {
                modular::drain_rx(*device_index, scratch)
            }
        }
    }

    pub fn maintenance(&mut self) -> Result<(), NetError> {
        match self {
            DriverInstance::E1000(dev) => dev.maintenance(),
            DriverInstance::Modular { device_index } => {
                modular::maintenance(*device_index)
                    .map_err(|_| NetError::HardwareFault)
            }
        }
    }

    pub fn mac_address(&self) -> [u8; 6] {
        match self {
            DriverInstance::E1000(dev) => dev.mac_address(),
            DriverInstance::Modular { device_index } => {
                modular::get_mac_address(*device_index).unwrap_or([0; 6])
            }
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

