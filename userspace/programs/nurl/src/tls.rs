/// TLS/SSL support for nurl
///
/// This module wraps the nssl library for HTTPS support.

#[cfg(feature = "https")]
use std::ffi::CString;

use crate::http::HttpError;

/// TLS connection wrapper
#[cfg(feature = "https")]
pub struct TlsConnection {
    ssl: *mut nssl::SslConnection,
    ctx: *mut nssl::SslContext,
}

#[cfg(feature = "https")]
impl TlsConnection {
    /// Create a new TLS connection
    pub fn new(fd: i32, hostname: &str, insecure: bool) -> Result<Self, HttpError> {
        // Initialize SSL library
        unsafe {
            nssl::SSL_library_init();
        }

        // Create SSL context
        let method = unsafe { nssl::TLS_client_method() };
        if method.is_null() {
            return Err(HttpError::TlsError(
                "Failed to get TLS client method".to_string(),
            ));
        }

        let ctx = unsafe { nssl::SSL_CTX_new(method) };
        if ctx.is_null() {
            return Err(HttpError::TlsError(
                "Failed to create SSL context".to_string(),
            ));
        }

        // Set verification mode
        unsafe {
            if insecure {
                nssl::SSL_CTX_set_verify(ctx, nssl::ssl_verify::SSL_VERIFY_NONE, None);
            } else {
                nssl::SSL_CTX_set_verify(ctx, nssl::ssl_verify::SSL_VERIFY_PEER, None);
                nssl::SSL_CTX_set_default_verify_paths(ctx);
            }
        }

        // Create SSL connection
        let ssl = unsafe { nssl::SSL_new(ctx) };
        if ssl.is_null() {
            unsafe {
                nssl::SSL_CTX_free(ctx);
            }
            return Err(HttpError::TlsError(
                "Failed to create SSL connection".to_string(),
            ));
        }

        // Set hostname for SNI
        let hostname_cstr = CString::new(hostname)
            .map_err(|_| HttpError::TlsError("Invalid hostname".to_string()))?;
        unsafe {
            nssl::SSL_set_tlsext_host_name(ssl, hostname_cstr.as_ptr() as *const i8);
        }

        // Set file descriptor
        let result = unsafe { nssl::SSL_set_fd(ssl, fd) };
        if result != 1 {
            unsafe {
                nssl::SSL_free(ssl);
                nssl::SSL_CTX_free(ctx);
            }
            return Err(HttpError::TlsError(
                "Failed to set SSL file descriptor".to_string(),
            ));
        }

        // Perform TLS handshake
        let result = unsafe { nssl::SSL_connect(ssl) };
        if result != 1 {
            let err = unsafe { nssl::SSL_get_error(ssl, result) };
            unsafe {
                nssl::SSL_free(ssl);
                nssl::SSL_CTX_free(ctx);
            }
            return Err(HttpError::TlsError(format!(
                "TLS handshake failed (error: {})",
                err
            )));
        }

        Ok(Self { ssl, ctx })
    }

    /// Get the negotiated TLS version
    pub fn version(&self) -> Option<String> {
        let version_ptr = unsafe { nssl::SSL_get_version(self.ssl) };
        if version_ptr.is_null() {
            return None;
        }
        let version = unsafe { std::ffi::CStr::from_ptr(version_ptr) };
        Some(version.to_string_lossy().into_owned())
    }

    /// Get the negotiated cipher suite
    pub fn cipher(&self) -> Option<String> {
        let cipher = unsafe { nssl::SSL_get_current_cipher(self.ssl) };
        if cipher.is_null() {
            return None;
        }
        let cipher_name = unsafe { nssl::SSL_CIPHER_get_name(cipher) };
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
                nssl::SSL_write(
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
            let n = unsafe { nssl::SSL_read(self.ssl, temp.as_mut_ptr(), temp.len() as i32) };
            if n > 0 {
                buf.extend_from_slice(&temp[..n as usize]);
                total += n as usize;
            } else if n == 0 {
                // Connection closed
                break;
            } else {
                let err = unsafe { nssl::SSL_get_error(self.ssl, n) };
                if err == nssl::ssl_error::SSL_ERROR_ZERO_RETURN {
                    // Clean shutdown
                    break;
                } else if err == nssl::ssl_error::SSL_ERROR_WANT_READ {
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

#[cfg(feature = "https")]
impl Drop for TlsConnection {
    fn drop(&mut self) {
        unsafe {
            nssl::SSL_shutdown(self.ssl);
            nssl::SSL_free(self.ssl);
            nssl::SSL_CTX_free(self.ctx);
        }
    }
}

/// Placeholder when HTTPS is not enabled
#[cfg(not(feature = "https"))]
pub struct TlsConnection;

#[cfg(not(feature = "https"))]
impl TlsConnection {
    pub fn new(_fd: i32, _hostname: &str, _insecure: bool) -> Result<Self, HttpError> {
        Err(HttpError::NotSupported(
            "HTTPS not supported (compile with 'https' feature)".to_string(),
        ))
    }
}
