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
    #[error("invalid vrf verification key")]
    InvalidVrfVerificationKey,
    #[error("kes period overflow")]
    KesPeriodOverflow,
    #[error("feature not implemented: {0}")]
    Unimplemented(&'static str),
}
