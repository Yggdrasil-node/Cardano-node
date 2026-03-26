use thiserror::Error;

/// Errors returned by cryptographic helpers and protocol-facing wrappers.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum CryptoError {
    #[error("invalid ed25519 verification key")]
    InvalidVerificationKey,
    #[error("ed25519 signature verification failed")]
    SignatureVerificationFailed,
    #[error("invalid kes period: {0}")]
    InvalidKesPeriod(u32),
    #[error("invalid kes key material length: {0}")]
    InvalidKesKeyMaterialLength(usize),
    #[error("invalid kes depth: {0}")]
    InvalidKesDepth(usize),
    #[error("kes verification key does not match compact signature")]
    KesVerificationKeyMismatch,
    #[error("invalid vrf proof")]
    InvalidVrfProof,
    #[error("invalid vrf signing key")]
    InvalidVrfSigningKey,
    #[error("invalid vrf verification key")]
    InvalidVrfVerificationKey,
    #[error("kes period overflow")]
    KesPeriodOverflow,
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("invalid signature format: {0}")]
    SignatureFormat(String),
    #[error("invalid elliptic curve point encoding")]
    InvalidPoint,
    #[error("invalid input length for curve operation")]
    InvalidLength,
    #[error("invalid or empty domain separation tag")]
    InvalidDomain,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_invalid_verification_key() {
        let e = CryptoError::InvalidVerificationKey;
        assert_eq!(e.to_string(), "invalid ed25519 verification key");
    }

    #[test]
    fn display_signature_verification_failed() {
        let e = CryptoError::SignatureVerificationFailed;
        assert_eq!(e.to_string(), "ed25519 signature verification failed");
    }

    #[test]
    fn display_invalid_kes_period() {
        let e = CryptoError::InvalidKesPeriod(42);
        assert_eq!(e.to_string(), "invalid kes period: 42");
    }

    #[test]
    fn display_invalid_kes_key_material_length() {
        let e = CryptoError::InvalidKesKeyMaterialLength(99);
        assert_eq!(e.to_string(), "invalid kes key material length: 99");
    }

    #[test]
    fn display_invalid_kes_depth() {
        let e = CryptoError::InvalidKesDepth(5);
        assert_eq!(e.to_string(), "invalid kes depth: 5");
    }

    #[test]
    fn display_kes_verification_key_mismatch() {
        let e = CryptoError::KesVerificationKeyMismatch;
        assert_eq!(
            e.to_string(),
            "kes verification key does not match compact signature"
        );
    }

    #[test]
    fn display_invalid_vrf_proof() {
        assert_eq!(CryptoError::InvalidVrfProof.to_string(), "invalid vrf proof");
    }

    #[test]
    fn display_invalid_vrf_signing_key() {
        assert_eq!(
            CryptoError::InvalidVrfSigningKey.to_string(),
            "invalid vrf signing key"
        );
    }

    #[test]
    fn display_invalid_vrf_verification_key() {
        assert_eq!(
            CryptoError::InvalidVrfVerificationKey.to_string(),
            "invalid vrf verification key"
        );
    }

    #[test]
    fn display_kes_period_overflow() {
        assert_eq!(
            CryptoError::KesPeriodOverflow.to_string(),
            "kes period overflow"
        );
    }

    #[test]
    fn display_invalid_key() {
        let e = CryptoError::InvalidKey("bad key data".into());
        assert_eq!(e.to_string(), "invalid key: bad key data");
    }

    #[test]
    fn display_signature_format() {
        let e = CryptoError::SignatureFormat("wrong length".into());
        assert_eq!(e.to_string(), "invalid signature format: wrong length");
    }

    #[test]
    fn display_invalid_point() {
        assert_eq!(
            CryptoError::InvalidPoint.to_string(),
            "invalid elliptic curve point encoding"
        );
    }

    #[test]
    fn display_invalid_length() {
        assert_eq!(
            CryptoError::InvalidLength.to_string(),
            "invalid input length for curve operation"
        );
    }

    #[test]
    fn display_invalid_domain() {
        assert_eq!(
            CryptoError::InvalidDomain.to_string(),
            "invalid or empty domain separation tag"
        );
    }

    #[test]
    fn equality_unit_variants() {
        assert_eq!(CryptoError::InvalidVrfProof, CryptoError::InvalidVrfProof);
        assert_ne!(CryptoError::InvalidVrfProof, CryptoError::InvalidPoint);
    }

    #[test]
    fn equality_parameterized_variants() {
        assert_eq!(
            CryptoError::InvalidKesPeriod(1),
            CryptoError::InvalidKesPeriod(1)
        );
        assert_ne!(
            CryptoError::InvalidKesPeriod(1),
            CryptoError::InvalidKesPeriod(2)
        );
    }

    #[test]
    fn debug_format_contains_variant_name() {
        let e = CryptoError::InvalidVrfProof;
        let dbg = format!("{:?}", e);
        assert!(dbg.contains("InvalidVrfProof"));
    }
}
