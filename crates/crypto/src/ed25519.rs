use crate::CryptoError;
use ed25519_dalek::{Signature as DalekSignature, Signer, SigningKey as DalekSigningKey, VerifyingKey};
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A byte-backed Ed25519 signing key.
///
/// This wrapper stores the 32-byte signing seed and derives the verification key
/// using `ed25519-dalek`, matching the usual Ed25519 seed-based API shape used by
/// upstream Cardano components.
///
/// Secret key material is zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
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

impl PartialEq for SigningKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for SigningKey {}

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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::test_vectors::ed25519_rfc8032_vectors;

    // ── Key construction & derivation ────────────────────────────────────

    #[test]
    fn signing_key_from_bytes_roundtrip() {
        let seed = [0x42_u8; 32];
        let sk = SigningKey::from_bytes(seed);
        assert_eq!(sk.to_bytes(), seed);
    }

    #[test]
    fn verification_key_derivation_is_deterministic() {
        let sk = SigningKey::from_bytes([1u8; 32]);
        let vk1 = sk.verification_key().unwrap();
        let vk2 = sk.verification_key().unwrap();
        assert_eq!(vk1, vk2);
    }

    #[test]
    fn different_seeds_produce_different_verification_keys() {
        let vk1 = SigningKey::from_bytes([0x01; 32]).verification_key().unwrap();
        let vk2 = SigningKey::from_bytes([0x02; 32]).verification_key().unwrap();
        assert_ne!(vk1, vk2);
    }

    #[test]
    fn verification_key_from_bytes_roundtrip() {
        let bytes = [0xAB; 32];
        let vk = VerificationKey::from_bytes(bytes);
        assert_eq!(vk.to_bytes(), bytes);
    }

    // ── Sign / verify roundtrip ──────────────────────────────────────────

    #[test]
    fn sign_and_verify_roundtrip() {
        let sk = SigningKey::from_bytes([7u8; 32]);
        let vk = sk.verification_key().unwrap();
        let msg = b"hello cardano";
        let sig = sk.sign(msg).unwrap();
        vk.verify(msg, &sig).expect("valid signature should verify");
    }

    #[test]
    fn sign_empty_message() {
        let sk = SigningKey::from_bytes([9u8; 32]);
        let vk = sk.verification_key().unwrap();
        let sig = sk.sign(b"").unwrap();
        vk.verify(b"", &sig).expect("empty message should verify");
    }

    #[test]
    fn sign_is_deterministic() {
        let sk = SigningKey::from_bytes([3u8; 32]);
        let sig1 = sk.sign(b"msg").unwrap();
        let sig2 = sk.sign(b"msg").unwrap();
        assert_eq!(sig1, sig2);
    }

    // ── Verification failures ────────────────────────────────────────────

    #[test]
    fn wrong_message_fails_verification() {
        let sk = SigningKey::from_bytes([5u8; 32]);
        let vk = sk.verification_key().unwrap();
        let sig = sk.sign(b"original").unwrap();
        let result = vk.verify(b"tampered", &sig);
        assert_eq!(result, Err(CryptoError::SignatureVerificationFailed));
    }

    #[test]
    fn wrong_key_fails_verification() {
        let sk1 = SigningKey::from_bytes([1u8; 32]);
        let sk2 = SigningKey::from_bytes([2u8; 32]);
        let vk2 = sk2.verification_key().unwrap();
        let sig = sk1.sign(b"msg").unwrap();
        let result = vk2.verify(b"msg", &sig);
        assert_eq!(result, Err(CryptoError::SignatureVerificationFailed));
    }

    #[test]
    fn corrupted_signature_fails_verification() {
        let sk = SigningKey::from_bytes([4u8; 32]);
        let vk = sk.verification_key().unwrap();
        let mut sig_bytes = sk.sign(b"msg").unwrap().to_bytes();
        sig_bytes[0] ^= 0xFF;
        let bad_sig = Signature::from_bytes(sig_bytes);
        assert!(vk.verify(b"msg", &bad_sig).is_err());
    }

    #[test]
    fn invalid_verification_key_bytes_rejected() {
        // All-zero bytes are not a valid Edwards point.
        let bad_vk = VerificationKey::from_bytes([0u8; 32]);
        let sig = Signature::from_bytes([0u8; 64]);
        let result = bad_vk.verify(b"msg", &sig);
        assert!(result.is_err());
    }

    // ── Signature serialization ──────────────────────────────────────────

    #[test]
    fn signature_from_bytes_roundtrip() {
        let bytes = [0xCD; 64];
        let sig = Signature::from_bytes(bytes);
        assert_eq!(sig.to_bytes(), bytes);
    }

    // ── Trait implementations ────────────────────────────────────────────

    #[test]
    fn signing_key_debug_is_redacted() {
        let sk = SigningKey::from_bytes([0u8; 32]);
        let dbg = format!("{:?}", sk);
        assert_eq!(dbg, "SigningKey([REDACTED])");
        assert!(!dbg.contains("0000"));
    }

    #[test]
    fn signing_key_constant_time_equality() {
        let sk1 = SigningKey::from_bytes([1u8; 32]);
        let sk2 = SigningKey::from_bytes([1u8; 32]);
        let sk3 = SigningKey::from_bytes([2u8; 32]);
        assert_eq!(sk1, sk2);
        assert_ne!(sk1, sk3);
    }

    #[test]
    fn verification_key_debug_shows_bytes() {
        let sk = SigningKey::from_bytes([1u8; 32]);
        let vk = sk.verification_key().unwrap();
        let dbg = format!("{:?}", vk);
        assert!(dbg.starts_with("VerificationKey("));
    }

    // ── RFC 8032 test vectors ────────────────────────────────────────────

    #[test]
    fn rfc8032_test_vectors_sign() {
        for v in ed25519_rfc8032_vectors() {
            let sk = SigningKey::from_bytes(v.secret_key);
            let sig = sk.sign(&v.message).unwrap();
            assert_eq!(
                sig.to_bytes(),
                v.signature,
                "signature mismatch for vector: {}",
                v.name
            );
        }
    }

    #[test]
    fn rfc8032_test_vectors_verify() {
        for v in ed25519_rfc8032_vectors() {
            let vk = VerificationKey::from_bytes(v.public_key);
            let sig = Signature::from_bytes(v.signature);
            vk.verify(&v.message, &sig)
                .unwrap_or_else(|e| panic!("vector {} should verify: {}", v.name, e));
        }
    }

    #[test]
    fn rfc8032_test_vectors_key_derivation() {
        for v in ed25519_rfc8032_vectors() {
            let sk = SigningKey::from_bytes(v.secret_key);
            let vk = sk.verification_key().unwrap();
            assert_eq!(
                vk.to_bytes(),
                v.public_key,
                "VK mismatch for vector: {}",
                v.name
            );
        }
    }

    #[test]
    fn rfc8032_wrong_message_rejects() {
        let v = &ed25519_rfc8032_vectors()[0];
        let vk = VerificationKey::from_bytes(v.public_key);
        let sig = Signature::from_bytes(v.signature);
        assert!(vk.verify(b"wrong", &sig).is_err());
    }
}
