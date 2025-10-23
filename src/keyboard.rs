/// PS/2 Keyboard driver
use spin::Mutex;

static SCANCODE_QUEUE: Mutex<[u8; 128]> = Mutex::new([0; 128]);
static QUEUE_HEAD: Mutex<usize> = Mutex::new(0);
static QUEUE_TAIL: Mutex<usize> = Mutex::new(0);

/// Add scancode to queue (called from interrupt handler)
pub fn add_scancode(scancode: u8) {
    let mut head = QUEUE_HEAD.lock();
    let tail = *QUEUE_TAIL.lock();
    
    let next_head = (*head + 1) % 128;
    if next_head != tail {
        let mut queue = SCANCODE_QUEUE.lock();
        queue[*head] = scancode;
        *head = next_head;
    }
}

/// Get next scancode from queue
fn get_scancode() -> Option<u8> {
    let mut tail = QUEUE_TAIL.lock();
    let head = *QUEUE_HEAD.lock();
    
    if *tail == head {
        None
    } else {
        let queue = SCANCODE_QUEUE.lock();
        let scancode = queue[*tail];
        *tail = (*tail + 1) % 128;
        Some(scancode)
    }
}

/// US QWERTY keyboard layout
const SCANCODE_TO_CHAR: [char; 128] = [
    '\0', '\x1B', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '\x08', '\t',
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']', '\n', '\0', 'a', 's',
    'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'', '`', '\0', '\\', 'z', 'x', 'c', 'v',
    'b', 'n', 'm', ',', '.', '/', '\0', '*', '\0', ' ', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '7', '8', '9', '-', '4', '5', '6', '+', '1',
    '2', '3', '0', '.', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
];

const SCANCODE_TO_CHAR_SHIFT: [char; 128] = [
    '\0', '\x1B', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '_', '+', '\x08', '\t',
    'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', '{', '}', '\n', '\0', 'A', 'S',
    'D', 'F', 'G', 'H', 'J', 'K', 'L', ':', '"', '~', '\0', '|', 'Z', 'X', 'C', 'V',
    'B', 'N', 'M', '<', '>', '?', '\0', '*', '\0', ' ', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '7', '8', '9', '-', '4', '5', '6', '+', '1',
    '2', '3', '0', '.', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
    '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0',
];

static SHIFT_PRESSED: Mutex<bool> = Mutex::new(false);

/// Read a character from keyboard (blocking)
pub fn read_char() -> Option<char> {
    loop {
        if let Some(scancode) = get_scancode() {
            // Handle key release
            if scancode & 0x80 != 0 {
                let key = scancode & 0x7F;
                if key == 0x2A || key == 0x36 {
                    *SHIFT_PRESSED.lock() = false;
                }
                continue;
            }
            
            // Handle shift keys
            if scancode == 0x2A || scancode == 0x36 {
                *SHIFT_PRESSED.lock() = true;
                continue;
            }
            
            // Get character
            let shift = *SHIFT_PRESSED.lock();
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

/// Read a line from keyboard
pub fn read_line(buf: &mut [u8]) -> usize {
    let mut pos = 0;
    
    loop {
        if let Some(ch) = read_char() {
            match ch {
                '\n' => {
                    crate::print!("\n");
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
