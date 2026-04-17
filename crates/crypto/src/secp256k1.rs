//! secp256k1 ECDSA and Schnorr signature verification.
//!
//! Used by PlutusV2 builtins `verifyEcdsaSecp256k1Signature` and
//! `verifySchnorrSecp256k1Signature`.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Crypto/Secp256k1.hs>
//!
//! ## ECDSA verification
//!
//! The Plutus ECDSA builtin expects:
//! - `vk`:  33 bytes (SEC1 compressed public key)
//! - `msg`: 32 bytes (already-hashed message digest)
//! - `sig`: 64 bytes (r ‖ s, each 32 bytes big-endian)
//!
//! ## Schnorr (BIP-340) verification
//!
//! The Plutus Schnorr builtin expects:
//! - `vk`:  32 bytes (x-only public key per BIP-340)
//! - `msg`: arbitrary-length message
//! - `sig`: 64 bytes (BIP-340 Schnorr signature)

use crate::CryptoError;

// ---------------------------------------------------------------------------
// ECDSA verification
// ---------------------------------------------------------------------------

/// Verify a secp256k1 ECDSA signature.
///
/// * `vk` — SEC1-compressed public key (33 bytes).
/// * `msg` — Pre-hashed 32-byte message digest.
/// * `sig` — 64-byte signature (r ‖ s, each 32 bytes big-endian).
///
/// Returns `Ok(true)` if valid, `Ok(false)` if the signature does not
/// match, or `Err` if the inputs have invalid lengths or encoding.
pub fn verify_ecdsa(vk: &[u8], msg: &[u8], sig: &[u8]) -> Result<bool, CryptoError> {
    use k256::ecdsa::{Signature, VerifyingKey, signature::hazmat::PrehashVerifier};

    if vk.len() != 33 {
        return Err(CryptoError::InvalidKey(format!(
            "ECDSA secp256k1 public key must be 33 bytes, got {}",
            vk.len()
        )));
    }
    if msg.len() != 32 {
        return Err(CryptoError::InvalidKey(format!(
            "ECDSA secp256k1 message digest must be 32 bytes, got {}",
            msg.len()
        )));
    }
    if sig.len() != 64 {
        return Err(CryptoError::SignatureFormat(format!(
            "ECDSA secp256k1 signature must be 64 bytes, got {}",
            sig.len()
        )));
    }

    let verifying_key = match VerifyingKey::from_sec1_bytes(vk) {
        Ok(key) => key,
        Err(_) => return Ok(false),
    };

    let signature = match Signature::from_slice(sig) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };

    // Plutus feeds a pre-hashed digest, so we use `verify_prehash`.
    match verifying_key.verify_prehash(msg, &signature) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// Schnorr (BIP-340) verification
// ---------------------------------------------------------------------------

/// Verify a secp256k1 Schnorr (BIP-340) signature.
///
/// * `vk` — 32-byte x-only public key.
/// * `msg` — Arbitrary-length message bytes.
/// * `sig` — 64-byte BIP-340 Schnorr signature.
///
/// Returns `Ok(true)` if valid, `Ok(false)` if the signature does not
/// match, or `Err` if the inputs have invalid lengths or encoding.
pub fn verify_schnorr(vk: &[u8], msg: &[u8], sig: &[u8]) -> Result<bool, CryptoError> {
    use k256::schnorr::{Signature, VerifyingKey, signature::Verifier};

    if vk.len() != 32 {
        return Err(CryptoError::InvalidKey(format!(
            "Schnorr secp256k1 public key must be 32 bytes, got {}",
            vk.len()
        )));
    }
    if sig.len() != 64 {
        return Err(CryptoError::SignatureFormat(format!(
            "Schnorr secp256k1 signature must be 64 bytes, got {}",
            sig.len()
        )));
    }

    let verifying_key = match VerifyingKey::from_bytes(vk) {
        Ok(key) => key,
        Err(_) => return Ok(false),
    };

    let signature = match Signature::try_from(sig) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };

    match verifying_key.verify(msg, &signature) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecdsa_rejects_wrong_key_length() {
        let result = verify_ecdsa(&[0u8; 32], &[0u8; 32], &[0u8; 64]);
        assert!(result.is_err());
    }

    #[test]
    fn ecdsa_rejects_wrong_message_length() {
        let result = verify_ecdsa(&[0u8; 33], &[0u8; 31], &[0u8; 64]);
        assert!(result.is_err());
    }

    #[test]
    fn ecdsa_rejects_wrong_sig_length() {
        let result = verify_ecdsa(&[0u8; 33], &[0u8; 32], &[0u8; 63]);
        assert!(result.is_err());
    }

    #[test]
    fn ecdsa_invalid_key_returns_false() {
        // All-zero is not a valid SEC1 point.
        let result = verify_ecdsa(&[0u8; 33], &[0u8; 32], &[0u8; 64]);
        assert_eq!(result, Ok(false));
    }

    #[test]
    fn schnorr_rejects_wrong_key_length() {
        let result = verify_schnorr(&[0u8; 33], b"hello", &[0u8; 64]);
        assert!(result.is_err());
    }

    #[test]
    fn schnorr_rejects_wrong_sig_length() {
        let result = verify_schnorr(&[0u8; 32], b"hello", &[0u8; 63]);
        assert!(result.is_err());
    }

    #[test]
    fn schnorr_invalid_key_returns_false() {
        let result = verify_schnorr(&[0u8; 32], b"hello", &[0u8; 64]);
        assert_eq!(result, Ok(false));
    }

    /// Round-trip: generate a key pair, sign, and verify.
    #[test]
    fn ecdsa_sign_and_verify_round_trip() {
        use k256::ecdsa::{Signature, SigningKey, signature::hazmat::PrehashSigner};
        use sha2::{Digest, Sha256};

        let signing_key = SigningKey::from_slice(&[1u8; 32]).expect("valid key");
        let verifying_key = signing_key.verifying_key();

        let msg = b"test message for ecdsa";
        let digest: [u8; 32] = Sha256::digest(msg).into();

        // Use prehash signing to match our prehash verification.
        let sig: Signature = signing_key.sign_prehash(&digest).expect("signing ok");
        let sig_bytes = sig.to_bytes();

        let vk_bytes = verifying_key.to_sec1_bytes();

        let result = verify_ecdsa(&vk_bytes, &digest, &sig_bytes);
        assert_eq!(result, Ok(true));

        // Mutate one byte of the signature.
        let mut bad_sig = sig_bytes.to_vec();
        bad_sig[0] ^= 0xFF;
        let result = verify_ecdsa(&vk_bytes, &digest, &bad_sig);
        assert_eq!(result, Ok(false));
    }

    /// Round-trip: generate a key pair, sign, and verify (Schnorr).
    #[test]
    fn schnorr_sign_and_verify_round_trip() {
        use k256::schnorr::{SigningKey, signature::Signer};

        let signing_key = SigningKey::from_bytes(&[1u8; 32]).expect("valid key");
        let verifying_key = signing_key.verifying_key();

        let msg = b"test message for schnorr";
        let sig = signing_key.sign(msg);
        let sig_bytes = sig.to_bytes();

        let vk_bytes = verifying_key.to_bytes();

        let result = verify_schnorr(&vk_bytes, msg, &sig_bytes);
        assert_eq!(result, Ok(true));

        // Mutate one byte of the signature.
        let mut bad_sig = sig_bytes.to_vec();
        bad_sig[0] ^= 0xFF;
        let result = verify_schnorr(&vk_bytes, msg, &bad_sig);
        assert_eq!(result, Ok(false));
    }
}
