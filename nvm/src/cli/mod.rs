//! NVM Command Line Interface
//!
//! Enterprise CLI tool for managing NVM hypervisor platform.
//! Similar to: virsh, prlctl, VBoxManage, esxcli

pub mod commands;
pub mod output;
pub mod config;
pub mod shell;
pub mod client;
pub mod state;

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// CLI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// API endpoint URL
    pub api_url: String,
    /// API token for authentication
    pub api_token: Option<String>,
    /// Default output format
    pub output_format: OutputFormat,
    /// Verify TLS certificates
    pub verify_tls: bool,
    /// Connection timeout (seconds)
    pub timeout: u64,
    /// Default node (for multi-node operations)
    pub default_node: Option<String>,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            api_url: "https://localhost:8006/api/v2".to_string(),
            api_token: None,
            output_format: OutputFormat::Table,
            verify_tls: true,
            timeout: 30,
            default_node: None,
        }
    }
}

/// Output format options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
    Csv,
    Plain,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "json" => Ok(OutputFormat::Json),
            "yaml" => Ok(OutputFormat::Yaml),
            "csv" => Ok(OutputFormat::Csv),
            "plain" | "text" => Ok(OutputFormat::Plain),
            _ => Err(format!("Unknown output format: {}", s)),
        }
    }
}

/// CLI command result
pub type CliResult<T> = Result<T, CliError>;

/// CLI errors
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("API error: {0}")]
    Api(String),
    
    #[error("Authentication failed: {0}")]
    Auth(String),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Resource not found: {0}")]
    NotFound(String),
    
    #[error("Invalid argument: {0}")]
    InvalidArg(String),
    
    #[error("Operation failed: {0}")]
    Operation(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Get config file path
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nvm")
        .join("config.yaml")
}

/// Load CLI configuration
pub fn load_config() -> CliConfig {
    let path = config_path();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_yaml::from_str(&content) {
                return config;
            }
        }
    }
    CliConfig::default()
}

/// Save CLI configuration
pub fn save_config(config: &CliConfig) -> Result<(), CliError> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_yaml::to_string(config)
        .map_err(|e| CliError::Config(e.to_string()))?;
    std::fs::write(&path, content)?;
    Ok(())
}
