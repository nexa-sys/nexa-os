//! Shell Builtin Commands Registry
//!
//! This module provides a modular architecture for shell builtin commands.
//! Each category of commands is in its own submodule.

pub mod config;
pub mod flow;
pub mod history;
pub mod info;
pub mod jobs;
pub mod misc;
pub mod navigation;
pub mod utility;
pub mod variables;

use crate::state::ShellState;
use std::collections::HashMap;

/// Builtin command result type
pub type BuiltinResult = Result<i32, String>;

/// Builtin command function signature
pub type BuiltinFn = fn(&mut ShellState, &[&str]) -> BuiltinResult;

/// Builtin command descriptor
pub struct BuiltinDesc {
    /// Function to execute
    pub func: BuiltinFn,
    /// Short description
    pub short_desc: &'static str,
    /// Long description (for help)
    pub long_desc: &'static str,
    /// Usage string
    pub usage: &'static str,
    /// Whether this builtin can be disabled
    pub can_disable: bool,
    /// Whether this builtin is currently enabled
    pub enabled: bool,
}

impl BuiltinDesc {
    pub const fn new(
        func: BuiltinFn,
        short_desc: &'static str,
        long_desc: &'static str,
        usage: &'static str,
        can_disable: bool,
    ) -> Self {
        Self {
            func,
            short_desc,
            long_desc,
            usage,
            can_disable,
            enabled: true,
        }
    }
}

/// Registry of all builtin commands
pub struct BuiltinRegistry {
    builtins: HashMap<&'static str, BuiltinDesc>,
}

impl BuiltinRegistry {
    /// Create a new registry with all builtins registered
    pub fn new() -> Self {
        let mut registry = Self {
            builtins: HashMap::new(),
        };

        // Register all builtin commands
        navigation::register(&mut registry);
        info::register(&mut registry);
        variables::register(&mut registry);
        flow::register(&mut registry);
        utility::register(&mut registry);
        jobs::register(&mut registry);
        history::register(&mut registry);
        config::register(&mut registry);
        misc::register(&mut registry);

        registry
    }

    /// Register a builtin command
    pub fn register(&mut self, name: &'static str, desc: BuiltinDesc) {
        self.builtins.insert(name, desc);
    }

    /// Check if a command is a builtin
    pub fn is_builtin(&self, name: &str) -> bool {
        self.builtins.contains_key(name)
    }

    /// Check if a builtin is enabled
    pub fn is_enabled(&self, name: &str) -> bool {
        self.builtins.get(name).map(|b| b.enabled).unwrap_or(false)
    }

    /// Enable or disable a builtin
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<(), String> {
        if let Some(builtin) = self.builtins.get_mut(name) {
            if builtin.can_disable {
                builtin.enabled = enabled;
                Ok(())
            } else {
                Err(format!("{}: 无法被禁用", name))
            }
        } else {
            Err(format!("{}: 不是 shell 内建命令", name))
        }
    }

    /// Execute a builtin command
    pub fn execute(
        &self,
        name: &str,
        state: &mut ShellState,
        args: &[&str],
    ) -> Option<BuiltinResult> {
        self.builtins.get(name).and_then(|builtin| {
            if builtin.enabled {
                Some((builtin.func)(state, args))
            } else {
                None
            }
        })
    }

    /// Get builtin descriptor
    pub fn get(&self, name: &str) -> Option<&BuiltinDesc> {
        self.builtins.get(name)
    }

    /// Get mutable builtin descriptor
    pub fn get_mut(&mut self, name: &str) -> Option<&mut BuiltinDesc> {
        self.builtins.get_mut(name)
    }

    /// List all builtin names
    pub fn list_builtins(&self) -> Vec<String> {
        let mut names: Vec<_> = self.builtins.keys().map(|&s| s.to_string()).collect();
        names.sort();
        names
    }

    /// List all builtins with descriptions
    pub fn list_with_desc(&self) -> Vec<(&'static str, &'static str, bool)> {
        let mut list: Vec<_> = self
            .builtins
            .iter()
            .map(|(&name, desc)| (name, desc.short_desc, desc.enabled))
            .collect();
        list.sort_by_key(|(name, _, _)| *name);
        list
    }

    /// Get disabled builtins
    pub fn list_disabled(&self) -> Vec<&'static str> {
        let mut list: Vec<_> = self
            .builtins
            .iter()
            .filter(|(_, desc)| !desc.enabled && desc.can_disable)
            .map(|(&name, _)| name)
            .collect();
        list.sort();
        list
    }
}

impl Default for BuiltinRegistry {
    fn default() -> Self {
        Self::new()
    }
}
