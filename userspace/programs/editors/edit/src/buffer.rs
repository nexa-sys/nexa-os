//! Text buffer implementation with undo/redo support

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;

/// A single line of text in the buffer
#[derive(Debug, Clone)]
pub struct Line {
    pub content: String,
}

impl Line {
    pub fn new() -> Self {
        Line {
            content: String::new(),
        }
    }

    pub fn from_string(s: String) -> Self {
        Line { content: s }
    }

    pub fn len(&self) -> usize {
        self.content.len()
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Get the display width of the line (tabs expanded)
    pub fn display_width(&self, tabstop: usize) -> usize {
        let mut width = 0;
        for ch in self.content.chars() {
            if ch == '\t' {
                width += tabstop - (width % tabstop);
            } else {
                width += 1;
            }
        }
        width
    }
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

/// An edit operation for undo/redo
#[derive(Debug, Clone)]
pub enum EditOp {
    /// Insert text at position
    Insert {
        line: usize,
        col: usize,
        text: String,
    },
    /// Delete text at position
    Delete {
        line: usize,
        col: usize,
        text: String,
    },
    /// Insert a new line
    InsertLine { line: usize, content: String },
    /// Delete a line
    DeleteLine { line: usize, content: String },
    /// Replace text
    Replace {
        line: usize,
        col: usize,
        old_text: String,
        new_text: String,
    },
    /// Join lines
    JoinLines { line: usize, col: usize },
    /// Split line
    SplitLine { line: usize, col: usize },
    /// Multiple operations grouped together
    Group(Vec<EditOp>),
}

/// Undo state
#[derive(Debug, Clone)]
struct UndoState {
    ops: Vec<EditOp>,
    cursor_line: usize,
    cursor_col: usize,
}

/// A text buffer representing a file
pub struct Buffer {
    /// Lines of text
    pub lines: Vec<Line>,
    /// File path (if any)
    pub path: Option<PathBuf>,
    /// Buffer name
    pub name: String,
    /// Whether the buffer has been modified
    pub modified: bool,
    /// Whether the buffer is read-only
    pub readonly: bool,
    /// Undo stack
    undo_stack: Vec<UndoState>,
    /// Redo stack
    redo_stack: Vec<UndoState>,
    /// Current edit group (for grouping operations)
    current_group: Vec<EditOp>,
    /// File type (for syntax highlighting)
    pub filetype: String,
    /// Line ending style
    pub line_ending: LineEnding,
}

/// Line ending style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Unix, // \n
    Dos,  // \r\n
    Mac,  // \r (classic Mac OS)
}

impl Default for LineEnding {
    fn default() -> Self {
        LineEnding::Unix
    }
}

impl Buffer {
    /// Create a new empty buffer
    pub fn new() -> Self {
        Buffer {
            lines: vec![Line::new()],
            path: None,
            name: String::from("[No Name]"),
            modified: false,
            readonly: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_group: Vec::new(),
            filetype: String::new(),
            line_ending: LineEnding::default(),
        }
    }

    /// Set the file path for this buffer
    pub fn set_path(&mut self, path: &str) {
        let path_buf = PathBuf::from(path);
        self.name = path_buf
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("[No Name]"));
        self.filetype = detect_filetype(&path_buf);
        self.path = Some(path_buf);
    }

    /// Create a buffer from a file
    pub fn from_file(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut lines = Vec::new();
        let mut line_ending = LineEnding::Unix;

        for line_result in reader.lines() {
            let line = line_result?;
            // Detect line ending from first line with content
            if lines.is_empty() && line.ends_with('\r') {
                line_ending = LineEnding::Mac;
            }
            lines.push(Line::from_string(line));
        }

        // Ensure at least one line
        if lines.is_empty() {
            lines.push(Line::new());
        }

        let path_buf = PathBuf::from(path);
        let name = path_buf
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("[No Name]"));

        let filetype = detect_filetype(&path_buf);

        Ok(Buffer {
            lines,
            path: Some(path_buf),
            name,
            modified: false,
            readonly: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_group: Vec::new(),
            filetype,
            line_ending,
        })
    }

    /// Save buffer to file
    pub fn save(&mut self) -> io::Result<()> {
        if let Some(path) = self.path.clone() {
            self.save_as(path.to_string_lossy().as_ref())
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "No file name"))
        }
    }

    /// Save buffer to a specific file
    pub fn save_as(&mut self, path: &str) -> io::Result<()> {
        let mut file = File::create(path)?;

        let line_sep = match self.line_ending {
            LineEnding::Unix => "\n",
            LineEnding::Dos => "\r\n",
            LineEnding::Mac => "\r",
        };

        for (i, line) in self.lines.iter().enumerate() {
            file.write_all(line.content.as_bytes())?;
            if i < self.lines.len() - 1 {
                file.write_all(line_sep.as_bytes())?;
            }
        }
        // Write final newline
        file.write_all(line_sep.as_bytes())?;

        self.path = Some(PathBuf::from(path));
        self.name = PathBuf::from(path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("[No Name]"));
        self.modified = false;
        self.filetype = detect_filetype(&PathBuf::from(path));

        Ok(())
    }

    /// Get the number of lines
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Get a line by index
    pub fn get_line(&self, line: usize) -> Option<&Line> {
        self.lines.get(line)
    }

    /// Get a mutable line by index
    pub fn get_line_mut(&mut self, line: usize) -> Option<&mut Line> {
        self.lines.get_mut(line)
    }

    /// Insert a character at position
    pub fn insert_char(&mut self, line: usize, col: usize, ch: char) {
        if let Some(l) = self.lines.get_mut(line) {
            // Find byte position from char position
            let byte_pos = char_to_byte_pos(&l.content, col);
            l.content.insert(byte_pos, ch);
            self.modified = true;

            self.current_group.push(EditOp::Insert {
                line,
                col,
                text: ch.to_string(),
            });
        }
    }

    /// Insert text at position
    pub fn insert_text(&mut self, line: usize, col: usize, text: &str) {
        if let Some(l) = self.lines.get_mut(line) {
            let byte_pos = char_to_byte_pos(&l.content, col);
            l.content.insert_str(byte_pos, text);
            self.modified = true;

            self.current_group.push(EditOp::Insert {
                line,
                col,
                text: text.to_string(),
            });
        }
    }

    /// Delete character at position
    pub fn delete_char(&mut self, line: usize, col: usize) -> Option<char> {
        if let Some(l) = self.lines.get_mut(line) {
            let byte_pos = char_to_byte_pos(&l.content, col);
            if byte_pos < l.content.len() {
                let ch = l.content.remove(byte_pos);
                self.modified = true;

                self.current_group.push(EditOp::Delete {
                    line,
                    col,
                    text: ch.to_string(),
                });

                return Some(ch);
            }
        }
        None
    }

    /// Delete range of text
    pub fn delete_range(&mut self, line: usize, start_col: usize, end_col: usize) -> String {
        if let Some(l) = self.lines.get_mut(line) {
            let start_byte = char_to_byte_pos(&l.content, start_col);
            let end_byte = char_to_byte_pos(&l.content, end_col);
            let deleted: String = l.content[start_byte..end_byte].to_string();
            l.content.drain(start_byte..end_byte);
            self.modified = true;

            self.current_group.push(EditOp::Delete {
                line,
                col: start_col,
                text: deleted.clone(),
            });

            return deleted;
        }
        String::new()
    }

    /// Insert a new line
    pub fn insert_line(&mut self, line: usize, content: String) {
        let new_line = Line::from_string(content.clone());
        if line <= self.lines.len() {
            self.lines.insert(line, new_line);
            self.modified = true;

            self.current_group
                .push(EditOp::InsertLine { line, content });
        }
    }

    /// Delete a line
    pub fn delete_line(&mut self, line: usize) -> Option<String> {
        if line < self.lines.len() && self.lines.len() > 1 {
            let removed = self.lines.remove(line);
            self.modified = true;

            self.current_group.push(EditOp::DeleteLine {
                line,
                content: removed.content.clone(),
            });

            return Some(removed.content);
        } else if line < self.lines.len() {
            // Last line - just clear it
            let content = self.lines[line].content.clone();
            self.lines[line].content.clear();
            self.modified = true;

            self.current_group.push(EditOp::DeleteLine {
                line,
                content: content.clone(),
            });

            return Some(content);
        }
        None
    }

    /// Split a line at position (for Enter key)
    pub fn split_line(&mut self, line: usize, col: usize) {
        if let Some(l) = self.lines.get_mut(line) {
            let byte_pos = char_to_byte_pos(&l.content, col);
            let new_content = l.content[byte_pos..].to_string();
            l.content.truncate(byte_pos);

            self.lines.insert(line + 1, Line::from_string(new_content));
            self.modified = true;

            self.current_group.push(EditOp::SplitLine { line, col });
        }
    }

    /// Join line with the next line
    pub fn join_lines(&mut self, line: usize) {
        if line + 1 < self.lines.len() {
            let next_content = self.lines.remove(line + 1).content;
            let col = self.lines[line].content.chars().count();

            // Add a space if needed
            if !self.lines[line].content.is_empty() && !next_content.is_empty() {
                self.lines[line].content.push(' ');
            }
            self.lines[line].content.push_str(next_content.trim_start());
            self.modified = true;

            self.current_group.push(EditOp::JoinLines { line, col });
        }
    }

    /// Begin a group of operations (for undo)
    pub fn begin_group(&mut self, cursor_line: usize, cursor_col: usize) {
        self.commit_group(cursor_line, cursor_col);
    }

    /// Commit current operation group
    pub fn commit_group(&mut self, cursor_line: usize, cursor_col: usize) {
        if !self.current_group.is_empty() {
            let ops = std::mem::take(&mut self.current_group);
            self.undo_stack.push(UndoState {
                ops,
                cursor_line,
                cursor_col,
            });
            self.redo_stack.clear();
        }
    }

    /// Undo last operation
    pub fn undo(&mut self) -> Option<(usize, usize)> {
        if let Some(state) = self.undo_stack.pop() {
            // Apply inverse operations in reverse order
            let mut redo_ops = Vec::new();
            for op in state.ops.iter().rev() {
                redo_ops.push(self.apply_inverse(op));
            }
            redo_ops.reverse();

            self.redo_stack.push(UndoState {
                ops: redo_ops,
                cursor_line: state.cursor_line,
                cursor_col: state.cursor_col,
            });

            return Some((state.cursor_line, state.cursor_col));
        }
        None
    }

    /// Redo last undone operation
    pub fn redo(&mut self) -> Option<(usize, usize)> {
        if let Some(state) = self.redo_stack.pop() {
            let mut undo_ops = Vec::new();
            for op in state.ops.iter() {
                undo_ops.push(self.apply_inverse(op));
            }

            self.undo_stack.push(UndoState {
                ops: undo_ops,
                cursor_line: state.cursor_line,
                cursor_col: state.cursor_col,
            });

            return Some((state.cursor_line, state.cursor_col));
        }
        None
    }

    /// Apply inverse of an operation
    fn apply_inverse(&mut self, op: &EditOp) -> EditOp {
        match op {
            EditOp::Insert { line, col, text } => {
                // Inverse of insert is delete
                if let Some(l) = self.lines.get_mut(*line) {
                    let byte_pos = char_to_byte_pos(&l.content, *col);
                    let end_byte = byte_pos + text.len();
                    l.content.drain(byte_pos..end_byte);
                }
                EditOp::Delete {
                    line: *line,
                    col: *col,
                    text: text.clone(),
                }
            }
            EditOp::Delete { line, col, text } => {
                // Inverse of delete is insert
                if let Some(l) = self.lines.get_mut(*line) {
                    let byte_pos = char_to_byte_pos(&l.content, *col);
                    l.content.insert_str(byte_pos, text);
                }
                EditOp::Insert {
                    line: *line,
                    col: *col,
                    text: text.clone(),
                }
            }
            EditOp::InsertLine { line, content } => {
                // Inverse is delete line
                if *line < self.lines.len() {
                    self.lines.remove(*line);
                }
                EditOp::DeleteLine {
                    line: *line,
                    content: content.clone(),
                }
            }
            EditOp::DeleteLine { line, content } => {
                // Inverse is insert line
                self.lines.insert(*line, Line::from_string(content.clone()));
                EditOp::InsertLine {
                    line: *line,
                    content: content.clone(),
                }
            }
            EditOp::SplitLine { line, col } => {
                // Inverse is join lines
                if *line + 1 < self.lines.len() {
                    let next = self.lines.remove(*line + 1).content;
                    if let Some(l) = self.lines.get_mut(*line) {
                        l.content.push_str(&next);
                    }
                }
                EditOp::JoinLines {
                    line: *line,
                    col: *col,
                }
            }
            EditOp::JoinLines { line, col } => {
                // Inverse is split line
                if let Some(l) = self.lines.get_mut(*line) {
                    let byte_pos = char_to_byte_pos(&l.content, *col);
                    let new_content = l.content[byte_pos..].to_string();
                    l.content.truncate(byte_pos);
                    self.lines.insert(*line + 1, Line::from_string(new_content));
                }
                EditOp::SplitLine {
                    line: *line,
                    col: *col,
                }
            }
            EditOp::Replace {
                line,
                col,
                old_text,
                new_text,
            } => {
                if let Some(l) = self.lines.get_mut(*line) {
                    let byte_pos = char_to_byte_pos(&l.content, *col);
                    let end_byte = byte_pos + new_text.len();
                    l.content.replace_range(byte_pos..end_byte, old_text);
                }
                EditOp::Replace {
                    line: *line,
                    col: *col,
                    old_text: new_text.clone(),
                    new_text: old_text.clone(),
                }
            }
            EditOp::Group(ops) => {
                let mut inverse_ops = Vec::new();
                for op in ops.iter().rev() {
                    inverse_ops.push(self.apply_inverse(op));
                }
                inverse_ops.reverse();
                EditOp::Group(inverse_ops)
            }
        }
    }

    /// Search for text in buffer
    pub fn search(
        &self,
        pattern: &str,
        start_line: usize,
        start_col: usize,
        forward: bool,
    ) -> Option<(usize, usize)> {
        if pattern.is_empty() {
            return None;
        }

        if forward {
            // Search forward from current position
            for line_idx in start_line..self.lines.len() {
                let line = &self.lines[line_idx].content;
                let search_start = if line_idx == start_line {
                    char_to_byte_pos(line, start_col + 1)
                } else {
                    0
                };

                if let Some(pos) = line[search_start..].find(pattern) {
                    let byte_pos = search_start + pos;
                    return Some((line_idx, byte_to_char_pos(line, byte_pos)));
                }
            }

            // Wrap around to beginning
            for line_idx in 0..=start_line {
                let line = &self.lines[line_idx].content;
                let search_end = if line_idx == start_line {
                    char_to_byte_pos(line, start_col)
                } else {
                    line.len()
                };

                if let Some(pos) = line[..search_end].find(pattern) {
                    return Some((line_idx, byte_to_char_pos(line, pos)));
                }
            }
        } else {
            // Search backward
            for line_idx in (0..=start_line).rev() {
                let line = &self.lines[line_idx].content;
                let search_end = if line_idx == start_line {
                    char_to_byte_pos(line, start_col)
                } else {
                    line.len()
                };

                if let Some(pos) = line[..search_end].rfind(pattern) {
                    return Some((line_idx, byte_to_char_pos(line, pos)));
                }
            }

            // Wrap around to end
            for line_idx in (start_line..self.lines.len()).rev() {
                let line = &self.lines[line_idx].content;
                let search_start = if line_idx == start_line {
                    char_to_byte_pos(line, start_col + 1)
                } else {
                    0
                };

                if let Some(pos) = line[search_start..].rfind(pattern) {
                    let byte_pos = search_start + pos;
                    return Some((line_idx, byte_to_char_pos(line, byte_pos)));
                }
            }
        }

        None
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert character position to byte position
fn char_to_byte_pos(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

/// Convert byte position to character position
fn byte_to_char_pos(s: &str, byte_pos: usize) -> usize {
    s[..byte_pos].chars().count()
}

/// Detect file type from extension
fn detect_filetype(path: &PathBuf) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust".to_string(),
        Some("c") | Some("h") => "c".to_string(),
        Some("cpp") | Some("cxx") | Some("cc") | Some("hpp") => "cpp".to_string(),
        Some("py") => "python".to_string(),
        Some("js") => "javascript".to_string(),
        Some("ts") => "typescript".to_string(),
        Some("sh") | Some("bash") => "sh".to_string(),
        Some("vim") => "vim".to_string(),
        Some("toml") => "toml".to_string(),
        Some("json") => "json".to_string(),
        Some("yaml") | Some("yml") => "yaml".to_string(),
        Some("md") => "markdown".to_string(),
        Some("txt") => "text".to_string(),
        Some("asm") | Some("S") => "asm".to_string(),
        _ => String::new(),
    }
}
