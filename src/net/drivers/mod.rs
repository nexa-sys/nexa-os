use crate::{bootinfo, uefi_compat::NetworkDescriptor};

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
    /// Network stack not ready (feature disabled)
    NotReady,
}

/// Driver instance - modular drivers loaded from .nkm modules
pub enum DriverInstance {
    /// Modular driver (loaded from .nkm module like e1000.nkm)
    Modular { device_index: usize },
}

impl DriverInstance {
    /// Create a new driver instance for the given device
    /// 
    /// This checks if a modular driver is available for the device.
    /// Network drivers must be loaded as kernel modules (e.g., e1000.nkm).
    pub fn new(index: usize, descriptor: NetworkDescriptor) -> Result<Self, NetError> {
        // Try to find a modular driver
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
                    crate::kerror!(
                        "net: modular driver failed for device {}: {:?}",
                        index, e
                    );
                    return Err(NetError::ModuleNotLoaded);
                }
                
                crate::kinfo!("net: using modular driver for device {}", index);
                return Ok(Self::Modular { device_index: index });
            }
        }
        
        // No driver found - network drivers must be loaded as modules
        crate::kwarn!("net: no driver module loaded for device {}", index);
        Err(NetError::ModuleNotLoaded)
    }

    pub fn init(&mut self) -> Result<(), NetError> {
        match self {
            DriverInstance::Modular { .. } => {
                // Modular drivers are initialized during create_driver_instance
                Ok(())
            }
        }
    }

    pub fn update_dma_addresses(&mut self) {
        match self {
            DriverInstance::Modular { device_index } => {
                modular::update_dma_addresses(*device_index);
            }
        }
    }

    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), NetError> {
        match self {
            DriverInstance::Modular { device_index } => {
                modular::transmit(*device_index, frame)
                    .map_err(|_| NetError::TxBusy)
            }
        }
    }

    pub fn drain_rx(&mut self, scratch: &mut [u8]) -> Option<usize> {
        match self {
            DriverInstance::Modular { device_index } => {
                modular::drain_rx(*device_index, scratch)
            }
        }
    }

    pub fn maintenance(&mut self) -> Result<(), NetError> {
        match self {
            DriverInstance::Modular { device_index } => {
                modular::maintenance(*device_index)
                    .map_err(|_| NetError::HardwareFault)
            }
        }
    }

    pub fn mac_address(&self) -> [u8; 6] {
        match self {
            DriverInstance::Modular { device_index } => {
                modular::get_mac_address(*device_index).unwrap_or([0; 6])
            }
        }
    }
}

