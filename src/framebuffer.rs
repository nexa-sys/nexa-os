use core::fmt::{self, Write};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use font8x8::legacy::BASIC_LEGACY;
use multiboot2::{BootInformation, FramebufferField, FramebufferTag, FramebufferType};
use spin::Mutex;

use crate::kinfo;

const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 8;

#[derive(Clone, Copy, Debug)]
struct FramebufferSpec {
    address: u64,
    pitch: u32,
    width: u32,
    height: u32,
    bpp: u8,
    red: FramebufferField,
    green: FramebufferField,
    blue: FramebufferField,
}

struct PackedColor {
    bytes: [u8; 4],
    len: usize,
}

impl PackedColor {
    fn new(value: u32, len: usize) -> Self {
        let mut bytes = value.to_le_bytes();
        if len < 4 {
            for byte in bytes[len..].iter_mut() {
                *byte = 0;
            }
        }
        Self { bytes, len }
    }
}

pub struct FramebufferWriter {
    buffer: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    bytes_per_pixel: usize,
    cursor_x: usize,
    cursor_y: usize,
    columns: usize,
    rows: usize,
    fg: PackedColor,
    bg: PackedColor,
}

unsafe impl Send for FramebufferWriter {}

impl FramebufferWriter {
    fn new(buffer: *mut u8, spec: FramebufferSpec) -> Option<Self> {
        if spec.bpp < 16 {
            return None;
        }

        let bytes_per_pixel = ((spec.bpp + 7) / 8) as usize;
        let columns = (spec.width as usize) / FONT_WIDTH;
        let rows = (spec.height as usize) / FONT_HEIGHT;
        if columns == 0 || rows == 0 {
            return None;
        }

        let fg = PackedColor::new(pack_color(&spec, 0xFF, 0xFF, 0xFF), bytes_per_pixel);
        let bg = PackedColor::new(pack_color(&spec, 0x00, 0x00, 0x00), bytes_per_pixel);

        Some(Self {
            buffer,
            width: spec.width as usize,
            height: spec.height as usize,
            pitch: spec.pitch as usize,
            bytes_per_pixel,
            cursor_x: 0,
            cursor_y: 0,
            columns,
            rows,
            fg,
            bg,
        })
    }

    fn newline(&mut self) {
        self.cursor_x = 0;
        if self.cursor_y + 1 >= self.rows {
            self.scroll_up();
        } else {
            self.cursor_y += 1;
        }
    }

    fn scroll_up(&mut self) {
        let row_size = self.pitch * FONT_HEIGHT;
        let total = self.pitch * self.height;
        if total <= row_size {
            return;
        }
        unsafe {
            ptr::copy(self.buffer.add(row_size), self.buffer, total - row_size);
            let clear_start = self.buffer.add(total - row_size);
            for i in 0..row_size {
                clear_start.add(i).write_volatile(0);
            }
        }
    }

    fn draw_cell(&mut self, col: usize, row: usize, glyph: &[u8; FONT_HEIGHT]) {
        let pixel_x = col * FONT_WIDTH;
        let pixel_y = row * FONT_HEIGHT;
        for (row_offset, bits) in glyph.iter().enumerate() {
            let mut mask = 0x80u8;
            for col_offset in 0..FONT_WIDTH {
                let color = if bits & mask != 0 { &self.fg } else { &self.bg };
                self.write_pixel(pixel_x + col_offset, pixel_y + row_offset, color);
                mask >>= 1;
            }
        }
    }

    fn write_pixel(&self, x: usize, y: usize, color: &PackedColor) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = y * self.pitch + x * self.bytes_per_pixel;
        unsafe {
            for i in 0..self.bytes_per_pixel {
                let value = if i < color.len { color.bytes[i] } else { 0 };
                self.buffer.add(offset + i).write_volatile(value);
            }
        }
    }

    fn clear_cell(&mut self, col: usize, row: usize) {
        let pixel_x = col * FONT_WIDTH;
        let pixel_y = row * FONT_HEIGHT;
        for y in 0..FONT_HEIGHT {
            for x in 0..FONT_WIDTH {
                self.write_pixel(pixel_x + x, pixel_y + y, &self.bg);
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor_x == 0 {
            if self.cursor_y == 0 {
                return;
            }
            self.cursor_y -= 1;
            self.cursor_x = self.columns.saturating_sub(1);
        } else {
            self.cursor_x -= 1;
        }
        self.clear_cell(self.cursor_x, self.cursor_y);
    }

    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => {
                self.cursor_x = 0;
            }
            _ => {
                if self.cursor_x >= self.columns {
                    self.newline();
                }
                let glyph = if (c as usize) < BASIC_LEGACY.len() {
                    &BASIC_LEGACY[c as usize]
                } else {
                    &BASIC_LEGACY[b'?' as usize]
                };
                self.draw_cell(self.cursor_x, self.cursor_y, glyph);
                self.cursor_x += 1;
            }
        }
    }

    pub fn clear(&mut self) {
        for row in 0..self.rows {
            for col in 0..self.columns {
                self.clear_cell(col, row);
            }
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }
}

impl Write for FramebufferWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for ch in s.chars() {
            self.write_char(ch);
        }
        Ok(())
    }
}

static FRAMEBUFFER_SPEC: Mutex<Option<FramebufferSpec>> = Mutex::new(None);
static FRAMEBUFFER_READY: AtomicBool = AtomicBool::new(false);
static FRAMEBUFFER_WRITER: Mutex<Option<FramebufferWriter>> = Mutex::new(None);

fn pack_color(spec: &FramebufferSpec, r: u8, g: u8, b: u8) -> u32 {
    fn pack_component(field: &FramebufferField, value: u8) -> u32 {
        if field.size == 0 {
            return 0;
        }
        let max_value = if field.size >= 31 {
            u32::MAX
        } else {
            (1u32 << field.size) - 1
        };
        let scaled = (value as u32 * max_value + 127) / 255;
        scaled << field.position
    }

    pack_component(&spec.red, r) | pack_component(&spec.green, g) | pack_component(&spec.blue, b)
}

pub fn early_init(boot_info: &BootInformation<'_>) {
    if let Some(tag_result) = boot_info.framebuffer_tag() {
        match tag_result {
            Ok(tag) => store_spec(tag),
            Err(err) => {
                crate::kwarn!("Failed to decode framebuffer tag: {:?}", err);
            }
        }
    }
}

fn store_spec(tag: &FramebufferTag) {
    match tag.buffer_type() {
        Ok(FramebufferType::RGB { red, green, blue }) => {
            let spec = FramebufferSpec {
                address: tag.address(),
                pitch: tag.pitch(),
                width: tag.width(),
                height: tag.height(),
                bpp: tag.bpp(),
                red,
                green,
                blue,
            };
            *FRAMEBUFFER_SPEC.lock() = Some(spec);
            kinfo!(
                "Framebuffer discovered: {}x{} {}bpp (pitch {})",
                spec.width,
                spec.height,
                spec.bpp,
                spec.pitch
            );
        }
        Ok(FramebufferType::Indexed { .. }) => {
            crate::kwarn!("Indexed framebuffer detected; unsupported for now");
        }
        Ok(FramebufferType::Text) => {
            // Nothing to do; VGA text mode already handled elsewhere.
        }
        Err(err) => {
            crate::kwarn!("Unknown framebuffer type: {:?}", err);
        }
    }
}

pub fn activate() {
    if FRAMEBUFFER_READY.load(Ordering::SeqCst) {
        return;
    }

    let spec = {
        let guard = FRAMEBUFFER_SPEC.lock();
        match *guard {
            Some(spec) => spec,
            None => return,
        }
    };

    let length = (spec.pitch as usize).saturating_mul(spec.height as usize);
    if length == 0 {
        return;
    }

    let buffer_ptr = match unsafe { crate::paging::map_device_region(spec.address, length) } {
        Ok(ptr) => ptr,
        Err(err) => {
            crate::kwarn!("Failed to map framebuffer: {:?}", err);
            return;
        }
    };

    let mut writer_guard = FRAMEBUFFER_WRITER.lock();
    if writer_guard.is_none() {
        if let Some(mut writer) = FramebufferWriter::new(buffer_ptr, spec) {
            writer.clear();
            *writer_guard = Some(writer);
            FRAMEBUFFER_READY.store(true, Ordering::SeqCst);
            kinfo!(
                "Framebuffer activated at {:#x} ({}x{} @ {}bpp)",
                spec.address,
                spec.width,
                spec.height,
                spec.bpp
            );
        }
    }
}

pub fn is_ready() -> bool {
    FRAMEBUFFER_READY.load(Ordering::SeqCst)
}

pub fn clear() {
    if let Some(writer) = FRAMEBUFFER_WRITER.lock().as_mut() {
        writer.clear();
    }
}

pub fn backspace() {
    if let Some(writer) = FRAMEBUFFER_WRITER.lock().as_mut() {
        writer.backspace();
    }
}

pub fn try_with_writer<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut FramebufferWriter) -> R,
{
    FRAMEBUFFER_WRITER.lock().as_mut().map(f)
}

pub(crate) fn _print(args: fmt::Arguments<'_>) {
    if let Some(writer) = FRAMEBUFFER_WRITER.lock().as_mut() {
        writer.write_fmt(args).ok();
    }
}
