//! NVM Server Binary
//!
//! Main entry point for the NVM hypervisor server.
//! Requires the 'webgui' feature to be enabled.

#[cfg(feature = "webgui")]
use nvm::webgui::{WebGuiServer, WebGuiConfig};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();
    
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║         NVM Enterprise Hypervisor Server v{}          ║", nvm::VERSION);
    println!("║         NexaOS Virtual Machine Platform                    ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    
    #[cfg(feature = "webgui")]
    {
        log::info!("Starting NVM Enterprise Server v{}", nvm::VERSION);
        
        // Load or create configuration
        let config = load_server_config();
        
        log::info!("Bind address: {}:{}", config.bind_address, config.http_port);
        log::info!("TLS enabled: {}", config.tls_enabled);
        
        // Create and start server
        let server = WebGuiServer::new(config);
        
        log::info!("NVM Server initializing...");
        log::info!("Web interface will be available at http://{}:{}", 
            "localhost", 8006);
        log::info!("API endpoint: http://{}:{}/api/v2", "localhost", 8006);
        
        // Start the server (blocking, handles Ctrl+C gracefully via tokio signals)
        server.start_blocking()?;
        
        Ok(())
    }
    
    #[cfg(not(feature = "webgui"))]
    {
        eprintln!("ERROR: The 'webgui' feature is not enabled.");
        eprintln!();
        eprintln!("NVM Server requires the webgui feature to run.");
        eprintln!("Please rebuild with:");
        eprintln!();
        eprintln!("    cargo build --features webgui -p nvm --bin nvm-server");
        eprintln!();
        eprintln!("Or build with all features:");
        eprintln!();
        eprintln!("    cargo build --features full -p nvm --bin nvm-server");
        eprintln!();
        Err("webgui feature not enabled".into())
    }
}

#[cfg(feature = "webgui")]
fn load_server_config() -> WebGuiConfig {
    use std::path::PathBuf;
    
    // Try to load from config file
    let config_paths = [
        PathBuf::from("/etc/nvm/server.yaml"),
        dirs::config_dir().unwrap_or_default().join("nvm/server.yaml"),
        PathBuf::from("nvm-server.yaml"),
    ];
    
    for path in &config_paths {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(config) = serde_yaml::from_str(&content) {
                    log::info!("Loaded configuration from {:?}", path);
                    return config;
                }
            }
        }
    }
    
    log::info!("Using default configuration");
    WebGuiConfig::default()
}
