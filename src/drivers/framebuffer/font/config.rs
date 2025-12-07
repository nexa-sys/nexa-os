//! Font configuration parser
//!
//! This module parses fontconfig-style XML configuration files
//! to determine font directories, preferences, and fallback chains.
//!
//! Supports a subset of the fontconfig format:
//! - `<dir>` elements for font directories
//! - `<include>` elements for including conf.d fragments
//! - `<alias>` elements for font family preferences

use alloc::string::String;
use alloc::vec::Vec;

/// Maximum number of font directories
const MAX_FONT_DIRS: usize = 16;

/// Maximum number of font preferences per family
const MAX_PREFERENCES: usize = 8;

/// Font family alias with preference list
#[derive(Clone)]
pub struct FontAlias {
    pub family: String,
    pub prefer: Vec<String>,
}

/// Font configuration
pub struct FontConfig {
    /// Font directories to search
    pub directories: Vec<String>,
    /// Font aliases (family -> preferred fonts)
    pub aliases: Vec<FontAlias>,
    /// Cache directory
    pub cache_dir: Option<String>,
}

impl FontConfig {
    /// Create a default configuration
    pub fn default() -> Self {
        let mut directories = Vec::new();
        directories.push(String::from("/usr/share/fonts"));
        directories.push(String::from("/usr/share/fonts/truetype"));
        directories.push(String::from("/usr/local/share/fonts"));

        Self {
            directories,
            aliases: Vec::new(),
            cache_dir: None,
        }
    }

    /// Load configuration from a file path
    pub fn load_from_file(path: &str) -> Option<Self> {
        let content = crate::fs::read_file_bytes(path)?;
        let content_str = core::str::from_utf8(content).ok()?;
        Self::parse(content_str)
    }

    /// Parse fontconfig XML content
    pub fn parse(content: &str) -> Option<Self> {
        let mut config = Self {
            directories: Vec::new(),
            aliases: Vec::new(),
            cache_dir: None,
        };

        let mut parser = XmlParser::new(content);

        while let Some(event) = parser.next() {
            match event {
                XmlEvent::StartElement { name, attrs } => {
                    match name {
                        "dir" => {
                            if let Some(text) = parser.read_text() {
                                if config.directories.len() < MAX_FONT_DIRS {
                                    let expanded = Self::expand_dir_path(text.trim(), attrs);
                                    if !expanded.is_empty() {
                                        config.directories.push(expanded);
                                    }
                                }
                            }
                        }
                        "cachedir" => {
                            if let Some(text) = parser.read_text() {
                                config.cache_dir = Some(String::from(text.trim()));
                            }
                        }
                        "include" => {
                            if let Some(text) = parser.read_text() {
                                // Load and merge included config
                                if let Some(included) = Self::load_from_file(text.trim()) {
                                    config.merge(&included);
                                }
                            }
                        }
                        "alias" => {
                            if let Some(alias) = Self::parse_alias(&mut parser) {
                                config.aliases.push(alias);
                            }
                        }
                        _ => {}
                    }
                }
                XmlEvent::EndElement { .. } => {}
                XmlEvent::Text(_) => {}
            }
        }

        // Ensure we have at least default directories
        if config.directories.is_empty() {
            config.directories.push(String::from("/usr/share/fonts"));
        }

        Some(config)
    }

    /// Parse an alias element
    fn parse_alias(parser: &mut XmlParser) -> Option<FontAlias> {
        let mut family: Option<String> = None;
        let mut prefer: Vec<String> = Vec::new();
        let mut in_prefer = false;

        while let Some(event) = parser.next() {
            match event {
                XmlEvent::StartElement { name, .. } => {
                    match name {
                        "family" => {
                            if let Some(text) = parser.read_text() {
                                if family.is_none() && !in_prefer {
                                    family = Some(String::from(text.trim()));
                                } else if in_prefer && prefer.len() < MAX_PREFERENCES {
                                    prefer.push(String::from(text.trim()));
                                }
                            }
                        }
                        "prefer" => {
                            in_prefer = true;
                        }
                        _ => {}
                    }
                }
                XmlEvent::EndElement { name } => {
                    if name == "alias" {
                        break;
                    } else if name == "prefer" {
                        in_prefer = false;
                    }
                }
                XmlEvent::Text(_) => {}
            }
        }

        family.map(|f| FontAlias { family: f, prefer })
    }

    /// Merge another configuration into this one
    fn merge(&mut self, other: &FontConfig) {
        for dir in &other.directories {
            if self.directories.len() < MAX_FONT_DIRS && !self.directories.contains(dir) {
                self.directories.push(dir.clone());
            }
        }

        for alias in &other.aliases {
            // Check if we already have this alias
            let existing = self.aliases.iter_mut().find(|a| a.family == alias.family);
            if let Some(existing) = existing {
                // Merge preferences
                for pref in &alias.prefer {
                    if existing.prefer.len() < MAX_PREFERENCES && !existing.prefer.contains(pref) {
                        existing.prefer.push(pref.clone());
                    }
                }
            } else {
                self.aliases.push(alias.clone());
            }
        }

        if self.cache_dir.is_none() {
            self.cache_dir = other.cache_dir.clone();
        }
    }

    /// Get preferred fonts for a family name
    pub fn get_preferred(&self, family: &str) -> Option<&[String]> {
        self.aliases
            .iter()
            .find(|a| a.family == family)
            .map(|a| a.prefer.as_slice())
    }

    /// Get all font directories
    pub fn get_directories(&self) -> &[String] {
        &self.directories
    }

    /// Expand font directory path based on prefix attribute
    /// - prefix="xdg" -> XDG_DATA_HOME/path or /root/.local/share/path
    /// - ~ at start -> /root (kernel runs as root)
    /// - relative path -> skip (not valid in kernel context)
    fn expand_dir_path(path: &str, attrs: &str) -> String {
        // Check for prefix="xdg" attribute
        let has_xdg_prefix = attrs.contains("prefix=\"xdg\"") || attrs.contains("prefix='xdg'");
        
        if has_xdg_prefix {
            // XDG_DATA_HOME defaults to ~/.local/share
            let mut result = String::from("/root/.local/share/");
            result.push_str(path);
            return result;
        }
        
        // Expand ~ to /root
        if path.starts_with("~/") {
            let mut result = String::from("/root");
            result.push_str(&path[1..]); // Skip the ~, keep the /
            return result;
        }
        
        if path == "~" {
            return String::from("/root");
        }
        
        // Skip relative paths (not valid for kernel font loading)
        if !path.starts_with('/') {
            return String::new(); // Return empty to indicate skip
        }
        
        String::from(path)
    }
}

/// XML parser events
#[derive(Debug)]
enum XmlEvent<'a> {
    StartElement { name: &'a str, attrs: &'a str },
    EndElement { name: &'a str },
    Text(&'a str),
}

/// Simple XML parser for fontconfig files
struct XmlParser<'a> {
    content: &'a str,
    pos: usize,
}

impl<'a> XmlParser<'a> {
    fn new(content: &'a str) -> Self {
        Self { content, pos: 0 }
    }

    fn next(&mut self) -> Option<XmlEvent<'a>> {
        self.skip_whitespace();

        if self.pos >= self.content.len() {
            return None;
        }

        let remaining = &self.content[self.pos..];

        // Check for comment
        if remaining.starts_with("<!--") {
            if let Some(end) = remaining.find("-->") {
                self.pos += end + 3;
                return self.next();
            }
            return None;
        }

        // Check for DOCTYPE or XML declaration
        if remaining.starts_with("<!") || remaining.starts_with("<?") {
            if let Some(end) = remaining.find('>') {
                self.pos += end + 1;
                return self.next();
            }
            return None;
        }

        // Check for tag
        if remaining.starts_with('<') {
            if remaining.starts_with("</") {
                // End tag
                let tag_start = self.pos + 2;
                if let Some(end) = remaining.find('>') {
                    let name = &self.content[tag_start..self.pos + end];
                    let name = name.trim();
                    self.pos += end + 1;
                    return Some(XmlEvent::EndElement { name });
                }
            } else {
                // Start tag
                let tag_start = self.pos + 1;
                if let Some(end) = remaining.find('>') {
                    let tag_content = &self.content[tag_start..self.pos + end];
                    
                    // Check for self-closing tag
                    let is_self_closing = tag_content.ends_with('/');
                    let tag_content = if is_self_closing {
                        &tag_content[..tag_content.len() - 1]
                    } else {
                        tag_content
                    };

                    // Extract tag name (first word) and remaining attrs
                    let mut parts = tag_content.splitn(2, |c: char| c.is_whitespace());
                    let name = parts.next().unwrap_or("").trim();
                    let attrs = parts.next().unwrap_or("").trim();

                    self.pos += end + 1;

                    if is_self_closing {
                        // Return start then queue end
                        return Some(XmlEvent::StartElement { name, attrs });
                    }

                    return Some(XmlEvent::StartElement { name, attrs });
                }
            }
        } else {
            // Text content
            let text_end = remaining.find('<').unwrap_or(remaining.len());
            if text_end > 0 {
                let text = &remaining[..text_end];
                self.pos += text_end;
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(XmlEvent::Text(trimmed));
                }
                return self.next();
            }
        }

        self.pos += 1;
        self.next()
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.content.len() {
            let c = self.content.as_bytes()[self.pos];
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn read_text(&mut self) -> Option<&'a str> {
        self.skip_whitespace();

        if self.pos >= self.content.len() {
            return None;
        }

        let remaining = &self.content[self.pos..];

        // Read until we hit a tag
        let text_end = remaining.find('<').unwrap_or(remaining.len());
        if text_end > 0 {
            let text = &remaining[..text_end];
            self.pos += text_end;
            return Some(text.trim());
        }

        None
    }
}
