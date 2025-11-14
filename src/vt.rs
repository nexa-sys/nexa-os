use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use spin::Mutex;

use crate::framebuffer;
use crate::vga_buffer::{self, Color};

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;
const MAX_TERMINALS: usize = 6;
const VGA_BUFFER_ADDR: usize = 0xb8000;

#[derive(Clone, Copy)]
struct Cell {
    ascii: u8,
    color: u8,
}

impl Cell {
    const fn blank() -> Self {
        Self {
            ascii: b' ',
            color: color_code(Color::LightGray, Color::Black),
        }
    }
}

#[derive(Clone, Copy)]
struct VirtualTerminal {
    cells: [[Cell; BUFFER_WIDTH]; BUFFER_HEIGHT],
    cursor_col: usize,
    color: u8,
}

impl VirtualTerminal {
    const fn new() -> Self {
        Self {
            cells: [[Cell::blank(); BUFFER_WIDTH]; BUFFER_HEIGHT],
            cursor_col: 0,
            color: color_code(Color::LightGreen, Color::Black),
        }
    }

    fn reset(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                self.cells[row][col] = Cell::blank();
            }
        }
        self.cursor_col = 0;
        self.color = color_code(Color::LightGreen, Color::Black);
    }

    fn write_bytes(&mut self, bytes: &[u8], color: u8, active: bool) {
        let previous = self.color;
        self.color = color;
        for &byte in bytes {
            self.write_byte(byte, active);
        }
        self.color = previous;
    }

    fn write_byte(&mut self, byte: u8, active: bool) {
        match byte {
            b'\n' => self.new_line(active),
            b'\r' => self.cursor_col = 0,
            b'\t' => self.write_tab(active),
            0x08 => self.backspace(active),
            0x20..=0x7e => self.write_visible(byte, active),
            _ => self.write_visible(0xfe, active),
        }
    }

    fn write_visible(&mut self, byte: u8, active: bool) {
        if self.cursor_col >= BUFFER_WIDTH {
            self.new_line(active);
        }

        let row = BUFFER_HEIGHT - 1;
        let col = self.cursor_col;
        self.cells[row][col] = Cell {
            ascii: byte,
            color: self.color,
        };

        if active {
            write_hw_cell(row, col, self.cells[row][col]);
        }

        self.cursor_col += 1;
    }

    fn write_tab(&mut self, active: bool) {
        let spaces = 4 - (self.cursor_col % 4);
        for _ in 0..spaces {
            self.write_visible(b' ', active);
        }
    }

    fn new_line(&mut self, active: bool) {
        for row in 1..BUFFER_HEIGHT {
            self.cells[row - 1] = self.cells[row];
        }
        self.clear_row(BUFFER_HEIGHT - 1, active);
        self.cursor_col = 0;
    }

    fn backspace(&mut self, active: bool) {
        if self.cursor_col == 0 {
            return;
        }
        self.cursor_col -= 1;
        let row = BUFFER_HEIGHT - 1;
        let col = self.cursor_col;
        self.cells[row][col] = Cell::blank();
        if active {
            write_hw_cell(row, col, self.cells[row][col]);
        }
    }

    fn clear_row(&mut self, row: usize, active: bool) {
        for col in 0..BUFFER_WIDTH {
            self.cells[row][col] = Cell::blank();
            if active {
                write_hw_cell(row, col, self.cells[row][col]);
            }
        }
    }

    fn render(&self) {
        if !vga_buffer::is_vga_ready() {
            return;
        }
        for row in 0..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                write_hw_cell(row, col, self.cells[row][col]);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum StreamKind {
    Stdout,
    Stderr,
    Input,
}

struct Manager {
    terminals: [VirtualTerminal; MAX_TERMINALS],
    count: usize,
}

impl Manager {
    const fn new() -> Self {
        Self {
            terminals: [VirtualTerminal::new(); MAX_TERMINALS],
            count: 1,
        }
    }
}

static MANAGER: Mutex<Manager> = Mutex::new(Manager::new());
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static ACTIVE_TERMINAL: AtomicUsize = AtomicUsize::new(0);
static TERMINAL_COUNT: AtomicUsize = AtomicUsize::new(1);

pub fn init(requested: usize) {
    let mut manager = MANAGER.lock();
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    let count = requested.clamp(1, MAX_TERMINALS);
    manager.count = count;
    for term in manager.terminals.iter_mut().take(count) {
        term.reset();
    }

    TERMINAL_COUNT.store(count, Ordering::SeqCst);
    ACTIVE_TERMINAL.store(0, Ordering::SeqCst);

    manager.terminals[0].render();
}

#[inline]
pub fn terminal_count() -> usize {
    TERMINAL_COUNT.load(Ordering::Relaxed)
}

#[inline]
pub fn active_terminal() -> usize {
    ACTIVE_TERMINAL.load(Ordering::Acquire)
}

pub fn switch_to(target: usize) {
    if target >= terminal_count() {
        return;
    }

    let current = active_terminal();
    if current == target {
        return;
    }

    let manager = MANAGER.lock();
    ACTIVE_TERMINAL.store(target, Ordering::Release);
    manager.terminals[target].render();
}

pub fn write_bytes(tty: usize, bytes: &[u8], stream: StreamKind) {
    if !INITIALIZED.load(Ordering::Acquire) {
        legacy_write(bytes);
        return;
    }

    let target = normalize_tty(tty);
    let active = active_terminal();
    let color = color_for_stream(stream);

    let mut manager = MANAGER.lock();
    manager.terminals[target].write_bytes(bytes, color, target == active);

    if target == active {
        forward_to_framebuffer(bytes);
    }
}

pub fn echo_input_byte(tty: usize, byte: u8) {
    write_bytes(tty, core::slice::from_ref(&byte), StreamKind::Input);
}

pub fn echo_input_newline(tty: usize) {
    write_bytes(tty, b"\r\n", StreamKind::Input);
}

pub fn echo_input_backspace(tty: usize) {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let target = normalize_tty(tty);
    let active = active_terminal();
    let mut manager = MANAGER.lock();
    manager.terminals[target].backspace(target == active);
}

fn normalize_tty(tty: usize) -> usize {
    let count = terminal_count();
    if count == 0 {
        0
    } else if tty >= count {
        count - 1
    } else {
        tty
    }
}

fn color_for_stream(stream: StreamKind) -> u8 {
    match stream {
        StreamKind::Stdout => color_code(Color::LightGreen, Color::Black),
        StreamKind::Stderr => color_code(Color::LightRed, Color::Black),
        StreamKind::Input => color_code(Color::LightGray, Color::Black),
    }
}

const fn color_code(foreground: Color, background: Color) -> u8 {
    ((background as u8) << 4) | (foreground as u8)
}

fn write_hw_cell(row: usize, col: usize, cell: Cell) {
    if !vga_buffer::is_vga_ready() {
        return;
    }

    unsafe {
        let ptr = VGA_BUFFER_ADDR as *mut u16;
        let index = row * BUFFER_WIDTH + col;
        let value = ((cell.color as u16) << 8) | cell.ascii as u16;
        ptr.add(index).write_volatile(value);
    }
}

fn forward_to_framebuffer(bytes: &[u8]) {
    if let Ok(text) = core::str::from_utf8(bytes) {
        framebuffer::write_str(text);
    } else {
        framebuffer::write_bytes(bytes);
    }
}

fn legacy_write(bytes: &[u8]) {
    crate::vga_buffer::with_writer(|writer| {
        use core::fmt::Write;
        if let Ok(text) = core::str::from_utf8(bytes) {
            let _ = writer.write_str(text);
        } else {
            for &byte in bytes {
                let ch = match byte {
                    b'\r' => '\r',
                    b'\n' => '\n',
                    b'\t' => '\t',
                    0x20..=0x7e => byte as char,
                    _ => '?',
                };
                let _ = writer.write_char(ch);
            }
        }
    });

    forward_to_framebuffer(bytes);
}
