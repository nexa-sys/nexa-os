//! Framebuffer specification types
//!
//! Contains the hardware framebuffer specification structure
//! and related initialization functions.

use multiboot2::{FramebufferField, FramebufferTag, FramebufferType};
use nexa_boot_info::FramebufferInfo as BootFramebufferInfo;

use crate::kinfo;

/// Hardware framebuffer specification
///
/// Contains all the information needed to render to the framebuffer,
/// including the physical address, dimensions, and pixel format.
#[derive(Clone, Copy, Debug)]
pub struct FramebufferSpec {
    pub address: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub red: FramebufferField,
    pub green: FramebufferField,
    pub blue: FramebufferField,
}

impl FramebufferSpec {
    /// Create spec from multiboot2 framebuffer tag
    pub fn from_tag(tag: &FramebufferTag) -> Option<Self> {
        match tag.buffer_type() {
            Ok(FramebufferType::RGB { red, green, blue }) => Some(Self {
                address: tag.address(),
                pitch: tag.pitch(),
                width: tag.width(),
                height: tag.height(),
                bpp: tag.bpp(),
                red,
                green,
                blue,
            }),
            Ok(FramebufferType::Indexed { .. }) => {
                crate::kwarn!("Indexed framebuffer detected; unsupported for now");
                None
            }
            Ok(FramebufferType::Text) => None,
            Err(err) => {
                crate::kwarn!("Unknown framebuffer type: {:?}", err);
                None
            }
        }
    }

    /// Create spec from UEFI boot info
    pub fn from_bootinfo(info: &BootFramebufferInfo) -> Option<Self> {
        if !info.is_valid() {
            return None;
        }

        Some(Self {
            address: info.address,
            pitch: info.pitch,
            width: info.width,
            height: info.height,
            bpp: info.bpp,
            red: FramebufferField {
                position: info.red_position,
                size: info.red_size,
            },
            green: FramebufferField {
                position: info.green_position,
                size: info.green_size,
            },
            blue: FramebufferField {
                position: info.blue_position,
                size: info.blue_size,
            },
        })
    }

    /// Log framebuffer info
    pub fn log_info(&self, source: &str) {
        kinfo!(
            "Framebuffer {} {}: {}x{} {}bpp (pitch {})",
            source,
            if self.address != 0 {
                "discovered"
            } else {
                "configured"
            },
            self.width,
            self.height,
            self.bpp,
            self.pitch
        );
    }

    /// Calculate total framebuffer size in bytes
    pub fn size(&self) -> usize {
        (self.pitch as usize).saturating_mul(self.height as usize)
    }

    /// Check if spec is valid for rendering
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.bpp >= 16 && self.pitch > 0
    }
}
