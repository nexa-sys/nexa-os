//! Device drivers for NexaOS
//!
//! This module contains hardware device drivers including:
//! - Serial port (UART 16550)
//! - PS/2 keyboard
//! - Framebuffer (GOP/VBE)
//! - VGA text mode buffer
//! - ACPI table parsing
//! - Parallel display compositor
//! - Random number generator (RDRAND/RDSEED + ChaCha20 CSPRNG)
//! - TTF font parser for Unicode/CJK support
//! - Block device abstraction layer

pub mod acpi;
pub mod block;
pub mod compositor;
pub mod framebuffer;
pub mod keyboard;
pub mod random;
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
    get_random_bytes, get_random_bytes_wait, get_random_u32, get_random_u64,
    init as init_random, is_initialized as random_is_initialized, sys_getrandom,
    GRND_INSECURE, GRND_NONBLOCK, GRND_RANDOM,
};
