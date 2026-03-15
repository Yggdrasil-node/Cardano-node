//! Minimum UTxO output validation.
//!
//! Enforces that every transaction output carries at least the minimum
//! required lovelace value as determined by the protocol parameters:
//!
//! - **Shelley–Mary**: flat `min_utxo_value`.
//! - **Alonzo+**: `coins_per_utxo_byte × (serialized_size + overhead)`.
//!
//! Reference:
//! `Cardano.Ledger.Shelley.Rules.Utxo` — `validateOutputTooSmallUTxO`
//! `Cardano.Ledger.Alonzo.Rules.Utxo` — `validateOutputTooBigUTxO`

use crate::cbor::CborEncode;
use crate::error::LedgerError;
use crate::protocol_params::ProtocolParameters;
use crate::utxo::MultiEraTxOut;

/// Validates that a single output meets the minimum lovelace requirement.
///
/// Returns `Ok(())` on success or `Err(LedgerError::OutputTooSmall)` when
/// the output carries less than the minimum.
pub fn validate_min_utxo(
    params: &ProtocolParameters,
    output: &MultiEraTxOut,
) -> Result<(), LedgerError> {
    let serialized_size = output.to_cbor_bytes().len();
    if let Some(minimum) = params.min_lovelace_for_utxo(serialized_size) {
        let actual = output.coin();
        if actual < minimum {
            return Err(LedgerError::OutputTooSmall { minimum, actual });
        }
    }
    Ok(())
}

/// Validates all outputs in a transaction body meet the minimum lovelace.
///
/// Returns on the first failing output.
pub fn validate_all_outputs_min_utxo(
    params: &ProtocolParameters,
    outputs: &[MultiEraTxOut],
) -> Result<(), LedgerError> {
    for output in outputs {
        validate_min_utxo(params, output)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eras::shelley::ShelleyTxOut;

    fn shelley_params() -> ProtocolParameters {
        ProtocolParameters::default() // min_utxo_value = Some(1_000_000)
    }

    fn alonzo_params() -> ProtocolParameters {
        ProtocolParameters::alonzo_defaults() // coins_per_utxo_byte = Some(4310)
    }

    fn make_shelley_output(lovelace: u64) -> MultiEraTxOut {
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: vec![0u8; 57],
            amount: lovelace,
        })
    }

    #[test]
    fn shelley_output_above_minimum() {
        let params = shelley_params();
        let output = make_shelley_output(2_000_000);
        assert!(validate_min_utxo(&params, &output).is_ok());
    }

    #[test]
    fn shelley_output_at_minimum() {
        let params = shelley_params();
        let output = make_shelley_output(1_000_000);
        assert!(validate_min_utxo(&params, &output).is_ok());
    }

    #[test]
    fn shelley_output_below_minimum() {
        let params = shelley_params();
        let output = make_shelley_output(500_000);
        let result = validate_min_utxo(&params, &output);
        assert!(matches!(result, Err(LedgerError::OutputTooSmall { .. })));
    }

    #[test]
    fn alonzo_output_sufficient() {
        let params = alonzo_params();
        // Create a reasonably-sized output
        let output = make_shelley_output(10_000_000);
        assert!(validate_min_utxo(&params, &output).is_ok());
    }

    #[test]
    fn alonzo_output_too_small() {
        let params = alonzo_params();
        // Very small amount that won't cover per-byte costing
        let output = make_shelley_output(100);
        let result = validate_min_utxo(&params, &output);
        assert!(matches!(result, Err(LedgerError::OutputTooSmall { .. })));
    }

    #[test]
    fn validate_all_outputs_pass() {
        let params = shelley_params();
        let outputs = vec![
            make_shelley_output(2_000_000),
            make_shelley_output(1_000_000),
        ];
        assert!(validate_all_outputs_min_utxo(&params, &outputs).is_ok());
    }

    #[test]
    fn validate_all_outputs_one_fails() {
        let params = shelley_params();
        let outputs = vec![
            make_shelley_output(2_000_000),
            make_shelley_output(100), // too small
        ];
        let result = validate_all_outputs_min_utxo(&params, &outputs);
        assert!(matches!(result, Err(LedgerError::OutputTooSmall { .. })));
    }
}
