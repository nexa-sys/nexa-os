//! Vim Script interpreter for NexaOS edit
//!
//! Supports a subset of Vim Script including:
//! - Variables (let, unlet)
//! - Conditionals (if/elseif/else/endif)
//! - Loops (while/endwhile, for/endfor)
//! - Functions (function/endfunction)
//! - Built-in functions
//! - Mappings
//! - Autocommands
//! - Basic expressions

mod lexer;
mod parser;
mod value;
mod builtins;
mod environment;

pub use value::Value;
pub use environment::Environment;

use std::collections::HashMap;
use std::fs;
use std::io;

/// Vim Script interpreter
pub struct VimScript {
    /// Global environment
    pub env: Environment,
    /// User-defined functions
    functions: HashMap<String, Function>,
    /// Key mappings
    pub mappings: Mappings,
    /// Autocommands
    pub autocommands: Vec<AutoCommand>,
    /// Options
    pub options: Options,
}

/// A user-defined function
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<String>,
    pub is_abort: bool,
    pub is_range: bool,
}

/// Key mappings for different modes
#[derive(Debug, Clone, Default)]
pub struct Mappings {
    /// Normal mode mappings
    pub normal: HashMap<String, String>,
    /// Insert mode mappings
    pub insert: HashMap<String, String>,
    /// Visual mode mappings
    pub visual: HashMap<String, String>,
    /// Command-line mode mappings
    pub command: HashMap<String, String>,
    /// Operator-pending mode mappings
    pub operator: HashMap<String, String>,
}

/// An autocommand
#[derive(Debug, Clone)]
pub struct AutoCommand {
    pub event: String,
    pub pattern: String,
    pub command: String,
    pub group: Option<String>,
}

/// Editor options
#[derive(Debug, Clone)]
pub struct Options {
    /// Number of spaces for a tab
    pub tabstop: usize,
    /// Number of spaces for auto-indent
    pub shiftwidth: usize,
    /// Use spaces instead of tabs
    pub expandtab: bool,
    /// Auto-indent new lines
    pub autoindent: bool,
    /// Smart indent
    pub smartindent: bool,
    /// Show line numbers
    pub number: bool,
    /// Show relative line numbers
    pub relativenumber: bool,
    /// Highlight search matches
    pub hlsearch: bool,
    /// Incremental search
    pub incsearch: bool,
    /// Ignore case in search
    pub ignorecase: bool,
    /// Smart case (override ignorecase if pattern has uppercase)
    pub smartcase: bool,
    /// Wrap long lines
    pub wrap: bool,
    /// Show cursor line
    pub cursorline: bool,
    /// Show matching brackets
    pub showmatch: bool,
    /// Number of lines to keep above/below cursor
    pub scrolloff: usize,
    /// Command line height
    pub cmdheight: usize,
    /// Show mode in status line
    pub showmode: bool,
    /// Enable mouse support
    pub mouse: String,
    /// Color scheme
    pub colorscheme: String,
    /// File encoding
    pub encoding: String,
    /// File format
    pub fileformat: String,
    /// Clipboard setting
    pub clipboard: String,
    /// Backup files
    pub backup: bool,
    /// Swap files
    pub swapfile: bool,
    /// Undo file
    pub undofile: bool,
    /// Syntax highlighting
    pub syntax: bool,
    /// File type detection
    pub filetype: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            tabstop: 4,
            shiftwidth: 4,
            expandtab: true,
            autoindent: true,
            smartindent: true,
            number: true,
            relativenumber: false,
            hlsearch: true,
            incsearch: true,
            ignorecase: false,
            smartcase: true,
            wrap: true,
            cursorline: false,
            showmatch: true,
            scrolloff: 3,
            cmdheight: 1,
            showmode: true,
            mouse: String::from("a"),
            colorscheme: String::from("default"),
            encoding: String::from("utf-8"),
            fileformat: String::from("unix"),
            clipboard: String::new(),
            backup: false,
            swapfile: false,
            undofile: false,
            syntax: true,
            filetype: true,
        }
    }
}

impl VimScript {
    /// Create a new Vim Script interpreter
    pub fn new() -> Self {
        VimScript {
            env: Environment::new(),
            functions: HashMap::new(),
            mappings: Mappings::default(),
            autocommands: Vec::new(),
            options: Options::default(),
        }
    }
    
    /// Execute a Vim Script file
    pub fn source_file(&mut self, path: &str) -> io::Result<()> {
        let content = fs::read_to_string(path)?;
        self.execute(&content)?;
        Ok(())
    }
    
    /// Execute a string of Vim Script
    pub fn execute(&mut self, script: &str) -> io::Result<Value> {
        let lines: Vec<&str> = script.lines().collect();
        self.execute_lines(&lines, 0, lines.len())
    }
    
    /// Execute a range of lines
    fn execute_lines(&mut self, lines: &[&str], start: usize, end: usize) -> io::Result<Value> {
        let mut result = Value::Null;
        let mut i = start;
        
        while i < end {
            let line = lines[i].trim();
            
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('"') {
                i += 1;
                continue;
            }
            
            // Handle multi-line constructs
            if line.starts_with("if ") || line == "if" {
                let end_idx = self.find_matching_end(lines, i, "if", "endif")?;
                result = self.execute_if(lines, i, end_idx)?;
                i = end_idx + 1;
                continue;
            }
            
            if line.starts_with("while ") || line == "while" {
                let end_idx = self.find_matching_end(lines, i, "while", "endwhile")?;
                result = self.execute_while(lines, i, end_idx)?;
                i = end_idx + 1;
                continue;
            }
            
            if line.starts_with("for ") {
                let end_idx = self.find_matching_end(lines, i, "for", "endfor")?;
                result = self.execute_for(lines, i, end_idx)?;
                i = end_idx + 1;
                continue;
            }
            
            if line.starts_with("function") || line.starts_with("function!") {
                let end_idx = self.find_matching_end(lines, i, "function", "endfunction")?;
                self.define_function(lines, i, end_idx)?;
                i = end_idx + 1;
                continue;
            }
            
            if line.starts_with("try") {
                let end_idx = self.find_matching_end(lines, i, "try", "endtry")?;
                result = self.execute_try(lines, i, end_idx)?;
                i = end_idx + 1;
                continue;
            }
            
            // Single line commands
            result = self.execute_line(line)?;
            i += 1;
        }
        
        Ok(result)
    }
    
    /// Execute a single line of Vim Script
    pub fn execute_line(&mut self, line: &str) -> io::Result<Value> {
        let line = line.trim();
        
        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('"') {
            return Ok(Value::Null);
        }
        
        // Remove inline comments (be careful with strings)
        let line = self.remove_inline_comment(line);
        
        // Parse and execute command
        if let Some(rest) = line.strip_prefix("let ") {
            return self.execute_let(rest);
        }
        
        if let Some(rest) = line.strip_prefix("unlet ") {
            return self.execute_unlet(rest);
        }
        
        if let Some(rest) = line.strip_prefix("set ") {
            return self.execute_set(rest);
        }
        
        if let Some(rest) = line.strip_prefix("setlocal ") {
            return self.execute_set(rest);
        }
        
        if let Some(rest) = line.strip_prefix("echo ") {
            return self.execute_echo(rest, false);
        }
        
        if let Some(rest) = line.strip_prefix("echom ") {
            return self.execute_echo(rest, true);
        }
        
        if let Some(rest) = line.strip_prefix("echomsg ") {
            return self.execute_echo(rest, true);
        }
        
        if let Some(rest) = line.strip_prefix("echoerr ") {
            return self.execute_echoerr(rest);
        }
        
        if let Some(rest) = line.strip_prefix("call ") {
            return self.execute_call(rest);
        }
        
        if let Some(rest) = line.strip_prefix("execute ") {
            return self.execute_execute(rest);
        }
        
        if let Some(rest) = line.strip_prefix("source ") {
            return self.source_file(rest.trim());
        }
        
        if let Some(rest) = line.strip_prefix("return ") {
            return self.evaluate_expression(rest);
        }
        
        if line == "return" {
            return Ok(Value::Null);
        }
        
        // Mapping commands
        if self.is_map_command(line) {
            return self.execute_map(line);
        }
        
        // Autocommand
        if let Some(rest) = line.strip_prefix("autocmd ") {
            return self.execute_autocmd(rest);
        }
        
        if let Some(rest) = line.strip_prefix("augroup ") {
            // Augroup handling (simplified)
            let _ = rest;
            return Ok(Value::Null);
        }
        
        // Syntax and highlight commands
        if line.starts_with("syntax ") || line.starts_with("highlight ") ||
           line.starts_with("hi ") || line.starts_with("colorscheme ") {
            // Ignored for now
            return Ok(Value::Null);
        }
        
        // Filetype command
        if line.starts_with("filetype ") {
            return Ok(Value::Null);
        }
        
        // Command definition
        if line.starts_with("command ") || line.starts_with("command! ") {
            return Ok(Value::Null);
        }
        
        // Unknown command - try to evaluate as expression
        Ok(Value::Null)
    }
    
    /// Remove inline comments (respecting strings)
    fn remove_inline_comment(&self, line: &str) -> String {
        let mut result = String::new();
        let mut in_string = false;
        let mut string_char = '"';
        let mut prev_char = '\0';
        
        for ch in line.chars() {
            if !in_string {
                if ch == '"' && prev_char != '\\' {
                    // Check if this is a comment or string start
                    if result.trim().is_empty() || 
                       result.ends_with(' ') || 
                       result.ends_with('=') ||
                       result.ends_with('(') ||
                       result.ends_with(',') {
                        in_string = true;
                        string_char = '"';
                    } else {
                        // This is a comment
                        break;
                    }
                } else if ch == '\'' && prev_char != '\\' {
                    in_string = true;
                    string_char = '\'';
                }
            } else {
                if ch == string_char && prev_char != '\\' {
                    in_string = false;
                }
            }
            
            result.push(ch);
            prev_char = ch;
        }
        
        result.trim_end().to_string()
    }
    
    /// Execute let command
    fn execute_let(&mut self, rest: &str) -> io::Result<Value> {
        // Parse: varname = expression
        if let Some(eq_pos) = rest.find('=') {
            let var_name = rest[..eq_pos].trim();
            let expr = rest[eq_pos + 1..].trim();
            
            let value = self.evaluate_expression(expr)?;
            self.env.set(var_name, value.clone());
            
            return Ok(value);
        }
        
        Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid let syntax"))
    }
    
    /// Execute unlet command
    fn execute_unlet(&mut self, rest: &str) -> io::Result<Value> {
        let var_name = rest.trim().trim_start_matches('!').trim();
        self.env.unset(var_name);
        Ok(Value::Null)
    }
    
    /// Execute set command
    fn execute_set(&mut self, rest: &str) -> io::Result<Value> {
        for option in rest.split_whitespace() {
            self.set_option(option)?;
        }
        Ok(Value::Null)
    }
    
    /// Set a single option
    fn set_option(&mut self, option: &str) -> io::Result<()> {
        // Handle no<option> form
        if let Some(opt) = option.strip_prefix("no") {
            return self.set_bool_option(opt, false);
        }
        
        // Handle <option>=<value> form
        if let Some(eq_pos) = option.find('=') {
            let name = &option[..eq_pos];
            let value = &option[eq_pos + 1..];
            return self.set_value_option(name, value);
        }
        
        // Handle <option>:<value> form
        if let Some(colon_pos) = option.find(':') {
            let name = &option[..colon_pos];
            let value = &option[colon_pos + 1..];
            return self.set_value_option(name, value);
        }
        
        // Handle boolean option (set <option>)
        self.set_bool_option(option, true)
    }
    
    /// Set a boolean option
    fn set_bool_option(&mut self, name: &str, value: bool) -> io::Result<()> {
        match name {
            "number" | "nu" => self.options.number = value,
            "relativenumber" | "rnu" => self.options.relativenumber = value,
            "wrap" => self.options.wrap = value,
            "hlsearch" | "hls" => self.options.hlsearch = value,
            "incsearch" | "is" => self.options.incsearch = value,
            "ignorecase" | "ic" => self.options.ignorecase = value,
            "smartcase" | "scs" => self.options.smartcase = value,
            "expandtab" | "et" => self.options.expandtab = value,
            "autoindent" | "ai" => self.options.autoindent = value,
            "smartindent" | "si" => self.options.smartindent = value,
            "cursorline" | "cul" => self.options.cursorline = value,
            "showmatch" | "sm" => self.options.showmatch = value,
            "showmode" | "smd" => self.options.showmode = value,
            "backup" | "bk" => self.options.backup = value,
            "swapfile" | "swf" => self.options.swapfile = value,
            "undofile" | "udf" => self.options.undofile = value,
            "syntax" => self.options.syntax = value,
            "filetype" => self.options.filetype = value,
            _ => {} // Ignore unknown options
        }
        Ok(())
    }
    
    /// Set an option with a value
    fn set_value_option(&mut self, name: &str, value: &str) -> io::Result<()> {
        match name {
            "tabstop" | "ts" => {
                self.options.tabstop = value.parse().unwrap_or(4);
            }
            "shiftwidth" | "sw" => {
                self.options.shiftwidth = value.parse().unwrap_or(4);
            }
            "scrolloff" | "so" => {
                self.options.scrolloff = value.parse().unwrap_or(3);
            }
            "cmdheight" | "ch" => {
                self.options.cmdheight = value.parse().unwrap_or(1);
            }
            "mouse" => {
                self.options.mouse = value.to_string();
            }
            "colorscheme" => {
                self.options.colorscheme = value.to_string();
            }
            "encoding" | "enc" => {
                self.options.encoding = value.to_string();
            }
            "fileformat" | "ff" => {
                self.options.fileformat = value.to_string();
            }
            "clipboard" => {
                self.options.clipboard = value.to_string();
            }
            _ => {} // Ignore unknown options
        }
        Ok(())
    }
    
    /// Execute echo command
    fn execute_echo(&mut self, rest: &str, _save_message: bool) -> io::Result<Value> {
        let value = self.evaluate_expression(rest)?;
        println!("{}", value);
        Ok(value)
    }
    
    /// Execute echoerr command
    fn execute_echoerr(&mut self, rest: &str) -> io::Result<Value> {
        let value = self.evaluate_expression(rest)?;
        eprintln!("{}", value);
        Ok(value)
    }
    
    /// Execute call command
    fn execute_call(&mut self, rest: &str) -> io::Result<Value> {
        self.evaluate_expression(rest)
    }
    
    /// Execute execute command
    fn execute_execute(&mut self, rest: &str) -> io::Result<Value> {
        let value = self.evaluate_expression(rest)?;
        let cmd = value.to_string();
        self.execute_line(&cmd)
    }
    
    /// Check if line is a map command
    fn is_map_command(&self, line: &str) -> bool {
        let prefixes = [
            "map", "nmap", "imap", "vmap", "cmap", "omap",
            "noremap", "nnoremap", "inoremap", "vnoremap", "cnoremap", "onoremap",
            "unmap", "nunmap", "iunmap", "vunmap", "cunmap", "ounmap",
        ];
        
        for prefix in prefixes {
            if line.starts_with(prefix) && 
               (line.len() == prefix.len() || line[prefix.len()..].starts_with(' ')) {
                return true;
            }
        }
        false
    }
    
    /// Execute mapping command
    fn execute_map(&mut self, line: &str) -> io::Result<Value> {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        
        if parts.len() < 3 {
            return Ok(Value::Null);
        }
        
        let cmd = parts[0];
        let lhs = parts[1];
        let rhs = parts[2];
        
        // Determine which mapping table to use
        let is_noremap = cmd.contains("noremap");
        let mapping = if is_noremap {
            format!("<noremap>{}", rhs)
        } else {
            rhs.to_string()
        };
        
        match cmd {
            "map" | "noremap" => {
                self.mappings.normal.insert(lhs.to_string(), mapping.clone());
                self.mappings.visual.insert(lhs.to_string(), mapping.clone());
                self.mappings.operator.insert(lhs.to_string(), mapping);
            }
            "nmap" | "nnoremap" => {
                self.mappings.normal.insert(lhs.to_string(), mapping);
            }
            "imap" | "inoremap" => {
                self.mappings.insert.insert(lhs.to_string(), mapping);
            }
            "vmap" | "vnoremap" => {
                self.mappings.visual.insert(lhs.to_string(), mapping);
            }
            "cmap" | "cnoremap" => {
                self.mappings.command.insert(lhs.to_string(), mapping);
            }
            "omap" | "onoremap" => {
                self.mappings.operator.insert(lhs.to_string(), mapping);
            }
            _ if cmd.starts_with("un") => {
                // Handle unmap commands
                match cmd {
                    "unmap" => {
                        self.mappings.normal.remove(lhs);
                        self.mappings.visual.remove(lhs);
                        self.mappings.operator.remove(lhs);
                    }
                    "nunmap" => { self.mappings.normal.remove(lhs); }
                    "iunmap" => { self.mappings.insert.remove(lhs); }
                    "vunmap" => { self.mappings.visual.remove(lhs); }
                    "cunmap" => { self.mappings.command.remove(lhs); }
                    "ounmap" => { self.mappings.operator.remove(lhs); }
                    _ => {}
                }
            }
            _ => {}
        }
        
        Ok(Value::Null)
    }
    
    /// Execute autocmd command
    fn execute_autocmd(&mut self, rest: &str) -> io::Result<Value> {
        let parts: Vec<&str> = rest.splitn(3, ' ').collect();
        
        if parts.len() >= 3 {
            let event = parts[0].to_string();
            let pattern = parts[1].to_string();
            let command = parts[2].to_string();
            
            self.autocommands.push(AutoCommand {
                event,
                pattern,
                command,
                group: None,
            });
        }
        
        Ok(Value::Null)
    }
    
    /// Find matching end keyword
    fn find_matching_end(&self, lines: &[&str], start: usize, begin_kw: &str, end_kw: &str) -> io::Result<usize> {
        let mut depth = 1;
        
        for i in (start + 1)..lines.len() {
            let line = lines[i].trim();
            
            if line.starts_with(begin_kw) && 
               (line.len() == begin_kw.len() || line[begin_kw.len()..].starts_with(' ') ||
                line[begin_kw.len()..].starts_with('!')) {
                depth += 1;
            } else if line == end_kw || line.starts_with(&format!("{} ", end_kw)) {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
        }
        
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Missing {} for {} at line {}", end_kw, begin_kw, start + 1),
        ))
    }
    
    /// Execute if statement
    fn execute_if(&mut self, lines: &[&str], start: usize, end: usize) -> io::Result<Value> {
        let mut i = start;
        let mut executed = false;
        
        while i <= end {
            let line = lines[i].trim();
            
            if (line.starts_with("if ") || line.starts_with("elseif ")) && !executed {
                let condition_start = if line.starts_with("if ") { 3 } else { 7 };
                let condition = &line[condition_start..];
                
                let cond_value = self.evaluate_expression(condition)?;
                
                if cond_value.is_truthy() {
                    // Find the end of this branch
                    let branch_end = self.find_branch_end(lines, i + 1, end)?;
                    self.execute_lines(lines, i + 1, branch_end)?;
                    executed = true;
                }
            } else if line == "else" && !executed {
                // Execute else branch
                self.execute_lines(lines, i + 1, end)?;
                executed = true;
            }
            
            i += 1;
        }
        
        Ok(Value::Null)
    }
    
    /// Find the end of a branch (elseif, else, or endif)
    fn find_branch_end(&self, lines: &[&str], start: usize, max_end: usize) -> io::Result<usize> {
        let mut depth = 0;
        
        for i in start..=max_end {
            let line = lines[i].trim();
            
            if line.starts_with("if ") {
                depth += 1;
            } else if depth == 0 && (line.starts_with("elseif ") || line == "else" || line == "endif") {
                return Ok(i);
            } else if line == "endif" {
                depth -= 1;
            }
        }
        
        Ok(max_end)
    }
    
    /// Execute while loop
    fn execute_while(&mut self, lines: &[&str], start: usize, end: usize) -> io::Result<Value> {
        let condition_line = lines[start].trim();
        let condition = condition_line.strip_prefix("while ").unwrap_or("");
        
        let max_iterations = 10000; // Prevent infinite loops
        let mut iterations = 0;
        
        while iterations < max_iterations {
            let cond_value = self.evaluate_expression(condition)?;
            
            if !cond_value.is_truthy() {
                break;
            }
            
            match self.execute_lines(lines, start + 1, end) {
                Ok(_) => {}
                Err(e) if e.to_string() == "break" => break,
                Err(e) if e.to_string() == "continue" => {}
                Err(e) => return Err(e),
            }
            
            iterations += 1;
        }
        
        Ok(Value::Null)
    }
    
    /// Execute for loop
    fn execute_for(&mut self, lines: &[&str], start: usize, end: usize) -> io::Result<Value> {
        let header = lines[start].trim();
        let header = header.strip_prefix("for ").unwrap_or("");
        
        // Parse: var in list
        if let Some(in_pos) = header.find(" in ") {
            let var_name = header[..in_pos].trim();
            let list_expr = header[in_pos + 4..].trim();
            
            let list_value = self.evaluate_expression(list_expr)?;
            
            if let Value::List(items) = list_value {
                for item in items {
                    self.env.set(var_name, item);
                    
                    match self.execute_lines(lines, start + 1, end) {
                        Ok(_) => {}
                        Err(e) if e.to_string() == "break" => break,
                        Err(e) if e.to_string() == "continue" => {}
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        
        Ok(Value::Null)
    }
    
    /// Execute try/catch block
    fn execute_try(&mut self, lines: &[&str], start: usize, end: usize) -> io::Result<Value> {
        // Find catch and finally sections
        let mut catch_start = None;
        let mut finally_start = None;
        
        for i in (start + 1)..end {
            let line = lines[i].trim();
            if line.starts_with("catch") && catch_start.is_none() {
                catch_start = Some(i);
            } else if line.starts_with("finally") {
                finally_start = Some(i);
            }
        }
        
        let try_end = catch_start.or(finally_start).unwrap_or(end);
        
        // Execute try block
        let result = self.execute_lines(lines, start + 1, try_end);
        
        // Execute catch block if error
        if result.is_err() {
            if let Some(catch_idx) = catch_start {
                let catch_end = finally_start.unwrap_or(end);
                let _ = self.execute_lines(lines, catch_idx + 1, catch_end);
            }
        }
        
        // Execute finally block
        if let Some(finally_idx) = finally_start {
            let _ = self.execute_lines(lines, finally_idx + 1, end);
        }
        
        Ok(Value::Null)
    }
    
    /// Define a function
    fn define_function(&mut self, lines: &[&str], start: usize, end: usize) -> io::Result<()> {
        let header = lines[start].trim();
        let header = header.strip_prefix("function!").or_else(|| header.strip_prefix("function"))
            .unwrap_or("")
            .trim();
        
        // Parse function name and parameters
        if let Some(paren_start) = header.find('(') {
            let name = header[..paren_start].trim().to_string();
            let params_end = header.find(')').unwrap_or(header.len());
            let params_str = &header[paren_start + 1..params_end];
            
            let params: Vec<String> = params_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            
            // Check for special attributes
            let is_abort = header.contains("abort");
            let is_range = header.contains("range");
            
            // Collect function body
            let body: Vec<String> = lines[start + 1..end]
                .iter()
                .map(|s| s.to_string())
                .collect();
            
            self.functions.insert(name.clone(), Function {
                name,
                params,
                body,
                is_abort,
                is_range,
            });
        }
        
        Ok(())
    }
    
    /// Call a user-defined function
    pub fn call_function(&mut self, name: &str, args: Vec<Value>) -> io::Result<Value> {
        // Check built-in functions first
        if let Some(result) = builtins::call_builtin(name, &args) {
            return result;
        }
        
        // Look up user-defined function
        let func = self.functions.get(name).cloned();
        
        if let Some(func) = func {
            // Create new scope
            self.env.push_scope();
            
            // Bind parameters
            for (i, param) in func.params.iter().enumerate() {
                if param == "..." {
                    // Variadic arguments
                    let rest: Vec<Value> = args[i..].to_vec();
                    self.env.set("a:000", Value::List(rest));
                    break;
                }
                
                let value = args.get(i).cloned().unwrap_or(Value::Null);
                self.env.set(&format!("a:{}", param), value);
            }
            
            // Execute function body
            let body_lines: Vec<&str> = func.body.iter().map(|s| s.as_str()).collect();
            let result = self.execute_lines(&body_lines, 0, body_lines.len());
            
            // Pop scope
            self.env.pop_scope();
            
            return result;
        }
        
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Unknown function: {}", name),
        ))
    }
    
    /// Evaluate an expression
    pub fn evaluate_expression(&mut self, expr: &str) -> io::Result<Value> {
        let expr = expr.trim();
        
        if expr.is_empty() {
            return Ok(Value::Null);
        }
        
        // String literals
        if (expr.starts_with('"') && expr.ends_with('"')) ||
           (expr.starts_with('\'') && expr.ends_with('\'')) {
            let s = &expr[1..expr.len()-1];
            return Ok(Value::String(self.unescape_string(s)));
        }
        
        // Number literals
        if let Ok(n) = expr.parse::<i64>() {
            return Ok(Value::Integer(n));
        }
        if let Ok(n) = expr.parse::<f64>() {
            return Ok(Value::Float(n));
        }
        
        // Boolean literals
        if expr == "v:true" || expr == "1" {
            return Ok(Value::Integer(1));
        }
        if expr == "v:false" || expr == "0" {
            return Ok(Value::Integer(0));
        }
        
        // List literal
        if expr.starts_with('[') && expr.ends_with(']') {
            return self.parse_list(expr);
        }
        
        // Dictionary literal
        if expr.starts_with('{') && expr.ends_with('}') {
            return self.parse_dict(expr);
        }
        
        // Binary operators (in order of precedence)
        // Comparison operators
        for op in ["==", "!=", ">=", "<=", ">", "<", "=~", "!~", "is", "isnot"] {
            if let Some((left, right)) = self.split_binary_op(expr, op) {
                let left_val = self.evaluate_expression(left)?;
                let right_val = self.evaluate_expression(right)?;
                return Ok(self.apply_comparison(op, &left_val, &right_val));
            }
        }
        
        // Logical operators
        if let Some((left, right)) = self.split_binary_op(expr, "&&") {
            let left_val = self.evaluate_expression(left)?;
            if !left_val.is_truthy() {
                return Ok(Value::Integer(0));
            }
            let right_val = self.evaluate_expression(right)?;
            return Ok(if right_val.is_truthy() { Value::Integer(1) } else { Value::Integer(0) });
        }
        
        if let Some((left, right)) = self.split_binary_op(expr, "||") {
            let left_val = self.evaluate_expression(left)?;
            if left_val.is_truthy() {
                return Ok(Value::Integer(1));
            }
            let right_val = self.evaluate_expression(right)?;
            return Ok(if right_val.is_truthy() { Value::Integer(1) } else { Value::Integer(0) });
        }
        
        // String concatenation
        if let Some((left, right)) = self.split_binary_op(expr, ".") {
            let left_val = self.evaluate_expression(left)?;
            let right_val = self.evaluate_expression(right)?;
            return Ok(Value::String(format!("{}{}", left_val, right_val)));
        }
        
        // Arithmetic operators
        for op in ["+", "-", "*", "/", "%"] {
            if let Some((left, right)) = self.split_binary_op(expr, op) {
                let left_val = self.evaluate_expression(left)?;
                let right_val = self.evaluate_expression(right)?;
                return self.apply_arithmetic(op, &left_val, &right_val);
            }
        }
        
        // Ternary operator
        if let Some((condition, rest)) = self.split_ternary(expr) {
            let cond_val = self.evaluate_expression(condition)?;
            if let Some((true_expr, false_expr)) = self.split_binary_op(rest, ":") {
                return if cond_val.is_truthy() {
                    self.evaluate_expression(true_expr)
                } else {
                    self.evaluate_expression(false_expr)
                };
            }
        }
        
        // Unary operators
        if let Some(rest) = expr.strip_prefix('!') {
            let val = self.evaluate_expression(rest)?;
            return Ok(if val.is_truthy() { Value::Integer(0) } else { Value::Integer(1) });
        }
        
        if let Some(rest) = expr.strip_prefix('-') {
            let val = self.evaluate_expression(rest)?;
            return match val {
                Value::Integer(n) => Ok(Value::Integer(-n)),
                Value::Float(n) => Ok(Value::Float(-n)),
                _ => Ok(Value::Integer(0)),
            };
        }
        
        // Function call
        if let Some(paren_start) = expr.find('(') {
            if expr.ends_with(')') {
                let func_name = expr[..paren_start].trim();
                let args_str = &expr[paren_start + 1..expr.len() - 1];
                let args = self.parse_function_args(args_str)?;
                return self.call_function(func_name, args);
            }
        }
        
        // Variable reference
        self.env.get(expr).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("Undefined variable: {}", expr))
        })
    }
    
    /// Split expression on binary operator
    fn split_binary_op<'a>(&self, expr: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
        let mut depth = 0;
        let mut in_string = false;
        let mut string_char = '"';
        let bytes = expr.as_bytes();
        
        for i in 0..expr.len() {
            if in_string {
                if bytes[i] == string_char as u8 && (i == 0 || bytes[i-1] != b'\\') {
                    in_string = false;
                }
                continue;
            }
            
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                in_string = true;
                string_char = bytes[i] as char;
                continue;
            }
            
            if bytes[i] == b'(' || bytes[i] == b'[' || bytes[i] == b'{' {
                depth += 1;
            } else if bytes[i] == b')' || bytes[i] == b']' || bytes[i] == b'}' {
                depth -= 1;
            } else if depth == 0 && expr[i..].starts_with(op) {
                // Check that it's not part of a larger operator
                let before_ok = i == 0 || !is_operator_char(bytes[i-1]);
                let after_ok = i + op.len() >= expr.len() || !is_operator_char(bytes[i + op.len()]);
                
                if before_ok && after_ok {
                    return Some((expr[..i].trim(), expr[i + op.len()..].trim()));
                }
            }
        }
        
        None
    }
    
    /// Split ternary expression
    fn split_ternary<'a>(&self, expr: &'a str) -> Option<(&'a str, &'a str)> {
        let mut depth = 0;
        let mut in_string = false;
        let bytes = expr.as_bytes();
        
        for i in 0..expr.len() {
            if in_string {
                if bytes[i] == b'"' && (i == 0 || bytes[i-1] != b'\\') {
                    in_string = false;
                }
                continue;
            }
            
            if bytes[i] == b'"' {
                in_string = true;
                continue;
            }
            
            if bytes[i] == b'(' || bytes[i] == b'[' || bytes[i] == b'{' {
                depth += 1;
            } else if bytes[i] == b')' || bytes[i] == b']' || bytes[i] == b'}' {
                depth -= 1;
            } else if depth == 0 && bytes[i] == b'?' {
                return Some((expr[..i].trim(), expr[i + 1..].trim()));
            }
        }
        
        None
    }
    
    /// Apply comparison operator
    fn apply_comparison(&self, op: &str, left: &Value, right: &Value) -> Value {
        let result = match op {
            "==" => left == right,
            "!=" => left != right,
            ">" => left.compare(right) > 0,
            "<" => left.compare(right) < 0,
            ">=" => left.compare(right) >= 0,
            "<=" => left.compare(right) <= 0,
            "is" => left == right,
            "isnot" => left != right,
            "=~" => {
                // Regex match (simplified)
                let s = left.to_string();
                let pattern = right.to_string();
                s.contains(&pattern)
            }
            "!~" => {
                let s = left.to_string();
                let pattern = right.to_string();
                !s.contains(&pattern)
            }
            _ => false,
        };
        
        Value::Integer(if result { 1 } else { 0 })
    }
    
    /// Apply arithmetic operator
    fn apply_arithmetic(&self, op: &str, left: &Value, right: &Value) -> io::Result<Value> {
        let (l, r) = match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => {
                return Ok(Value::Integer(match op {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "/" => if *b != 0 { a / b } else { 0 },
                    "%" => if *b != 0 { a % b } else { 0 },
                    _ => 0,
                }));
            }
            (Value::Float(a), Value::Float(b)) => (*a, *b),
            (Value::Integer(a), Value::Float(b)) => (*a as f64, *b),
            (Value::Float(a), Value::Integer(b)) => (*a, *b as f64),
            _ => (left.to_float(), right.to_float()),
        };
        
        Ok(Value::Float(match op {
            "+" => l + r,
            "-" => l - r,
            "*" => l * r,
            "/" => if r != 0.0 { l / r } else { 0.0 },
            "%" => if r != 0.0 { l % r } else { 0.0 },
            _ => 0.0,
        }))
    }
    
    /// Parse a list literal
    fn parse_list(&mut self, expr: &str) -> io::Result<Value> {
        let inner = &expr[1..expr.len()-1];
        if inner.trim().is_empty() {
            return Ok(Value::List(Vec::new()));
        }
        
        let items = self.split_list_items(inner)?;
        let mut result = Vec::new();
        
        for item in items {
            result.push(self.evaluate_expression(item.trim())?);
        }
        
        Ok(Value::List(result))
    }
    
    /// Parse a dictionary literal
    fn parse_dict(&mut self, expr: &str) -> io::Result<Value> {
        let inner = &expr[1..expr.len()-1];
        if inner.trim().is_empty() {
            return Ok(Value::Dict(HashMap::new()));
        }
        
        let items = self.split_list_items(inner)?;
        let mut result = HashMap::new();
        
        for item in items {
            if let Some(colon_pos) = item.find(':') {
                let key = self.evaluate_expression(item[..colon_pos].trim())?;
                let value = self.evaluate_expression(item[colon_pos + 1..].trim())?;
                result.insert(key.to_string(), value);
            }
        }
        
        Ok(Value::Dict(result))
    }
    
    /// Split list items (respecting nested structures)
    fn split_list_items<'a>(&self, s: &'a str) -> io::Result<Vec<&'a str>> {
        let mut items = Vec::new();
        let mut depth = 0;
        let mut start = 0;
        let mut in_string = false;
        let bytes = s.as_bytes();
        
        for i in 0..s.len() {
            if in_string {
                if bytes[i] == b'"' && (i == 0 || bytes[i-1] != b'\\') {
                    in_string = false;
                }
                continue;
            }
            
            match bytes[i] {
                b'"' => in_string = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    items.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        
        if start < s.len() {
            items.push(&s[start..]);
        }
        
        Ok(items)
    }
    
    /// Parse function arguments
    fn parse_function_args(&mut self, args_str: &str) -> io::Result<Vec<Value>> {
        if args_str.trim().is_empty() {
            return Ok(Vec::new());
        }
        
        let items = self.split_list_items(args_str)?;
        let mut result = Vec::new();
        
        for item in items {
            result.push(self.evaluate_expression(item.trim())?);
        }
        
        Ok(result)
    }
    
    /// Unescape string
    fn unescape_string(&self, s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                if let Some(&next) = chars.peek() {
                    result.push(match next {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '\\' => '\\',
                        '"' => '"',
                        '\'' => '\'',
                        _ => {
                            result.push('\\');
                            next
                        }
                    });
                    chars.next();
                } else {
                    result.push('\\');
                }
            } else {
                result.push(ch);
            }
        }
        
        result
    }
    
    /// Trigger autocommands for an event
    pub fn trigger_autocmd(&mut self, event: &str, filename: &str) {
        let matching: Vec<AutoCommand> = self.autocommands
            .iter()
            .filter(|ac| ac.event.eq_ignore_ascii_case(event))
            .filter(|ac| self.pattern_matches(&ac.pattern, filename))
            .cloned()
            .collect();
        
        for ac in matching {
            let _ = self.execute_line(&ac.command);
        }
    }
    
    /// Check if pattern matches filename
    fn pattern_matches(&self, pattern: &str, filename: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        
        // Simple glob matching
        if pattern.starts_with("*.") {
            let ext = &pattern[2..];
            return filename.ends_with(&format!(".{}", ext));
        }
        
        filename == pattern
    }
}

impl Default for VimScript {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if byte is an operator character
fn is_operator_char(b: u8) -> bool {
    matches!(b, b'=' | b'!' | b'<' | b'>' | b'&' | b'|' | b'+' | b'-' | b'*' | b'/' | b'%' | b'.')
}
