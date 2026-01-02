//! HTML Templates
//!
//! Server-side rendered templates for the WebGUI.

use std::collections::HashMap;

/// Template engine (simple implementation)
pub struct TemplateEngine {
    templates: HashMap<String, String>,
}

impl TemplateEngine {
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// Load a template
    pub fn load(&mut self, name: &str, content: &str) {
        self.templates.insert(name.to_string(), content.to_string());
    }

    /// Render a template with context
    pub fn render(&self, name: &str, context: &HashMap<String, String>) -> Option<String> {
        let template = self.templates.get(name)?;
        let mut result = template.clone();
        
        for (key, value) in context {
            let placeholder = format!("{{{{{}}}}}", key);
            result = result.replace(&placeholder, value);
        }
        
        Some(result)
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Built-in templates
pub mod builtin {
    /// Login page template
    pub const LOGIN_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>NVM Login</title></head>
<body>
<h1>NVM Enterprise Login</h1>
<form method="post" action="/api/v2/auth/login">
  <input type="text" name="username" placeholder="Username" />
  <input type="password" name="password" placeholder="Password" />
  <button type="submit">Login</button>
</form>
</body>
</html>"#;

    /// Error page template
    pub const ERROR_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>Error</title></head>
<body>
<h1>Error {{code}}</h1>
<p>{{message}}</p>
</body>
</html>"#;
}
