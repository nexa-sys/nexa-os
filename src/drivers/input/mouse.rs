//! Mouse Input Device
//!
//! This module provides the mouse input event interface for /dev/input/event1
//! and /dev/input/mice (combined mouse device).

use super::event::{
    InputEvent, InputId, InputDeviceInfo,
    EV_KEY, EV_REL, EV_SYN, SYN_REPORT,
    REL_X, REL_Y, REL_WHEEL,
    BTN_LEFT, BTN_RIGHT, BTN_MIDDLE,
    BUS_I8042,
};
use core::sync::atomic::{AtomicBool, AtomicI8, Ordering};
use spin::Mutex;

/// Event buffer size
const EVENT_BUFFER_SIZE: usize = 64;

/// Mouse event buffer
struct MouseBuffer {
    events: [InputEvent; EVENT_BUFFER_SIZE],
    head: usize,
    tail: usize,
}

impl MouseBuffer {
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
}

static MOUSE_BUFFER: Mutex<MouseBuffer> = Mutex::new(MouseBuffer::new());

/// Mouse button states
static LEFT_BUTTON: AtomicBool = AtomicBool::new(false);
static RIGHT_BUTTON: AtomicBool = AtomicBool::new(false);
static MIDDLE_BUTTON: AtomicBool = AtomicBool::new(false);

/// Mouse position (for absolute mode, not used in relative mode)
static MOUSE_X: AtomicI8 = AtomicI8::new(0);
static MOUSE_Y: AtomicI8 = AtomicI8::new(0);

/// PS/2 mouse packet state
struct MousePacketState {
    bytes: [u8; 4],
    count: usize,
    has_wheel: bool,
}

impl MousePacketState {
    const fn new() -> Self {
        Self {
            bytes: [0; 4],
            count: 0,
            has_wheel: false,
        }
    }

    fn reset(&mut self) {
        self.count = 0;
    }
}

static MOUSE_PACKET: Mutex<MousePacketState> = Mutex::new(MousePacketState::new());

/// Mouse device instance
pub struct MouseDevice {
    id: InputId,
}

impl MouseDevice {
    pub const fn new() -> Self {
        Self {
            id: InputId {
                bustype: BUS_I8042,
                vendor: 0x0002,
                product: 0x0001,
                version: 0x0000,
            },
        }
    }
}

/// Initialize mouse input device
pub fn init() {
    crate::kdebug!("Mouse input device initialized (event1)");
}

/// Get mouse device ID
pub fn get_id() -> InputId {
    InputId {
        bustype: BUS_I8042,
        vendor: 0x0002,
        product: 0x0001,
        version: 0x0000,
    }
}

/// Get mouse device info
pub fn get_info() -> InputDeviceInfo {
    InputDeviceInfo {
        name: "PS/2 Generic Mouse",
        phys: "isa0060/serio1/input0",
        uniq: "",
        id: get_id(),
        evbit: (1 << super::EV_SYN) | (1 << super::EV_KEY) | (1 << super::EV_REL),
        keybit: [0; 12],
        relbit: (1 << REL_X) | (1 << REL_Y) | (1 << REL_WHEEL),
        absbit: 0,
    }
}

/// Check if there are pending mouse events
pub fn has_events() -> bool {
    !MOUSE_BUFFER.lock().is_empty()
}

/// Read mouse events into buffer
pub fn read_events(buf: &mut [InputEvent]) -> usize {
    let mut buffer = MOUSE_BUFFER.lock();
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

/// Process a byte from the PS/2 mouse
/// Called from the mouse interrupt handler (IRQ 12)
pub fn process_byte(byte: u8) {
    let mut packet = MOUSE_PACKET.lock();

    // Validate first byte of packet
    if packet.count == 0 {
        // Bit 3 should always be 1 in the first byte of a PS/2 mouse packet
        if (byte & 0x08) == 0 {
            // Invalid packet start, resync
            return;
        }
    }

    let idx = packet.count;
    packet.bytes[idx] = byte;
    packet.count += 1;

    // Standard PS/2 mouse sends 3-byte packets
    // Scroll wheel mice send 4-byte packets
    let packet_size = if packet.has_wheel { 4 } else { 3 };

    if packet.count >= packet_size {
        // Complete packet received
        let buttons = packet.bytes[0];
        let dx = packet.bytes[1] as i8;
        let dy = packet.bytes[2] as i8;
        let dz = if packet.has_wheel {
            packet.bytes[3] as i8
        } else {
            0
        };

        // Apply sign extension from status byte
        let dx = if buttons & 0x10 != 0 {
            dx as i32 - 256
        } else {
            dx as i32
        };
        let dy = if buttons & 0x20 != 0 {
            dy as i32 - 256
        } else {
            dy as i32
        };

        // Invert Y axis (PS/2 has inverted Y)
        let dy = -dy;

        packet.reset();
        drop(packet);

        // Generate events
        generate_events(
            buttons & 0x01 != 0, // Left button
            buttons & 0x02 != 0, // Right button
            buttons & 0x04 != 0, // Middle button
            dx,
            dy,
            dz as i32,
        );
    }
}

/// Generate input events from mouse state
fn generate_events(
    left: bool,
    right: bool,
    middle: bool,
    dx: i32,
    dy: i32,
    dz: i32,
) {
    let mut buffer = MOUSE_BUFFER.lock();
    let mut has_events = false;

    // Button state changes
    let prev_left = LEFT_BUTTON.swap(left, Ordering::AcqRel);
    let prev_right = RIGHT_BUTTON.swap(right, Ordering::AcqRel);
    let prev_middle = MIDDLE_BUTTON.swap(middle, Ordering::AcqRel);

    if left != prev_left {
        buffer.push(InputEvent::with_timestamp(
            EV_KEY,
            BTN_LEFT,
            if left { 1 } else { 0 },
        ));
        has_events = true;
    }

    if right != prev_right {
        buffer.push(InputEvent::with_timestamp(
            EV_KEY,
            BTN_RIGHT,
            if right { 1 } else { 0 },
        ));
        has_events = true;
    }

    if middle != prev_middle {
        buffer.push(InputEvent::with_timestamp(
            EV_KEY,
            BTN_MIDDLE,
            if middle { 1 } else { 0 },
        ));
        has_events = true;
    }

    // Relative movement
    if dx != 0 {
        buffer.push(InputEvent::with_timestamp(EV_REL, REL_X, dx));
        has_events = true;
    }

    if dy != 0 {
        buffer.push(InputEvent::with_timestamp(EV_REL, REL_Y, dy));
        has_events = true;
    }

    if dz != 0 {
        buffer.push(InputEvent::with_timestamp(EV_REL, REL_WHEEL, dz));
        has_events = true;
    }

    // Sync event
    if has_events {
        buffer.push(InputEvent::sync());
    }
}

/// Inject a mouse movement event (for virtual mice or USB mice)
pub fn inject_movement(dx: i32, dy: i32) {
    let mut buffer = MOUSE_BUFFER.lock();

    if dx != 0 {
        buffer.push(InputEvent::with_timestamp(EV_REL, REL_X, dx));
    }

    if dy != 0 {
        buffer.push(InputEvent::with_timestamp(EV_REL, REL_Y, dy));
    }

    if dx != 0 || dy != 0 {
        buffer.push(InputEvent::sync());
    }
}

/// Inject a mouse button event (for virtual mice or USB mice)
pub fn inject_button(button: u16, pressed: bool) {
    let mut buffer = MOUSE_BUFFER.lock();

    buffer.push(InputEvent::with_timestamp(
        EV_KEY,
        button,
        if pressed { 1 } else { 0 },
    ));
    buffer.push(InputEvent::sync());
}

/// Inject a scroll wheel event
pub fn inject_scroll(delta: i32) {
    let mut buffer = MOUSE_BUFFER.lock();

    if delta != 0 {
        buffer.push(InputEvent::with_timestamp(EV_REL, REL_WHEEL, delta));
        buffer.push(InputEvent::sync());
    }
}

/// Enable scroll wheel support (call after detecting IntelliMouse)
pub fn enable_wheel() {
    MOUSE_PACKET.lock().has_wheel = true;
    crate::kdebug!("Mouse scroll wheel enabled");
}

/// Get current button states
pub fn get_button_states() -> (bool, bool, bool) {
    (
        LEFT_BUTTON.load(Ordering::Acquire),
        RIGHT_BUTTON.load(Ordering::Acquire),
        MIDDLE_BUTTON.load(Ordering::Acquire),
    )
}

/// Read raw mouse data in ImPS/2 format (for /dev/input/mice)
/// Returns: (buttons, dx, dy, dz)
pub fn read_imps2(buf: &mut [u8]) -> usize {
    if buf.len() < 4 {
        return 0;
    }

    // Read events and convert to ImPS/2 format
    let mut buffer = MOUSE_BUFFER.lock();

    let mut dx: i32 = 0;
    let mut dy: i32 = 0;
    let mut dz: i32 = 0;
    let mut buttons: u8 = 0;
    let mut has_data = false;

    // Drain events and accumulate movement
    while let Some(event) = buffer.pop() {
        match event.event_type {
            EV_REL => {
                match event.code {
                    REL_X => dx += event.value,
                    REL_Y => dy += event.value,
                    REL_WHEEL => dz += event.value,
                    _ => {}
                }
                has_data = true;
            }
            EV_KEY => {
                match event.code {
                    BTN_LEFT => {
                        if event.value != 0 {
                            buttons |= 0x01;
                        }
                    }
                    BTN_RIGHT => {
                        if event.value != 0 {
                            buttons |= 0x02;
                        }
                    }
                    BTN_MIDDLE => {
                        if event.value != 0 {
                            buttons |= 0x04;
                        }
                    }
                    _ => {}
                }
                has_data = true;
            }
            EV_SYN => {
                // End of event group
                break;
            }
            _ => {}
        }
    }

    if !has_data {
        return 0;
    }

    // Clamp values to signed byte range
    let dx = dx.clamp(-127, 127) as i8;
    let dy = dy.clamp(-127, 127) as i8;
    let dz = dz.clamp(-7, 7) as i8;

    // ImPS/2 format (4 bytes)
    buf[0] = buttons | 0x08; // Bit 3 always set
    buf[1] = dx as u8;
    buf[2] = dy as u8;
    buf[3] = dz as u8;

    4
}
