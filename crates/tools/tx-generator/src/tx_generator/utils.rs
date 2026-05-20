//! Utility functions used across the transaction generator.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Utils.hs`.
//! Ports the pure value-splitting helpers and `TxIn` parser used by
//! the generator runtime before the remaining transaction-body builder
//! is wired.

use yggdrasil_ledger::ShelleyTxIn;

use crate::types::{Lovelace, PayWithChange};

/// Mirror of upstream `inputsToOutputsWithFee`.
pub fn inputs_to_outputs_with_fee(
    fee: Lovelace,
    count: usize,
    inputs: &[Lovelace],
) -> Result<Vec<Lovelace>, String> {
    if count == 0 {
        return Err("inputsToOutputsWithFee: output count must be positive".to_string());
    }

    let total = sum_lovelace(inputs).map_err(|err| format!("inputsToOutputsWithFee: {err}"))?;
    let available = total.checked_sub(u128::from(fee)).ok_or_else(|| {
        format!("inputsToOutputsWithFee: insufficient funds, inputs={inputs:?}, fee={fee}")
    })?;
    let count_u128 = count as u128;
    let out = available / count_u128;
    let rest = available % count_u128;

    let first = lovelace_from_u128(out + rest, "inputsToOutputsWithFee")?;
    let repeated = lovelace_from_u128(out, "inputsToOutputsWithFee")?;
    let mut outputs = Vec::with_capacity(count);
    outputs.push(first);
    outputs.extend(std::iter::repeat_n(repeated, count - 1));
    Ok(outputs)
}

/// Mirror of upstream `includeChange`.
pub fn include_change(
    fee: Lovelace,
    spend: &[Lovelace],
    have: &[Lovelace],
) -> Result<PayWithChange, String> {
    let have_total = sum_lovelace(have).map_err(|err| format!("includeChange: {err}"))?;
    let spend_total = sum_lovelace(spend).map_err(|err| format!("includeChange: {err}"))?;
    let needed = spend_total
        .checked_add(u128::from(fee))
        .ok_or_else(|| "includeChange: spend plus fee overflowed".to_string())?;

    match have_total.cmp(&needed) {
        std::cmp::Ordering::Greater => Ok(PayWithChange::PayWithChange(
            lovelace_from_u128(have_total - needed, "includeChange")?,
            spend.to_vec(),
        )),
        std::cmp::Ordering::Equal => Ok(PayWithChange::PayExact(spend.to_vec())),
        std::cmp::Ordering::Less => Err(format!(
            "includeChange: Bad transaction: insufficient funds\n   have: {have:?}\n  spend: {spend:?}\n    fee: {fee}"
        )),
    }
}

/// Mirror of upstream `mkTxIn`.
pub fn mk_tx_in(raw: &str) -> Result<ShelleyTxIn, String> {
    let (tx_id_hex, index) = raw
        .split_once('#')
        .ok_or_else(|| format!("mkTxIn: expected TXID#IX, got `{raw}`"))?;
    let tx_id = hex::decode(tx_id_hex)
        .map_err(|err| format!("mkTxIn: transaction id is not hex: {err}"))?;
    let transaction_id: [u8; 32] = tx_id
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("mkTxIn: expected 32-byte tx id, got {}", bytes.len()))?;
    let index = index
        .parse::<u16>()
        .map_err(|err| format!("mkTxIn: output index is not u16: {err}"))?;
    Ok(ShelleyTxIn {
        transaction_id,
        index,
    })
}

fn sum_lovelace(values: &[Lovelace]) -> Result<u128, String> {
    values.iter().try_fold(0_u128, |acc, value| {
        acc.checked_add(u128::from(*value))
            .ok_or_else(|| "lovelace sum overflowed".to_string())
    })
}

fn lovelace_from_u128(value: u128, context: &str) -> Result<Lovelace, String> {
    Lovelace::try_from(value).map_err(|_| format!("{context}: lovelace value exceeds u64"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_to_outputs_with_fee_puts_remainder_in_first_output() {
        assert_eq!(
            inputs_to_outputs_with_fee(10, 3, &[100]).expect("split"),
            vec![30, 30, 30]
        );
        assert_eq!(
            inputs_to_outputs_with_fee(0, 3, &[100]).expect("split"),
            vec![34, 33, 33]
        );
    }

    #[test]
    fn inputs_to_outputs_with_fee_rejects_zero_count_and_underflow() {
        assert_eq!(
            inputs_to_outputs_with_fee(0, 0, &[1]),
            Err("inputsToOutputsWithFee: output count must be positive".to_string())
        );
        assert_eq!(
            inputs_to_outputs_with_fee(2, 1, &[1]),
            Err("inputsToOutputsWithFee: insufficient funds, inputs=[1], fee=2".to_string())
        );
    }

    #[test]
    fn include_change_matches_pay_exact_and_change_cases() {
        assert_eq!(
            include_change(5, &[10, 15], &[30]).expect("change"),
            PayWithChange::PayExact(vec![10, 15])
        );
        assert_eq!(
            include_change(5, &[10], &[30]).expect("change"),
            PayWithChange::PayWithChange(15, vec![10])
        );
    }

    #[test]
    fn include_change_preserves_upstream_insufficient_funds_message() {
        let err = include_change(5, &[20], &[10]).expect_err("insufficient");

        assert!(err.contains("includeChange: Bad transaction: insufficient funds"));
        assert!(err.contains("have: [10]"));
        assert!(err.contains("spend: [20]"));
        assert!(err.contains("fee: 5"));
    }

    #[test]
    fn mk_tx_in_parses_hash_hash_index_text() {
        let tx_in = mk_tx_in("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f#42")
            .expect("tx in");

        assert_eq!(tx_in.transaction_id[0], 0x00);
        assert_eq!(tx_in.transaction_id[31], 0x1f);
        assert_eq!(tx_in.index, 42);
    }
}
