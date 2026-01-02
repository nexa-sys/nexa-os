//! WebGUI Placeholder Modules
//! These modules provide stub implementations for WebGUI components

// Route definitions (implemented in handlers)
pub use super::handlers as routes;

// Dashboard module
pub mod dashboard {
    pub use super::super::handlers::dashboard::*;
}

// Template rendering (stubs)
pub mod templates {
    //! HTML template rendering (using Askama or similar)
    
    /// Base page template context
    pub struct PageContext<'a> {
        pub title: &'a str,
        pub user: Option<&'a str>,
        pub theme: &'a str,
    }
}

// Static asset serving
pub mod assets {
    //! Static asset management
    
    /// Asset manifest for cache busting
    pub struct AssetManifest {
        pub version: String,
        pub files: std::collections::HashMap<String, String>,
    }
    
    impl Default for AssetManifest {
        fn default() -> Self {
            Self {
                version: env!("CARGO_PKG_VERSION").to_string(),
                files: std::collections::HashMap::new(),
            }
        }
    }
}

// Authentication middleware
pub mod middleware {
    //! HTTP middleware for authentication, logging, etc.
    
    /// Authentication middleware state
    pub struct AuthState {
        pub required: bool,
        pub skip_paths: Vec<String>,
    }
    
    impl Default for AuthState {
        fn default() -> Self {
            Self {
                required: true,
                skip_paths: vec![
                    "/api/v2/auth/login".to_string(),
                    "/static".to_string(),
                    "/health".to_string(),
                ],
            }
        }
    }
}
