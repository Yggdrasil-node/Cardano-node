use crate::{CryptoError, SigningKey};
use curve25519_dalek::traits::IsIdentity;
use curve25519_dalek::{
    edwards::{CompressedEdwardsY, EdwardsPoint},
    scalar::Scalar,
    traits::VartimeMultiscalarMul,
};
use curve25519_elligator2::{edwards::EdwardsPoint as LegacyEdwardsPoint, elligator2::Legacy};
use sha2::{Digest, Sha512};
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

const SUITE: u8 = 0x04;
const ONE: u8 = 0x01;
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
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
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

    /// Produces a standard (draft03) VRF proof for a message.
    ///
    /// Mirrors the upstream `crypto_vrf_ietfdraft03_prove` from
    /// `cardano-crypto-praos`.  Returns the deterministic VRF output
    /// and the 80-byte proof `(Gamma || challenge || response)`.
    pub fn prove(&self, message: &[u8]) -> Result<(VrfOutput, VrfProof), CryptoError> {
        self.validate()?;
        let vk_bytes = self.verification_key().to_bytes();
        let (mut secret_scalar, mut nonce_prefix) = derive_secret_scalar_and_nonce(&self.0);

        let h_point = encode_standard_hash_point(&vk_bytes, message)?;
        let h_bytes = h_point.compress().to_bytes();

        let gamma = secret_scalar * h_point;
        let gamma_bytes = gamma.compress().to_bytes();

        let mut nonce = derive_nonce(&nonce_prefix, &h_bytes);
        let k_b = nonce * curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
        let k_h = nonce * h_point;
        let k_b_bytes = k_b.compress().to_bytes();
        let k_h_bytes = k_h.compress().to_bytes();

        let challenge = standard_challenge(&h_bytes, &gamma_bytes, &k_b_bytes, &k_h_bytes);
        let challenge_scalar = truncated_challenge_scalar(challenge);
        let response = challenge_scalar * secret_scalar + nonce;

        secret_scalar.zeroize();
        nonce_prefix.zeroize();
        nonce.zeroize();

        let mut proof_bytes = [0_u8; VRF_PROOF_SIZE];
        proof_bytes[..32].copy_from_slice(&gamma_bytes);
        proof_bytes[32..48].copy_from_slice(&challenge);
        proof_bytes[48..].copy_from_slice(&response.to_bytes());

        let proof = VrfProof(proof_bytes);
        let output = proof.output()?;
        Ok((output, proof))
    }

    /// Produces a batch-compatible (draft13) VRF proof for a message.
    ///
    /// Mirrors the upstream `crypto_vrf_ietfdraft13_prove_batchcompat` from
    /// `cardano-crypto-praos`.  Returns the deterministic VRF output
    /// and the 128-byte proof `(Gamma || kB || kH || response)`.
    pub fn prove_batchcompat(
        &self,
        message: &[u8],
    ) -> Result<(VrfOutput, VrfBatchCompatProof), CryptoError> {
        self.validate()?;
        let vk_bytes = self.verification_key().to_bytes();
        let (mut secret_scalar, mut nonce_prefix) = derive_secret_scalar_and_nonce(&self.0);

        let h_point = encode_batchcompat_hash_point(&vk_bytes, message)?;
        let h_bytes = h_point.compress().to_bytes();

        let gamma = secret_scalar * h_point;
        let gamma_bytes = gamma.compress().to_bytes();

        let mut nonce = derive_nonce(&nonce_prefix, &h_bytes);
        let k_b = nonce * curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
        let k_h = nonce * h_point;
        let k_b_bytes = k_b.compress().to_bytes();
        let k_h_bytes = k_h.compress().to_bytes();

        let challenge =
            batchcompat_challenge(&vk_bytes, &h_bytes, &gamma_bytes, &k_b_bytes, &k_h_bytes);
        let challenge_scalar = truncated_challenge_scalar(challenge);
        let response = challenge_scalar * secret_scalar + nonce;

        secret_scalar.zeroize();
        nonce_prefix.zeroize();
        nonce.zeroize();

        let mut proof_bytes = [0_u8; VRF_BATCHCOMPAT_PROOF_SIZE];
        proof_bytes[..32].copy_from_slice(&gamma_bytes);
        proof_bytes[32..64].copy_from_slice(&k_b_bytes);
        proof_bytes[64..96].copy_from_slice(&k_h_bytes);
        proof_bytes[96..].copy_from_slice(&response.to_bytes());

        let proof = VrfBatchCompatProof(proof_bytes);
        let output = proof.output()?;
        Ok((output, proof))
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
    /// Follows the upstream `crypto_vrf_ietfdraft03_verify` relation used
    /// by `cardano-crypto-praos`.
    pub fn verify(&self, message: &[u8], proof: &VrfProof) -> Result<VrfOutput, CryptoError> {
        self.validate()?;
        proof.validate()?;

        let decoded = decode_standard_proof(&proof.0)?;
        let public_key =
            parse_point(&self.0).map_err(|_| CryptoError::InvalidVrfVerificationKey)?;
        let h_point = encode_standard_hash_point(&self.0, message)?;
        let h_bytes = h_point.compress().to_bytes();

        let challenge_scalar = truncated_challenge_scalar(decoded.challenge);
        let neg_challenge = -challenge_scalar;

        let u_point = EdwardsPoint::vartime_double_scalar_mul_basepoint(
            &neg_challenge,
            &public_key,
            &decoded.response,
        );

        let v_point = EdwardsPoint::vartime_multiscalar_mul(
            [&neg_challenge, &decoded.response],
            [&decoded.gamma, &h_point],
        );

        let gamma_bytes: [u8; 32] = proof.0[..32]
            .try_into()
            .expect("gamma slice should match the fixed point length");

        let recomputed = standard_challenge(
            &h_bytes,
            &gamma_bytes,
            &u_point.compress().to_bytes(),
            &v_point.compress().to_bytes(),
        );

        if recomputed != decoded.challenge {
            return Err(CryptoError::InvalidVrfProof);
        }

        proof.output()
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
        let public_key =
            parse_point(&self.0).map_err(|_| CryptoError::InvalidVrfVerificationKey)?;
        let h_point = encode_batchcompat_hash_point(&self.0, message)?;

        let challenge = batchcompat_challenge(
            &self.0,
            &h_point.compress().to_bytes(),
            &proof.0[..32]
                .try_into()
                .expect("gamma slice should match the fixed point length"),
            &proof.0[32..64]
                .try_into()
                .expect("announcement_1 slice should match the fixed point length"),
            &proof.0[64..96]
                .try_into()
                .expect("announcement_2 slice should match the fixed point length"),
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

        let announcement_1_bytes: [u8; 32] = proof.0[32..64]
            .try_into()
            .expect("announcement_1 slice should match the fixed point length");
        let announcement_2_bytes: [u8; 32] = proof.0[64..96]
            .try_into()
            .expect("announcement_2 slice should match the fixed point length");

        if expected_u != announcement_1_bytes || expected_v != announcement_2_bytes {
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

fn proof_to_output<const N: usize>(
    proof: &[u8; N],
    batch_compat: bool,
) -> Result<VrfOutput, CryptoError> {
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

    let expanded =
        cardano_h2c_string_to_hash_sha512(48, RFC9380_EDWARDS25519_ELL2_NU_SUITE, &string_to_hash);

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

/// Computes the standard (draft03) VRF hash-to-curve point.
///
/// Mirrors `cardano_ge25519_from_uniform` from `ed25519_ref10.c`:
/// `SHA-512(SUITE || ONE || pk || alpha)` → take first 32 bytes → clear
/// bit 255 → elligator2 map → normalize Edwards X sign → cofactor
/// multiply → compress → decompress to Edwards point.
///
/// The upstream C code extracts bit 255 as the target sign bit (always 0
/// since `verify.c` clears it beforehand), runs elligator2, then
/// conditionally negates the Edwards X coordinate so that
/// `is_negative(X) == x_sign`.  Because `x_sign` is always 0, this
/// forces X to be non-negative.  The Rust elligator2 crate does **not**
/// apply this normalization, so we replicate it by clearing the sign bit
/// (bit 7 of byte 31) in the compressed encoding before decompressing.
fn encode_standard_hash_point(
    verification_key: &[u8; VRF_VERIFICATION_KEY_SIZE],
    message: &[u8],
) -> Result<EdwardsPoint, CryptoError> {
    let mut hash = Sha512::new();
    hash.update([SUITE, ONE]);
    hash.update(verification_key);
    hash.update(message);
    let digest: [u8; 64] = hash.finalize().into();

    let mut r_bytes = [0_u8; 32];
    r_bytes.copy_from_slice(&digest[..32]);
    r_bytes[31] &= 0x7f;

    let mapped = LegacyEdwardsPoint::from_representative::<Legacy>(&r_bytes)
        .ok_or(CryptoError::InvalidVrfProof)?;

    // Normalize: the C code forces Edwards X to be non-negative (sign=0).
    // Compressed Edwards Y encodes the sign of X in bit 7 of byte 31.
    // Clearing that bit ensures X is non-negative after decompression.
    let mut normalized = mapped.compress().to_bytes();
    normalized[31] &= 0x7f;

    let normalized_point = curve25519_elligator2::edwards::CompressedEdwardsY(normalized)
        .decompress()
        .ok_or(CryptoError::InvalidVrfProof)?;

    // Cofactor multiply, matching cardano_ge25519_clear_cofactor.
    let cofactored = normalized_point.mul_by_cofactor();
    let final_bytes = cofactored.compress().to_bytes();

    parse_point(&final_bytes)
}

fn cardano_h2c_string_to_hash_sha512(h_len: u8, dst: &[u8], message: &[u8]) -> Vec<u8> {
    let mut effective_dst = dst.to_vec();
    if effective_dst.len() > u8::MAX as usize {
        let mut hash = Sha512::new();
        hash.update(b"H2C-OVERSIZE-DST-");
        hash.update(&effective_dst);
        effective_dst = hash.finalize().to_vec();
    }

    let dst_len_u8 =
        u8::try_from(effective_dst.len()).expect("effective DST length should fit in one byte");
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

/// Reduces a 64-byte hash to a 32-byte field element representative.
///
/// Mirrors `cardano_fe25519_reduce64` from cardano-base `ed25519_ref10.c`:
/// splits the 64-byte input into low and high 32-byte halves, clears bit 255
/// of each, adds the bit-255 corrections (`19 * low_bit255 + 722 * high_bit255`),
/// adds `38 * high` to `low`, and folds any resulting bit 255 by adding 19
/// (since `2^255 ≡ 19 mod p` where `p = 2^255 − 19`).
fn reduce_hash_input_to_representative(hash_input: &[u8; 64]) -> [u8; 32] {
    let mut lower = [0_u8; 32];
    let mut upper = [0_u8; 32];
    lower.copy_from_slice(&hash_input[..32]);
    upper.copy_from_slice(&hash_input[32..]);

    // Extract and clear bit 255 from each half.
    let lower_high_bit = (lower[31] >> 7) as u16;
    let upper_high_bit = (upper[31] >> 7) as u16;
    lower[31] &= 0x7f;
    upper[31] &= 0x7f;

    // Compute lower + 38 * upper in a 33-byte accumulator.
    let mut reduced = [0_u8; 33];
    let mut carry: u16 = 0;
    for i in 0..32 {
        let v = u16::from(lower[i]) + u16::from(upper[i]) * 38 + carry;
        reduced[i] = (v & 0xff) as u8;
        carry = v >> 8;
    }
    reduced[32] = carry as u8;

    // Add the bit-255 corrections: 2^255 ≡ 19 (mod p), 2^511 ≡ 722 (mod p).
    let extra = lower_high_bit * 19 + upper_high_bit * 722;
    let mut carry_extra = extra;
    let mut i = 0_usize;
    while carry_extra > 0 {
        let v = u16::from(reduced[i]) + (carry_extra & 0xff);
        reduced[i] = (v & 0xff) as u8;
        carry_extra = (carry_extra >> 8) + (v >> 8);
        i += 1;
    }

    // Extract and fold any overflow above 255 bits.
    let mut result = [0_u8; 32];
    result.copy_from_slice(&reduced[..32]);
    let high = u16::from(result[31] >> 7) + (u16::from(reduced[32]) << 1);
    result[31] &= 0x7f;

    let mut carry_fold = high * 19;
    let mut pos = 0_usize;
    while carry_fold > 0 {
        let v = u16::from(result[pos]) + (carry_fold & 0xff);
        result[pos] = (v & 0xff) as u8;
        carry_fold = (carry_fold >> 8) + (v >> 8);
        pos += 1;
    }

    // Second fold in case the addition of 19 set bit 255 again.
    let second_high = result[31] >> 7;
    result[31] &= 0x7f;
    if second_high != 0 {
        let mut c = 19_u16;
        for byte in result.iter_mut() {
            c += *byte as u16;
            *byte = c as u8;
            c >>= 8;
        }
        result[31] &= 0x7f;
    }

    result
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

/// Computes the standard (draft03) VRF challenge.
///
/// Mirrors the upstream `vrf03/verify.c` challenge derivation:
/// `SHA-512(SUITE || TWO || H_string || Gamma || U_string || V_string)` →
/// first 16 bytes.  Note: unlike batchcompat, the standard challenge uses
/// `H_string` (the hash-to-curve point) instead of the verification key,
/// and omits the trailing `ZERO` byte.
fn standard_challenge(
    hash_point: &[u8; 32],
    gamma: &[u8; 32],
    u_bytes: &[u8; 32],
    v_bytes: &[u8; 32],
) -> [u8; VRF_CHALLENGE_SIZE] {
    let mut hash = Sha512::new();
    hash.update([SUITE, TWO]);
    hash.update(hash_point);
    hash.update(gamma);
    hash.update(u_bytes);
    hash.update(v_bytes);

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

/// Derives the Ed25519-style secret scalar and nonce prefix from a signing key.
///
/// Mirrors the upstream C convention: `SHA-512(seed)` → first 32 bytes get
/// clamped to produce the secret scalar `x`; bytes 32..64 are the nonce prefix
/// used in deterministic nonce generation.
fn derive_secret_scalar_and_nonce(signing_key: &[u8; VRF_SIGNING_KEY_SIZE]) -> (Scalar, [u8; 32]) {
    let mut az: [u8; 64] = Sha512::digest(&signing_key[..32]).into();

    let mut clamped = [0_u8; 32];
    clamped.copy_from_slice(&az[..32]);
    clamped[0] &= 248;
    clamped[31] &= 127;
    clamped[31] |= 64;

    let secret_scalar = Scalar::from_bytes_mod_order(clamped);
    let nonce_prefix: [u8; 32] = az[32..]
        .try_into()
        .expect("nonce prefix slice should match 32 bytes");

    az.zeroize();
    clamped.zeroize();

    (secret_scalar, nonce_prefix)
}

/// Derives the deterministic nonce scalar from the nonce prefix and H point.
///
/// Mirrors the upstream C convention: `SHA-512(nonce_prefix || H_string)` →
/// 64-byte output → `sc25519_reduce` → scalar `k`.
fn derive_nonce(nonce_prefix: &[u8; 32], h_bytes: &[u8; 32]) -> Scalar {
    let mut hash = Sha512::new();
    hash.update(nonce_prefix);
    hash.update(h_bytes);
    let nonce_hash: [u8; 64] = hash.finalize().into();
    Scalar::from_bytes_mod_order_wide(&nonce_hash)
}

fn decode_standard_proof(
    proof: &[u8; VRF_PROOF_SIZE],
) -> Result<DecodedStandardProof, CryptoError> {
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
    let response = parse_vrf_response_scalar(response_bytes)?;

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
    let response = parse_vrf_response_scalar(response_bytes)?;

    Ok(DecodedBatchCompatProof {
        gamma,
        announcement_1,
        announcement_2,
        response,
    })
}

fn parse_vrf_response_scalar(response_bytes: [u8; 32]) -> Result<Scalar, CryptoError> {
    if response_bytes[31] & 0xF0 != 0
        && !bool::from(Scalar::from_canonical_bytes(response_bytes).is_some())
    {
        return Err(CryptoError::InvalidVrfProof);
    }

    Ok(Scalar::from_bytes_mod_order(response_bytes))
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::test_vectors::{vrf_praos_batchcompat_test_vectors, vrf_praos_test_vectors};

    // ── Proof decoding (existing) ────────────────────────────────────────

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

    // ── VrfSecretKey construction ────────────────────────────────────────

    #[test]
    fn secret_key_from_seed_roundtrip() {
        let seed = [0x42; VRF_SEED_SIZE];
        let sk = VrfSecretKey::from_seed(seed);
        assert_eq!(sk.seed_bytes(), seed);
    }

    #[test]
    fn secret_key_from_bytes_roundtrip() {
        let bytes = [0xAB; VRF_SIGNING_KEY_SIZE];
        let sk = VrfSecretKey::from_bytes(bytes);
        assert_eq!(sk.to_bytes(), bytes);
    }

    #[test]
    fn secret_key_normalized_matches_from_seed() {
        let seed = [0x01; VRF_SEED_SIZE];
        let sk = VrfSecretKey::from_seed(seed);
        let normalized = sk.normalized();
        assert_eq!(sk, normalized);
    }

    #[test]
    fn secret_key_validate_accepts_valid() {
        let sk = VrfSecretKey::from_seed([7u8; VRF_SEED_SIZE]);
        sk.validate().expect("valid key should pass validation");
    }

    #[test]
    fn secret_key_validate_rejects_corrupt_vk() {
        let mut bytes = VrfSecretKey::from_seed([7u8; VRF_SEED_SIZE]).to_bytes();
        bytes[VRF_SEED_SIZE] ^= 0xFF; // corrupt the embedded VK
        let sk = VrfSecretKey::from_bytes(bytes);
        assert_eq!(sk.validate(), Err(CryptoError::InvalidVrfSigningKey));
    }

    #[test]
    fn secret_key_debug_is_redacted() {
        let sk = VrfSecretKey::from_seed([0; VRF_SEED_SIZE]);
        let dbg = format!("{:?}", sk);
        assert_eq!(dbg, "VrfSecretKey([REDACTED])");
    }

    #[test]
    fn secret_key_constant_time_eq() {
        let sk1 = VrfSecretKey::from_seed([1u8; 32]);
        let sk2 = VrfSecretKey::from_seed([1u8; 32]);
        let sk3 = VrfSecretKey::from_seed([2u8; 32]);
        assert_eq!(sk1, sk2);
        assert_ne!(sk1, sk3);
    }

    // ── VrfVerificationKey ───────────────────────────────────────────────

    #[test]
    fn verification_key_from_seed() {
        let vk1 = VrfVerificationKey::from_seed([1u8; 32]);
        let vk2 = VrfSecretKey::from_seed([1u8; 32]).verification_key();
        assert_eq!(vk1, vk2);
    }

    #[test]
    fn verification_key_from_bytes_roundtrip() {
        let bytes = [0xCD; VRF_VERIFICATION_KEY_SIZE];
        let vk = VrfVerificationKey::from_bytes(bytes);
        assert_eq!(vk.to_bytes(), bytes);
    }

    #[test]
    fn verification_key_validate_accepts_valid() {
        let vk = VrfVerificationKey::from_seed([7u8; 32]);
        vk.validate().expect("valid VK should pass validation");
    }

    #[test]
    fn verification_key_validate_rejects_identity() {
        // Compressed identity point on Ed25519 = [1, 0, ..., 0]
        let mut identity = [0u8; 32];
        identity[0] = 1;
        let vk = VrfVerificationKey::from_bytes(identity);
        assert_eq!(vk.validate(), Err(CryptoError::InvalidVrfVerificationKey));
    }

    #[test]
    fn verification_key_validate_rejects_low_order() {
        // The all-zero compressed Edwards Y is the identity point (0, 1);
        // mul_by_cofactor produces the identity, which must be rejected.
        let identity = {
            let mut buf = [0u8; 32];
            buf[0] = 0x01; // compressed (0,1) is [1,0,...,0]
            buf
        };
        let vk = VrfVerificationKey::from_bytes(identity);
        assert!(vk.validate().is_err(), "low-order VK should be rejected");
    }

    // ── VrfProof / VrfBatchCompatProof construction ──────────────────────

    #[test]
    fn proof_from_bytes_roundtrip() {
        let bytes = [0x11; VRF_PROOF_SIZE];
        let proof = VrfProof::from_bytes(bytes);
        assert_eq!(proof.to_bytes(), bytes);
    }

    #[test]
    fn batchcompat_proof_from_bytes_roundtrip() {
        let bytes = [0x22; VRF_BATCHCOMPAT_PROOF_SIZE];
        let proof = VrfBatchCompatProof::from_bytes(bytes);
        assert_eq!(proof.to_bytes(), bytes);
    }

    #[test]
    fn vrf_output_from_bytes_roundtrip() {
        let bytes = [0x33; VRF_OUTPUT_SIZE];
        let out = VrfOutput::from_bytes(bytes);
        assert_eq!(out.to_bytes(), bytes);
    }

    // ── Standard prove / verify roundtrip ────────────────────────────────

    #[test]
    fn standard_prove_verify_roundtrip() {
        let sk = VrfSecretKey::from_seed([5u8; 32]);
        let vk = sk.verification_key();
        let msg = b"hello vrf";
        let (output, proof) = sk.prove(msg).unwrap();
        let verified_output = vk.verify(msg, &proof).unwrap();
        assert_eq!(output, verified_output);
    }

    #[test]
    fn standard_prove_is_deterministic() {
        let sk = VrfSecretKey::from_seed([3u8; 32]);
        let (o1, p1) = sk.prove(b"msg").unwrap();
        let (o2, p2) = sk.prove(b"msg").unwrap();
        assert_eq!(o1, o2);
        assert_eq!(p1, p2);
    }

    #[test]
    fn standard_prove_different_messages_different_output() {
        let sk = VrfSecretKey::from_seed([4u8; 32]);
        let (o1, _) = sk.prove(b"msg1").unwrap();
        let (o2, _) = sk.prove(b"msg2").unwrap();
        assert_ne!(o1, o2);
    }

    #[test]
    fn standard_verify_wrong_message_fails() {
        let sk = VrfSecretKey::from_seed([6u8; 32]);
        let vk = sk.verification_key();
        let (_, proof) = sk.prove(b"good").unwrap();
        assert_eq!(vk.verify(b"bad", &proof), Err(CryptoError::InvalidVrfProof));
    }

    #[test]
    fn standard_verify_wrong_key_fails() {
        let sk1 = VrfSecretKey::from_seed([1u8; 32]);
        let vk2 = VrfVerificationKey::from_seed([2u8; 32]);
        let (_, proof) = sk1.prove(b"msg").unwrap();
        assert!(vk2.verify(b"msg", &proof).is_err());
    }

    #[test]
    fn standard_prove_empty_message() {
        let sk = VrfSecretKey::from_seed([8u8; 32]);
        let vk = sk.verification_key();
        let (output, proof) = sk.prove(b"").unwrap();
        let verified = vk.verify(b"", &proof).unwrap();
        assert_eq!(output, verified);
    }

    // ── Batchcompat prove / verify roundtrip ─────────────────────────────

    #[test]
    fn batchcompat_prove_verify_roundtrip() {
        let sk = VrfSecretKey::from_seed([5u8; 32]);
        let vk = sk.verification_key();
        let msg = b"batchcompat test";
        let (output, proof) = sk.prove_batchcompat(msg).unwrap();
        let verified_output = vk.verify_batchcompat(msg, &proof).unwrap();
        assert_eq!(output, verified_output);
    }

    #[test]
    fn batchcompat_prove_is_deterministic() {
        let sk = VrfSecretKey::from_seed([3u8; 32]);
        let (o1, p1) = sk.prove_batchcompat(b"msg").unwrap();
        let (o2, p2) = sk.prove_batchcompat(b"msg").unwrap();
        assert_eq!(o1, o2);
        assert_eq!(p1, p2);
    }

    #[test]
    fn batchcompat_verify_wrong_message_fails() {
        let sk = VrfSecretKey::from_seed([6u8; 32]);
        let vk = sk.verification_key();
        let (_, proof) = sk.prove_batchcompat(b"good").unwrap();
        assert_eq!(
            vk.verify_batchcompat(b"bad", &proof),
            Err(CryptoError::InvalidVrfProof)
        );
    }

    #[test]
    fn batchcompat_verify_wrong_key_fails() {
        let sk1 = VrfSecretKey::from_seed([1u8; 32]);
        let vk2 = VrfVerificationKey::from_seed([2u8; 32]);
        let (_, proof) = sk1.prove_batchcompat(b"msg").unwrap();
        assert!(vk2.verify_batchcompat(b"msg", &proof).is_err());
    }

    #[test]
    fn batchcompat_prove_empty_message() {
        let sk = VrfSecretKey::from_seed([8u8; 32]);
        let vk = sk.verification_key();
        let (output, proof) = sk.prove_batchcompat(b"").unwrap();
        let verified = vk.verify_batchcompat(b"", &proof).unwrap();
        assert_eq!(output, verified);
    }

    // ── Standard vs batchcompat outputs differ ───────────────────────────

    #[test]
    fn standard_and_batchcompat_produce_different_outputs() {
        let sk = VrfSecretKey::from_seed([9u8; 32]);
        let (std_out, _) = sk.prove(b"msg").unwrap();
        let (bc_out, _) = sk.prove_batchcompat(b"msg").unwrap();
        // Different VRF schemes should produce different outputs for same input.
        assert_ne!(std_out, bc_out);
    }

    // ── Standard test vector verification ────────────────────────────────

    #[test]
    fn standard_test_vectors_prove() {
        for v in vrf_praos_test_vectors() {
            let sk = VrfSecretKey::from_bytes(v.secret_key);
            let (output, proof) = sk
                .prove(&v.message)
                .unwrap_or_else(|e| panic!("prove failed for {}: {e}", v.name));
            assert_eq!(
                proof.to_bytes(),
                v.proof,
                "proof mismatch for vector {}",
                v.name
            );
            assert_eq!(
                output.to_bytes(),
                v.output,
                "output mismatch for vector {}",
                v.name
            );
        }
    }

    #[test]
    fn standard_test_vectors_verify() {
        for v in vrf_praos_test_vectors() {
            let vk = VrfVerificationKey::from_bytes(v.public_key);
            let proof = VrfProof::from_bytes(v.proof);
            let output = vk
                .verify(&v.message, &proof)
                .unwrap_or_else(|e| panic!("verify failed for {}: {e}", v.name));
            assert_eq!(
                output.to_bytes(),
                v.output,
                "output mismatch for vector {}",
                v.name
            );
        }
    }

    #[test]
    fn standard_test_vectors_key_derivation() {
        for v in vrf_praos_test_vectors() {
            let sk = VrfSecretKey::from_bytes(v.secret_key);
            assert_eq!(
                sk.verification_key().to_bytes(),
                v.public_key,
                "VK mismatch for vector {}",
                v.name
            );
        }
    }

    #[test]
    fn standard_test_vectors_output_from_proof() {
        for v in vrf_praos_test_vectors() {
            let proof = VrfProof::from_bytes(v.proof);
            let output = proof
                .output()
                .unwrap_or_else(|e| panic!("output() failed for {}: {e}", v.name));
            assert_eq!(
                output.to_bytes(),
                v.output,
                "output mismatch for vector {}",
                v.name
            );
        }
    }

    // ── Batchcompat test vector verification ─────────────────────────────

    #[test]
    fn batchcompat_test_vectors_prove() {
        for v in vrf_praos_batchcompat_test_vectors() {
            let sk = VrfSecretKey::from_bytes(v.secret_key);
            let (output, proof) = sk
                .prove_batchcompat(&v.message)
                .unwrap_or_else(|e| panic!("prove_batchcompat failed for {}: {e}", v.name));
            assert_eq!(
                proof.to_bytes(),
                v.proof,
                "proof mismatch for vector {}",
                v.name
            );
            assert_eq!(
                output.to_bytes(),
                v.output,
                "output mismatch for vector {}",
                v.name
            );
        }
    }

    #[test]
    fn batchcompat_test_vectors_verify() {
        for v in vrf_praos_batchcompat_test_vectors() {
            let vk = VrfVerificationKey::from_bytes(v.public_key);
            let proof = VrfBatchCompatProof::from_bytes(v.proof);
            let output = vk
                .verify_batchcompat(&v.message, &proof)
                .unwrap_or_else(|e| panic!("verify_batchcompat failed for {}: {e}", v.name));
            assert_eq!(
                output.to_bytes(),
                v.output,
                "output mismatch for vector {}",
                v.name
            );
        }
    }

    #[test]
    fn batchcompat_test_vectors_key_derivation() {
        for v in vrf_praos_batchcompat_test_vectors() {
            let sk = VrfSecretKey::from_bytes(v.secret_key);
            assert_eq!(
                sk.verification_key().to_bytes(),
                v.public_key,
                "VK mismatch for vector {}",
                v.name
            );
        }
    }

    #[test]
    fn batchcompat_test_vectors_output_from_proof() {
        for v in vrf_praos_batchcompat_test_vectors() {
            let proof = VrfBatchCompatProof::from_bytes(v.proof);
            let output = proof
                .output()
                .unwrap_or_else(|e| panic!("output() failed for {}: {e}", v.name));
            assert_eq!(
                output.to_bytes(),
                v.output,
                "output mismatch for vector {}",
                v.name
            );
        }
    }

    // ── Proof validation ─────────────────────────────────────────────────

    #[test]
    fn standard_proof_validate_published_vectors() {
        for v in vrf_praos_test_vectors() {
            let proof = VrfProof::from_bytes(v.proof);
            proof
                .validate()
                .unwrap_or_else(|e| panic!("validate failed for {}: {e}", v.name));
        }
    }

    #[test]
    fn batchcompat_proof_validate_published_vectors() {
        for v in vrf_praos_batchcompat_test_vectors() {
            let proof = VrfBatchCompatProof::from_bytes(v.proof);
            proof
                .validate()
                .unwrap_or_else(|e| panic!("validate failed for {}: {e}", v.name));
        }
    }
}
