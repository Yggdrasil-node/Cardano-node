use crate::CryptoError;
use ed25519_dalek::{Signature as DalekSignature, Signer, SigningKey as DalekSigningKey, VerifyingKey};
use std::fmt;

/// A byte-backed Ed25519 signing key.
///
/// This wrapper stores the 32-byte signing seed and derives the verification key
/// using `ed25519-dalek`, matching the usual Ed25519 seed-based API shape used by
/// upstream Cardano components.
#[derive(Clone, Eq, PartialEq)]
pub struct SigningKey(pub [u8; 32]);

/// A byte-backed Ed25519 verification key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationKey(pub [u8; 32]);

/// A byte-backed Ed25519 signature.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature(pub [u8; 64]);

impl SigningKey {
    /// Constructs a signing key from a 32-byte Ed25519 seed.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the 32-byte Ed25519 seed backing this signing key.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// Derives the verification key corresponding to this signing key.
    pub fn verification_key(&self) -> Result<VerificationKey, CryptoError> {
        let signing_key = DalekSigningKey::from_bytes(&self.0);
        Ok(VerificationKey(signing_key.verifying_key().to_bytes()))
    }

    /// Signs a message using strict Ed25519 semantics.
    pub fn sign(&self, message: &[u8]) -> Result<Signature, CryptoError> {
        let signing_key = DalekSigningKey::from_bytes(&self.0);
        Ok(Signature(signing_key.sign(message).to_bytes()))
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SigningKey([REDACTED])")
    }
}

impl VerificationKey {
    /// Constructs a verification key from its 32-byte encoding.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the 32-byte encoded verification key.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// Verifies a message and signature pair using strict Ed25519 verification.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), CryptoError> {
        let verification_key = VerifyingKey::from_bytes(&self.0)
            .map_err(|_| CryptoError::InvalidVerificationKey)?;
        let signature = DalekSignature::from_bytes(&signature.0);

        verification_key
            .verify_strict(message, &signature)
            .map_err(|_| CryptoError::SignatureVerificationFailed)
    }
}

impl Signature {
    /// Constructs a signature from its 64-byte encoding.
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Returns the 64-byte encoded signature.
    pub fn to_bytes(&self) -> [u8; 64] {
        self.0
    }
}
