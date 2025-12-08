//! Framebuffer graphics driver
//!
//! This module provides framebuffer-based text console output with:
//! - ANSI escape sequence support for colors and attributes
//! - High-performance rendering with GPU-style optimizations
//! - Multi-core parallel operations via compositor (optional, `gfx_compositor` feature)
//! - TrueType font (TTF) support for Chinese and Unicode characters (optional, `gfx_ttf` feature)
//!
//! # Module Organization
//!
//! - `ansi`: ANSI escape sequence parser
//! - `color`: Color types and palettes
//! - `font`: TTF font parsing and rendering (post-pivot_root, requires `gfx_ttf` feature)
//! - `render`: Low-level rendering primitives
//! - `spec`: Framebuffer hardware specification
//! - `writer`: High-level text console writer

mod ansi;
mod color;
#[cfg(feature = "gfx_ttf")]
pub mod font;
#[cfg(not(feature = "gfx_ttf"))]
pub mod font {
    //! TTF font stub module (feature disabled)
    //! Falls back to 8x8 bitmap font for ASCII only.
    
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FontSystemState { Uninitialized, Initializing, Ready, Failed }
    
    /// Glyph bitmap for rendering (stub - never instantiated)
    #[derive(Clone)]
    pub struct GlyphBitmap {
        pub width: u16,
        pub height: u16,
        pub bearing_x: i16,
        pub bearing_y: i16,
        pub advance: u16,
        pub data: alloc::vec::Vec<u8>,
    }
    
    pub fn is_ready() -> bool { false }
    pub fn state() -> FontSystemState { FontSystemState::Uninitialized }
    pub fn init_after_pivot_root() {
        crate::kinfo!("TTF font support disabled (gfx_ttf feature not enabled)");
    }
    pub fn get_glyph(_ch: char, _size: u16) -> Option<GlyphBitmap> { None }
    pub fn get_baseline_offset(_size: u16) -> u16 { 0 }
}
mod render;
mod spec;
mod writer;

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

use multiboot2::BootInformation;
use nexa_boot_info::FramebufferInfo as BootFramebufferInfo;
use spin::Mutex;

use crate::kinfo;

pub use spec::FramebufferSpec;
pub use writer::FramebufferWriter;

// Global state
static FRAMEBUFFER_SPEC: Mutex<Option<FramebufferSpec>> = Mutex::new(None);
static FRAMEBUFFER_READY: AtomicBool = AtomicBool::new(false);
static FRAMEBUFFER_WRITER: Mutex<Option<FramebufferWriter>> = Mutex::new(None);

/// Early initialization - extract framebuffer info from multiboot2
pub fn early_init(boot_info: &BootInformation<'_>) {
    if let Some(tag_result) = boot_info.framebuffer_tag() {
        match tag_result {
            Ok(tag) => {
                if let Some(spec) = FramebufferSpec::from_tag(tag) {
                    spec.log_info("multiboot2");
                    *FRAMEBUFFER_SPEC.lock() = Some(spec);
                }
            }
            Err(err) => {
                crate::kwarn!("Failed to decode framebuffer tag: {:?}", err);
            }
        }
    }
}

/// Install framebuffer from UEFI boot info
pub fn install_from_bootinfo(info: &BootFramebufferInfo) {
    if let Some(spec) = FramebufferSpec::from_bootinfo(info) {
        kinfo!(
            "Framebuffer provided by UEFI: {}x{} {}bpp (pitch {})",
            spec.width,
            spec.height,
            spec.bpp,
            spec.pitch
        );
        *FRAMEBUFFER_SPEC.lock() = Some(spec);
        FRAMEBUFFER_READY.store(false, Ordering::SeqCst);
    }
}

/// Activate the framebuffer for rendering
///
/// Maps the framebuffer memory and initializes the writer.
/// This should be called after memory management is initialized.
pub fn activate() {
    if FRAMEBUFFER_READY.load(Ordering::SeqCst) {
        return;
    }

    let spec = {
        let guard = FRAMEBUFFER_SPEC.lock();
        match *guard {
            Some(spec) => spec,
            None => return,
        }
    };

    let length = spec.size();
    if length == 0 {
        return;
    }

    let buffer_ptr = match unsafe { crate::paging::map_device_region(spec.address, length) } {
        Ok(ptr) => ptr,
        Err(err) => {
            crate::kwarn!("Failed to map framebuffer: {:?}", err);
            return;
        }
    };

    let mut writer_guard = FRAMEBUFFER_WRITER.lock();
    let mut activated = false;

    if writer_guard.is_none() {
        if let Some(mut writer) = FramebufferWriter::new(buffer_ptr, spec) {
            writer.clear();
            *writer_guard = Some(writer);
            FRAMEBUFFER_READY.store(true, Ordering::SeqCst);
            activated = true;
        }
    }

    drop(writer_guard);

    if activated {
        kinfo!(
            "Framebuffer activated at {:#x} ({}x{} @ {}bpp)",
            spec.address,
            spec.width,
            spec.height,
            spec.bpp
        );
    }
}

/// Check if the framebuffer is ready for use
pub fn is_ready() -> bool {
    FRAMEBUFFER_READY.load(Ordering::SeqCst)
}

/// Clear the framebuffer screen
pub fn clear() {
    if let Some(writer) = FRAMEBUFFER_WRITER.lock().as_mut() {
        writer.clear();
    }
}

/// Handle backspace
pub fn backspace() {
    if let Some(writer) = FRAMEBUFFER_WRITER.lock().as_mut() {
        writer.backspace();
    }
}

/// Write a string to the framebuffer
pub fn write_str(text: &str) {
    if !FRAMEBUFFER_READY.load(Ordering::SeqCst) {
        return;
    }

    if let Some(mut guard) = FRAMEBUFFER_WRITER.try_lock() {
        if let Some(writer) = guard.as_mut() {
            use core::fmt::Write;
            let _ = writer.write_str(text);
        }
    }
}

/// Write bytes to the framebuffer
pub fn write_bytes(bytes: &[u8]) {
    if !FRAMEBUFFER_READY.load(Ordering::SeqCst) {
        return;
    }

    // Try to interpret as UTF-8 string for TTF rendering
    if let Ok(text) = core::str::from_utf8(bytes) {
        if let Some(mut guard) = FRAMEBUFFER_WRITER.try_lock() {
            if let Some(writer) = guard.as_mut() {
                use core::fmt::Write;
                let _ = writer.write_str(text);
            }
        }
    } else {
        // Fallback to byte-by-byte for invalid UTF-8
        if let Some(mut guard) = FRAMEBUFFER_WRITER.try_lock() {
            if let Some(writer) = guard.as_mut() {
                for &byte in bytes {
                    writer.process_byte(byte);
                }
            }
        }
    }
}

/// Execute a closure with access to the framebuffer writer
pub fn try_with_writer<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut FramebufferWriter) -> R,
{
    FRAMEBUFFER_WRITER.lock().as_mut().map(f)
}

/// Print formatted output to the framebuffer
pub(crate) fn _print(args: fmt::Arguments<'_>) {
    use core::fmt::Write;
    if let Some(writer) = FRAMEBUFFER_WRITER.lock().as_mut() {
        writer.write_fmt(args).ok();
    }
}
