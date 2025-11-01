/// PS/2 Keyboard driver
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

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

/// Add scancode to queue (called from interrupt handler)
pub fn add_scancode(scancode: u8) {
    let mut queue = SCANCODE_QUEUE.lock();
    queue.push(scancode);
}

/// Get next scancode from queue
fn get_scancode() -> Option<u8> {
    let mut queue = SCANCODE_QUEUE.lock();
    queue.pop()
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

/// Read a character from keyboard (blocking)
pub fn read_char() -> Option<char> {
    loop {
        if let Some(scancode) = get_scancode() {
            // Handle key release
            if scancode & 0x80 != 0 {
                let key = scancode & 0x7F;
                if key == 0x2A || key == 0x36 {
                    SHIFT_PRESSED.store(false, Ordering::Release);
                }
                continue;
            }

            // Handle shift keys
            if scancode == 0x2A || scancode == 0x36 {
                SHIFT_PRESSED.store(true, Ordering::Release);
                continue;
            }

            // Get character
            let shift = SHIFT_PRESSED.load(Ordering::Acquire);
            let ch = if shift {
                SCANCODE_TO_CHAR_SHIFT[scancode as usize]
            } else {
                SCANCODE_TO_CHAR[scancode as usize]
            };

            if ch != '\0' {
                return Some(ch);
            }
        } else {
            // Wait for interrupt
            x86_64::instructions::hlt();
        }
    }
}

/// Try to read a character from keyboard (non-blocking)
pub fn try_read_char() -> Option<char> {
    if let Some(scancode) = get_scancode() {
        // Handle key release
        if scancode & 0x80 != 0 {
            let key = scancode & 0x7F;
            if key == 0x2A || key == 0x36 {
                SHIFT_PRESSED.store(false, Ordering::Release);
            }
            return None;
        }

        // Handle shift keys
        if scancode == 0x2A || scancode == 0x36 {
            SHIFT_PRESSED.store(true, Ordering::Release);
            return None;
        }

        // Get character
        let shift = SHIFT_PRESSED.load(Ordering::Acquire);
        let ch = if shift {
            SCANCODE_TO_CHAR_SHIFT[scancode as usize]
        } else {
            SCANCODE_TO_CHAR[scancode as usize]
        };

        if ch != '\0' {
            Some(ch)
        } else {
            None
        }
    } else {
        None
    }
}

/// Read a line from keyboard
pub fn read_line(buf: &mut [u8]) -> usize {
    let mut pos = 0;

    loop {
        if let Some(ch) = read_char() {
            match ch {
                '\n' => {
                    crate::kprint!("\n");
                    return pos;
                }
                '\x08' => {
                    // Backspace
                    if pos > 0 {
                        pos -= 1;
                        crate::print!("\x08 \x08");
                    }
                }
                _ => {
                    if pos < buf.len() {
                        buf[pos] = ch as u8;
                        pos += 1;
                        crate::print!("{}", ch);
                    }
                }
            }
        }
    }
}
