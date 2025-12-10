/// TLS/SSL support for nurl (Dynamic Linking Version)
///
/// This module provides TLS support by dynamically linking against libnssl.so
/// instead of statically linking the nssl crate.
use crate::http::HttpError;
use crate::nssl_ffi::*;
use std::ffi::CString;

/// TLS connection wrapper (dynamic linking version)
pub struct TlsConnection {
    ssl: *mut SSL,
    ctx: *mut SSL_CTX,
    alpn_protocol: Option<String>,
}

impl TlsConnection {
    /// Create a new TLS connection
    pub fn new(fd: i32, hostname: &str, insecure: bool) -> Result<Self, HttpError> {
        Self::new_with_alpn(fd, hostname, insecure, &[])
    }

    /// Create a new TLS connection with ALPN protocol negotiation
    pub fn new_with_alpn(
        fd: i32,
        hostname: &str,
        insecure: bool,
        alpn_protocols: &[&str],
    ) -> Result<Self, HttpError> {
        // Initialize SSL library
        unsafe {
            SSL_library_init();
        }

        // Create SSL context
        let method = unsafe { TLS_client_method() };
        if method.is_null() {
            return Err(HttpError::TlsError(
                "Failed to get TLS client method".to_string(),
            ));
        }

        let ctx = unsafe { SSL_CTX_new(method) };
        if ctx.is_null() {
            return Err(HttpError::TlsError(
                "Failed to create SSL context".to_string(),
            ));
        }

        // Set ALPN protocols if provided
        if !alpn_protocols.is_empty() {
            // Build ALPN wire format: length-prefixed strings
            let mut alpn_data = Vec::new();
            for proto in alpn_protocols {
                alpn_data.push(proto.len() as u8);
                alpn_data.extend_from_slice(proto.as_bytes());
            }
            unsafe {
                SSL_CTX_set_alpn_protos(ctx, alpn_data.as_ptr(), alpn_data.len() as u32);
            }
        }

        // Set verification mode
        unsafe {
            if insecure {
                SSL_CTX_set_verify(ctx, SSL_VERIFY_NONE, None);
            } else {
                SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, None);
                SSL_CTX_set_default_verify_paths(ctx);
            }
        }

        // Create SSL connection
        let ssl = unsafe { SSL_new(ctx) };
        if ssl.is_null() {
            unsafe {
                SSL_CTX_free(ctx);
            }
            return Err(HttpError::TlsError(
                "Failed to create SSL connection".to_string(),
            ));
        }

        // Set hostname for SNI
        let hostname_cstr = CString::new(hostname)
            .map_err(|_| HttpError::TlsError("Invalid hostname".to_string()))?;
        unsafe {
            SSL_set_tlsext_host_name(ssl, hostname_cstr.as_ptr());
        }

        // Set file descriptor
        let result = unsafe { SSL_set_fd(ssl, fd) };
        if result != 1 {
            unsafe {
                SSL_free(ssl);
                SSL_CTX_free(ctx);
            }
            return Err(HttpError::TlsError(
                "Failed to set SSL file descriptor".to_string(),
            ));
        }

        // Perform TLS handshake
        let result = unsafe { SSL_connect(ssl) };
        if result != 1 {
            let err = unsafe { SSL_get_error(ssl, result) };
            unsafe {
                SSL_free(ssl);
                SSL_CTX_free(ctx);
            }
            return Err(HttpError::TlsError(format!(
                "TLS handshake failed (error: {})",
                err
            )));
        }

        // Get negotiated ALPN protocol
        let alpn_protocol = unsafe {
            let mut proto_ptr: *const u8 = std::ptr::null();
            let mut proto_len: u32 = 0;
            SSL_get0_alpn_selected(ssl, &mut proto_ptr, &mut proto_len);
            if !proto_ptr.is_null() && proto_len > 0 {
                let slice = std::slice::from_raw_parts(proto_ptr, proto_len as usize);
                Some(String::from_utf8_lossy(slice).into_owned())
            } else {
                None
            }
        };

        Ok(Self {
            ssl,
            ctx,
            alpn_protocol,
        })
    }

    /// Get the negotiated ALPN protocol
    pub fn alpn_protocol(&self) -> Option<&str> {
        self.alpn_protocol.as_deref()
    }

    /// Get the negotiated TLS version
    pub fn version(&self) -> Option<String> {
        let version_ptr = unsafe { SSL_get_version(self.ssl) };
        if version_ptr.is_null() {
            return None;
        }
        let version = unsafe { std::ffi::CStr::from_ptr(version_ptr) };
        Some(version.to_string_lossy().into_owned())
    }

    /// Get the negotiated cipher suite
    pub fn cipher(&self) -> Option<String> {
        let cipher = unsafe { SSL_get_current_cipher(self.ssl) };
        if cipher.is_null() {
            return None;
        }
        let cipher_name = unsafe { SSL_CIPHER_get_name(cipher) };
        if cipher_name.is_null() {
            return None;
        }
        let name = unsafe { std::ffi::CStr::from_ptr(cipher_name) };
        Some(name.to_string_lossy().into_owned())
    }

    /// Write all data to the TLS connection
    pub fn write_all(&mut self, data: &[u8]) -> Result<(), HttpError> {
        let mut written = 0;
        while written < data.len() {
            let n = unsafe {
                SSL_write(
                    self.ssl,
                    data[written..].as_ptr(),
                    (data.len() - written) as i32,
                )
            };
            if n <= 0 {
                return Err(HttpError::SendFailed(
                    "Failed to write to TLS connection".to_string(),
                ));
            }
            written += n as usize;
        }
        Ok(())
    }

    /// Read all available data from the TLS connection
    pub fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize, HttpError> {
        let mut temp = [0u8; 4096];
        let mut total = 0;

        loop {
            let n = unsafe { SSL_read(self.ssl, temp.as_mut_ptr(), temp.len() as i32) };
            if n > 0 {
                buf.extend_from_slice(&temp[..n as usize]);
                total += n as usize;
            } else if n == 0 {
                // Connection closed
                break;
            } else {
                let err = unsafe { SSL_get_error(self.ssl, n) };
                if err == SSL_ERROR_ZERO_RETURN {
                    // Clean shutdown
                    break;
                } else if err == SSL_ERROR_WANT_READ {
                    // Would block, try again
                    continue;
                } else {
                    // Error
                    break;
                }
            }
        }

        Ok(total)
    }
}

impl std::io::Read for TlsConnection {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = unsafe { SSL_read(self.ssl, buf.as_mut_ptr(), buf.len() as i32) };
        if n > 0 {
            Ok(n as usize)
        } else if n == 0 {
            Ok(0)
        } else {
            let err = unsafe { SSL_get_error(self.ssl, n) };
            if err == SSL_ERROR_ZERO_RETURN {
                Ok(0)
            } else if err == SSL_ERROR_WANT_READ {
                Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "would block",
                ))
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("SSL read error: {}", err),
                ))
            }
        }
    }
}

impl std::io::Write for TlsConnection {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = unsafe { SSL_write(self.ssl, buf.as_ptr(), buf.len() as i32) };
        if n > 0 {
            Ok(n as usize)
        } else {
            let err = unsafe { SSL_get_error(self.ssl, n) };
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("SSL write error: {}", err),
            ))
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for TlsConnection {
    fn drop(&mut self) {
        unsafe {
            SSL_shutdown(self.ssl);
            SSL_free(self.ssl);
            SSL_CTX_free(self.ctx);
        }
    }
}
