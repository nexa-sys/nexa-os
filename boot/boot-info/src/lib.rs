#![no_std]

/// Signature used to validate NexaOS UEFI boot handoff blocks ("NEXAUEFI").
pub const BOOT_INFO_SIGNATURE: [u8; 8] = *b"NEXAUEFI";

/// Current version of the [`BootInfo`] structure.
pub const BOOT_INFO_VERSION: u16 = 1;

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

/// Boot handoff structure written by the UEFI isolation layer.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
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
    /// Reserved for future extensions (ABI padding to 128 bytes total).
    pub reserved: [u8; 32],
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
}
