//! ANSI escape sequence parser
//!
//! This module handles ANSI terminal escape sequences including:
//! - SGR (Select Graphic Rendition) for colors and text attributes
//! - CSI sequences for cursor control and screen clearing

use super::color::{select_color, RgbColor};

/// ANSI parser state machine states
#[derive(Clone, Copy)]
pub enum AnsiState {
    /// Normal character processing
    Ground,
    /// Received ESC (0x1B), waiting for sequence type
    Escape,
    /// Processing CSI sequence (ESC [)
    Csi,
}

/// ANSI escape sequence parser
///
/// Maintains parser state and provides methods for processing
/// escape sequences and extracting color/attribute changes.
pub struct AnsiParser {
    pub state: AnsiState,
    pub param_buf: [u8; 32],
    pub param_len: usize,
}

impl AnsiParser {
    pub const fn new() -> Self {
        Self {
            state: AnsiState::Ground,
            param_buf: [0; 32],
            param_len: 0,
        }
    }

    /// Reset parser to ground state
    pub fn reset(&mut self) {
        self.state = AnsiState::Ground;
        self.param_len = 0;
    }

    /// Add a byte to the parameter buffer
    pub fn push_param(&mut self, byte: u8) {
        if self.param_len < self.param_buf.len() {
            self.param_buf[self.param_len] = byte;
            self.param_len += 1;
        }
    }

    /// Parse accumulated parameters into numeric values
    pub fn parse_params(&self) -> ([u16; 16], usize) {
        let mut params = [0u16; 16];
        if self.param_len == 0 {
            params[0] = 0;
            return (params, 1);
        }

        let mut count = 0usize;
        let mut value = 0u16;
        let mut has_value = false;

        for &byte in &self.param_buf[..self.param_len] {
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
}

/// Result of SGR parameter processing
pub enum SgrAction {
    /// Reset all attributes to default
    Reset,
    /// Enable bold/bright mode
    Bold,
    /// Disable bold/bright mode
    NoBold,
    /// Set foreground to ANSI color (index, bright)
    SetFg(usize, bool),
    /// Set background to ANSI color (index, bright)
    SetBg(usize, bool),
    /// Set foreground to RGB color
    SetFgRgb(RgbColor),
    /// Set background to RGB color
    SetBgRgb(RgbColor),
    /// Reset foreground to default
    ResetFg,
    /// Reset background to default
    ResetBg,
    /// Unknown or unsupported parameter
    None,
}

/// Process SGR parameters and yield actions
///
/// This is a stateful iterator that processes SGR parameters
/// and yields the corresponding actions.
pub struct SgrProcessor<'a> {
    params: &'a [u16],
    index: usize,
    bold: bool,
}

impl<'a> SgrProcessor<'a> {
    pub fn new(params: &'a [u16], bold: bool) -> Self {
        Self {
            params,
            index: 0,
            bold,
        }
    }

    pub fn is_bold(&self) -> bool {
        self.bold
    }
}

impl Iterator for SgrProcessor<'_> {
    type Item = SgrAction;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.params.len() {
            return None;
        }

        let param = self.params[self.index];
        self.index += 1;

        let action = match param {
            0 => {
                self.bold = false;
                SgrAction::Reset
            }
            1 => {
                self.bold = true;
                SgrAction::Bold
            }
            2 | 22 => {
                self.bold = false;
                SgrAction::NoBold
            }
            30..=37 => {
                let idx = (param - 30) as usize;
                SgrAction::SetFg(idx, self.bold)
            }
            90..=97 => {
                let idx = (param - 90) as usize;
                SgrAction::SetFg(idx, true)
            }
            39 => SgrAction::ResetFg,
            40..=47 => {
                let idx = (param - 40) as usize;
                SgrAction::SetBg(idx, false)
            }
            100..=107 => {
                let idx = (param - 100) as usize;
                SgrAction::SetBg(idx, true)
            }
            49 => SgrAction::ResetBg,
            38 => {
                // 24-bit foreground: 38;2;r;g;b
                if self.index + 3 < self.params.len() && self.params[self.index] == 2 {
                    let r = self.params[self.index + 1].min(255) as u8;
                    let g = self.params[self.index + 2].min(255) as u8;
                    let b = self.params[self.index + 3].min(255) as u8;
                    self.index += 4;
                    SgrAction::SetFgRgb(RgbColor::new(r, g, b))
                } else {
                    SgrAction::None
                }
            }
            48 => {
                // 24-bit background: 48;2;r;g;b
                if self.index + 3 < self.params.len() && self.params[self.index] == 2 {
                    let r = self.params[self.index + 1].min(255) as u8;
                    let g = self.params[self.index + 2].min(255) as u8;
                    let b = self.params[self.index + 3].min(255) as u8;
                    self.index += 4;
                    SgrAction::SetBgRgb(RgbColor::new(r, g, b))
                } else {
                    SgrAction::None
                }
            }
            _ => SgrAction::None,
        };

        Some(action)
    }
}

/// Helper to get color from SGR action
pub fn color_from_sgr(idx: usize, bright: bool) -> RgbColor {
    select_color(idx, bright)
}
