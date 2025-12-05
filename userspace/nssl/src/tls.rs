//! TLS Protocol Constants and Utilities

/// TLS content types
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentType {
    ChangeCipherSpec = 20,
    Alert = 21,
    Handshake = 22,
    ApplicationData = 23,
}

/// TLS handshake types
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakeType {
    ClientHello = 1,
    ServerHello = 2,
    NewSessionTicket = 4,
    EndOfEarlyData = 5,
    EncryptedExtensions = 8,
    Certificate = 11,
    CertificateRequest = 13,
    CertificateVerify = 15,
    Finished = 20,
    KeyUpdate = 24,
    MessageHash = 254,
}

/// TLS extension types
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionType {
    ServerName = 0,
    MaxFragmentLength = 1,
    StatusRequest = 5,
    SupportedGroups = 10,
    SignatureAlgorithms = 13,
    UseSrtp = 14,
    Heartbeat = 15,
    ApplicationLayerProtocolNegotiation = 16,
    SignedCertificateTimestamp = 18,
    ClientCertificateType = 19,
    ServerCertificateType = 20,
    Padding = 21,
    PreSharedKey = 41,
    EarlyData = 42,
    SupportedVersions = 43,
    Cookie = 44,
    PskKeyExchangeModes = 45,
    CertificateAuthorities = 47,
    OidFilters = 48,
    PostHandshakeAuth = 49,
    SignatureAlgorithmsCert = 50,
    KeyShare = 51,
}

/// Named groups (curves)
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedGroup {
    // Elliptic curves
    Secp256r1 = 23,
    Secp384r1 = 24,
    Secp521r1 = 25,
    X25519 = 29,
    X448 = 30,
    
    // Finite field groups (not recommended)
    Ffdhe2048 = 256,
    Ffdhe3072 = 257,
    Ffdhe4096 = 258,
}

/// Signature algorithms
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureScheme {
    // RSA PKCS#1 v1.5
    RsaPkcs1Sha256 = 0x0401,
    RsaPkcs1Sha384 = 0x0501,
    RsaPkcs1Sha512 = 0x0601,
    
    // ECDSA
    EcdsaSecp256r1Sha256 = 0x0403,
    EcdsaSecp384r1Sha384 = 0x0503,
    EcdsaSecp521r1Sha512 = 0x0603,
    
    // RSA-PSS with SHA-256/384/512
    RsaPssRsaeSha256 = 0x0804,
    RsaPssRsaeSha384 = 0x0805,
    RsaPssRsaeSha512 = 0x0806,
    
    // EdDSA
    Ed25519 = 0x0807,
    Ed448 = 0x0808,
    
    // RSA-PSS with public key OID rsassa-pss
    RsaPssPssSha256 = 0x0809,
    RsaPssPssSha384 = 0x080a,
    RsaPssPssSha512 = 0x080b,
}

/// Default supported named groups (preference order)
pub const DEFAULT_NAMED_GROUPS: &[NamedGroup] = &[
    NamedGroup::X25519,
    NamedGroup::Secp256r1,
    NamedGroup::Secp384r1,
];

/// Default signature algorithms (preference order)
pub const DEFAULT_SIGNATURE_ALGORITHMS: &[SignatureScheme] = &[
    SignatureScheme::Ed25519,
    SignatureScheme::EcdsaSecp256r1Sha256,
    SignatureScheme::EcdsaSecp384r1Sha384,
    SignatureScheme::RsaPssRsaeSha256,
    SignatureScheme::RsaPssRsaeSha384,
    SignatureScheme::RsaPssRsaeSha512,
    SignatureScheme::RsaPkcs1Sha256,
    SignatureScheme::RsaPkcs1Sha384,
    SignatureScheme::RsaPkcs1Sha512,
];

/// PSK key exchange modes
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PskKeyExchangeMode {
    PskKe = 0,
    PskDheKe = 1,
}

impl ContentType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            20 => Some(Self::ChangeCipherSpec),
            21 => Some(Self::Alert),
            22 => Some(Self::Handshake),
            23 => Some(Self::ApplicationData),
            _ => None,
        }
    }
}

impl HandshakeType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::ClientHello),
            2 => Some(Self::ServerHello),
            4 => Some(Self::NewSessionTicket),
            5 => Some(Self::EndOfEarlyData),
            8 => Some(Self::EncryptedExtensions),
            11 => Some(Self::Certificate),
            13 => Some(Self::CertificateRequest),
            15 => Some(Self::CertificateVerify),
            20 => Some(Self::Finished),
            24 => Some(Self::KeyUpdate),
            254 => Some(Self::MessageHash),
            _ => None,
        }
    }
}
