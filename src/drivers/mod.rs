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
//! - Loop devices (file-backed block devices)
//! - Unified input event subsystem (keyboard, mouse)

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
pub mod input;
pub mod keyboard;
pub mod r#loop;
pub mod random;
pub mod rtc;
pub mod serial;
pub mod vga;
pub mod watchdog;

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

// Re-export from loop (loop device driver)
pub use r#loop::{
    attach as loop_attach, detach as loop_detach, get_device_info as loop_get_device_info,
    get_free as loop_get_free, init as init_loop, is_attached as loop_is_attached,
    loop_control_ioctl, loop_device_ioctl, read_sectors as loop_read_sectors,
    write_sectors as loop_write_sectors, LoopFlags, LoopInfo64, LOOP_CLR_FD, LOOP_CTL_ADD,
    LOOP_CTL_GET_FREE, LOOP_CTL_REMOVE, LOOP_GET_STATUS64, LOOP_SET_FD, LOOP_SET_STATUS64,
    MAX_LOOP_DEVICES,
};

// Re-export from input (unified input event subsystem)
pub use input::event::{
    InputDeviceInfo, InputEvent, InputId, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, EV_ABS, EV_KEY, EV_LED,
    EV_MSC, EV_REL, EV_SYN, REL_WHEEL, REL_X, REL_Y, SYN_DROPPED, SYN_REPORT,
};
pub use input::{
    device_count as input_device_count, device_exists as input_device_exists,
    get_device_id as input_get_device_id, get_device_info as input_get_device_info,
    has_events as input_has_events, init as init_input, list_devices as input_list_devices,
    read_events as input_read_events, InputDeviceType,
};

// Re-export from watchdog (hardware watchdog timer)
pub use watchdog::{
    disable as watchdog_disable, enable as watchdog_enable, feed as watchdog_feed,
    get_info as watchdog_get_info, get_timeout as watchdog_get_timeout,
    get_type as watchdog_get_type, init as init_watchdog, is_enabled as watchdog_is_enabled,
    is_initialized as watchdog_is_initialized, set_timeout as watchdog_set_timeout, watchdog_ioctl,
    WatchdogInfo, WatchdogType,
};
