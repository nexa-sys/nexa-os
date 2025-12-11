//! Certificate Verification
//!
//! Implements certificate chain verification and hostname matching.

use crate::x509::{verify_error, X509Store, X509};

/// Certificate verification result
pub struct VerifyResult {
    pub ok: bool,
    pub error_code: i64,
    pub error_depth: usize,
    pub error_message: &'static str,
}

impl VerifyResult {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error_code: verify_error::X509_V_OK,
            error_depth: 0,
            error_message: "ok",
        }
    }

    pub fn error(code: i64, depth: usize) -> Self {
        Self {
            ok: false,
            error_code: code,
            error_depth: depth,
            error_message: error_code_to_string(code),
        }
    }
}

/// Verify certificate chain
pub fn verify_chain(
    chain: &[X509],
    store: &X509Store,
    hostname: Option<&str>,
    depth_limit: usize,
) -> VerifyResult {
    if chain.is_empty() {
        return VerifyResult::error(verify_error::X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT, 0);
    }

    // Check chain depth
    if chain.len() > depth_limit {
        return VerifyResult::error(verify_error::X509_V_ERR_CERT_CHAIN_TOO_LONG, chain.len());
    }

    // Verify each certificate in chain
    for (i, cert) in chain.iter().enumerate() {
        // Check validity period
        if !cert.is_valid() {
            return VerifyResult::error(verify_error::X509_V_ERR_CERT_HAS_EXPIRED, i);
        }
    }

    // Verify hostname for leaf certificate
    if let Some(host) = hostname {
        if !chain[0].verify_hostname(host) {
            return VerifyResult::error(verify_error::X509_V_ERR_HOSTNAME_MISMATCH, 0);
        }
    }

    // Verify signatures up the chain
    for i in 0..chain.len().saturating_sub(1) {
        let cert = &chain[i];
        let issuer = &chain[i + 1];

        if !cert.verify_signature(issuer.get_public_key()) {
            return VerifyResult::error(verify_error::X509_V_ERR_CERT_SIGNATURE_FAILURE, i);
        }
    }

    // Check if root is trusted
    let result = store.verify(chain);
    if result != verify_error::X509_V_OK {
        return VerifyResult::error(result, chain.len() - 1);
    }

    VerifyResult::ok()
}

/// Get error string for error code
fn error_code_to_string(code: i64) -> &'static str {
    match code {
        verify_error::X509_V_OK => "ok",
        verify_error::X509_V_ERR_UNSPECIFIED => "unspecified certificate verification error",
        verify_error::X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT => "unable to get issuer certificate",
        verify_error::X509_V_ERR_UNABLE_TO_GET_CRL => "unable to get certificate CRL",
        verify_error::X509_V_ERR_UNABLE_TO_DECRYPT_CERT_SIGNATURE => {
            "unable to decrypt certificate's signature"
        }
        verify_error::X509_V_ERR_UNABLE_TO_DECRYPT_CRL_SIGNATURE => {
            "unable to decrypt CRL's signature"
        }
        verify_error::X509_V_ERR_UNABLE_TO_DECODE_ISSUER_PUBLIC_KEY => {
            "unable to decode issuer public key"
        }
        verify_error::X509_V_ERR_CERT_SIGNATURE_FAILURE => "certificate signature failure",
        verify_error::X509_V_ERR_CRL_SIGNATURE_FAILURE => "CRL signature failure",
        verify_error::X509_V_ERR_CERT_NOT_YET_VALID => "certificate is not yet valid",
        verify_error::X509_V_ERR_CERT_HAS_EXPIRED => "certificate has expired",
        verify_error::X509_V_ERR_CRL_NOT_YET_VALID => "CRL is not yet valid",
        verify_error::X509_V_ERR_CRL_HAS_EXPIRED => "CRL has expired",
        verify_error::X509_V_ERR_DEPTH_ZERO_SELF_SIGNED_CERT => "self signed certificate",
        verify_error::X509_V_ERR_SELF_SIGNED_CERT_IN_CHAIN => {
            "self signed certificate in certificate chain"
        }
        verify_error::X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT_LOCALLY => {
            "unable to get local issuer certificate"
        }
        verify_error::X509_V_ERR_UNABLE_TO_VERIFY_LEAF_SIGNATURE => {
            "unable to verify the first certificate"
        }
        verify_error::X509_V_ERR_CERT_CHAIN_TOO_LONG => "certificate chain too long",
        verify_error::X509_V_ERR_CERT_REVOKED => "certificate revoked",
        verify_error::X509_V_ERR_HOSTNAME_MISMATCH => "hostname mismatch",
        _ => "unknown certificate verification error",
    }
}

/// Verify hostname matches certificate
pub fn verify_hostname(cert: &X509, hostname: &str) -> bool {
    cert.verify_hostname(hostname)
}

/// Check OCSP response (stub)
pub fn check_ocsp(_cert: &X509, _issuer: &X509, _response: &[u8]) -> bool {
    // TODO: Implement OCSP checking
    true
}

/// Check certificate transparency (stub)
pub fn check_ct(_cert: &X509, _scts: &[Vec<u8>]) -> bool {
    // TODO: Implement CT verification
    true
}
