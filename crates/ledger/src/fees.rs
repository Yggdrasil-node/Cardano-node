//! Fee calculation and validation.
//!
//! Implements the Cardano fee formula for all eras:
//! - **Shelley–Mary**: `min_fee = min_fee_a × tx_size + min_fee_b`
//! - **Alonzo+**: adds `script_fee = Σ(price_mem × mem + price_step × steps)`
//!
//! Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `validateFeeTooSmallUTxO`
//! and `Cardano.Ledger.Alonzo.Rules.Utxo` — `validateExUnitsTooBigUTxO`.

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
}
