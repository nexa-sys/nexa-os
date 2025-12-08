//! Shell Parser - Control Flow Syntax Support
//!
//! Supports: if/then/elif/else/fi, case/esac, for/while/until, select, function, (( )), [[ ]], { }
//! Also supports redirections (>, >>, <, <<, 2>, 2>>, &>, etc.) and here-documents.

use crate::state::ShellState;
use crate::builtins::BuiltinRegistry;
use std::collections::VecDeque;

/// Redirection types
#[derive(Debug, Clone, PartialEq)]
pub enum RedirectType {
    /// > file (output)
    Output,
    /// >> file (append output)
    Append,
    /// < file (input)
    Input,
    /// << word (here-document)
    HereDoc,
    /// <<< word (here-string)
    HereString,
    /// 2> file (stderr)
    Stderr,
    /// 2>> file (append stderr)
    StderrAppend,
    /// &> file (both stdout and stderr)
    Both,
    /// &>> file (append both)
    BothAppend,
    /// n> file (fd n to file)
    FdOutput(i32),
    /// n>> file (fd n append to file)
    FdAppend(i32),
    /// n< file (fd n from file)
    FdInput(i32),
    /// n>&m (duplicate fd)
    FdDup(i32, i32),
    /// n<&m (duplicate fd for input)
    FdDupIn(i32, i32),
    /// n>&- (close fd)
    FdClose(i32),
}

/// A single redirection
#[derive(Debug, Clone)]
pub struct Redirect {
    pub rtype: RedirectType,
    pub target: String,  // filename or here-doc content
}

/// Parsed command types
#[derive(Debug, Clone)]
pub enum Command {
    /// Simple command with optional redirections: cmd arg1 arg2 ... [redirections]
    Simple(Vec<String>),
    /// Simple command with redirections
    SimpleWithRedirects {
        args: Vec<String>,
        redirects: Vec<Redirect>,
    },
    /// Pipeline: cmd1 | cmd2 | cmd3
    Pipeline(Vec<Command>),
    /// And list: cmd1 && cmd2
    AndList(Vec<Command>),
    /// Or list: cmd1 || cmd2
    OrList(Vec<Command>),
    /// Background: cmd &
    Background(Box<Command>),
    /// Subshell: ( cmd )
    Subshell(Box<Command>),
    /// Brace group: { cmd; }
    BraceGroup(Vec<Command>),
    /// If statement
    If {
        condition: Vec<Command>,
        then_part: Vec<Command>,
        elif_parts: Vec<(Vec<Command>, Vec<Command>)>,
        else_part: Option<Vec<Command>>,
    },
    /// Case statement
    Case {
        word: String,
        cases: Vec<(Vec<String>, Vec<Command>)>,
    },
    /// For loop
    For {
        var: String,
        words: Vec<String>,
        body: Vec<Command>,
    },
    /// While loop
    While {
        condition: Vec<Command>,
        body: Vec<Command>,
    },
    /// Until loop
    Until {
        condition: Vec<Command>,
        body: Vec<Command>,
    },
    /// Select menu
    Select {
        var: String,
        words: Vec<String>,
        body: Vec<Command>,
    },
    /// Function definition
    Function {
        name: String,
        body: Vec<Command>,
    },
    /// Arithmetic expression: (( expr ))
    Arithmetic(String),
    /// Conditional expression: [[ expr ]]
    Conditional(Vec<String>),
    /// Empty command
    Empty,
}

/// Token types for lexer
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(String),
    // Operators
    Pipe,           // |
    And,            // &&
    Or,             // ||
    Semi,           // ;
    Newline,
    Amp,            // &
    LParen,         // (
    RParen,         // )
    LBrace,         // {
    RBrace,         // }
    DoubleParen,    // ((
    DoubleParenEnd, // ))
    DoubleBracket,  // [[
    DoubleBracketEnd, // ]]
    // Redirections
    RedirectOut,        // >
    RedirectAppend,     // >>
    RedirectIn,         // <
    HereDoc,            // <<
    HereString,         // <<<
    RedirectBoth,       // &>
    RedirectBothAppend, // &>>
    RedirectFd(i32),    // n> (file descriptor redirect)
    RedirectFdAppend(i32), // n>>
    RedirectFdIn(i32),  // n<
    DupFd,              // >&
    DupFdIn,            // <&
    // Keywords
    If,
    Then,
    Elif,
    Else,
    Fi,
    Case,
    Esac,
    For,
    In,
    Do,
    Done,
    While,
    Until,
    Select,
    Function,
    Time,
    Coproc,
    // End of input
    Eof,
}

/// Tokenizer
pub struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek();
        self.pos += 1;
        c
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_word(&mut self) -> String {
        let mut word = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut escaped = false;

        while let Some(c) = self.peek() {
            if escaped {
                word.push(c);
                self.advance();
                escaped = false;
                continue;
            }

            match c {
                '\\' if !in_single_quote => {
                    escaped = true;
                    self.advance();
                }
                '\'' if !in_double_quote => {
                    in_single_quote = !in_single_quote;
                    self.advance();
                }
                '"' if !in_single_quote => {
                    in_double_quote = !in_double_quote;
                    self.advance();
                }
                ' ' | '\t' | '\n' | ';' | '|' | '&' | '(' | ')' | '{' | '}' | '<' | '>'
                    if !in_single_quote && !in_double_quote => {
                    break;
                }
                _ => {
                    word.push(c);
                    self.advance();
                }
            }
        }
        word
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();

        match self.peek() {
            None => Token::Eof,
            Some('\n') => {
                self.advance();
                Token::Newline
            }
            Some(';') => {
                self.advance();
                Token::Semi
            }
            Some('|') => {
                self.advance();
                if self.peek() == Some('|') {
                    self.advance();
                    Token::Or
                } else {
                    Token::Pipe
                }
            }
            Some('&') => {
                self.advance();
                if self.peek() == Some('&') {
                    self.advance();
                    Token::And
                } else if self.peek() == Some('>') {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        Token::RedirectBothAppend
                    } else {
                        Token::RedirectBoth
                    }
                } else {
                    Token::Amp
                }
            }
            Some('(') => {
                self.advance();
                if self.peek() == Some('(') {
                    self.advance();
                    Token::DoubleParen
                } else {
                    Token::LParen
                }
            }
            Some(')') => {
                self.advance();
                if self.peek() == Some(')') {
                    self.advance();
                    Token::DoubleParenEnd
                } else {
                    Token::RParen
                }
            }
            Some('[') => {
                self.advance();
                if self.peek() == Some('[') {
                    self.advance();
                    Token::DoubleBracket
                } else {
                    // Put back and read as word
                    self.pos -= 1;
                    let word = self.read_word();
                    self.keyword_or_word(word)
                }
            }
            Some(']') => {
                self.advance();
                if self.peek() == Some(']') {
                    self.advance();
                    Token::DoubleBracketEnd
                } else {
                    self.pos -= 1;
                    let word = self.read_word();
                    self.keyword_or_word(word)
                }
            }
            Some('{') => {
                self.advance();
                Token::LBrace
            }
            Some('}') => {
                self.advance();
                Token::RBrace
            }
            Some('>') => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    Token::RedirectAppend
                } else if self.peek() == Some('&') {
                    self.advance();
                    Token::DupFd
                } else {
                    Token::RedirectOut
                }
            }
            Some('<') => {
                self.advance();
                if self.peek() == Some('<') {
                    self.advance();
                    if self.peek() == Some('<') {
                        self.advance();
                        Token::HereString
                    } else {
                        Token::HereDoc
                    }
                } else if self.peek() == Some('&') {
                    self.advance();
                    Token::DupFdIn
                } else {
                    Token::RedirectIn
                }
            }
            Some('#') => {
                // Comment - skip to end of line
                while let Some(c) = self.peek() {
                    if c == '\n' { break; }
                    self.advance();
                }
                self.next_token()
            }
            Some(c) if c.is_ascii_digit() => {
                // Check for fd redirect like 2> or 2>>
                let start_pos = self.pos;
                let mut fd_str = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        fd_str.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                
                match self.peek() {
                    Some('>') => {
                        self.advance();
                        let fd: i32 = fd_str.parse().unwrap_or(1);
                        if self.peek() == Some('>') {
                            self.advance();
                            Token::RedirectFdAppend(fd)
                        } else if self.peek() == Some('&') {
                            self.advance();
                            // Parse target fd: n>&m or n>&-
                            self.skip_whitespace();
                            if self.peek() == Some('-') {
                                self.advance();
                                // Close fd: n>&-
                                return Token::Word(format!("{fd}>&-"));
                            }
                            // n>&m - we'll handle as a special word
                            return Token::Word(format!("{fd}>&"));
                        } else {
                            Token::RedirectFd(fd)
                        }
                    }
                    Some('<') => {
                        self.advance();
                        let fd: i32 = fd_str.parse().unwrap_or(0);
                        if self.peek() == Some('&') {
                            self.advance();
                            return Token::Word(format!("{fd}<&"));
                        }
                        Token::RedirectFdIn(fd)
                    }
                    _ => {
                        // Not a redirect, restore position and read as word
                        self.pos = start_pos;
                        let word = self.read_word();
                        if word.is_empty() {
                            Token::Eof
                        } else {
                            self.keyword_or_word(word)
                        }
                    }
                }
            }
            Some(_) => {
                let word = self.read_word();
                if word.is_empty() {
                    Token::Eof
                } else {
                    self.keyword_or_word(word)
                }
            }
        }
    }

    fn keyword_or_word(&self, word: String) -> Token {
        match word.as_str() {
            "if" => Token::If,
            "then" => Token::Then,
            "elif" => Token::Elif,
            "else" => Token::Else,
            "fi" => Token::Fi,
            "case" => Token::Case,
            "esac" => Token::Esac,
            "for" => Token::For,
            "in" => Token::In,
            "do" => Token::Do,
            "done" => Token::Done,
            "while" => Token::While,
            "until" => Token::Until,
            "select" => Token::Select,
            "function" => Token::Function,
            "time" => Token::Time,
            "coproc" => Token::Coproc,
            _ => Token::Word(word),
        }
    }

    /// Tokenize entire input
    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            if tok == Token::Eof {
                tokens.push(tok);
                break;
            }
            tokens.push(tok);
        }
        tokens
    }
}

/// Parser for shell commands
pub struct Parser {
    tokens: VecDeque<Token>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens: tokens.into(),
        }
    }

    fn peek(&self) -> &Token {
        self.tokens.front().unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        self.tokens.pop_front().unwrap_or(Token::Eof)
    }

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(&expected) {
            Ok(())
        } else {
            Err(format!("期望 {:?}, 得到 {:?}", expected, tok))
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline | Token::Semi) {
            self.advance();
        }
    }

    /// Parse a complete script
    pub fn parse(&mut self) -> Result<Vec<Command>, String> {
        let mut commands = Vec::new();
        self.skip_newlines();
        
        while !matches!(self.peek(), Token::Eof) {
            let cmd = self.parse_list()?;
            if !matches!(cmd, Command::Empty) {
                commands.push(cmd);
            }
            self.skip_newlines();
        }
        
        Ok(commands)
    }

    /// Parse command list (separated by ; or newline)
    fn parse_list(&mut self) -> Result<Command, String> {
        let first = self.parse_and_or()?;
        
        // Handle background
        if matches!(self.peek(), Token::Amp) {
            self.advance();
            return Ok(Command::Background(Box::new(first)));
        }

        Ok(first)
    }

    /// Parse && and || chains
    fn parse_and_or(&mut self) -> Result<Command, String> {
        let mut left = self.parse_pipeline()?;

        loop {
            match self.peek() {
                Token::And => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_pipeline()?;
                    left = match left {
                        Command::AndList(mut list) => {
                            list.push(right);
                            Command::AndList(list)
                        }
                        _ => Command::AndList(vec![left, right]),
                    };
                }
                Token::Or => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_pipeline()?;
                    left = match left {
                        Command::OrList(mut list) => {
                            list.push(right);
                            Command::OrList(list)
                        }
                        _ => Command::OrList(vec![left, right]),
                    };
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// Parse pipeline: cmd1 | cmd2 | cmd3
    fn parse_pipeline(&mut self) -> Result<Command, String> {
        let first = self.parse_command()?;
        
        if !matches!(self.peek(), Token::Pipe) {
            return Ok(first);
        }

        let mut commands = vec![first];
        while matches!(self.peek(), Token::Pipe) {
            self.advance();
            self.skip_newlines();
            commands.push(self.parse_command()?);
        }

        Ok(Command::Pipeline(commands))
    }

    /// Parse a single command (could be compound or simple)
    fn parse_command(&mut self) -> Result<Command, String> {
        self.skip_newlines();
        
        match self.peek() {
            Token::If => self.parse_if(),
            Token::Case => self.parse_case(),
            Token::For => self.parse_for(),
            Token::While => self.parse_while(),
            Token::Until => self.parse_until(),
            Token::Select => self.parse_select(),
            Token::Function => self.parse_function(),
            Token::LParen => self.parse_subshell(),
            Token::LBrace => self.parse_brace_group(),
            Token::DoubleParen => self.parse_arithmetic(),
            Token::DoubleBracket => self.parse_conditional(),
            Token::Word(w) if self.is_function_definition(&w) => self.parse_function_shorthand(),
            _ => self.parse_simple_command(),
        }
    }

    fn is_function_definition(&self, _word: &str) -> bool {
        // Check if next token is ()
        if let Some(Token::Word(s)) = self.tokens.get(1) {
            s == "()"
        } else {
            false
        }
    }

    /// Parse if statement
    fn parse_if(&mut self) -> Result<Command, String> {
        self.expect(Token::If)?;
        self.skip_newlines();
        
        // Parse condition
        let condition = self.parse_compound_list_until(&[Token::Then])?;
        self.expect(Token::Then)?;
        self.skip_newlines();
        
        // Parse then part
        let then_part = self.parse_compound_list_until(&[Token::Elif, Token::Else, Token::Fi])?;
        
        // Parse elif parts
        let mut elif_parts = Vec::new();
        while matches!(self.peek(), Token::Elif) {
            self.advance();
            self.skip_newlines();
            let elif_cond = self.parse_compound_list_until(&[Token::Then])?;
            self.expect(Token::Then)?;
            self.skip_newlines();
            let elif_then = self.parse_compound_list_until(&[Token::Elif, Token::Else, Token::Fi])?;
            elif_parts.push((elif_cond, elif_then));
        }
        
        // Parse else part
        let else_part = if matches!(self.peek(), Token::Else) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_compound_list_until(&[Token::Fi])?)
        } else {
            None
        };
        
        self.expect(Token::Fi)?;
        
        Ok(Command::If {
            condition,
            then_part,
            elif_parts,
            else_part,
        })
    }

    /// Parse case statement
    fn parse_case(&mut self) -> Result<Command, String> {
        self.expect(Token::Case)?;
        self.skip_newlines();
        
        let word = match self.advance() {
            Token::Word(w) => w,
            t => return Err(format!("case: 期望词语, 得到 {:?}", t)),
        };
        
        self.skip_newlines();
        self.expect(Token::In)?;
        self.skip_newlines();
        
        let mut cases = Vec::new();
        while !matches!(self.peek(), Token::Esac | Token::Eof) {
            // Parse patterns
            let mut patterns = Vec::new();
            loop {
                match self.advance() {
                    Token::Word(p) => patterns.push(p),
                    Token::LParen => continue, // Optional leading (
                    t => return Err(format!("case: 期望模式, 得到 {:?}", t)),
                }
                
                match self.peek() {
                    Token::Pipe => {
                        self.advance();
                        continue;
                    }
                    Token::RParen => {
                        self.advance();
                        break;
                    }
                    Token::Word(w) if w == ")" => {
                        self.advance();
                        break;
                    }
                    _ => break,
                }
            }
            
            self.skip_newlines();
            
            // Parse commands until ;; or esac
            let mut cmds = Vec::new();
            while !matches!(self.peek(), Token::Esac | Token::Eof) {
                if let Token::Word(w) = self.peek() {
                    if w == ";;" {
                        self.advance();
                        break;
                    }
                }
                let cmd = self.parse_list()?;
                if !matches!(cmd, Command::Empty) {
                    cmds.push(cmd);
                }
                self.skip_newlines();
            }
            
            if !patterns.is_empty() {
                cases.push((patterns, cmds));
            }
            self.skip_newlines();
        }
        
        self.expect(Token::Esac)?;
        
        Ok(Command::Case { word, cases })
    }

    /// Parse for loop
    fn parse_for(&mut self) -> Result<Command, String> {
        self.expect(Token::For)?;
        self.skip_newlines();
        
        let var = match self.advance() {
            Token::Word(w) => w,
            t => return Err(format!("for: 期望变量名, 得到 {:?}", t)),
        };
        
        self.skip_newlines();
        
        // Optional 'in words...'
        let words = if matches!(self.peek(), Token::In) {
            self.advance();
            let mut words = Vec::new();
            while let Token::Word(w) = self.peek().clone() {
                self.advance();
                words.push(w);
            }
            words
        } else {
            // Default to "$@"
            vec!["\"$@\"".to_string()]
        };
        
        self.skip_newlines();
        
        // Expect do or ; do
        if matches!(self.peek(), Token::Semi) {
            self.advance();
        }
        self.skip_newlines();
        self.expect(Token::Do)?;
        self.skip_newlines();
        
        let body = self.parse_compound_list_until(&[Token::Done])?;
        self.expect(Token::Done)?;
        
        Ok(Command::For { var, words, body })
    }

    /// Parse while loop
    fn parse_while(&mut self) -> Result<Command, String> {
        self.expect(Token::While)?;
        self.skip_newlines();
        
        let condition = self.parse_compound_list_until(&[Token::Do])?;
        self.expect(Token::Do)?;
        self.skip_newlines();
        
        let body = self.parse_compound_list_until(&[Token::Done])?;
        self.expect(Token::Done)?;
        
        Ok(Command::While { condition, body })
    }

    /// Parse until loop
    fn parse_until(&mut self) -> Result<Command, String> {
        self.expect(Token::Until)?;
        self.skip_newlines();
        
        let condition = self.parse_compound_list_until(&[Token::Do])?;
        self.expect(Token::Do)?;
        self.skip_newlines();
        
        let body = self.parse_compound_list_until(&[Token::Done])?;
        self.expect(Token::Done)?;
        
        Ok(Command::Until { condition, body })
    }

    /// Parse select menu
    fn parse_select(&mut self) -> Result<Command, String> {
        self.expect(Token::Select)?;
        self.skip_newlines();
        
        let var = match self.advance() {
            Token::Word(w) => w,
            t => return Err(format!("select: 期望变量名, 得到 {:?}", t)),
        };
        
        self.skip_newlines();
        
        // 'in words...'
        self.expect(Token::In)?;
        let mut words = Vec::new();
        while let Token::Word(w) = self.peek().clone() {
            self.advance();
            words.push(w);
        }
        
        self.skip_newlines();
        if matches!(self.peek(), Token::Semi) {
            self.advance();
        }
        self.skip_newlines();
        self.expect(Token::Do)?;
        self.skip_newlines();
        
        let body = self.parse_compound_list_until(&[Token::Done])?;
        self.expect(Token::Done)?;
        
        Ok(Command::Select { var, words, body })
    }

    /// Parse function definition: function name { ... }
    fn parse_function(&mut self) -> Result<Command, String> {
        self.expect(Token::Function)?;
        self.skip_newlines();
        
        let name = match self.advance() {
            Token::Word(w) => w,
            t => return Err(format!("function: 期望函数名, 得到 {:?}", t)),
        };
        
        self.skip_newlines();
        
        // Optional ()
        if let Token::Word(w) = self.peek() {
            if w == "()" {
                self.advance();
            }
        }
        
        self.skip_newlines();
        self.expect(Token::LBrace)?;
        self.skip_newlines();
        
        let body = self.parse_compound_list_until(&[Token::RBrace])?;
        self.expect(Token::RBrace)?;
        
        Ok(Command::Function { name, body })
    }

    /// Parse function shorthand: name() { ... }
    fn parse_function_shorthand(&mut self) -> Result<Command, String> {
        let name = match self.advance() {
            Token::Word(w) => w,
            t => return Err(format!("function: 期望函数名, 得到 {:?}", t)),
        };
        
        // Skip ()
        if let Token::Word(w) = self.peek() {
            if w == "()" {
                self.advance();
            }
        }
        
        self.skip_newlines();
        self.expect(Token::LBrace)?;
        self.skip_newlines();
        
        let body = self.parse_compound_list_until(&[Token::RBrace])?;
        self.expect(Token::RBrace)?;
        
        Ok(Command::Function { name, body })
    }

    /// Parse subshell: ( commands )
    fn parse_subshell(&mut self) -> Result<Command, String> {
        self.expect(Token::LParen)?;
        self.skip_newlines();
        
        let mut commands = Vec::new();
        while !matches!(self.peek(), Token::RParen | Token::Eof) {
            let cmd = self.parse_list()?;
            if !matches!(cmd, Command::Empty) {
                commands.push(cmd);
            }
            self.skip_newlines();
        }
        
        self.expect(Token::RParen)?;
        
        if commands.len() == 1 {
            Ok(Command::Subshell(Box::new(commands.remove(0))))
        } else {
            Ok(Command::Subshell(Box::new(Command::BraceGroup(commands))))
        }
    }

    /// Parse brace group: { commands; }
    fn parse_brace_group(&mut self) -> Result<Command, String> {
        self.expect(Token::LBrace)?;
        self.skip_newlines();
        
        let commands = self.parse_compound_list_until(&[Token::RBrace])?;
        self.expect(Token::RBrace)?;
        
        Ok(Command::BraceGroup(commands))
    }

    /// Parse arithmetic expression: (( expr ))
    fn parse_arithmetic(&mut self) -> Result<Command, String> {
        self.expect(Token::DoubleParen)?;
        
        let mut expr = String::new();
        loop {
            match self.advance() {
                Token::DoubleParenEnd => break,
                Token::Word(w) => {
                    if !expr.is_empty() {
                        expr.push(' ');
                    }
                    expr.push_str(&w);
                }
                Token::Eof => return Err("((...)) 未闭合".to_string()),
                _ => {}
            }
        }
        
        Ok(Command::Arithmetic(expr))
    }

    /// Parse conditional expression: [[ expr ]]
    fn parse_conditional(&mut self) -> Result<Command, String> {
        self.expect(Token::DoubleBracket)?;
        
        let mut args = Vec::new();
        loop {
            match self.peek() {
                Token::DoubleBracketEnd => {
                    self.advance();
                    break;
                }
                Token::Word(w) => {
                    args.push(w.clone());
                    self.advance();
                }
                Token::Eof => return Err("[[ ... ]] 未闭合".to_string()),
                _ => {
                    self.advance();
                }
            }
        }
        
        Ok(Command::Conditional(args))
    }

    /// Parse simple command with optional redirections
    fn parse_simple_command(&mut self) -> Result<Command, String> {
        let mut words = Vec::new();
        let mut redirects = Vec::new();
        
        loop {
            match self.peek() {
                Token::Word(w) => {
                    words.push(w.clone());
                    self.advance();
                }
                // Output redirections
                Token::RedirectOut => {
                    self.advance();
                    let target = self.expect_word("重定向目标")?;
                    redirects.push(Redirect {
                        rtype: RedirectType::Output,
                        target,
                    });
                }
                Token::RedirectAppend => {
                    self.advance();
                    let target = self.expect_word("重定向目标")?;
                    redirects.push(Redirect {
                        rtype: RedirectType::Append,
                        target,
                    });
                }
                Token::RedirectIn => {
                    self.advance();
                    let target = self.expect_word("重定向目标")?;
                    redirects.push(Redirect {
                        rtype: RedirectType::Input,
                        target,
                    });
                }
                Token::HereDoc => {
                    self.advance();
                    let delimiter = self.expect_word("here-doc 分隔符")?;
                    // Read here-doc content (simplified: just store delimiter, content read later)
                    redirects.push(Redirect {
                        rtype: RedirectType::HereDoc,
                        target: delimiter,
                    });
                }
                Token::HereString => {
                    self.advance();
                    let content = self.expect_word("here-string 内容")?;
                    redirects.push(Redirect {
                        rtype: RedirectType::HereString,
                        target: content,
                    });
                }
                Token::RedirectBoth => {
                    self.advance();
                    let target = self.expect_word("重定向目标")?;
                    redirects.push(Redirect {
                        rtype: RedirectType::Both,
                        target,
                    });
                }
                Token::RedirectBothAppend => {
                    self.advance();
                    let target = self.expect_word("重定向目标")?;
                    redirects.push(Redirect {
                        rtype: RedirectType::BothAppend,
                        target,
                    });
                }
                Token::RedirectFd(fd) => {
                    let fd = *fd;
                    self.advance();
                    let target = self.expect_word("重定向目标")?;
                    redirects.push(Redirect {
                        rtype: RedirectType::FdOutput(fd),
                        target,
                    });
                }
                Token::RedirectFdAppend(fd) => {
                    let fd = *fd;
                    self.advance();
                    let target = self.expect_word("重定向目标")?;
                    redirects.push(Redirect {
                        rtype: RedirectType::FdAppend(fd),
                        target,
                    });
                }
                Token::RedirectFdIn(fd) => {
                    let fd = *fd;
                    self.advance();
                    let target = self.expect_word("重定向目标")?;
                    redirects.push(Redirect {
                        rtype: RedirectType::FdInput(fd),
                        target,
                    });
                }
                Token::DupFd => {
                    self.advance();
                    let target = self.expect_word("文件描述符")?;
                    if target == "-" {
                        redirects.push(Redirect {
                            rtype: RedirectType::FdClose(1),
                            target: String::new(),
                        });
                    } else if let Ok(fd) = target.parse::<i32>() {
                        redirects.push(Redirect {
                            rtype: RedirectType::FdDup(1, fd),
                            target: String::new(),
                        });
                    } else {
                        // >&file means redirect both stdout and stderr
                        redirects.push(Redirect {
                            rtype: RedirectType::Both,
                            target,
                        });
                    }
                }
                Token::DupFdIn => {
                    self.advance();
                    let target = self.expect_word("文件描述符")?;
                    if target == "-" {
                        redirects.push(Redirect {
                            rtype: RedirectType::FdClose(0),
                            target: String::new(),
                        });
                    } else if let Ok(fd) = target.parse::<i32>() {
                        redirects.push(Redirect {
                            rtype: RedirectType::FdDupIn(0, fd),
                            target: String::new(),
                        });
                    }
                }
                Token::Pipe | Token::And | Token::Or | Token::Semi | Token::Amp |
                Token::Newline | Token::Eof | Token::Then | Token::Do | Token::Done |
                Token::Fi | Token::Elif | Token::Else | Token::Esac | Token::RParen |
                Token::RBrace | Token::In => break,
                _ => {
                    self.advance();
                }
            }
        }
        
        if words.is_empty() && redirects.is_empty() {
            Ok(Command::Empty)
        } else if redirects.is_empty() {
            Ok(Command::Simple(words))
        } else {
            Ok(Command::SimpleWithRedirects {
                args: words,
                redirects,
            })
        }
    }

    /// Expect a word token, return error with context if not found
    fn expect_word(&mut self, context: &str) -> Result<String, String> {
        self.skip_whitespace_tokens();
        match self.advance() {
            Token::Word(w) => Ok(w),
            t => Err(format!("期望 {}, 得到 {:?}", context, t)),
        }
    }

    /// Skip whitespace-like tokens (useful when parsing redirect targets)
    fn skip_whitespace_tokens(&mut self) {
        // In this lexer, whitespace is already skipped, but we might need to skip newlines in some contexts
    }

    /// Parse compound list until one of the stop tokens
    fn parse_compound_list_until(&mut self, stops: &[Token]) -> Result<Vec<Command>, String> {
        let mut commands = Vec::new();
        
        loop {
            self.skip_newlines();
            
            let stop = stops.iter().any(|s| {
                std::mem::discriminant(self.peek()) == std::mem::discriminant(s)
            });
            
            if stop || matches!(self.peek(), Token::Eof) {
                break;
            }
            
            let cmd = self.parse_list()?;
            if !matches!(cmd, Command::Empty) {
                commands.push(cmd);
            }
            
            // Consume separator
            if matches!(self.peek(), Token::Semi | Token::Newline) {
                self.advance();
            }
        }
        
        Ok(commands)
    }
}

/// Parse a shell command string
pub fn parse_command(input: &str) -> Result<Vec<Command>, String> {
    let mut lexer = Lexer::new(input);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    parser.parse()
}
