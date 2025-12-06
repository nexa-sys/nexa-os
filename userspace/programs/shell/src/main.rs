//! NexaOS Shell - A POSIX-compatible command-line shell
//!
//! This shell uses Rust std functionality for clean, idiomatic code.
//! NexaOS-specific syscalls are used only where std cannot provide the functionality.
//!
//! Features a modular builtin command system with 35+ bash-compatible builtins.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

mod builtins;
mod state;

use builtins::BuiltinRegistry;
use state::{normalize_path, ShellState};

// ============================================================================
// NexaOS-specific syscalls (only what shell needs internally)
// ============================================================================

mod nexaos {
    use std::arch::asm;

    const SYS_LIST_FILES: u64 = 200;
    const SYS_GETERRNO: u64 = 201;
    const SYS_USER_INFO: u64 = 222;

    const LIST_FLAG_INCLUDE_HIDDEN: u64 = 0x1;

    #[inline(always)]
    fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
        let ret: u64;
        unsafe {
            asm!(
                "int 0x81",
                in("rax") n,
                in("rdi") a1,
                in("rsi") a2,
                in("rdx") a3,
                lateout("rax") ret,
                clobber_abi("sysv64")
            );
        }
        ret
    }

    #[inline(always)]
    fn syscall1(n: u64, a1: u64) -> u64 {
        syscall3(n, a1, 0, 0)
    }

    fn errno() -> i32 {
        syscall1(SYS_GETERRNO, 0) as i32
    }

    #[repr(C)]
    struct ListDirRequest {
        path_ptr: u64,
        path_len: u64,
        flags: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct UserInfo {
        pub username: [u8; 32],
        pub username_len: u64,
        pub uid: u32,
        pub gid: u32,
        pub is_admin: u32,
    }

    /// List files in a directory (for tab completion)
    pub fn list_files(path: Option<&str>, include_hidden: bool) -> Result<String, i32> {
        let mut request = ListDirRequest {
            path_ptr: 0,
            path_len: 0,
            flags: if include_hidden { LIST_FLAG_INCLUDE_HIDDEN } else { 0 },
        };

        if let Some(p) = path {
            if p != "/" {
                request.path_ptr = p.as_ptr() as u64;
                request.path_len = p.len() as u64;
            }
        }

        let req_ptr = if request.path_len == 0 && request.flags == 0 {
            0
        } else {
            &request as *const ListDirRequest as u64
        };

        let mut buf = vec![0u8; 4096];
        let written = syscall3(SYS_LIST_FILES, buf.as_mut_ptr() as u64, buf.len() as u64, req_ptr);
        
        if written == u64::MAX {
            return Err(errno());
        }

        buf.truncate(written as usize);
        String::from_utf8(buf).map_err(|_| -1)
    }

    /// Get current user info (for prompt display)
    pub fn get_user_info() -> Option<UserInfo> {
        let mut info = UserInfo::default();
        let ret = syscall3(SYS_USER_INFO, &mut info as *mut UserInfo as u64, 0, 0);
        if ret != u64::MAX { Some(info) } else { None }
    }
}

// ============================================================================
// Shell Configuration
// ============================================================================

const HOSTNAME: &str = "nexa";
const SEARCH_PATHS: &[&str] = &["/bin", "/sbin", "/usr/bin", "/usr/sbin"];

// External commands (for tab completion hints)
const EXTERNAL_COMMANDS: &[&str] = &[
    "ls", "cat", "stat", "pwd", "uname", "mkdir", "clear", "whoami", "users",
    "login", "logout", "adduser",  // User management
    "ipc-create", "ipc-send", "ipc-recv",  // IPC utilities
];

// ============================================================================
// Terminal Input Handling
// ============================================================================

struct LineEditor {
    buffer: String,
    stdout: io::Stdout,
}

impl LineEditor {
    fn new() -> Self {
        Self {
            buffer: String::with_capacity(256),
            stdout: io::stdout(),
        }
    }

    fn beep(&mut self) {
        let _ = self.stdout.write_all(b"\x07");
        let _ = self.stdout.flush();
    }

    fn write(&mut self, data: &[u8]) {
        let _ = self.stdout.write_all(data);
        let _ = self.stdout.flush();
    }

    fn erase_char(&mut self) -> bool {
        if self.buffer.pop().is_some() {
            self.write(b"\x08 \x08");
            true
        } else {
            false
        }
    }

    fn erase_word(&mut self) {
        // Remove trailing whitespace
        while self.buffer.ends_with(char::is_whitespace) {
            if !self.erase_char() { return; }
        }
        // Remove word
        while !self.buffer.is_empty() && !self.buffer.ends_with(char::is_whitespace) {
            if !self.erase_char() { break; }
        }
    }

    fn clear_line(&mut self) {
        while self.erase_char() {}
    }

    fn append(&mut self, s: &str) {
        self.buffer.push_str(s);
        self.write(s.as_bytes());
    }

    fn read_line(&mut self, state: &ShellState, registry: &BuiltinRegistry) -> String {
        self.buffer.clear();
        print_prompt(state);

        // Use raw byte reading for terminal control
        let stdin = io::stdin();
        let mut stdin_lock = stdin.lock();
        let mut byte_buf = [0u8; 1];

        loop {
            if stdin_lock.read(&mut byte_buf).unwrap_or(0) != 1 {
                continue;
            }
            let ch = byte_buf[0];

            match ch {
                b'\r' | b'\n' => {
                    println!();
                    return std::mem::take(&mut self.buffer);
                }
                0x03 => { // Ctrl-C
                    self.buffer.clear();
                    self.write(b"^C\n");
                    return String::new();
                }
                0x04 => { // Ctrl-D
                    if self.buffer.is_empty() {
                        println!("exit");
                        process::exit(0);
                    } else {
                        self.beep();
                    }
                }
                0x08 | 0x7f => { // Backspace
                    if !self.erase_char() {
                        self.beep();
                    }
                }
                b'\t' => { // Tab completion
                    self.handle_tab_completion(state, registry);
                }
                0x15 => { // Ctrl-U
                    self.clear_line();
                }
                0x17 => { // Ctrl-W
                    self.erase_word();
                }
                0x0c => { // Ctrl-L
                    clear_screen();
                    print_prompt(state);
                    let buf_copy = self.buffer.clone();
                    self.write(buf_copy.as_bytes());
                }
                0x1b => { // Escape sequence
                    self.discard_escape_sequence(&mut stdin_lock);
                }
                ch if ch < 0x20 => {
                    self.beep();
                }
                _ => {
                    self.buffer.push(ch as char);
                    self.write(&[ch]);
                }
            }
        }
    }

    fn discard_escape_sequence(&mut self, stdin: &mut io::StdinLock) {
        let mut buf = [0u8; 1];
        for _ in 0..4 {
            if stdin.read(&mut buf).unwrap_or(0) != 1 {
                break;
            }
            if (0x40..=0x7e).contains(&buf[0]) {
                break;
            }
        }
    }

    fn handle_tab_completion(&mut self, state: &ShellState, registry: &BuiltinRegistry) {
        let parts: Vec<String> = self.buffer.split_whitespace().map(String::from).collect();
        let has_trailing_space = self.buffer.ends_with(' ');

        if parts.is_empty() || (parts.len() == 1 && !has_trailing_space) {
            // Complete command name
            let prefix = parts.first().map(|s| s.as_str()).unwrap_or("").to_string();
            self.complete_command(&prefix, state, registry);
        } else {
            // Complete path argument
            let cmd = parts[0].clone();
            let prefix = if has_trailing_space { 
                String::new() 
            } else { 
                parts.last().cloned().unwrap_or_default()
            };
            
            if command_accepts_path(&cmd) {
                self.complete_path(&prefix, state);
            } else {
                self.beep();
            }
        }
    }

    fn complete_command(&mut self, prefix: &str, state: &ShellState, registry: &BuiltinRegistry) {
        // Collect all available commands (builtins + external)
        let builtin_names = registry.list_builtins();
        let mut all_commands: Vec<&str> = builtin_names
            .iter()
            .map(|s| s.as_str())
            .collect();
        all_commands.extend(EXTERNAL_COMMANDS.iter().copied());
        
        // Also add executables from PATH directories
        for dir in SEARCH_PATHS {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        // Leak to get 'static lifetime (acceptable for shell process lifetime)
                        all_commands.push(Box::leak(name.to_string().into_boxed_str()));
                    }
                }
            }
        }
        
        let matches: Vec<&str> = all_commands.iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .copied()
            .collect();

        self.apply_completion(&matches, prefix, state);
    }

    fn complete_path(&mut self, prefix: &str, state: &ShellState) {
        let (dir_part, name_prefix) = if let Some(idx) = prefix.rfind('/') {
            (&prefix[..=idx], &prefix[idx + 1..])
        } else {
            ("", prefix)
        };

        let directory = if prefix.starts_with('/') {
            if dir_part.is_empty() { PathBuf::from("/") } else { normalize_path(Path::new(dir_part)) }
        } else if dir_part.is_empty() {
            state.cwd().to_path_buf()
        } else {
            state.resolve_path(dir_part)
        };

        let show_hidden = name_prefix.starts_with('.');
        
        // Use NexaOS syscall for directory listing
        let list = match nexaos::list_files(Some(directory.to_str().unwrap_or("/")), show_hidden) {
            Ok(l) => l,
            Err(_) => {
                self.beep();
                return;
            }
        };

        let mut matches: Vec<String> = Vec::new();
        let mut is_dir: Vec<bool> = Vec::new();

        for entry in list.lines() {
            if entry.is_empty() { continue; }
            if !show_hidden && entry.starts_with('.') { continue; }
            if name_prefix.is_empty() && (entry == "." || entry == "..") { continue; }
            
            if entry.starts_with(name_prefix) {
                let full_path = directory.join(entry);
                let dir = fs::metadata(&full_path).map(|m| m.is_dir()).unwrap_or(false);
                matches.push(entry.to_string());
                is_dir.push(dir);
            }
        }

        if matches.is_empty() {
            self.beep();
            return;
        }

        if matches.len() == 1 {
            let suffix = &matches[0][name_prefix.len()..];
            self.append(suffix);
            if is_dir[0] {
                self.append("/");
            } else {
                self.append(" ");
            }
            return;
        }

        // Find longest common prefix
        let lcp = longest_common_prefix(&matches);
        if lcp.len() > name_prefix.len() {
            self.append(&lcp[name_prefix.len()..]);
            return;
        }

        // Show all matches
        println!();
        for (i, entry) in matches.iter().enumerate() {
            print!("{}{}{}", dir_part, entry, if is_dir[i] { "/" } else { "" });
            println!();
        }
        print_prompt(state);
        let buf_copy = self.buffer.clone();
        self.write(buf_copy.as_bytes());
    }

    fn apply_completion(&mut self, matches: &[&str], prefix: &str, state: &ShellState) {
        if matches.is_empty() {
            self.beep();
            return;
        }

        if matches.len() == 1 {
            let suffix = &matches[0][prefix.len()..];
            self.append(suffix);
            self.append(" ");
            return;
        }

        let lcp = longest_common_prefix_str(matches);
        if lcp.len() > prefix.len() {
            self.append(&lcp[prefix.len()..]);
            return;
        }

        // Show all matches
        println!();
        for m in matches {
            println!("{}", m);
        }
        print_prompt(state);
        let buf_copy = self.buffer.clone();
        self.write(buf_copy.as_bytes());
    }
}

fn command_accepts_path(cmd: &str) -> bool {
    matches!(cmd, "ls" | "cat" | "stat" | "cd" | "mkdir" | "source" | ".")
}

fn longest_common_prefix(items: &[String]) -> String {
    if items.is_empty() { return String::new(); }
    let first = &items[0];
    let mut len = first.len();
    for item in &items[1..] {
        len = first.chars().zip(item.chars())
            .take_while(|(a, b)| a == b)
            .count()
            .min(len);
        if len == 0 { break; }
    }
    first[..len].to_string()
}

fn longest_common_prefix_str(items: &[&str]) -> String {
    if items.is_empty() { return String::new(); }
    let first = items[0];
    let mut len = first.len();
    for item in &items[1..] {
        len = first.chars().zip(item.chars())
            .take_while(|(a, b)| a == b)
            .count()
            .min(len);
        if len == 0 { break; }
    }
    first[..len].to_string()
}

// ============================================================================
// Output Helpers
// ============================================================================

fn print_prompt(state: &ShellState) {
    let username = nexaos::get_user_info()
        .and_then(|info| {
            let len = info.username_len as usize;
            if len == 0 { None }
            else { std::str::from_utf8(&info.username[..len]).ok().map(String::from) }
        })
        .unwrap_or_else(|| "anonymous".to_string());

    print!("{}@{}:{}$ ", username, HOSTNAME, state.cwd_str());
    let _ = io::stdout().flush();
}

fn clear_screen() {
    print!("\x1b[2J\x1b[H");
    let _ = io::stdout().flush();
}

// ============================================================================
// Shell Built-in Commands
// ============================================================================

// All builtin commands are now in the `builtins` module.
// See builtins/mod.rs for the registry and the various submodules:
// - builtins/navigation.rs: cd, pwd, pushd, popd, dirs
// - builtins/info.rs: help, type, hash, enable
// - builtins/variables.rs: export, unset, set, declare, readonly, alias, unalias, local
// - builtins/flow.rs: exit, return, break, continue, test, [, true, false, :
// - builtins/utility.rs: echo, printf, source, ., eval, exec, command, builtin, read

// ============================================================================
// External Command Execution
// ============================================================================

fn find_executable(cmd: &str) -> Option<PathBuf> {
    for dir in SEARCH_PATHS {
        let path = Path::new(dir).join(cmd);
        if fs::metadata(&path).is_ok() {
            return Some(path);
        }
    }
    None
}

fn execute_external_command(state: &ShellState, cmd: &str, args: &[&str]) -> i32 {
    let path = match find_executable(cmd) {
        Some(p) => p,
        None => {
            eprintln!("{}: 未找到命令", cmd);
            return 127;
        }
    };

    // Set current directory for the child process
    match Command::new(&path)
        .args(args)
        .current_dir(state.cwd())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
    {
        Ok(status) => {
            status.code().unwrap_or(1)
        }
        Err(e) => {
            eprintln!("执行 '{}' 失败: {}", cmd, e);
            126
        }
    }
}

// ============================================================================
// Command Dispatcher
// ============================================================================

fn handle_command(state: &mut ShellState, registry: &BuiltinRegistry, line: &str) {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let cmd = match parts.first() {
        Some(c) => *c,
        None => return,
    };
    let args: Vec<&str> = parts[1..].to_vec();

    // Check for alias expansion - clone the alias value to avoid borrowing issues
    let expanded = state.get_alias(cmd).map(|s| s.to_string());
    let (actual_cmd, actual_args): (&str, Vec<&str>) = if let Some(alias_value) = &expanded {
        // Expand alias
        let alias_parts: Vec<&str> = alias_value.split_whitespace().collect();
        if alias_parts.is_empty() {
            (cmd, args.clone())
        } else {
            let mut new_args = alias_parts[1..].to_vec();
            new_args.extend(args.iter().copied());
            (alias_parts[0], new_args)
        }
    } else {
        (cmd, args)
    };

    // Try builtin first
    if let Some(result) = registry.execute(actual_cmd, state, &actual_args) {
        match result {
            Ok(code) => {
                state.last_exit_status = code;
            }
            Err(e) => {
                // Check for special error codes
                if e.starts_with("HELP_PATTERN:") {
                    // Handle help for specific commands
                    let patterns: Vec<&str> = e[13..].split(',').collect();
                    for pattern in patterns {
                        if let Some(builtin) = registry.get(pattern) {
                            println!("{}: {}", pattern, builtin.usage);
                            println!("    {}", builtin.long_desc.replace('\n', "\n    "));
                        } else {
                            eprintln!("help: 没有与 `{}' 匹配的帮助主题", pattern);
                        }
                    }
                    state.last_exit_status = 0;
                } else if e.starts_with("BUILTIN_EXEC:") {
                    // Handle builtin command execution
                    let inner_cmd = &e[13..];
                    let inner_parts: Vec<&str> = inner_cmd.split_whitespace().collect();
                    if !inner_parts.is_empty() {
                        if let Some(result) = registry.execute(inner_parts[0], state, &inner_parts[1..]) {
                            state.last_exit_status = result.unwrap_or_else(|e| {
                                eprintln!("{}", e);
                                1
                            });
                        }
                    }
                } else {
                    eprintln!("{}", e);
                    state.last_exit_status = 1;
                }
            }
        }
    } else {
        // Not a builtin, try external command
        state.last_exit_status = execute_external_command(state, actual_cmd, &actual_args);
    }
}

// ============================================================================
// Main Entry Point
// ============================================================================

fn main() -> ! {
    println!("欢迎使用 NexaOS Shell。输入 'help' 获取命令列表。");
    
    let mut state = ShellState::new();
    let registry = BuiltinRegistry::new();
    let mut editor = LineEditor::new();

    loop {
        let line = editor.read_line(&state, &registry);
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            handle_command(&mut state, &registry, trimmed);
        }
    }
}
