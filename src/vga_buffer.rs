use core::fmt::{self, Write};
use core::ptr;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::kinfo;

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;
const VGA_BUFFER_ADDR: usize = 0xb8000;

#[allow(dead_code)]
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Color {
    Black = 0x0,
    Blue = 0x1,
    Green = 0x2,
    Cyan = 0x3,
    Red = 0x4,
    Magenta = 0x5,
    Brown = 0x6,
    LightGray = 0x7,
    DarkGray = 0x8,
    LightBlue = 0x9,
    LightGreen = 0xA,
    LightCyan = 0xB,
    LightRed = 0xC,
    Pink = 0xD,
    Yellow = 0xE,
    White = 0xF,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ColorCode(u8);

impl ColorCode {
    const fn new(foreground: Color, background: Color) -> Self {
        Self((background as u8) << 4 | (foreground as u8))
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

pub static VGA_WRITER: Mutex<Writer> = Mutex::new(Writer::new());

// Indicates whether the VGA buffer has been mapped and is safe to write.
pub static VGA_READY: AtomicBool = AtomicBool::new(false);

pub fn set_vga_ready() {
    // Mark VGA as ready and emit a serial-only confirmation so we can observe
    // at runtime whether VGA ready was actually set. We avoid using the
    // higher-level kinfo! macro here to guarantee this appears on serial even
    // if logger or VGA are in an unexpected state.
    VGA_READY.store(true, Ordering::SeqCst);
    kinfo!("VGA_READY set");
}

pub fn is_vga_ready() -> bool {
    VGA_READY.load(Ordering::SeqCst)
}

pub fn init() {
    // Initialize the VGA writer and clear the screen. Also emit a serial-only
    // notice that we ran VGA init to help debug early boot ordering issues.
    clear_screen();
    // Use kernel logging macro so the message follows the kernel's logging
    // conventions (emitted to serial and VGA when available).
    crate::kinfo!("vga_buffer::init() called");
}

pub(crate) fn _print(args: fmt::Arguments<'_>) {
    // Print to both VGA and serial
    crate::serial::_print(args);
    with_writer(|writer| {
        writer.write_fmt(args).ok();
    });
}

pub struct Writer {
    column_position: usize,
    color_code: ColorCode,
    buffer_ptr: *mut ScreenChar,
}

unsafe impl Send for Writer {}

impl Writer {
    const fn new() -> Self {
        Self {
            column_position: 0,
            color_code: ColorCode::new(Color::LightGreen, Color::Black),
            buffer_ptr: VGA_BUFFER_ADDR as *mut ScreenChar,
        }
    }

    pub fn color_code(&self) -> ColorCode {
        self.color_code
    }

    pub fn set_color(&mut self, foreground: Color, background: Color) {
        self.color_code = ColorCode::new(foreground, background);
    }

    pub fn set_color_code(&mut self, color_code: ColorCode) {
        self.color_code = color_code;
    }

    pub fn with_color<F, R>(&mut self, foreground: Color, background: Color, f: F) -> R
    where
        F: FnOnce(&mut Writer) -> R,
    {
        let previous = self.color_code;
        self.color_code = ColorCode::new(foreground, background);
        let result = f(self);
        self.color_code = previous;
        result
    }

    fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                unsafe {
                    self.write_at(
                        row,
                        col,
                        ScreenChar {
                            ascii_character: byte,
                            color_code: self.color_code,
                        },
                    );
                }
                self.column_position += 1;
            }
        }
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                unsafe {
                    let character = self.read_at(row, col);
                    self.write_at(row - 1, col, character);
                }
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            unsafe {
                self.write_at(row, col, blank);
            }
        }
    }

    unsafe fn write_at(&mut self, row: usize, col: usize, character: ScreenChar) {
        let index = row * BUFFER_WIDTH + col;
        ptr::write_volatile(self.buffer_ptr.add(index), character);
    }

    unsafe fn read_at(&self, row: usize, col: usize) -> ScreenChar {
        let index = row * BUFFER_WIDTH + col;
        ptr::read_volatile(self.buffer_ptr.add(index))
    }
}

impl Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                _ => self.write_byte(0xfe),
            }
        }
        Ok(())
    }
}

impl Writer {
    pub fn clear_screen(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row);
        }
        self.column_position = 0;
    }
}

pub fn clear_screen() {
    let mut writer = VGA_WRITER.lock();
    for row in 0..BUFFER_HEIGHT {
        writer.clear_row(row);
    }
    writer.column_position = 0;
}

pub fn with_writer<F, R>(f: F) -> R
where
    F: FnOnce(&mut Writer) -> R,
{
    let mut writer = VGA_WRITER.lock();
    f(&mut writer)
}

pub static WRITER: Mutex<Option<&'static mut Writer>> = Mutex::new(None);

pub fn print_char(c: char) {
    if let Some(writer) = WRITER.lock().as_mut() {
        writer.write_byte(c as u8);
    }
}
