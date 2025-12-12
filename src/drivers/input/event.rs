//! Input Event Types and Structures
//!
//! This module defines the Linux-compatible input event structures
//! used by the /dev/input/event* interface.

/// Input event structure (Linux compatible - struct input_event)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct InputEvent {
    /// Event timestamp (seconds)
    pub time_sec: u64,
    /// Event timestamp (microseconds)
    pub time_usec: u64,
    /// Event type (EV_KEY, EV_REL, etc.)
    pub event_type: u16,
    /// Event code (key code, axis code, etc.)
    pub code: u16,
    /// Event value (key state, relative movement, etc.)
    pub value: i32,
}

impl InputEvent {
    pub const fn new(event_type: u16, code: u16, value: i32) -> Self {
        Self {
            time_sec: 0,
            time_usec: 0,
            event_type,
            code,
            value,
        }
    }

    /// Create event with current timestamp
    pub fn with_timestamp(event_type: u16, code: u16, value: i32) -> Self {
        let (sec, usec) = get_timestamp();
        Self {
            time_sec: sec,
            time_usec: usec,
            event_type,
            code,
            value,
        }
    }

    /// Create a synchronization event
    pub fn sync() -> Self {
        Self::with_timestamp(EV_SYN, SYN_REPORT, 0)
    }
}

/// Get current timestamp as (seconds, microseconds)
fn get_timestamp() -> (u64, u64) {
    // Use TSC for timing (approximate, ~2GHz assumed)
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    // Assume ~2GHz TSC frequency for rough time estimate
    let usec_total = tsc / 2000; // TSC ticks to microseconds
    let sec = usec_total / 1_000_000;
    let usec = usec_total % 1_000_000;
    (sec, usec)
}

// ============================================================================
// Event Types (EV_*)
// ============================================================================

/// Synchronization events
pub const EV_SYN: u16 = 0x00;
/// Key/button events
pub const EV_KEY: u16 = 0x01;
/// Relative movement events (mouse)
pub const EV_REL: u16 = 0x02;
/// Absolute movement events (touchscreen)
pub const EV_ABS: u16 = 0x03;
/// Miscellaneous events
pub const EV_MSC: u16 = 0x04;
/// Switch events
pub const EV_SW: u16 = 0x05;
/// LED control events
pub const EV_LED: u16 = 0x11;
/// Sound events
pub const EV_SND: u16 = 0x12;
/// Auto-repeat events
pub const EV_REP: u16 = 0x14;
/// Force feedback events
pub const EV_FF: u16 = 0x15;
/// Power events
pub const EV_PWR: u16 = 0x16;
/// Force feedback status
pub const EV_FF_STATUS: u16 = 0x17;

// ============================================================================
// Synchronization Codes (SYN_*)
// ============================================================================

/// Used to synchronize and separate events
pub const SYN_REPORT: u16 = 0;
/// Configuration changed
pub const SYN_CONFIG: u16 = 1;
/// MT slot changed
pub const SYN_MT_REPORT: u16 = 2;
/// Events dropped
pub const SYN_DROPPED: u16 = 3;

// ============================================================================
// Relative Axes (REL_*)
// ============================================================================

/// Relative X movement
pub const REL_X: u16 = 0x00;
/// Relative Y movement
pub const REL_Y: u16 = 0x01;
/// Relative Z movement
pub const REL_Z: u16 = 0x02;
/// Relative RX movement
pub const REL_RX: u16 = 0x03;
/// Relative RY movement
pub const REL_RY: u16 = 0x04;
/// Relative RZ movement
pub const REL_RZ: u16 = 0x05;
/// Horizontal wheel
pub const REL_HWHEEL: u16 = 0x06;
/// Dial
pub const REL_DIAL: u16 = 0x07;
/// Vertical wheel
pub const REL_WHEEL: u16 = 0x08;
/// Misc
pub const REL_MISC: u16 = 0x09;

// ============================================================================
// Absolute Axes (ABS_*)
// ============================================================================

/// Absolute X position
pub const ABS_X: u16 = 0x00;
/// Absolute Y position
pub const ABS_Y: u16 = 0x01;
/// Absolute Z position
pub const ABS_Z: u16 = 0x02;
/// Absolute RX position
pub const ABS_RX: u16 = 0x03;
/// Absolute RY position
pub const ABS_RY: u16 = 0x04;
/// Absolute RZ position
pub const ABS_RZ: u16 = 0x05;
/// Throttle
pub const ABS_THROTTLE: u16 = 0x06;
/// Rudder
pub const ABS_RUDDER: u16 = 0x07;
/// Wheel
pub const ABS_WHEEL: u16 = 0x08;
/// Gas
pub const ABS_GAS: u16 = 0x09;
/// Brake
pub const ABS_BRAKE: u16 = 0x0a;
/// Pressure
pub const ABS_PRESSURE: u16 = 0x18;
/// Distance
pub const ABS_DISTANCE: u16 = 0x19;
/// Tool width
pub const ABS_TOOL_WIDTH: u16 = 0x1c;

// ============================================================================
// Key/Button Codes (KEY_* / BTN_*)
// ============================================================================

// Reserved key codes
pub const KEY_RESERVED: u16 = 0;
pub const KEY_ESC: u16 = 1;
pub const KEY_1: u16 = 2;
pub const KEY_2: u16 = 3;
pub const KEY_3: u16 = 4;
pub const KEY_4: u16 = 5;
pub const KEY_5: u16 = 6;
pub const KEY_6: u16 = 7;
pub const KEY_7: u16 = 8;
pub const KEY_8: u16 = 9;
pub const KEY_9: u16 = 10;
pub const KEY_0: u16 = 11;
pub const KEY_MINUS: u16 = 12;
pub const KEY_EQUAL: u16 = 13;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_TAB: u16 = 15;
pub const KEY_Q: u16 = 16;
pub const KEY_W: u16 = 17;
pub const KEY_E: u16 = 18;
pub const KEY_R: u16 = 19;
pub const KEY_T: u16 = 20;
pub const KEY_Y: u16 = 21;
pub const KEY_U: u16 = 22;
pub const KEY_I: u16 = 23;
pub const KEY_O: u16 = 24;
pub const KEY_P: u16 = 25;
pub const KEY_LEFTBRACE: u16 = 26;
pub const KEY_RIGHTBRACE: u16 = 27;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_A: u16 = 30;
pub const KEY_S: u16 = 31;
pub const KEY_D: u16 = 32;
pub const KEY_F: u16 = 33;
pub const KEY_G: u16 = 34;
pub const KEY_H: u16 = 35;
pub const KEY_J: u16 = 36;
pub const KEY_K: u16 = 37;
pub const KEY_L: u16 = 38;
pub const KEY_SEMICOLON: u16 = 39;
pub const KEY_APOSTROPHE: u16 = 40;
pub const KEY_GRAVE: u16 = 41;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_BACKSLASH: u16 = 43;
pub const KEY_Z: u16 = 44;
pub const KEY_X: u16 = 45;
pub const KEY_C: u16 = 46;
pub const KEY_V: u16 = 47;
pub const KEY_B: u16 = 48;
pub const KEY_N: u16 = 49;
pub const KEY_M: u16 = 50;
pub const KEY_COMMA: u16 = 51;
pub const KEY_DOT: u16 = 52;
pub const KEY_SLASH: u16 = 53;
pub const KEY_RIGHTSHIFT: u16 = 54;
pub const KEY_KPASTERISK: u16 = 55;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_SPACE: u16 = 57;
pub const KEY_CAPSLOCK: u16 = 58;
pub const KEY_F1: u16 = 59;
pub const KEY_F2: u16 = 60;
pub const KEY_F3: u16 = 61;
pub const KEY_F4: u16 = 62;
pub const KEY_F5: u16 = 63;
pub const KEY_F6: u16 = 64;
pub const KEY_F7: u16 = 65;
pub const KEY_F8: u16 = 66;
pub const KEY_F9: u16 = 67;
pub const KEY_F10: u16 = 68;
pub const KEY_NUMLOCK: u16 = 69;
pub const KEY_SCROLLLOCK: u16 = 70;
pub const KEY_F11: u16 = 87;
pub const KEY_F12: u16 = 88;

// Arrow keys
pub const KEY_UP: u16 = 103;
pub const KEY_LEFT: u16 = 105;
pub const KEY_RIGHT: u16 = 106;
pub const KEY_DOWN: u16 = 108;

// Navigation keys
pub const KEY_HOME: u16 = 102;
pub const KEY_END: u16 = 107;
pub const KEY_PAGEUP: u16 = 104;
pub const KEY_PAGEDOWN: u16 = 109;
pub const KEY_INSERT: u16 = 110;
pub const KEY_DELETE: u16 = 111;

// Mouse buttons (BTN_* range starts at 0x100)
pub const BTN_MISC: u16 = 0x100;
pub const BTN_0: u16 = 0x100;
pub const BTN_1: u16 = 0x101;
pub const BTN_2: u16 = 0x102;
pub const BTN_3: u16 = 0x103;
pub const BTN_4: u16 = 0x104;
pub const BTN_5: u16 = 0x105;
pub const BTN_6: u16 = 0x106;
pub const BTN_7: u16 = 0x107;
pub const BTN_8: u16 = 0x108;
pub const BTN_9: u16 = 0x109;

// Mouse buttons
pub const BTN_MOUSE: u16 = 0x110;
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_SIDE: u16 = 0x113;
pub const BTN_EXTRA: u16 = 0x114;
pub const BTN_FORWARD: u16 = 0x115;
pub const BTN_BACK: u16 = 0x116;
pub const BTN_TASK: u16 = 0x117;

// ============================================================================
// Miscellaneous Codes (MSC_*)
// ============================================================================

pub const MSC_SERIAL: u16 = 0x00;
pub const MSC_PULSELED: u16 = 0x01;
pub const MSC_GESTURE: u16 = 0x02;
pub const MSC_RAW: u16 = 0x03;
pub const MSC_SCAN: u16 = 0x04;
pub const MSC_TIMESTAMP: u16 = 0x05;

// ============================================================================
// LED Codes (LED_*)
// ============================================================================

pub const LED_NUML: u16 = 0x00;
pub const LED_CAPSL: u16 = 0x01;
pub const LED_SCROLLL: u16 = 0x02;
pub const LED_COMPOSE: u16 = 0x03;
pub const LED_KANA: u16 = 0x04;

// ============================================================================
// Input Device ID (struct input_id)
// ============================================================================

/// Input device ID structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct InputId {
    /// Bus type
    pub bustype: u16,
    /// Vendor ID
    pub vendor: u16,
    /// Product ID
    pub product: u16,
    /// Version
    pub version: u16,
}

/// Bus types
pub const BUS_PCI: u16 = 0x01;
pub const BUS_ISAPNP: u16 = 0x02;
pub const BUS_USB: u16 = 0x03;
pub const BUS_HIL: u16 = 0x04;
pub const BUS_BLUETOOTH: u16 = 0x05;
pub const BUS_VIRTUAL: u16 = 0x06;
pub const BUS_ISA: u16 = 0x10;
pub const BUS_I8042: u16 = 0x11;
pub const BUS_XTKBD: u16 = 0x12;
pub const BUS_RS232: u16 = 0x13;
pub const BUS_GAMEPORT: u16 = 0x14;
pub const BUS_PARPORT: u16 = 0x15;
pub const BUS_AMIGA: u16 = 0x16;
pub const BUS_ADB: u16 = 0x17;
pub const BUS_I2C: u16 = 0x18;
pub const BUS_HOST: u16 = 0x19;
pub const BUS_GSC: u16 = 0x1A;
pub const BUS_ATARI: u16 = 0x1B;
pub const BUS_SPI: u16 = 0x1C;

// ============================================================================
// Input Event Type Enum
// ============================================================================

/// Input event type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEventType {
    Sync,
    Key,
    Rel,
    Abs,
    Msc,
    Led,
    Unknown(u16),
}

impl From<u16> for InputEventType {
    fn from(val: u16) -> Self {
        match val {
            EV_SYN => Self::Sync,
            EV_KEY => Self::Key,
            EV_REL => Self::Rel,
            EV_ABS => Self::Abs,
            EV_MSC => Self::Msc,
            EV_LED => Self::Led,
            other => Self::Unknown(other),
        }
    }
}

impl From<InputEventType> for u16 {
    fn from(val: InputEventType) -> Self {
        match val {
            InputEventType::Sync => EV_SYN,
            InputEventType::Key => EV_KEY,
            InputEventType::Rel => EV_REL,
            InputEventType::Abs => EV_ABS,
            InputEventType::Msc => EV_MSC,
            InputEventType::Led => EV_LED,
            InputEventType::Unknown(v) => v,
        }
    }
}

// ============================================================================
// Input Device Info
// ============================================================================

/// Input device information (for EVIOCGNAME, EVIOCGPHYS, etc.)
#[derive(Debug, Clone)]
pub struct InputDeviceInfo {
    /// Device name
    pub name: &'static str,
    /// Physical path (e.g., "isa0060/serio0/input0")
    pub phys: &'static str,
    /// Unique ID string
    pub uniq: &'static str,
    /// Device ID
    pub id: InputId,
    /// Supported event type bits
    pub evbit: u32,
    /// Supported key bits (subset)
    pub keybit: [u64; 12],
    /// Supported relative axis bits
    pub relbit: u32,
    /// Supported absolute axis bits
    pub absbit: u32,
}

impl InputDeviceInfo {
    pub const fn empty() -> Self {
        Self {
            name: "",
            phys: "",
            uniq: "",
            id: InputId {
                bustype: 0,
                vendor: 0,
                product: 0,
                version: 0,
            },
            evbit: 0,
            keybit: [0; 12],
            relbit: 0,
            absbit: 0,
        }
    }
}
