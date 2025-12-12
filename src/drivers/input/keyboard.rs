//! Keyboard Input Device
//!
//! This module provides the keyboard input event interface for /dev/input/event0.
//! It translates PS/2 scancodes to Linux input events.

use super::event::{
    InputEvent, InputId, InputDeviceInfo,
    EV_KEY, EV_MSC, EV_SYN, SYN_REPORT, MSC_SCAN,
    KEY_ESC, KEY_1, KEY_2, KEY_3, KEY_4, KEY_5, KEY_6, KEY_7, KEY_8, KEY_9, KEY_0,
    KEY_MINUS, KEY_EQUAL, KEY_BACKSPACE, KEY_TAB, KEY_Q, KEY_W, KEY_E, KEY_R, KEY_T,
    KEY_Y, KEY_U, KEY_I, KEY_O, KEY_P, KEY_LEFTBRACE, KEY_RIGHTBRACE, KEY_ENTER,
    KEY_LEFTCTRL, KEY_A, KEY_S, KEY_D, KEY_F, KEY_G, KEY_H, KEY_J, KEY_K, KEY_L,
    KEY_SEMICOLON, KEY_APOSTROPHE, KEY_GRAVE, KEY_LEFTSHIFT, KEY_BACKSLASH,
    KEY_Z, KEY_X, KEY_C, KEY_V, KEY_B, KEY_N, KEY_M, KEY_COMMA, KEY_DOT, KEY_SLASH,
    KEY_RIGHTSHIFT, KEY_KPASTERISK, KEY_LEFTALT, KEY_SPACE, KEY_CAPSLOCK,
    KEY_F1, KEY_F2, KEY_F3, KEY_F4, KEY_F5, KEY_F6, KEY_F7, KEY_F8, KEY_F9, KEY_F10,
    KEY_NUMLOCK, KEY_SCROLLLOCK, KEY_F11, KEY_F12,
    KEY_UP, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_HOME, KEY_END, KEY_PAGEUP, KEY_PAGEDOWN,
    KEY_INSERT, KEY_DELETE,
    BUS_I8042,
};
use spin::Mutex;

/// Event buffer size
const EVENT_BUFFER_SIZE: usize = 64;

/// Keyboard event buffer
struct KeyboardBuffer {
    events: [InputEvent; EVENT_BUFFER_SIZE],
    head: usize,
    tail: usize,
}

impl KeyboardBuffer {
    const fn new() -> Self {
        Self {
            events: [InputEvent::new(0, 0, 0); EVENT_BUFFER_SIZE],
            head: 0,
            tail: 0,
        }
    }

    fn push(&mut self, event: InputEvent) {
        let next = (self.head + 1) % EVENT_BUFFER_SIZE;
        if next != self.tail {
            self.events[self.head] = event;
            self.head = next;
        }
    }

    fn pop(&mut self) -> Option<InputEvent> {
        if self.head == self.tail {
            None
        } else {
            let event = self.events[self.tail];
            self.tail = (self.tail + 1) % EVENT_BUFFER_SIZE;
            Some(event)
        }
    }

    fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    fn len(&self) -> usize {
        if self.head >= self.tail {
            self.head - self.tail
        } else {
            EVENT_BUFFER_SIZE - self.tail + self.head
        }
    }
}

static KEYBOARD_BUFFER: Mutex<KeyboardBuffer> = Mutex::new(KeyboardBuffer::new());
static EXTENDED_MODE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Keyboard device instance
pub struct KeyboardDevice {
    id: InputId,
}

impl KeyboardDevice {
    pub const fn new() -> Self {
        Self {
            id: InputId {
                bustype: BUS_I8042,
                vendor: 0x0001,
                product: 0x0001,
                version: 0xAB41,
            },
        }
    }
}

/// Initialize keyboard input device
pub fn init() {
    crate::kdebug!("Keyboard input device initialized (event0)");
}

/// Get keyboard device ID
pub fn get_id() -> InputId {
    InputId {
        bustype: BUS_I8042,
        vendor: 0x0001,
        product: 0x0001,
        version: 0xAB41,
    }
}

/// Get keyboard device info
pub fn get_info() -> InputDeviceInfo {
    InputDeviceInfo {
        name: "AT Translated Set 2 keyboard",
        phys: "isa0060/serio0/input0",
        uniq: "",
        id: get_id(),
        evbit: (1 << super::EV_SYN) | (1 << super::EV_KEY) | (1 << super::EV_MSC),
        keybit: [0xFFFFFFFF; 12], // Support all key codes
        relbit: 0,
        absbit: 0,
    }
}

/// Check if there are pending keyboard events
pub fn has_events() -> bool {
    !KEYBOARD_BUFFER.lock().is_empty()
}

/// Read keyboard events into buffer
pub fn read_events(buf: &mut [InputEvent]) -> usize {
    let mut buffer = KEYBOARD_BUFFER.lock();
    let mut count = 0;

    while count < buf.len() {
        if let Some(event) = buffer.pop() {
            buf[count] = event;
            count += 1;
        } else {
            break;
        }
    }

    count
}

/// Scancode to Linux key code mapping
/// This maps PS/2 Set 2 scancodes to Linux KEY_* codes
const SCANCODE_TO_KEYCODE: [u16; 128] = [
    0,            // 0x00
    KEY_ESC,      // 0x01
    KEY_1,        // 0x02
    KEY_2,        // 0x03
    KEY_3,        // 0x04
    KEY_4,        // 0x05
    KEY_5,        // 0x06
    KEY_6,        // 0x07
    KEY_7,        // 0x08
    KEY_8,        // 0x09
    KEY_9,        // 0x0A
    KEY_0,        // 0x0B
    KEY_MINUS,    // 0x0C
    KEY_EQUAL,    // 0x0D
    KEY_BACKSPACE,// 0x0E
    KEY_TAB,      // 0x0F
    KEY_Q,        // 0x10
    KEY_W,        // 0x11
    KEY_E,        // 0x12
    KEY_R,        // 0x13
    KEY_T,        // 0x14
    KEY_Y,        // 0x15
    KEY_U,        // 0x16
    KEY_I,        // 0x17
    KEY_O,        // 0x18
    KEY_P,        // 0x19
    KEY_LEFTBRACE,// 0x1A
    KEY_RIGHTBRACE,// 0x1B
    KEY_ENTER,    // 0x1C
    KEY_LEFTCTRL, // 0x1D
    KEY_A,        // 0x1E
    KEY_S,        // 0x1F
    KEY_D,        // 0x20
    KEY_F,        // 0x21
    KEY_G,        // 0x22
    KEY_H,        // 0x23
    KEY_J,        // 0x24
    KEY_K,        // 0x25
    KEY_L,        // 0x26
    KEY_SEMICOLON,// 0x27
    KEY_APOSTROPHE,// 0x28
    KEY_GRAVE,    // 0x29
    KEY_LEFTSHIFT,// 0x2A
    KEY_BACKSLASH,// 0x2B
    KEY_Z,        // 0x2C
    KEY_X,        // 0x2D
    KEY_C,        // 0x2E
    KEY_V,        // 0x2F
    KEY_B,        // 0x30
    KEY_N,        // 0x31
    KEY_M,        // 0x32
    KEY_COMMA,    // 0x33
    KEY_DOT,      // 0x34
    KEY_SLASH,    // 0x35
    KEY_RIGHTSHIFT,// 0x36
    KEY_KPASTERISK,// 0x37
    KEY_LEFTALT,  // 0x38
    KEY_SPACE,    // 0x39
    KEY_CAPSLOCK, // 0x3A
    KEY_F1,       // 0x3B
    KEY_F2,       // 0x3C
    KEY_F3,       // 0x3D
    KEY_F4,       // 0x3E
    KEY_F5,       // 0x3F
    KEY_F6,       // 0x40
    KEY_F7,       // 0x41
    KEY_F8,       // 0x42
    KEY_F9,       // 0x43
    KEY_F10,      // 0x44
    KEY_NUMLOCK,  // 0x45
    KEY_SCROLLLOCK,// 0x46
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x47-0x56
    KEY_F11,      // 0x57
    KEY_F12,      // 0x58
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x59-0x68
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x69-0x78
    0, 0, 0, 0, 0, 0, 0,                             // 0x79-0x7F
];

/// Extended scancode to keycode mapping (0xE0 prefix)
fn extended_scancode_to_keycode(scancode: u8) -> u16 {
    match scancode {
        0x48 => KEY_UP,
        0x50 => KEY_DOWN,
        0x4B => KEY_LEFT,
        0x4D => KEY_RIGHT,
        0x47 => KEY_HOME,
        0x4F => KEY_END,
        0x49 => KEY_PAGEUP,
        0x51 => KEY_PAGEDOWN,
        0x52 => KEY_INSERT,
        0x53 => KEY_DELETE,
        _ => 0,
    }
}

/// Process a scancode and generate input events
/// Called from the keyboard interrupt handler
pub fn process_scancode(scancode: u8) {
    use core::sync::atomic::Ordering;

    // Handle extended prefix
    if scancode == 0xE0 {
        EXTENDED_MODE.store(true, Ordering::Release);
        return;
    }

    let extended = EXTENDED_MODE.swap(false, Ordering::AcqRel);
    let released = (scancode & 0x80) != 0;
    let code = scancode & 0x7F;

    // Get key code
    let keycode = if extended {
        extended_scancode_to_keycode(code)
    } else if (code as usize) < SCANCODE_TO_KEYCODE.len() {
        SCANCODE_TO_KEYCODE[code as usize]
    } else {
        0
    };

    if keycode == 0 {
        return; // Unknown key
    }

    let value = if released { 0 } else { 1 };

    let mut buffer = KEYBOARD_BUFFER.lock();

    // Generate MSC_SCAN event (raw scancode)
    let msc_event = InputEvent::with_timestamp(EV_MSC, MSC_SCAN, code as i32);
    buffer.push(msc_event);

    // Generate KEY event
    let key_event = InputEvent::with_timestamp(EV_KEY, keycode, value);
    buffer.push(key_event);

    // Generate SYN_REPORT
    let syn_event = InputEvent::sync();
    buffer.push(syn_event);
}

/// Inject a key event directly (for virtual keyboards)
pub fn inject_key(keycode: u16, pressed: bool) {
    let mut buffer = KEYBOARD_BUFFER.lock();

    let key_event = InputEvent::with_timestamp(EV_KEY, keycode, if pressed { 1 } else { 0 });
    buffer.push(key_event);

    let syn_event = InputEvent::sync();
    buffer.push(syn_event);
}
