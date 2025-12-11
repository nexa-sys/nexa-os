//! TLS Alert Protocol

/// Alert level
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertLevel {
    Warning = 1,
    Fatal = 2,
}

/// Alert description
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertDescription {
    CloseNotify = 0,
    UnexpectedMessage = 10,
    BadRecordMac = 20,
    RecordOverflow = 22,
    HandshakeFailure = 40,
    BadCertificate = 42,
    UnsupportedCertificate = 43,
    CertificateRevoked = 44,
    CertificateExpired = 45,
    CertificateUnknown = 46,
    IllegalParameter = 47,
    UnknownCa = 48,
    AccessDenied = 49,
    DecodeError = 50,
    DecryptError = 51,
    ProtocolVersion = 70,
    InsufficientSecurity = 71,
    InternalError = 80,
    InappropriateFallback = 86,
    UserCanceled = 90,
    MissingExtension = 109,
    UnsupportedExtension = 110,
    UnrecognizedName = 112,
    BadCertificateStatusResponse = 113,
    UnknownPskIdentity = 115,
    CertificateRequired = 116,
    NoApplicationProtocol = 120,
}

/// TLS Alert
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Alert {
    pub level: AlertLevel,
    pub description: AlertDescription,
}

impl Alert {
    pub fn new(level: AlertLevel, description: AlertDescription) -> Self {
        Self { level, description }
    }

    pub fn close_notify() -> Self {
        Self::new(AlertLevel::Warning, AlertDescription::CloseNotify)
    }

    pub fn fatal(description: AlertDescription) -> Self {
        Self::new(AlertLevel::Fatal, description)
    }

    pub fn to_bytes(&self) -> [u8; 2] {
        [self.level as u8, self.description as u8]
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 2 {
            return None;
        }
        Some(Self {
            level: match data[0] {
                1 => AlertLevel::Warning,
                2 => AlertLevel::Fatal,
                _ => return None,
            },
            description: AlertDescription::from_u8(data[1])?,
        })
    }

    pub fn is_fatal(&self) -> bool {
        matches!(self.level, AlertLevel::Fatal)
    }

    pub fn is_close_notify(&self) -> bool {
        matches!(self.description, AlertDescription::CloseNotify)
    }
}

impl AlertDescription {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::CloseNotify),
            10 => Some(Self::UnexpectedMessage),
            20 => Some(Self::BadRecordMac),
            22 => Some(Self::RecordOverflow),
            40 => Some(Self::HandshakeFailure),
            42 => Some(Self::BadCertificate),
            43 => Some(Self::UnsupportedCertificate),
            44 => Some(Self::CertificateRevoked),
            45 => Some(Self::CertificateExpired),
            46 => Some(Self::CertificateUnknown),
            47 => Some(Self::IllegalParameter),
            48 => Some(Self::UnknownCa),
            49 => Some(Self::AccessDenied),
            50 => Some(Self::DecodeError),
            51 => Some(Self::DecryptError),
            70 => Some(Self::ProtocolVersion),
            71 => Some(Self::InsufficientSecurity),
            80 => Some(Self::InternalError),
            86 => Some(Self::InappropriateFallback),
            90 => Some(Self::UserCanceled),
            109 => Some(Self::MissingExtension),
            110 => Some(Self::UnsupportedExtension),
            112 => Some(Self::UnrecognizedName),
            113 => Some(Self::BadCertificateStatusResponse),
            115 => Some(Self::UnknownPskIdentity),
            116 => Some(Self::CertificateRequired),
            120 => Some(Self::NoApplicationProtocol),
            _ => None,
        }
    }

    pub fn to_string(&self) -> &'static str {
        match self {
            Self::CloseNotify => "close notify",
            Self::UnexpectedMessage => "unexpected message",
            Self::BadRecordMac => "bad record mac",
            Self::RecordOverflow => "record overflow",
            Self::HandshakeFailure => "handshake failure",
            Self::BadCertificate => "bad certificate",
            Self::UnsupportedCertificate => "unsupported certificate",
            Self::CertificateRevoked => "certificate revoked",
            Self::CertificateExpired => "certificate expired",
            Self::CertificateUnknown => "certificate unknown",
            Self::IllegalParameter => "illegal parameter",
            Self::UnknownCa => "unknown CA",
            Self::AccessDenied => "access denied",
            Self::DecodeError => "decode error",
            Self::DecryptError => "decrypt error",
            Self::ProtocolVersion => "protocol version",
            Self::InsufficientSecurity => "insufficient security",
            Self::InternalError => "internal error",
            Self::InappropriateFallback => "inappropriate fallback",
            Self::UserCanceled => "user canceled",
            Self::MissingExtension => "missing extension",
            Self::UnsupportedExtension => "unsupported extension",
            Self::UnrecognizedName => "unrecognized name",
            Self::BadCertificateStatusResponse => "bad certificate status response",
            Self::UnknownPskIdentity => "unknown PSK identity",
            Self::CertificateRequired => "certificate required",
            Self::NoApplicationProtocol => "no application protocol",
        }
    }
}
