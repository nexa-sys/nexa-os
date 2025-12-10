//! Vim Script environment (variable scope management)

use super::Value;
use std::collections::HashMap;

/// Variable scope
#[derive(Debug, Clone)]
struct Scope {
    variables: HashMap<String, Value>,
}

impl Scope {
    fn new() -> Self {
        Scope {
            variables: HashMap::new(),
        }
    }
}

/// Environment for variable management
pub struct Environment {
    /// Global variables (g:)
    global: HashMap<String, Value>,
    /// Buffer-local variables (b:)
    buffer: HashMap<String, Value>,
    /// Window-local variables (w:)
    window: HashMap<String, Value>,
    /// Tab-local variables (t:)
    tab: HashMap<String, Value>,
    /// Vim variables (v:)
    vim: HashMap<String, Value>,
    /// Script-local variables (s:) - stack for nested scripts
    script_stack: Vec<HashMap<String, Value>>,
    /// Local variable scopes (function call stack)
    local_stack: Vec<Scope>,
    /// Environment variables
    env_vars: HashMap<String, Value>,
}

impl Environment {
    /// Create a new environment
    pub fn new() -> Self {
        let mut env = Environment {
            global: HashMap::new(),
            buffer: HashMap::new(),
            window: HashMap::new(),
            tab: HashMap::new(),
            vim: HashMap::new(),
            script_stack: vec![HashMap::new()],
            local_stack: Vec::new(),
            env_vars: HashMap::new(),
        };

        // Initialize vim variables
        env.init_vim_variables();

        env
    }

    /// Initialize default vim variables
    fn init_vim_variables(&mut self) {
        self.vim.insert("version".to_string(), Value::Integer(900));
        self.vim
            .insert("progname".to_string(), Value::String("edit".to_string()));
        self.vim.insert("true".to_string(), Value::Integer(1));
        self.vim.insert("false".to_string(), Value::Integer(0));
        self.vim.insert("null".to_string(), Value::Null);
        self.vim.insert("none".to_string(), Value::Null);
        self.vim.insert("count".to_string(), Value::Integer(0));
        self.vim.insert("count1".to_string(), Value::Integer(1));
        self.vim
            .insert("errmsg".to_string(), Value::String(String::new()));
        self.vim
            .insert("statusmsg".to_string(), Value::String(String::new()));
        self.vim
            .insert("warningmsg".to_string(), Value::String(String::new()));
        self.vim
            .insert("shell_error".to_string(), Value::Integer(0));
        self.vim.insert("t_string".to_string(), Value::Integer(1));
        self.vim.insert("t_number".to_string(), Value::Integer(0));
        self.vim.insert("t_float".to_string(), Value::Integer(5));
        self.vim.insert("t_list".to_string(), Value::Integer(3));
        self.vim.insert("t_dict".to_string(), Value::Integer(4));
        self.vim.insert("t_func".to_string(), Value::Integer(2));
        self.vim.insert("t_bool".to_string(), Value::Integer(6));
        self.vim.insert("t_none".to_string(), Value::Integer(7));

        // Load environment variables
        if let Ok(home) = std::env::var("HOME") {
            self.env_vars
                .insert("HOME".to_string(), Value::String(home));
        }
        if let Ok(user) = std::env::var("USER") {
            self.env_vars
                .insert("USER".to_string(), Value::String(user));
        }
        if let Ok(path) = std::env::var("PATH") {
            self.env_vars
                .insert("PATH".to_string(), Value::String(path));
        }
        if let Ok(term) = std::env::var("TERM") {
            self.env_vars
                .insert("TERM".to_string(), Value::String(term));
        }
    }

    /// Get a variable by name
    pub fn get(&self, name: &str) -> Option<Value> {
        // Parse scope prefix
        if let Some(rest) = name.strip_prefix("g:") {
            return self.global.get(rest).cloned();
        }
        if let Some(rest) = name.strip_prefix("b:") {
            return self.buffer.get(rest).cloned();
        }
        if let Some(rest) = name.strip_prefix("w:") {
            return self.window.get(rest).cloned();
        }
        if let Some(rest) = name.strip_prefix("t:") {
            return self.tab.get(rest).cloned();
        }
        if let Some(rest) = name.strip_prefix("v:") {
            return self.vim.get(rest).cloned();
        }
        if let Some(rest) = name.strip_prefix("s:") {
            if let Some(script_vars) = self.script_stack.last() {
                return script_vars.get(rest).cloned();
            }
            return None;
        }
        if let Some(rest) = name.strip_prefix("l:") {
            if let Some(scope) = self.local_stack.last() {
                return scope.variables.get(rest).cloned();
            }
            return None;
        }
        if let Some(rest) = name.strip_prefix("a:") {
            // Function arguments
            if let Some(scope) = self.local_stack.last() {
                return scope.variables.get(&format!("a:{}", rest)).cloned();
            }
            return None;
        }
        if let Some(rest) = name.strip_prefix("$") {
            // Environment variable
            return self
                .env_vars
                .get(rest)
                .cloned()
                .or_else(|| std::env::var(rest).ok().map(Value::String));
        }
        if let Some(rest) = name.strip_prefix("&") {
            // Option (handled elsewhere)
            let _ = rest;
            return None;
        }
        if let Some(rest) = name.strip_prefix("@") {
            // Register (simplified)
            let _ = rest;
            return Some(Value::String(String::new()));
        }

        // No prefix - search in order: local, script, global
        if let Some(scope) = self.local_stack.last() {
            if let Some(val) = scope.variables.get(name) {
                return Some(val.clone());
            }
        }

        if let Some(script_vars) = self.script_stack.last() {
            if let Some(val) = script_vars.get(name) {
                return Some(val.clone());
            }
        }

        self.global.get(name).cloned()
    }

    /// Set a variable
    pub fn set(&mut self, name: &str, value: Value) {
        // Parse scope prefix
        if let Some(rest) = name.strip_prefix("g:") {
            self.global.insert(rest.to_string(), value);
            return;
        }
        if let Some(rest) = name.strip_prefix("b:") {
            self.buffer.insert(rest.to_string(), value);
            return;
        }
        if let Some(rest) = name.strip_prefix("w:") {
            self.window.insert(rest.to_string(), value);
            return;
        }
        if let Some(rest) = name.strip_prefix("t:") {
            self.tab.insert(rest.to_string(), value);
            return;
        }
        if let Some(rest) = name.strip_prefix("v:") {
            self.vim.insert(rest.to_string(), value);
            return;
        }
        if let Some(rest) = name.strip_prefix("s:") {
            if let Some(script_vars) = self.script_stack.last_mut() {
                script_vars.insert(rest.to_string(), value);
            }
            return;
        }
        if let Some(rest) = name.strip_prefix("l:") {
            if let Some(scope) = self.local_stack.last_mut() {
                scope.variables.insert(rest.to_string(), value);
            }
            return;
        }
        if name.starts_with("a:") {
            // Function arguments (store with prefix)
            if let Some(scope) = self.local_stack.last_mut() {
                scope.variables.insert(name.to_string(), value);
            }
            return;
        }
        if let Some(rest) = name.strip_prefix("$") {
            self.env_vars.insert(rest.to_string(), value);
            return;
        }

        // No prefix - set in current scope (local if in function, else global)
        if let Some(scope) = self.local_stack.last_mut() {
            scope.variables.insert(name.to_string(), value);
        } else {
            self.global.insert(name.to_string(), value);
        }
    }

    /// Unset a variable
    pub fn unset(&mut self, name: &str) {
        if let Some(rest) = name.strip_prefix("g:") {
            self.global.remove(rest);
            return;
        }
        if let Some(rest) = name.strip_prefix("b:") {
            self.buffer.remove(rest);
            return;
        }
        if let Some(rest) = name.strip_prefix("w:") {
            self.window.remove(rest);
            return;
        }
        if let Some(rest) = name.strip_prefix("t:") {
            self.tab.remove(rest);
            return;
        }
        if let Some(rest) = name.strip_prefix("s:") {
            if let Some(script_vars) = self.script_stack.last_mut() {
                script_vars.remove(rest);
            }
            return;
        }
        if let Some(rest) = name.strip_prefix("l:") {
            if let Some(scope) = self.local_stack.last_mut() {
                scope.variables.remove(rest);
            }
            return;
        }

        // No prefix - try all scopes
        if let Some(scope) = self.local_stack.last_mut() {
            scope.variables.remove(name);
        }
        if let Some(script_vars) = self.script_stack.last_mut() {
            script_vars.remove(name);
        }
        self.global.remove(name);
    }

    /// Check if variable exists
    pub fn exists(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Push a new local scope (for function calls)
    pub fn push_scope(&mut self) {
        self.local_stack.push(Scope::new());
    }

    /// Pop a local scope
    pub fn pop_scope(&mut self) {
        self.local_stack.pop();
    }

    /// Push a new script scope
    pub fn push_script_scope(&mut self) {
        self.script_stack.push(HashMap::new());
    }

    /// Pop script scope
    pub fn pop_script_scope(&mut self) {
        if self.script_stack.len() > 1 {
            self.script_stack.pop();
        }
    }

    /// Get all global variables
    pub fn globals(&self) -> &HashMap<String, Value> {
        &self.global
    }

    /// Set vim variable
    pub fn set_vim_var(&mut self, name: &str, value: Value) {
        self.vim.insert(name.to_string(), value);
    }

    /// Get vim variable
    pub fn get_vim_var(&self, name: &str) -> Option<Value> {
        self.vim.get(name).cloned()
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}
