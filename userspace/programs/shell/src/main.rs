//! NexaOS Shell - A simple command-line shell
//!
//! This shell uses Rust std functionality for clean, idiomatic code.
//! NexaOS-specific syscalls are used only where std cannot provide the functionality.
//!
//! Most commands (ls, cat, pwd, etc.) are now external programs in /bin.
//! Only shell-specific builtins (cd, exit, help) remain internal.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

// ============================================================================
// NexaOS-specific syscalls (not available in std)
// ============================================================================

mod nexaos {
    use std::arch::asm;

    pub const SYS_LIST_FILES: u64 = 200;
    pub const SYS_GETERRNO: u64 = 201;
    pub const SYS_IPC_CREATE: u64 = 210;
    pub const SYS_IPC_SEND: u64 = 211;
    pub const SYS_IPC_RECV: u64 = 212;
    pub const SYS_USER_ADD: u64 = 220;
    pub const SYS_USER_LOGIN: u64 = 221;
    pub const SYS_USER_INFO: u64 = 222;
    pub const SYS_USER_LIST: u64 = 223;
    pub const SYS_USER_LOGOUT: u64 = 224;

    pub const LIST_FLAG_INCLUDE_HIDDEN: u64 = 0x1;
    pub const USER_FLAG_ADMIN: u64 = 0x1;

    #[inline(always)]
    pub fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
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
    pub fn syscall1(n: u64, a1: u64) -> u64 {
        syscall3(n, a1, 0, 0)
    }

    #[inline(always)]
    pub fn syscall0(n: u64) -> u64 {
        syscall3(n, 0, 0, 0)
    }

    pub fn errno() -> i32 {
        syscall1(SYS_GETERRNO, 0) as i32
    }

    #[repr(C)]
    pub struct ListDirRequest {
        pub path_ptr: u64,
        pub path_len: u64,
        pub flags: u64,
    }

    #[repr(C)]
    pub struct UserRequest {
        pub username_ptr: u64,
        pub username_len: u64,
        pub password_ptr: u64,
        pub password_len: u64,
        pub flags: u64,
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

    #[repr(C)]
    pub struct IpcTransferRequest {
        pub channel_id: u32,
        pub flags: u32,
        pub buffer_ptr: u64,
        pub buffer_len: u64,
    }

    /// List files in a directory using NexaOS syscall
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

    /// Get current user info
    pub fn get_user_info() -> Option<UserInfo> {
        let mut info = UserInfo::default();
        let ret = syscall3(SYS_USER_INFO, &mut info as *mut UserInfo as u64, 0, 0);
        if ret != u64::MAX { Some(info) } else { None }
    }

    /// Login user
    pub fn login(username: &str, password: &str) -> Result<(), i32> {
        let request = UserRequest {
            username_ptr: username.as_ptr() as u64,
            username_len: username.len() as u64,
            password_ptr: password.as_ptr() as u64,
            password_len: password.len() as u64,
            flags: 0,
        };
        let ret = syscall3(SYS_USER_LOGIN, &request as *const UserRequest as u64, 0, 0);
        if ret == u64::MAX { Err(errno()) } else { Ok(()) }
    }

    /// Add user
    pub fn add_user(username: &str, password: &str, admin: bool) -> Result<(), i32> {
        let request = UserRequest {
            username_ptr: username.as_ptr() as u64,
            username_len: username.len() as u64,
            password_ptr: password.as_ptr() as u64,
            password_len: password.len() as u64,
            flags: if admin { USER_FLAG_ADMIN } else { 0 },
        };
        let ret = syscall3(SYS_USER_ADD, &request as *const UserRequest as u64, 0, 0);
        if ret == u64::MAX { Err(errno()) } else { Ok(()) }
    }

    /// Logout user
    pub fn logout() -> Result<(), i32> {
        let ret = syscall1(SYS_USER_LOGOUT, 0);
        if ret == u64::MAX { Err(errno()) } else { Ok(()) }
    }

    /// List all users
    pub fn list_users() -> Result<String, i32> {
        let mut buffer = vec![0u8; 512];
        let written = syscall3(SYS_USER_LIST, buffer.as_mut_ptr() as u64, buffer.len() as u64, 0);
        if written == u64::MAX {
            return Err(errno());
        }
        buffer.truncate(written as usize);
        String::from_utf8(buffer).map_err(|_| -1)
    }

    /// Create IPC channel
    pub fn ipc_create() -> Result<u64, i32> {
        let id = syscall0(SYS_IPC_CREATE);
        if id == u64::MAX { Err(errno()) } else { Ok(id) }
    }

    /// Send IPC message
    pub fn ipc_send(channel: u32, message: &str) -> Result<(), i32> {
        let request = IpcTransferRequest {
            channel_id: channel,
            flags: 0,
            buffer_ptr: message.as_ptr() as u64,
            buffer_len: message.len() as u64,
        };
        let ret = syscall3(SYS_IPC_SEND, &request as *const IpcTransferRequest as u64, 0, 0);
        if ret == u64::MAX { Err(errno()) } else { Ok(()) }
    }

    /// Receive IPC message
    pub fn ipc_recv(channel: u32) -> Result<String, i32> {
        let mut buffer = vec![0u8; 256];
        let request = IpcTransferRequest {
            channel_id: channel,
            flags: 0,
            buffer_ptr: buffer.as_mut_ptr() as u64,
            buffer_len: buffer.len() as u64,
        };
        let ret = syscall3(SYS_IPC_RECV, &request as *const IpcTransferRequest as u64, 0, 0);
        if ret == u64::MAX {
            return Err(errno());
        }
        buffer.truncate(ret as usize);
        String::from_utf8(buffer).map_err(|_| -1)
    }
}

// ============================================================================
// Shell Configuration
// ============================================================================

const HOSTNAME: &str = "nexa";
const SEARCH_PATHS: &[&str] = &["/bin", "/sbin", "/usr/bin", "/usr/sbin"];

// Shell builtins: commands that must be handled internally
// (cd changes shell state, exit terminates shell, help shows shell help)
const SHELL_BUILTINS: &[&str] = &[
    "cd", "exit", "help",
    // User management (requires shell's syscall interface)
    "login", "logout", "adduser",
    // IPC commands (shell-specific)
    "ipc-create", "ipc-send", "ipc-recv",
];

// External commands that were moved out of the shell
const EXTERNAL_COMMANDS: &[&str] = &[
    "ls", "cat", "stat", "pwd", "echo", "uname", "mkdir", "clear", "whoami", "users",
];

// ============================================================================
// Shell State
// ============================================================================

struct ShellState {
    cwd: PathBuf,
}

impl ShellState {
    fn new() -> Self {
        Self {
            cwd: PathBuf::from("/"),
        }
    }

    fn current_path(&self) -> &Path {
        &self.cwd
    }

    fn current_path_str(&self) -> &str {
        self.cwd.to_str().unwrap_or("/")
    }

    fn set_path(&mut self, path: impl AsRef<Path>) {
        self.cwd = path.as_ref().to_path_buf();
    }

    /// Resolve a path relative to cwd
    fn resolve(&self, input: &str) -> PathBuf {
        if input.starts_with('/') {
            normalize_path(Path::new(input))
        } else {
            normalize_path(&self.cwd.join(input))
        }
    }
}

/// Normalize a path by resolving . and ..
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            c => components.push(c),
        }
    }
    
    if components.is_empty() {
        PathBuf::from("/")
    } else {
        components.iter().collect()
    }
}

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

    fn read_line(&mut self, state: &ShellState) -> String {
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
                    self.handle_tab_completion(state);
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

    fn handle_tab_completion(&mut self, state: &ShellState) {
        let parts: Vec<String> = self.buffer.split_whitespace().map(String::from).collect();
        let has_trailing_space = self.buffer.ends_with(' ');

        if parts.is_empty() || (parts.len() == 1 && !has_trailing_space) {
            // Complete command name
            let prefix = parts.first().map(|s| s.as_str()).unwrap_or("").to_string();
            self.complete_command(&prefix, state);
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

    fn complete_command(&mut self, prefix: &str, state: &ShellState) {
        // Collect all available commands (builtins + external)
        let mut all_commands: Vec<&str> = SHELL_BUILTINS.to_vec();
        all_commands.extend(EXTERNAL_COMMANDS.iter().copied());
        
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
            state.cwd.clone()
        } else {
            state.resolve(dir_part)
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
    matches!(cmd, "ls" | "cat" | "stat" | "cd" | "mkdir")
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

    print!("{}@{}:{}$ ", username, HOSTNAME, state.current_path_str());
    let _ = io::stdout().flush();
}

fn clear_screen() {
    print!("\x1b[2J\x1b[H");
    let _ = io::stdout().flush();
}

// ============================================================================
// Shell Built-in Commands
// ============================================================================

fn cmd_help() {
    println!("NexaOS Shell, version 0.1.0");
    println!("These shell commands are defined internally. Type `help' to see this list.");
    println!();
    println!("  help              Show this message");
    println!("  cd [dir]          Change directory");
    println!("  exit [n]          Exit the shell");
    println!("  login <user>      Switch active user");
    println!("  logout            Log out current user");
    println!("  adduser [-a] <u>  Create a new user (-a for admin)");
    println!("  ipc-create        Allocate IPC channel");
    println!("  ipc-send <c> <m>  Send IPC message");
    println!("  ipc-recv <c>      Receive IPC message");
}

fn cmd_cd(state: &mut ShellState, path: Option<&str>) {
    let target = path.unwrap_or("/");
    let resolved = state.resolve(target);
    
    match fs::metadata(&resolved) {
        Ok(meta) if meta.is_dir() => {
            state.set_path(resolved);
        }
        Ok(_) => {
            println!("cd: not a directory: {}", target);
        }
        Err(e) => {
            println!("cd: {}: {}", target, e);
        }
    }
}

fn cmd_login(username: &str) {
    if username.is_empty() {
        println!("login: missing user name");
        return;
    }

    print!("password: ");
    let _ = io::stdout().flush();
    
    let mut password = String::new();
    if io::stdin().read_line(&mut password).is_err() {
        println!("login: failed to read password");
        return;
    }
    let password = password.trim();

    match nexaos::login(username, password) {
        Ok(()) => println!("login successful"),
        Err(e) => println!("login failed (errno {})", e),
    }
}

fn cmd_adduser(username: &str, admin: bool) {
    if username.is_empty() {
        println!("adduser: missing user name");
        return;
    }

    print!("new password: ");
    let _ = io::stdout().flush();
    
    let mut password = String::new();
    if io::stdin().read_line(&mut password).is_err() {
        println!("adduser: failed to read password");
        return;
    }
    let password = password.trim();

    match nexaos::add_user(username, password, admin) {
        Ok(()) => println!("adduser: user created"),
        Err(e) => println!("adduser: failed (errno {})", e),
    }
}

fn cmd_logout() {
    match nexaos::logout() {
        Ok(()) => println!("logged out"),
        Err(e) => println!("logout: failed (errno {})", e),
    }
}

fn cmd_ipc_create() {
    match nexaos::ipc_create() {
        Ok(id) => println!("channel {} created", id),
        Err(e) => println!("ipc-create: failed (errno {})", e),
    }
}

fn cmd_ipc_send(channel: u32, message: &str) {
    if message.is_empty() {
        println!("ipc-send: message cannot be empty");
        return;
    }
    match nexaos::ipc_send(channel, message) {
        Ok(()) => println!("ipc-send: message queued"),
        Err(e) => println!("ipc-send: failed (errno {})", e),
    }
}

fn cmd_ipc_recv(channel: u32) {
    match nexaos::ipc_recv(channel) {
        Ok(msg) => println!("ipc-recv: {}", msg),
        Err(e) => println!("ipc-recv: failed (errno {})", e),
    }
}

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

fn execute_external_command(state: &ShellState, cmd: &str, args: &[&str]) -> bool {
    let path = match find_executable(cmd) {
        Some(p) => p,
        None => {
            println!("Command not found: {}", cmd);
            return false;
        }
    };

    // Set current directory for the child process
    match Command::new(&path)
        .args(args)
        .current_dir(state.current_path())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
    {
        Ok(status) => {
            if !status.success() {
                if let Some(code) = status.code() {
                    if code != 0 {
                        // Only print non-zero exit codes
                        println!("Command exited with status {}", code);
                    }
                } else {
                    println!("Command terminated by signal");
                }
            }
            true
        }
        Err(e) => {
            println!("Failed to execute '{}': {}", cmd, e);
            false
        }
    }
}

// ============================================================================
// Command Dispatcher
// ============================================================================

fn handle_command(state: &mut ShellState, line: &str) {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let cmd = match parts.first() {
        Some(c) => *c,
        None => return,
    };
    let args = &parts[1..];

    // Check for shell builtins first
    match cmd {
        "help" => cmd_help(),
        "cd" => cmd_cd(state, args.first().copied()),
        "exit" => {
            println!("Bye!");
            process::exit(0);
        }
        
        // User management builtins
        "login" => {
            if let Some(user) = args.first() {
                cmd_login(user);
            } else {
                println!("login: missing user name");
            }
        }
        "logout" => cmd_logout(),
        "adduser" => {
            let mut admin = false;
            let mut username = None;
            for arg in args {
                if *arg == "-a" {
                    admin = true;
                } else {
                    username = Some(*arg);
                }
            }
            if let Some(user) = username {
                cmd_adduser(user, admin);
            } else {
                println!("adduser: missing user name");
            }
        }
        
        // IPC builtins
        "ipc-create" => cmd_ipc_create(),
        "ipc-send" => {
            if args.len() >= 2 {
                if let Ok(channel) = args[0].parse::<u32>() {
                    cmd_ipc_send(channel, args[1]);
                } else {
                    println!("ipc-send: invalid channel");
                }
            } else {
                println!("ipc-send: missing channel or message");
            }
        }
        "ipc-recv" => {
            if let Some(chan) = args.first() {
                if let Ok(channel) = chan.parse::<u32>() {
                    cmd_ipc_recv(channel);
                } else {
                    println!("ipc-recv: invalid channel");
                }
            } else {
                println!("ipc-recv: missing channel");
            }
        }
        
        // All other commands are external
        _ => {
            let args: Vec<&str> = args.to_vec();
            execute_external_command(state, cmd, &args);
        }
    }
}

// ============================================================================
// Main Entry Point
// ============================================================================

fn main() -> ! {
    println!("Welcome to NexaOS shell. Type 'help' for commands.");
    
    let mut state = ShellState::new();
    let mut editor = LineEditor::new();

    loop {
        let line = editor.read_line(&state);
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            handle_command(&mut state, trimmed);
        }
    }
}
