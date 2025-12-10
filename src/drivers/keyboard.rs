/// PS/2 Keyboard driver
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::vt;

const QUEUE_CAPACITY: usize = 128;

struct KeyboardBuffer {
    data: [u8; QUEUE_CAPACITY],
    head: usize,
    tail: usize,
}

impl KeyboardBuffer {
    const fn new() -> Self {
        Self {
            data: [0; QUEUE_CAPACITY],
            head: 0,
            tail: 0,
        }
    }

    fn push(&mut self, scancode: u8) {
        let next_head = (self.head + 1) % QUEUE_CAPACITY;
        if next_head != self.tail {
            self.data[self.head] = scancode;
            self.head = next_head;
        } else {
            // Buffer full; drop the oldest scancode to keep the latest input responsive.
            self.tail = (self.tail + 1) % QUEUE_CAPACITY;
            self.data[self.head] = scancode;
            self.head = next_head;
        }
    }

    fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            None
        } else {
            let scancode = self.data[self.tail];
            self.tail = (self.tail + 1) % QUEUE_CAPACITY;
            Some(scancode)
        }
    }
}

static SCANCODE_QUEUE: Mutex<KeyboardBuffer> = Mutex::new(KeyboardBuffer::new());
static SHIFT_PRESSED: AtomicBool = AtomicBool::new(false);
static LAST_BYTE_WAS_CR: AtomicBool = AtomicBool::new(false);
static ALT_PRESSED: AtomicBool = AtomicBool::new(false);
static EXTENDED_MODE: AtomicBool = AtomicBool::new(false);

struct PendingBuffer {
    chars: [char; 8],
    count: usize,
    index: usize,
}

impl PendingBuffer {
    const fn new() -> Self {
        Self {
            chars: ['\0'; 8],
            count: 0,
            index: 0,
        }
    }

    fn push(&mut self, c: char) {
        if self.count < 8 {
            self.chars[self.count] = c;
            self.count += 1;
        }
    }

    fn pop(&mut self) -> Option<char> {
        if self.index < self.count {
            let c = self.chars[self.index];
            self.index += 1;
            if self.index == self.count {
                self.count = 0;
                self.index = 0;
            }
            Some(c)
        } else {
            None
        }
    }
}

static PENDING_KEYS: Mutex<PendingBuffer> = Mutex::new(PendingBuffer::new());

/// Add scancode to queue (called from interrupt handler)
pub fn add_scancode(scancode: u8) {
    interrupts::without_interrupts(|| {
        let mut queue = SCANCODE_QUEUE.lock();
        queue.push(scancode);
    });
}

/// Get next scancode from queue
fn get_scancode() -> Option<u8> {
    interrupts::without_interrupts(|| {
        let mut queue = SCANCODE_QUEUE.lock();
        queue.pop()
    })
}

/// US QWERTY keyboard layout
const SCANCODE_TO_CHAR: [char; 127] = [
    '\0', '\x1B', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '\x08', '\t', 'q',
    'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']', '\n', '\0', 'a', 's', 'd', 'f', 'g',
    'h', 'j', 'k', 'l', ';', '\'', '`', '\0', '\\', 'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.',
    '/', '\0', '*', '\0', ' ', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '7', '8', '9', '-', '4', '5', '6', '+', '1', '2', '3', '0', '.', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
];

const SCANCODE_TO_CHAR_SHIFT: [char; 128] = [
    '\0', '\x1B', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '_', '+', '\x08', '\t', 'Q',
    'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', '{', '}', '\n', '\0', 'A', 'S', 'D', 'F', 'G',
    'H', 'J', 'K', 'L', ':', '"', '~', '\0', '|', 'Z', 'X', 'C', 'V', 'B', 'N', 'M', '<', '>', '?',
    '\0', '*', '\0', ' ', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '7', '8', '9', '-', '4', '5', '6', '+', '1', '2', '3', '0', '.', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
];

fn echo_char(tty: usize, byte: u8) {
    crate::serial::write_bytes(&[byte]);
    vt::echo_input_byte(tty, byte);
}

fn echo_newline(tty: usize) {
    crate::serial::write_bytes(b"\r\n");
    vt::echo_input_newline(tty);
}

fn echo_backspace(tty: usize) {
    crate::serial::write_bytes(b"\x08 \x08");
    vt::echo_input_backspace(tty);
}

fn decode_scancode(scancode: u8) -> Option<char> {
    // Handle extended prefix
    if scancode == 0xE0 {
        EXTENDED_MODE.store(true, Ordering::Release);
        return None;
    }

    let extended = EXTENDED_MODE.swap(false, Ordering::AcqRel);

    // Handle key release
    if scancode & 0x80 != 0 {
        let key = scancode & 0x7F;
        if key == 0x2A || key == 0x36 {
            SHIFT_PRESSED.store(false, Ordering::Release);
        }
        if key == 0x38 {
            ALT_PRESSED.store(false, Ordering::Release);
        }
        return None;
    }

    // Handle extended keys (arrows, etc.)
    if extended {
        match scancode {
            0x48 => {
                // Up
                let mut pending = PENDING_KEYS.lock();
                pending.push('[');
                pending.push('A');
                return Some('\x1B');
            }
            0x4B => {
                // Left
                let mut pending = PENDING_KEYS.lock();
                pending.push('[');
                pending.push('D');
                return Some('\x1B');
            }
            0x4D => {
                // Right
                let mut pending = PENDING_KEYS.lock();
                pending.push('[');
                pending.push('C');
                return Some('\x1B');
            }
            0x50 => {
                // Down
                let mut pending = PENDING_KEYS.lock();
                pending.push('[');
                pending.push('B');
                return Some('\x1B');
            }
            0x53 => {
                // Delete
                let mut pending = PENDING_KEYS.lock();
                pending.push('[');
                pending.push('3');
                pending.push('~');
                return Some('\x1B');
            }
            0x47 => {
                // Home
                let mut pending = PENDING_KEYS.lock();
                pending.push('[');
                pending.push('H');
                return Some('\x1B');
            }
            0x4F => {
                // End
                let mut pending = PENDING_KEYS.lock();
                pending.push('[');
                pending.push('F');
                return Some('\x1B');
            }
            _ => return None,
        }
    }

    // Handle shift keys
    if scancode == 0x2A || scancode == 0x36 {
        SHIFT_PRESSED.store(true, Ordering::Release);
        return None;
    }

    if scancode == 0x38 {
        ALT_PRESSED.store(true, Ordering::Release);
        return None;
    }

    if handle_vt_switch(scancode) {
        return None;
    }

    let shift = SHIFT_PRESSED.load(Ordering::Acquire);
    let ch = if shift {
        SCANCODE_TO_CHAR_SHIFT[scancode as usize]
    } else {
        SCANCODE_TO_CHAR[scancode as usize]
    };

    if ch == '\r' {
        LAST_BYTE_WAS_CR.store(true, Ordering::Release);
        Some('\n')
    } else if ch != '\0' {
        LAST_BYTE_WAS_CR.store(false, Ordering::Release);
        Some(ch)
    } else {
        None
    }
}

fn handle_vt_switch(scancode: u8) -> bool {
    if !ALT_PRESSED.load(Ordering::Acquire) {
        return false;
    }

    let target = match scancode {
        0x3B => Some(0), // F1
        0x3C => Some(1), // F2
        0x3D => Some(2), // F3
        0x3E => Some(3), // F4
        0x3F => Some(4), // F5
        0x40 => Some(5), // F6
        _ => None,
    };

    if let Some(idx) = target {
        if idx < vt::terminal_count() {
            vt::switch_to(idx);
        }
        return true;
    }

    false
}

fn decode_serial_byte(byte: u8) -> Option<char> {
    match byte {
        b'\r' => {
            LAST_BYTE_WAS_CR.store(true, Ordering::Release);
            Some('\n')
        }
        b'\n' => {
            if LAST_BYTE_WAS_CR.swap(false, Ordering::AcqRel) {
                None
            } else {
                Some('\n')
            }
        }
        8 | 0x7F => {
            LAST_BYTE_WAS_CR.store(false, Ordering::Release);
            Some('\x08')
        }
        0 => None,
        byte if byte.is_ascii() => {
            LAST_BYTE_WAS_CR.store(false, Ordering::Release);
            Some(byte as char)
        }
        _ => {
            LAST_BYTE_WAS_CR.store(false, Ordering::Release);
            None
        }
    }
}

fn poll_input_char() -> Option<char> {
    // Check pending first
    {
        let mut pending = PENDING_KEYS.lock();
        if let Some(c) = pending.pop() {
            return Some(c);
        }
    }

    if let Some(scancode) = get_scancode() {
        if let Some(ch) = decode_scancode(scancode) {
            return Some(ch);
        }
    }

    if let Some(byte) = crate::serial::try_read_byte() {
        if let Some(ch) = decode_serial_byte(byte) {
            return Some(ch);
        }
    }

    None
}

/// Read a character from keyboard (blocking)
pub fn read_char() -> Option<char> {
    loop {
        if let Some(ch) = poll_input_char() {
            return Some(ch);
        }
        // Wait for interrupt (keyboard IRQ or serial receive interrupt)
        x86_64::instructions::hlt();
    }
}

/// Try to read a character from keyboard (non-blocking)
pub fn try_read_char() -> Option<char> {
    poll_input_char()
}

/// Read a line from keyboard (with echo)
pub fn read_line(buf: &mut [u8]) -> usize {
    read_line_for_tty(vt::active_terminal(), buf)
}

fn read_line_for_tty(tty: usize, buf: &mut [u8]) -> usize {
    LAST_BYTE_WAS_CR.store(false, Ordering::Release);
    let mut pos = 0;

    loop {
        if tty != vt::active_terminal() {
            x86_64::instructions::hlt();
            continue;
        }

        if let Some(ch) = try_read_char() {
            match ch {
                '\n' => {
                    echo_newline(tty);
                    return pos;
                }
                '\x08' => {
                    if pos > 0 {
                        pos -= 1;
                        echo_backspace(tty);
                    }
                }
                _ => {
                    if pos < buf.len() {
                        buf[pos] = ch as u8;
                        pos += 1;
                        echo_char(tty, ch as u8);
                    }
                }
            }
        } else {
            x86_64::instructions::hlt();
        }
    }
}

/// Read raw bytes from keyboard (no echo, for userspace control)
/// Returns as soon as at least 1 byte is available (non-line-buffered mode)
pub fn read_raw(buf: &mut [u8], count: usize) -> usize {
    read_raw_for_tty(vt::active_terminal(), buf, count)
}

pub fn read_raw_for_tty(tty: usize, buf: &mut [u8], count: usize) -> usize {
    let mut pos = 0;
    let max_read = core::cmp::min(buf.len(), count);

    if max_read == 0 {
        return 0;
    }

    // Wait for at least one character, then return immediately
    // This enables raw/character-by-character input mode
    while pos < max_read {
        if tty != vt::active_terminal() {
            x86_64::instructions::hlt();
            continue;
        }

        if let Some(ch) = try_read_char() {
            buf[pos] = ch as u8;
            pos += 1;
            // Return immediately after getting at least one character
            // This allows single-character reads for shell line editing
            break;
        } else {
            x86_64::instructions::hlt();
        }
    }

    pos
}
