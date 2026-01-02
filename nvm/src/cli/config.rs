//! CLI Configuration management

use super::{CliConfig, CliResult, CliError};
use std::path::PathBuf;

/// Configuration commands
pub fn configure(key: &str, value: &str) -> CliResult<()> {
    let mut config = super::load_config();
    
    match key {
        "api_url" | "api-url" | "url" => {
            config.api_url = value.to_string();
        }
        "api_token" | "api-token" | "token" => {
            config.api_token = Some(value.to_string());
        }
        "output" | "format" => {
            config.output_format = value.parse()
                .map_err(|e| CliError::InvalidArg(e))?;
        }
        "verify_tls" | "verify-tls" | "tls" => {
            config.verify_tls = value.parse()
                .map_err(|_| CliError::InvalidArg("Expected true/false".to_string()))?;
        }
        "timeout" => {
            config.timeout = value.parse()
                .map_err(|_| CliError::InvalidArg("Expected number".to_string()))?;
        }
        "default_node" | "default-node" | "node" => {
            config.default_node = Some(value.to_string());
        }
        _ => {
            return Err(CliError::InvalidArg(format!("Unknown config key: {}", key)));
        }
    }
    
    super::save_config(&config)?;
    Ok(())
}

/// Show current configuration
pub fn show() -> CliResult<CliConfig> {
    Ok(super::load_config())
}

/// Login and save token
pub fn login(url: &str, username: &str, password: &str) -> CliResult<()> {
    // In real implementation, call API to get token
    let mut config = super::load_config();
    config.api_url = url.to_string();
    config.api_token = Some("demo-token".to_string()); // Would be real token
    super::save_config(&config)?;
    Ok(())
}

/// Clear saved credentials
pub fn logout() -> CliResult<()> {
    let mut config = super::load_config();
    config.api_token = None;
    super::save_config(&config)?;
    Ok(())
}

/// Interactive setup wizard
pub fn setup() -> CliResult<()> {
    use std::io::{self, Write};
    
    println!("NVM CLI Setup Wizard");
    println!("====================\n");
    
    let mut config = CliConfig::default();
    
    // Get API URL
    print!("API URL [{}]: ", config.api_url);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let api_input = input.trim();
    if !api_input.is_empty() {
        config.api_url = api_input.to_string();
    }
    
    // Get username
    print!("Username: ");
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    let username = input.trim().to_string();
    
    // Get password
    print!("Password: ");
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    let _password = input.trim();
    
    // In real implementation, authenticate and get token
    config.api_token = Some(format!("token-for-{}", username));
    
    // Save config
    super::save_config(&config)?;
    
    println!("\n✓ Configuration saved to {:?}", super::config_path());
    Ok(())
}
