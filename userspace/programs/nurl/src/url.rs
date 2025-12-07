/// URL parsing for nurl

/// Parsed URL components
#[derive(Debug, Clone)]
pub struct ParsedUrl {
    /// Whether this is an HTTPS URL
    pub is_https: bool,
    /// Hostname
    pub host: String,
    /// Port number
    pub port: u16,
    /// Path (including query string)
    pub path: String,
    /// Original URL
    pub original: String,
}

impl ParsedUrl {
    /// Get the authority string (host:port or just host for default ports)
    pub fn authority(&self) -> String {
        let default_port = if self.is_https { 443 } else { 80 };
        if self.port == default_port {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// Get the scheme string
    pub fn scheme(&self) -> &str {
        if self.is_https {
            "https"
        } else {
            "http"
        }
    }

    /// Get the address string for connection (host:port)
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Parse URL into components
/// 
/// Supports:
/// - `http://host/path`
/// - `https://host/path`
/// - `http://host:port/path`
/// - `host/path` (defaults to http)
pub fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    // Check for https:// or http:// prefix
    let (is_https, url_rest) = if url.starts_with("https://") {
        (true, &url[8..])
    } else if url.starts_with("http://") {
        (false, &url[7..])
    } else {
        // Default to HTTP if no scheme specified
        (false, url)
    };

    // Default port based on protocol
    let default_port = if is_https { 443 } else { 80 };

    // Find the first / to split host and path
    let (host_port, path) = if let Some(slash_pos) = url_rest.find('/') {
        (&url_rest[..slash_pos], url_rest[slash_pos..].to_string())
    } else {
        (url_rest, "/".to_string())
    };

    // Handle IPv6 addresses in brackets [::1]
    let (host, port) = if host_port.starts_with('[') {
        // IPv6 address
        if let Some(bracket_end) = host_port.find(']') {
            let host = host_port[1..bracket_end].to_string();
            let after_bracket = &host_port[bracket_end + 1..];
            if after_bracket.starts_with(':') {
                let port_str = &after_bracket[1..];
                let port = port_str
                    .parse::<u16>()
                    .map_err(|_| "Invalid port number".to_string())?;
                (host, port)
            } else {
                (host, default_port)
            }
        } else {
            return Err("Invalid IPv6 address - missing closing bracket".to_string());
        }
    } else {
        // IPv4 or hostname - split host and port
        if let Some(colon_pos) = host_port.rfind(':') {
            let host = host_port[..colon_pos].to_string();
            let port_str = &host_port[colon_pos + 1..];
            let port = port_str
                .parse::<u16>()
                .map_err(|_| "Invalid port number".to_string())?;
            (host, port)
        } else {
            (host_port.to_string(), default_port)
        }
    };

    if host.is_empty() {
        return Err("Empty hostname".to_string());
    }

    Ok(ParsedUrl {
        is_https,
        host,
        port,
        path,
        original: url.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_http() {
        let url = parse_url("http://example.com/path").unwrap();
        assert!(!url.is_https);
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/path");
    }

    #[test]
    fn test_parse_https_with_port() {
        let url = parse_url("https://example.com:8443/api").unwrap();
        assert!(url.is_https);
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 8443);
        assert_eq!(url.path, "/api");
    }

    #[test]
    fn test_parse_no_scheme() {
        let url = parse_url("example.com/test").unwrap();
        assert!(!url.is_https);
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/test");
    }

    #[test]
    fn test_parse_no_path() {
        let url = parse_url("https://example.com").unwrap();
        assert_eq!(url.path, "/");
    }
}
