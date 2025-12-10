//! Shell State Management
//!
//! This module contains the shell's runtime state including:
//! - Current working directory
//! - Directory stack (for pushd/popd)
//! - Environment variables
//! - Shell options
//! - Exit status
//! - Aliases

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Shell variable attributes
#[derive(Clone, Debug, Default)]
pub struct VarAttributes {
    pub exported: bool,
    pub readonly: bool,
    pub integer: bool,
    pub lowercase: bool,
    pub uppercase: bool,
}

/// Shell variable with value and attributes
#[derive(Clone, Debug)]
pub struct ShellVar {
    pub value: String,
    pub attrs: VarAttributes,
}

impl ShellVar {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            attrs: VarAttributes::default(),
        }
    }

    pub fn with_export(mut self) -> Self {
        self.attrs.exported = true;
        self
    }

    pub fn with_readonly(mut self) -> Self {
        self.attrs.readonly = true;
        self
    }
}

/// Shell options (set -o)
#[derive(Clone, Debug, Default)]
pub struct ShellOptions {
    pub errexit: bool,     // -e: Exit on error
    pub nounset: bool,     // -u: Error on unset variables
    pub xtrace: bool,      // -x: Print commands before execution
    pub verbose: bool,     // -v: Print input lines
    pub noclobber: bool,   // -C: Don't overwrite files with >
    pub allexport: bool,   // -a: Export all variables
    pub notify: bool,      // -b: Notify immediately of job termination
    pub noglob: bool,      // -f: Disable pathname expansion
    pub ignoreeof: bool,   // Ignore EOF (Ctrl-D)
    pub hashall: bool,     // -h: Hash commands
    pub interactive: bool, // Shell is interactive
}

/// Command alias definition
#[derive(Clone, Debug)]
pub struct Alias {
    pub name: String,
    pub value: String,
}

/// Shell runtime state
pub struct ShellState {
    /// Current working directory
    cwd: PathBuf,
    /// Directory stack for pushd/popd
    dir_stack: Vec<PathBuf>,
    /// Shell variables
    variables: HashMap<String, ShellVar>,
    /// Command aliases
    aliases: HashMap<String, String>,
    /// Shell options
    pub options: ShellOptions,
    /// Last exit status ($?)
    pub last_exit_status: i32,
    /// Current loop depth (for break/continue)
    pub loop_depth: usize,
    /// Break/continue level requested
    pub flow_control: Option<FlowControl>,
    /// Function call depth (for return)
    pub function_depth: usize,
    /// Hashed command paths
    hash_table: HashMap<String, PathBuf>,
    /// Job control - list of background jobs
    jobs: Vec<Job>,
    /// Next job ID
    next_job_id: usize,
    /// Command history
    history: Vec<String>,
    /// History file position (for reading new entries)
    history_file_pos: usize,
    /// Signal traps
    traps: HashMap<String, String>,
    /// Shopt options (shell options beyond set -o)
    shopts: HashMap<String, bool>,
    /// Is this a login shell?
    login_shell: bool,
    /// File mode creation mask
    umask: u32,
    /// Array variables
    arrays: HashMap<String, Vec<String>>,
    /// Completion specifications
    completion_specs: HashMap<String, String>,
    /// Call stack for caller builtin
    call_stack: Vec<CallFrame>,
    /// Positional parameters ($1, $2, etc.)
    positional_params: Vec<String>,
    /// Script/shell name ($0)
    shell_name: String,
    /// Shell functions
    functions: HashMap<String, String>,
    /// Last background process PID ($!)
    last_bg_pid: i32,
    /// Last argument of previous command ($_)
    last_arg: String,
}

/// Flow control signals for break/continue/return
#[derive(Clone, Debug)]
pub enum FlowControl {
    Break(usize),
    Continue(usize),
    Return(i32),
}

/// Job information for job control
#[derive(Clone, Debug)]
pub struct Job {
    pub job_id: usize,
    pub pid: i32,
    pub status: String,
    pub command: String,
    pub is_current: bool,
    pub is_previous: bool,
    pub no_hup: bool,
}

/// Call frame for caller builtin
#[derive(Clone, Debug)]
pub struct CallFrame {
    pub line: usize,
    pub name: String,
    pub file: String,
}

impl ShellState {
    /// Create a new shell state
    pub fn new() -> Self {
        let mut state = Self {
            cwd: PathBuf::from("/"),
            dir_stack: Vec::new(),
            variables: HashMap::new(),
            aliases: HashMap::new(),
            options: ShellOptions::default(),
            last_exit_status: 0,
            loop_depth: 0,
            flow_control: None,
            function_depth: 0,
            hash_table: HashMap::new(),
            jobs: Vec::new(),
            next_job_id: 1,
            history: Vec::new(),
            history_file_pos: 0,
            traps: HashMap::new(),
            shopts: Self::default_shopts(),
            login_shell: false,
            umask: 0o022,
            arrays: HashMap::new(),
            completion_specs: HashMap::new(),
            call_stack: Vec::new(),
            positional_params: Vec::new(),
            shell_name: "shell".to_string(),
            functions: HashMap::new(),
            last_bg_pid: 0,
            last_arg: String::new(),
        };

        // Initialize some default variables
        state.set_var("SHELL", "/bin/shell");
        state.set_var("PATH", "/bin:/sbin:/usr/bin:/usr/sbin");
        state.set_var("HOME", "/root");
        state.set_var("PWD", "/");
        state.set_var("OLDPWD", "/");
        state.set_var("IFS", " \t\n");

        // Export PATH by default
        if let Some(var) = state.variables.get_mut("PATH") {
            var.attrs.exported = true;
        }

        state.options.hashall = true;
        state.options.interactive = true;

        state
    }

    // ========================================================================
    // Directory Management
    // ========================================================================

    /// Get current working directory
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Get current working directory as string
    pub fn cwd_str(&self) -> &str {
        self.cwd.to_str().unwrap_or("/")
    }

    /// Set current working directory
    pub fn set_cwd(&mut self, path: impl AsRef<Path>) {
        let old = self.cwd.clone();
        self.cwd = path.as_ref().to_path_buf();
        let new_pwd = self.cwd.to_str().unwrap_or("/").to_string();
        self.set_var("OLDPWD", old.to_str().unwrap_or("/"));
        self.set_var("PWD", new_pwd);
    }

    /// Resolve a path relative to cwd
    pub fn resolve_path(&self, input: &str) -> PathBuf {
        if input.starts_with('/') {
            normalize_path(Path::new(input))
        } else if input.starts_with('~') {
            let home = self.get_var("HOME").unwrap_or("/root");
            if input == "~" {
                PathBuf::from(home)
            } else if input.starts_with("~/") {
                normalize_path(&PathBuf::from(home).join(&input[2..]))
            } else {
                // ~user syntax - not implemented yet
                normalize_path(Path::new(input))
            }
        } else {
            normalize_path(&self.cwd.join(input))
        }
    }

    /// Push directory onto stack
    pub fn push_dir(&mut self, path: PathBuf) {
        self.dir_stack.push(path);
    }

    /// Pop directory from stack
    pub fn pop_dir(&mut self) -> Option<PathBuf> {
        self.dir_stack.pop()
    }

    /// Get directory stack (including cwd at top)
    pub fn dir_stack(&self) -> Vec<&Path> {
        let mut stack: Vec<&Path> = vec![&self.cwd];
        stack.extend(self.dir_stack.iter().rev().map(|p| p.as_path()));
        stack
    }

    /// Rotate directory stack
    pub fn rotate_dir_stack(&mut self, n: i32) -> Option<PathBuf> {
        if self.dir_stack.is_empty() {
            return None;
        }
        let len = self.dir_stack.len();
        let idx = if n >= 0 {
            (n as usize) % (len + 1)
        } else {
            len - ((-n as usize - 1) % (len + 1))
        };

        if idx == 0 {
            Some(self.cwd.clone())
        } else if idx <= len {
            Some(self.dir_stack[len - idx].clone())
        } else {
            None
        }
    }

    // ========================================================================
    // Variable Management
    // ========================================================================

    /// Get a variable value
    pub fn get_var(&self, name: &str) -> Option<&str> {
        // Check special variables first
        match name {
            "?" => {
                return Some(Box::leak(
                    self.last_exit_status.to_string().into_boxed_str(),
                ))
            }
            "PWD" => return Some(self.cwd_str()),
            _ => {}
        }
        self.variables.get(name).map(|v| v.value.as_str())
    }

    /// Set a variable value
    pub fn set_var(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let mut value = value.into();

        if let Some(existing) = self.variables.get(&name) {
            if existing.attrs.readonly {
                return; // Cannot modify readonly variables
            }
            // Apply transformations
            if existing.attrs.lowercase {
                value = value.to_lowercase();
            } else if existing.attrs.uppercase {
                value = value.to_uppercase();
            }
        }

        self.variables
            .entry(name.clone())
            .and_modify(|v| v.value = value.clone())
            .or_insert_with(|| ShellVar::new(value));

        // If allexport is set, auto-export new variables
        if self.options.allexport {
            if let Some(var) = self.variables.get_mut(&name) {
                var.attrs.exported = true;
            }
        }
    }

    /// Unset a variable
    pub fn unset_var(&mut self, name: &str) -> Result<(), String> {
        if let Some(var) = self.variables.get(name) {
            if var.attrs.readonly {
                return Err(format!("{}: 只读变量", name));
            }
        }
        self.variables.remove(name);
        Ok(())
    }

    /// Export a variable
    pub fn export_var(&mut self, name: &str) {
        if let Some(var) = self.variables.get_mut(name) {
            var.attrs.exported = true;
        } else {
            // Create empty exported variable
            let mut var = ShellVar::new("");
            var.attrs.exported = true;
            self.variables.insert(name.to_string(), var);
        }
    }

    /// Mark a variable as readonly
    pub fn set_readonly(&mut self, name: &str) -> Result<(), String> {
        if let Some(var) = self.variables.get_mut(name) {
            var.attrs.readonly = true;
            Ok(())
        } else {
            Err(format!("{}: 未设置", name))
        }
    }

    /// Get all exported variables
    pub fn exported_vars(&self) -> Vec<(&str, &str)> {
        self.variables
            .iter()
            .filter(|(_, v)| v.attrs.exported)
            .map(|(k, v)| (k.as_str(), v.value.as_str()))
            .collect()
    }

    /// Get all variables
    pub fn all_vars(&self) -> Vec<(&str, &ShellVar)> {
        self.variables
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    // ========================================================================
    // Alias Management
    // ========================================================================

    /// Define an alias
    pub fn set_alias(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.aliases.insert(name.into(), value.into());
    }

    /// Remove an alias
    pub fn unset_alias(&mut self, name: &str) -> bool {
        self.aliases.remove(name).is_some()
    }

    /// Get an alias
    pub fn get_alias(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(|s| s.as_str())
    }

    /// List all aliases
    pub fn list_aliases(&self) -> Vec<(&str, &str)> {
        let mut list: Vec<_> = self
            .aliases
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        list.sort_by_key(|(k, _)| *k);
        list
    }

    /// Remove all aliases
    pub fn clear_aliases(&mut self) {
        self.aliases.clear();
    }

    // ========================================================================
    // Command Hash Table
    // ========================================================================

    /// Add command to hash table
    pub fn hash_command(&mut self, name: impl Into<String>, path: PathBuf) {
        self.hash_table.insert(name.into(), path);
    }

    /// Get hashed command path
    pub fn get_hashed(&self, name: &str) -> Option<&PathBuf> {
        self.hash_table.get(name)
    }

    /// Clear hash table
    pub fn clear_hash(&mut self) {
        self.hash_table.clear();
    }

    /// Remove from hash table
    pub fn unhash(&mut self, name: &str) -> bool {
        self.hash_table.remove(name).is_some()
    }

    /// List all hashed commands
    pub fn list_hashed(&self) -> Vec<(&str, &Path)> {
        let mut list: Vec<_> = self
            .hash_table
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_path()))
            .collect();
        list.sort_by_key(|(k, _)| *k);
        list
    }

    /// Get mutable access to variables (for declare builtin)
    pub fn variables_mut(&mut self) -> &mut HashMap<String, ShellVar> {
        &mut self.variables
    }

    // ========================================================================
    // Job Control
    // ========================================================================

    /// List all jobs
    pub fn list_jobs(&self) -> Vec<Job> {
        self.jobs.clone()
    }

    /// Get a job by ID
    pub fn get_job(&self, job_id: usize) -> Option<&Job> {
        self.jobs.iter().find(|j| j.job_id == job_id)
    }

    /// Get job PID from job spec (e.g., %1, %+, %-)
    pub fn get_job_pid(&self, spec: &str) -> Option<i32> {
        if spec == "%%" || spec == "%+" {
            self.jobs.iter().find(|j| j.is_current).map(|j| j.pid)
        } else if spec == "%-" {
            self.jobs.iter().find(|j| j.is_previous).map(|j| j.pid)
        } else if let Some(id_str) = spec.strip_prefix('%') {
            if let Ok(id) = id_str.parse::<usize>() {
                self.jobs.iter().find(|j| j.job_id == id).map(|j| j.pid)
            } else {
                // Match by command prefix
                self.jobs
                    .iter()
                    .find(|j| j.command.starts_with(id_str))
                    .map(|j| j.pid)
            }
        } else {
            None
        }
    }

    /// Parse job spec to job ID
    pub fn parse_job_spec(&self, spec: &str) -> Option<usize> {
        if spec == "%%" || spec == "%+" {
            self.jobs.iter().find(|j| j.is_current).map(|j| j.job_id)
        } else if spec == "%-" {
            self.jobs.iter().find(|j| j.is_previous).map(|j| j.job_id)
        } else if let Some(id_str) = spec.strip_prefix('%') {
            id_str.parse().ok()
        } else {
            None
        }
    }

    /// Add a new job
    pub fn add_job(&mut self, pid: i32, command: &str) -> usize {
        // Mark old current as previous
        for job in &mut self.jobs {
            if job.is_current {
                job.is_current = false;
                job.is_previous = true;
            } else {
                job.is_previous = false;
            }
        }

        let job_id = self.next_job_id;
        self.next_job_id += 1;

        self.jobs.push(Job {
            job_id,
            pid,
            status: "Running".to_string(),
            command: command.to_string(),
            is_current: true,
            is_previous: false,
            no_hup: false,
        });

        job_id
    }

    /// Resume a job (fg/bg)
    pub fn resume_job(&mut self, spec: Option<&str>, foreground: bool) -> Result<usize, String> {
        let job_id = if let Some(s) = spec {
            self.parse_job_spec(s)
                .ok_or_else(|| format!("{}: 没有这个作业", s))?
        } else {
            self.jobs
                .iter()
                .find(|j| j.is_current)
                .map(|j| j.job_id)
                .ok_or_else(|| "当前: 没有这个作业".to_string())?
        };

        if let Some(job) = self.jobs.iter_mut().find(|j| j.job_id == job_id) {
            job.status = if foreground { "Running" } else { "Running" }.to_string();

            #[cfg(target_family = "unix")]
            unsafe {
                libc::kill(job.pid, libc::SIGCONT);
            }

            Ok(job_id)
        } else {
            Err(format!("{}: 没有这个作业", job_id))
        }
    }

    /// Wait for a specific job
    pub fn wait_for_job(&mut self, job_id: usize) -> Option<i32> {
        if let Some(job) = self.jobs.iter().find(|j| j.job_id == job_id) {
            let pid = job.pid;

            #[cfg(target_family = "unix")]
            {
                let mut status: i32 = 0;
                unsafe {
                    libc::waitpid(pid, &mut status, 0);
                }

                // Remove completed job
                self.jobs.retain(|j| j.job_id != job_id);

                if libc::WIFEXITED(status) {
                    Some(libc::WEXITSTATUS(status))
                } else {
                    Some(128 + libc::WTERMSIG(status))
                }
            }

            #[cfg(not(target_family = "unix"))]
            {
                self.jobs.retain(|j| j.job_id != job_id);
                Some(0)
            }
        } else {
            None
        }
    }

    /// Wait for a specific PID
    pub fn wait_for_pid(&mut self, pid: i32) -> i32 {
        #[cfg(target_family = "unix")]
        {
            let mut status: i32 = 0;
            unsafe {
                libc::waitpid(pid, &mut status, 0);
            }

            // Remove job with this PID if any
            self.jobs.retain(|j| j.pid != pid);

            if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status)
            } else {
                128 + libc::WTERMSIG(status)
            }
        }

        #[cfg(not(target_family = "unix"))]
        {
            self.jobs.retain(|j| j.pid != pid);
            0
        }
    }

    /// Wait for all jobs
    pub fn wait_all_jobs(&mut self) -> i32 {
        let mut last_status = 0;
        while !self.jobs.is_empty() {
            if let Some(job) = self.jobs.first() {
                let job_id = job.job_id;
                if let Some(status) = self.wait_for_job(job_id) {
                    last_status = status;
                }
            } else {
                break;
            }
        }
        last_status
    }

    /// Wait for any job
    pub fn wait_any_job(&mut self) -> (i32, i32) {
        #[cfg(target_family = "unix")]
        {
            let mut status: i32 = 0;
            let pid = unsafe { libc::waitpid(-1, &mut status, 0) };

            if pid > 0 {
                self.jobs.retain(|j| j.pid != pid);
                let exit_code = if libc::WIFEXITED(status) {
                    libc::WEXITSTATUS(status)
                } else {
                    128 + libc::WTERMSIG(status)
                };
                (pid, exit_code)
            } else {
                (0, 0)
            }
        }

        #[cfg(not(target_family = "unix"))]
        {
            (0, 0)
        }
    }

    /// Disown current job
    pub fn disown_current_job(&mut self, no_hup: bool) -> Result<(), String> {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.is_current) {
            if no_hup {
                job.no_hup = true;
            } else {
                let job_id = job.job_id;
                self.jobs.retain(|j| j.job_id != job_id);
            }
            Ok(())
        } else {
            Err("当前: 没有这个作业".to_string())
        }
    }

    /// Disown a specific job
    pub fn disown_job(&mut self, spec: &str, no_hup: bool) -> Result<(), String> {
        let job_id = self
            .parse_job_spec(spec)
            .ok_or_else(|| format!("{}: 没有这个作业", spec))?;

        if let Some(job) = self.jobs.iter_mut().find(|j| j.job_id == job_id) {
            if no_hup {
                job.no_hup = true;
            } else {
                self.jobs.retain(|j| j.job_id != job_id);
            }
            Ok(())
        } else {
            Err(format!("{}: 没有这个作业", spec))
        }
    }

    /// Disown all jobs
    pub fn disown_all_jobs(&mut self, no_hup: bool, running_only: bool) {
        if no_hup {
            for job in &mut self.jobs {
                if !running_only || job.status == "Running" {
                    job.no_hup = true;
                }
            }
        } else {
            if running_only {
                self.jobs.retain(|j| j.status != "Running");
            } else {
                self.jobs.clear();
            }
        }
    }

    /// Check if this is a login shell
    pub fn is_login_shell(&self) -> bool {
        self.login_shell
    }

    /// Set login shell status
    pub fn set_login_shell(&mut self, is_login: bool) {
        self.login_shell = is_login;
    }

    // ========================================================================
    // History Management
    // ========================================================================

    /// Add command to history
    pub fn add_history(&mut self, command: &str) {
        if !command.is_empty() && !command.starts_with(' ') {
            // Don't add duplicates of the last command
            if self.history.last().map(|s| s.as_str()) != Some(command) {
                self.history.push(command.to_string());
            }
        }
    }

    /// Get history list
    pub fn get_history(&self) -> &[String] {
        &self.history
    }

    /// Clear history
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Delete history entry at offset
    pub fn delete_history_entry(&mut self, offset: i32) -> Result<(), String> {
        let idx = if offset < 0 {
            self.history.len().checked_sub((-offset) as usize)
        } else {
            Some((offset - 1) as usize)
        };

        if let Some(i) = idx {
            if i < self.history.len() {
                self.history.remove(i);
                return Ok(());
            }
        }
        Err(format!("{}: 历史记录位置越界", offset))
    }

    /// Append history to file
    pub fn append_history_to_file(&self, path: &Path) -> std::io::Result<()> {
        use std::fs::OpenOptions;
        use std::io::Write;

        let mut file = OpenOptions::new().create(true).append(true).open(path)?;

        for entry in &self.history[self.history_file_pos..] {
            writeln!(file, "{}", entry)?;
        }
        Ok(())
    }

    /// Read history from file
    pub fn read_history_from_file(&mut self, path: &Path) -> std::io::Result<()> {
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        if path.exists() {
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                self.history.push(line?);
            }
            self.history_file_pos = self.history.len();
        }
        Ok(())
    }

    /// Read new history entries from file
    pub fn read_new_history_from_file(&mut self, path: &Path) -> std::io::Result<()> {
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        if path.exists() {
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            for (i, line) in reader.lines().enumerate() {
                if i >= self.history_file_pos {
                    self.history.push(line?);
                }
            }
            self.history_file_pos = self.history.len();
        }
        Ok(())
    }

    /// Write history to file
    pub fn write_history_to_file(&self, path: &Path) -> std::io::Result<()> {
        use std::fs::File;
        use std::io::Write;

        let mut file = File::create(path)?;
        for entry in &self.history {
            writeln!(file, "{}", entry)?;
        }
        Ok(())
    }

    /// Expand history references (!, !!, !n, etc.)
    pub fn expand_history(&self, input: &str) -> String {
        // Simple history expansion
        if input == "!!" {
            return self.history.last().cloned().unwrap_or_default();
        }
        if let Some(n_str) = input.strip_prefix('!') {
            if let Ok(n) = n_str.parse::<i32>() {
                let idx = if n < 0 {
                    self.history.len().saturating_sub((-n) as usize)
                } else {
                    (n - 1) as usize
                };
                if idx < self.history.len() {
                    return self.history[idx].clone();
                }
            } else {
                // Search by prefix
                for entry in self.history.iter().rev() {
                    if entry.starts_with(n_str) {
                        return entry.clone();
                    }
                }
            }
        }
        input.to_string()
    }

    // ========================================================================
    // Signal Traps
    // ========================================================================

    /// Set a signal trap
    pub fn set_trap(&mut self, signal: &str, action: &str) {
        self.traps.insert(signal.to_uppercase(), action.to_string());
    }

    /// Reset a signal trap
    pub fn reset_trap(&mut self, signal: &str) {
        self.traps.remove(&signal.to_uppercase());
    }

    /// Get all traps
    pub fn get_traps(&self) -> Vec<(&str, &str)> {
        self.traps
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    // ========================================================================
    // Shopt Options
    // ========================================================================

    /// Default shopt options
    fn default_shopts() -> HashMap<String, bool> {
        let mut shopts = HashMap::new();
        shopts.insert("autocd".to_string(), false);
        shopts.insert("cdable_vars".to_string(), false);
        shopts.insert("cdspell".to_string(), false);
        shopts.insert("checkhash".to_string(), false);
        shopts.insert("checkjobs".to_string(), false);
        shopts.insert("checkwinsize".to_string(), true);
        shopts.insert("cmdhist".to_string(), true);
        shopts.insert("compat31".to_string(), false);
        shopts.insert("compat32".to_string(), false);
        shopts.insert("compat40".to_string(), false);
        shopts.insert("compat41".to_string(), false);
        shopts.insert("compat42".to_string(), false);
        shopts.insert("compat43".to_string(), false);
        shopts.insert("compat44".to_string(), false);
        shopts.insert("complete_fullquote".to_string(), true);
        shopts.insert("direxpand".to_string(), false);
        shopts.insert("dirspell".to_string(), false);
        shopts.insert("dotglob".to_string(), false);
        shopts.insert("execfail".to_string(), false);
        shopts.insert("expand_aliases".to_string(), true);
        shopts.insert("extdebug".to_string(), false);
        shopts.insert("extglob".to_string(), true);
        shopts.insert("extquote".to_string(), true);
        shopts.insert("failglob".to_string(), false);
        shopts.insert("force_fignore".to_string(), true);
        shopts.insert("globasciiranges".to_string(), true);
        shopts.insert("globstar".to_string(), false);
        shopts.insert("gnu_errfmt".to_string(), false);
        shopts.insert("histappend".to_string(), false);
        shopts.insert("histreedit".to_string(), false);
        shopts.insert("histverify".to_string(), false);
        shopts.insert("hostcomplete".to_string(), true);
        shopts.insert("huponexit".to_string(), false);
        shopts.insert("inherit_errexit".to_string(), false);
        shopts.insert("interactive_comments".to_string(), true);
        shopts.insert("lastpipe".to_string(), false);
        shopts.insert("lithist".to_string(), false);
        shopts.insert("localvar_inherit".to_string(), false);
        shopts.insert("localvar_unset".to_string(), false);
        shopts.insert("login_shell".to_string(), false);
        shopts.insert("mailwarn".to_string(), false);
        shopts.insert("no_empty_cmd_completion".to_string(), false);
        shopts.insert("nocaseglob".to_string(), false);
        shopts.insert("nocasematch".to_string(), false);
        shopts.insert("nullglob".to_string(), false);
        shopts.insert("progcomp".to_string(), true);
        shopts.insert("progcomp_alias".to_string(), false);
        shopts.insert("promptvars".to_string(), true);
        shopts.insert("restricted_shell".to_string(), false);
        shopts.insert("shift_verbose".to_string(), false);
        shopts.insert("sourcepath".to_string(), true);
        shopts.insert("xpg_echo".to_string(), false);
        shopts
    }

    /// Get shopt options
    pub fn get_shopts(&self) -> HashMap<String, bool> {
        self.shopts.clone()
    }

    /// Set shopt option
    pub fn set_shopt(&mut self, name: &str, value: bool) {
        self.shopts.insert(name.to_string(), value);
    }

    /// Get set -o options
    pub fn get_set_options(&self) -> HashMap<String, bool> {
        let mut opts = HashMap::new();
        opts.insert("allexport".to_string(), self.options.allexport);
        opts.insert("errexit".to_string(), self.options.errexit);
        opts.insert("hashall".to_string(), self.options.hashall);
        opts.insert("ignoreeof".to_string(), self.options.ignoreeof);
        opts.insert("noglob".to_string(), self.options.noglob);
        opts.insert("notify".to_string(), self.options.notify);
        opts.insert("nounset".to_string(), self.options.nounset);
        opts.insert("verbose".to_string(), self.options.verbose);
        opts.insert("xtrace".to_string(), self.options.xtrace);
        opts.insert("noclobber".to_string(), self.options.noclobber);
        opts
    }

    /// Get a specific set -o option
    pub fn get_set_option(&self, name: &str) -> Option<bool> {
        match name {
            "allexport" => Some(self.options.allexport),
            "errexit" => Some(self.options.errexit),
            "hashall" => Some(self.options.hashall),
            "ignoreeof" => Some(self.options.ignoreeof),
            "noglob" => Some(self.options.noglob),
            "notify" => Some(self.options.notify),
            "nounset" => Some(self.options.nounset),
            "verbose" => Some(self.options.verbose),
            "xtrace" => Some(self.options.xtrace),
            "noclobber" => Some(self.options.noclobber),
            _ => None,
        }
    }

    /// Set a set -o option
    pub fn set_option(&mut self, name: &str, value: bool) -> Result<(), String> {
        match name {
            "allexport" => self.options.allexport = value,
            "errexit" => self.options.errexit = value,
            "hashall" => self.options.hashall = value,
            "ignoreeof" => self.options.ignoreeof = value,
            "noglob" => self.options.noglob = value,
            "notify" => self.options.notify = value,
            "nounset" => self.options.nounset = value,
            "verbose" => self.options.verbose = value,
            "xtrace" => self.options.xtrace = value,
            "noclobber" => self.options.noclobber = value,
            _ => return Err(format!("set: {}: 无效选项名", name)),
        }
        Ok(())
    }

    // ========================================================================
    // Umask
    // ========================================================================

    /// Get umask
    pub fn get_umask(&self) -> u32 {
        self.umask
    }

    /// Set umask
    pub fn set_umask(&mut self, mask: u32) {
        self.umask = mask & 0o777;

        #[cfg(target_family = "unix")]
        unsafe {
            libc::umask(self.umask as libc::mode_t);
        }
    }

    // ========================================================================
    // Array Variables
    // ========================================================================

    /// Set array element
    pub fn set_array_element(&mut self, name: &str, index: usize, value: &str) {
        let arr = self.arrays.entry(name.to_string()).or_insert_with(Vec::new);
        while arr.len() <= index {
            arr.push(String::new());
        }
        arr[index] = value.to_string();
    }

    /// Get array
    pub fn get_array(&self, name: &str) -> Option<&Vec<String>> {
        self.arrays.get(name)
    }

    /// Clear array
    pub fn clear_array(&mut self, name: &str) {
        self.arrays.remove(name);
    }

    // ========================================================================
    // Completion Specs
    // ========================================================================

    /// Set completion spec
    pub fn set_completion_spec(&mut self, name: &str, spec: &str) {
        self.completion_specs
            .insert(name.to_string(), spec.to_string());
    }

    /// Get completion specs
    pub fn get_completion_specs(&self) -> Vec<(&str, &str)> {
        self.completion_specs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    /// Remove completion spec
    pub fn remove_completion_spec(&mut self, name: &str) {
        self.completion_specs.remove(name);
    }

    /// Clear all completion specs
    pub fn clear_completion_specs(&mut self) {
        self.completion_specs.clear();
    }

    /// Set completion option
    pub fn set_completion_option(&mut self, _name: &str, _option: &str, _value: bool) {
        // Completion options are stored per-spec
        // This is a simplified implementation
    }

    // ========================================================================
    // Call Stack (for caller builtin)
    // ========================================================================

    /// Push call frame
    pub fn push_call_frame(&mut self, line: usize, name: &str, file: &str) {
        self.call_stack.push(CallFrame {
            line,
            name: name.to_string(),
            file: file.to_string(),
        });
    }

    /// Pop call frame
    pub fn pop_call_frame(&mut self) {
        self.call_stack.pop();
    }

    /// Get caller info at a specific level
    pub fn get_caller_info(&self, level: usize) -> Option<(usize, String, String)> {
        if level < self.call_stack.len() {
            let idx = self.call_stack.len() - 1 - level;
            let frame = &self.call_stack[idx];
            Some((frame.line, frame.name.clone(), frame.file.clone()))
        } else {
            None
        }
    }

    // ========================================================================
    // Positional Parameters
    // ========================================================================

    /// Set positional parameters
    pub fn set_positional_params(&mut self, params: Vec<String>) {
        self.positional_params = params;
    }

    /// Get positional parameter by index (1-based)
    pub fn get_positional_param(&self, index: usize) -> Option<&str> {
        if index == 0 {
            Some(&self.shell_name)
        } else {
            self.positional_params.get(index - 1).map(|s| s.as_str())
        }
    }

    /// Get number of positional parameters ($#)
    pub fn positional_param_count(&self) -> usize {
        self.positional_params.len()
    }

    /// Get all positional parameters as $@
    pub fn positional_params_at(&self) -> Vec<&str> {
        self.positional_params.iter().map(|s| s.as_str()).collect()
    }

    /// Get all positional parameters as $* (joined by first char of IFS)
    pub fn positional_params_star(&self) -> String {
        let ifs = self.get_var("IFS").unwrap_or(" ");
        let sep = ifs.chars().next().unwrap_or(' ');
        self.positional_params.join(&sep.to_string())
    }

    /// Shift positional parameters
    pub fn shift_positional_params(&mut self, n: usize) -> Result<(), String> {
        if n > self.positional_params.len() {
            return Err(format!(
                "shift: 移动次数 {} 超出位置参数数量 {}",
                n,
                self.positional_params.len()
            ));
        }
        self.positional_params = self.positional_params[n..].to_vec();
        Ok(())
    }

    /// Set shell name ($0)
    pub fn set_shell_name(&mut self, name: &str) {
        self.shell_name = name.to_string();
    }

    /// Get shell name ($0)
    pub fn shell_name(&self) -> &str {
        &self.shell_name
    }

    // ========================================================================
    // Functions
    // ========================================================================

    /// Define a function
    pub fn define_function(&mut self, name: &str, body: &str) {
        self.functions.insert(name.to_string(), body.to_string());
    }

    /// Get function body
    pub fn get_function(&self, name: &str) -> Option<&str> {
        self.functions.get(name).map(|s| s.as_str())
    }

    /// Remove a function
    pub fn unset_function(&mut self, name: &str) -> bool {
        self.functions.remove(name).is_some()
    }

    /// Check if a function exists
    pub fn is_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    /// List all functions
    pub fn list_functions(&self) -> Vec<(&str, &str)> {
        self.functions
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    // ========================================================================
    // Background Process
    // ========================================================================

    /// Set last background PID ($!)
    pub fn set_last_bg_pid(&mut self, pid: i32) {
        self.last_bg_pid = pid;
    }

    /// Get last background PID ($!)
    pub fn last_bg_pid(&self) -> i32 {
        self.last_bg_pid
    }

    /// Set last argument ($_)
    pub fn set_last_arg(&mut self, arg: &str) {
        self.last_arg = arg.to_string();
    }

    /// Get last argument ($_)
    pub fn last_arg(&self) -> &str {
        &self.last_arg
    }
}

impl Default for ShellState {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize a path by resolving . and ..
pub fn normalize_path(path: &Path) -> PathBuf {
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
