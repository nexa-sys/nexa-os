//! Vim Script lexer (placeholder module)
//! 
//! This module provides tokenization for Vim Script.
//! Currently the parser uses simple string parsing.

/// Token types for Vim Script
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenType {
    // Literals
    Number,
    Float,
    String,
    
    // Identifiers and keywords
    Identifier,
    Keyword,
    
    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Dot,
    Equal,
    EqualEqual,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    Not,
    
    // Delimiters
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Comma,
    Colon,
    Question,
    
    // Special
    Newline,
    Comment,
    Eof,
}

/// A token in Vim Script
#[derive(Debug, Clone)]
pub struct Token {
    pub token_type: TokenType,
    pub value: String,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(token_type: TokenType, value: String, line: usize, column: usize) -> Self {
        Token {
            token_type,
            value,
            line,
            column,
        }
    }
}

/// Keywords in Vim Script
pub const KEYWORDS: &[&str] = &[
    "if", "else", "elseif", "endif",
    "while", "endwhile",
    "for", "endfor", "in",
    "function", "endfunction",
    "return", "break", "continue",
    "try", "catch", "finally", "endtry", "throw",
    "let", "unlet", "const",
    "call", "execute",
    "set", "setlocal",
    "echo", "echom", "echoerr",
    "source", "runtime",
    "autocmd", "augroup",
    "command", "delcommand",
    "map", "nmap", "imap", "vmap", "cmap", "omap",
    "noremap", "nnoremap", "inoremap", "vnoremap", "cnoremap", "onoremap",
    "unmap", "mapclear",
    "syntax", "highlight", "hi",
    "filetype",
    "finish",
];

/// Check if string is a keyword
pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}
