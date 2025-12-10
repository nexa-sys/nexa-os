//! Shell Command Executor
//!
//! Executes parsed commands including control flow structures and redirections

use crate::builtins::BuiltinRegistry;
use crate::parser::{Command, Redirect, RedirectType};
use crate::state::ShellState;
use std::io::{self, Write};
use std::string::String;
use std::vec::Vec;

// Syscall numbers
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_PIPE: u64 = 22;
const SYS_DUP2: u64 = 33;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAITPID: u64 = 61;

// Open flags
const O_RDONLY: u64 = 0;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_CREAT: u64 = 0o100;
const O_TRUNC: u64 = 0o1000;
const O_APPEND: u64 = 0o2000;

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
            Command::Simple(args) => self.execute_simple(args, &[]),
            Command::SimpleWithRedirects { args, redirects } => {
                self.execute_simple(args, redirects)
            }
            Command::Pipeline(cmds) => self.execute_pipeline(cmds),
            Command::AndList(cmds) => self.execute_and_list(cmds),
            Command::OrList(cmds) => self.execute_or_list(cmds),
            Command::Background(cmd) => self.execute_background(cmd),
            Command::Subshell(cmd) => self.execute_subshell(cmd),
            Command::BraceGroup(cmds) => self.execute(cmds),
            Command::If {
                condition,
                then_part,
                elif_parts,
                else_part,
            } => self.execute_if(condition, then_part, elif_parts, else_part),
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

    /// Execute simple command (builtin or external) with redirections
    fn execute_simple(&mut self, args: &[String], redirects: &[Redirect]) -> i32 {
        if args.is_empty() {
            // Handle redirections without command (e.g., just "> file" to truncate)
            if !redirects.is_empty() {
                return self.apply_redirects_only(redirects);
            }
            return 0;
        }

        // Expand variables in args
        let expanded: Vec<String> = args.iter().map(|a| self.expand_string(a)).collect();

        // Expand glob patterns
        let expanded = self.expand_globs(&expanded);

        let cmd = &expanded[0];
        let cmd_args: Vec<&str> = expanded.iter().map(|s| s.as_str()).collect();

        // Check for function first
        if let Some(func_body) = self.state.get_function(cmd).map(|s| s.to_string()) {
            // Save old positional params
            let old_params: Vec<String> = self
                .state
                .positional_params_at()
                .iter()
                .map(|s| s.to_string())
                .collect();

            // Set new positional params from arguments
            self.state.set_positional_params(expanded[1..].to_vec());

            // Parse and execute function body (TODO: apply redirects for functions)
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

        // Check for builtin - execute in subshell if redirects present
        if self.registry.is_builtin(cmd) {
            if redirects.is_empty() {
                if let Some(result) = self.registry.execute(cmd, self.state, &cmd_args[1..]) {
                    let code = result.unwrap_or(1);
                    self.state.set_var("?", &code.to_string());
                    return code;
                }
            } else {
                // Execute builtin with redirections in a forked process
                return self.execute_builtin_with_redirects(&expanded, redirects);
            }
        }

        // External command with redirections
        self.execute_external_with_redirects(&expanded, redirects)
    }

    /// Apply redirects in current process (for commands like "> file" without actual command)
    fn apply_redirects_only(&mut self, redirects: &[Redirect]) -> i32 {
        for redirect in redirects {
            let target = self.expand_string(&redirect.target);
            match &redirect.rtype {
                RedirectType::Output => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_TRUNC, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return 1;
                    }
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::Append => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_APPEND, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return 1;
                    }
                    syscall1(SYS_CLOSE, fd as u64);
                }
                _ => {}
            }
        }
        0
    }

    /// Open a file with the given flags
    fn open_file(&self, path: &str, flags: u64, mode: u64) -> i64 {
        let path_cstr = format!("{}\0", path);
        syscall3(SYS_OPEN, path_cstr.as_ptr() as u64, flags, mode)
    }

    /// Execute builtin command with redirections (in a subshell)
    fn execute_builtin_with_redirects(&mut self, args: &[String], redirects: &[Redirect]) -> i32 {
        let pid = syscall0(SYS_FORK);
        if pid < 0 {
            eprintln!("fork 失败");
            return 1;
        }

        if pid == 0 {
            // Child - apply redirects then run builtin
            if !self.setup_redirects(redirects) {
                syscall1(SYS_EXIT, 1);
                unreachable!()
            }

            let cmd = &args[0];
            let cmd_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let code = if let Some(result) = self.registry.execute(cmd, self.state, &cmd_args[1..])
            {
                result.unwrap_or(1)
            } else {
                1
            };
            syscall1(SYS_EXIT, code as u64);
            unreachable!()
        } else {
            // Parent - wait for child
            let mut status: i32 = 0;
            syscall3(SYS_WAITPID, pid as u64, &mut status as *mut i32 as u64, 0);
            let exit_code = if status & 0x7f == 0 {
                (status >> 8) & 0xff
            } else {
                128 + (status & 0x7f)
            };
            self.state.set_var("?", &exit_code.to_string());
            exit_code
        }
    }

    /// Setup redirections in the current process (called in child after fork)
    fn setup_redirects(&self, redirects: &[Redirect]) -> bool {
        for redirect in redirects {
            let target = redirect.target.clone();
            match &redirect.rtype {
                RedirectType::Output => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_TRUNC, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, 1); // stdout
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::Append => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_APPEND, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, 1); // stdout
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::Input => {
                    let fd = self.open_file(&target, O_RDONLY, 0);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, 0); // stdin
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::Stderr => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_TRUNC, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, 2); // stderr
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::StderrAppend => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_APPEND, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, 2); // stderr
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::Both => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_TRUNC, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, 1); // stdout
                    syscall2(SYS_DUP2, fd as u64, 2); // stderr
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::BothAppend => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_APPEND, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, 1); // stdout
                    syscall2(SYS_DUP2, fd as u64, 2); // stderr
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::FdOutput(fd_num) => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_TRUNC, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, *fd_num as u64);
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::FdAppend(fd_num) => {
                    let fd = self.open_file(&target, O_WRONLY | O_CREAT | O_APPEND, 0o644);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, *fd_num as u64);
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::FdInput(fd_num) => {
                    let fd = self.open_file(&target, O_RDONLY, 0);
                    if fd < 0 {
                        eprintln!("无法打开文件: {}", target);
                        return false;
                    }
                    syscall2(SYS_DUP2, fd as u64, *fd_num as u64);
                    syscall1(SYS_CLOSE, fd as u64);
                }
                RedirectType::FdDup(from_fd, to_fd) => {
                    syscall2(SYS_DUP2, *to_fd as u64, *from_fd as u64);
                }
                RedirectType::FdDupIn(from_fd, to_fd) => {
                    syscall2(SYS_DUP2, *to_fd as u64, *from_fd as u64);
                }
                RedirectType::FdClose(fd_num) => {
                    syscall1(SYS_CLOSE, *fd_num as u64);
                }
                RedirectType::HereDoc => {
                    // Create a pipe and write the here-doc content to it
                    let mut pipe_fds = [0i32; 2];
                    if syscall1(SYS_PIPE, pipe_fds.as_mut_ptr() as u64) < 0 {
                        eprintln!("pipe 创建失败");
                        return false;
                    }
                    // Write here-doc content (target contains the delimiter, content should be parsed)
                    // For now, just use target as content (simplified)
                    let content = target.as_bytes();
                    syscall3(
                        SYS_WRITE,
                        pipe_fds[1] as u64,
                        content.as_ptr() as u64,
                        content.len() as u64,
                    );
                    syscall1(SYS_CLOSE, pipe_fds[1] as u64);
                    syscall2(SYS_DUP2, pipe_fds[0] as u64, 0);
                    syscall1(SYS_CLOSE, pipe_fds[0] as u64);
                }
                RedirectType::HereString => {
                    // Create a pipe and write the here-string content to it
                    let mut pipe_fds = [0i32; 2];
                    if syscall1(SYS_PIPE, pipe_fds.as_mut_ptr() as u64) < 0 {
                        eprintln!("pipe 创建失败");
                        return false;
                    }
                    let content = format!("{}\n", target);
                    let content_bytes = content.as_bytes();
                    syscall3(
                        SYS_WRITE,
                        pipe_fds[1] as u64,
                        content_bytes.as_ptr() as u64,
                        content_bytes.len() as u64,
                    );
                    syscall1(SYS_CLOSE, pipe_fds[1] as u64);
                    syscall2(SYS_DUP2, pipe_fds[0] as u64, 0);
                    syscall1(SYS_CLOSE, pipe_fds[0] as u64);
                }
            }
        }
        true
    }

    /// Execute external command with redirections
    fn execute_external_with_redirects(&mut self, args: &[String], redirects: &[Redirect]) -> i32 {
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
            // Child process - apply redirects first
            if !self.setup_redirects(redirects) {
                syscall1(SYS_EXIT, 1);
                unreachable!()
            }

            let path_cstr = format!("{}\0", path);

            // Prepare argv
            let argv_strs: Vec<String> = args.iter().map(|s| format!("{}\0", s)).collect();
            let argv_ptrs: Vec<*const u8> = argv_strs
                .iter()
                .map(|s| s.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect();

            // Prepare envp from exported variables
            let env_strs: Vec<String> = self
                .state
                .exported_vars()
                .iter()
                .map(|(k, v)| format!("{}={}\0", k, v))
                .collect();
            let envp_ptrs: Vec<*const u8> = env_strs
                .iter()
                .map(|s| s.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect();

            let ret = syscall3(
                SYS_EXECVE,
                path_cstr.as_ptr() as u64,
                argv_ptrs.as_ptr() as u64,
                envp_ptrs.as_ptr() as u64,
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

    /// Expand glob patterns in arguments
    fn expand_globs(&self, args: &[String]) -> Vec<String> {
        let mut result = Vec::new();
        for arg in args {
            if arg.contains('*') || arg.contains('?') || arg.contains('[') {
                // Try to expand glob
                if let Some(expanded) = self.glob_expand(arg) {
                    result.extend(expanded);
                } else {
                    // No matches, keep original
                    result.push(arg.clone());
                }
            } else {
                result.push(arg.clone());
            }
        }
        result
    }

    /// Expand a single glob pattern
    fn glob_expand(&self, pattern: &str) -> Option<Vec<String>> {
        // Split pattern into directory and filename parts
        let (dir, file_pattern) = if let Some(pos) = pattern.rfind('/') {
            (&pattern[..pos], &pattern[pos + 1..])
        } else {
            (".", pattern)
        };

        // Read directory entries
        let dir_cstr = format!("{}\0", dir);
        let fd = syscall3(SYS_OPEN, dir_cstr.as_ptr() as u64, O_RDONLY, 0);
        if fd < 0 {
            return None;
        }

        let mut matches = Vec::new();
        let mut buf = [0u8; 4096];

        // Read directory (using getdents64 syscall)
        const SYS_GETDENTS64: u64 = 78;
        loop {
            let n = syscall3(
                SYS_GETDENTS64,
                fd as u64,
                buf.as_mut_ptr() as u64,
                buf.len() as u64,
            );
            if n <= 0 {
                break;
            }

            let mut offset = 0usize;
            while offset < n as usize {
                // Parse dirent64 structure
                // struct dirent64 { ino: u64, off: u64, reclen: u16, type: u8, name: [char] }
                if offset + 19 > n as usize {
                    break;
                }
                let reclen = u16::from_ne_bytes([buf[offset + 16], buf[offset + 17]]) as usize;
                if reclen == 0 || offset + reclen > n as usize {
                    break;
                }

                // Extract name (null-terminated string starting at offset + 19)
                let name_start = offset + 19;
                let name_end = buf[name_start..offset + reclen]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| name_start + p)
                    .unwrap_or(offset + reclen);

                if let Ok(name) = std::str::from_utf8(&buf[name_start..name_end]) {
                    if name != "." && name != ".." && self.glob_match(file_pattern, name) {
                        let full_path = if dir == "." {
                            name.to_string()
                        } else {
                            format!("{}/{}", dir, name)
                        };
                        matches.push(full_path);
                    }
                }

                offset += reclen;
            }
        }

        syscall1(SYS_CLOSE, fd as u64);

        if matches.is_empty() {
            None
        } else {
            matches.sort();
            Some(matches)
        }
    }

    /// Execute external command (legacy, without redirects)
    fn execute_external(&mut self, args: &[String]) -> i32 {
        self.execute_external_with_redirects(args, &[])
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
                0,
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
    fn execute_if(
        &mut self,
        condition: &[Command],
        then_part: &[Command],
        elif_parts: &[(Vec<Command>, Vec<Command>)],
        else_part: &Option<Vec<Command>>,
    ) -> i32 {
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
        let expanded_words: Vec<String> = words
            .iter()
            .flat_map(|w| {
                let expanded = self.expand_string(w);
                if w == "\"$@\"" || w == "$@" {
                    self.state
                        .positional_params_at()
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<String>>()
                } else {
                    expanded
                        .split_whitespace()
                        .map(String::from)
                        .collect::<Vec<String>>()
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
        let expanded_words: Vec<String> = words.iter().map(|w| self.expand_string(w)).collect();

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
            Command::SimpleWithRedirects { args, redirects } => {
                let mut s = args.join(" ");
                for r in redirects {
                    s.push(' ');
                    s.push_str(&self.serialize_redirect(r));
                }
                s
            }
            Command::Pipeline(cmds) => cmds
                .iter()
                .map(|c| self.serialize_command(c))
                .collect::<Vec<_>>()
                .join(" | "),
            Command::AndList(cmds) => cmds
                .iter()
                .map(|c| self.serialize_command(c))
                .collect::<Vec<_>>()
                .join(" && "),
            Command::OrList(cmds) => cmds
                .iter()
                .map(|c| self.serialize_command(c))
                .collect::<Vec<_>>()
                .join(" || "),
            Command::Background(cmd) => format!("{} &", self.serialize_command(cmd)),
            Command::Subshell(cmd) => format!("( {} )", self.serialize_command(cmd)),
            Command::BraceGroup(cmds) => format!("{{ {}; }}", self.serialize_commands(cmds)),
            Command::If {
                condition,
                then_part,
                elif_parts,
                else_part,
            } => {
                let mut s = format!(
                    "if {}; then {}",
                    self.serialize_commands(condition),
                    self.serialize_commands(then_part)
                );
                for (cond, then_) in elif_parts {
                    s.push_str(&format!(
                        "; elif {}; then {}",
                        self.serialize_commands(cond),
                        self.serialize_commands(then_)
                    ));
                }
                if let Some(else_cmds) = else_part {
                    s.push_str(&format!("; else {}", self.serialize_commands(else_cmds)));
                }
                s.push_str("; fi");
                s
            }
            Command::For { var, words, body } => format!(
                "for {} in {}; do {}; done",
                var,
                words.join(" "),
                self.serialize_commands(body)
            ),
            Command::While { condition, body } => format!(
                "while {}; do {}; done",
                self.serialize_commands(condition),
                self.serialize_commands(body)
            ),
            Command::Until { condition, body } => format!(
                "until {}; do {}; done",
                self.serialize_commands(condition),
                self.serialize_commands(body)
            ),
            Command::Case { word, cases } => {
                let mut s = format!("case {} in", word);
                for (patterns, cmds) in cases {
                    s.push_str(&format!(
                        " {}) {};; ",
                        patterns.join("|"),
                        self.serialize_commands(cmds)
                    ));
                }
                s.push_str("esac");
                s
            }
            Command::Select { var, words, body } => format!(
                "select {} in {}; do {}; done",
                var,
                words.join(" "),
                self.serialize_commands(body)
            ),
            Command::Function { name, body } => {
                format!("function {} {{ {}; }}", name, self.serialize_commands(body))
            }
            Command::Arithmetic(expr) => format!("(( {} ))", expr),
            Command::Conditional(args) => format!("[[ {} ]]", args.join(" ")),
            Command::Empty => String::new(),
        }
    }

    /// Serialize a redirect to string
    fn serialize_redirect(&self, r: &Redirect) -> String {
        match &r.rtype {
            RedirectType::Output => format!("> {}", r.target),
            RedirectType::Append => format!(">> {}", r.target),
            RedirectType::Input => format!("< {}", r.target),
            RedirectType::HereDoc => format!("<< {}", r.target),
            RedirectType::HereString => format!("<<< {}", r.target),
            RedirectType::Stderr => format!("2> {}", r.target),
            RedirectType::StderrAppend => format!("2>> {}", r.target),
            RedirectType::Both => format!("&> {}", r.target),
            RedirectType::BothAppend => format!("&>> {}", r.target),
            RedirectType::FdOutput(fd) => format!("{}> {}", fd, r.target),
            RedirectType::FdAppend(fd) => format!("{}>> {}", fd, r.target),
            RedirectType::FdInput(fd) => format!("{0}< {1}", fd, r.target),
            RedirectType::FdDup(from, to) => format!("{}>&{}", from, to),
            RedirectType::FdDupIn(from, to) => format!("{}<&{}", from, to),
            RedirectType::FdClose(fd) => format!("{}>&-", fd),
        }
    }

    /// Execute arithmetic expression
    fn execute_arithmetic(&mut self, expr: &str) -> i32 {
        let expanded = self.expand_string(expr);
        match self.evaluate_arithmetic(&expanded) {
            Ok(result) => {
                if result == 0 {
                    1
                } else {
                    0
                }
            }
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
                    if prev
                        .map(|p| !matches!(p, '*' | '/' | '%' | '+' | '-' | '(' | ' '))
                        .unwrap_or(false)
                    {
                        let left = self.evaluate_arithmetic(&expr[..i])?;
                        let right = self.evaluate_arithmetic(&expr[i + 1..])?;
                        return Ok(if chars[i] == '+' {
                            left + right
                        } else {
                            left - right
                        });
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
                    if right == 0 {
                        return Err("除以零".to_string());
                    }
                    return Ok(left / right);
                }
                '%' if depth == 0 && i > 0 => {
                    let left = self.evaluate_arithmetic(&expr[..i])?;
                    let right = self.evaluate_arithmetic(&expr[i + 1..])?;
                    if right == 0 {
                        return Err("除以零".to_string());
                    }
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
        if trimmed.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !trimmed
                .chars()
                .next()
                .map(|c| c.is_numeric())
                .unwrap_or(true)
        {
            if let Some(val) = self.state.get_var(trimmed) {
                return val.parse().map_err(|_| format!("无效数字: {}", val));
            }
            return Ok(0);
        }

        trimmed
            .parse()
            .map_err(|_| format!("无效表达式: {}", trimmed))
    }

    fn execute_conditional(&mut self, args: &[String]) -> i32 {
        let expanded: Vec<String> = args.iter().map(|a| self.expand_string(a)).collect();
        if expanded.is_empty() {
            return 1;
        }
        self.evaluate_conditional(&expanded)
    }

    fn evaluate_conditional(&self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 1;
        }

        if args.len() >= 2 {
            let op = &args[0];
            let arg = &args[1];

            match op.as_str() {
                "-z" => return if arg.is_empty() { 0 } else { 1 },
                "-n" => return if !arg.is_empty() { 0 } else { 1 },
                "-e" | "-a" => return if self.file_exists(arg) { 0 } else { 1 },
                "-f" => {
                    return if self.file_exists(arg) && !arg.ends_with('/') {
                        0
                    } else {
                        1
                    }
                }
                "-d" => return if self.is_directory(arg) { 0 } else { 1 },
                "!" => {
                    return if self.evaluate_conditional(&args[1..]) != 0 {
                        0
                    } else {
                        1
                    }
                }
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
                '`' if !in_single_quote => {
                    // Backtick command substitution
                    let mut cmd = String::new();
                    while let Some(c) = chars.next() {
                        if c == '`' {
                            break;
                        }
                        if c == '\\' {
                            if let Some(next) = chars.next() {
                                match next {
                                    '`' | '\\' | '$' => cmd.push(next),
                                    _ => {
                                        cmd.push('\\');
                                        cmd.push(next);
                                    }
                                }
                            }
                        } else {
                            cmd.push(c);
                        }
                    }
                    result.push_str(&self.execute_command_substitution(&cmd));
                }
                '\\' if !in_single_quote => {
                    if let Some(next) = chars.next() {
                        if in_double_quote {
                            match next {
                                '$' | '`' | '"' | '\\' | '\n' => result.push(next),
                                _ => {
                                    result.push('\\');
                                    result.push(next);
                                }
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
            Some('(') => {
                chars.next();
                // Command substitution $(...) or arithmetic expansion $((...))
                if chars.peek() == Some(&'(') {
                    // Arithmetic expansion $(( ... ))
                    chars.next();
                    let mut expr = String::new();
                    let mut depth = 2;
                    while let Some(c) = chars.next() {
                        if c == ')' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        } else if c == '(' {
                            depth += 1;
                        }
                        if depth > 0 {
                            expr.push(c);
                        }
                    }
                    // Skip the final )
                    if chars.peek() == Some(&')') {
                        chars.next();
                    }
                    let expanded = self.expand_string(&expr);
                    match self.evaluate_arithmetic(&expanded) {
                        Ok(v) => v.to_string(),
                        Err(_) => "0".to_string(),
                    }
                } else {
                    // Command substitution $(...)
                    let cmd = self.read_command_substitution(chars);
                    self.execute_command_substitution(&cmd)
                }
            }
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
            Some('?') => {
                chars.next();
                self.state.get_var("?").unwrap_or("0").to_string()
            }
            Some('$') => {
                chars.next();
                "1".to_string()
            }
            Some('!') => {
                chars.next();
                self.state.get_var("!").unwrap_or("0").to_string()
            }
            Some('#') => {
                chars.next();
                self.state.positional_param_count().to_string()
            }
            Some('*') => {
                chars.next();
                self.state.positional_params_star()
            }
            Some('@') => {
                chars.next();
                self.state.positional_params_star()
            }
            Some('0') => {
                chars.next();
                self.state.shell_name().to_string()
            }
            Some(c) if c.is_numeric() => {
                let n = c.to_digit(10).unwrap() as usize;
                chars.next();
                self.state.get_positional_param(n).unwrap_or("").to_string()
            }
            _ => "$".to_string(),
        }
    }

    /// Read command substitution content from $(...)
    fn read_command_substitution(
        &self,
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> String {
        let mut cmd = String::new();
        let mut depth = 1;
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut escaped = false;

        while let Some(c) = chars.next() {
            if escaped {
                cmd.push(c);
                escaped = false;
                continue;
            }

            match c {
                '\\' if !in_single_quote => {
                    escaped = true;
                    cmd.push(c);
                }
                '\'' if !in_double_quote => {
                    in_single_quote = !in_single_quote;
                    cmd.push(c);
                }
                '"' if !in_single_quote => {
                    in_double_quote = !in_double_quote;
                    cmd.push(c);
                }
                '(' if !in_single_quote && !in_double_quote => {
                    depth += 1;
                    cmd.push(c);
                }
                ')' if !in_single_quote && !in_double_quote => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    cmd.push(c);
                }
                _ => cmd.push(c),
            }
        }

        cmd
    }

    /// Execute command substitution and capture output
    fn execute_command_substitution(&self, cmd: &str) -> String {
        // Create a pipe for capturing output
        let mut pipe_fds = [0i32; 2];
        if syscall1(SYS_PIPE, pipe_fds.as_mut_ptr() as u64) < 0 {
            return String::new();
        }

        let pid = syscall0(SYS_FORK);
        if pid < 0 {
            syscall1(SYS_CLOSE, pipe_fds[0] as u64);
            syscall1(SYS_CLOSE, pipe_fds[1] as u64);
            return String::new();
        }

        if pid == 0 {
            // Child: redirect stdout to pipe, execute command
            syscall1(SYS_CLOSE, pipe_fds[0] as u64);
            syscall2(SYS_DUP2, pipe_fds[1] as u64, 1);
            syscall1(SYS_CLOSE, pipe_fds[1] as u64);

            // Execute the command using /bin/sh -c
            let sh_path = "/bin/sh\0";
            let c_flag = "-c\0";
            let cmd_str = format!("{}\0", cmd);

            let argv_ptrs: [*const u8; 4] = [
                sh_path.as_ptr(),
                c_flag.as_ptr(),
                cmd_str.as_ptr(),
                std::ptr::null(),
            ];

            syscall3(
                SYS_EXECVE,
                sh_path.as_ptr() as u64,
                argv_ptrs.as_ptr() as u64,
                std::ptr::null::<u8>() as u64,
            );
            syscall1(SYS_EXIT, 1);
            unreachable!()
        } else {
            // Parent: read output from pipe
            syscall1(SYS_CLOSE, pipe_fds[1] as u64);

            let mut output = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = syscall3(
                    SYS_READ,
                    pipe_fds[0] as u64,
                    buf.as_mut_ptr() as u64,
                    buf.len() as u64,
                );
                if n <= 0 {
                    break;
                }
                output.extend_from_slice(&buf[..n as usize]);
            }
            syscall1(SYS_CLOSE, pipe_fds[0] as u64);

            // Wait for child
            let mut status: i32 = 0;
            syscall3(SYS_WAITPID, pid as u64, &mut status as *mut i32 as u64, 0);

            // Convert to string, remove trailing newlines
            let mut result = String::from_utf8_lossy(&output).to_string();
            while result.ends_with('\n') {
                result.pop();
            }
            result
        }
    }

    fn expand_braced_variable(&self, chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut content = String::new();
        let mut depth = 1;

        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    depth += 1;
                    content.push(c);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    content.push(c);
                }
                _ => content.push(c),
            }
        }

        if content.starts_with('#') {
            let var_name = &content[1..];
            return self
                .state
                .get_var(var_name)
                .map(|v| v.len().to_string())
                .unwrap_or_else(|| "0".to_string());
        }

        self.state.get_var(&content).unwrap_or("").to_string()
    }
}
