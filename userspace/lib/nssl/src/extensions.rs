//! TLS Extensions
//!
//! Parsing and building of TLS extensions.

use crate::tls::ExtensionType;
use std::string::String;
use std::vec::Vec;

/// Parsed extension data
pub enum ExtensionData {
    /// Server Name Indication
    ServerName(Vec<ServerName>),
    /// Supported Versions
    SupportedVersions(Vec<u16>),
    /// Supported Groups
    SupportedGroups(Vec<u16>),
    /// Signature Algorithms
    SignatureAlgorithms(Vec<u16>),
    /// Key Share (client)
    KeyShareClient(Vec<KeyShareEntry>),
    /// Key Share (server)
    KeyShareServer(KeyShareEntry),
    /// Key Share HelloRetryRequest
    KeyShareHelloRetryRequest(u16),
    /// Pre-Shared Key (client)
    PreSharedKeyClient(PreSharedKeyClientHello),
    /// Pre-Shared Key (server)
    PreSharedKeyServer(u16),
    /// PSK Key Exchange Modes
    PskKeyExchangeModes(Vec<u8>),
    /// Early Data
    EarlyData(Option<u32>),
    /// Cookie
    Cookie(Vec<u8>),
    /// ALPN
    Alpn(Vec<String>),
    /// OCSP Status Request
    StatusRequest,
    /// Signed Certificate Timestamp
    SignedCertificateTimestamp(Vec<u8>),
    /// Unknown extension
    Unknown(Vec<u8>),
}

/// Server name entry
pub struct ServerName {
    pub name_type: u8,
    pub name: String,
}

/// Key share entry
#[derive(Clone)]
pub struct KeyShareEntry {
    pub group: u16,
    pub key_exchange: Vec<u8>,
}

/// Pre-shared key extension (ClientHello)
pub struct PreSharedKeyClientHello {
    pub identities: Vec<PskIdentity>,
    pub binders: Vec<Vec<u8>>,
}

/// PSK identity
pub struct PskIdentity {
    pub identity: Vec<u8>,
    pub obfuscated_ticket_age: u32,
}

/// Parse extension
pub fn parse_extension(ext_type: u16, data: &[u8]) -> ExtensionData {
    match ext_type {
        0 => parse_server_name(data),
        43 => parse_supported_versions(data),
        10 => parse_supported_groups(data),
        13 => parse_signature_algorithms(data),
        51 => parse_key_share(data),
        41 => parse_pre_shared_key(data),
        45 => parse_psk_key_exchange_modes(data),
        42 => parse_early_data(data),
        44 => parse_cookie(data),
        16 => parse_alpn(data),
        _ => ExtensionData::Unknown(data.to_vec()),
    }
}

/// Parse server_name extension
fn parse_server_name(data: &[u8]) -> ExtensionData {
    let mut names = Vec::new();

    if data.len() < 2 {
        return ExtensionData::ServerName(names);
    }

    let list_len = ((data[0] as usize) << 8) | (data[1] as usize);
    let mut pos = 2;

    while pos + 3 <= data.len() && pos < 2 + list_len {
        let name_type = data[pos];
        let name_len = ((data[pos + 1] as usize) << 8) | (data[pos + 2] as usize);
        pos += 3;

        if pos + name_len <= data.len() {
            if let Ok(name) = std::str::from_utf8(&data[pos..pos + name_len]) {
                names.push(ServerName {
                    name_type,
                    name: name.to_string(),
                });
            }
            pos += name_len;
        }
    }

    ExtensionData::ServerName(names)
}

/// Parse supported_versions extension
fn parse_supported_versions(data: &[u8]) -> ExtensionData {
    let mut versions = Vec::new();

    if data.is_empty() {
        return ExtensionData::SupportedVersions(versions);
    }

    // Check if this is ClientHello format (with length byte) or ServerHello format (just version)
    if data.len() == 2 {
        // ServerHello format
        let version = ((data[0] as u16) << 8) | (data[1] as u16);
        versions.push(version);
    } else if data.len() >= 1 {
        // ClientHello format
        let len = data[0] as usize;
        let mut pos = 1;

        while pos + 2 <= data.len() && pos < 1 + len {
            let version = ((data[pos] as u16) << 8) | (data[pos + 1] as u16);
            versions.push(version);
            pos += 2;
        }
    }

    ExtensionData::SupportedVersions(versions)
}

/// Parse supported_groups extension
fn parse_supported_groups(data: &[u8]) -> ExtensionData {
    let mut groups = Vec::new();

    if data.len() < 2 {
        return ExtensionData::SupportedGroups(groups);
    }

    let len = ((data[0] as usize) << 8) | (data[1] as usize);
    let mut pos = 2;

    while pos + 2 <= data.len() && pos < 2 + len {
        let group = ((data[pos] as u16) << 8) | (data[pos + 1] as u16);
        groups.push(group);
        pos += 2;
    }

    ExtensionData::SupportedGroups(groups)
}

/// Parse signature_algorithms extension
fn parse_signature_algorithms(data: &[u8]) -> ExtensionData {
    let mut algs = Vec::new();

    if data.len() < 2 {
        return ExtensionData::SignatureAlgorithms(algs);
    }

    let len = ((data[0] as usize) << 8) | (data[1] as usize);
    let mut pos = 2;

    while pos + 2 <= data.len() && pos < 2 + len {
        let alg = ((data[pos] as u16) << 8) | (data[pos + 1] as u16);
        algs.push(alg);
        pos += 2;
    }

    ExtensionData::SignatureAlgorithms(algs)
}

/// Parse key_share extension
fn parse_key_share(data: &[u8]) -> ExtensionData {
    let mut entries = Vec::new();

    if data.len() < 2 {
        // ServerHello format (single entry, no length prefix)
        if data.len() >= 4 {
            let group = ((data[0] as u16) << 8) | (data[1] as u16);
            let key_len = ((data[2] as usize) << 8) | (data[3] as usize);
            if data.len() >= 4 + key_len {
                return ExtensionData::KeyShareServer(KeyShareEntry {
                    group,
                    key_exchange: data[4..4 + key_len].to_vec(),
                });
            }
        }
        return ExtensionData::KeyShareClient(entries);
    }

    // ClientHello format
    let len = ((data[0] as usize) << 8) | (data[1] as usize);
    let mut pos = 2;

    while pos + 4 <= data.len() && pos < 2 + len {
        let group = ((data[pos] as u16) << 8) | (data[pos + 1] as u16);
        let key_len = ((data[pos + 2] as usize) << 8) | (data[pos + 3] as usize);
        pos += 4;

        if pos + key_len <= data.len() {
            entries.push(KeyShareEntry {
                group,
                key_exchange: data[pos..pos + key_len].to_vec(),
            });
            pos += key_len;
        }
    }

    ExtensionData::KeyShareClient(entries)
}

/// Parse pre_shared_key extension
fn parse_pre_shared_key(data: &[u8]) -> ExtensionData {
    // Simplified parsing
    if data.len() == 2 {
        // ServerHello format
        let selected = ((data[0] as u16) << 8) | (data[1] as u16);
        return ExtensionData::PreSharedKeyServer(selected);
    }

    // ClientHello format
    let mut identities = Vec::new();
    let mut binders = Vec::new();

    if data.len() >= 2 {
        let ident_len = ((data[0] as usize) << 8) | (data[1] as usize);
        let mut pos = 2;

        while pos + 6 <= data.len() && pos < 2 + ident_len {
            let id_len = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
            pos += 2;

            if pos + id_len + 4 <= data.len() {
                let identity = data[pos..pos + id_len].to_vec();
                pos += id_len;

                let age =
                    u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                pos += 4;

                identities.push(PskIdentity {
                    identity,
                    obfuscated_ticket_age: age,
                });
            }
        }

        // Parse binders
        if pos + 2 <= data.len() {
            let binders_len = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
            pos += 2;

            while pos + 1 <= data.len() && pos < pos + binders_len {
                let binder_len = data[pos] as usize;
                pos += 1;

                if pos + binder_len <= data.len() {
                    binders.push(data[pos..pos + binder_len].to_vec());
                    pos += binder_len;
                }
            }
        }
    }

    ExtensionData::PreSharedKeyClient(PreSharedKeyClientHello {
        identities,
        binders,
    })
}

/// Parse psk_key_exchange_modes extension
fn parse_psk_key_exchange_modes(data: &[u8]) -> ExtensionData {
    let mut modes = Vec::new();

    if !data.is_empty() {
        let len = data[0] as usize;
        for &mode in data.get(1..1 + len).unwrap_or(&[]) {
            modes.push(mode);
        }
    }

    ExtensionData::PskKeyExchangeModes(modes)
}

/// Parse early_data extension
fn parse_early_data(data: &[u8]) -> ExtensionData {
    if data.len() >= 4 {
        let max_size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        ExtensionData::EarlyData(Some(max_size))
    } else {
        ExtensionData::EarlyData(None)
    }
}

/// Parse cookie extension
fn parse_cookie(data: &[u8]) -> ExtensionData {
    if data.len() >= 2 {
        let len = ((data[0] as usize) << 8) | (data[1] as usize);
        if data.len() >= 2 + len {
            return ExtensionData::Cookie(data[2..2 + len].to_vec());
        }
    }
    ExtensionData::Cookie(Vec::new())
}

/// Parse application_layer_protocol_negotiation extension
fn parse_alpn(data: &[u8]) -> ExtensionData {
    let mut protocols = Vec::new();

    if data.len() < 2 {
        return ExtensionData::Alpn(protocols);
    }

    let list_len = ((data[0] as usize) << 8) | (data[1] as usize);
    let mut pos = 2;

    while pos + 1 <= data.len() && pos < 2 + list_len {
        let proto_len = data[pos] as usize;
        pos += 1;

        if pos + proto_len <= data.len() {
            if let Ok(proto) = std::str::from_utf8(&data[pos..pos + proto_len]) {
                protocols.push(proto.to_string());
            }
            pos += proto_len;
        }
    }

    ExtensionData::Alpn(protocols)
}

/// Build ALPN extension for wire format
pub fn build_alpn_extension(protocols: &[&str]) -> Vec<u8> {
    let mut data = Vec::new();

    // Calculate total length
    let mut list_len = 0;
    for proto in protocols {
        list_len += 1 + proto.len();
    }

    // Extension type
    data.push((ExtensionType::ApplicationLayerProtocolNegotiation as u16 >> 8) as u8);
    data.push((ExtensionType::ApplicationLayerProtocolNegotiation as u16 & 0xFF) as u8);

    // Extension length
    let ext_len = 2 + list_len;
    data.push((ext_len >> 8) as u8);
    data.push((ext_len & 0xFF) as u8);

    // Protocol list length
    data.push((list_len >> 8) as u8);
    data.push((list_len & 0xFF) as u8);

    // Protocols
    for proto in protocols {
        data.push(proto.len() as u8);
        data.extend_from_slice(proto.as_bytes());
    }

    data
}

/// Encode ALPN protocols to wire format (for SSL_CTX_set_alpn_protos)
pub fn encode_alpn_protos(protocols: &[&str]) -> Vec<u8> {
    let mut data = Vec::new();

    for proto in protocols {
        data.push(proto.len() as u8);
        data.extend_from_slice(proto.as_bytes());
    }

    data
}
