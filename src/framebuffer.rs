use core::fmt::{self, Write};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use font8x8::legacy::BASIC_LEGACY;
use multiboot2::{BootInformation, FramebufferField, FramebufferTag, FramebufferType};
use nexa_boot_info::FramebufferInfo as BootFramebufferInfo;
use spin::Mutex;

use crate::kinfo;

const BASE_FONT_WIDTH: usize = 8;
const BASE_FONT_HEIGHT: usize = 8;
const SCALE_X: usize = 2;
const SCALE_Y: usize = 2;
const CELL_WIDTH: usize = BASE_FONT_WIDTH * SCALE_X;
const CELL_HEIGHT: usize = BASE_FONT_HEIGHT * SCALE_Y;
const TAB_WIDTH: usize = 4;

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

#[derive(Clone, Copy)]
struct RgbColor {
    r: u8,
    g: u8,
    b: u8,
}

impl RgbColor {
    const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

const DEFAULT_FG: RgbColor = RgbColor::new(0xE6, 0xEC, 0xF1);
const DEFAULT_BG: RgbColor = RgbColor::new(0x08, 0x0C, 0x12);

const ANSI_BASE_COLORS: [RgbColor; 8] = [
    RgbColor::new(0x00, 0x00, 0x00), // Black
    RgbColor::new(0xAA, 0x00, 0x00), // Red
    RgbColor::new(0x00, 0xAA, 0x00), // Green
    RgbColor::new(0xAA, 0x55, 0x00), // Yellow/Brown
    RgbColor::new(0x00, 0x00, 0xAA), // Blue
    RgbColor::new(0xAA, 0x00, 0xAA), // Magenta
    RgbColor::new(0x00, 0xAA, 0xAA), // Cyan
    RgbColor::new(0xAA, 0xAA, 0xAA), // Light gray
];

const ANSI_BRIGHT_COLORS: [RgbColor; 8] = [
    RgbColor::new(0x55, 0x55, 0x55), // Dark gray
    RgbColor::new(0xFF, 0x55, 0x55), // Bright red
    RgbColor::new(0x55, 0xFF, 0x55), // Bright green
    RgbColor::new(0xFF, 0xFF, 0x55), // Bright yellow
    RgbColor::new(0x55, 0x55, 0xFF), // Bright blue
    RgbColor::new(0xFF, 0x55, 0xFF), // Bright magenta
    RgbColor::new(0x55, 0xFF, 0xFF), // Bright cyan
    RgbColor::new(0xFF, 0xFF, 0xFF), // White
];

#[derive(Clone, Copy)]
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

#[derive(Clone, Copy)]
enum AnsiState {
    Ground,
    Escape,
    Csi,
}

pub struct FramebufferWriter {
    buffer: *mut u8,
    spec: FramebufferSpec,
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
    fg_rgb: RgbColor,
    bg_rgb: RgbColor,
    default_fg_rgb: RgbColor,
    default_bg_rgb: RgbColor,
    bold: bool,
    ansi_state: AnsiState,
    ansi_param_buf: [u8; 32],
    ansi_param_len: usize,
}

unsafe impl Send for FramebufferWriter {}

impl FramebufferWriter {
    fn new(buffer: *mut u8, spec: FramebufferSpec) -> Option<Self> {
        if spec.bpp < 16 {
            return None;
        }

        let bytes_per_pixel = ((spec.bpp + 7) / 8) as usize;
        let columns = (spec.width as usize) / CELL_WIDTH;
        let rows = (spec.height as usize) / CELL_HEIGHT;
        if columns == 0 || rows == 0 {
            return None;
        }

        let default_fg = PackedColor::new(
            pack_color(&spec, DEFAULT_FG.r, DEFAULT_FG.g, DEFAULT_FG.b),
            bytes_per_pixel,
        );
        let default_bg = PackedColor::new(
            pack_color(&spec, DEFAULT_BG.r, DEFAULT_BG.g, DEFAULT_BG.b),
            bytes_per_pixel,
        );

        Some(Self {
            buffer,
            spec,
            width: spec.width as usize,
            height: spec.height as usize,
            pitch: spec.pitch as usize,
            bytes_per_pixel,
            cursor_x: 0,
            cursor_y: 0,
            columns,
            rows,
            fg: default_fg,
            bg: default_bg,
            fg_rgb: DEFAULT_FG,
            bg_rgb: DEFAULT_BG,
            default_fg_rgb: DEFAULT_FG,
            default_bg_rgb: DEFAULT_BG,
            bold: false,
            ansi_state: AnsiState::Ground,
            ansi_param_buf: [0; 32],
            ansi_param_len: 0,
        })
    }

    fn pack_rgb(&self, color: RgbColor) -> PackedColor {
        PackedColor::new(
            pack_color(&self.spec, color.r, color.g, color.b),
            self.bytes_per_pixel,
        )
    }

    fn set_colors(&mut self, fg: RgbColor, bg: RgbColor) {
        self.fg = self.pack_rgb(fg);
        self.bg = self.pack_rgb(bg);
        self.fg_rgb = fg;
        self.bg_rgb = bg;
    }

    fn reset_colors(&mut self) {
        self.bold = false;
        self.set_colors(self.default_fg_rgb, self.default_bg_rgb);
    }

    fn set_fg_color(&mut self, color: RgbColor) {
        self.fg = self.pack_rgb(color);
        self.fg_rgb = color;
    }

    fn set_bg_color(&mut self, color: RgbColor) {
        self.bg = self.pack_rgb(color);
        self.bg_rgb = color;
    }

    fn reset_fg(&mut self) {
        self.set_fg_color(self.default_fg_rgb);
    }

    fn reset_bg(&mut self) {
        self.set_bg_color(self.default_bg_rgb);
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
        let row_size = self.pitch * CELL_HEIGHT;
        let total = self.pitch * self.height;
        if total <= row_size {
            return;
        }
        unsafe {
            ptr::copy(self.buffer.add(row_size), self.buffer, total - row_size);
            let clear_start = self.buffer.add(total - row_size);
            self.fill_raw_block(clear_start, row_size, &self.bg);
        }
    }

    fn fill_raw_block(&self, start: *mut u8, len: usize, color: &PackedColor) {
        let mut offset = 0;
        while offset + self.bytes_per_pixel <= len {
            unsafe {
                for i in 0..self.bytes_per_pixel {
                    let value = if i < color.len { color.bytes[i] } else { 0 };
                    start.add(offset + i).write_volatile(value);
                }
            }
            offset += self.bytes_per_pixel;
        }

        while offset < len {
            let idx = offset % self.bytes_per_pixel;
            let value = if idx < color.len { color.bytes[idx] } else { 0 };
            unsafe {
                start.add(offset).write_volatile(value);
            }
            offset += 1;
        }
    }

    fn draw_cell(&mut self, col: usize, row: usize, glyph: &[u8; BASE_FONT_HEIGHT]) {
        let pixel_x = col * CELL_WIDTH;
        let pixel_y = row * CELL_HEIGHT;
        for (row_offset, bits) in glyph.iter().enumerate() {
            for sy in 0..SCALE_Y {
                let target_y = pixel_y + row_offset * SCALE_Y + sy;
                for col_offset in 0..BASE_FONT_WIDTH {
                    let mask = 1u8 << col_offset;
                    let color = if bits & mask != 0 { &self.fg } else { &self.bg };
                    let target_x = pixel_x + col_offset * SCALE_X;
                    for sx in 0..SCALE_X {
                        self.write_pixel(target_x + sx, target_y, color);
                    }
                }
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
        let pixel_x = col * CELL_WIDTH;
        let pixel_y = row * CELL_HEIGHT;
        for y in 0..CELL_HEIGHT {
            for x in 0..CELL_WIDTH {
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
            '\t' => {
                let next_tab = ((self.cursor_x / TAB_WIDTH) + 1) * TAB_WIDTH;
                while self.cursor_x < next_tab {
                    self.write_char(' ');
                }
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

    fn process_byte(&mut self, byte: u8) {
        match self.ansi_state {
            AnsiState::Ground => match byte {
                0x1B => {
                    self.ansi_state = AnsiState::Escape;
                }
                0x08 => self.backspace(),
                b'\n' | b'\r' | b'\t' => self.write_char(byte as char),
                0x20..=0x7E => self.write_char(byte as char),
                _ => {}
            },
            AnsiState::Escape => {
                if byte == b'[' {
                    self.ansi_state = AnsiState::Csi;
                    self.ansi_param_len = 0;
                } else {
                    self.ansi_state = AnsiState::Ground;
                    self.process_byte(byte);
                }
            }
            AnsiState::Csi => {
                match byte {
                    b'0'..=b'9' | b';' => {
                        if self.ansi_param_len < self.ansi_param_buf.len() {
                            self.ansi_param_buf[self.ansi_param_len] = byte;
                            self.ansi_param_len += 1;
                        }
                    }
                    b'm' | b'J' | b'K' => {
                        let (params, count) = self.parse_params();
                        self.handle_csi(byte, &params[..count]);
                        self.ansi_state = AnsiState::Ground;
                        self.ansi_param_len = 0;
                    }
                    0x1B => {
                        // Restart escape sequence if a new ESC arrives mid-CSI
                        self.ansi_state = AnsiState::Escape;
                        self.ansi_param_len = 0;
                        self.process_byte(0x1B);
                    }
                    _ => {
                        self.ansi_state = AnsiState::Ground;
                    }
                }
            }
        }
    }

    fn parse_params(&self) -> ([u16; 16], usize) {
        let mut params = [0u16; 16];
        if self.ansi_param_len == 0 {
            params[0] = 0;
            return (params, 1);
        }

        let mut count = 0usize;
        let mut value = 0u16;
        let mut has_value = false;
        for &byte in &self.ansi_param_buf[..self.ansi_param_len] {
            if byte == b';' {
                if count < params.len() {
                    params[count] = if has_value { value } else { 0 };
                    count += 1;
                }
                value = 0;
                has_value = false;
            } else if byte.is_ascii_digit() {
                value = value
                    .saturating_mul(10)
                    .saturating_add((byte - b'0') as u16);
                has_value = true;
            }
        }

        if count < params.len() {
            params[count] = if has_value { value } else { 0 };
            count += 1;
        }

        (params, count)
    }

    fn handle_csi(&mut self, command: u8, params: &[u16]) {
        match command {
            b'm' => self.apply_sgr(params),
            b'J' => self.handle_erase_display(params),
            b'K' => self.handle_erase_line(params),
            _ => {}
        }
    }

    fn handle_erase_display(&mut self, params: &[u16]) {
        let mode = params.first().copied().unwrap_or(0);
        match mode {
            2 => self.clear(),
            0 => {
                // Clear from cursor to end of screen
                let start_pixel_y = self.cursor_y * CELL_HEIGHT;
                let start_pixel_x = self.cursor_x * CELL_WIDTH;
                let width = self.width.saturating_sub(start_pixel_x);
                self.fill_rect(start_pixel_x, start_pixel_y, width, CELL_HEIGHT, self.bg);
                self.clear_area(self.cursor_y + 1, self.rows);
            }
            1 => {
                // Clear from start to cursor
                self.clear_area(0, self.cursor_y);
                let start_pixel_y = self.cursor_y * CELL_HEIGHT;
                self.fill_rect(
                    0,
                    start_pixel_y,
                    self.cursor_x * CELL_WIDTH,
                    CELL_HEIGHT,
                    self.bg,
                );
            }
            _ => {}
        }
    }

    fn handle_erase_line(&mut self, params: &[u16]) {
        let mode = params.first().copied().unwrap_or(0);
        let pixel_y = self.cursor_y * CELL_HEIGHT;
        match mode {
            0 => {
                let start_pixel_x = self.cursor_x * CELL_WIDTH;
                let width = self.width.saturating_sub(start_pixel_x);
                self.fill_rect(start_pixel_x, pixel_y, width, CELL_HEIGHT, self.bg);
            }
            1 => {
                self.fill_rect(0, pixel_y, self.cursor_x * CELL_WIDTH, CELL_HEIGHT, self.bg);
            }
            2 => {
                self.fill_rect(0, pixel_y, self.width, CELL_HEIGHT, self.bg);
            }
            _ => {}
        }
    }

    fn clear_area(&mut self, start_row: usize, end_row: usize) {
        for row in start_row..end_row {
            let pixel_y = row * CELL_HEIGHT;
            self.fill_rect(0, pixel_y, self.width, CELL_HEIGHT, self.bg);
        }
    }

    fn fill_rect(
        &mut self,
        start_x: usize,
        start_y: usize,
        width: usize,
        height: usize,
        color: PackedColor,
    ) {
        for y in 0..height {
            for x in 0..width {
                self.write_pixel(start_x + x, start_y + y, &color);
            }
        }
    }

    fn apply_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.reset_colors();
            return;
        }

        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => {
                    self.reset_colors();
                }
                1 => {
                    self.bold = true;
                }
                2 | 22 => {
                    self.bold = false;
                }
                30..=37 => {
                    let idx = (params[i] - 30) as usize;
                    let bright = self.bold;
                    self.set_fg_color(select_color(idx, bright));
                }
                90..=97 => {
                    let idx = (params[i] - 90) as usize;
                    self.set_fg_color(select_color(idx, true));
                }
                39 => self.reset_fg(),
                40..=47 => {
                    let idx = (params[i] - 40) as usize;
                    self.set_bg_color(select_color(idx, false));
                }
                100..=107 => {
                    let idx = (params[i] - 100) as usize;
                    self.set_bg_color(select_color(idx, true));
                }
                49 => self.reset_bg(),
                38 => {
                    if i + 4 < params.len() && params[i + 1] == 2 {
                        let r = params[i + 2].min(255) as u8;
                        let g = params[i + 3].min(255) as u8;
                        let b = params[i + 4].min(255) as u8;
                        self.set_fg_color(RgbColor::new(r, g, b));
                        i += 4;
                    }
                }
                48 => {
                    if i + 4 < params.len() && params[i + 1] == 2 {
                        let r = params[i + 2].min(255) as u8;
                        let g = params[i + 3].min(255) as u8;
                        let b = params[i + 4].min(255) as u8;
                        self.set_bg_color(RgbColor::new(r, g, b));
                        i += 4;
                    }
                }
                _ => {}
            }
            i += 1;
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
        self.reset_colors();
        self.ansi_state = AnsiState::Ground;
        self.ansi_param_len = 0;
    }
}

impl Write for FramebufferWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for ch in s.chars() {
            if ch == '\u{1B}' {
                self.process_byte(0x1B);
            } else if (ch as u32) <= 0x7F {
                self.process_byte(ch as u8);
            } else {
                self.process_byte(b'?');
            }
        }
        Ok(())
    }
}

fn select_color(index: usize, bright: bool) -> RgbColor {
    if bright {
        ANSI_BRIGHT_COLORS.get(index).copied().unwrap_or(DEFAULT_FG)
    } else {
        ANSI_BASE_COLORS.get(index).copied().unwrap_or(DEFAULT_FG)
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

pub fn install_from_bootinfo(info: &BootFramebufferInfo) {
    if !info.is_valid() {
        return;
    }

    let spec = FramebufferSpec {
        address: info.address,
        pitch: info.pitch,
        width: info.width,
        height: info.height,
        bpp: info.bpp,
        red: FramebufferField {
            position: info.red_position,
            size: info.red_size,
        },
        green: FramebufferField {
            position: info.green_position,
            size: info.green_size,
        },
        blue: FramebufferField {
            position: info.blue_position,
            size: info.blue_size,
        },
    };

    *FRAMEBUFFER_SPEC.lock() = Some(spec);
    FRAMEBUFFER_READY.store(false, Ordering::SeqCst);
    kinfo!(
        "Framebuffer provided by UEFI: {}x{} {}bpp (pitch {})",
        spec.width,
        spec.height,
        spec.bpp,
        spec.pitch
    );
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
    let mut activated = false;
    if writer_guard.is_none() {
        if let Some(mut writer) = FramebufferWriter::new(buffer_ptr, spec) {
            writer.clear();
            *writer_guard = Some(writer);
            FRAMEBUFFER_READY.store(true, Ordering::SeqCst);
            activated = true;
        }
    }
    drop(writer_guard);

    if activated {
        kinfo!(
            "Framebuffer activated at {:#x} ({}x{} @ {}bpp)",
            spec.address,
            spec.width,
            spec.height,
            spec.bpp
        );
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

pub fn write_str(text: &str) {
    write_bytes(text.as_bytes());
}

pub fn write_bytes(bytes: &[u8]) {
    if !FRAMEBUFFER_READY.load(Ordering::SeqCst) {
        return;
    }

    if let Some(mut guard) = FRAMEBUFFER_WRITER.try_lock() {
        if let Some(writer) = guard.as_mut() {
            for &byte in bytes {
                writer.process_byte(byte);
            }
        }
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
