use crate::{CryptoError, SigningKey};
use curve25519_dalek::{edwards::{CompressedEdwardsY, EdwardsPoint}, scalar::Scalar};
use curve25519_dalek::traits::IsIdentity;
use sha2::{Digest, Sha512};
use std::fmt;
use subtle::ConstantTimeEq;

const SUITE: u8 = 0x04;
const THREE: u8 = 0x03;
const ZERO: u8 = 0x00;

/// Serialized size of a Praos VRF signing key.
pub const VRF_SIGNING_KEY_SIZE: usize = 64;
/// Serialized size of a Praos VRF verification key.
pub const VRF_VERIFICATION_KEY_SIZE: usize = 32;
/// Serialized size of a Praos VRF proof.
pub const VRF_PROOF_SIZE: usize = 80;
/// Serialized size of a Praos batch-compatible VRF proof.
pub const VRF_BATCHCOMPAT_PROOF_SIZE: usize = 128;
/// Serialized size of a Praos VRF output.
pub const VRF_OUTPUT_SIZE: usize = 64;
/// Serialized size of a Praos VRF seed.
pub const VRF_SEED_SIZE: usize = 32;
const VRF_CHALLENGE_SIZE: usize = 16;

/// A byte-backed Praos VRF signing key.
///
/// Cardano serializes this key as the 32-byte seed followed by the 32-byte
/// verification key, matching the upstream `cardano-crypto-praos` layout.
#[derive(Clone)]
pub struct VrfSecretKey(pub [u8; VRF_SIGNING_KEY_SIZE]);

/// A byte-backed Praos VRF verification key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VrfVerificationKey(pub [u8; VRF_VERIFICATION_KEY_SIZE]);

/// A byte-backed Praos VRF proof using the 80-byte draft03 layout.
///
/// The current standard proof fixtures in this workspace follow the older
/// draft03-era Cardano Praos layout mirrored from `cardano-crypto-praos`, not
/// the final RFC 9381 Edwards25519 proof and challenge conventions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VrfProof(pub [u8; VRF_PROOF_SIZE]);

/// A byte-backed Praos batch-compatible VRF proof using the 128-byte draft13 layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VrfBatchCompatProof(pub [u8; VRF_BATCHCOMPAT_PROOF_SIZE]);

/// A byte-backed VRF output hash.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VrfOutput(pub [u8; VRF_OUTPUT_SIZE]);

impl VrfSecretKey {
    /// Derives a VRF signing key from a 32-byte seed.
    ///
    /// Upstream Praos serializes the signing key as `seed || vk`, where `vk`
    /// is the Ed25519-compatible public key derived from the hashed and clamped
    /// seed.
    pub fn from_seed(seed: [u8; VRF_SEED_SIZE]) -> Self {
        let verification_key = SigningKey::from_bytes(seed)
            .verification_key()
            .expect("a 32-byte seed should derive a VRF verification key")
            .to_bytes();
        let mut bytes = [0_u8; VRF_SIGNING_KEY_SIZE];
        bytes[..VRF_SEED_SIZE].copy_from_slice(&seed);
        bytes[VRF_SEED_SIZE..].copy_from_slice(&verification_key);
        Self(bytes)
    }

    /// Constructs a VRF signing key from its 64-byte serialized form.
    pub fn from_bytes(bytes: [u8; VRF_SIGNING_KEY_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 64-byte serialized signing key.
    pub fn to_bytes(&self) -> [u8; VRF_SIGNING_KEY_SIZE] {
        self.0
    }

    /// Returns the 32-byte seed prefix stored in the serialized signing key.
    pub fn seed_bytes(&self) -> [u8; VRF_SEED_SIZE] {
        self.0[..VRF_SEED_SIZE]
            .try_into()
            .expect("seed slice should match the fixed VRF seed length")
    }

    /// Derives the verification key embedded in the serialized signing key.
    pub fn verification_key(&self) -> VrfVerificationKey {
        VrfVerificationKey(
            self.0[VRF_SEED_SIZE..]
                .try_into()
                .expect("verification key slice should match the fixed VRF key length"),
        )
    }

    /// Re-derives the serialized signing key from its embedded seed prefix.
    pub fn normalized(&self) -> Self {
        Self::from_seed(self.seed_bytes())
    }

    /// Validates that the embedded verification key matches the seed prefix.
    ///
    /// Cardano serializes Praos signing keys as `seed || vk`. This rejects
    /// malformed key material whose verification-key suffix does not match the
    /// verification key deterministically derived from the seed.
    pub fn validate(&self) -> Result<(), CryptoError> {
        if self.normalized() != *self {
            return Err(CryptoError::InvalidVrfSigningKey);
        }

        Ok(())
    }

    /// Produces a VRF proof for a message.
    ///
    /// Full Praos proof generation remains unimplemented until the workspace has
    /// a pure Rust ECVRF path that can be validated against upstream vectors.
    pub fn prove(&self, _message: &[u8]) -> Result<(VrfOutput, VrfProof), CryptoError> {
        self.validate()?;
        Err(CryptoError::Unimplemented("VRF proof generation"))
    }
}

impl fmt::Debug for VrfSecretKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VrfSecretKey([REDACTED])")
    }
}

impl PartialEq for VrfSecretKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for VrfSecretKey {}

impl VrfVerificationKey {
    /// Derives a VRF verification key directly from a 32-byte seed.
    pub fn from_seed(seed: [u8; VRF_SEED_SIZE]) -> Self {
        VrfSecretKey::from_seed(seed).verification_key()
    }

    /// Constructs a VRF verification key from its 32-byte serialized form.
    pub fn from_bytes(bytes: [u8; VRF_VERIFICATION_KEY_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 32-byte serialized verification key.
    pub fn to_bytes(&self) -> [u8; VRF_VERIFICATION_KEY_SIZE] {
        self.0
    }

    /// Validates this verification key encoding.
    ///
    /// The key must decode to an Edwards point and must not be a
    /// low-order point after multiplying by the curve cofactor.
    pub fn validate(&self) -> Result<(), CryptoError> {
        let point = CompressedEdwardsY(self.0)
            .decompress()
            .ok_or(CryptoError::InvalidVrfVerificationKey)?;

        if point.mul_by_cofactor().is_identity() {
            return Err(CryptoError::InvalidVrfVerificationKey);
        }

        Ok(())
    }

    /// Verifies a VRF proof for a message.
    ///
    /// Full Praos verification remains unimplemented until the workspace has a
    /// pure Rust ECVRF path with upstream vector parity.
    pub fn verify(&self, message: &[u8], proof: &VrfProof) -> Result<VrfOutput, CryptoError> {
        self.validate()?;
        proof.validate()?;
        let _ = message;

        Err(CryptoError::Unimplemented("VRF verification"))
    }
}

impl VrfProof {
    /// Constructs a Praos proof from its 80-byte serialized form.
    pub fn from_bytes(bytes: [u8; VRF_PROOF_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 80-byte serialized Praos proof.
    pub fn to_bytes(&self) -> [u8; VRF_PROOF_SIZE] {
        self.0
    }

    /// Validates the structural encoding constraints of this proof.
    ///
    /// This checks point and scalar encoding constraints that are required
    /// before proof verification can proceed.
    pub fn validate(&self) -> Result<(), CryptoError> {
        let decoded = decode_standard_proof(&self.0)?;
        let _ = decoded.gamma;
        let _ = decoded.challenge;
        let _ = decoded.response;
        Ok(())
    }

    /// Computes the Praos VRF output hash encoded by this proof.
    pub fn output(&self) -> Result<VrfOutput, CryptoError> {
        proof_to_output(&self.0, false)
    }
}

impl VrfBatchCompatProof {
    /// Constructs a batch-compatible Praos proof from its 128-byte serialized form.
    pub fn from_bytes(bytes: [u8; VRF_BATCHCOMPAT_PROOF_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 128-byte serialized batch-compatible Praos proof.
    pub fn to_bytes(&self) -> [u8; VRF_BATCHCOMPAT_PROOF_SIZE] {
        self.0
    }

    /// Validates the structural encoding constraints of this proof.
    ///
    /// This checks point and scalar encoding constraints that are required
    /// before proof verification can proceed.
    pub fn validate(&self) -> Result<(), CryptoError> {
        parse_gamma_bytes(&self.0)?;
        Ok(())
    }

    /// Computes the batch-compatible Praos VRF output hash encoded by this proof.
    pub fn output(&self) -> Result<VrfOutput, CryptoError> {
        proof_to_output(&self.0, true)
    }
}

impl VrfOutput {
    /// Constructs a VRF output from its 64-byte serialized form.
    pub fn from_bytes(bytes: [u8; VRF_OUTPUT_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 64-byte serialized VRF output.
    pub fn to_bytes(&self) -> [u8; VRF_OUTPUT_SIZE] {
        self.0
    }
}

fn proof_to_output<const N: usize>(proof: &[u8; N], batch_compat: bool) -> Result<VrfOutput, CryptoError> {
    let gamma_bytes = parse_gamma_bytes(proof)?;
    let mut hash = Sha512::new();
    hash.update([SUITE, THREE]);
    hash.update(gamma_bytes);
    if batch_compat {
        hash.update([ZERO]);
    }

    Ok(VrfOutput(hash.finalize().into()))
}

fn decode_standard_proof(proof: &[u8; VRF_PROOF_SIZE]) -> Result<DecodedStandardProof, CryptoError> {
    let gamma = parse_point(
        &proof[..32]
            .try_into()
            .expect("gamma slice should match the fixed point length"),
    )?;
    let challenge = proof[32..32 + VRF_CHALLENGE_SIZE]
        .try_into()
        .expect("challenge slice should match the fixed challenge length");
    let response_bytes: [u8; 32] = proof[32 + VRF_CHALLENGE_SIZE..]
        .try_into()
        .expect("response slice should match the fixed scalar length");
    let response = Scalar::from_canonical_bytes(response_bytes)
        .into_option()
        .ok_or(CryptoError::InvalidVrfProof)?;

    Ok(DecodedStandardProof {
        gamma,
        challenge,
        response,
    })
}

fn parse_gamma_bytes<const N: usize>(proof: &[u8; N]) -> Result<[u8; 32], CryptoError> {
    let scalar_offset = N - 32;
    let scalar_bytes: [u8; 32] = proof[scalar_offset..]
        .try_into()
        .expect("scalar slice should match the fixed scalar length");

    if scalar_bytes[31] & 0xF0 != 0
        && !bool::from(Scalar::from_canonical_bytes(scalar_bytes).is_some())
    {
        return Err(CryptoError::InvalidVrfProof);
    }

    let gamma = parse_point(
        &proof[..32]
            .try_into()
            .expect("gamma slice should match the fixed point length"),
    )?
    .mul_by_cofactor();

    Ok(gamma.compress().to_bytes())
}

fn parse_point(bytes: &[u8; 32]) -> Result<EdwardsPoint, CryptoError> {
    CompressedEdwardsY(*bytes)
        .decompress()
        .ok_or(CryptoError::InvalidVrfProof)
}

struct DecodedStandardProof {
    gamma: EdwardsPoint,
    challenge: [u8; VRF_CHALLENGE_SIZE],
    response: Scalar,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_vectors::vrf_praos_test_vectors;

    #[test]
    fn standard_proof_decoding_extracts_challenge_and_response() {
        let vector = vrf_praos_test_vectors()
            .into_iter()
            .nth(1)
            .expect("the second published standard Praos vector should exist");
        let decoded = decode_standard_proof(&vector.proof).expect("published proof should decode");

        assert_eq!(decoded.gamma.compress().to_bytes(), vector.proof[..32]);
        assert_eq!(decoded.challenge, vector.proof[32..48]);
        assert_eq!(decoded.response.to_bytes(), vector.proof[48..80]);
    }
}
