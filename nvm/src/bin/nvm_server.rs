//! NVM Server Binary
//!
//! Main entry point for the NVM hypervisor server.

use nvm::webgui::{WebGuiServer, WebGuiConfig};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    env_logger::init();
    
    log::info!("Starting NVM Enterprise Server v{}", nvm::VERSION);
    
    // Create configuration
    let config = WebGuiConfig::default();
    
    // Create and start server
    let server = WebGuiServer::new(config);
    
    log::info!("NVM Server initializing...");
    
    // Start the server (blocking)
    server.start_blocking()?;
    
    Ok(())
}
