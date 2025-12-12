//! Device drivers for NexaOS
//!
//! This module contains hardware device drivers including:
//! - Serial port (UART 16550)
//! - PS/2 keyboard
//! - Framebuffer (GOP/VBE)
//! - VGA text mode buffer
//! - ACPI table parsing
//! - Parallel display compositor (optional, `gfx_compositor` feature)
//! - Random number generator (RDRAND/RDSEED + ChaCha20 CSPRNG)
//! - TTF font parser for Unicode/CJK support (optional, `gfx_ttf` feature)
//! - Block device abstraction layer

pub mod acpi;
pub mod block;
#[cfg(feature = "gfx_compositor")]
pub mod compositor;
#[cfg(not(feature = "gfx_compositor"))]
pub mod compositor {
    //! Compositor stub module (feature disabled)
    //! Provides no-op implementations when compositor is disabled.

    pub struct CompositorStats {
        pub frames: u64,
        pub total_time_us: u64,
    }
    pub struct CompositionRegion;
    pub struct CompositionLayer;
    #[derive(Clone, Copy)]
    pub enum BlendMode {
        Replace,
        Alpha,
        Additive,
    }

    pub fn init() {}
    pub fn is_initialized() -> bool {
        false
    }
    pub fn worker_count() -> usize {
        0
    }
    pub fn stats() -> CompositorStats {
        CompositorStats {
            frames: 0,
            total_time_us: 0,
        }
    }
    pub fn debug_info() {}
    pub fn compose(_layers: &[CompositionLayer], _region: &CompositionRegion) {}
    pub fn fill_rect(_x: u32, _y: u32, _w: u32, _h: u32, _color: u32) {}
    pub fn parallel_fill(_ptr: *mut u8, _len: usize, _value: u8) {}
    pub fn scroll_up_fast(
        _dst: *mut u8,
        _src: *const u8,
        _copy_len: usize,
        _clear_ptr: *mut u8,
        _clear_len: usize,
        _clear_val: u8,
    ) {
    }
    pub fn ap_work_entry() {}
}
pub mod framebuffer;
pub mod keyboard;
pub mod random;
pub mod rtc;
pub mod serial;
pub mod vga;

// Re-export commonly used items from serial
pub use serial::{init as init_serial, try_read_byte, write_byte, write_bytes, write_str};

// Re-export from keyboard
pub use keyboard::{add_scancode, read_char, read_line, read_raw, read_raw_for_tty, try_read_char};

// Re-export from framebuffer
pub use framebuffer::{
    activate as activate_framebuffer, backspace as fb_backspace, clear as clear_framebuffer,
    early_init as early_init_framebuffer, install_from_bootinfo, is_ready as fb_is_ready,
    try_with_writer as fb_try_with_writer, write_bytes as fb_write_bytes,
    write_str as fb_write_str, FramebufferWriter,
};

// Re-export from vga
pub use vga::{
    clear_screen, init as init_vga, is_vga_ready, print_char, set_vga_ready, try_with_writer,
    with_writer, Color, ColorCode, Writer, VGA_READY, VGA_WRITER, WRITER,
};

// Re-export from acpi
pub use acpi::{cpus as acpi_cpus, init as init_acpi, lapic_base, CpuDescriptor, MAX_CPUS};

// Re-export from compositor
#[cfg(feature = "gfx_compositor")]
pub use compositor::{
    compose as compositor_compose, debug_info as compositor_debug_info,
    fill_rect as compositor_fill_rect, init as init_compositor,
    is_initialized as compositor_is_initialized, stats as compositor_stats,
    worker_count as compositor_worker_count, BlendMode, CompositionLayer, CompositionRegion,
    CompositorStats,
};
#[cfg(not(feature = "gfx_compositor"))]
pub use compositor::{
    compose as compositor_compose, debug_info as compositor_debug_info,
    fill_rect as compositor_fill_rect, init as init_compositor,
    is_initialized as compositor_is_initialized, stats as compositor_stats,
    worker_count as compositor_worker_count, BlendMode, CompositionLayer, CompositionRegion,
    CompositorStats,
};

// Re-export from random
pub use random::{
    add_entropy, dev_random_read, dev_random_write, dev_urandom_read, entropy_available,
    get_random_bytes, get_random_bytes_wait, get_random_u32, get_random_u64, init as init_random,
    is_initialized as random_is_initialized, sys_getrandom, GRND_INSECURE, GRND_NONBLOCK,
    GRND_RANDOM,
};
