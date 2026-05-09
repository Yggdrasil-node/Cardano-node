//! Praos / TPraos protocol implementation.
//!
//! Top-level entry points for slot leader election:
//!
//! - [`check_is_leader`] — full pipeline: VRF proof + threshold check.
//! - [`check_leader_value`] — threshold check on a known VRF output.
//! - [`verify_leader_proof`] / [`verify_leader_proof_output`] — verifier-side
//!   leader VRF proof check.
//! - [`verify_nonce_proof`] — verifier-side nonce VRF proof check.
//!
//! Sub-modules:
//!
//! - [`vrf`] — VRF input construction (TPraos `mkSeed` / Praos `mkInputVRF`).
//! - [`common`] — `ActiveSlotCoeff` + Taylor-series math primitives for
//!   deterministic leader-value comparisons.
//!
//! Mirrors upstream `Ouroboros.Consensus.Protocol.Praos` (entry points) +
//! `.../Praos/VRF.hs` (VRF input + leader-value math) +
//! `Cardano.Ledger.BaseTypes::ActiveSlotCoeff` (preprocessed `f`).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell that
//! splits upstream `Ouroboros.Consensus.Protocol.Praos.hs` (top-
//! level Praos protocol module) into focused sub-modules:
//! `vrf.rs` (VRF input construction — TPraos `mkSeed` + Praos
//! `mkInputVRF`) and `common.rs` (active-slot-coefficient + math
//! primitives). Upstream keeps everything in `Praos.hs` and
//! `Praos/Common.hs` (a separate file); Yggdrasil's `praos.rs`
//! is the leader-check entry-point shell.

pub mod common;
pub mod vrf;

pub use common::{ActiveSlotCoeff, leadership_threshold};
pub use vrf::{VrfMode, VrfUsage, praos_vrf_input, tpraos_vrf_seed, vrf_input};

use num_bigint::BigUint;
use num_traits::One;
use yggdrasil_crypto::blake2b::hash_bytes_256;
use yggdrasil_crypto::vrf::{VrfOutput, VrfSecretKey, VrfVerificationKey};
use yggdrasil_ledger::{Nonce, SlotNo};

use crate::ConsensusError;
use common::taylor_exp_cmp;
#[cfg(test)]
use vrf::{raw_vrf_input_bytes, tpraos_seed_tag_hash};

// ---------------------------------------------------------------------------
// Leader check — deterministic integer arithmetic
// ---------------------------------------------------------------------------

/// Determines whether a VRF output qualifies the holder as slot leader
/// given their relative stake and the active slot coefficient.
///
/// The check is fully deterministic and uses a Taylor-expansion comparison
/// on `exp(−σ × activeSlotLog)` to avoid floating-point arithmetic.
///
/// For **TPraos** (Shelley–Alonzo): the raw 64-byte VRF output is interpreted
/// as a 512-bit big-endian unsigned integer.  `certNatMax = 2^512`.
/// Reference: `checkLeaderValue` in `Cardano.Protocol.TPraos.BHeader`.
///
/// For **Praos** (Babbage/Conway): VRF range extension is applied —
/// `Blake2b-256("L" || output)` → 32 bytes → natural.  `certNatMax = 2^256`.
/// Reference: `vrfLeaderValue` + `checkLeaderNatValue` in
/// `Ouroboros.Consensus.Protocol.Praos.VRF`.
///
/// `sigma_num` / `sigma_den` encode the pool's relative stake σ as a
/// rational.
pub fn check_leader_value(
    vrf_output: &VrfOutput,
    sigma_num: u64,
    sigma_den: u64,
    active_slot_coeff: &ActiveSlotCoeff,
    mode: VrfMode,
) -> Result<bool, ConsensusError> {
    if sigma_den == 0 {
        return Err(ConsensusError::InvalidActiveSlotCoeff);
    }
    // σ = 0 → never a leader.
    if sigma_num == 0 {
        return Ok(false);
    }

    let (cert_nat, cert_nat_max) = match mode {
        VrfMode::TPraos => {
            // Raw 512-bit output, certNatMax = 2^512.
            let max: BigUint = BigUint::one() << 512u32;
            let nat = BigUint::from_bytes_be(vrf_output.to_bytes().as_ref());
            (nat, max)
        }
        VrfMode::Praos => {
            // Range-extended: Blake2b-256("L" || output) → 32 bytes → natural.
            // certNatMax = 2^256.
            let output_bytes = vrf_output.to_bytes();
            let mut prefixed = Vec::with_capacity(1 + output_bytes.len());
            prefixed.push(b'L');
            prefixed.extend_from_slice(&output_bytes);
            let leader_hash = hash_bytes_256(&prefixed).0;
            let max: BigUint = BigUint::one() << 256u32;
            let nat = BigUint::from_bytes_be(&leader_hash);
            (nat, max)
        }
    };

    if cert_nat >= cert_nat_max {
        return Ok(false);
    }
    let target = &cert_nat_max - &cert_nat;

    // We need: target > certNatMax × (1−f)^σ
    // ⟺ target > certNatMax × exp(−σ × activeSlotLog)
    // where activeSlotLog = −ln(1−f) > 0.
    let x_num = BigUint::from(sigma_num) * &active_slot_coeff.log_num;
    let x_den = BigUint::from(sigma_den) * &active_slot_coeff.log_den;

    taylor_exp_cmp(&cert_nat_max, &target, &x_num, &x_den)
}

// ---------------------------------------------------------------------------
// Full leader-election helper
// ---------------------------------------------------------------------------

/// Evaluates whether the given VRF secret key wins the slot lottery.
///
/// Performs the full pipeline:
/// 1. Construct the VRF input from `slot` and `epoch_nonce` using `mode`.
/// 2. Produce a VRF proof using the secret key.
/// 3. Check the output against the leader threshold.
///
/// Returns `Ok(Some((output, proof_bytes)))` if the key is elected leader,
/// `Ok(None)` otherwise.
///
/// Reference: `checkIsLeader` in
/// `Ouroboros.Consensus.Protocol.Praos`.
pub fn check_is_leader(
    sk: &VrfSecretKey,
    slot: SlotNo,
    epoch_nonce: Nonce,
    sigma_num: u64,
    sigma_den: u64,
    active_slot_coeff: &ActiveSlotCoeff,
    mode: VrfMode,
) -> Result<Option<(VrfOutput, Vec<u8>)>, ConsensusError> {
    let input = vrf_input(slot, epoch_nonce, mode, VrfUsage::Leader);
    let (output, proof) = sk
        .prove(&input)
        .map_err(|_| ConsensusError::InvalidVrfProof)?;
    let is_leader = check_leader_value(&output, sigma_num, sigma_den, active_slot_coeff, mode)?;
    if is_leader {
        Ok(Some((output, proof.to_bytes().to_vec())))
    } else {
        Ok(None)
    }
}

/// Verifies a claimed leader proof against a public VRF key and the
/// election parameters.
///
/// Returns `Ok(true)` if the proof is valid *and* the output meets the
/// leadership threshold, `Ok(false)` if the proof is valid but the
/// output does not meet the threshold, and `Err` on VRF verification
/// failure.
///
/// Reference: `validateVRFSignature` in
/// `Ouroboros.Consensus.Protocol.Praos`.
//
// Argument count mirrors upstream `validateVRFSignature`, which threads
// the verification key, slot, epoch nonce, proof bytes, stake fraction
// (numerator/denominator), active-slot coefficient, and VRF mode through
// a single call site. Bagging into a struct here would diverge from the
// upstream signature without simplifying the call sites.
#[allow(clippy::too_many_arguments)]
pub fn verify_leader_proof(
    vk: &VrfVerificationKey,
    slot: SlotNo,
    epoch_nonce: Nonce,
    proof_bytes: &[u8],
    sigma_num: u64,
    sigma_den: u64,
    active_slot_coeff: &ActiveSlotCoeff,
    mode: VrfMode,
) -> Result<bool, ConsensusError> {
    let output = verify_leader_proof_output(vk, slot, epoch_nonce, proof_bytes, mode)?;

    check_leader_value(&output, sigma_num, sigma_den, active_slot_coeff, mode)
}

/// Verifies a claimed leader VRF proof and returns its output without applying
/// the stake-threshold leader check.
///
/// This is needed for TPraos overlay slots: upstream `pbftVrfChecks` verifies
/// `bheaderL` with `mkSeed seedL slot eta0` but does not call
/// `checkLeaderValue`, because the overlay schedule already selected the
/// genesis delegate for the slot.
///
/// Reference: `Cardano.Protocol.TPraos.Rules.Overlay` `pbftVrfChecks`.
pub fn verify_leader_proof_output(
    vk: &VrfVerificationKey,
    slot: SlotNo,
    epoch_nonce: Nonce,
    proof_bytes: &[u8],
    mode: VrfMode,
) -> Result<VrfOutput, ConsensusError> {
    use yggdrasil_crypto::vrf::{VRF_PROOF_SIZE, VrfProof};

    let proof_arr: [u8; VRF_PROOF_SIZE] = proof_bytes
        .try_into()
        .map_err(|_| ConsensusError::InvalidVrfProof)?;
    let proof = VrfProof::from_bytes(proof_arr);

    let input = vrf_input(slot, epoch_nonce, mode, VrfUsage::Leader);
    let output = vk
        .verify(&input, &proof)
        .map_err(|_| ConsensusError::InvalidVrfProof)?;

    Ok(output)
}

/// Verifies a TPraos nonce VRF proof (`bheaderEta`) for a Shelley-through-Alonzo
/// block header.
///
/// For TPraos-era blocks, the header carries a separate nonce VRF proof computed
/// with `mkSeed seedEta slot eta0`. This function cryptographically verifies that
/// proof against the block producer's VRF verification key.
///
/// For Praos-era blocks (Babbage/Conway), there is no separate nonce proof — the
/// single unified VRF result covers both leader election and nonce contribution.
/// Callers should skip this function for Praos blocks.
///
/// Reference: `vrfChecks` in `Cardano.Protocol.TPraos.OCert` /
/// `Cardano.Ledger.Shelley.Rules.Overlay` — verifies `bheaderEta` with
/// `mkSeed seedEta slot eta0`.
pub fn verify_nonce_proof(
    vk: &VrfVerificationKey,
    slot: SlotNo,
    epoch_nonce: Nonce,
    nonce_proof_bytes: &[u8],
) -> Result<(), ConsensusError> {
    use yggdrasil_crypto::vrf::{VRF_PROOF_SIZE, VrfProof};

    let proof_arr: [u8; VRF_PROOF_SIZE] = nonce_proof_bytes
        .try_into()
        .map_err(|_| ConsensusError::InvalidVrfProof)?;
    let proof = VrfProof::from_bytes(proof_arr);

    let input = tpraos_vrf_seed(slot, epoch_nonce, VrfUsage::Nonce);
    vk.verify(&input, &proof)
        .map_err(|_| ConsensusError::InvalidVrfProof)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_slot_coeff_from_rational_mainnet() {
        let asc = ActiveSlotCoeff::from_rational(1, 20).expect("1/20 is valid");
        assert!((asc.to_f64() - 0.05).abs() < 1e-10);
    }

    #[test]
    fn active_slot_coeff_new_from_f64() {
        let asc = ActiveSlotCoeff::new(0.05).expect("0.05 is valid");
        assert!((asc.to_f64() - 0.05).abs() < 1e-10);
    }

    #[test]
    fn active_slot_coeff_rejects_zero() {
        assert!(ActiveSlotCoeff::from_rational(0, 1).is_err());
        assert!(ActiveSlotCoeff::new(0.0).is_err());
    }

    #[test]
    fn active_slot_coeff_rejects_greater_than_one() {
        assert!(ActiveSlotCoeff::from_rational(2, 1).is_err());
        assert!(ActiveSlotCoeff::new(1.5).is_err());
    }

    #[test]
    fn active_slot_coeff_accepts_one() {
        assert!(ActiveSlotCoeff::from_rational(1, 1).is_ok());
        assert!(ActiveSlotCoeff::new(1.0).is_ok());
    }

    #[test]
    fn leader_check_all_zeros_is_leader() {
        let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
        let output = VrfOutput::from_bytes([0u8; 64]);
        // Full stake: sigma = 1/1.  TPraos mode (raw 512-bit check).
        let result = check_leader_value(&output, 1, 1, &asc, VrfMode::TPraos).expect("valid");
        assert!(
            result,
            "all-zeros VRF output should always qualify as leader"
        );
    }

    #[test]
    fn leader_check_all_ones_not_leader_small_stake() {
        let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
        let output = VrfOutput::from_bytes([0xFF; 64]);
        // Small stake: sigma = 1/100.
        let result = check_leader_value(&output, 1, 100, &asc, VrfMode::TPraos).expect("valid");
        assert!(
            !result,
            "all-ones VRF output should exceed threshold for small stake"
        );
    }

    #[test]
    fn leader_check_zero_stake_never_leader() {
        let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
        let output = VrfOutput::from_bytes([0u8; 64]);
        let result = check_leader_value(&output, 0, 1, &asc, VrfMode::TPraos).expect("valid");
        assert!(!result, "zero stake should never qualify");
    }

    #[test]
    fn leadership_threshold_display() {
        let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
        let t = leadership_threshold(&asc, 1.0);
        assert!((t - 0.05).abs() < 1e-10);
    }

    // ----- Era-aware VRF input tests -----

    #[test]
    fn praos_vrf_input_is_32_bytes() {
        let nonce = Nonce::Hash([0xAA; 32]);
        let input = praos_vrf_input(SlotNo(42), nonce);
        assert_eq!(
            input.len(),
            32,
            "Praos mkInputVRF produces Blake2b-256 hash"
        );
    }

    #[test]
    fn tpraos_vrf_seed_is_32_bytes() {
        let nonce = Nonce::Hash([0xAA; 32]);
        let seed = tpraos_vrf_seed(SlotNo(42), nonce, VrfUsage::Leader);
        assert_eq!(seed.len(), 32, "TPraos mkSeed produces 32-byte XOR'd hash");
    }

    #[test]
    fn tpraos_vrf_seed_uses_word64be_seed_tags() {
        let nonce = Nonce::Hash([0xAA; 32]);
        let nonce_seed = tpraos_vrf_seed(SlotNo(42), nonce, VrfUsage::Nonce);
        let leader_seed = tpraos_vrf_seed(SlotNo(42), nonce, VrfUsage::Leader);

        assert_eq!(
            nonce_seed,
            [
                0xfe, 0x84, 0x09, 0x6f, 0x3d, 0xb6, 0xa6, 0x88, 0x61, 0xb6, 0x55, 0x51, 0x9a, 0xdb,
                0x10, 0xa7, 0x74, 0x01, 0xbf, 0xb2, 0x55, 0xa0, 0x91, 0x9a, 0xb9, 0xe5, 0x32, 0x02,
                0xae, 0xcf, 0x1a, 0xba,
            ]
            .to_vec(),
            "seedEta must match upstream mkNonceFromNumber 0"
        );
        assert_eq!(
            leader_seed,
            [
                0x6d, 0xbd, 0x79, 0x1c, 0xa6, 0x0a, 0x1f, 0xa8, 0x93, 0x9d, 0x61, 0xe6, 0xc2, 0xce,
                0x1b, 0x93, 0xf1, 0xe2, 0x73, 0x53, 0x8e, 0x3f, 0x08, 0x51, 0xb1, 0x13, 0x2a, 0x2f,
                0xe0, 0x15, 0xd1, 0x86,
            ]
            .to_vec(),
            "seedL must match upstream mkNonceFromNumber 1"
        );
    }

    #[test]
    fn tpraos_leader_and_nonce_seeds_differ() {
        let nonce = Nonce::Hash([0xBB; 32]);
        let leader_seed = tpraos_vrf_seed(SlotNo(100), nonce, VrfUsage::Leader);
        let nonce_seed = tpraos_vrf_seed(SlotNo(100), nonce, VrfUsage::Nonce);
        assert_ne!(leader_seed, nonce_seed, "seedL and seedEta must differ");
    }

    #[test]
    fn praos_and_tpraos_inputs_differ() {
        let nonce = Nonce::Hash([0xCC; 32]);
        let praos = praos_vrf_input(SlotNo(50), nonce);
        let tpraos_leader = tpraos_vrf_seed(SlotNo(50), nonce, VrfUsage::Leader);
        let tpraos_nonce = tpraos_vrf_seed(SlotNo(50), nonce, VrfUsage::Nonce);
        assert_ne!(
            praos, tpraos_leader,
            "Praos mkInputVRF != TPraos mkSeed seedL"
        );
        assert_ne!(
            praos, tpraos_nonce,
            "Praos mkInputVRF != TPraos mkSeed seedEta"
        );
    }

    #[test]
    fn vrf_input_dispatch_praos() {
        let nonce = Nonce::Hash([0xDD; 32]);
        let direct = praos_vrf_input(SlotNo(7), nonce);
        let dispatched = vrf_input(SlotNo(7), nonce, VrfMode::Praos, VrfUsage::Leader);
        assert_eq!(direct, dispatched);
        // Praos ignores usage — nonce variant should also match.
        let dispatched_n = vrf_input(SlotNo(7), nonce, VrfMode::Praos, VrfUsage::Nonce);
        assert_eq!(direct, dispatched_n);
    }

    #[test]
    fn vrf_input_dispatch_tpraos() {
        let nonce = Nonce::Hash([0xEE; 32]);
        let direct = tpraos_vrf_seed(SlotNo(7), nonce, VrfUsage::Leader);
        let dispatched = vrf_input(SlotNo(7), nonce, VrfMode::TPraos, VrfUsage::Leader);
        assert_eq!(direct, dispatched);
    }

    #[test]
    fn praos_leader_check_uses_256_bit_range() {
        // Praos range-extends with Blake2b-256("L"||output), so even all-zeros
        // output becomes a non-trivial hash.  Use f=1 (always leader) to check
        // that the Praos path itself works without tripping on the hash value.
        let asc = ActiveSlotCoeff::from_rational(1, 1).expect("valid");
        let output = VrfOutput::from_bytes([0u8; 64]);
        let tpraos_result = check_leader_value(&output, 1, 1, &asc, VrfMode::TPraos).expect("ok");
        let praos_result = check_leader_value(&output, 1, 1, &asc, VrfMode::Praos).expect("ok");
        // With f=1 and full stake, both paths must elect leader.
        assert!(tpraos_result);
        assert!(praos_result);
    }

    #[test]
    fn praos_leader_check_rejects_high_hash_small_stake() {
        // For small stake, both modes should reject high VRF outputs.
        let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
        let output = VrfOutput::from_bytes([0xFF; 64]);
        let tpraos = check_leader_value(&output, 1, 100, &asc, VrfMode::TPraos).expect("ok");
        let praos = check_leader_value(&output, 1, 100, &asc, VrfMode::Praos).expect("ok");
        assert!(!tpraos);
        assert!(!praos);
    }

    #[test]
    #[allow(non_snake_case)] // mirrors upstream `mkInputVRF` from `Ouroboros.Consensus.Protocol.Praos.VRF`
    fn mkInputVRF_matches_upstream_blake2b_hash() {
        // Verify that praos_vrf_input is Blake2b-256 of the raw slot||nonce bytes.
        let slot = SlotNo(42);
        let nonce = Nonce::Hash([0xAA; 32]);
        let raw = raw_vrf_input_bytes(slot, nonce);
        let expected = hash_bytes_256(&raw).0;
        let actual = praos_vrf_input(slot, nonce);
        assert_eq!(actual, expected.to_vec());
    }

    #[test]
    fn tpraos_seed_xor_is_reversible() {
        // XOR with the same tag twice should yield the original base hash.
        let slot = SlotNo(99);
        let nonce = Nonce::Hash([0x55; 32]);
        let base_hash = hash_bytes_256(&raw_vrf_input_bytes(slot, nonce)).0;
        let seed = tpraos_vrf_seed(slot, nonce, VrfUsage::Leader);
        let tag = tpraos_seed_tag_hash(VrfUsage::Leader);
        let mut recovered = [0u8; 32];
        for i in 0..32 {
            recovered[i] = seed[i] ^ tag[i];
        }
        assert_eq!(recovered, base_hash);
    }
}
