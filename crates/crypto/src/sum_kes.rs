//! Sum-composition Key-Evolving Signatures (SumKES).
//!
//! Implements the binary sum composition from Section 3.1 of:
//!
//! > "Composition and Efficiency Tradeoffs for Forward-Secure Digital
//! > Signatures" — Tal Malkin, Daniele Micciancio, Sara Miner
//! > <https://eprint.iacr.org/2001/034>
//!
//! A `SumKES` of *depth d* supports `2^d` signing periods by recursively
//! splitting a seed into left/right halves and building a binary tree of
//! single-period Ed25519 keys at the leaves.
//!
//! ## Upstream Reference
//!
//! `Cardano.Crypto.KES.Sum` in `cardano-base`.
//!
//! ## Size Chart
//!
//! | Depth | Periods | VK size | Sig size                     |
//! |-------|---------|---------|------------------------------|
//! | 0     | 1       | 32 B    | 64 B                         |
//! | 1     | 2       | 32 B    | 64 + 2×32 = 128 B            |
//! | d     | 2^d     | 32 B    | 64 + d×64 B                  |
//! | 6     | 64      | 32 B    | 64 + 6×64 = 448 B (mainnet)  |

use crate::blake2b::hash_bytes_256;
use crate::ed25519::SigningKey;
use crate::error::CryptoError;
use crate::kes::{KesPeriod, KesSignature, KesSigningKey, KesVerificationKey};
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

// ───────────────────────────────────────────────────────────────────────────
// Constants
// ───────────────────────────────────────────────────────────────────────────

/// Size of a single-period verification key (Ed25519 = 32 bytes).
const VK_SIZE: usize = 32;
/// Size of a single-period signature (Ed25519 = 64 bytes).
const SIG_SIZE: usize = 64;
/// Size of the KES seed (Ed25519 = 32 bytes).
const SEED_SIZE: usize = 32;

// ───────────────────────────────────────────────────────────────────────────
// SumKES Verification Key
// ───────────────────────────────────────────────────────────────────────────

/// SumKES verification key — always 32 bytes regardless of depth.
///
/// At depth 0 this is the raw Ed25519 verification key. At depth > 0
/// this is `Blake2b-256(vk_left || vk_right)`.
///
/// Reference: `VerKeySumKES` in upstream.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SumKesVerificationKey(pub [u8; VK_SIZE]);

impl fmt::Debug for SumKesVerificationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SumKesVK({:02x}{:02x}…)", self.0[0], self.0[1])
    }
}

impl SumKesVerificationKey {
    /// Constructs a verification key from its 32-byte encoding.
    pub fn from_bytes(bytes: [u8; VK_SIZE]) -> Self {
        Self(bytes)
    }

    /// Returns the 32-byte encoded verification key.
    pub fn to_bytes(&self) -> [u8; VK_SIZE] {
        self.0
    }
}

// ───────────────────────────────────────────────────────────────────────────
// SumKES Signature
// ───────────────────────────────────────────────────────────────────────────

/// SumKES signature carrying the base Ed25519 signature and a Merkle path
/// of sibling verification keys.
///
/// At depth `d` the encoding is:
///   `ed25519_sig (64 B) || vk_0 (32 B) || vk_1 (32 B) || … (d pairs)`
///
/// Total size: `64 + d * 64` bytes.
///
/// Reference: `SigSumKES` in upstream — `(sigma, vk_0, vk_1)` per level.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SumKesSignature {
    depth: u32,
    data: Vec<u8>,
}

impl SumKesSignature {
    /// Returns the depth (number of composition levels).
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// Expected serialized byte length for a given depth.
    pub fn expected_size(depth: u32) -> usize {
        SIG_SIZE + (depth as usize) * (2 * VK_SIZE)
    }

    /// Constructs a signature from raw bytes at the given depth.
    pub fn from_bytes(depth: u32, data: &[u8]) -> Result<Self, CryptoError> {
        let expected = Self::expected_size(depth);
        if data.len() != expected {
            return Err(CryptoError::InvalidKesKeyMaterialLength(data.len()));
        }
        Ok(Self {
            depth,
            data: data.to_vec(),
        })
    }

    /// Returns the raw serialized bytes.
    pub fn to_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Extracts the base Ed25519 signature bytes.
    fn base_signature(&self) -> [u8; SIG_SIZE] {
        self.data[..SIG_SIZE]
            .try_into()
            .expect("SumKES signature should contain base Ed25519 signature")
    }

    /// Extracts the left sibling VK at composition level `level` (0-indexed
    /// from the outermost).
    fn vk_left(&self, level: u32) -> [u8; VK_SIZE] {
        let off = SIG_SIZE + (level as usize) * (2 * VK_SIZE);
        self.data[off..off + VK_SIZE]
            .try_into()
            .expect("SumKES signature should contain left VK at this level")
    }

    /// Extracts the right sibling VK at composition level `level`.
    fn vk_right(&self, level: u32) -> [u8; VK_SIZE] {
        let off = SIG_SIZE + (level as usize) * (2 * VK_SIZE) + VK_SIZE;
        self.data[off..off + VK_SIZE]
            .try_into()
            .expect("SumKES signature should contain right VK at this level")
    }
}

// ───────────────────────────────────────────────────────────────────────────
// SumKES Signing Key
// ───────────────────────────────────────────────────────────────────────────

/// SumKES signing key at a given depth.
///
/// At depth 0 this wraps a 32-byte Ed25519 seed.
/// At depth > 0 this contains:
///   `sk_current (recursive) || seed_right (32 B) || vk_left (32 B) || vk_right (32 B)`
///
/// Reference: `SignKeySumKES` — `(sk_0, r_1, vk_0, vk_1)` in upstream.
#[derive(Clone)]
pub struct SumKesSigningKey {
    depth: u32,
    data: Vec<u8>,
}

impl Zeroize for SumKesSigningKey {
    fn zeroize(&mut self) {
        self.data.zeroize();
    }
}

impl Drop for SumKesSigningKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl PartialEq for SumKesSigningKey {
    fn eq(&self, other: &Self) -> bool {
        self.depth == other.depth && self.data.ct_eq(&other.data).into()
    }
}

impl Eq for SumKesSigningKey {}

impl fmt::Debug for SumKesSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SumKesSigningKey")
            .field("depth", &self.depth)
            .field("data", &"[REDACTED]")
            .finish()
    }
}

impl SumKesSigningKey {
    /// Returns the depth (number of composition levels).
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// Total number of supported periods: `2^depth`.
    pub fn total_periods(&self) -> u32 {
        1u32 << self.depth
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Seed expansion
// ───────────────────────────────────────────────────────────────────────────

/// Expands a 32-byte seed into left and right child seeds.
///
/// Reference: `expandSeed` in upstream — `left = Hash(0x01 || seed)`,
/// `right = Hash(0x02 || seed)`.
fn expand_seed(seed: &[u8; SEED_SIZE]) -> ([u8; SEED_SIZE], [u8; SEED_SIZE]) {
    let mut left_input = Vec::with_capacity(1 + SEED_SIZE);
    left_input.push(0x01);
    left_input.extend_from_slice(seed);
    let left = hash_bytes_256(&left_input);

    let mut right_input = Vec::with_capacity(1 + SEED_SIZE);
    right_input.push(0x02);
    right_input.extend_from_slice(seed);
    let right = hash_bytes_256(&right_input);

    (left.0, right.0)
}

/// Computes the SumKES verification key from a pair of child VKs.
///
/// Reference: `hashPairOfVKeys` — `Blake2b-256(vk_left || vk_right)`.
fn hash_pair_of_vkeys(vk_left: &[u8; VK_SIZE], vk_right: &[u8; VK_SIZE]) -> [u8; VK_SIZE] {
    let mut combined = [0u8; VK_SIZE * 2];
    combined[..VK_SIZE].copy_from_slice(vk_left);
    combined[VK_SIZE..].copy_from_slice(vk_right);
    hash_bytes_256(&combined).0
}

// ───────────────────────────────────────────────────────────────────────────
// Key generation
// ───────────────────────────────────────────────────────────────────────────

/// Expected signing key byte length for a given depth.
fn sk_size(depth: u32) -> usize {
    if depth == 0 {
        SEED_SIZE
    } else {
        // sk_child + seed_right + vk_left + vk_right
        sk_size(depth - 1) + SEED_SIZE + 2 * VK_SIZE
    }
}

/// Generates a SumKES signing key from a 32-byte seed at the given depth.
///
/// Reference: `genKeyKES` / `unsoundPureGenKeyKES` in upstream.
pub fn gen_sum_kes_signing_key(
    seed: &[u8; SEED_SIZE],
    depth: u32,
) -> Result<SumKesSigningKey, CryptoError> {
    if depth == 0 {
        // Base case: just the Ed25519 seed.
        return Ok(SumKesSigningKey {
            depth: 0,
            data: seed.to_vec(),
        });
    }

    let (left_seed, right_seed) = expand_seed(seed);

    // Generate left subtree key.
    let sk_left = gen_sum_kes_signing_key(&left_seed, depth - 1)?;
    let vk_left = derive_sum_kes_vk(&sk_left)?;

    // Generate right subtree — we only need its VK; the signing key is
    // reconstructed lazily from seed_right during key evolution.
    let sk_right = gen_sum_kes_signing_key(&right_seed, depth - 1)?;
    let vk_right = derive_sum_kes_vk(&sk_right)?;
    // sk_right is dropped here (as in upstream's forgetSignKeyKES)

    // Assemble: sk_left_data || seed_right || vk_left || vk_right
    let mut data = Vec::with_capacity(sk_size(depth));
    data.extend_from_slice(&sk_left.data);
    data.extend_from_slice(&right_seed);
    data.extend_from_slice(&vk_left.0);
    data.extend_from_slice(&vk_right.0);

    Ok(SumKesSigningKey { depth, data })
}

/// Derives the SumKES verification key from a signing key.
///
/// Reference: `deriveVerKeyKES` in upstream.
pub fn derive_sum_kes_vk(sk: &SumKesSigningKey) -> Result<SumKesVerificationKey, CryptoError> {
    if sk.depth == 0 {
        // Base case: derive Ed25519 VK from seed.
        let ed_sk = SigningKey::from_bytes(
            sk.data[..SEED_SIZE]
                .try_into()
                .map_err(|_| CryptoError::InvalidKesKeyMaterialLength(sk.data.len()))?,
        );
        let vk = ed_sk.verification_key()?;
        return Ok(SumKesVerificationKey(vk.to_bytes()));
    }

    // Extract vk_left and vk_right from the tail of the signing key data.
    let vk_left = extract_vk_left_from_sk(sk);
    let vk_right = extract_vk_right_from_sk(sk);
    Ok(SumKesVerificationKey(hash_pair_of_vkeys(
        &vk_left, &vk_right,
    )))
}

// ───────────────────────────────────────────────────────────────────────────
// Signing
// ───────────────────────────────────────────────────────────────────────────

/// Signs a message with the SumKES signing key at the given period.
///
/// Reference: `signKES` / `unsoundPureSignKES` in upstream.
pub fn sign_sum_kes(
    sk: &SumKesSigningKey,
    period: u32,
    message: &[u8],
) -> Result<SumKesSignature, CryptoError> {
    let total = sk.total_periods();
    if period >= total {
        return Err(CryptoError::InvalidKesPeriod(period));
    }

    if sk.depth == 0 {
        // Base case: sign with Ed25519.
        let ed_sk = KesSigningKey::from_bytes(
            sk.data[..SEED_SIZE]
                .try_into()
                .map_err(|_| CryptoError::InvalidKesKeyMaterialLength(sk.data.len()))?,
        );
        let sig = ed_sk.sign(KesPeriod(0), message)?;
        return Ok(SumKesSignature {
            depth: 0,
            data: sig.to_bytes().to_vec(),
        });
    }

    let half = total / 2;
    let child_sk = extract_child_sk(sk);
    let vk_left = extract_vk_left_from_sk(sk);
    let vk_right = extract_vk_right_from_sk(sk);

    // Recurse into left or right subtree.
    let child_period = if period < half { period } else { period - half };
    let child_sig = sign_sum_kes(&child_sk, child_period, message)?;

    // Build the SumKES signature: child_sig || vk_left || vk_right
    let mut data = Vec::with_capacity(SumKesSignature::expected_size(sk.depth));
    data.extend_from_slice(child_sig.to_bytes());
    data.extend_from_slice(&vk_left);
    data.extend_from_slice(&vk_right);

    Ok(SumKesSignature {
        depth: sk.depth,
        data,
    })
}

// ───────────────────────────────────────────────────────────────────────────
// Verification
// ───────────────────────────────────────────────────────────────────────────

/// Verifies a SumKES signature against a verification key.
///
/// Reference: `verifyKES` in upstream.
pub fn verify_sum_kes(
    vk: &SumKesVerificationKey,
    period: u32,
    message: &[u8],
    sig: &SumKesSignature,
) -> Result<(), CryptoError> {
    if sig.depth == 0 {
        // Base case: verify Ed25519 signature.
        let ed_vk = KesVerificationKey::from_bytes(vk.0);
        let ed_sig = KesSignature::from_bytes(sig.base_signature());
        return ed_vk.verify(KesPeriod(0), message, &ed_sig);
    }

    let total_periods = 1u32 << sig.depth;
    if period >= total_periods {
        return Err(CryptoError::InvalidKesPeriod(period));
    }

    // Extract the sibling VKs from the outermost level of the signature.
    // The outermost level is stored LAST in our encoding (depth-1 index
    // matches the outermost composition layer).
    let vk_left = sig.vk_left(sig.depth - 1);
    let vk_right = sig.vk_right(sig.depth - 1);

    // Verify that hash(vk_left, vk_right) matches the provided VK.
    let expected_vk = hash_pair_of_vkeys(&vk_left, &vk_right);
    if vk.0 != expected_vk {
        return Err(CryptoError::KesVerificationKeyMismatch);
    }

    // Recurse into the appropriate subtree.
    let half = total_periods / 2;
    let (child_vk_bytes, child_period) = if period < half {
        (vk_left, period)
    } else {
        (vk_right, period - half)
    };

    // Build the child signature (strip the outermost VK pair).
    let child_sig_data = &sig.data[..SumKesSignature::expected_size(sig.depth - 1)];
    let child_sig = SumKesSignature {
        depth: sig.depth - 1,
        data: child_sig_data.to_vec(),
    };

    let child_vk = SumKesVerificationKey(child_vk_bytes);
    verify_sum_kes(&child_vk, child_period, message, &child_sig)
}

// ───────────────────────────────────────────────────────────────────────────
// Key Update (Evolution)
// ───────────────────────────────────────────────────────────────────────────

/// Evolves a SumKES signing key from the current period to the next.
///
/// Returns `None` if the key is already at its final period.
///
/// Reference: `updateKES` / `unsoundPureUpdateKES` in upstream.
pub fn update_sum_kes(
    sk: &SumKesSigningKey,
    current_period: u32,
) -> Result<Option<SumKesSigningKey>, CryptoError> {
    let total = sk.total_periods();
    if current_period + 1 >= total {
        // At the last period — cannot evolve further.
        return Ok(None);
    }

    if sk.depth == 0 {
        // Depth 0 only has period 0 — should not reach here due to check above.
        return Ok(None);
    }

    let half = total / 2;

    match (current_period + 1).cmp(&half) {
        std::cmp::Ordering::Less => {
            // Still in the left subtree — update the left child.
            let child_sk = extract_child_sk(sk);
            let child_updated = update_sum_kes(&child_sk, current_period)?;
            match child_updated {
                Some(new_child) => {
                    let seed_right = extract_seed_right(sk);
                    let vk_left = extract_vk_left_from_sk(sk);
                    let vk_right = extract_vk_right_from_sk(sk);

                    let mut data = Vec::with_capacity(sk_size(sk.depth));
                    data.extend_from_slice(&new_child.data);
                    data.extend_from_slice(&seed_right);
                    data.extend_from_slice(&vk_left);
                    data.extend_from_slice(&vk_right);

                    Ok(Some(SumKesSigningKey {
                        depth: sk.depth,
                        data,
                    }))
                }
                None => Ok(None),
            }
        }
        std::cmp::Ordering::Equal => {
            // Transition from left to right — generate right subtree from seed.
            let seed_right = extract_seed_right(sk);
            let seed_right_arr: [u8; SEED_SIZE] = seed_right
                .try_into()
                .map_err(|_| CryptoError::InvalidKesKeyMaterialLength(0))?;
            let new_child = gen_sum_kes_signing_key(&seed_right_arr, sk.depth - 1)?;

            let vk_left = extract_vk_left_from_sk(sk);
            let vk_right = extract_vk_right_from_sk(sk);

            // Zero out the saved seed (it is no longer needed).
            let zeroed_seed = [0u8; SEED_SIZE];
            let mut data = Vec::with_capacity(sk_size(sk.depth));
            data.extend_from_slice(&new_child.data);
            data.extend_from_slice(&zeroed_seed);
            data.extend_from_slice(&vk_left);
            data.extend_from_slice(&vk_right);

            Ok(Some(SumKesSigningKey {
                depth: sk.depth,
                data,
            }))
        }
        std::cmp::Ordering::Greater => {
            // In the right subtree — update the right child.
            let child_sk = extract_child_sk(sk);
            let child_updated = update_sum_kes(&child_sk, current_period - half)?;
            match child_updated {
                Some(new_child) => {
                    let zeroed_seed = [0u8; SEED_SIZE]; // seed already consumed
                    let vk_left = extract_vk_left_from_sk(sk);
                    let vk_right = extract_vk_right_from_sk(sk);

                    let mut data = Vec::with_capacity(sk_size(sk.depth));
                    data.extend_from_slice(&new_child.data);
                    data.extend_from_slice(&zeroed_seed);
                    data.extend_from_slice(&vk_left);
                    data.extend_from_slice(&vk_right);

                    Ok(Some(SumKesSigningKey {
                        depth: sk.depth,
                        data,
                    }))
                }
                None => Ok(None),
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Internal helpers for signing key field extraction
// ───────────────────────────────────────────────────────────────────────────

/// Extracts the child signing key from a depth > 0 signing key.
fn extract_child_sk(sk: &SumKesSigningKey) -> SumKesSigningKey {
    let child_size = sk_size(sk.depth - 1);
    SumKesSigningKey {
        depth: sk.depth - 1,
        data: sk.data[..child_size].to_vec(),
    }
}

/// Extracts the right-child seed from a depth > 0 signing key.
fn extract_seed_right(sk: &SumKesSigningKey) -> Vec<u8> {
    let child_size = sk_size(sk.depth - 1);
    sk.data[child_size..child_size + SEED_SIZE].to_vec()
}

/// Extracts the left sibling VK from a depth > 0 signing key.
fn extract_vk_left_from_sk(sk: &SumKesSigningKey) -> [u8; VK_SIZE] {
    let off = sk_size(sk.depth - 1) + SEED_SIZE;
    sk.data[off..off + VK_SIZE]
        .try_into()
        .expect("signing key should contain left VK")
}

/// Extracts the right sibling VK from a depth > 0 signing key.
fn extract_vk_right_from_sk(sk: &SumKesSigningKey) -> [u8; VK_SIZE] {
    let off = sk_size(sk.depth - 1) + SEED_SIZE + VK_SIZE;
    sk.data[off..off + VK_SIZE]
        .try_into()
        .expect("signing key should contain right VK")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_seed(byte: u8) -> [u8; SEED_SIZE] {
        [byte; SEED_SIZE]
    }

    // ── SumKesVerificationKey ────────────────────────────────────────────

    #[test]
    fn vk_from_bytes_roundtrip() {
        let bytes = [0xAB; VK_SIZE];
        let vk = SumKesVerificationKey::from_bytes(bytes);
        assert_eq!(vk.to_bytes(), bytes);
    }

    #[test]
    fn vk_debug_format() {
        let vk = SumKesVerificationKey::from_bytes([0x12; VK_SIZE]);
        let dbg = format!("{:?}", vk);
        assert!(dbg.starts_with("SumKesVK("));
    }

    // ── SumKesSignature ──────────────────────────────────────────────────

    #[test]
    fn signature_expected_size_depth_0() {
        assert_eq!(SumKesSignature::expected_size(0), 64);
    }

    #[test]
    fn signature_expected_size_depth_1() {
        assert_eq!(SumKesSignature::expected_size(1), 64 + 64);
    }

    #[test]
    fn signature_expected_size_depth_6() {
        assert_eq!(SumKesSignature::expected_size(6), 64 + 6 * 64);
    }

    #[test]
    fn signature_from_bytes_correct_len() {
        let data = vec![0u8; SumKesSignature::expected_size(2)];
        let sig = SumKesSignature::from_bytes(2, &data).unwrap();
        assert_eq!(sig.depth(), 2);
    }

    #[test]
    fn signature_from_bytes_wrong_len() {
        let data = vec![0u8; 10];
        let result = SumKesSignature::from_bytes(1, &data);
        assert!(result.is_err());
    }

    // ── SumKesSigningKey ─────────────────────────────────────────────────

    #[test]
    fn signing_key_debug_redacted() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 0).unwrap();
        let dbg = format!("{:?}", sk);
        assert!(dbg.contains("REDACTED"));
    }

    #[test]
    fn signing_key_total_periods() {
        for depth in 0..=6 {
            let sk = gen_sum_kes_signing_key(&test_seed(1), depth).unwrap();
            assert_eq!(sk.total_periods(), 1u32 << depth);
        }
    }

    #[test]
    fn signing_key_constant_time_eq() {
        let sk1 = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        let sk2 = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        let sk3 = gen_sum_kes_signing_key(&test_seed(2), 1).unwrap();
        assert_eq!(sk1, sk2);
        assert_ne!(sk1, sk3);
    }

    // ── Seed expansion ───────────────────────────────────────────────────

    #[test]
    fn expand_seed_produces_different_halves() {
        let (left, right) = expand_seed(&test_seed(0));
        assert_ne!(left, right);
    }

    #[test]
    fn expand_seed_is_deterministic() {
        let (l1, r1) = expand_seed(&test_seed(42));
        let (l2, r2) = expand_seed(&test_seed(42));
        assert_eq!(l1, l2);
        assert_eq!(r1, r2);
    }

    #[test]
    fn expand_seed_different_inputs_differ() {
        let (l1, _) = expand_seed(&test_seed(1));
        let (l2, _) = expand_seed(&test_seed(2));
        assert_ne!(l1, l2);
    }

    // ── hash_pair_of_vkeys ───────────────────────────────────────────────

    #[test]
    fn hash_pair_of_vkeys_deterministic() {
        let a = [1u8; VK_SIZE];
        let b = [2u8; VK_SIZE];
        assert_eq!(hash_pair_of_vkeys(&a, &b), hash_pair_of_vkeys(&a, &b));
    }

    #[test]
    fn hash_pair_of_vkeys_order_matters() {
        let a = [1u8; VK_SIZE];
        let b = [2u8; VK_SIZE];
        assert_ne!(hash_pair_of_vkeys(&a, &b), hash_pair_of_vkeys(&b, &a));
    }

    // ── Key generation ───────────────────────────────────────────────────

    #[test]
    fn gen_depth_0() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 0).unwrap();
        assert_eq!(sk.depth(), 0);
        assert_eq!(sk.data.len(), SEED_SIZE);
    }

    #[test]
    fn gen_depth_1() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        assert_eq!(sk.depth(), 1);
        assert_eq!(sk.data.len(), sk_size(1));
    }

    #[test]
    fn gen_depth_3() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 3).unwrap();
        assert_eq!(sk.depth(), 3);
        assert_eq!(sk.data.len(), sk_size(3));
        assert_eq!(sk.total_periods(), 8);
    }

    // ── VK derivation ────────────────────────────────────────────────────

    #[test]
    fn derive_vk_depth_0_matches_ed25519() {
        let seed = test_seed(7);
        let sk = gen_sum_kes_signing_key(&seed, 0).unwrap();
        let sum_vk = derive_sum_kes_vk(&sk).unwrap();
        let ed_vk = crate::ed25519::SigningKey::from_bytes(seed)
            .verification_key()
            .unwrap();
        assert_eq!(sum_vk.0, ed_vk.to_bytes());
    }

    #[test]
    fn derive_vk_depth_1_is_hash_of_children() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();
        // VK should be Blake2b-256(vk_left || vk_right).
        let vk_left = extract_vk_left_from_sk(&sk);
        let vk_right = extract_vk_right_from_sk(&sk);
        assert_eq!(vk.0, hash_pair_of_vkeys(&vk_left, &vk_right));
    }

    #[test]
    fn derive_vk_deterministic() {
        let sk = gen_sum_kes_signing_key(&test_seed(5), 2).unwrap();
        let vk1 = derive_sum_kes_vk(&sk).unwrap();
        let vk2 = derive_sum_kes_vk(&sk).unwrap();
        assert_eq!(vk1, vk2);
    }

    // ── sign + verify depth 0 ────────────────────────────────────────────

    #[test]
    fn sign_verify_depth_0_period_0() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 0).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();
        let sig = sign_sum_kes(&sk, 0, b"hello").unwrap();
        verify_sum_kes(&vk, 0, b"hello", &sig).expect("depth 0 period 0 should verify");
    }

    #[test]
    fn sign_depth_0_period_1_rejected() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 0).unwrap();
        assert_eq!(
            sign_sum_kes(&sk, 1, b"msg"),
            Err(CryptoError::InvalidKesPeriod(1))
        );
    }

    // ── sign + verify depth 1 ────────────────────────────────────────────

    #[test]
    fn sign_verify_depth_1_both_periods() {
        let sk = gen_sum_kes_signing_key(&test_seed(3), 1).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();

        // Period 0 with the initial key.
        let sig0 = sign_sum_kes(&sk, 0, b"test").unwrap();
        verify_sum_kes(&vk, 0, b"test", &sig0).expect("depth 1 period 0 should verify");

        // Evolve to period 1.
        let sk1 = update_sum_kes(&sk, 0).unwrap().expect("should evolve");
        let sig1 = sign_sum_kes(&sk1, 1, b"test").unwrap();
        verify_sum_kes(&vk, 1, b"test", &sig1).expect("depth 1 period 1 should verify");
    }

    #[test]
    fn depth_1_period_2_rejected() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        assert_eq!(
            sign_sum_kes(&sk, 2, b"msg"),
            Err(CryptoError::InvalidKesPeriod(2))
        );
    }

    // ── sign + verify depth 2 ────────────────────────────────────────────

    #[test]
    fn sign_verify_depth_2_all_periods() {
        let sk = gen_sum_kes_signing_key(&test_seed(9), 2).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();

        let mut current_sk = sk;
        for period in 0..4 {
            let sig = sign_sum_kes(&current_sk, period, b"depth2").unwrap();
            verify_sum_kes(&vk, period, b"depth2", &sig)
                .unwrap_or_else(|e| panic!("depth 2 period {period} should verify: {e}"));
            if period < 3 {
                current_sk = update_sum_kes(&current_sk, period)
                    .unwrap()
                    .unwrap_or_else(|| panic!("should evolve from period {period}"));
            }
        }
    }

    // ── sign + verify depth 3 ────────────────────────────────────────────

    #[test]
    fn sign_verify_depth_3_all_periods() {
        let sk = gen_sum_kes_signing_key(&test_seed(0xAA), 3).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();

        let mut current_sk = sk;
        for period in 0..8 {
            let sig = sign_sum_kes(&current_sk, period, b"depth3msg").unwrap();
            verify_sum_kes(&vk, period, b"depth3msg", &sig)
                .unwrap_or_else(|e| panic!("depth 3 period {period} should verify: {e}"));
            if period < 7 {
                current_sk = update_sum_kes(&current_sk, period)
                    .unwrap()
                    .unwrap_or_else(|| panic!("should evolve from period {period}"));
            }
        }
    }

    // ── Signature tamper detection ───────────────────────────────────────

    #[test]
    fn tampered_signature_rejected() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();
        let sig = sign_sum_kes(&sk, 0, b"msg").unwrap();
        let mut bad_data = sig.data.clone();
        bad_data[0] ^= 0xFF;
        let bad_sig = SumKesSignature {
            depth: sig.depth,
            data: bad_data,
        };
        assert!(verify_sum_kes(&vk, 0, b"msg", &bad_sig).is_err());
    }

    #[test]
    fn wrong_message_rejected() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();
        let sig = sign_sum_kes(&sk, 0, b"good").unwrap();
        assert!(verify_sum_kes(&vk, 0, b"bad", &sig).is_err());
    }

    #[test]
    fn wrong_vk_rejected() {
        let sk1 = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        let vk2 = derive_sum_kes_vk(&gen_sum_kes_signing_key(&test_seed(2), 1).unwrap()).unwrap();
        let sig = sign_sum_kes(&sk1, 0, b"msg").unwrap();
        assert!(verify_sum_kes(&vk2, 0, b"msg", &sig).is_err());
    }

    #[test]
    fn verify_out_of_range_period_rejected() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();
        let sig = sign_sum_kes(&sk, 0, b"msg").unwrap();
        assert_eq!(
            verify_sum_kes(&vk, 2, b"msg", &sig),
            Err(CryptoError::InvalidKesPeriod(2))
        );
    }

    // ── Key update (evolution) ───────────────────────────────────────────

    #[test]
    fn update_depth_0_returns_none() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 0).unwrap();
        assert!(update_sum_kes(&sk, 0).unwrap().is_none());
    }

    #[test]
    fn update_depth_1_from_period_0() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();

        let sk1 = update_sum_kes(&sk, 0)
            .unwrap()
            .expect("should evolve to period 1");
        let sig = sign_sum_kes(&sk1, 1, b"evolved").unwrap();
        verify_sum_kes(&vk, 1, b"evolved", &sig).expect("evolved key should sign period 1 validly");
    }

    #[test]
    fn update_depth_1_at_last_period_returns_none() {
        let sk = gen_sum_kes_signing_key(&test_seed(1), 1).unwrap();
        let sk1 = update_sum_kes(&sk, 0).unwrap().unwrap();
        assert!(update_sum_kes(&sk1, 1).unwrap().is_none());
    }

    #[test]
    fn update_depth_2_full_evolution() {
        let sk = gen_sum_kes_signing_key(&test_seed(5), 2).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();

        // Evolve through all 4 periods: 0 → 1 → 2 → 3
        let mut current_sk = sk;
        for period in 0..4 {
            let sig = sign_sum_kes(&current_sk, period, b"evo").unwrap();
            verify_sum_kes(&vk, period, b"evo", &sig)
                .unwrap_or_else(|e| panic!("period {period} should verify: {e}"));

            if period < 3 {
                current_sk = update_sum_kes(&current_sk, period)
                    .unwrap()
                    .unwrap_or_else(|| panic!("should evolve from period {period}"));
            } else {
                assert!(update_sum_kes(&current_sk, period).unwrap().is_none());
            }
        }
    }

    #[test]
    fn update_depth_3_full_evolution() {
        let sk = gen_sum_kes_signing_key(&test_seed(0xBB), 3).unwrap();
        let vk = derive_sum_kes_vk(&sk).unwrap();

        let mut current_sk = sk;
        for period in 0..8 {
            let sig = sign_sum_kes(&current_sk, period, b"d3evo").unwrap();
            verify_sum_kes(&vk, period, b"d3evo", &sig)
                .unwrap_or_else(|e| panic!("period {period} should verify: {e}"));

            if period < 7 {
                current_sk = update_sum_kes(&current_sk, period)
                    .unwrap()
                    .unwrap_or_else(|| panic!("should evolve from period {period}"));
            } else {
                assert!(update_sum_kes(&current_sk, period).unwrap().is_none());
            }
        }
    }

    #[test]
    fn evolved_key_preserves_vk() {
        let sk = gen_sum_kes_signing_key(&test_seed(3), 2).unwrap();
        let vk_before = derive_sum_kes_vk(&sk).unwrap();

        let sk1 = update_sum_kes(&sk, 0).unwrap().unwrap();
        let vk_after = derive_sum_kes_vk(&sk1).unwrap();
        assert_eq!(vk_before, vk_after, "VK should be stable across evolution");
    }

    // ── sk_size helper ───────────────────────────────────────────────────

    #[test]
    fn sk_size_depth_0() {
        assert_eq!(sk_size(0), SEED_SIZE);
    }

    #[test]
    fn sk_size_depth_1() {
        assert_eq!(sk_size(1), SEED_SIZE + SEED_SIZE + 2 * VK_SIZE);
    }

    #[test]
    fn sk_size_matches_generated_key_data() {
        for depth in 0..=4 {
            let sk = gen_sum_kes_signing_key(&test_seed(1), depth).unwrap();
            assert_eq!(
                sk.data.len(),
                sk_size(depth),
                "data length mismatch at depth {depth}"
            );
        }
    }
}
