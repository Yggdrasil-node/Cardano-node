use yggdrasil_crypto::vrf::{VrfOutput, VrfSecretKey, VrfVerificationKey};
use yggdrasil_ledger::{Nonce, SlotNo};

use crate::ConsensusError;

/// The Praos active slot coefficient, expected to be in the inclusive range
/// `(0, 1]`.
///
/// This represents the probability that a party holding *all* the stake will
/// be elected as leader for a given slot.
///
/// Reference: `Cardano.Ledger.BaseTypes` — `ActiveSlotCoeff`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveSlotCoeff(pub f64);

/// Computes the Praos leadership threshold φ_f(σ) = 1 − (1 − f)^σ.
///
/// Returns the probability that a stake-pool with relative stake `sigma`
/// is elected leader in a slot.
///
/// Reference: Section 4.1 of the Praos paper; `checkLeaderNatValue` in
/// `Cardano.Protocol.TPraos.BHeader`.
pub fn leadership_threshold(
    active_slot_coeff: ActiveSlotCoeff,
    sigma: f64,
) -> Result<f64, ConsensusError> {
    if !(0.0..=1.0).contains(&active_slot_coeff.0) {
        return Err(ConsensusError::InvalidActiveSlotCoeff);
    }

    Ok(1.0 - (1.0 - active_slot_coeff.0).powf(sigma))
}

// ---------------------------------------------------------------------------
// VRF input construction
// ---------------------------------------------------------------------------

/// Builds the VRF input bytes from a slot number and an epoch nonce.
///
/// The input is `slot_be8 || nonce_bytes` (or just `slot_be8` when the nonce
/// is neutral).
///
/// Reference: `mkInputVRF` in
/// `Ouroboros.Consensus.Protocol.Praos.VRF` — the produced bytes are
/// hashed *by the VRF prove function itself*, so we return the raw
/// concatenation here.
pub fn vrf_input(slot: SlotNo, epoch_nonce: Nonce) -> Vec<u8> {
    let mut buf = Vec::with_capacity(40);
    buf.extend_from_slice(&slot.0.to_be_bytes());
    if let Nonce::Hash(h) = epoch_nonce {
        buf.extend_from_slice(&h);
    }
    buf
}

// ---------------------------------------------------------------------------
// Leader check
// ---------------------------------------------------------------------------

/// Determines whether a VRF output qualifies the holder as slot leader
/// given their relative stake `sigma` and the active slot coefficient.
///
/// The check mirrors upstream: interpret the (range-extended) VRF output
/// as a natural number `v` in `[0, 2^(8*output_len))`, compute the
/// normalised value `p = v / max`, and accept if `p < threshold`.
///
/// Instead of the upstream Taylor-expansion fixed-point comparison we use
/// floating-point arithmetic for now; this is sufficient for early
/// integration and testing but will be replaced with deterministic
/// fixed-point math before mainnet parity is claimed.
///
/// Reference: `checkLeaderValue` / `checkLeaderNatValue` in
/// `Cardano.Protocol.TPraos.BHeader`.
pub fn check_leader_value(
    vrf_output: &VrfOutput,
    sigma: f64,
    active_slot_coeff: ActiveSlotCoeff,
) -> Result<bool, ConsensusError> {
    let threshold = leadership_threshold(active_slot_coeff, sigma)?;

    // Interpret the 64-byte output as a big-endian unsigned integer
    // normalised to [0, 1).
    let bytes = vrf_output.to_bytes();
    let p = nat_fraction(&bytes);

    Ok(p < threshold)
}

/// Converts an arbitrary-length big-endian byte string into a fraction
/// in `[0, 1)` by dividing by `2^(8 * len)`.
///
/// Uses the first 8 bytes to avoid f64 precision issues with very long
/// byte strings while preserving enough entropy for the comparison.
fn nat_fraction(bytes: &[u8]) -> f64 {
    // Take the first 8 bytes as a u64 and normalise.
    let mut eight = [0u8; 8];
    let take = bytes.len().min(8);
    eight[..take].copy_from_slice(&bytes[..take]);
    let val = u64::from_be_bytes(eight);
    val as f64 / u64::MAX as f64
}

// ---------------------------------------------------------------------------
// Full leader-election helper
// ---------------------------------------------------------------------------

/// Evaluates whether the given VRF secret key wins the slot lottery.
///
/// Performs the full pipeline:
/// 1. Construct the VRF input from `slot` and `epoch_nonce`.
/// 2. Produce a VRF proof using the secret key.
/// 3. Check the output against the leader threshold.
///
/// Returns `Ok(Some((output, proof_bytes)))` if the key is elected leader,
/// `Ok(None)` otherwise.
///
/// The proof is generated using the standard (draft-03) VRF algorithm,
/// producing an 80-byte proof that matches the on-chain `vrf_cert` wire
/// format (`bytes .size 80`).
///
/// Reference: `checkIsLeader` in
/// `Ouroboros.Consensus.Protocol.Praos`.
pub fn check_is_leader(
    sk: &VrfSecretKey,
    slot: SlotNo,
    epoch_nonce: Nonce,
    sigma: f64,
    active_slot_coeff: ActiveSlotCoeff,
) -> Result<Option<(VrfOutput, Vec<u8>)>, ConsensusError> {
    let input = vrf_input(slot, epoch_nonce);
    let (output, proof) = sk
        .prove(&input)
        .map_err(|_| ConsensusError::InvalidVrfProof)?;
    let is_leader = check_leader_value(&output, sigma, active_slot_coeff)?;
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
/// Expects a standard (draft-03) VRF proof of 80 bytes, matching the
/// on-chain `vrf_cert = [bytes, bytes .size 80]` wire format used across
/// all eras (Shelley through Conway).
///
/// Reference: `validateVRFSignature` in
/// `Ouroboros.Consensus.Protocol.Praos`.
pub fn verify_leader_proof(
    vk: &VrfVerificationKey,
    slot: SlotNo,
    epoch_nonce: Nonce,
    proof_bytes: &[u8],
    sigma: f64,
    active_slot_coeff: ActiveSlotCoeff,
) -> Result<bool, ConsensusError> {
    use yggdrasil_crypto::vrf::{VrfProof, VRF_PROOF_SIZE};

    let proof_arr: [u8; VRF_PROOF_SIZE] = proof_bytes
        .try_into()
        .map_err(|_| ConsensusError::InvalidVrfProof)?;
    let proof = VrfProof::from_bytes(proof_arr);

    let input = vrf_input(slot, epoch_nonce);
    let output = vk
        .verify(&input, &proof)
        .map_err(|_| ConsensusError::InvalidVrfProof)?;

    check_leader_value(&output, sigma, active_slot_coeff)
}
