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
    // Use the inner era-specific output size, matching upstream `sizedSize`
    // which measures the raw TxOut CBOR encoding without the MultiEraTxOut
    // enum wrapper.
    let serialized_size = output.inner_cbor_size();
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

/// Validates that the serialized value of each output does not exceed
/// `max_val_size`.
///
/// Returns `Err(LedgerError::OutputTooBig)` on the first violation.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `validateOutputTooBigUTxO`.
pub fn validate_output_not_too_big(
    params: &ProtocolParameters,
    outputs: &[MultiEraTxOut],
) -> Result<(), LedgerError> {
    let max_val = match params.max_val_size {
        Some(m) => m as usize,
        None => return Ok(()),
    };
    for output in outputs {
        let val_bytes = output.value().to_cbor_bytes();
        if val_bytes.len() > max_val {
            return Err(LedgerError::OutputTooBig {
                actual: val_bytes.len(),
                max: max_val,
            });
        }
    }
    Ok(())
}

/// Validates that no transaction output carries a multi-asset entry with
/// a zero quantity.
///
/// Zero-valued tokens waste UTxO space and are disallowed by the formal
/// spec from Mary onward. Upstream `nonAdaValue` filtering ensures zero
/// entries are rejected.
///
/// Reference: `Cardano.Ledger.Mary.Value` — non-zero invariant on `MaryValue`.
pub fn validate_no_zero_valued_multi_asset(
    outputs: &[MultiEraTxOut],
) -> Result<(), LedgerError> {
    use crate::eras::mary::Value;
    for output in outputs {
        let val = output.value();
        match &val {
            Value::CoinAndAssets(_, ma) => {
                for (policy_id, assets) in ma {
                    for (asset_name, &quantity) in assets {
                        if quantity == 0 {
                            return Err(LedgerError::ZeroValuedMultiAssetOutput {
                                policy_id: *policy_id,
                                asset_name: asset_name.clone(),
                            });
                        }
                    }
                }
            }
            Value::Coin(_) => {}
        }
    }
    Ok(())
}

/// Maximum allowed size of serialized attributes in a Byron bootstrap
/// address appearing in a Shelley+ transaction output.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `validateOutputBootAddrAttrsTooBig`.
const BOOT_ADDR_ATTRS_MAX: usize = 64;

/// Validates that no output carries a Byron bootstrap address whose
/// serialized attributes exceed 64 bytes.
///
/// Returns `Err(LedgerError::OutputBootAddrAttrsTooBig)` on the first violation.
///
/// Upstream restricts bootstrap address attribute size to prevent
/// unbounded growth in the UTxO set. The limit is on the CBOR-serialized
/// attributes map inside the address payload, not the address itself.
pub fn validate_output_boot_addr_attrs(
    outputs: &[MultiEraTxOut],
) -> Result<(), LedgerError> {
    for output in outputs {
        let addr_bytes = output.address();
        if let Some(attrs_size) = byron_addr_attrs_size(addr_bytes) {
            if attrs_size > BOOT_ADDR_ATTRS_MAX {
                return Err(LedgerError::OutputBootAddrAttrsTooBig {
                    size: attrs_size,
                });
            }
        }
    }
    Ok(())
}

/// Extracts the size of the serialized attributes blob from a Byron
/// bootstrap address, or `None` if the address is not a Byron address
/// or cannot be parsed.
///
/// Byron address on-wire format: `[TAG(24) BYTES(payload), checksum]`
/// where `payload` decodes as `[root, attributes_bytes, addr_type]`.
fn byron_addr_attrs_size(raw: &[u8]) -> Option<usize> {
    if raw.is_empty() {
        return None;
    }
    // Byron (legacy) addresses start with byte >= 0x82 (CBOR array(2))
    // Shelley addresses have high nibble 0..7 for type.
    let header = raw[0];
    // Byron addresses are detected by the CBOR array-of-2 wrapper.
    // Quick heuristic: if header byte ≤ 0x07 (type 0–7 Shelley) it's not Byron.
    if header & 0xF0 != 0x80 {
        // Not 0x80..0x8F — not a 2-element CBOR array start
        return None;
    }
    // Attempt minimal CBOR parse: array(2) → tag(24) → bstr(payload)
    let mut dec = crate::cbor::Decoder::new(raw);
    let arr_len = dec.array().ok()?;
    if arr_len != 2 {
        return None;
    }
    let tag = dec.tag().ok()?;
    if tag != 24 {
        return None;
    }
    let payload = dec.bytes().ok()?;
    // payload: CBOR array(3) = [root_hash, attributes_bytes, addr_type]
    let mut pdec = crate::cbor::Decoder::new(payload);
    let inner_len = pdec.array().ok()?;
    if inner_len < 2 {
        return None;
    }
    // Skip root hash (tag(24) bstr)
    let _ = pdec.tag().ok();
    let _ = pdec.bytes().ok()?;
    // Next element is the serialized attributes map — measure its encoded size.
    let attrs_start = pdec.position();
    // Skip one CBOR item (the attributes map)
    pdec.skip().ok()?;
    let attrs_end = pdec.position();
    Some(attrs_end - attrs_start)
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

    // ----- Zero-valued multi-asset output tests -----

    fn make_mary_output_with_assets(lovelace: u64, assets: Vec<([u8; 28], Vec<u8>, u64)>) -> MultiEraTxOut {
        use crate::eras::mary::{MaryTxOut, Value, MultiAsset};
        use std::collections::BTreeMap;

        let mut ma: MultiAsset = BTreeMap::new();
        for (policy, asset_name, qty) in assets {
            ma.entry(policy).or_default().insert(asset_name, qty);
        }
        MultiEraTxOut::Mary(MaryTxOut {
            address: vec![0u8; 57],
            amount: Value::CoinAndAssets(lovelace, ma),
        })
    }

    #[test]
    fn zero_valued_multi_asset_rejected() {
        let output = make_mary_output_with_assets(
            2_000_000,
            vec![([0xAA; 28], vec![0x01], 0)], // zero quantity
        );
        let result = validate_no_zero_valued_multi_asset(&[output]);
        assert!(matches!(result, Err(LedgerError::ZeroValuedMultiAssetOutput { .. })));
    }

    #[test]
    fn nonzero_multi_asset_accepted() {
        let output = make_mary_output_with_assets(
            2_000_000,
            vec![([0xAA; 28], vec![0x01], 100)],
        );
        assert!(validate_no_zero_valued_multi_asset(&[output]).is_ok());
    }

    #[test]
    fn pure_coin_output_no_multi_asset_check() {
        let output = make_shelley_output(2_000_000);
        assert!(validate_no_zero_valued_multi_asset(&[output]).is_ok());
    }
}
