//! Shell Command Executor
//!
//! Executes parsed commands including control flow structures

use crate::parser::Command;
use crate::state::ShellState;
use crate::builtins::BuiltinRegistry;
use std::io::{self, Write};
use std::string::String;
use std::vec::Vec;

// Syscall numbers
const SYS_READ: u64 = 0;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAITPID: u64 = 61;
const SYS_PIPE: u64 = 22;
const SYS_DUP2: u64 = 33;
const SYS_CLOSE: u64 = 3;

#[inline(always)]
fn syscall0(n: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "int 0x81",
            inout("rax") n => ret,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
fn syscall1(n: u64, arg1: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "int 0x81",
            inout("rax") n => ret,
            in("rdi") arg1,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
fn syscall2(n: u64, arg1: u64, arg2: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "int 0x81",
            inout("rax") n => ret,
            in("rdi") arg1,
            in("rsi") arg2,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
fn syscall3(n: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "int 0x81",
            inout("rax") n => ret,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    ret
}

/// Shell executor
pub struct Executor<'a> {
    state: &'a mut ShellState,
    registry: &'a BuiltinRegistry,
    last_status: i32,
}

impl<'a> Executor<'a> {
    pub fn new(state: &'a mut ShellState, registry: &'a BuiltinRegistry) -> Self {
        Self {
            state,
            registry,
            last_status: 0,
        }
    }

    /// Execute a list of commands
    pub fn execute(&mut self, commands: &[Command]) -> i32 {
        for cmd in commands {
            self.last_status = self.execute_command(cmd);
        }
        self.last_status
    }

    /// Execute a single command
    pub fn execute_command(&mut self, cmd: &Command) -> i32 {
        match cmd {
            Command::Simple(args) => self.execute_simple(args),
            Command::Pipeline(cmds) => self.execute_pipeline(cmds),
            Command::AndList(cmds) => self.execute_and_list(cmds),
            Command::OrList(cmds) => self.execute_or_list(cmds),
            Command::Background(cmd) => self.execute_background(cmd),
            Command::Subshell(cmd) => self.execute_subshell(cmd),
            Command::BraceGroup(cmds) => self.execute(cmds),
            Command::If { condition, then_part, elif_parts, else_part } => {
                self.execute_if(condition, then_part, elif_parts, else_part)
            }
            Command::Case { word, cases } => self.execute_case(word, cases),
            Command::For { var, words, body } => self.execute_for(var, words, body),
            Command::While { condition, body } => self.execute_while(condition, body),
            Command::Until { condition, body } => self.execute_until(condition, body),
            Command::Select { var, words, body } => self.execute_select(var, words, body),
            Command::Function { name, body } => self.execute_function_def(name, body),
            Command::Arithmetic(expr) => self.execute_arithmetic(expr),
            Command::Conditional(args) => self.execute_conditional(args),
            Command::Empty => 0,
        }
    }

    /// Execute simple command (builtin or external)
    fn execute_simple(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 0;
        }

        // Expand variables
        let expanded: Vec<String> = args.iter()
            .map(|a| self.expand_string(a))
            .collect();

        let cmd = &expanded[0];
        let cmd_args: Vec<&str> = expanded.iter().map(|s| s.as_str()).collect();

        // Check for function first
        if let Some(func_body) = self.state.get_function(cmd).map(|s| s.to_string()) {
            // Save old positional params
            let old_params: Vec<String> = self.state.positional_params_at()
                .iter().map(|s| s.to_string()).collect();
            
            // Set new positional params from arguments
            self.state.set_positional_params(expanded[1..].to_vec());
            
            // Parse and execute function body
            let result = match crate::parser::parse_command(&func_body) {
                Ok(cmds) => self.execute(&cmds),
                Err(e) => {
                    eprintln!("{}: 函数执行错误: {}", cmd, e);
                    1
                }
            };
            
            // Restore positional params
            self.state.set_positional_params(old_params);
            return result;
        }

        // Check for builtin
        if let Some(result) = self.registry.execute(cmd, self.state, &cmd_args[1..]) {
            let code = result.unwrap_or(1);
            self.state.set_var("?", &code.to_string());
            return code;
        }

        // External command
        self.execute_external(&expanded)
    }

    /// Execute external command
    fn execute_external(&mut self, args: &[String]) -> i32 {
        let path = self.find_executable(&args[0]);
        if path.is_none() {
            eprintln!("{}: 命令未找到", args[0]);
            return 127;
        }
        let path = path.unwrap();

        // Fork and exec
        let pid = syscall0(SYS_FORK);
        if pid < 0 {
            eprintln!("fork 失败");
            return 1;
        }

        if pid == 0 {
            // Child process
            let path_cstr = format!("{}\0", path);
            
            // Prepare argv
            let argv_strs: Vec<String> = args.iter()
                .map(|s| format!("{}\0", s))
                .collect();
            let argv_ptrs: Vec<*const u8> = argv_strs.iter()
                .map(|s| s.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect();

            // Prepare envp from exported variables
            let env_strs: Vec<String> = self.state.exported_vars().iter()
                .map(|(k, v)| format!("{}={}\0", k, v))
                .collect();
            let envp_ptrs: Vec<*const u8> = env_strs.iter()
                .map(|s| s.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect();

            let ret = syscall3(
                SYS_EXECVE,
                path_cstr.as_ptr() as u64,
                argv_ptrs.as_ptr() as u64,
                envp_ptrs.as_ptr() as u64
            );

            if ret < 0 {
                eprintln!("{}: 执行失败", args[0]);
            }
            syscall1(SYS_EXIT, 127);
            unreachable!()
        } else {
            // Parent - wait for child
            let mut status: i32 = 0;
            syscall3(SYS_WAITPID, pid as u64, &mut status as *mut i32 as u64, 0);
            
            // Extract exit code (lower 8 bits if normal exit)
            let exit_code = if status & 0x7f == 0 {
                (status >> 8) & 0xff
            } else {
                128 + (status & 0x7f)
            };
            
            self.state.set_var("?", &exit_code.to_string());
            exit_code
        }
    }

    /// Find executable in PATH
    fn find_executable(&self, cmd: &str) -> Option<String> {
        // Absolute or relative path
        if cmd.contains('/') {
            return Some(cmd.to_string());
        }

        // Search PATH
        let path = self.state.get_var("PATH").unwrap_or("/bin:/usr/bin");
        for dir in path.split(':') {
            let full = format!("{}/{}", dir, cmd);
            // Simple existence check via open
            let full_cstr = format!("{}\0", full);
            let fd = syscall3(
                5, // SYS_OPEN
                full_cstr.as_ptr() as u64,
                0, // O_RDONLY
                0
            );
            if fd >= 0 {
                syscall1(SYS_CLOSE, fd as u64);
                return Some(full);
            }
        }

        None
    }

    /// Execute pipeline
    fn execute_pipeline(&mut self, cmds: &[Command]) -> i32 {
        if cmds.len() == 1 {
            return self.execute_command(&cmds[0]);
        }

        let mut prev_read_fd: i32 = -1;
        let mut last_status = 0;
        let mut pids = Vec::new();

        for (i, cmd) in cmds.iter().enumerate() {
            let is_last = i == cmds.len() - 1;
            
            // Create pipe (except for last command)
            let mut pipe_fds = [0i32; 2];
            if !is_last {
                let ret = syscall1(SYS_PIPE, pipe_fds.as_mut_ptr() as u64);
                if ret < 0 {
                    eprintln!("pipe 失败");
                    return 1;
                }
            }

            let pid = syscall0(SYS_FORK);
            if pid < 0 {
                eprintln!("fork 失败");
                return 1;
            }

            if pid == 0 {
                // Child
                if prev_read_fd != -1 {
                    syscall2(SYS_DUP2, prev_read_fd as u64, 0); // stdin
                    syscall1(SYS_CLOSE, prev_read_fd as u64);
                }
                if !is_last {
                    syscall1(SYS_CLOSE, pipe_fds[0] as u64);
                    syscall2(SYS_DUP2, pipe_fds[1] as u64, 1); // stdout
                    syscall1(SYS_CLOSE, pipe_fds[1] as u64);
                }
                
                let status = self.execute_command(cmd);
                syscall1(SYS_EXIT, status as u64);
                unreachable!()
            } else {
                // Parent
                pids.push(pid);
                
                if prev_read_fd != -1 {
                    syscall1(SYS_CLOSE, prev_read_fd as u64);
                }
                if !is_last {
                    syscall1(SYS_CLOSE, pipe_fds[1] as u64);
                    prev_read_fd = pipe_fds[0];
                }
            }
        }

        // Wait for all children
        for pid in pids {
            let mut status: i32 = 0;
            syscall3(SYS_WAITPID, pid as u64, &mut status as *mut i32 as u64, 0);
            last_status = if status & 0x7f == 0 {
                (status >> 8) & 0xff
            } else {
                128 + (status & 0x7f)
            };
        }

        last_status
    }

    /// Execute && list
    fn execute_and_list(&mut self, cmds: &[Command]) -> i32 {
        for cmd in cmds {
            self.last_status = self.execute_command(cmd);
            if self.last_status != 0 {
                return self.last_status;
            }
        }
        0
    }

    /// Execute || list
    fn execute_or_list(&mut self, cmds: &[Command]) -> i32 {
        for cmd in cmds {
            self.last_status = self.execute_command(cmd);
            if self.last_status == 0 {
                return 0;
            }
        }
        self.last_status
    }

    /// Execute background command
    fn execute_background(&mut self, cmd: &Command) -> i32 {
        let pid = syscall0(SYS_FORK);
        if pid < 0 {
            eprintln!("fork 失败");
            return 1;
        }

        if pid == 0 {
            // Child - execute command and exit
            let status = self.execute_command(cmd);
            syscall1(SYS_EXIT, status as u64);
            unreachable!()
        } else {
            // Parent - record background job
            self.state.set_last_bg_pid(pid as i32);
            self.state.set_var("!", &pid.to_string());
            let job_count = self.state.list_jobs().len();
            println!("[{}] {}", job_count + 1, pid);
            0
        }
    }

    /// Execute subshell
    fn execute_subshell(&mut self, cmd: &Command) -> i32 {
        let pid = syscall0(SYS_FORK);
        if pid < 0 {
            eprintln!("fork 失败");
            return 1;
        }

        if pid == 0 {
            // Child - execute in subshell
            let status = self.execute_command(cmd);
            syscall1(SYS_EXIT, status as u64);
            unreachable!()
        } else {
            // Parent - wait
            let mut status: i32 = 0;
            syscall3(SYS_WAITPID, pid as u64, &mut status as *mut i32 as u64, 0);
            if status & 0x7f == 0 {
                (status >> 8) & 0xff
            } else {
                128 + (status & 0x7f)
            }
        }
    }

    /// Execute if statement
    fn execute_if(&mut self, condition: &[Command], then_part: &[Command], 
                  elif_parts: &[(Vec<Command>, Vec<Command>)], 
                  else_part: &Option<Vec<Command>>) -> i32 {
        // Evaluate main condition
        let cond_result = self.execute(condition);
        
        if cond_result == 0 {
            return self.execute(then_part);
        }

        // Check elif clauses
        for (elif_cond, elif_then) in elif_parts {
            let elif_result = self.execute(elif_cond);
            if elif_result == 0 {
                return self.execute(elif_then);
            }
        }

        // Execute else part
        if let Some(else_cmds) = else_part {
            return self.execute(else_cmds);
        }

        0
    }

    /// Execute case statement
    fn execute_case(&mut self, word: &str, cases: &[(Vec<String>, Vec<Command>)]) -> i32 {
        let expanded_word = self.expand_string(word);

        for (patterns, cmds) in cases {
            for pattern in patterns {
                let expanded_pattern = self.expand_string(pattern);
                if self.glob_match(&expanded_pattern, &expanded_word) {
                    return self.execute(cmds);
                }
            }
        }

        0
    }

    /// Simple glob matching (supports * and ?)
    fn glob_match(&self, pattern: &str, text: &str) -> bool {
        let mut p = pattern.chars().peekable();
        let mut t = text.chars().peekable();

        while let Some(pc) = p.next() {
            match pc {
                '*' => {
                    if p.peek().is_none() {
                        return true;
                    }
                    let remaining: String = p.collect();
                    let text_remaining: String = t.collect();
                    for i in 0..=text_remaining.len() {
                        if self.glob_match(&remaining, &text_remaining[i..]) {
                            return true;
                        }
                    }
                    return false;
                }
                '?' => {
                    if t.next().is_none() {
                        return false;
                    }
                }
                _ => {
                    if t.next() != Some(pc) {
                        return false;
                    }
                }
            }
        }

        t.next().is_none()
    }

    /// Execute for loop
    fn execute_for(&mut self, var: &str, words: &[String], body: &[Command]) -> i32 {
        let expanded_words: Vec<String> = words.iter()
            .flat_map(|w| {
                let expanded = self.expand_string(w);
                if w == "\"$@\"" || w == "$@" {
                    self.state.positional_params_at().iter().map(|s| s.to_string()).collect::<Vec<String>>()
                } else {
                    expanded.split_whitespace().map(String::from).collect::<Vec<String>>()
                }
            })
            .collect();

        let mut last_status = 0;
        for word in expanded_words {
            self.state.set_var(var, &word);
            last_status = self.execute(body);
        }

        last_status
    }

    /// Execute while loop
    fn execute_while(&mut self, condition: &[Command], body: &[Command]) -> i32 {
        let mut last_status = 0;
        
        loop {
            let cond_result = self.execute(condition);
            if cond_result != 0 {
                break;
            }
            last_status = self.execute(body);
        }

        last_status
    }

    /// Execute until loop
    fn execute_until(&mut self, condition: &[Command], body: &[Command]) -> i32 {
        let mut last_status = 0;
        
        loop {
            let cond_result = self.execute(condition);
            if cond_result == 0 {
                break;
            }
            last_status = self.execute(body);
        }

        last_status
    }

    /// Execute select menu
    fn execute_select(&mut self, var: &str, words: &[String], body: &[Command]) -> i32 {
        let expanded_words: Vec<String> = words.iter()
            .map(|w| self.expand_string(w))
            .collect();

        loop {
            for (i, word) in expanded_words.iter().enumerate() {
                println!("{}) {}", i + 1, word);
            }
            
            let ps3 = self.state.get_var("PS3").unwrap_or("#? ").to_string();
            print!("{}", ps3);
            io::stdout().flush().ok();

            let mut buf = [0u8; 256];
            let n = syscall3(SYS_READ, 0, buf.as_mut_ptr() as u64, 256) as usize;
            if n == 0 {
                break;
            }
            let input = String::from_utf8_lossy(&buf[..n]).trim().to_string();

            if input.is_empty() {
                continue;
            }

            if let Ok(num) = input.parse::<usize>() {
                if num > 0 && num <= expanded_words.len() {
                    self.state.set_var(var, &expanded_words[num - 1]);
                    self.state.set_var("REPLY", &input);
                    self.execute(body);
                }
            }
        }

        0
    }

    /// Define a function
    fn execute_function_def(&mut self, name: &str, body: &[Command]) -> i32 {
        let body_str = self.serialize_commands(body);
        self.state.define_function(name, &body_str);
        0
    }

    fn serialize_commands(&self, cmds: &[Command]) -> String {
        let mut result = String::new();
        for cmd in cmds {
            if !result.is_empty() {
                result.push_str("; ");
            }
            result.push_str(&self.serialize_command(cmd));
        }
        result
    }

    fn serialize_command(&self, cmd: &Command) -> String {
        match cmd {
            Command::Simple(args) => args.join(" "),
            Command::Pipeline(cmds) => cmds.iter().map(|c| self.serialize_command(c)).collect::<Vec<_>>().join(" | "),
            Command::AndList(cmds) => cmds.iter().map(|c| self.serialize_command(c)).collect::<Vec<_>>().join(" && "),
            Command::OrList(cmds) => cmds.iter().map(|c| self.serialize_command(c)).collect::<Vec<_>>().join(" || "),
            Command::Background(cmd) => format!("{} &", self.serialize_command(cmd)),
            Command::Subshell(cmd) => format!("( {} )", self.serialize_command(cmd)),
            Command::BraceGroup(cmds) => format!("{{ {}; }}", self.serialize_commands(cmds)),
            Command::If { condition, then_part, elif_parts, else_part } => {
                let mut s = format!("if {}; then {}", self.serialize_commands(condition), self.serialize_commands(then_part));
                for (cond, then_) in elif_parts {
                    s.push_str(&format!("; elif {}; then {}", self.serialize_commands(cond), self.serialize_commands(then_)));
                }
                if let Some(else_cmds) = else_part {
                    s.push_str(&format!("; else {}", self.serialize_commands(else_cmds)));
                }
                s.push_str("; fi");
                s
            }
            Command::For { var, words, body } => format!("for {} in {}; do {}; done", var, words.join(" "), self.serialize_commands(body)),
            Command::While { condition, body } => format!("while {}; do {}; done", self.serialize_commands(condition), self.serialize_commands(body)),
            Command::Until { condition, body } => format!("until {}; do {}; done", self.serialize_commands(condition), self.serialize_commands(body)),
            Command::Case { word, cases } => {
                let mut s = format!("case {} in", word);
                for (patterns, cmds) in cases {
                    s.push_str(&format!(" {}) {};; ", patterns.join("|"), self.serialize_commands(cmds)));
                }
                s.push_str("esac");
                s
            }
            Command::Select { var, words, body } => format!("select {} in {}; do {}; done", var, words.join(" "), self.serialize_commands(body)),
            Command::Function { name, body } => format!("function {} {{ {}; }}", name, self.serialize_commands(body)),
            Command::Arithmetic(expr) => format!("(( {} ))", expr),
            Command::Conditional(args) => format!("[[ {} ]]", args.join(" ")),
            Command::Empty => String::new(),
        }
    }

    /// Execute arithmetic expression
    fn execute_arithmetic(&mut self, expr: &str) -> i32 {
        let expanded = self.expand_string(expr);
        match self.evaluate_arithmetic(&expanded) {
            Ok(result) => if result == 0 { 1 } else { 0 },
            Err(e) => {
                eprintln!("(( )): {}", e);
                1
            }
        }
    }

    fn evaluate_arithmetic(&self, expr: &str) -> Result<i64, String> {
        let expr = expr.trim();
        if expr.is_empty() {
            return Ok(0);
        }
        
        // Handle comparison operators
        if let Some(pos) = expr.rfind("==") {
            let left = self.evaluate_arithmetic(&expr[..pos])?;
            let right = self.evaluate_arithmetic(&expr[pos + 2..])?;
            return Ok(if left == right { 1 } else { 0 });
        }
        if let Some(pos) = expr.rfind("!=") {
            let left = self.evaluate_arithmetic(&expr[..pos])?;
            let right = self.evaluate_arithmetic(&expr[pos + 2..])?;
            return Ok(if left != right { 1 } else { 0 });
        }

        // Handle + and -
        let mut depth = 0;
        let chars: Vec<char> = expr.chars().collect();
        for i in (0..chars.len()).rev() {
            match chars[i] {
                ')' => depth += 1,
                '(' => depth -= 1,
                '+' | '-' if depth == 0 && i > 0 => {
                    let prev = chars.get(i - 1);
                    if prev.map(|p| !matches!(p, '*' | '/' | '%' | '+' | '-' | '(' | ' ')).unwrap_or(false) {
                        let left = self.evaluate_arithmetic(&expr[..i])?;
                        let right = self.evaluate_arithmetic(&expr[i + 1..])?;
                        return Ok(if chars[i] == '+' { left + right } else { left - right });
                    }
                }
                _ => {}
            }
        }

        // Handle * / %
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                ')' => depth += 1,
                '(' => depth -= 1,
                '*' if depth == 0 && i > 0 => {
                    let left = self.evaluate_arithmetic(&expr[..i])?;
                    let right = self.evaluate_arithmetic(&expr[i + 1..])?;
                    return Ok(left * right);
                }
                '/' if depth == 0 && i > 0 => {
                    let left = self.evaluate_arithmetic(&expr[..i])?;
                    let right = self.evaluate_arithmetic(&expr[i + 1..])?;
                    if right == 0 { return Err("除以零".to_string()); }
                    return Ok(left / right);
                }
                '%' if depth == 0 && i > 0 => {
                    let left = self.evaluate_arithmetic(&expr[..i])?;
                    let right = self.evaluate_arithmetic(&expr[i + 1..])?;
                    if right == 0 { return Err("除以零".to_string()); }
                    return Ok(left % right);
                }
                _ => {}
            }
        }

        // Handle parentheses
        let trimmed = expr.trim();
        if trimmed.starts_with('(') && trimmed.ends_with(')') {
            return self.evaluate_arithmetic(&trimmed[1..trimmed.len() - 1]);
        }

        // Handle unary
        if trimmed.starts_with('-') {
            return Ok(-self.evaluate_arithmetic(&trimmed[1..])?);
        }
        if trimmed.starts_with('+') {
            return self.evaluate_arithmetic(&trimmed[1..]);
        }

        // Handle variable
        if trimmed.chars().all(|c| c.is_alphanumeric() || c == '_') && !trimmed.chars().next().map(|c| c.is_numeric()).unwrap_or(true) {
            if let Some(val) = self.state.get_var(trimmed) {
                return val.parse().map_err(|_| format!("无效数字: {}", val));
            }
            return Ok(0);
        }

        trimmed.parse().map_err(|_| format!("无效表达式: {}", trimmed))
    }

    fn execute_conditional(&mut self, args: &[String]) -> i32 {
        let expanded: Vec<String> = args.iter().map(|a| self.expand_string(a)).collect();
        if expanded.is_empty() { return 1; }
        self.evaluate_conditional(&expanded)
    }

    fn evaluate_conditional(&self, args: &[String]) -> i32 {
        if args.is_empty() { return 1; }

        if args.len() >= 2 {
            let op = &args[0];
            let arg = &args[1];
            
            match op.as_str() {
                "-z" => return if arg.is_empty() { 0 } else { 1 },
                "-n" => return if !arg.is_empty() { 0 } else { 1 },
                "-e" | "-a" => return if self.file_exists(arg) { 0 } else { 1 },
                "-f" => return if self.file_exists(arg) && !arg.ends_with('/') { 0 } else { 1 },
                "-d" => return if self.is_directory(arg) { 0 } else { 1 },
                "!" => return if self.evaluate_conditional(&args[1..]) != 0 { 0 } else { 1 },
                _ => {}
            }
        }

        if args.len() >= 3 {
            let left = &args[0];
            let op = &args[1];
            let right = &args[2];

            match op.as_str() {
                "=" | "==" => return if left == right { 0 } else { 1 },
                "!=" => return if left != right { 0 } else { 1 },
                "-eq" => {
                    let l: i64 = left.parse().unwrap_or(0);
                    let r: i64 = right.parse().unwrap_or(0);
                    return if l == r { 0 } else { 1 };
                }
                "-ne" => {
                    let l: i64 = left.parse().unwrap_or(0);
                    let r: i64 = right.parse().unwrap_or(0);
                    return if l != r { 0 } else { 1 };
                }
                "-lt" => {
                    let l: i64 = left.parse().unwrap_or(0);
                    let r: i64 = right.parse().unwrap_or(0);
                    return if l < r { 0 } else { 1 };
                }
                "-gt" => {
                    let l: i64 = left.parse().unwrap_or(0);
                    let r: i64 = right.parse().unwrap_or(0);
                    return if l > r { 0 } else { 1 };
                }
                _ => {}
            }
        }

        if args.len() == 1 {
            return if args[0].is_empty() { 1 } else { 0 };
        }

        1
    }

    fn file_exists(&self, path: &str) -> bool {
        let path_cstr = format!("{}\0", path);
        let fd = syscall3(5, path_cstr.as_ptr() as u64, 0, 0);
        if fd >= 0 {
            syscall1(SYS_CLOSE, fd as u64);
            true
        } else {
            false
        }
    }

    fn is_directory(&self, path: &str) -> bool {
        let path_cstr = format!("{}\0", path);
        let fd = syscall3(5, path_cstr.as_ptr() as u64, 0o200000, 0);
        if fd >= 0 {
            syscall1(SYS_CLOSE, fd as u64);
            true
        } else {
            false
        }
    }

    pub fn expand_string(&self, s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        let mut in_single_quote = false;
        let mut in_double_quote = false;

        while let Some(c) = chars.next() {
            match c {
                '\'' if !in_double_quote => in_single_quote = !in_single_quote,
                '"' if !in_single_quote => in_double_quote = !in_double_quote,
                '$' if !in_single_quote => {
                    let expanded = self.expand_variable(&mut chars);
                    result.push_str(&expanded);
                }
                '\\' if !in_single_quote => {
                    if let Some(next) = chars.next() {
                        if in_double_quote {
                            match next {
                                '$' | '`' | '"' | '\\' | '\n' => result.push(next),
                                _ => { result.push('\\'); result.push(next); }
                            }
                        } else {
                            result.push(next);
                        }
                    }
                }
                '~' if !in_single_quote && !in_double_quote && result.is_empty() => {
                    result.push_str(self.state.get_var("HOME").unwrap_or("/root"));
                }
                _ => result.push(c),
            }
        }

        result
    }

    fn expand_variable(&self, chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        match chars.peek() {
            Some('{') => {
                chars.next();
                self.expand_braced_variable(chars)
            }
            Some(c) if c.is_alphabetic() || *c == '_' => {
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                self.state.get_var(&name).unwrap_or("").to_string()
            }
            Some('?') => { chars.next(); self.state.get_var("?").unwrap_or("0").to_string() }
            Some('$') => { chars.next(); "1".to_string() }
            Some('!') => { chars.next(); self.state.get_var("!").unwrap_or("0").to_string() }
            Some('#') => { chars.next(); self.state.positional_param_count().to_string() }
            Some('*') => { chars.next(); self.state.positional_params_star() }
            Some('@') => { chars.next(); self.state.positional_params_star() }
            Some('0') => { chars.next(); self.state.shell_name().to_string() }
            Some(c) if c.is_numeric() => {
                let n = c.to_digit(10).unwrap() as usize;
                chars.next();
                self.state.get_positional_param(n).unwrap_or("").to_string()
            }
            _ => "$".to_string(),
        }
    }

    fn expand_braced_variable(&self, chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut content = String::new();
        let mut depth = 1;

        while let Some(c) = chars.next() {
            match c {
                '{' => { depth += 1; content.push(c); }
                '}' => {
                    depth -= 1;
                    if depth == 0 { break; }
                    content.push(c);
                }
                _ => content.push(c),
            }
        }

        if content.starts_with('#') {
            let var_name = &content[1..];
            return self.state.get_var(var_name).map(|v| v.len().to_string()).unwrap_or_else(|| "0".to_string());
        }

        self.state.get_var(&content).unwrap_or("").to_string()
    }
}
