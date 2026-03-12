use thiserror::Error;

/// Errors returned by cryptographic helpers and protocol-facing wrappers.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum CryptoError {
    #[error("invalid ed25519 verification key")]
    InvalidVerificationKey,
    #[error("ed25519 signature verification failed")]
    SignatureVerificationFailed,
    #[error("invalid vrf proof")]
    InvalidVrfProof,
    #[error("kes period overflow")]
    KesPeriodOverflow,
    #[error("feature not implemented: {0}")]
    Unimplemented(&'static str),
}
