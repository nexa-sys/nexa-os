use core::fmt::{self, Write};
use core::ptr;
use spin::Mutex;

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
struct ColorCode(u8);

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

pub fn init() {
    clear_screen();
}

pub fn writer() -> impl Write {
    VGAWriter
}

pub(crate) fn _print(args: fmt::Arguments<'_>) {
    VGAWriter.write_fmt(args).ok();
}

struct VGAWriter;

impl Write for VGAWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        VGA_WRITER.lock().write_str(s)
    }
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

pub fn clear_screen() {
    let mut writer = VGA_WRITER.lock();
    for row in 0..BUFFER_HEIGHT {
        writer.clear_row(row);
    }
    writer.column_position = 0;
}
