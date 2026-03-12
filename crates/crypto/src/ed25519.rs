use crate::CryptoError;
use ed25519_dalek::{Signature as DalekSignature, Signer, SigningKey as DalekSigningKey, VerifyingKey};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct SigningKey(pub [u8; 32]);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationKey(pub [u8; 32]);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature(pub [u8; 64]);

impl SigningKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    pub fn verification_key(&self) -> Result<VerificationKey, CryptoError> {
        let signing_key = DalekSigningKey::from_bytes(&self.0);
        Ok(VerificationKey(signing_key.verifying_key().to_bytes()))
    }

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
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

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
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        self.0
    }
}
