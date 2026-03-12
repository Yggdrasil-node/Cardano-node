use crate::{CryptoError, SigningKey};
use curve25519_elligator2::{edwards::EdwardsPoint as LegacyEdwardsPoint, elligator2::Legacy};
use curve25519_dalek::{
    edwards::{CompressedEdwardsY, EdwardsPoint},
    scalar::Scalar,
    traits::VartimeMultiscalarMul,
};
use curve25519_dalek::traits::IsIdentity;
use sha2::{Digest, Sha512};
use std::fmt;
use subtle::ConstantTimeEq;

const SUITE: u8 = 0x04;
const TWO: u8 = 0x02;
const THREE: u8 = 0x03;
const ZERO: u8 = 0x00;
const RFC9380_EDWARDS25519_ELL2_NU_SUITE: &[u8] = b"ECVRF_edwards25519_XMD:SHA-512_ELL2_NU_\x04";

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

    /// Verifies a batch-compatible VRF proof for a message.
    ///
    /// This follows the upstream `crypto_vrf_ietfdraft13_verify_batchcompat`
    /// relation used by `cardano-crypto-praos`.
    pub fn verify_batchcompat(
        &self,
        message: &[u8],
        proof: &VrfBatchCompatProof,
    ) -> Result<VrfOutput, CryptoError> {
        self.validate()?;

        let decoded = decode_batchcompat_proof(&proof.0)?;
        let public_key = parse_point(&self.0).map_err(|_| CryptoError::InvalidVrfVerificationKey)?;
        let h_point = encode_batchcompat_hash_point(&self.0, message)?;

        let challenge = batchcompat_challenge(
            &self.0,
            &h_point.compress().to_bytes(),
            &decoded.gamma.compress().to_bytes(),
            &decoded.announcement_1.compress().to_bytes(),
            &decoded.announcement_2.compress().to_bytes(),
        );

        let challenge_scalar = truncated_challenge_scalar(challenge);
        let neg_challenge = -challenge_scalar;

        let expected_u = EdwardsPoint::vartime_double_scalar_mul_basepoint(
            &neg_challenge,
            &public_key,
            &decoded.response,
        )
        .compress()
        .to_bytes();

        let expected_v = EdwardsPoint::vartime_multiscalar_mul(
            [&neg_challenge, &decoded.response],
            [&decoded.gamma, &h_point],
        )
        .compress()
        .to_bytes();

        if expected_u != decoded.announcement_1.compress().to_bytes()
            || expected_v != decoded.announcement_2.compress().to_bytes()
        {
            return Err(CryptoError::InvalidVrfProof);
        }

        proof.output()
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
        let decoded = decode_batchcompat_proof(&self.0)?;
        let _ = decoded.gamma;
        let _ = decoded.announcement_1;
        let _ = decoded.announcement_2;
        let _ = decoded.response;
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

fn encode_batchcompat_hash_point(
    verification_key: &[u8; VRF_VERIFICATION_KEY_SIZE],
    message: &[u8],
) -> Result<EdwardsPoint, CryptoError> {
    let mut string_to_hash = Vec::with_capacity(verification_key.len() + message.len());
    string_to_hash.extend_from_slice(verification_key);
    string_to_hash.extend_from_slice(message);

    let expanded = cardano_h2c_string_to_hash_sha512(
        48,
        RFC9380_EDWARDS25519_ELL2_NU_SUITE,
        &string_to_hash,
    );

    let mut hash_input = [0_u8; 64];
    for (index, value) in expanded.iter().rev().enumerate() {
        hash_input[index] = *value;
    }

    let representative = reduce_hash_input_to_representative(&hash_input);
    let mapped = LegacyEdwardsPoint::from_representative::<Legacy>(&representative)
        .ok_or(CryptoError::InvalidVrfProof)?
        .mul_by_cofactor()
        .compress()
        .to_bytes();

    parse_point(&mapped)
}

fn cardano_h2c_string_to_hash_sha512(h_len: u8, dst: &[u8], message: &[u8]) -> Vec<u8> {
    let mut effective_dst = dst.to_vec();
    if effective_dst.len() > u8::MAX as usize {
        let mut hash = Sha512::new();
        hash.update(b"H2C-OVERSIZE-DST-");
        hash.update(&effective_dst);
        effective_dst = hash.finalize().to_vec();
    }

    let dst_len_u8 = u8::try_from(effective_dst.len())
        .expect("effective DST length should fit in one byte");
    let mut t = [0_u8, h_len, 0_u8];

    let mut hash = Sha512::new();
    hash.update([0_u8; 128]);
    hash.update(message);
    hash.update(t);
    hash.update(&effective_dst);
    hash.update([dst_len_u8]);
    let u0: [u8; 64] = hash.finalize().into();

    let mut output = vec![0_u8; usize::from(h_len)];
    let mut ux = [0_u8; 64];
    let mut offset = 0_usize;

    while offset < output.len() {
        for (slot, value) in ux.iter_mut().zip(u0.iter()) {
            *slot ^= *value;
        }

        t[2] = t[2].wrapping_add(1);
        let mut chunk_hash = Sha512::new();
        chunk_hash.update(ux);
        chunk_hash.update([t[2]]);
        chunk_hash.update(&effective_dst);
        chunk_hash.update([dst_len_u8]);
        ux = chunk_hash.finalize().into();

        let copy_len = core::cmp::min(ux.len(), output.len() - offset);
        output[offset..offset + copy_len].copy_from_slice(&ux[..copy_len]);
        offset += copy_len;
    }

    output
}

fn reduce_hash_input_to_representative(hash_input: &[u8; 64]) -> [u8; 32] {
    let mut lower = [0_u8; 32];
    let mut upper = [0_u8; 32];
    lower.copy_from_slice(&hash_input[..32]);
    upper.copy_from_slice(&hash_input[32..]);
    let lower_high_bit = (lower[31] >> 7) as u16;
    let upper_high_bit = (upper[31] >> 7) as u16;
    lower[31] &= 0x7f;
    upper[31] &= 0x7f;

    let mut reduced = [0_u8; 33];
    let mut carry: u16 = 0;
    for index in 0..32 {
        let value = u16::from(lower[index]) + (u16::from(upper[index]) * 38) + carry;
        reduced[index] = (value & 0xff) as u8;
        carry = value >> 8;
    }
    reduced[32] = carry as u8;

    let extra = (lower_high_bit * 19) + (upper_high_bit * 722);
    let mut carry_extra = extra;
    let mut index = 0_usize;
    while carry_extra > 0 {
        let value = u16::from(reduced[index]) + (carry_extra & 0xff);
        reduced[index] = (value & 0xff) as u8;
        carry_extra = (carry_extra >> 8) + (value >> 8);
        index += 1;
    }

    let mut representative = [0_u8; 32];
    representative.copy_from_slice(&reduced[..32]);

    let high = u16::from(reduced[31] >> 7) + (u16::from(reduced[32]) << 1);
    representative[31] &= 0x7f;

    let mut carry_high = high * 19;
    let mut position = 0_usize;
    while carry_high > 0 {
        let value = u16::from(representative[position]) + (carry_high & 0xff);
        representative[position] = (value & 0xff) as u8;
        carry_high = (carry_high >> 8) + (value >> 8);
        position += 1;
    }

    let high_second_fold = representative[31] >> 7;
    representative[31] &= 0x7f;
    if high_second_fold != 0 {
        let mut carry_second_fold = u16::from(high_second_fold) * 19;
        let mut position = 0_usize;
        while carry_second_fold > 0 {
            let value = u16::from(representative[position]) + (carry_second_fold & 0xff);
            representative[position] = (value & 0xff) as u8;
            carry_second_fold = (carry_second_fold >> 8) + (value >> 8);
            position += 1;
        }
    }

    const P: [u8; 32] = [
        0xed, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0x7f,
    ];

    if representative >= P {
        let mut borrow = 0_i16;
        for idx in 0..32 {
            let value = i16::from(representative[idx]) - i16::from(P[idx]) - borrow;
            if value < 0 {
                representative[idx] = (value + 256) as u8;
                borrow = 1;
            } else {
                representative[idx] = value as u8;
                borrow = 0;
            }
        }
    }

    representative
}

fn batchcompat_challenge(
    verification_key: &[u8; VRF_VERIFICATION_KEY_SIZE],
    hash_point: &[u8; 32],
    gamma: &[u8; 32],
    announcement_1: &[u8; 32],
    announcement_2: &[u8; 32],
) -> [u8; VRF_CHALLENGE_SIZE] {
    let mut hash = Sha512::new();
    hash.update([SUITE, TWO]);
    hash.update(verification_key);
    hash.update(hash_point);
    hash.update(gamma);
    hash.update(announcement_1);
    hash.update(announcement_2);
    hash.update([ZERO]);

    let digest = hash.finalize();
    digest[..VRF_CHALLENGE_SIZE]
        .try_into()
        .expect("challenge hash prefix should match the fixed truncated challenge size")
}

fn truncated_challenge_scalar(challenge: [u8; VRF_CHALLENGE_SIZE]) -> Scalar {
    let mut scalar = [0_u8; 32];
    scalar[..VRF_CHALLENGE_SIZE].copy_from_slice(&challenge);
    Scalar::from_bytes_mod_order(scalar)
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

fn decode_batchcompat_proof(
    proof: &[u8; VRF_BATCHCOMPAT_PROOF_SIZE],
) -> Result<DecodedBatchCompatProof, CryptoError> {
    let gamma = parse_point(
        &proof[..32]
            .try_into()
            .expect("gamma slice should match the fixed point length"),
    )?;
    let announcement_1 = parse_point(
        &proof[32..64]
            .try_into()
            .expect("announcement_1 slice should match the fixed point length"),
    )?;
    let announcement_2 = parse_point(
        &proof[64..96]
            .try_into()
            .expect("announcement_2 slice should match the fixed point length"),
    )?;
    let response_bytes: [u8; 32] = proof[96..]
        .try_into()
        .expect("response slice should match the fixed scalar length");
    let response = Scalar::from_canonical_bytes(response_bytes)
        .into_option()
        .ok_or(CryptoError::InvalidVrfProof)?;

    Ok(DecodedBatchCompatProof {
        gamma,
        announcement_1,
        announcement_2,
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

struct DecodedBatchCompatProof {
    gamma: EdwardsPoint,
    announcement_1: EdwardsPoint,
    announcement_2: EdwardsPoint,
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
