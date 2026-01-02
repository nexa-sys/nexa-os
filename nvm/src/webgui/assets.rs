//! Static Asset Management

use std::collections::HashMap;
use std::path::PathBuf;

/// Asset manager for static files
pub struct AssetManager {
    /// Base path for assets
    pub base_path: PathBuf,
    /// In-memory cache
    cache: HashMap<String, Vec<u8>>,
    /// Enable caching
    caching_enabled: bool,
}

impl AssetManager {
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            base_path,
            cache: HashMap::new(),
            caching_enabled: true,
        }
    }

    /// Disable caching (for development)
    pub fn disable_cache(mut self) -> Self {
        self.caching_enabled = false;
        self
    }

    /// Get asset content
    pub fn get(&mut self, path: &str) -> Option<Vec<u8>> {
        if self.caching_enabled {
            if let Some(cached) = self.cache.get(path) {
                return Some(cached.clone());
            }
        }

        let full_path = self.base_path.join(path);
        let content = std::fs::read(&full_path).ok()?;

        if self.caching_enabled {
            self.cache.insert(path.to_string(), content.clone());
        }

        Some(content)
    }

    /// Get content type for file
    pub fn content_type(path: &str) -> &'static str {
        match path.rsplit('.').next() {
            Some("html") => "text/html",
            Some("css") => "text/css",
            Some("js") => "application/javascript",
            Some("json") => "application/json",
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("svg") => "image/svg+xml",
            Some("ico") => "image/x-icon",
            Some("woff") => "font/woff",
            Some("woff2") => "font/woff2",
            _ => "application/octet-stream",
        }
    }

    /// Clear cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

impl Default for AssetManager {
    fn default() -> Self {
        Self::new(PathBuf::from("/usr/share/nvm/assets"))
    }
}
