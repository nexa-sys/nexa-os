//! SSL Method Types
//!
//! Defines the SSL/TLS methods (protocol versions and modes).

use crate::c_int;

/// SSL method type
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SslMethodType {
    /// TLS client (auto-negotiate highest version)
    TlsClient = 0,
    /// TLS server (auto-negotiate highest version)
    TlsServer = 1,
    /// TLS auto (client or server determined by first operation)
    Tls = 2,
    /// TLS 1.2 client only
    Tls12Client = 3,
    /// TLS 1.2 server only
    Tls12Server = 4,
    /// TLS 1.3 client only
    Tls13Client = 5,
    /// TLS 1.3 server only
    Tls13Server = 6,
}

/// SSL method descriptor
#[repr(C)]
pub struct SslMethod {
    /// Method type
    pub method_type: SslMethodType,
    /// Minimum TLS version
    pub min_version: u16,
    /// Maximum TLS version
    pub max_version: u16,
    /// Is client method
    pub is_client: bool,
    /// Is server method
    pub is_server: bool,
}

impl SslMethod {
    /// Create a new TLS client method
    pub const fn tls_client() -> Self {
        Self {
            method_type: SslMethodType::TlsClient,
            min_version: crate::TLS1_2_VERSION,
            max_version: crate::TLS1_3_VERSION,
            is_client: true,
            is_server: false,
        }
    }

    /// Create a new TLS server method
    pub const fn tls_server() -> Self {
        Self {
            method_type: SslMethodType::TlsServer,
            min_version: crate::TLS1_2_VERSION,
            max_version: crate::TLS1_3_VERSION,
            is_client: false,
            is_server: true,
        }
    }

    /// Create a new TLS method (auto client/server)
    pub const fn tls() -> Self {
        Self {
            method_type: SslMethodType::Tls,
            min_version: crate::TLS1_2_VERSION,
            max_version: crate::TLS1_3_VERSION,
            is_client: true,
            is_server: true,
        }
    }

    /// Create a TLS 1.2 client method
    pub const fn tls12_client() -> Self {
        Self {
            method_type: SslMethodType::Tls12Client,
            min_version: crate::TLS1_2_VERSION,
            max_version: crate::TLS1_2_VERSION,
            is_client: true,
            is_server: false,
        }
    }

    /// Create a TLS 1.2 server method
    pub const fn tls12_server() -> Self {
        Self {
            method_type: SslMethodType::Tls12Server,
            min_version: crate::TLS1_2_VERSION,
            max_version: crate::TLS1_2_VERSION,
            is_client: false,
            is_server: true,
        }
    }

    /// Check if this is a client method
    pub fn is_client_method(&self) -> bool {
        self.is_client
    }

    /// Check if this is a server method
    pub fn is_server_method(&self) -> bool {
        self.is_server
    }

    /// Get minimum supported version
    pub fn get_min_version(&self) -> u16 {
        self.min_version
    }

    /// Get maximum supported version
    pub fn get_max_version(&self) -> u16 {
        self.max_version
    }
}

// Static method instances for C ABI
pub static TLS_CLIENT_METHOD: SslMethod = SslMethod::tls_client();
pub static TLS_SERVER_METHOD: SslMethod = SslMethod::tls_server();
pub static TLS_METHOD: SslMethod = SslMethod::tls();
pub static TLS12_CLIENT_METHOD: SslMethod = SslMethod::tls12_client();
pub static TLS12_SERVER_METHOD: SslMethod = SslMethod::tls12_server();
