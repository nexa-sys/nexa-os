#![no_std]

use core::slice;

/// Signature used to validate NexaOS UEFI boot handoff blocks ("NEXAUEFI").
pub const BOOT_INFO_SIGNATURE: [u8; 8] = *b"NEXAUEFI";

/// Current version of the [`BootInfo`] structure.
pub const BOOT_INFO_VERSION: u16 = 3;

/// Maximum number of device descriptors exported in [`BootInfo`].
pub const MAX_DEVICE_DESCRIPTORS: usize = 32;

/// Bit flags stored in [`BootInfo::flags`].
pub mod flags {
    /// Initramfs payload is present in [`BootInfo::initramfs`].
    pub const HAS_INITRAMFS: u32 = 1 << 0;
    /// Root filesystem image is present in [`BootInfo::rootfs`].
    pub const HAS_ROOTFS: u32 = 1 << 1;
    /// Command line string is available in [`BootInfo::cmdline`].
    pub const HAS_CMDLINE: u32 = 1 << 2;
    /// Framebuffer information (`gop`) is populated.
    pub const HAS_FRAMEBUFFER: u32 = 1 << 3;
    /// Device descriptors are populated in [`BootInfo::devices`].
    pub const HAS_DEVICE_TABLE: u32 = 1 << 4;
    /// Kernel was loaded at a different address than expected (has relocation offset).
    pub const HAS_KERNEL_OFFSET: u32 = 1 << 5;
    /// Kernel segment layout table is populated in [`BootInfo::kernel_segments`].
    pub const HAS_KERNEL_SEGMENTS: u32 = 1 << 6;
}

/// Flags describing capabilities/features of a device descriptor.
pub mod device_flags {
    /// Device provides block storage access (e.g. virtio-blk, AHCI).
    pub const BLOCK: u16 = 1 << 0;
    /// Device provides packet network access.
    pub const NETWORK: u16 = 1 << 1;
    /// Device exposes USB host controller capabilities.
    pub const USB_HOST: u16 = 1 << 2;
    /// Device exposes GPU/framebuffer functionality beyond GOP (e.g. PCI GPU).
    pub const GRAPHICS: u16 = 1 << 3;
    /// Device provides HID input (keyboard/mouse).
    pub const HID_INPUT: u16 = 1 << 4;
    /// Device has MSI/MSI-X enabled during boot.
    pub const MSI_ENABLED: u16 = 1 << 8;
}

/// Device kinds exported via [`DeviceDescriptor`].
#[repr(u16)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DeviceKind {
    /// PCI/PCIe device configured by firmware.
    Pci = 1,
    /// ACPI-described platform device.
    Acpi = 2,
    /// Logical block device (Block I/O protocol).
    Block = 3,
    /// Network interface (Simple Network Protocol).
    Network = 4,
    /// USB host controller (xHCI/EHCI/OHCI).
    UsbHost = 5,
    /// HID input device (keyboard/mouse via USB or PS/2).
    HidInput = 6,
    /// Other/unknown.
    Other = 0xFFFF,
}

/// A physical memory region handed off by the UEFI loader.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct MemoryRegion {
    pub phys_addr: u64,
    pub length: u64,
}

impl MemoryRegion {
    /// Returns an empty region descriptor.
    pub const fn empty() -> Self {
        Self {
            phys_addr: 0,
            length: 0,
        }
    }

    /// Indicates whether the region contains usable data.
    pub const fn is_empty(&self) -> bool {
        self.length == 0 || self.phys_addr == 0
    }
}

/// RGB framebuffer description equivalent to the Multiboot structure.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FramebufferInfo {
    pub address: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub red_position: u8,
    pub red_size: u8,
    pub green_position: u8,
    pub green_size: u8,
    pub blue_position: u8,
    pub blue_size: u8,
    pub reserved: [u8; 5],
}

impl FramebufferInfo {
    /// Returns `true` when the framebuffer description looks valid.
    pub const fn is_valid(&self) -> bool {
        self.address != 0 && self.width != 0 && self.height != 0 && self.bpp >= 16
    }
}

/// PCI BAR description captured after firmware initialisation.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct PciBarInfo {
    pub base: u64,
    pub length: u64,
    pub bar_flags: u32,
    pub reserved: u32,
}

impl PciBarInfo {
    pub const fn empty() -> Self {
        Self {
            base: 0,
            length: 0,
            bar_flags: 0,
            reserved: 0,
        }
    }
}

/// PCI specific data exported through [`DeviceDescriptor`].
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct PciDeviceInfo {
    pub segment: u16,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub header_type: u8,
    pub reserved: u8,
    pub device_flags: u16,
    pub bars: [PciBarInfo; 6],
}

impl PciDeviceInfo {
    pub const fn empty() -> Self {
        Self {
            segment: 0,
            bus: 0,
            device: 0,
            function: 0,
            vendor_id: 0,
            device_id: 0,
            class_code: 0,
            subclass: 0,
            prog_if: 0,
            revision: 0,
            interrupt_line: 0,
            interrupt_pin: 0,
            header_type: 0,
            reserved: 0,
            device_flags: 0,
            bars: [PciBarInfo::empty(); 6],
        }
    }
}

/// Flags describing PCI BAR properties.
pub mod bar_flags {
    /// BAR targets I/O port space instead of MMIO.
    pub const IO_SPACE: u32 = 1 << 0;
    /// BAR is prefetchable.
    pub const PREFETCHABLE: u32 = 1 << 1;
    /// BAR spans a 64-bit address range.
    pub const MEMORY_64BIT: u32 = 1 << 2;
}

/// Flags associated with [`BlockDeviceInfo`].
pub mod block_flags {
    /// Media is currently present in the device.
    pub const MEDIA_PRESENT: u16 = 1 << 0;
    /// Media is marked read only.
    pub const READ_ONLY: u16 = 1 << 1;
    /// Device reports removable media.
    pub const REMOVABLE: u16 = 1 << 2;
    /// Block device is a logical partition view (not whole disk).
    pub const LOGICAL_PARTITION: u16 = 1 << 3;
    /// Write caching is enabled on the device.
    pub const WRITE_CACHING: u16 = 1 << 4;
}

/// Flags associated with [`NetworkDeviceInfo`].
pub mod network_flags {
    /// Link is reported as up.
    pub const LINK_UP: u16 = 1 << 0;
    /// Media is present/connected.
    pub const MEDIA_PRESENT: u16 = 1 << 1;
    /// Device allows MAC address changes.
    pub const MAC_MUTABLE: u16 = 1 << 2;
    /// Multiple TX operations supported concurrently.
    pub const MULTIPLE_TX: u16 = 1 << 3;
}

/// Logical block device information (UEFI Block I/O media).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct BlockDeviceInfo {
    pub pci_segment: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub block_size: u32,
    pub last_block: u64,
    pub media_id: u32,
    pub io_align: u32,
    pub logical_blocks_per_physical: u32,
    pub optimal_transfer_granularity: u32,
    pub lowest_aligned_lba: u64,
    pub flags: u16,
    pub reserved: [u8; 46],
}

impl BlockDeviceInfo {
    pub const fn empty() -> Self {
        Self {
            pci_segment: 0,
            pci_bus: 0,
            pci_device: 0,
            pci_function: 0,
            block_size: 0,
            last_block: 0,
            media_id: 0,
            io_align: 0,
            logical_blocks_per_physical: 0,
            optimal_transfer_granularity: 0,
            lowest_aligned_lba: 0,
            flags: 0,
            reserved: [0; 46],
        }
    }
}

/// Network device information (UEFI Simple Network Protocol).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct NetworkDeviceInfo {
    pub pci_segment: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub if_type: u8,
    pub mac_len: u8,
    pub mac_address: [u8; 32],
    pub max_packet_size: u32,
    pub link_speed_mbps: u32,
    pub receive_filter_mask: u32,
    pub receive_filter_setting: u32,
    pub flags: u16,
    pub reserved: [u8; 46],
}

impl NetworkDeviceInfo {
    pub const fn empty() -> Self {
        Self {
            pci_segment: 0,
            pci_bus: 0,
            pci_device: 0,
            pci_function: 0,
            if_type: 0,
            mac_len: 0,
            mac_address: [0; 32],
            max_packet_size: 0,
            link_speed_mbps: 0,
            receive_filter_mask: 0,
            receive_filter_setting: 0,
            flags: 0,
            reserved: [0; 46],
        }
    }
}

/// USB host controller information (xHCI/EHCI/OHCI).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct UsbHostInfo {
    pub pci_segment: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    /// USB controller type: 0=unknown, 1=OHCI, 2=EHCI, 3=xHCI
    pub controller_type: u8,
    /// Number of root hub ports
    pub port_count: u8,
    /// USB version (e.g., 0x0200 for USB 2.0, 0x0300 for USB 3.0)
    pub usb_version: u16,
    /// MMIO base address for controller registers
    pub mmio_base: u64,
    /// MMIO region size
    pub mmio_size: u64,
    /// Interrupt line (if using legacy interrupts)
    pub interrupt_line: u8,
    pub reserved: [u8; 151],
}

impl UsbHostInfo {
    pub const fn empty() -> Self {
        Self {
            pci_segment: 0,
            pci_bus: 0,
            pci_device: 0,
            pci_function: 0,
            controller_type: 0,
            port_count: 0,
            usb_version: 0,
            mmio_base: 0,
            mmio_size: 0,
            interrupt_line: 0,
            reserved: [0; 151],
        }
    }
}

/// HID input device information (keyboard/mouse).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct HidInputInfo {
    /// Device type: 0=unknown, 1=keyboard, 2=mouse, 3=combined
    pub device_type: u8,
    /// Interface protocol: 0=none, 1=keyboard, 2=mouse
    pub protocol: u8,
    /// Connected via USB (vs PS/2 emulation)
    pub is_usb: u8,
    /// Parent USB host controller PCI address (if USB)
    pub usb_host_bus: u8,
    pub usb_host_device: u8,
    pub usb_host_function: u8,
    /// USB device address
    pub usb_device_addr: u8,
    /// USB endpoint for input
    pub usb_endpoint: u8,
    /// Vendor ID (if available)
    pub vendor_id: u16,
    /// Product ID (if available)
    pub product_id: u16,
    pub reserved: [u8; 176],
}

impl HidInputInfo {
    pub const fn empty() -> Self {
        Self {
            device_type: 0,
            protocol: 0,
            is_usb: 0,
            usb_host_bus: 0,
            usb_host_device: 0,
            usb_host_function: 0,
            usb_device_addr: 0,
            usb_endpoint: 0,
            vendor_id: 0,
            product_id: 0,
            reserved: [0; 176],
        }
    }
}

/// Fixed-size payload used for device descriptors.
pub const DEVICE_DATA_SIZE: usize = 192;

/// Union used to pack various device payloads.
#[repr(C)]
#[derive(Copy, Clone)]
pub union DeviceData {
    pub pci: PciDeviceInfo,
    pub block: BlockDeviceInfo,
    pub network: NetworkDeviceInfo,
    pub usb_host: UsbHostInfo,
    pub hid_input: HidInputInfo,
    pub raw: [u8; DEVICE_DATA_SIZE],
}

/// Generic device descriptor.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DeviceDescriptor {
    pub kind: DeviceKind,
    pub flags: u16,
    pub reserved: u32,
    pub data: DeviceData,
}

impl DeviceDescriptor {
    pub const fn empty() -> Self {
        Self {
            kind: DeviceKind::Other,
            flags: 0,
            reserved: 0,
            data: DeviceData { raw: [0; DEVICE_DATA_SIZE] },
        }
    }
}

/// Description of an individual kernel segment as loaded by the UEFI stub.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct KernelSegment {
    /// Physical address the kernel expected this segment to occupy.
    pub expected_addr: u64,
    /// Physical address where the loader actually placed the segment.
    pub actual_addr: u64,
    /// Number of bytes mapped for this segment.
    pub mem_size: u64,
}

/// Boot handoff structure written by the UEFI isolation layer.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BootInfo {
    /// Must contain [`BOOT_INFO_SIGNATURE`].
    pub signature: [u8; 8],
    /// Structure version. Should match [`BOOT_INFO_VERSION`].
    pub version: u16,
    /// Total structure size in bytes (for forward compatibility).
    pub size: u16,
    /// Bitfield using constants from [`flags`].
    pub flags: u32,
    /// Physical address of the loaded initramfs archive.
    pub initramfs: MemoryRegion,
    /// Physical address of the root filesystem (usually ext2) image.
    pub rootfs: MemoryRegion,
    /// Optional kernel command line (UTF-8, not necessarily NUL-terminated).
    pub cmdline: MemoryRegion,
    /// Framebuffer information if available.
    pub framebuffer: FramebufferInfo,
    /// Number of valid entries in `devices`.
    pub device_count: u16,
    pub _padding: u16,
    /// Firmware-prepared device descriptors.
    pub devices: [DeviceDescriptor; MAX_DEVICE_DESCRIPTORS],
    /// Kernel entry point as encoded in the ELF image.
    pub kernel_expected_entry: u64,
    /// Actual entry address after the UEFI loader applied its relocation.
    pub kernel_actual_entry: u64,
    /// Physical memory region describing the kernel segment table.
    pub kernel_segments: MemoryRegion,
    /// Kernel load offset (actual_address - expected_address).
    /// Only valid if HAS_KERNEL_OFFSET flag is set.
    pub kernel_load_offset: i64,
    /// Reserved for future extensions.
    pub reserved: [u8; 24],
}

impl BootInfo {
    /// Returns whether the signature matches [`BOOT_INFO_SIGNATURE`].
    pub fn has_valid_signature(&self) -> bool {
        self.signature == BOOT_INFO_SIGNATURE
    }

    /// Returns whether this structure declares an initramfs payload.
    pub fn has_initramfs(&self) -> bool {
        (self.flags & flags::HAS_INITRAMFS) != 0 && !self.initramfs.is_empty()
    }

    /// Returns whether this structure declares a root filesystem payload.
    pub fn has_rootfs(&self) -> bool {
        (self.flags & flags::HAS_ROOTFS) != 0 && !self.rootfs.is_empty()
    }

    /// Returns whether a command line string is available.
    pub fn has_cmdline(&self) -> bool {
        (self.flags & flags::HAS_CMDLINE) != 0 && !self.cmdline.is_empty()
    }

    /// Returns whether framebuffer data is present.
    pub fn has_framebuffer(&self) -> bool {
        (self.flags & flags::HAS_FRAMEBUFFER) != 0 && self.framebuffer.is_valid()
    }

    /// Returns `true` if the device table contains entries.
    pub fn has_device_table(&self) -> bool {
        (self.flags & flags::HAS_DEVICE_TABLE) != 0 && self.device_count != 0
    }

    /// Returns whether kernel was loaded with a relocation offset.
    pub fn has_kernel_offset(&self) -> bool {
        (self.flags & flags::HAS_KERNEL_OFFSET) != 0
    }

    /// Returns the populated device descriptors.
    pub fn devices(&self) -> &[DeviceDescriptor] {
        let count = core::cmp::min(self.device_count as usize, MAX_DEVICE_DESCRIPTORS);
        &self.devices[..count]
    }

    /// Returns whether the loader provided explicit kernel segment layout.
    pub fn has_kernel_segments(&self) -> bool {
        (self.flags & flags::HAS_KERNEL_SEGMENTS) != 0 && !self.kernel_segments.is_empty()
    }

    /// Returns the kernel segments supplied by the loader, if present.
    pub fn kernel_segments(&self) -> Option<&[KernelSegment]> {
        if !self.has_kernel_segments() {
            return None;
        }
        if self.kernel_segments.length == 0 {
            return None;
        }
        let count = (self.kernel_segments.length as usize) / core::mem::size_of::<KernelSegment>();
        if count == 0 {
            return None;
        }
        unsafe {
            Some(slice::from_raw_parts(
                self.kernel_segments.phys_addr as *const KernelSegment,
                count,
            ))
        }
    }
}
