//! Active slot coefficient + deterministic-integer math primitives for
//! Praos leader election.
//!
//! Mirrors upstream `Cardano.Ledger.BaseTypes::ActiveSlotCoeff` and the
//! `taylorExpCmp` / `boundedRationalFromRational` helpers in
//! `Ouroboros.Consensus.Protocol.Praos.VRF`.
//!
//! Two public types/fns:
//!
//! - `ActiveSlotCoeff` — pre-computed `f` with `-ln(1 - f)` rational stored
//!   to ≥512 bits of precision so leader checks can compare against
//!   512-bit VRF outputs without floating-point arithmetic.
//! - `leadership_threshold` — diagnostic-only floating-point computation
//!   of `phi_f(sigma) = 1 - (1 - f)^sigma`.
//!
//! Plus `pub(super)` items used by the residual leader-check entry points
//! that stay in the parent module:
//!
//! - `taylor_exp_cmp` — Taylor-series comparison of `target` vs
//!   `q * exp(-x)` over a rational `x`.
//! - `EXP_SERIES_TERMS` — series-truncation constant.
//!
//! Extracted from `praos.rs` in R273a (Phase γ §R273 first slice).

use num_bigint::BigUint;
use num_integer::Integer;
use num_traits::{One, Zero};

use crate::ConsensusError;

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
    pub(super) log_num: BigUint,
    /// Denominator of `-ln(1 - f)`.
    pub(super) log_den: BigUint,
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
pub(super) fn taylor_exp_cmp(
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
