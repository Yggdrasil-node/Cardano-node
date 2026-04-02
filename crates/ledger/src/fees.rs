//! Fee calculation and validation.
//!
//! Implements the Cardano fee formula for all eras:
//! - **Shelley–Mary**: `min_fee = min_fee_a × tx_size + min_fee_b`
//! - **Alonzo+**: adds `script_fee = Σ(price_mem × mem + price_step × steps)`
//! - **Conway**: adds `ref_script_fee = tierRefScriptFee(multiplier, stride, base, size)`
//!
//! Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `validateFeeTooSmallUTxO`,
//! `Cardano.Ledger.Alonzo.Rules.Utxo` — `validateExUnitsTooBigUTxO`, and
//! `Cardano.Ledger.Conway.Tx` — `getConwayMinFeeTx` / `tierRefScriptFee`.

use crate::eras::alonzo::ExUnits;
use crate::error::LedgerError;
use crate::protocol_params::ProtocolParameters;
use crate::types::UnitInterval;

/// Computes the minimum fee for a transaction of the given serialized size.
///
/// `min_fee = min_fee_a × tx_size_bytes + min_fee_b`
///
/// Reference: `Cardano.Ledger.Core.PParams` — `minFee`.
pub fn min_fee_linear(params: &ProtocolParameters, tx_size_bytes: usize) -> u64 {
    params
        .min_fee_a
        .saturating_mul(tx_size_bytes as u64)
        .saturating_add(params.min_fee_b)
}

/// Evaluates a `UnitInterval` rational as a u64-scaled value.
///
/// `rational_ceil(value, UnitInterval { n, d }) = ceil(value × n / d)`
fn rational_ceil(value: u64, ratio: &UnitInterval) -> u64 {
    if ratio.denominator == 0 {
        return 0;
    }
    let num = value as u128 * ratio.numerator as u128;
    let den = ratio.denominator as u128;
    num.div_ceil(den) as u64
}

/// Computes the fee contribution from script execution units.
///
/// `script_fee = ceil(mem × price_mem) + ceil(steps × price_step)`
///
/// Returns 0 when execution-unit prices are not configured (pre-Alonzo).
///
/// Reference: `Cardano.Ledger.Alonzo.Tx` — `totExUnits` pricing.
pub fn script_fee(params: &ProtocolParameters, total_ex_units: &ExUnits) -> u64 {
    let mem_fee = params
        .price_mem
        .as_ref()
        .map(|pm| rational_ceil(total_ex_units.mem, pm))
        .unwrap_or(0);
    let step_fee = params
        .price_step
        .as_ref()
        .map(|ps| rational_ceil(total_ex_units.steps, ps))
        .unwrap_or(0);
    mem_fee.saturating_add(step_fee)
}

/// Computes the total minimum fee for a transaction, including script costs.
///
/// `total_min_fee = linear_fee + script_fee`
pub fn total_min_fee(
    params: &ProtocolParameters,
    tx_size_bytes: usize,
    total_ex_units: Option<&ExUnits>,
) -> u64 {
    let base = min_fee_linear(params, tx_size_bytes);
    match total_ex_units {
        Some(units) => base.saturating_add(script_fee(params, units)),
        None => base,
    }
}

// ---------------------------------------------------------------------------
// Conway reference-script tiered fee
// ---------------------------------------------------------------------------

/// Upstream constants from `Cardano.Ledger.Conway.PParams`:
/// `ppRefScriptCostMultiplierG = 1.2` and `ppRefScriptCostStrideG = 25_600`.
const REF_SCRIPT_COST_MULTIPLIER_NUM: u128 = 6;
const REF_SCRIPT_COST_MULTIPLIER_DEN: u128 = 5;
/// Size increment (in bytes) at which the price per byte grows by
/// `REF_SCRIPT_COST_MULTIPLIER`.  Upstream: `ppRefScriptCostStrideG = 25_600`.
const REF_SCRIPT_COST_STRIDE: usize = 25_600;

/// Computes the tiered fee for reference scripts attached to a transaction.
///
/// The fee grows exponentially with the total reference-script size:
/// for each `stride`-byte tier, the price per byte is multiplied by
/// `multiplier` (1.2).  The final partial tier is priced at the
/// then-current tier price.
///
/// `tierRefScriptFee multiplier sizeIncrement baseFee totalSize`
///
/// Reference: `Cardano.Ledger.Conway.Tx` — `tierRefScriptFee`.
pub fn tier_ref_script_fee(base_fee: &UnitInterval, total_ref_script_size: usize) -> u64 {
    if total_ref_script_size == 0 || base_fee.numerator == 0 {
        return 0;
    }

    // Work in u128 rational arithmetic: accumulator and tier price are
    // kept as (numerator, denominator) pairs to avoid precision loss.
    let stride = REF_SCRIPT_COST_STRIDE as u128;
    let mut remaining = total_ref_script_size as u128;

    // tier_price = base_fee  (as rational num/den)
    let mut tp_num = base_fee.numerator as u128;
    let mut tp_den = base_fee.denominator as u128;
    if tp_den == 0 {
        return 0;
    }

    // accumulator rational (acc_num / acc_den) — starts at 0/1
    let mut acc_num: u128 = 0;
    let mut acc_den: u128 = 1;

    while remaining >= stride {
        // acc += stride * tier_price
        // stride * tp_num / tp_den  →  add to acc_num/acc_den
        let chunk_num = stride * tp_num;
        let chunk_den = tp_den;
        // acc = acc + chunk  →  (acc_num * chunk_den + chunk_num * acc_den) / (acc_den * chunk_den)
        acc_num = acc_num * chunk_den + chunk_num * acc_den;
        acc_den *= chunk_den;

        // Reduce to prevent overflow: gcd simplification
        let g = gcd_u128(acc_num, acc_den);
        acc_num /= g;
        acc_den /= g;

        // tier_price *= multiplier  (6/5)
        tp_num *= REF_SCRIPT_COST_MULTIPLIER_NUM;
        tp_den *= REF_SCRIPT_COST_MULTIPLIER_DEN;
        let g = gcd_u128(tp_num, tp_den);
        tp_num /= g;
        tp_den /= g;

        remaining -= stride;
    }

    // Final partial tier: remaining bytes at current tier price
    if remaining > 0 {
        let chunk_num = remaining * tp_num;
        let chunk_den = tp_den;
        acc_num = acc_num * chunk_den + chunk_num * acc_den;
        acc_den *= chunk_den;
        let g = gcd_u128(acc_num, acc_den);
        acc_num /= g;
        acc_den /= g;
    }

    // floor(acc)
    (acc_num / acc_den) as u64
}

/// Computes the Conway-era total minimum fee: base Alonzo fee plus the
/// tiered reference-script fee.
///
/// Reference: `Cardano.Ledger.Conway.Tx` — `getConwayMinFeeTx`.
pub fn conway_total_min_fee(
    params: &ProtocolParameters,
    tx_size_bytes: usize,
    total_ex_units: Option<&ExUnits>,
    ref_scripts_size: usize,
) -> u64 {
    let base = total_min_fee(params, tx_size_bytes, total_ex_units);
    let ref_fee = params
        .min_fee_ref_script_cost_per_byte
        .as_ref()
        .map(|cost| tier_ref_script_fee(cost, ref_scripts_size))
        .unwrap_or(0);
    base.saturating_add(ref_fee)
}

/// GCD helper for u128 (binary GCD / Stein's algorithm).
fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    if a == 0 { return b; }
    if b == 0 { return a; }
    let shift = (a | b).trailing_zeros();
    a >>= a.trailing_zeros();
    loop {
        b >>= b.trailing_zeros();
        if a > b { std::mem::swap(&mut a, &mut b); }
        b -= a;
        if b == 0 { return a << shift; }
    }
}

/// Validates that the declared fee meets the minimum requirement.
///
/// Returns `Err(LedgerError::FeeTooSmall)` when `declared_fee < min_fee`.
pub fn validate_fee(
    params: &ProtocolParameters,
    tx_size_bytes: usize,
    total_ex_units: Option<&ExUnits>,
    declared_fee: u64,
) -> Result<(), LedgerError> {
    let min = total_min_fee(params, tx_size_bytes, total_ex_units);
    if declared_fee < min {
        return Err(LedgerError::FeeTooSmall {
            minimum: min,
            declared: declared_fee,
        });
    }
    Ok(())
}

/// Validates that the declared fee meets the Conway-era minimum, which
/// includes the tiered reference-script fee.
///
/// Reference: `Cardano.Ledger.Conway.Tx` — `getConwayMinFeeTx`.
pub fn validate_conway_fee(
    params: &ProtocolParameters,
    tx_size_bytes: usize,
    total_ex_units: Option<&ExUnits>,
    ref_scripts_size: usize,
    declared_fee: u64,
) -> Result<(), LedgerError> {
    let min = conway_total_min_fee(params, tx_size_bytes, total_ex_units, ref_scripts_size);
    if declared_fee < min {
        return Err(LedgerError::FeeTooSmall {
            minimum: min,
            declared: declared_fee,
        });
    }
    Ok(())
}

/// Validates that a transaction's total execution units do not exceed the
/// per-transaction limit.
///
/// Returns `Err(LedgerError::ExUnitsExceedTxLimit)` on violation.
pub fn validate_tx_ex_units(
    params: &ProtocolParameters,
    total_ex_units: &ExUnits,
) -> Result<(), LedgerError> {
    if let Some(ref max) = params.max_tx_ex_units {
        if total_ex_units.mem > max.mem || total_ex_units.steps > max.steps {
            return Err(LedgerError::ExUnitsExceedTxLimit {
                tx_mem: total_ex_units.mem,
                tx_steps: total_ex_units.steps,
                max_mem: max.mem,
                max_steps: max.steps,
            });
        }
    }
    Ok(())
}

/// Validates that the declared transaction size does not exceed the maximum.
///
/// Returns `Err(LedgerError::TxTooLarge)` on violation.
pub fn validate_tx_size(
    params: &ProtocolParameters,
    tx_size_bytes: usize,
) -> Result<(), LedgerError> {
    if tx_size_bytes > params.max_tx_size as usize {
        return Err(LedgerError::TxTooLarge {
            actual: tx_size_bytes,
            max: params.max_tx_size as usize,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelley_min_fee() {
        let params = ProtocolParameters::default();
        // 44 * 200 + 155_381 = 8800 + 155_381 = 164_181
        assert_eq!(min_fee_linear(&params, 200), 164_181);
    }

    #[test]
    fn alonzo_script_fee() {
        let params = ProtocolParameters::alonzo_defaults();
        let units = ExUnits {
            mem: 1_000_000,
            steps: 1_000_000_000,
        };
        let fee = script_fee(&params, &units);
        // ceil(1_000_000 * 577/10_000) + ceil(1_000_000_000 * 721/10_000_000)
        // = 57_700 + 72_100 = 129_800
        assert_eq!(fee, 129_800);
    }

    #[test]
    fn fee_validation_pass() {
        let params = ProtocolParameters::default();
        assert!(validate_fee(&params, 200, None, 200_000).is_ok());
    }

    #[test]
    fn fee_validation_fail() {
        let params = ProtocolParameters::default();
        let result = validate_fee(&params, 200, None, 100);
        assert!(matches!(result, Err(LedgerError::FeeTooSmall { .. })));
    }

    #[test]
    fn tx_size_validation_pass() {
        let params = ProtocolParameters::default();
        assert!(validate_tx_size(&params, 1000).is_ok());
    }

    #[test]
    fn tx_size_validation_fail() {
        let params = ProtocolParameters::default();
        let result = validate_tx_size(&params, 100_000);
        assert!(matches!(result, Err(LedgerError::TxTooLarge { .. })));
    }

    #[test]
    fn tx_ex_units_validation_pass() {
        let params = ProtocolParameters::alonzo_defaults();
        let units = ExUnits {
            mem: 1_000,
            steps: 1_000,
        };
        assert!(validate_tx_ex_units(&params, &units).is_ok());
    }

    #[test]
    fn tx_ex_units_validation_fail_mem() {
        let params = ProtocolParameters::alonzo_defaults();
        let units = ExUnits {
            mem: u64::MAX,
            steps: 1,
        };
        assert!(matches!(
            validate_tx_ex_units(&params, &units),
            Err(LedgerError::ExUnitsExceedTxLimit { .. })
        ));
    }

    // ── Tiered reference-script fee tests (Conway) ─────────────────────

    #[test]
    fn tier_ref_script_fee_zero_size() {
        let base = UnitInterval { numerator: 15, denominator: 1 };
        assert_eq!(tier_ref_script_fee(&base, 0), 0);
    }

    #[test]
    fn tier_ref_script_fee_zero_base() {
        let base = UnitInterval { numerator: 0, denominator: 1 };
        assert_eq!(tier_ref_script_fee(&base, 10_000), 0);
    }

    #[test]
    fn tier_ref_script_fee_single_tier_sub_stride() {
        // 10_000 bytes, base = 15/1 lovelace per byte
        // Only tier 0: 10_000 * 15 = 150_000
        let base = UnitInterval { numerator: 15, denominator: 1 };
        assert_eq!(tier_ref_script_fee(&base, 10_000), 150_000);
    }

    #[test]
    fn tier_ref_script_fee_exact_stride() {
        // 25_600 bytes at base = 15/1 per byte
        // Full first tier: 25_600 * 15 = 384_000
        let base = UnitInterval { numerator: 15, denominator: 1 };
        assert_eq!(tier_ref_script_fee(&base, 25_600), 384_000);
    }

    #[test]
    fn tier_ref_script_fee_two_tiers() {
        // 51_200 bytes at base = 15/1 per byte
        // Tier 0: 25_600 * 15 = 384_000
        // Tier 1: 25_600 * 15 * 1.2 = 25_600 * 18 = 460_800
        // Total = 844_800
        let base = UnitInterval { numerator: 15, denominator: 1 };
        assert_eq!(tier_ref_script_fee(&base, 51_200), 844_800);
    }

    #[test]
    fn tier_ref_script_fee_partial_second_tier() {
        // 30_000 bytes at base = 15/1
        // Tier 0: 25_600 * 15 = 384_000
        // Tier 1 (partial, 4_400 bytes): 4_400 * 15 * 1.2 = 4_400 * 18 = 79_200
        // Total = 463_200
        let base = UnitInterval { numerator: 15, denominator: 1 };
        assert_eq!(tier_ref_script_fee(&base, 30_000), 463_200);
    }

    #[test]
    fn tier_ref_script_fee_fractional_base() {
        // base = 1/10 per byte, 25_600 bytes
        // Tier 0: 25_600 * 0.1 = 2_560
        let base = UnitInterval { numerator: 1, denominator: 10 };
        assert_eq!(tier_ref_script_fee(&base, 25_600), 2_560);
    }

    #[test]
    fn tier_ref_script_fee_three_tiers_full() {
        // 76_800 bytes at base = 10/1
        // Tier 0: 25_600 * 10 = 256_000
        // Tier 1: 25_600 * 12 = 307_200
        // Tier 2: 25_600 * 14.4 = 368_640
        // Total = 931_840
        let base = UnitInterval { numerator: 10, denominator: 1 };
        assert_eq!(tier_ref_script_fee(&base, 76_800), 931_840);
    }

    #[test]
    fn conway_total_min_fee_with_ref_scripts() {
        let mut params = ProtocolParameters::alonzo_defaults();
        params.min_fee_ref_script_cost_per_byte = Some(UnitInterval {
            numerator: 15,
            denominator: 1,
        });
        // Base fee for 200-byte tx with no script units = min_fee_linear(params, 200)
        let base = total_min_fee(&params, 200, None);
        let with_ref = conway_total_min_fee(&params, 200, None, 10_000);
        // ref_fee = 10_000 * 15 = 150_000
        assert_eq!(with_ref, base + 150_000);
    }

    #[test]
    fn conway_total_min_fee_zero_ref_scripts() {
        let mut params = ProtocolParameters::alonzo_defaults();
        params.min_fee_ref_script_cost_per_byte = Some(UnitInterval {
            numerator: 15,
            denominator: 1,
        });
        let base = total_min_fee(&params, 200, None);
        let with_ref = conway_total_min_fee(&params, 200, None, 0);
        assert_eq!(with_ref, base);
    }

    #[test]
    fn validate_conway_fee_passes() {
        let mut params = ProtocolParameters::alonzo_defaults();
        params.min_fee_ref_script_cost_per_byte = Some(UnitInterval {
            numerator: 15,
            denominator: 1,
        });
        let min = conway_total_min_fee(&params, 200, None, 10_000);
        assert!(validate_conway_fee(&params, 200, None, 10_000, min).is_ok());
        assert!(validate_conway_fee(&params, 200, None, 10_000, min + 1).is_ok());
    }

    #[test]
    fn validate_conway_fee_fails() {
        let mut params = ProtocolParameters::alonzo_defaults();
        params.min_fee_ref_script_cost_per_byte = Some(UnitInterval {
            numerator: 15,
            denominator: 1,
        });
        let min = conway_total_min_fee(&params, 200, None, 10_000);
        let result = validate_conway_fee(&params, 200, None, 10_000, min - 1);
        assert!(matches!(result, Err(LedgerError::FeeTooSmall { .. })));
    }

    #[test]
    fn gcd_helper() {
        assert_eq!(gcd_u128(0, 5), 5);
        assert_eq!(gcd_u128(5, 0), 5);
        assert_eq!(gcd_u128(12, 8), 4);
        assert_eq!(gcd_u128(17, 13), 1);
        assert_eq!(gcd_u128(100, 100), 100);
    }
}
