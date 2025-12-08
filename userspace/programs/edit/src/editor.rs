//! Main editor implementation

use std::io;

use crate::buffer::Buffer;
use crate::input::{InputReader, Key};
use crate::mode::{Mode, Operator};
use crate::render::Renderer;
use crate::terminal::Terminal;
use crate::vimscript::VimScript;

/// The main editor structure
pub struct Editor {
    /// Terminal handler
    terminal: Terminal,
    /// Text buffers
    buffers: Vec<Buffer>,
    /// Current buffer index
    current_buffer: usize,
    /// Current mode
    mode: Mode,
    /// Pending operator
    operator: Operator,
    /// Operator count
    count: Option<usize>,
    /// Cursor line (0-indexed)
    cursor_line: usize,
    /// Cursor column (0-indexed)
    cursor_col: usize,
    /// Desired column (for vertical movement)
    desired_col: usize,
    /// Scroll row offset
    scroll_row: usize,
    /// Scroll column offset
    scroll_col: usize,
    /// Visual mode start position
    visual_start: Option<(usize, usize)>,
    /// Command line input
    command_line: String,
    /// Search pattern
    search_pattern: String,
    /// Search direction (true = forward)
    search_forward: bool,
    /// Status message
    message: String,
    /// Message is an error
    message_is_error: bool,
    /// Renderer
    renderer: Renderer,
    /// Input reader
    input_reader: InputReader,
    /// Vim Script interpreter
    vimscript: VimScript,
    /// Yank register
    yank_register: String,
    /// Yank is linewise
    yank_linewise: bool,
    /// Last search pattern
    last_search: String,
    /// Should quit
    should_quit: bool,
    /// Pending keys for mapping
    pending_keys: String,
}

impl Editor {
    /// Create a new editor instance
    pub fn new() -> io::Result<Self> {
        let terminal = Terminal::new()?;
        
        Ok(Editor {
            terminal,
            buffers: Vec::new(),
            current_buffer: 0,
            mode: Mode::Normal,
            operator: Operator::None,
            count: None,
            cursor_line: 0,
            cursor_col: 0,
            desired_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            visual_start: None,
            command_line: String::new(),
            search_pattern: String::new(),
            search_forward: true,
            message: String::new(),
            message_is_error: false,
            renderer: Renderer::new(),
            input_reader: InputReader::new(),
            vimscript: VimScript::new(),
            yank_register: String::new(),
            yank_linewise: false,
            last_search: String::new(),
            should_quit: false,
            pending_keys: String::new(),
        })
    }
    
    /// Create a new empty buffer
    pub fn new_buffer(&mut self) {
        self.buffers.push(Buffer::new());
        self.current_buffer = self.buffers.len() - 1;
    }
    
    /// Open a file into a new buffer
    pub fn open_file(&mut self, path: &str) -> io::Result<()> {
        let buffer = Buffer::from_file(path)?;
        self.buffers.push(buffer);
        self.current_buffer = self.buffers.len() - 1;
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_row = 0;
        self.scroll_col = 0;
        
        // Trigger BufRead autocmd
        self.vimscript.trigger_autocmd("BufRead", path);
        self.vimscript.trigger_autocmd("BufReadPost", path);
        
        Ok(())
    }
    
    /// Get current buffer
    fn buffer(&self) -> &Buffer {
        &self.buffers[self.current_buffer]
    }
    
    /// Get current buffer mutably
    fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.current_buffer]
    }
    
    /// Source a Vim Script file
    pub fn source_script(&mut self, path: &str) -> io::Result<()> {
        self.vimscript.source_file(path)?;
        Ok(())
    }
    
    /// Execute a command
    pub fn execute_command(&mut self, cmd: &str) -> io::Result<()> {
        self.run_ex_command(cmd)
    }
    
    /// Go to a specific line
    pub fn goto_line(&mut self, line: usize) {
        let line = line.saturating_sub(1); // Convert to 0-indexed
        let max_line = self.buffer().line_count().saturating_sub(1);
        self.cursor_line = line.min(max_line);
        self.cursor_col = 0;
        self.ensure_cursor_visible();
    }
    
    /// Run the main editor loop
    pub fn run(&mut self) -> io::Result<()> {
        self.terminal.enable_raw_mode()?;
        self.terminal.enter_alt_screen();
        
        // Initial render
        self.render();
        
        // Main loop
        while !self.should_quit {
            self.terminal.update_size();
            
            // Read input
            let mut buf = [0u8; 32];
            match self.terminal.read_bytes(&mut buf) {
                Ok(0) => continue,
                Ok(n) => {
                    if let Some(key) = self.input_reader.parse_key(&buf[..n]) {
                        self.handle_key(key)?;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e),
            }
            
            self.render();
        }
        
        self.terminal.leave_alt_screen();
        let _ = self.terminal.flush();
        self.terminal.disable_raw_mode()?;
        
        Ok(())
    }
    
    /// Render the screen
    fn render(&mut self) {
        let buffer = &self.buffers[self.current_buffer];
        
        self.renderer.render(
            &mut self.terminal,
            buffer,
            self.mode,
            self.cursor_line,
            self.cursor_col,
            self.scroll_row,
            self.scroll_col,
            self.visual_start,
            &self.message,
            &self.command_line,
        );
    }
    
    /// Handle a key event
    fn handle_key(&mut self, key: Key) -> io::Result<()> {
        // Clear message on any key press
        if !self.message.is_empty() && !matches!(self.mode, Mode::Command | Mode::Search) {
            self.message.clear();
        }
        
        match self.mode {
            Mode::Normal => self.handle_normal_key(key),
            Mode::Insert => self.handle_insert_key(key),
            Mode::Visual | Mode::VisualLine => self.handle_visual_key(key),
            Mode::Command => self.handle_command_key(key),
            Mode::Search => self.handle_search_key(key),
            Mode::Replace => self.handle_replace_key(key),
            _ => Ok(()),
        }
    }
    
    /// Handle key in normal mode
    fn handle_normal_key(&mut self, key: Key) -> io::Result<()> {
        match key {
            // Mode switching
            Key::Char('i') => {
                self.enter_insert_mode();
            }
            Key::Char('I') => {
                self.cursor_col = self.first_non_blank();
                self.enter_insert_mode();
            }
            Key::Char('a') => {
                self.cursor_col = (self.cursor_col + 1).min(self.current_line_len());
                self.enter_insert_mode();
            }
            Key::Char('A') => {
                self.cursor_col = self.current_line_len();
                self.enter_insert_mode();
            }
            Key::Char('o') => {
                self.open_line_below();
            }
            Key::Char('O') => {
                self.open_line_above();
            }
            Key::Char('v') => {
                self.mode = Mode::Visual;
                self.visual_start = Some((self.cursor_line, self.cursor_col));
            }
            Key::Char('V') => {
                self.mode = Mode::VisualLine;
                self.visual_start = Some((self.cursor_line, 0));
            }
            Key::Char('R') => {
                self.mode = Mode::Replace;
            }
            Key::Char(':') => {
                self.mode = Mode::Command;
                self.command_line.clear();
            }
            Key::Char('/') => {
                self.mode = Mode::Search;
                self.search_forward = true;
                self.command_line.clear();
            }
            Key::Char('?') => {
                self.mode = Mode::Search;
                self.search_forward = false;
                self.command_line.clear();
            }
            
            // Movement
            Key::Char('h') | Key::Left => self.move_left(),
            Key::Char('j') | Key::Down => self.move_down(),
            Key::Char('k') | Key::Up => self.move_up(),
            Key::Char('l') | Key::Right => self.move_right(),
            Key::Char('w') => self.move_word_forward(),
            Key::Char('b') => self.move_word_backward(),
            Key::Char('e') => self.move_word_end(),
            Key::Char('0') | Key::Home => self.cursor_col = 0,
            Key::Char('^') => self.cursor_col = self.first_non_blank(),
            Key::Char('$') | Key::End => {
                self.cursor_col = self.current_line_len().saturating_sub(1).max(0);
            }
            Key::Char('G') => {
                self.cursor_line = self.buffer().line_count().saturating_sub(1);
                self.cursor_col = 0;
            }
            Key::Char('g') => {
                // Wait for next key
                // Simplified: gg goes to start
                self.cursor_line = 0;
                self.cursor_col = 0;
            }
            Key::Ctrl('f') | Key::PageDown => self.page_down(),
            Key::Ctrl('b') | Key::PageUp => self.page_up(),
            Key::Ctrl('d') => self.half_page_down(),
            Key::Ctrl('u') => self.half_page_up(),
            
            // Editing
            Key::Char('x') => self.delete_char_under_cursor(),
            Key::Char('X') => self.delete_char_before_cursor(),
            Key::Char('r') => {
                // Replace single character - wait for next key
                // Simplified for now
            }
            Key::Char('d') => {
                self.operator = Operator::Delete;
                // Wait for motion
            }
            Key::Char('y') => {
                self.operator = Operator::Yank;
            }
            Key::Char('c') => {
                self.operator = Operator::Change;
            }
            Key::Char('p') => self.paste_after(),
            Key::Char('P') => self.paste_before(),
            Key::Char('u') => self.undo(),
            Key::Ctrl('r') => self.redo(),
            Key::Char('J') => self.join_lines(),
            
            // Search
            Key::Char('n') => self.search_next(),
            Key::Char('N') => self.search_prev(),
            Key::Char('*') => self.search_word_under_cursor(true),
            Key::Char('#') => self.search_word_under_cursor(false),
            
            // Other
            Key::Escape => {
                self.operator = Operator::None;
                self.count = None;
            }
            Key::Ctrl('l') => {
                // Redraw screen
            }
            
            // Number for count
            Key::Char(c) if c.is_ascii_digit() && (c != '0' || self.count.is_some()) => {
                let digit = c.to_digit(10).unwrap() as usize;
                self.count = Some(self.count.unwrap_or(0) * 10 + digit);
            }
            
            _ => {}
        }
        
        // Handle operator + motion
        if self.operator != Operator::None {
            self.handle_operator_motion(key)?;
        }
        
        self.ensure_cursor_visible();
        Ok(())
    }
    
    /// Handle operator pending motion
    fn handle_operator_motion(&mut self, key: Key) -> io::Result<()> {
        let op = self.operator;
        
        // Check for doubled operator (dd, yy, cc)
        let doubled = match (op, &key) {
            (Operator::Delete, Key::Char('d')) => true,
            (Operator::Yank, Key::Char('y')) => true,
            (Operator::Change, Key::Char('c')) => true,
            _ => false,
        };
        
        if doubled {
            // Operate on whole line
            let count = self.count.unwrap_or(1);
            match op {
                Operator::Delete => {
                    self.delete_lines(self.cursor_line, count);
                }
                Operator::Yank => {
                    self.yank_lines(self.cursor_line, count);
                }
                Operator::Change => {
                    self.delete_lines(self.cursor_line, count);
                    self.enter_insert_mode();
                }
                _ => {}
            }
            self.operator = Operator::None;
            self.count = None;
        }
        
        Ok(())
    }
    
    /// Handle key in insert mode
    fn handle_insert_key(&mut self, key: Key) -> io::Result<()> {
        match key {
            Key::Escape => {
                self.leave_insert_mode();
            }
            Key::Enter => {
                self.insert_newline();
            }
            Key::Backspace => {
                self.backspace();
            }
            Key::Delete => {
                self.delete_char_under_cursor();
            }
            Key::Tab => {
                if self.vimscript.options.expandtab {
                    let spaces = self.vimscript.options.tabstop;
                    for _ in 0..spaces {
                        self.insert_char(' ');
                    }
                } else {
                    self.insert_char('\t');
                }
            }
            Key::Left => self.move_left(),
            Key::Right => self.move_right(),
            Key::Up => self.move_up(),
            Key::Down => self.move_down(),
            Key::Home => self.cursor_col = 0,
            Key::End => self.cursor_col = self.current_line_len(),
            Key::Char(c) => {
                self.insert_char(c);
            }
            Key::Ctrl('h') => self.backspace(),
            Key::Ctrl('w') => self.delete_word_backward(),
            Key::Ctrl('u') => self.delete_to_line_start(),
            _ => {}
        }
        
        self.ensure_cursor_visible();
        Ok(())
    }
    
    /// Handle key in visual mode
    fn handle_visual_key(&mut self, key: Key) -> io::Result<()> {
        match key {
            Key::Escape | Key::Char('v') | Key::Char('V') => {
                self.mode = Mode::Normal;
                self.visual_start = None;
            }
            
            // Movement (same as normal mode)
            Key::Char('h') | Key::Left => self.move_left(),
            Key::Char('j') | Key::Down => self.move_down(),
            Key::Char('k') | Key::Up => self.move_up(),
            Key::Char('l') | Key::Right => self.move_right(),
            Key::Char('w') => self.move_word_forward(),
            Key::Char('b') => self.move_word_backward(),
            Key::Char('0') | Key::Home => self.cursor_col = 0,
            Key::Char('$') | Key::End => {
                self.cursor_col = self.current_line_len().saturating_sub(1);
            }
            Key::Char('G') => {
                self.cursor_line = self.buffer().line_count().saturating_sub(1);
            }
            Key::Char('g') => {
                self.cursor_line = 0;
            }
            
            // Operations on selection
            Key::Char('d') | Key::Char('x') => {
                self.delete_selection();
                self.mode = Mode::Normal;
                self.visual_start = None;
            }
            Key::Char('y') => {
                self.yank_selection();
                self.mode = Mode::Normal;
                self.visual_start = None;
            }
            Key::Char('c') => {
                self.delete_selection();
                self.enter_insert_mode();
                self.visual_start = None;
            }
            
            _ => {}
        }
        
        self.ensure_cursor_visible();
        Ok(())
    }
    
    /// Handle key in command mode
    fn handle_command_key(&mut self, key: Key) -> io::Result<()> {
        match key {
            Key::Escape => {
                self.mode = Mode::Normal;
                self.command_line.clear();
            }
            Key::Enter => {
                let cmd = self.command_line.clone();
                self.command_line.clear();
                self.mode = Mode::Normal;
                self.run_ex_command(&cmd)?;
            }
            Key::Backspace => {
                self.command_line.pop();
                if self.command_line.is_empty() {
                    self.mode = Mode::Normal;
                }
            }
            Key::Char(c) => {
                self.command_line.push(c);
            }
            _ => {}
        }
        Ok(())
    }
    
    /// Handle key in search mode
    fn handle_search_key(&mut self, key: Key) -> io::Result<()> {
        match key {
            Key::Escape => {
                self.mode = Mode::Normal;
                self.command_line.clear();
            }
            Key::Enter => {
                self.last_search = self.command_line.clone();
                self.command_line.clear();
                self.mode = Mode::Normal;
                self.search_next();
            }
            Key::Backspace => {
                self.command_line.pop();
                if self.command_line.is_empty() {
                    self.mode = Mode::Normal;
                }
            }
            Key::Char(c) => {
                self.command_line.push(c);
                // Incremental search
                if self.vimscript.options.incsearch {
                    self.incremental_search();
                }
            }
            _ => {}
        }
        Ok(())
    }
    
    /// Handle key in replace mode
    fn handle_replace_key(&mut self, key: Key) -> io::Result<()> {
        match key {
            Key::Escape => {
                self.mode = Mode::Normal;
            }
            Key::Char(c) => {
                // Replace character under cursor
                let line = self.cursor_line;
                let col = self.cursor_col;
                if col < self.current_line_len() {
                    self.buffer_mut().delete_char(line, col);
                }
                self.buffer_mut().insert_char(line, col, c);
                self.move_right();
            }
            Key::Backspace => {
                self.move_left();
            }
            _ => {}
        }
        Ok(())
    }
    
    // ---- Movement helpers ----
    
    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            self.desired_col = self.cursor_col;
        }
    }
    
    fn move_right(&mut self) {
        let max_col = if self.mode.is_editing() {
            self.current_line_len()
        } else {
            self.current_line_len().saturating_sub(1)
        };
        
        if self.cursor_col < max_col {
            self.cursor_col += 1;
            self.desired_col = self.cursor_col;
        }
    }
    
    fn move_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.adjust_cursor_col();
        }
    }
    
    fn move_down(&mut self) {
        if self.cursor_line < self.buffer().line_count() - 1 {
            self.cursor_line += 1;
            self.adjust_cursor_col();
        }
    }
    
    fn adjust_cursor_col(&mut self) {
        let line_len = self.current_line_len();
        let max_col = if self.mode.is_editing() {
            line_len
        } else {
            line_len.saturating_sub(1).max(0)
        };
        self.cursor_col = self.desired_col.min(max_col);
    }
    
    fn move_word_forward(&mut self) {
        let line = self.buffer().get_line(self.cursor_line)
            .map(|l| l.content.clone())
            .unwrap_or_default();
        let chars: Vec<char> = line.chars().collect();
        
        let mut col = self.cursor_col;
        
        // Skip current word
        while col < chars.len() && !chars[col].is_whitespace() {
            col += 1;
        }
        // Skip whitespace
        while col < chars.len() && chars[col].is_whitespace() {
            col += 1;
        }
        
        if col >= chars.len() && self.cursor_line < self.buffer().line_count() - 1 {
            self.cursor_line += 1;
            self.cursor_col = self.first_non_blank();
        } else {
            self.cursor_col = col;
        }
        self.desired_col = self.cursor_col;
    }
    
    fn move_word_backward(&mut self) {
        if self.cursor_col == 0 && self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.current_line_len().saturating_sub(1);
        }
        
        let line = self.buffer().get_line(self.cursor_line)
            .map(|l| l.content.clone())
            .unwrap_or_default();
        let chars: Vec<char> = line.chars().collect();
        
        let mut col = self.cursor_col.saturating_sub(1);
        
        // Skip whitespace
        while col > 0 && chars.get(col).map(|c| c.is_whitespace()).unwrap_or(false) {
            col -= 1;
        }
        // Skip to start of word
        while col > 0 && !chars.get(col - 1).map(|c| c.is_whitespace()).unwrap_or(true) {
            col -= 1;
        }
        
        self.cursor_col = col;
        self.desired_col = self.cursor_col;
    }
    
    fn move_word_end(&mut self) {
        let line = self.buffer().get_line(self.cursor_line)
            .map(|l| l.content.clone())
            .unwrap_or_default();
        let chars: Vec<char> = line.chars().collect();
        
        let mut col = self.cursor_col + 1;
        
        // Skip whitespace
        while col < chars.len() && chars[col].is_whitespace() {
            col += 1;
        }
        // Skip to end of word
        while col < chars.len() - 1 && !chars[col + 1].is_whitespace() {
            col += 1;
        }
        
        self.cursor_col = col.min(chars.len().saturating_sub(1));
        self.desired_col = self.cursor_col;
    }
    
    fn page_down(&mut self) {
        let page_size = self.terminal.get_size().rows.saturating_sub(2);
        self.cursor_line = (self.cursor_line + page_size).min(self.buffer().line_count() - 1);
        self.adjust_cursor_col();
    }
    
    fn page_up(&mut self) {
        let page_size = self.terminal.get_size().rows.saturating_sub(2);
        self.cursor_line = self.cursor_line.saturating_sub(page_size);
        self.adjust_cursor_col();
    }
    
    fn half_page_down(&mut self) {
        let half_page = self.terminal.get_size().rows.saturating_sub(2) / 2;
        self.cursor_line = (self.cursor_line + half_page).min(self.buffer().line_count() - 1);
        self.adjust_cursor_col();
    }
    
    fn half_page_up(&mut self) {
        let half_page = self.terminal.get_size().rows.saturating_sub(2) / 2;
        self.cursor_line = self.cursor_line.saturating_sub(half_page);
        self.adjust_cursor_col();
    }
    
    fn current_line_len(&self) -> usize {
        self.buffer().get_line(self.cursor_line)
            .map(|l| l.content.chars().count())
            .unwrap_or(0)
    }
    
    fn first_non_blank(&self) -> usize {
        let line = self.buffer().get_line(self.cursor_line)
            .map(|l| l.content.clone())
            .unwrap_or_default();
        
        line.chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0)
    }
    
    fn ensure_cursor_visible(&mut self) {
        let size = self.terminal.get_size();
        let text_rows = self.renderer.text_rows(size.rows);
        let scrolloff = self.vimscript.options.scrolloff;
        
        // Vertical scroll
        if self.cursor_line < self.scroll_row + scrolloff {
            self.scroll_row = self.cursor_line.saturating_sub(scrolloff);
        }
        if self.cursor_line >= self.scroll_row + text_rows - scrolloff {
            self.scroll_row = self.cursor_line.saturating_sub(text_rows - scrolloff - 1);
        }
        
        // Horizontal scroll
        let text_cols = size.cols.saturating_sub(self.renderer.text_col_offset());
        if self.cursor_col < self.scroll_col {
            self.scroll_col = self.cursor_col;
        }
        if self.cursor_col >= self.scroll_col + text_cols {
            self.scroll_col = self.cursor_col.saturating_sub(text_cols - 1);
        }
    }
    
    // ---- Editing helpers ----
    
    fn enter_insert_mode(&mut self) {
        let cursor_line = self.cursor_line;
        let cursor_col = self.cursor_col;
        self.buffer_mut().begin_group(cursor_line, cursor_col);
        self.mode = Mode::Insert;
    }
    
    fn leave_insert_mode(&mut self) {
        let cursor_line = self.cursor_line;
        let cursor_col = self.cursor_col;
        self.buffer_mut().commit_group(cursor_line, cursor_col);
        self.mode = Mode::Normal;
        // Adjust cursor to be within line bounds
        if self.cursor_col > 0 {
            self.cursor_col = self.cursor_col.min(self.current_line_len().saturating_sub(1));
        }
    }
    
    fn insert_char(&mut self, c: char) {
        let cursor_line = self.cursor_line;
        let cursor_col = self.cursor_col;
        self.buffer_mut().insert_char(cursor_line, cursor_col, c);
        self.cursor_col += 1;
    }
    
    fn insert_newline(&mut self) {
        let autoindent = self.vimscript.options.autoindent;
        let indent = if autoindent {
            self.get_current_indent()
        } else {
            String::new()
        };
        
        let cursor_line = self.cursor_line;
        let cursor_col = self.cursor_col;
        self.buffer_mut().split_line(cursor_line, cursor_col);
        self.cursor_line += 1;
        self.cursor_col = 0;
        
        if !indent.is_empty() {
            let cursor_line = self.cursor_line;
            self.buffer_mut().insert_text(cursor_line, 0, &indent);
            self.cursor_col = indent.len();
        }
    }
    
    fn get_current_indent(&self) -> String {
        let line = self.buffer().get_line(self.cursor_line)
            .map(|l| l.content.clone())
            .unwrap_or_default();
        
        line.chars()
            .take_while(|c| c.is_whitespace())
            .collect()
    }
    
    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            let cursor_line = self.cursor_line;
            let cursor_col = self.cursor_col;
            self.buffer_mut().delete_char(cursor_line, cursor_col);
        } else if self.cursor_line > 0 {
            // Join with previous line
            let prev_line_len = self.buffer().get_line(self.cursor_line - 1)
                .map(|l| l.content.chars().count())
                .unwrap_or(0);
            
            self.cursor_line -= 1;
            self.cursor_col = prev_line_len;
            let cursor_line = self.cursor_line;
            self.buffer_mut().join_lines(cursor_line);
        }
    }
    
    fn delete_char_under_cursor(&mut self) {
        if self.current_line_len() > 0 {
            let cursor_line = self.cursor_line;
            let cursor_col = self.cursor_col;
            self.buffer_mut().delete_char(cursor_line, cursor_col);
            // Adjust cursor if at end of line
            let line_len = self.current_line_len();
            if self.cursor_col >= line_len && line_len > 0 {
                self.cursor_col = line_len - 1;
            }
        }
    }
    
    fn delete_char_before_cursor(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            let cursor_line = self.cursor_line;
            let cursor_col = self.cursor_col;
            self.buffer_mut().delete_char(cursor_line, cursor_col);
        }
    }
    
    fn delete_word_backward(&mut self) {
        let start_col = self.cursor_col;
        self.move_word_backward();
        let end_col = self.cursor_col;
        
        if end_col < start_col {
            let cursor_line = self.cursor_line;
            self.buffer_mut().delete_range(cursor_line, end_col, start_col);
        }
    }
    
    fn delete_to_line_start(&mut self) {
        if self.cursor_col > 0 {
            let cursor_line = self.cursor_line;
            let cursor_col = self.cursor_col;
            self.buffer_mut().delete_range(cursor_line, 0, cursor_col);
            self.cursor_col = 0;
        }
    }
    
    fn open_line_below(&mut self) {
        let indent = self.get_current_indent();
        let cursor_line = self.cursor_line;
        self.buffer_mut().insert_line(cursor_line + 1, indent.clone());
        self.cursor_line += 1;
        self.cursor_col = indent.len();
        self.enter_insert_mode();
    }
    
    fn open_line_above(&mut self) {
        let indent = self.get_current_indent();
        let cursor_line = self.cursor_line;
        self.buffer_mut().insert_line(cursor_line, indent.clone());
        self.cursor_col = indent.len();
        self.enter_insert_mode();
    }
    
    fn delete_lines(&mut self, start: usize, count: usize) {
        let mut yanked = Vec::new();
        
        for _ in 0..count {
            if start < self.buffer().line_count() {
                if let Some(content) = self.buffer_mut().delete_line(start) {
                    yanked.push(content);
                }
            }
        }
        
        self.yank_register = yanked.join("\n");
        self.yank_linewise = true;
        
        // Adjust cursor
        if self.cursor_line >= self.buffer().line_count() {
            self.cursor_line = self.buffer().line_count().saturating_sub(1);
        }
        self.cursor_col = self.first_non_blank();
    }
    
    fn yank_lines(&mut self, start: usize, count: usize) {
        let mut yanked = Vec::new();
        
        for i in 0..count {
            if let Some(line) = self.buffer().get_line(start + i) {
                yanked.push(line.content.clone());
            }
        }
        
        self.yank_register = yanked.join("\n");
        self.yank_linewise = true;
        
        self.message = format!("{} line(s) yanked", count);
    }
    
    fn delete_selection(&mut self) {
        if let Some((start_line, start_col)) = self.visual_start {
            let end_line = self.cursor_line;
            let end_col = self.cursor_col;
            
            // Normalize selection
            let (begin_line, begin_col, end_line, end_col) = if start_line < end_line
                || (start_line == end_line && start_col < end_col)
            {
                (start_line, start_col, end_line, end_col)
            } else {
                (end_line, end_col, start_line, start_col)
            };
            
            if self.mode == Mode::VisualLine {
                self.delete_lines(begin_line, end_line - begin_line + 1);
            } else {
                // Character-wise deletion (simplified)
                if begin_line == end_line {
                    let deleted = self.buffer_mut().delete_range(begin_line, begin_col, end_col + 1);
                    self.yank_register = deleted;
                    self.yank_linewise = false;
                } else {
                    // Multi-line selection - delete lines
                    self.delete_lines(begin_line, end_line - begin_line + 1);
                }
            }
            
            self.cursor_line = begin_line;
            self.cursor_col = begin_col;
        }
    }
    
    fn yank_selection(&mut self) {
        if let Some((start_line, start_col)) = self.visual_start {
            let end_line = self.cursor_line;
            
            // Normalize
            let (begin_line, end_line) = if start_line < end_line {
                (start_line, end_line)
            } else {
                (end_line, start_line)
            };
            
            if self.mode == Mode::VisualLine {
                self.yank_lines(begin_line, end_line - begin_line + 1);
            } else {
                // Character-wise yank (simplified)
                if begin_line == end_line {
                    let line = self.buffer().get_line(begin_line)
                        .map(|l| l.content.clone())
                        .unwrap_or_default();
                    
                    let begin_col = start_col.min(self.cursor_col);
                    let end_col = start_col.max(self.cursor_col);
                    
                    let chars: Vec<char> = line.chars().collect();
                    self.yank_register = chars[begin_col..=end_col.min(chars.len() - 1)].iter().collect();
                    self.yank_linewise = false;
                } else {
                    self.yank_lines(begin_line, end_line - begin_line + 1);
                }
            }
        }
    }
    
    fn paste_after(&mut self) {
        if self.yank_register.is_empty() {
            return;
        }
        
        if self.yank_linewise {
            let lines: Vec<String> = self.yank_register.split('\n').map(|s| s.to_string()).collect();
            for line in lines {
                let cursor_line = self.cursor_line;
                self.buffer_mut().insert_line(cursor_line + 1, line);
                self.cursor_line += 1;
            }
            self.cursor_col = self.first_non_blank();
        } else {
            let cursor_line = self.cursor_line;
            let cursor_col = self.cursor_col;
            let text = self.yank_register.clone();
            self.buffer_mut().insert_text(cursor_line, cursor_col + 1, &text);
            self.cursor_col += text.chars().count();
        }
    }
    
    fn paste_before(&mut self) {
        if self.yank_register.is_empty() {
            return;
        }
        
        if self.yank_linewise {
            let lines: Vec<(usize, String)> = self.yank_register.split('\n')
                .enumerate()
                .map(|(i, s)| (i, s.to_string()))
                .collect();
            for (i, line) in lines {
                let cursor_line = self.cursor_line;
                self.buffer_mut().insert_line(cursor_line + i, line);
            }
            self.cursor_col = self.first_non_blank();
        } else {
            let cursor_line = self.cursor_line;
            let cursor_col = self.cursor_col;
            let text = self.yank_register.clone();
            self.buffer_mut().insert_text(cursor_line, cursor_col, &text);
        }
    }
    
    fn join_lines(&mut self) {
        let cursor_line = self.cursor_line;
        if cursor_line < self.buffer().line_count() - 1 {
            self.buffer_mut().join_lines(cursor_line);
        }
    }
    
    fn undo(&mut self) {
        if let Some((line, col)) = self.buffer_mut().undo() {
            self.cursor_line = line.min(self.buffer().line_count().saturating_sub(1));
            self.cursor_col = col;
            self.message = "Undo".to_string();
        } else {
            self.message = "Already at oldest change".to_string();
        }
    }
    
    fn redo(&mut self) {
        if let Some((line, col)) = self.buffer_mut().redo() {
            self.cursor_line = line.min(self.buffer().line_count().saturating_sub(1));
            self.cursor_col = col;
            self.message = "Redo".to_string();
        } else {
            self.message = "Already at newest change".to_string();
        }
    }
    
    // ---- Search helpers ----
    
    fn search_next(&mut self) {
        if self.last_search.is_empty() {
            self.message = "No previous search pattern".to_string();
            return;
        }
        
        let forward = self.search_forward;
        if let Some((line, col)) = self.buffer().search(
            &self.last_search,
            self.cursor_line,
            self.cursor_col,
            forward,
        ) {
            self.cursor_line = line;
            self.cursor_col = col;
            self.message = format!("/{}", self.last_search);
        } else {
            self.message = format!("Pattern not found: {}", self.last_search);
        }
    }
    
    fn search_prev(&mut self) {
        if self.last_search.is_empty() {
            self.message = "No previous search pattern".to_string();
            return;
        }
        
        let forward = !self.search_forward;
        if let Some((line, col)) = self.buffer().search(
            &self.last_search,
            self.cursor_line,
            self.cursor_col,
            forward,
        ) {
            self.cursor_line = line;
            self.cursor_col = col;
            self.message = format!("?{}", self.last_search);
        } else {
            self.message = format!("Pattern not found: {}", self.last_search);
        }
    }
    
    fn incremental_search(&mut self) {
        if !self.command_line.is_empty() {
            if let Some((line, col)) = self.buffer().search(
                &self.command_line,
                self.cursor_line,
                self.cursor_col,
                self.search_forward,
            ) {
                self.cursor_line = line;
                self.cursor_col = col;
                self.ensure_cursor_visible();
            }
        }
    }
    
    fn search_word_under_cursor(&mut self, forward: bool) {
        let word = self.get_word_under_cursor();
        if !word.is_empty() {
            self.last_search = word;
            self.search_forward = forward;
            self.search_next();
        }
    }
    
    fn get_word_under_cursor(&self) -> String {
        let line = self.buffer().get_line(self.cursor_line)
            .map(|l| l.content.clone())
            .unwrap_or_default();
        
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() || self.cursor_col >= chars.len() {
            return String::new();
        }
        
        let mut start = self.cursor_col;
        let mut end = self.cursor_col;
        
        // Find start of word
        while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }
        
        // Find end of word
        while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        
        chars[start..end].iter().collect()
    }
    
    // ---- Ex commands ----
    
    fn run_ex_command(&mut self, cmd: &str) -> io::Result<()> {
        let cmd = cmd.trim();
        
        // Parse command and arguments
        let (cmd_name, args) = match cmd.find(|c: char| c.is_whitespace()) {
            Some(pos) => (&cmd[..pos], cmd[pos..].trim()),
            None => (cmd, ""),
        };
        
        match cmd_name {
            "w" | "write" => self.cmd_write(args)?,
            "q" | "quit" => self.cmd_quit(false)?,
            "q!" | "quit!" => self.cmd_quit(true)?,
            "wq" | "x" | "exit" => {
                self.cmd_write(args)?;
                self.cmd_quit(false)?;
            }
            "e" | "edit" => self.cmd_edit(args)?,
            "set" => self.cmd_set(args)?,
            "source" | "so" => self.cmd_source(args)?,
            "new" => self.cmd_new()?,
            "bn" | "bnext" => self.cmd_next_buffer()?,
            "bp" | "bprev" => self.cmd_prev_buffer()?,
            "bd" | "bdelete" => self.cmd_delete_buffer()?,
            "ls" | "buffers" => self.cmd_list_buffers()?,
            "help" | "h" => self.cmd_help(args)?,
            "version" | "ver" => {
                self.message = "edit 0.1.0 - NexaOS Vim-like Editor".to_string();
            }
            "" => {}
            _ => {
                // Try to run as Vim Script
                if let Err(e) = self.vimscript.execute_line(cmd) {
                    self.message = format!("E492: Not an editor command: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    fn cmd_write(&mut self, args: &str) -> io::Result<()> {
        if !args.is_empty() {
            self.buffer_mut().save_as(args)?;
            self.message = format!("\"{}\" written", args);
        } else if self.buffer().path.is_some() {
            self.buffer_mut().save()?;
            let name = self.buffer().name.clone();
            self.message = format!("\"{}\" written", name);
        } else {
            self.message = "E32: No file name".to_string();
            return Ok(());
        }
        Ok(())
    }
    
    fn cmd_quit(&mut self, force: bool) -> io::Result<()> {
        if !force && self.buffer().modified {
            self.message = "E37: No write since last change (add ! to override)".to_string();
            return Ok(());
        }
        
        if self.buffers.len() > 1 {
            self.buffers.remove(self.current_buffer);
            if self.current_buffer >= self.buffers.len() {
                self.current_buffer = self.buffers.len() - 1;
            }
            self.cursor_line = 0;
            self.cursor_col = 0;
        } else {
            self.should_quit = true;
        }
        Ok(())
    }
    
    fn cmd_edit(&mut self, args: &str) -> io::Result<()> {
        if args.is_empty() {
            self.message = "E32: No file name".to_string();
            return Ok(());
        }
        self.open_file(args)
    }
    
    fn cmd_set(&mut self, args: &str) -> io::Result<()> {
        if args.is_empty() {
            // Show all options
            self.message = format!(
                "tabstop={} shiftwidth={} {}expandtab {}number",
                self.vimscript.options.tabstop,
                self.vimscript.options.shiftwidth,
                if self.vimscript.options.expandtab { "" } else { "no" },
                if self.vimscript.options.number { "" } else { "no" },
            );
        } else {
            // Apply option through Vim Script
            let _ = self.vimscript.execute_line(&format!("set {}", args));
            
            // Sync renderer options
            self.renderer.show_line_numbers = self.vimscript.options.number;
            self.renderer.tabstop = self.vimscript.options.tabstop;
        }
        Ok(())
    }
    
    fn cmd_source(&mut self, args: &str) -> io::Result<()> {
        if args.is_empty() {
            self.message = "E471: Argument required".to_string();
            return Ok(());
        }
        
        match self.vimscript.source_file(args) {
            Ok(_) => {
                self.message = format!("\"{}\" sourced", args);
                // Sync options
                self.renderer.show_line_numbers = self.vimscript.options.number;
                self.renderer.tabstop = self.vimscript.options.tabstop;
            }
            Err(e) => {
                self.message = format!("E484: Can't open file {}: {}", args, e);
            }
        }
        Ok(())
    }
    
    fn cmd_new(&mut self) -> io::Result<()> {
        self.new_buffer();
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.message = "New buffer".to_string();
        Ok(())
    }
    
    fn cmd_next_buffer(&mut self) -> io::Result<()> {
        if self.buffers.len() > 1 {
            self.current_buffer = (self.current_buffer + 1) % self.buffers.len();
            self.cursor_line = 0;
            self.cursor_col = 0;
        }
        Ok(())
    }
    
    fn cmd_prev_buffer(&mut self) -> io::Result<()> {
        if self.buffers.len() > 1 {
            self.current_buffer = if self.current_buffer == 0 {
                self.buffers.len() - 1
            } else {
                self.current_buffer - 1
            };
            self.cursor_line = 0;
            self.cursor_col = 0;
        }
        Ok(())
    }
    
    fn cmd_delete_buffer(&mut self) -> io::Result<()> {
        if self.buffer().modified {
            self.message = "E89: No write since last change for buffer".to_string();
            return Ok(());
        }
        
        if self.buffers.len() > 1 {
            self.buffers.remove(self.current_buffer);
            if self.current_buffer >= self.buffers.len() {
                self.current_buffer = self.buffers.len() - 1;
            }
            self.cursor_line = 0;
            self.cursor_col = 0;
        } else {
            self.buffers[0] = Buffer::new();
            self.cursor_line = 0;
            self.cursor_col = 0;
        }
        Ok(())
    }
    
    fn cmd_list_buffers(&mut self) -> io::Result<()> {
        let mut msg = String::new();
        for (i, buf) in self.buffers.iter().enumerate() {
            let current = if i == self.current_buffer { "%" } else { " " };
            let modified = if buf.modified { "+" } else { " " };
            msg.push_str(&format!("{}{}{} {}\n", i + 1, current, modified, buf.name));
        }
        self.message = msg;
        Ok(())
    }
    
    fn cmd_help(&mut self, _args: &str) -> io::Result<()> {
        self.message = "Type :q to quit, :w to save, :help <topic> for help".to_string();
        Ok(())
    }
}
