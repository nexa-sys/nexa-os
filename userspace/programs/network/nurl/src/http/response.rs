/// HTTP response parsing utilities

use super::{HttpError, HttpResponse, HttpResult};

/// Parse an HTTP/1.1 response from raw bytes
pub fn parse_http1_response(data: &[u8]) -> HttpResult<HttpResponse> {
    // Find end of headers by searching for \r\n\r\n in raw bytes
    let header_end = find_header_end(data)
        .ok_or_else(|| HttpError::InvalidResponse("No header end found".to_string()))?;

    // Only the headers need to be valid UTF-8, body can be binary
    let headers_bytes = &data[..header_end];
    let headers_str = std::str::from_utf8(headers_bytes)
        .map_err(|_| HttpError::InvalidResponse("Invalid UTF-8 in headers".to_string()))?;

    let body_start = header_end + 4; // Skip \r\n\r\n

    // Parse status line
    let mut lines = headers_str.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| HttpError::InvalidResponse("No status line".to_string()))?;

    // Parse "HTTP/x.x STATUS REASON"
    let (version, status_code, reason) = parse_status_line(status_line)?;

    // Parse headers
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_string();
            let value = line[colon_pos + 1..].trim().to_string();
            headers.push((key, value));
        }
    }

    // Extract body (can be binary data)
    let body = if body_start < data.len() {
        // Check for chunked transfer encoding
        let is_chunked = headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked")
        });

        if is_chunked {
            decode_chunked(&data[body_start..])?
        } else {
            data[body_start..].to_vec()
        }
    } else {
        Vec::new()
    };

    Ok(HttpResponse {
        status_code,
        reason,
        headers,
        body,
        version,
    })
}

/// Parse HTTP status line
fn parse_status_line(line: &str) -> HttpResult<(String, u16, String)> {
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err(HttpError::InvalidResponse(format!(
            "Invalid status line: {}",
            line
        )));
    }

    let version = parts[0].to_string();
    let status_code = parts[1]
        .parse::<u16>()
        .map_err(|_| HttpError::InvalidResponse(format!("Invalid status code: {}", parts[1])))?;
    let reason = if parts.len() > 2 {
        parts[2].to_string()
    } else {
        status_reason(status_code).to_string()
    };

    Ok((version, status_code, reason))
}

/// Get default reason phrase for status code
fn status_reason(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

/// Find the position of \r\n\r\n in the byte slice (end of HTTP headers)
fn find_header_end(data: &[u8]) -> Option<usize> {
    if data.len() < 4 {
        return None;
    }
    for i in 0..data.len() - 3 {
        if data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n'
        {
            return Some(i);
        }
    }
    None
}

/// Decode chunked transfer encoding
fn decode_chunked(data: &[u8]) -> HttpResult<Vec<u8>> {
    let mut result = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        // Find the end of chunk size line
        let line_end = find_crlf(&data[pos..]).ok_or_else(|| {
            HttpError::InvalidResponse("Invalid chunked encoding: no chunk size".to_string())
        })?;

        // Parse chunk size (hex)
        let size_str = std::str::from_utf8(&data[pos..pos + line_end])
            .map_err(|_| HttpError::InvalidResponse("Invalid chunk size encoding".to_string()))?;

        // Remove any chunk extension (after semicolon)
        let size_str = size_str.split(';').next().unwrap_or("0").trim();

        let chunk_size = usize::from_str_radix(size_str, 16).map_err(|_| {
            HttpError::InvalidResponse(format!("Invalid chunk size: {}", size_str))
        })?;

        if chunk_size == 0 {
            // Last chunk
            break;
        }

        pos += line_end + 2; // Skip size line and CRLF

        if pos + chunk_size > data.len() {
            return Err(HttpError::InvalidResponse("Truncated chunk data".to_string()));
        }

        result.extend_from_slice(&data[pos..pos + chunk_size]);
        pos += chunk_size + 2; // Skip chunk data and trailing CRLF
    }

    Ok(result)
}

/// Find \r\n in byte slice
fn find_crlf(data: &[u8]) -> Option<usize> {
    if data.len() < 2 {
        return None;
    }
    for i in 0..data.len() - 1 {
        if data[i] == b'\r' && data[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}

/// HTTP/2 response parsing (placeholder)
#[cfg(feature = "http2")]
pub mod http2 {
    use super::*;

    /// Parse HTTP/2 HEADERS and DATA frames into HttpResponse
    pub fn parse_http2_response(_frames: &[u8]) -> HttpResult<HttpResponse> {
        // TODO: Implement HTTP/2 response parsing
        // - HPACK header decompression
        // - Frame parsing (HEADERS, DATA, etc.)
        // - Stream reassembly
        Err(HttpError::NotSupported("HTTP/2 parsing not implemented".to_string()))
    }
}

/// HTTP/3 response parsing (placeholder)
#[cfg(feature = "http3")]
pub mod http3 {
    use super::*;

    /// Parse HTTP/3 frames into HttpResponse
    pub fn parse_http3_response(_frames: &[u8]) -> HttpResult<HttpResponse> {
        // TODO: Implement HTTP/3 response parsing
        // - QPACK header decompression
        // - HTTP/3 frame parsing
        // - QUIC stream handling
        Err(HttpError::NotSupported("HTTP/3 parsing not implemented".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_response() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let parsed = parse_http1_response(response).unwrap();
        assert_eq!(parsed.status_code, 200);
        assert_eq!(parsed.reason, "OK");
        assert_eq!(parsed.body, b"hello");
    }

    #[test]
    fn test_parse_chunked_response() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let parsed = parse_http1_response(response).unwrap();
        assert_eq!(parsed.body, b"hello");
    }
}
