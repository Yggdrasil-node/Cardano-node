use num_bigint::BigUint;
use num_integer::Integer;
use num_traits::{One, Zero};
use yggdrasil_crypto::blake2b::hash_bytes_256;
use yggdrasil_crypto::vrf::{VrfOutput, VrfSecretKey, VrfVerificationKey};
use yggdrasil_ledger::{Nonce, SlotNo};

use crate::ConsensusError;

/// Distinguishes the two VRF protocol modes used across Cardano eras.
///
/// - **TPraos** (Shelley–Alonzo): uses `mkSeed` with a per-purpose XOR tag
///   and checks the raw 512-bit VRF output against `2^512`.
/// - **Praos** (Babbage/Conway): uses `mkInputVRF` (Blake2b-256 of slot||nonce)
///   and applies range extension (`Blake2b-256("L" || output)`) to check a
///   256-bit value against `2^256`.
///
/// Reference: `Ouroboros.Consensus.Protocol.TPraos` vs
/// `Ouroboros.Consensus.Protocol.Praos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VrfMode {
    /// Shelley through Alonzo: `mkSeed` construction, raw 512-bit leader check.
    TPraos,
    /// Babbage and Conway: `mkInputVRF` construction, range-extended 256-bit
    /// leader check.
    Praos,
}

/// Distinguishes the two VRF proof purposes within a TPraos block header.
///
/// TPraos headers carry two VRF proofs (`nonce_vrf` and `leader_vrf`), each
/// produced over a different seed.  Praos headers carry only one unified VRF
/// proof that serves both purposes.
///
/// Reference: `seedEta` / `seedL` in `Cardano.Protocol.TPraos.BHeader`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VrfUsage {
    /// Leader election proof (TPraos `seedL`, tag = `mkNonceFromNumber 1`).
    Leader,
    /// Nonce contribution proof (TPraos `seedEta`, tag = `mkNonceFromNumber 0`).
    Nonce,
}

/// Pre-computed active slot coefficient for deterministic leader election.
///
/// Internally stores `-ln(1 - f)` as a rational number (numerator /
/// denominator) computed to at least 512 bits of precision, enabling
/// exact integer comparisons against 512-bit VRF outputs without any
/// floating-point arithmetic in the consensus-critical path.
///
/// Reference: `ActiveSlotCoeff` in `Cardano.Ledger.BaseTypes`, specifically
/// the `activeSlotLog` field.
#[derive(Clone, Debug)]
pub struct ActiveSlotCoeff {
    /// The original coefficient for display/diagnostics.
    f_val: f64,
    /// Numerator of `-ln(1 - f)` (positive when 0 < f ≤ 1).
    log_num: BigUint,
    /// Denominator of `-ln(1 - f)`.
    log_den: BigUint,
}

/// Number of Taylor-series terms used when pre-computing `-ln(1 - p/q)`.
///
/// Each term contributes `(p/q)^k / k` to the sum.  With `p/q ≤ 1`,
/// the truncation error after `N` terms is bounded by `(p/q)^(N+1) / (N+1)`.
/// For the mainnet value `p/q = 1/20`, `(1/20)^201 / 201 < 2^{-512}`.
const LN_SERIES_TERMS: u64 = 260;

/// Number of Taylor-series terms used inside `taylor_exp_cmp` to decide
/// the leader-value comparison.  With `|x| = σ * activeSlotLog` bounded
/// by `activeSlotLog < 1`, 80 terms give truncation error far below
/// `2^{-512}`.
const EXP_SERIES_TERMS: u64 = 80;

impl ActiveSlotCoeff {
    /// Creates an `ActiveSlotCoeff` from a rational `numerator / denominator`
    /// in `(0, 1]`.
    ///
    /// This is the preferred constructor because it avoids any floating-point
    /// imprecision. For mainnet, use `from_rational(1, 20)` for `f = 0.05`.
    pub fn from_rational(num: u64, den: u64) -> Result<Self, ConsensusError> {
        if den == 0 || num == 0 || num > den {
            return Err(ConsensusError::InvalidActiveSlotCoeff);
        }
        let f_val = num as f64 / den as f64;
        let (log_num, log_den) = compute_neg_ln_one_minus(num, den, LN_SERIES_TERMS);
        Ok(Self {
            f_val,
            log_num,
            log_den,
        })
    }

    /// Creates an `ActiveSlotCoeff` from an `f64` value in `(0, 1]`.
    ///
    /// The float is converted to a rational approximation with denominator
    /// `10^9`, which is sufficient for genesis-level precision.
    pub fn new(f: f64) -> Result<Self, ConsensusError> {
        if !f.is_finite() || f <= 0.0 || f > 1.0 {
            return Err(ConsensusError::InvalidActiveSlotCoeff);
        }
        // Convert to rational: round(f * 10^9) / 10^9.
        let scale: u64 = 1_000_000_000;
        let num = (f * scale as f64).round() as u64;
        let den = scale;
        // Reduce.
        let g = gcd_u64(num, den);
        Self::from_rational(num / g, den / g)
    }

    /// Returns the original coefficient as `f64` (for diagnostics only).
    pub fn to_f64(&self) -> f64 {
        self.f_val
    }
}

impl PartialEq for ActiveSlotCoeff {
    fn eq(&self, other: &Self) -> bool {
        // Two coefficients are equal when their log rationals are equal.
        self.log_num.clone() * other.log_den.clone() == other.log_num.clone() * self.log_den.clone()
    }
}

/// Computes the Praos leadership threshold φ_f(σ) = 1 − (1 − f)^σ
/// using floating-point arithmetic.
///
/// This function is **not** used in the consensus-critical leader check;
/// it exists for diagnostics, tests, and human-readable threshold display.
///
/// Reference: Section 4.1 of the Praos paper.
pub fn leadership_threshold(active_slot_coeff: &ActiveSlotCoeff, sigma: f64) -> f64 {
    1.0 - (1.0 - active_slot_coeff.f_val).powf(sigma)
}

// ---------------------------------------------------------------------------
// VRF input construction
// ---------------------------------------------------------------------------

/// Builds the raw VRF input bytes from a slot number and an epoch nonce
/// (pre-hash concatenation, no Blake2b-256, no seed tag).
///
/// This is the base concatenation `slot_be8 || nonce_bytes` before any
/// protocol-specific hashing or XOR.  Callers that need upstream-compatible
/// VRF inputs should use [`praos_vrf_input`] or [`tpraos_vrf_seed`] instead.
fn raw_vrf_input_bytes(slot: SlotNo, epoch_nonce: Nonce) -> Vec<u8> {
    let mut buf = Vec::with_capacity(40);
    buf.extend_from_slice(&slot.0.to_be_bytes());
    if let Nonce::Hash(h) = epoch_nonce {
        buf.extend_from_slice(&h);
    }
    buf
}

/// Builds the Praos (Babbage/Conway) VRF input: `Blake2b-256(slot_be8 || nonce_bytes)`.
///
/// The result is a 32-byte hash matching upstream `mkInputVRF` from
/// `Ouroboros.Consensus.Protocol.Praos.VRF`, which is used as
/// `getSignableRepresentation` for the single unified VRF proof.
pub fn praos_vrf_input(slot: SlotNo, epoch_nonce: Nonce) -> Vec<u8> {
    hash_bytes_256(&raw_vrf_input_bytes(slot, epoch_nonce))
        .0
        .to_vec()
}

/// Pre-computed seed tag hashes for TPraos VRF input construction.
///
/// Upstream `mkNonceFromNumber n` = `Nonce (Blake2b-256(CBOR(n)))`.
/// CBOR(0) = `0x00`, CBOR(1) = `0x01`.
///
/// Reference: `mkNonceFromNumber` in `Cardano.Ledger.BaseTypes`.
fn tpraos_seed_tag_hash(usage: VrfUsage) -> [u8; 32] {
    match usage {
        VrfUsage::Nonce => hash_bytes_256(&[0x00]).0, // mkNonceFromNumber 0 = seedEta
        VrfUsage::Leader => hash_bytes_256(&[0x01]).0, // mkNonceFromNumber 1 = seedL
    }
}

/// Builds a TPraos (Shelley–Alonzo) VRF seed: `Blake2b-256(slot_be8 || nonce_bytes) XOR tag_hash`.
///
/// `usage` selects the seed tag:
/// - `VrfUsage::Leader` → `seedL` (tag 1): used for the leader VRF proof.
/// - `VrfUsage::Nonce`  → `seedEta` (tag 0): used for the nonce VRF proof.
///
/// The result is a 32-byte value matching upstream `mkSeed` from
/// `Cardano.Protocol.TPraos.BHeader`.
pub fn tpraos_vrf_seed(slot: SlotNo, epoch_nonce: Nonce, usage: VrfUsage) -> Vec<u8> {
    let base_hash = hash_bytes_256(&raw_vrf_input_bytes(slot, epoch_nonce)).0;
    let tag_hash = tpraos_seed_tag_hash(usage);
    // XOR the two 32-byte hashes.
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = base_hash[i] ^ tag_hash[i];
    }
    result.to_vec()
}

/// Builds the VRF input for the given mode and usage.
///
/// - `VrfMode::Praos` ignores `usage` (single unified VRF) and returns
///   `praos_vrf_input()`.
/// - `VrfMode::TPraos` returns `tpraos_vrf_seed()` with the given usage.
pub fn vrf_input(slot: SlotNo, epoch_nonce: Nonce, mode: VrfMode, usage: VrfUsage) -> Vec<u8> {
    match mode {
        VrfMode::Praos => praos_vrf_input(slot, epoch_nonce),
        VrfMode::TPraos => tpraos_vrf_seed(slot, epoch_nonce, usage),
    }
}

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

/// Computes `exp(−x)` where `x = x_num/x_den > 0` via Taylor expansion,
/// and checks whether `target > q × exp(−x)`.
///
/// Returns `Ok(true)` (is leader) when `target > q × exp(−x)`, meaning the
/// VRF value is small enough to qualify.
///
/// The Taylor series of `exp(−x) = Σ_{k=0}^∞ (−x)^k / k!` alternates in
/// sign, so partial sums after an even number of terms overestimate
/// (upper bound) and after an odd number underestimate (lower bound).
///
/// Reference: `taylorExpCmp` in `Ouroboros.Consensus.Protocol.Praos.VRF`.
fn taylor_exp_cmp(
    q: &BigUint,
    target: &BigUint,
    x_num: &BigUint,
    x_den: &BigUint,
) -> Result<bool, ConsensusError> {
    // We maintain the partial sum and current term as rationals with a
    // common denominator, scaled by q.
    //
    // sum_scaled = q × partial_sum, tracked as (sum_num / sum_den).
    // term_scaled = q × current_term_magnitude.
    //
    // Initially: sum = q (the k=0 term), term = q.
    // At step k (1-based): term *= x / k, then sum += (-1)^k * term.

    let mut sum_num: BigUint = q.clone() * x_den; // q * x_den / x_den = q
    let mut sum_den: BigUint = x_den.clone();
    let mut term_num: BigUint = q.clone(); // magnitude of current term (numerator over term_den)
    let mut term_den: BigUint = BigUint::one();

    // target_scaled for comparison: target * sum_den (recomputed per step).
    for k in 1..=EXP_SERIES_TERMS {
        // term_{k} = term_{k-1} * x / k
        term_num *= x_num;
        term_den = term_den * x_den * BigUint::from(k);

        // Reduce to prevent unbounded growth.
        let g = term_num.gcd(&term_den);
        if !g.is_zero() && !g.is_one() {
            term_num /= &g;
            term_den /= &g;
        }

        // Bring sum and term to common denominator for add/subtract.
        // sum_num/sum_den  ±  term_num/term_den
        // = (sum_num*term_den ± term_num*sum_den) / (sum_den*term_den)
        let common_add = &sum_num * &term_den;
        let common_term = &term_num * &sum_den;
        let new_den = &sum_den * &term_den;

        if k % 2 == 1 {
            // Odd k: subtract (term is negative in exp(-x) expansion).
            // sum is now a lower bound.
            if common_add >= common_term {
                sum_num = common_add - &common_term;
            } else {
                // exp(-x) partial sum went negative — target > 0 → leader.
                return Ok(true);
            }
            sum_den = new_den;

            // Lower bound: if target * sum_den > sum_num * 1 → target > sum → leader.
            let target_scaled = target * &sum_den;
            if target_scaled > sum_num {
                return Ok(true);
            }
        } else {
            // Even k: add (term is positive).
            // sum is now an upper bound.
            sum_num = common_add + common_term;
            sum_den = new_den;

            // Upper bound: if target * sum_den <= sum_num → target ≤ sum → not leader.
            let target_scaled = target * &sum_den;
            if target_scaled <= sum_num {
                return Ok(false);
            }
        }

        // Reduce sum fraction.
        let g = sum_num.gcd(&sum_den);
        if !g.is_zero() && !g.is_one() {
            sum_num /= &g;
            sum_den /= &g;
        }
    }

    // If we exhaust the series without deciding, the value is extremely
    // close to the boundary.  The upstream Haskell returns `MaxReached`
    // which is treated as "not leader" (conservative).
    Ok(false)
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
    use yggdrasil_crypto::vrf::{VRF_PROOF_SIZE, VrfProof};

    let proof_arr: [u8; VRF_PROOF_SIZE] = proof_bytes
        .try_into()
        .map_err(|_| ConsensusError::InvalidVrfProof)?;
    let proof = VrfProof::from_bytes(proof_arr);

    let input = vrf_input(slot, epoch_nonce, mode, VrfUsage::Leader);
    let output = vk
        .verify(&input, &proof)
        .map_err(|_| ConsensusError::InvalidVrfProof)?;

    check_leader_value(&output, sigma_num, sigma_den, active_slot_coeff, mode)
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

/// Computes `-ln(1 - p/q)` as a rational `(numerator, denominator)` using
/// the Taylor series `-ln(1-x) = x + x²/2 + x³/3 + …` where `x = p/q`.
///
/// Accumulates `terms` terms of the series.
fn compute_neg_ln_one_minus(p: u64, q: u64, terms: u64) -> (BigUint, BigUint) {
    // -ln(1 - p/q) = Σ_{k=1}^{N} (p/q)^k / k
    //              = Σ (p^k) / (k * q^k)
    //
    // We compute this as a single rational: sum_num / sum_den.
    let bp = BigUint::from(p);
    let bq = BigUint::from(q);

    let mut sum_num = BigUint::zero();
    let mut sum_den = BigUint::one();

    // p_pow_k = p^k, q_pow_k = q^k, accumulated across iterations.
    let mut p_pow_k = BigUint::one();
    let mut q_pow_k = BigUint::one();

    for k in 1..=terms {
        p_pow_k *= &bp;
        q_pow_k *= &bq;

        // term = p^k / (k * q^k)
        let term_num = &p_pow_k;
        let term_den = BigUint::from(k) * &q_pow_k;

        // sum += term: sum_num/sum_den + term_num/term_den
        sum_num = sum_num * &term_den + term_num * &sum_den;
        sum_den *= term_den;

        // Reduce every 20 iterations to keep numerators manageable.
        if k % 20 == 0 {
            let g = sum_num.gcd(&sum_den);
            if !g.is_zero() && !g.is_one() {
                sum_num /= &g;
                sum_den /= &g;
            }
        }
    }

    // Final reduction.
    let g = sum_num.gcd(&sum_den);
    if !g.is_zero() && !g.is_one() {
        sum_num /= &g;
        sum_den /= &g;
    }

    (sum_num, sum_den)
}

/// Simple GCD for u64 values.
fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

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
