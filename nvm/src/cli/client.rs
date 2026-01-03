//! API Client for NVM CLI
//!
//! Handles HTTP communication with nvmserver.

use super::{CliConfig, CliError, CliResult};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// API Client for communicating with nvmserver
pub struct ApiClient {
    config: CliConfig,
}

impl ApiClient {
    /// Create a new API client with the given configuration
    pub fn new(config: CliConfig) -> Self {
        Self { config }
    }
    
    /// Create a client from the saved configuration
    pub fn from_config() -> Self {
        Self::new(super::load_config())
    }
    
    /// Check if the server is reachable
    pub fn health_check(&self) -> CliResult<bool> {
        // Try to connect to the API endpoint
        let url = format!("{}/system/info", self.config.api_url);
        
        match self.do_get::<serde_json::Value>(&url) {
            Ok(_) => Ok(true),
            Err(CliError::Network(msg)) if msg.contains("Connection refused") => {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }
    
    /// Ensure the server is running, return helpful error if not
    pub fn ensure_connected(&self) -> CliResult<()> {
        match self.health_check() {
            Ok(true) => Ok(()),
            Ok(false) => Err(CliError::Network(format!(
                "Cannot connect to nvmserver at {}\n\
                 Please ensure nvmserver is running:\n\
                   - Start with: nvmserver (or systemctl start nvm-server)\n\
                   - Check status: nvmctl system info",
                self.config.api_url
            ))),
            Err(e) => Err(e),
        }
    }
    
    /// Execute a GET request
    pub fn get<T: DeserializeOwned>(&self, endpoint: &str) -> CliResult<T> {
        let url = format!("{}{}", self.config.api_url, endpoint);
        self.do_get(&url)
    }
    
    /// Execute a POST request
    pub fn post<T: DeserializeOwned, B: Serialize>(&self, endpoint: &str, body: &B) -> CliResult<T> {
        let url = format!("{}{}", self.config.api_url, endpoint);
        self.do_post(&url, body)
    }
    
    /// Execute a DELETE request
    pub fn delete(&self, endpoint: &str) -> CliResult<()> {
        let url = format!("{}{}", self.config.api_url, endpoint);
        self.do_delete(&url)
    }
    
    /// Internal GET implementation
    /// 
    /// Note: In a full implementation, this would use reqwest or hyper.
    /// For now, we implement a minimal HTTP client using std::net.
    fn do_get<T: DeserializeOwned>(&self, url: &str) -> CliResult<T> {
        let response = self.http_request("GET", url, None)?;
        serde_json::from_str(&response)
            .map_err(|e| CliError::Api(format!("Failed to parse response: {}", e)))
    }
    
    /// Internal POST implementation
    fn do_post<T: DeserializeOwned, B: Serialize>(&self, url: &str, body: &B) -> CliResult<T> {
        let body_str = serde_json::to_string(body)
            .map_err(|e| CliError::Api(format!("Failed to serialize body: {}", e)))?;
        let response = self.http_request("POST", url, Some(&body_str))?;
        serde_json::from_str(&response)
            .map_err(|e| CliError::Api(format!("Failed to parse response: {}", e)))
    }
    
    /// Internal DELETE implementation
    fn do_delete(&self, url: &str) -> CliResult<()> {
        self.http_request("DELETE", url, None)?;
        Ok(())
    }
    
    /// Minimal HTTP client using std::net
    fn http_request(&self, method: &str, url: &str, body: Option<&str>) -> CliResult<String> {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        
        // Parse URL
        let url = url.trim_start_matches("http://").trim_start_matches("https://");
        let (host_port, path) = match url.find('/') {
            Some(idx) => (&url[..idx], &url[idx..]),
            None => (url, "/"),
        };
        
        // Parse host and port
        let (host, port) = match host_port.find(':') {
            Some(idx) => (&host_port[..idx], host_port[idx + 1..].parse::<u16>().unwrap_or(80)),
            None => (host_port, 80),
        };
        
        // Connect
        let addr = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr)
            .map_err(|e| CliError::Network(format!("Connection refused: {} ({})", addr, e)))?;
        
        stream.set_read_timeout(Some(Duration::from_secs(self.config.timeout)))
            .map_err(|e| CliError::Network(e.to_string()))?;
        stream.set_write_timeout(Some(Duration::from_secs(self.config.timeout)))
            .map_err(|e| CliError::Network(e.to_string()))?;
        
        // Build request
        let mut request = format!("{} {} HTTP/1.1\r\n", method, path);
        request.push_str(&format!("Host: {}\r\n", host_port));
        request.push_str("Connection: close\r\n");
        request.push_str("Accept: application/json\r\n");
        
        if let Some(token) = &self.config.api_token {
            request.push_str(&format!("Authorization: Bearer {}\r\n", token));
        }
        
        if let Some(body) = body {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
            request.push_str("\r\n");
            request.push_str(body);
        } else {
            request.push_str("\r\n");
        }
        
        // Send request
        stream.write_all(request.as_bytes())
            .map_err(|e| CliError::Network(format!("Failed to send request: {}", e)))?;
        
        // Read response
        let mut response = String::new();
        stream.read_to_string(&mut response)
            .map_err(|e| CliError::Network(format!("Failed to read response: {}", e)))?;
        
        // Parse response
        let parts: Vec<&str> = response.splitn(2, "\r\n\r\n").collect();
        if parts.len() != 2 {
            return Err(CliError::Api("Invalid HTTP response".into()));
        }
        
        let headers = parts[0];
        let body = parts[1];
        
        // Check status code
        let status_line = headers.lines().next().unwrap_or("");
        let status_code: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        
        match status_code {
            200..=299 => Ok(body.to_string()),
            401 => Err(CliError::Auth("Authentication required. Use 'nvmctl config login' first.".into())),
            403 => Err(CliError::Auth("Permission denied".into())),
            404 => Err(CliError::NotFound("Resource not found".into())),
            500..=599 => Err(CliError::Api(format!("Server error: {}", body))),
            _ => Err(CliError::Api(format!("HTTP {}: {}", status_code, body))),
        }
    }
}

/// API response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub meta: Option<ResponseMeta>,
}

/// Response metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMeta {
    pub total: Option<u64>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}
