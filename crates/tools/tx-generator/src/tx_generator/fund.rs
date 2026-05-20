//! Funds available for transaction construction.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Fund.hs`.
//! Ports the `Fund`/`FundInEra` carrier and accessors needed by the
//! wallet and generator runtime slices.

use std::cmp::Ordering;

use crate::types::{AnyCardanoEra, ExecutionUnits, Lovelace};
use yggdrasil_ledger::PlutusData;

/// Rust carrier for upstream `PlutusScriptWitness ...`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptWitnessForSpending {
    /// Upstream Plutus language tag rendered for diagnostics.
    pub language: String,
    /// Serialised Plutus script bytes.
    pub script_bytes: Vec<u8>,
    /// Datum supplied with the spending witness.
    pub datum: PlutusData,
    /// Redeemer supplied with the spending witness.
    pub redeemer: PlutusData,
    /// Execution units stated for the script witness.
    pub execution_units: ExecutionUnits,
}

/// Mirror of upstream `Witness WitCtxTxIn era` for this pure-Rust slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FundWitness {
    /// Upstream `KeyWitness KeyWitnessForSpending`.
    KeyWitnessForSpending,
    /// Upstream `ScriptWitness ScriptWitnessForSpending`.
    ScriptWitness(ScriptWitnessForSpending),
}

/// Mirror of upstream `FundInEra era`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FundInEra {
    /// Upstream `_fundTxIn`.
    pub fund_tx_in: String,
    /// Upstream `_fundWitness`.
    pub fund_witness: FundWitness,
    /// Upstream `_fundVal`, restricted to lovelace in this slice.
    pub fund_val: Lovelace,
    /// Upstream `_fundSigningKey`.
    pub fund_signing_key: Option<String>,
}

/// Mirror of upstream heterogenous `Fund`.
#[derive(Clone, Debug, Eq)]
pub struct Fund {
    /// Era associated with the spendable output.
    pub era: AnyCardanoEra,
    /// Transaction input rendered in upstream `TxIn` text form.
    pub tx_in: String,
    /// Lovelace amount carried by the fund.
    pub lovelace: Lovelace,
    /// Signing key name that can spend this fund.
    pub key_name: String,
    /// Witness required to spend this fund.
    pub witness: FundWitness,
    /// Signing key name, absent for script-witnessed funds.
    pub signing_key: Option<String>,
}

impl Fund {
    /// Construct a key-witnessed fund, matching `addFundToWallet`.
    pub fn key_fund(
        era: AnyCardanoEra,
        tx_in: impl Into<String>,
        lovelace: Lovelace,
        key_name: impl Into<String>,
    ) -> Self {
        Self {
            era,
            tx_in: tx_in.into(),
            lovelace,
            key_name: key_name.into(),
            witness: FundWitness::KeyWitnessForSpending,
            signing_key: None,
        }
        .with_key_name_as_signing_key()
    }

    /// Construct a script-witnessed fund, matching `mkUTxOScript`.
    pub fn script_fund(
        era: AnyCardanoEra,
        tx_in: impl Into<String>,
        lovelace: Lovelace,
        witness: FundWitness,
    ) -> Self {
        Self {
            era,
            tx_in: tx_in.into(),
            lovelace,
            key_name: String::new(),
            witness,
            signing_key: None,
        }
    }

    /// Convert to the era-specific carrier shape.
    pub fn fund_in_era(&self) -> FundInEra {
        FundInEra {
            fund_tx_in: self.tx_in.clone(),
            fund_witness: self.witness.clone(),
            fund_val: self.lovelace,
            fund_signing_key: self.signing_key.clone(),
        }
    }

    fn with_key_name_as_signing_key(mut self) -> Self {
        self.signing_key = Some(self.key_name.clone());
        self
    }
}

impl PartialEq for Fund {
    fn eq(&self, other: &Self) -> bool {
        get_fund_tx_in(self) == get_fund_tx_in(other)
    }
}

impl Ord for Fund {
    fn cmp(&self, other: &Self) -> Ordering {
        get_fund_tx_in(self).cmp(get_fund_tx_in(other))
    }
}

impl PartialOrd for Fund {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Mirror of upstream `getFundTxIn`.
pub fn get_fund_tx_in(fund: &Fund) -> &str {
    &fund.tx_in
}

/// Mirror of upstream `getFundKey`.
pub fn get_fund_key(fund: &Fund) -> Option<&str> {
    fund.signing_key.as_deref()
}

/// Mirror of upstream `getFundCoin`.
pub fn get_fund_coin(fund: &Fund) -> Lovelace {
    fund.lovelace
}

/// Mirror of upstream `getFundWitness`.
pub fn get_fund_witness(era: AnyCardanoEra, fund: &Fund) -> Result<FundWitness, String> {
    if era == fund.era {
        Ok(fund.witness.clone())
    } else {
        Err("getFundWitness: era mismatch".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_and_order_are_tx_in_only() {
        let a = Fund::key_fund(AnyCardanoEra::Conway, "abc#0", 1, "key-a");
        let b = Fund::key_fund(AnyCardanoEra::Conway, "abc#0", 99, "key-b");
        let c = Fund::key_fund(AnyCardanoEra::Conway, "def#0", 1, "key-c");

        assert_eq!(a, b);
        assert!(a < c);
    }

    #[test]
    fn accessors_match_upstream_field_views() {
        let fund = Fund::key_fund(AnyCardanoEra::Conway, "abc#0", 12, "key");

        assert_eq!(get_fund_tx_in(&fund), "abc#0");
        assert_eq!(get_fund_key(&fund), Some("key"));
        assert_eq!(get_fund_coin(&fund), 12);
        assert_eq!(
            get_fund_witness(AnyCardanoEra::Conway, &fund),
            Ok(FundWitness::KeyWitnessForSpending)
        );
        assert_eq!(
            get_fund_witness(AnyCardanoEra::Babbage, &fund),
            Err("getFundWitness: era mismatch".to_string())
        );
        assert_eq!(fund.fund_in_era().fund_signing_key, Some("key".to_string()));
    }

    #[test]
    fn script_fund_has_witness_and_no_signing_key() {
        let witness = ScriptWitnessForSpending {
            language: "PlutusV2".to_string(),
            script_bytes: vec![1, 2, 3],
            datum: PlutusData::integer(0),
            redeemer: PlutusData::integer(1),
            execution_units: ExecutionUnits {
                execution_steps: 3,
                execution_memory: 2,
            },
        };
        let fund = Fund::script_fund(
            AnyCardanoEra::Conway,
            "abc#0",
            12,
            FundWitness::ScriptWitness(witness.clone()),
        );

        assert_eq!(get_fund_key(&fund), None);
        assert_eq!(
            get_fund_witness(AnyCardanoEra::Conway, &fund),
            Ok(FundWitness::ScriptWitness(witness))
        );
        assert_eq!(fund.fund_in_era().fund_signing_key, None);
    }
}
