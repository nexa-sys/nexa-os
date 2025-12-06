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
    pub errexit: bool,      // -e: Exit on error
    pub nounset: bool,      // -u: Error on unset variables
    pub xtrace: bool,       // -x: Print commands before execution
    pub verbose: bool,      // -v: Print input lines
    pub noclobber: bool,    // -C: Don't overwrite files with >
    pub allexport: bool,    // -a: Export all variables
    pub notify: bool,       // -b: Notify immediately of job termination
    pub noglob: bool,       // -f: Disable pathname expansion
    pub ignoreeof: bool,    // Ignore EOF (Ctrl-D)
    pub hashall: bool,      // -h: Hash commands
    pub interactive: bool,  // Shell is interactive
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
}

/// Flow control signals for break/continue/return
#[derive(Clone, Debug)]
pub enum FlowControl {
    Break(usize),
    Continue(usize),
    Return(i32),
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
            "?" => return Some(Box::leak(self.last_exit_status.to_string().into_boxed_str())),
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
        let mut list: Vec<_> = self.aliases.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
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
        let mut list: Vec<_> = self.hash_table
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
