//! Screen rendering for the editor

use crate::buffer::Buffer;
use crate::mode::Mode;
use crate::terminal::{Color, Terminal};

/// Syntax highlighting token types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Normal,
    Keyword,
    Type,
    String,
    Number,
    Comment,
    Function,
    Operator,
    Macro,
    Constant,
    Special,
}

impl TokenType {
    pub fn color(&self) -> Color {
        match self {
            TokenType::Normal => Color::Default,
            TokenType::Keyword => Color::Blue,
            TokenType::Type => Color::Cyan,
            TokenType::String => Color::Green,
            TokenType::Number => Color::Magenta,
            TokenType::Comment => Color::Indexed(245), // Gray
            TokenType::Function => Color::Yellow,
            TokenType::Operator => Color::Red,
            TokenType::Macro => Color::Cyan,
            TokenType::Constant => Color::Magenta,
            TokenType::Special => Color::Yellow,
        }
    }
}

/// Renderer for the editor screen
pub struct Renderer {
    /// Number of lines reserved for status bar
    status_lines: usize,
    /// Whether to show line numbers
    pub show_line_numbers: bool,
    /// Width of line number column
    line_number_width: usize,
    /// Tab stop width
    pub tabstop: usize,
}

impl Renderer {
    pub fn new() -> Self {
        Renderer {
            status_lines: 2, // Status line + command line
            show_line_numbers: true,
            line_number_width: 4,
            tabstop: 4,
        }
    }

    /// Get the number of text rows available
    pub fn text_rows(&self, term_rows: usize) -> usize {
        term_rows.saturating_sub(self.status_lines)
    }

    /// Get the text column offset (for line numbers)
    pub fn text_col_offset(&self) -> usize {
        if self.show_line_numbers {
            self.line_number_width + 1
        } else {
            0
        }
    }

    /// Render the entire screen
    pub fn render(
        &mut self,
        term: &mut Terminal,
        buffer: &Buffer,
        mode: Mode,
        cursor_line: usize,
        cursor_col: usize,
        scroll_row: usize,
        scroll_col: usize,
        visual_start: Option<(usize, usize)>,
        message: &str,
        command_line: &str,
    ) {
        let size = term.get_size();
        let text_rows = self.text_rows(size.rows);

        // Update line number width based on total lines
        self.line_number_width = format!("{}", buffer.line_count()).len().max(4);

        term.hide_cursor();
        term.move_cursor(1, 1);

        // Render text area
        for row in 0..text_rows {
            let line_idx = scroll_row + row;

            term.move_cursor(row + 1, 1);
            term.clear_to_eol();

            // Line numbers
            if self.show_line_numbers {
                if line_idx < buffer.line_count() {
                    term.set_fg_color(Color::Yellow);
                    let line_num =
                        format!("{:>width$}", line_idx + 1, width = self.line_number_width);
                    term.write_str(&line_num);
                    term.reset_style();
                    term.write_char(' ');
                } else {
                    term.set_fg_color(Color::Blue);
                    let tilde = format!("{:>width$}", "~", width = self.line_number_width);
                    term.write_str(&tilde);
                    term.reset_style();
                    term.write_char(' ');
                }
            }

            // Line content
            if line_idx < buffer.line_count() {
                let line = &buffer.lines[line_idx];
                self.render_line(
                    term,
                    &line.content,
                    scroll_col,
                    size.cols.saturating_sub(self.text_col_offset()),
                    line_idx,
                    cursor_line,
                    cursor_col,
                    visual_start,
                    &buffer.filetype,
                );
            }
        }

        // Render status line
        self.render_status_line(term, buffer, mode, cursor_line, cursor_col, size);

        // Render command/message line
        self.render_command_line(term, mode, message, command_line, size);

        // Position cursor
        let screen_row = cursor_line.saturating_sub(scroll_row) + 1;
        let screen_col = self.text_col_offset()
            + self
                .display_col(&buffer.lines[cursor_line].content, cursor_col)
                .saturating_sub(scroll_col)
            + 1;

        term.move_cursor(screen_row, screen_col);
        term.show_cursor();

        let _ = term.flush();
    }

    /// Render a single line with syntax highlighting
    fn render_line(
        &self,
        term: &mut Terminal,
        content: &str,
        scroll_col: usize,
        max_width: usize,
        line_idx: usize,
        cursor_line: usize,
        cursor_col: usize,
        visual_start: Option<(usize, usize)>,
        filetype: &str,
    ) {
        let tokens = self.tokenize(content, filetype);

        let mut display_col = 0;
        let mut written = 0;

        for (token_type, text) in tokens {
            for ch in text.chars() {
                let char_width = if ch == '\t' {
                    self.tabstop - (display_col % self.tabstop)
                } else {
                    1
                };

                // Check if in visible area
                if display_col + char_width > scroll_col && written < max_width {
                    // Check for visual selection highlighting
                    let in_selection = self.is_in_selection(
                        line_idx,
                        display_col,
                        cursor_line,
                        cursor_col,
                        visual_start,
                    );

                    if in_selection {
                        term.set_reverse();
                    }

                    term.set_fg_color(token_type.color());

                    if ch == '\t' {
                        // Render tab as spaces
                        let spaces_to_write = (char_width).min(max_width - written);
                        for _ in 0..spaces_to_write {
                            term.write_char(' ');
                            written += 1;
                        }
                    } else if display_col >= scroll_col {
                        term.write_char(ch);
                        written += 1;
                    }

                    term.reset_style();
                }

                display_col += char_width;
            }
        }
    }

    /// Check if a position is in the visual selection
    fn is_in_selection(
        &self,
        line: usize,
        col: usize,
        cursor_line: usize,
        cursor_col: usize,
        visual_start: Option<(usize, usize)>,
    ) -> bool {
        if let Some((start_line, start_col)) = visual_start {
            let (begin_line, begin_col, end_line, end_col) = if start_line < cursor_line
                || (start_line == cursor_line && start_col < cursor_col)
            {
                (start_line, start_col, cursor_line, cursor_col)
            } else {
                (cursor_line, cursor_col, start_line, start_col)
            };

            if line > begin_line && line < end_line {
                return true;
            }
            if line == begin_line && line == end_line {
                return col >= begin_col && col <= end_col;
            }
            if line == begin_line {
                return col >= begin_col;
            }
            if line == end_line {
                return col <= end_col;
            }
        }
        false
    }

    /// Render the status line
    fn render_status_line(
        &self,
        term: &mut Terminal,
        buffer: &Buffer,
        mode: Mode,
        cursor_line: usize,
        cursor_col: usize,
        size: crate::terminal::TermSize,
    ) {
        let status_row = size.rows - 1;

        term.move_cursor(status_row, 1);
        term.set_reverse();

        // Left side: mode and filename
        let mode_str = format!(" {} ", mode.indicator());
        let modified = if buffer.modified { "[+]" } else { "" };
        let readonly = if buffer.readonly { "[RO]" } else { "" };
        let left = format!("{} {}{}{}", mode_str, buffer.name, modified, readonly);

        // Right side: position info
        let right = format!(
            " {}:{} ({}/{}) ",
            cursor_line + 1,
            cursor_col + 1,
            cursor_line + 1,
            buffer.line_count()
        );

        // Calculate padding
        let padding = size.cols.saturating_sub(left.len() + right.len());

        term.write_str(&left);
        for _ in 0..padding {
            term.write_char(' ');
        }
        term.write_str(&right);

        term.reset_style();
    }

    /// Render the command/message line
    fn render_command_line(
        &self,
        term: &mut Terminal,
        mode: Mode,
        message: &str,
        command_line: &str,
        size: crate::terminal::TermSize,
    ) {
        term.move_cursor(size.rows, 1);
        term.clear_to_eol();

        match mode {
            Mode::Command => {
                term.write_char(':');
                term.write_str(command_line);
            }
            Mode::Search => {
                term.write_char('/');
                term.write_str(command_line);
            }
            _ => {
                if !message.is_empty() {
                    term.write_str(message);
                }
            }
        }
    }

    /// Calculate display column from character column (accounting for tabs)
    fn display_col(&self, content: &str, char_col: usize) -> usize {
        let mut display = 0;
        for (i, ch) in content.chars().enumerate() {
            if i >= char_col {
                break;
            }
            if ch == '\t' {
                display += self.tabstop - (display % self.tabstop);
            } else {
                display += 1;
            }
        }
        display
    }

    /// Simple tokenizer for syntax highlighting
    fn tokenize<'a>(&self, content: &'a str, filetype: &str) -> Vec<(TokenType, &'a str)> {
        if filetype.is_empty() {
            return vec![(TokenType::Normal, content)];
        }

        let mut tokens = Vec::new();
        let mut pos = 0;
        let bytes = content.as_bytes();

        while pos < bytes.len() {
            // Comments
            if filetype == "rust" || filetype == "c" || filetype == "cpp" {
                if pos + 1 < bytes.len() && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
                    tokens.push((TokenType::Comment, &content[pos..]));
                    break;
                }
            }
            if filetype == "python" || filetype == "sh" || filetype == "vim" {
                if bytes[pos] == b'#' {
                    tokens.push((TokenType::Comment, &content[pos..]));
                    break;
                }
            }

            // Strings
            if bytes[pos] == b'"' || bytes[pos] == b'\'' {
                let quote = bytes[pos];
                let start = pos;
                pos += 1;
                while pos < bytes.len() && bytes[pos] != quote {
                    if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
                        pos += 2;
                    } else {
                        pos += 1;
                    }
                }
                if pos < bytes.len() {
                    pos += 1;
                }
                tokens.push((TokenType::String, &content[start..pos]));
                continue;
            }

            // Numbers
            if bytes[pos].is_ascii_digit() {
                let start = pos;
                while pos < bytes.len()
                    && (bytes[pos].is_ascii_alphanumeric()
                        || bytes[pos] == b'.'
                        || bytes[pos] == b'_')
                {
                    pos += 1;
                }
                tokens.push((TokenType::Number, &content[start..pos]));
                continue;
            }

            // Identifiers and keywords
            if bytes[pos].is_ascii_alphabetic() || bytes[pos] == b'_' {
                let start = pos;
                while pos < bytes.len()
                    && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_')
                {
                    pos += 1;
                }
                let word = &content[start..pos];
                let token_type = self.classify_word(word, filetype);
                tokens.push((token_type, word));
                continue;
            }

            // Operators and other characters
            let start = pos;
            pos += 1;
            tokens.push((TokenType::Normal, &content[start..pos]));
        }

        tokens
    }

    /// Classify a word as keyword, type, etc.
    fn classify_word(&self, word: &str, filetype: &str) -> TokenType {
        match filetype {
            "rust" => match word {
                "fn" | "let" | "mut" | "const" | "static" | "if" | "else" | "match" | "loop"
                | "while" | "for" | "in" | "return" | "break" | "continue" | "pub" | "mod"
                | "use" | "struct" | "enum" | "impl" | "trait" | "where" | "type" | "as"
                | "ref" | "move" | "async" | "await" | "unsafe" | "extern" | "crate" | "self"
                | "super" | "dyn" => TokenType::Keyword,
                "bool" | "char" | "str" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
                | "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "f32" | "f64" | "String"
                | "Vec" | "Option" | "Result" | "Box" | "Rc" | "Arc" | "Self" => TokenType::Type,
                "true" | "false" | "None" | "Some" | "Ok" | "Err" => TokenType::Constant,
                _ if word.starts_with(char::is_uppercase) => TokenType::Type,
                _ => TokenType::Normal,
            },
            "vim" => match word {
                "if" | "else" | "elseif" | "endif" | "while" | "endwhile" | "for" | "endfor"
                | "function" | "endfunction" | "return" | "let" | "unlet" | "set" | "call"
                | "execute" | "source" | "autocmd" | "augroup" | "map" | "nmap" | "imap"
                | "vmap" | "noremap" | "nnoremap" | "inoremap" | "vnoremap" | "command"
                | "filetype" | "syntax" | "highlight" | "echo" | "echom" | "echoerr" | "try"
                | "catch" | "finally" | "endtry" | "throw" | "break" | "continue" => {
                    TokenType::Keyword
                }
                _ if word.starts_with("g:")
                    || word.starts_with("s:")
                    || word.starts_with("l:")
                    || word.starts_with("b:")
                    || word.starts_with("v:") =>
                {
                    TokenType::Special
                }
                _ => TokenType::Normal,
            },
            "c" | "cpp" => match word {
                "if" | "else" | "switch" | "case" | "default" | "while" | "do" | "for"
                | "break" | "continue" | "return" | "goto" | "struct" | "union" | "enum"
                | "typedef" | "sizeof" | "static" | "extern" | "const" | "volatile"
                | "register" | "auto" | "inline" | "restrict" => TokenType::Keyword,
                "class" | "public" | "private" | "protected" | "virtual" | "override" | "final"
                | "template" | "typename" | "namespace" | "using" | "new" | "delete" | "try"
                | "catch" | "throw" | "noexcept" | "constexpr" | "nullptr"
                    if filetype == "cpp" =>
                {
                    TokenType::Keyword
                }
                "void" | "int" | "char" | "short" | "long" | "float" | "double" | "signed"
                | "unsigned" | "bool" | "size_t" | "uint8_t" | "uint16_t" | "uint32_t"
                | "uint64_t" | "int8_t" | "int16_t" | "int32_t" | "int64_t" => TokenType::Type,
                "true" | "false" | "NULL" | "TRUE" | "FALSE" => TokenType::Constant,
                _ if word.chars().all(|c| c.is_uppercase() || c == '_') => TokenType::Constant,
                _ => TokenType::Normal,
            },
            "python" => match word {
                "if" | "elif" | "else" | "for" | "while" | "break" | "continue" | "return"
                | "def" | "class" | "import" | "from" | "as" | "try" | "except" | "finally"
                | "raise" | "with" | "assert" | "pass" | "lambda" | "yield" | "global"
                | "nonlocal" | "and" | "or" | "not" | "in" | "is" | "del" | "async" | "await" => {
                    TokenType::Keyword
                }
                "int" | "float" | "str" | "bool" | "list" | "dict" | "tuple" | "set" | "None"
                | "True" | "False" => TokenType::Type,
                _ => TokenType::Normal,
            },
            _ => TokenType::Normal,
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}
