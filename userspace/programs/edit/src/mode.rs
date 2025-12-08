//! Editor mode definitions

/// The current mode of the editor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Normal mode - navigation and commands
    Normal,
    /// Insert mode - text insertion
    Insert,
    /// Visual mode - character-wise selection
    Visual,
    /// Visual Line mode - line-wise selection
    VisualLine,
    /// Visual Block mode - block selection
    VisualBlock,
    /// Command-line mode - ex commands
    Command,
    /// Search mode - searching text
    Search,
    /// Replace mode - overwrite text
    Replace,
}

impl Mode {
    /// Get the mode indicator string for status line
    pub fn indicator(&self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::VisualBlock => "V-BLOCK",
            Mode::Command => "COMMAND",
            Mode::Search => "SEARCH",
            Mode::Replace => "REPLACE",
        }
    }
    
    /// Check if the mode allows text editing
    pub fn is_editing(&self) -> bool {
        matches!(self, Mode::Insert | Mode::Replace)
    }
    
    /// Check if the mode is a visual selection mode
    pub fn is_visual(&self) -> bool {
        matches!(self, Mode::Visual | Mode::VisualLine | Mode::VisualBlock)
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}

/// Pending operator for operator-pending mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    None,
    Delete,     // d
    Yank,       // y
    Change,     // c
    Indent,     // >
    Unindent,   // <
    Format,     // gq
    Uppercase,  // gU
    Lowercase,  // gu
}

impl Default for Operator {
    fn default() -> Self {
        Operator::None
    }
}

/// Motion type for operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionType {
    /// Character-wise motion (inclusive)
    CharInclusive,
    /// Character-wise motion (exclusive)
    CharExclusive,
    /// Line-wise motion
    LineWise,
    /// Block-wise motion
    BlockWise,
}
