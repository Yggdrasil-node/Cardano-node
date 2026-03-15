//! Collateral validation for Alonzo+ script transactions.
//!
//! When a transaction includes Plutus scripts (phase-2 validation), it must
//! pledge collateral inputs that cover the fee penalty if any script fails.
//!
//! Rules enforced:
//! - At least one collateral input (Alonzo+).
//! - Number of collateral inputs ≤ `max_collateral_inputs`.
//! - Collateral inputs must exist in the UTxO set.
//! - Total collateral value ≥ `fee × collateral_percentage / 100`.
//! - Collateral outputs contain only ADA (no multi-asset tokens).
//!
//! Reference:
//! `Cardano.Ledger.Alonzo.Rules.Utxo` — `validateTotalCollateral`
//! `Cardano.Ledger.Alonzo.Rules.Utxo` — `validateCollateralContainsNonADA`

use crate::eras::shelley::ShelleyTxIn;
use crate::error::LedgerError;
use crate::protocol_params::ProtocolParameters;
use crate::utxo::{MultiEraTxOut, MultiEraUtxo};

/// Validates collateral inputs for a script transaction.
///
/// `collateral_inputs` is the `set<transaction_input>` from CDDL key 13
/// of the Alonzo+ transaction body.
///
/// Returns the total collateral lovelace on success.
pub fn validate_collateral(
    params: &ProtocolParameters,
    utxo: &MultiEraUtxo,
    collateral_inputs: &[ShelleyTxIn],
    declared_fee: u64,
) -> Result<u64, LedgerError> {
    // Must have at least one collateral input.
    if collateral_inputs.is_empty() {
        return Err(LedgerError::NoCollateralInputs);
    }

    // Check count limit.
    if let Some(max) = params.max_collateral_inputs {
        if collateral_inputs.len() > max as usize {
            return Err(LedgerError::TooManyCollateralInputs {
                count: collateral_inputs.len(),
                max,
            });
        }
    }

    // Resolve each collateral input and accumulate value.
    let mut total_collateral: u64 = 0;
    for input in collateral_inputs {
        let txout = utxo
            .get(input)
            .ok_or(LedgerError::CollateralInputNotInUtxo)?;

        // Collateral must be pure ADA — no multi-asset tokens.
        if has_non_ada(txout) {
            return Err(LedgerError::CollateralContainsNonAda);
        }

        total_collateral = total_collateral.saturating_add(txout.coin());
    }

    // Check that collateral covers the required percentage of the fee.
    if let Some(pct) = params.collateral_percentage {
        let required = fee_collateral_required(declared_fee, pct);
        if total_collateral < required {
            return Err(LedgerError::InsufficientCollateral {
                fee: declared_fee,
                percentage: pct,
                required,
                provided: total_collateral,
            });
        }
    }

    Ok(total_collateral)
}

/// Computes the minimum collateral required: `ceil(fee × percentage / 100)`.
fn fee_collateral_required(fee: u64, percentage: u64) -> u64 {
    let num = fee as u128 * percentage as u128;
    num.div_ceil(100) as u64
}

/// Returns `true` when a transaction output contains multi-asset tokens.
fn has_non_ada(txout: &MultiEraTxOut) -> bool {
    match txout.value() {
        crate::eras::mary::Value::Coin(_) => false,
        crate::eras::mary::Value::CoinAndAssets(_, ref assets) => !assets.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eras::shelley::{ShelleyTxIn, ShelleyTxOut};
    use crate::protocol_params::ProtocolParameters;
    use crate::utxo::MultiEraUtxo;

    fn make_txin(index: u16) -> ShelleyTxIn {
        ShelleyTxIn {
            transaction_id: [0u8; 32],
            index,
        }
    }

    fn make_ada_txout(lovelace: u64) -> MultiEraTxOut {
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: vec![0u8; 57],
            amount: lovelace,
        })
    }

    fn alonzo_params() -> ProtocolParameters {
        ProtocolParameters::alonzo_defaults()
    }

    #[test]
    fn valid_collateral() {
        let params = alonzo_params();
        let txin = make_txin(0);
        let mut utxo = MultiEraUtxo::new();
        utxo.insert(txin.clone(), make_ada_txout(5_000_000));

        let result = validate_collateral(&params, &utxo, &[txin], 1_000_000);
        assert!(result.is_ok());
        assert_eq!(result.expect("valid collateral"), 5_000_000);
    }

    #[test]
    fn no_collateral_inputs() {
        let params = alonzo_params();
        let utxo = MultiEraUtxo::new();
        let result = validate_collateral(&params, &utxo, &[], 1_000_000);
        assert!(matches!(result, Err(LedgerError::NoCollateralInputs)));
    }

    #[test]
    fn too_many_collateral_inputs() {
        let params = alonzo_params(); // max_collateral_inputs = 3
        let mut utxo = MultiEraUtxo::new();
        let inputs: Vec<ShelleyTxIn> = (0..4)
            .map(|i| {
                let txin = make_txin(i);
                utxo.insert(txin.clone(), make_ada_txout(1_000_000));
                txin
            })
            .collect();

        let result = validate_collateral(&params, &utxo, &inputs, 100_000);
        assert!(matches!(
            result,
            Err(LedgerError::TooManyCollateralInputs { count: 4, max: 3 })
        ));
    }

    #[test]
    fn collateral_input_not_in_utxo() {
        let params = alonzo_params();
        let utxo = MultiEraUtxo::new();
        let txin = make_txin(0);
        let result = validate_collateral(&params, &utxo, &[txin], 1_000_000);
        assert!(matches!(result, Err(LedgerError::CollateralInputNotInUtxo)));
    }

    #[test]
    fn insufficient_collateral() {
        let params = alonzo_params(); // collateral_percentage = 150
        let txin = make_txin(0);
        let mut utxo = MultiEraUtxo::new();
        utxo.insert(txin.clone(), make_ada_txout(1_000_000));

        // Fee = 2_000_000, required collateral = ceil(2_000_000 * 150 / 100) = 3_000_000
        let result = validate_collateral(&params, &utxo, &[txin], 2_000_000);
        assert!(matches!(
            result,
            Err(LedgerError::InsufficientCollateral { .. })
        ));
    }

    #[test]
    fn fee_collateral_ceil() {
        // ceil(1_000_001 * 150 / 100) = ceil(1_500_001.5) = 1_500_002
        assert_eq!(fee_collateral_required(1_000_001, 150), 1_500_002);
        // Exact: 1_000_000 * 150 / 100 = 1_500_000
        assert_eq!(fee_collateral_required(1_000_000, 150), 1_500_000);
    }

    #[test]
    fn collateral_non_ada_rejected() {
        use crate::eras::mary::Value;
        use std::collections::BTreeMap;

        let params = alonzo_params();
        let txin = make_txin(0);

        // Build a Mary-era output with multi-asset
        let mut assets = BTreeMap::new();
        let mut inner = BTreeMap::new();
        inner.insert(b"token".to_vec(), 100);
        assets.insert([1u8; 28], inner);
        let mary_out = crate::eras::mary::MaryTxOut {
            address: vec![0u8; 57],
            amount: Value::CoinAndAssets(5_000_000, assets),
        };

        let mut utxo = MultiEraUtxo::new();
        utxo.insert(txin.clone(), MultiEraTxOut::Mary(mary_out));

        let result = validate_collateral(&params, &utxo, &[txin], 1_000_000);
        assert!(matches!(result, Err(LedgerError::CollateralContainsNonAda)));
    }
}
