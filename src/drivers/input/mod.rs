//! Unified Input Event Subsystem
//!
//! This module implements the Linux-compatible input event interface for
//! /dev/input/event* devices. It provides a unified way to access keyboard,
//! mouse, and other input devices through a common event structure.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
//! │  PS/2 Keyboard  │────▶│                 │     │ /dev/input/     │
//! │  Driver         │     │  Input Event    │────▶│   event0        │
//! └─────────────────┘     │  Subsystem      │     │   event1        │
//! ┌─────────────────┐     │                 │     │   mice          │
//! │  PS/2 Mouse     │────▶│                 │     │   ...           │
//! │  Driver         │     └─────────────────┘     └─────────────────┘
//! └─────────────────┘
//! ```
//!
//! # Event Types (Linux compatible)
//!
//! - `EV_SYN` (0): Synchronization events
//! - `EV_KEY` (1): Key/button events
//! - `EV_REL` (2): Relative movement events (mouse)
//! - `EV_ABS` (3): Absolute movement events (touchscreen)
//! - `EV_MSC` (4): Miscellaneous events
//! - `EV_LED` (17): LED control events

pub mod event;
pub mod keyboard;
pub mod mouse;

pub use event::{
    InputDeviceInfo, InputEvent, InputEventType, InputId, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, EV_ABS,
    EV_KEY, EV_LED, EV_MSC, EV_REL, EV_SYN, REL_WHEEL, REL_X, REL_Y, SYN_DROPPED, SYN_REPORT,
};
pub use keyboard::KeyboardDevice;
pub use mouse::MouseDevice;

use alloc::vec::Vec;
use spin::Mutex;

/// Maximum number of input devices
pub const MAX_INPUT_DEVICES: usize = 16;

/// Input device trait
pub trait InputDevice: Send + Sync {
    /// Get device name
    fn name(&self) -> &str;

    /// Get device ID information
    fn id(&self) -> InputId;

    /// Get supported event types bitmap
    fn event_types(&self) -> u32;

    /// Check if device has pending events
    fn has_events(&self) -> bool;

    /// Read next event (non-blocking)
    fn read_event(&mut self) -> Option<InputEvent>;

    /// Get all pending events
    fn drain_events(&mut self, buf: &mut [InputEvent]) -> usize;
}

/// Input device handle
struct InputDeviceHandle {
    device_type: InputDeviceType,
    index: usize,
    name: &'static str,
}

/// Input device types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDeviceType {
    /// Keyboard device
    Keyboard,
    /// Mouse device
    Mouse,
    /// Combined mice device (/dev/input/mice)
    CombinedMice,
}

/// Global input device registry
struct InputSubsystem {
    /// Device handles by event number
    devices: [Option<InputDeviceHandle>; MAX_INPUT_DEVICES],
    /// Number of registered devices
    device_count: usize,
    /// Initialized flag
    initialized: bool,
}

impl InputSubsystem {
    const fn new() -> Self {
        Self {
            devices: [const { None }; MAX_INPUT_DEVICES],
            device_count: 0,
            initialized: false,
        }
    }
}

static INPUT_SUBSYSTEM: Mutex<InputSubsystem> = Mutex::new(InputSubsystem::new());

/// Initialize the input subsystem
pub fn init() {
    let mut subsystem = INPUT_SUBSYSTEM.lock();
    if subsystem.initialized {
        return;
    }
    subsystem.initialized = true;

    // Register built-in keyboard device (event0)
    subsystem.devices[0] = Some(InputDeviceHandle {
        device_type: InputDeviceType::Keyboard,
        index: 0,
        name: "AT Translated Set 2 keyboard",
    });
    subsystem.device_count = 1;

    // Register mouse device (event1) - PS/2 mouse or emulated
    subsystem.devices[1] = Some(InputDeviceHandle {
        device_type: InputDeviceType::Mouse,
        index: 0,
        name: "PS/2 Generic Mouse",
    });
    subsystem.device_count = 2;

    drop(subsystem);

    // Initialize keyboard and mouse event buffers
    keyboard::init();
    mouse::init();

    // Register input devices in devfs
    crate::fs::devfs::register_input_event_device(0); // event0 (keyboard)
    crate::fs::devfs::register_input_event_device(1); // event1 (mouse)
    crate::fs::devfs::register_input_mice(); // /dev/input/mice

    crate::kinfo!("Input subsystem initialized (2 devices)");
}

/// Get device count
pub fn device_count() -> usize {
    INPUT_SUBSYSTEM.lock().device_count
}

/// Check if device exists
pub fn device_exists(event_num: usize) -> bool {
    if event_num >= MAX_INPUT_DEVICES {
        return false;
    }
    INPUT_SUBSYSTEM.lock().devices[event_num].is_some()
}

/// Get device type
pub fn get_device_type(event_num: usize) -> Option<InputDeviceType> {
    if event_num >= MAX_INPUT_DEVICES {
        return None;
    }
    INPUT_SUBSYSTEM.lock().devices[event_num]
        .as_ref()
        .map(|h| h.device_type)
}

/// Get device name
pub fn get_device_name(event_num: usize) -> Option<&'static str> {
    if event_num >= MAX_INPUT_DEVICES {
        return None;
    }
    INPUT_SUBSYSTEM.lock().devices[event_num]
        .as_ref()
        .map(|h| h.name)
}

/// Check if device has pending events
pub fn has_events(event_num: usize) -> bool {
    let Some(dev_type) = get_device_type(event_num) else {
        return false;
    };

    match dev_type {
        InputDeviceType::Keyboard => keyboard::has_events(),
        InputDeviceType::Mouse => mouse::has_events(),
        InputDeviceType::CombinedMice => mouse::has_events(),
    }
}

/// Read input events from a device
///
/// Returns the number of events read
pub fn read_events(event_num: usize, buf: &mut [InputEvent]) -> usize {
    let Some(dev_type) = get_device_type(event_num) else {
        return 0;
    };

    match dev_type {
        InputDeviceType::Keyboard => keyboard::read_events(buf),
        InputDeviceType::Mouse => mouse::read_events(buf),
        InputDeviceType::CombinedMice => mouse::read_events(buf),
    }
}

/// Get device info for ioctl EVIOCGID
pub fn get_device_id(event_num: usize) -> Option<InputId> {
    let Some(dev_type) = get_device_type(event_num) else {
        return None;
    };

    match dev_type {
        InputDeviceType::Keyboard => Some(keyboard::get_id()),
        InputDeviceType::Mouse => Some(mouse::get_id()),
        InputDeviceType::CombinedMice => Some(mouse::get_id()),
    }
}

/// Get device info
pub fn get_device_info(event_num: usize) -> Option<InputDeviceInfo> {
    let Some(dev_type) = get_device_type(event_num) else {
        return None;
    };

    match dev_type {
        InputDeviceType::Keyboard => Some(keyboard::get_info()),
        InputDeviceType::Mouse => Some(mouse::get_info()),
        InputDeviceType::CombinedMice => Some(mouse::get_info()),
    }
}

/// List all input devices
pub fn list_devices() -> Vec<(usize, &'static str, InputDeviceType)> {
    let subsystem = INPUT_SUBSYSTEM.lock();
    let mut result = Vec::new();

    for (i, dev) in subsystem.devices.iter().enumerate() {
        if let Some(ref handle) = dev {
            result.push((i, handle.name, handle.device_type));
        }
    }

    result
}
