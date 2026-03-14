//! Multi-era UTxO set.
//!
//! Provides a generalized UTxO set that tracks outputs across all eras
//! (Shelley through Conway). The set is keyed by `ShelleyTxIn` (used
//! unchanged across all eras) and stores `MultiEraTxOut` values that
//! preserve full era-specific output data.
//!
//! Reference: `Cardano.Ledger.UTxO` — `UTxO`.

use std::collections::HashMap;

use crate::eras::allegra::AllegraTxBody;
use crate::eras::alonzo::{AlonzoTxBody, AlonzoTxOut};
use crate::eras::babbage::{BabbageTxBody, BabbageTxOut};
use crate::eras::conway::ConwayTxBody;
use crate::eras::mary::{MaryTxBody, MaryTxOut, MintAsset, MultiAsset, Value};
use crate::eras::shelley::{ShelleyTxBody, ShelleyTxIn, ShelleyTxOut};
use crate::{CborDecode, CborEncode, Decoder, Encoder};
use crate::error::LedgerError;

/// A transaction output that can represent any Cardano era.
///
/// Each variant preserves the full era-specific structure so that
/// round-trip serialization and era-aware queries remain possible.
///
/// Reference: `Cardano.Ledger.Core` — `TxOut`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultiEraTxOut {
    /// Shelley or Allegra output (address + coin).
    Shelley(ShelleyTxOut),
    /// Mary output (address + Value).
    Mary(MaryTxOut),
    /// Alonzo output (address + Value + optional datum hash).
    Alonzo(AlonzoTxOut),
    /// Babbage or Conway output (address + Value + optional datum + optional script ref).
    Babbage(BabbageTxOut),
}

impl CborEncode for MultiEraTxOut {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::Shelley(tx_out) => {
                enc.array(2).unsigned(0);
                tx_out.encode_cbor(enc);
            }
            Self::Mary(tx_out) => {
                enc.array(2).unsigned(1);
                tx_out.encode_cbor(enc);
            }
            Self::Alonzo(tx_out) => {
                enc.array(2).unsigned(2);
                tx_out.encode_cbor(enc);
            }
            Self::Babbage(tx_out) => {
                enc.array(2).unsigned(3);
                tx_out.encode_cbor(enc);
            }
        }
    }
}

impl CborDecode for MultiEraTxOut {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }

        match dec.unsigned()? {
            0 => Ok(Self::Shelley(ShelleyTxOut::decode_cbor(dec)?)),
            1 => Ok(Self::Mary(MaryTxOut::decode_cbor(dec)?)),
            2 => Ok(Self::Alonzo(AlonzoTxOut::decode_cbor(dec)?)),
            3 => Ok(Self::Babbage(BabbageTxOut::decode_cbor(dec)?)),
            tag => Err(LedgerError::CborInvalidAdditionalInfo(tag as u8)),
        }
    }
}

impl MultiEraTxOut {
    /// Returns the lovelace amount contained in this output.
    pub fn coin(&self) -> u64 {
        match self {
            Self::Shelley(o) => o.amount,
            Self::Mary(o) => o.amount.coin(),
            Self::Alonzo(o) => o.amount.coin(),
            Self::Babbage(o) => o.amount.coin(),
        }
    }

    /// Returns the full `Value` for this output.
    ///
    /// Shelley/Allegra outputs are promoted to `Value::Coin`.
    pub fn value(&self) -> Value {
        match self {
            Self::Shelley(o) => Value::Coin(o.amount),
            Self::Mary(o) => o.amount.clone(),
            Self::Alonzo(o) => o.amount.clone(),
            Self::Babbage(o) => o.amount.clone(),
        }
    }

    /// Returns the raw address bytes.
    pub fn address(&self) -> &[u8] {
        match self {
            Self::Shelley(o) => &o.address,
            Self::Mary(o) => &o.address,
            Self::Alonzo(o) => &o.address,
            Self::Babbage(o) => &o.address,
        }
    }
}

/// A multi-era UTxO set.
///
/// Uses the same `ShelleyTxIn` key type that is shared across all Cardano
/// eras. Stores `MultiEraTxOut` values so outputs from any era can coexist
/// in a single set (as happens after era transitions on the live chain).
///
/// Reference: `Cardano.Ledger.UTxO` — `UTxO`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MultiEraUtxo {
    entries: HashMap<ShelleyTxIn, MultiEraTxOut>,
}

impl CborEncode for MultiEraUtxo {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let mut entries: Vec<_> = self.entries.iter().collect();
        entries.sort_by(|(left, _), (right, _)| left.cmp(right));

        enc.array(entries.len() as u64);
        for (txin, txout) in entries {
            enc.array(2);
            txin.encode_cbor(enc);
            txout.encode_cbor(enc);
        }
    }
}

impl CborDecode for MultiEraUtxo {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        let mut entries = HashMap::with_capacity(len as usize);

        for _ in 0..len {
            let pair_len = dec.array()?;
            if pair_len != 2 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 2,
                    actual: pair_len as usize,
                });
            }

            let txin = ShelleyTxIn::decode_cbor(dec)?;
            let txout = MultiEraTxOut::decode_cbor(dec)?;
            entries.insert(txin, txout);
        }

        Ok(Self { entries })
    }
}

impl MultiEraUtxo {
    /// Creates an empty UTxO set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a multi-era UTxO entry.
    pub fn insert(&mut self, txin: ShelleyTxIn, txout: MultiEraTxOut) {
        self.entries.insert(txin, txout);
    }

    /// Inserts a Shelley-era UTxO entry (convenience for seeding).
    pub fn insert_shelley(&mut self, txin: ShelleyTxIn, txout: ShelleyTxOut) {
        self.entries.insert(txin, MultiEraTxOut::Shelley(txout));
    }

    /// Looks up a UTxO entry.
    pub fn get(&self, txin: &ShelleyTxIn) -> Option<&MultiEraTxOut> {
        self.entries.get(txin)
    }

    /// Iterates over all UTxO entries.
    pub fn iter(&self) -> impl Iterator<Item = (&ShelleyTxIn, &MultiEraTxOut)> {
        self.entries.iter()
    }

    /// Returns the number of entries in the UTxO set.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when the set is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    // -- Era-specific apply methods -----------------------------------------

    /// Applies a Shelley transaction body to this UTxO set.
    ///
    /// Validates: non-empty inputs/outputs, TTL, input existence, and
    /// coin value preservation.
    pub fn apply_shelley_tx(
        &mut self,
        tx_id: [u8; 32],
        body: &ShelleyTxBody,
        current_slot: u64,
    ) -> Result<(), LedgerError> {
        self.apply_shelley_tx_withdrawals(tx_id, body, current_slot, 0)
    }

    /// Applies a Shelley transaction body with a pre-validated withdrawal total.
    pub fn apply_shelley_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &ShelleyTxBody,
        current_slot: u64,
        withdrawal_total: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;

        // TTL check (mandatory in Shelley).
        if current_slot > body.ttl {
            return Err(LedgerError::TxExpired {
                ttl: body.ttl,
                slot: current_slot,
            });
        }

        // Input existence + consumed coin.
        let consumed = self.sum_consumed_coin(&body.inputs)?;

        // Value preservation (coin only).
        let produced: u64 = body
            .outputs
            .iter()
            .map(|o| o.amount)
            .fold(0u64, u64::saturating_add);
        check_coin_preservation(consumed.saturating_add(withdrawal_total), produced, body.fee)?;

        // State update.
        self.remove_inputs(&body.inputs);
        for (idx, output) in body.outputs.iter().enumerate() {
            let txin = ShelleyTxIn {
                transaction_id: tx_id,
                index: idx as u16,
            };
            self.entries
                .insert(txin, MultiEraTxOut::Shelley(output.clone()));
        }

        Ok(())
    }

    /// Applies an Allegra transaction body to this UTxO set.
    ///
    /// Like Shelley but TTL is optional and validity_interval_start is
    /// checked when present.
    pub fn apply_allegra_tx(
        &mut self,
        tx_id: [u8; 32],
        body: &AllegraTxBody,
        current_slot: u64,
    ) -> Result<(), LedgerError> {
        self.apply_allegra_tx_withdrawals(tx_id, body, current_slot, 0)
    }

    /// Applies an Allegra transaction body with a pre-validated withdrawal total.
    pub fn apply_allegra_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &AllegraTxBody,
        current_slot: u64,
        withdrawal_total: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;

        // TTL check (optional in Allegra).
        if let Some(ttl) = body.ttl {
            if current_slot > ttl {
                return Err(LedgerError::TxExpired {
                    ttl,
                    slot: current_slot,
                });
            }
        }

        // Validity interval start.
        if let Some(start) = body.validity_interval_start {
            if current_slot < start {
                return Err(LedgerError::TxNotYetValid {
                    start,
                    slot: current_slot,
                });
            }
        }

        let consumed = self.sum_consumed_coin(&body.inputs)?;
        let produced: u64 = body
            .outputs
            .iter()
            .map(|o| o.amount)
            .fold(0u64, u64::saturating_add);
        check_coin_preservation(consumed.saturating_add(withdrawal_total), produced, body.fee)?;

        self.remove_inputs(&body.inputs);
        for (idx, output) in body.outputs.iter().enumerate() {
            let txin = ShelleyTxIn {
                transaction_id: tx_id,
                index: idx as u16,
            };
            self.entries
                .insert(txin, MultiEraTxOut::Shelley(output.clone()));
        }

        Ok(())
    }

    /// Applies a Mary transaction body to this UTxO set.
    ///
    /// Validates coin preservation and multi-asset balance (produced
    /// multi-assets must equal consumed multi-assets plus minted/burned).
    pub fn apply_mary_tx(
        &mut self,
        tx_id: [u8; 32],
        body: &MaryTxBody,
        current_slot: u64,
    ) -> Result<(), LedgerError> {
        self.apply_mary_tx_withdrawals(tx_id, body, current_slot, 0)
    }

    /// Applies a Mary transaction body with a pre-validated withdrawal total.
    pub fn apply_mary_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &MaryTxBody,
        current_slot: u64,
        withdrawal_total: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;

        if let Some(ttl) = body.ttl {
            if current_slot > ttl {
                return Err(LedgerError::TxExpired {
                    ttl,
                    slot: current_slot,
                });
            }
        }

        if let Some(start) = body.validity_interval_start {
            if current_slot < start {
                return Err(LedgerError::TxNotYetValid {
                    start,
                    slot: current_slot,
                });
            }
        }

        let consumed = self.sum_consumed_coin(&body.inputs)?;
        let produced: u64 = body
            .outputs
            .iter()
            .map(|o| o.amount.coin())
            .fold(0u64, u64::saturating_add);
        check_coin_preservation(consumed.saturating_add(withdrawal_total), produced, body.fee)?;

        // Multi-asset preservation.
        let consumed_ma = self.sum_consumed_multi_asset(&body.inputs);
        let produced_ma = sum_output_multi_asset_mary(&body.outputs);
        check_multi_asset_preservation(&consumed_ma, &produced_ma, &body.mint)?;

        self.remove_inputs(&body.inputs);
        for (idx, output) in body.outputs.iter().enumerate() {
            let txin = ShelleyTxIn {
                transaction_id: tx_id,
                index: idx as u16,
            };
            self.entries
                .insert(txin, MultiEraTxOut::Mary(output.clone()));
        }

        Ok(())
    }

    /// Applies an Alonzo transaction body to this UTxO set.
    pub fn apply_alonzo_tx(
        &mut self,
        tx_id: [u8; 32],
        body: &AlonzoTxBody,
        current_slot: u64,
    ) -> Result<(), LedgerError> {
        self.apply_alonzo_tx_withdrawals(tx_id, body, current_slot, 0)
    }

    /// Applies an Alonzo transaction body with a pre-validated withdrawal total.
    pub fn apply_alonzo_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &AlonzoTxBody,
        current_slot: u64,
        withdrawal_total: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;

        if let Some(ttl) = body.ttl {
            if current_slot > ttl {
                return Err(LedgerError::TxExpired {
                    ttl,
                    slot: current_slot,
                });
            }
        }
        if let Some(start) = body.validity_interval_start {
            if current_slot < start {
                return Err(LedgerError::TxNotYetValid {
                    start,
                    slot: current_slot,
                });
            }
        }

        let consumed = self.sum_consumed_coin(&body.inputs)?;
        let produced: u64 = body
            .outputs
            .iter()
            .map(|o| o.amount.coin())
            .fold(0u64, u64::saturating_add);
        check_coin_preservation(consumed.saturating_add(withdrawal_total), produced, body.fee)?;

        let consumed_ma = self.sum_consumed_multi_asset(&body.inputs);
        let produced_ma = sum_output_multi_asset_alonzo(&body.outputs);
        check_multi_asset_preservation(&consumed_ma, &produced_ma, &body.mint)?;

        self.remove_inputs(&body.inputs);
        for (idx, output) in body.outputs.iter().enumerate() {
            let txin = ShelleyTxIn {
                transaction_id: tx_id,
                index: idx as u16,
            };
            self.entries
                .insert(txin, MultiEraTxOut::Alonzo(output.clone()));
        }

        Ok(())
    }

    /// Applies a Babbage transaction body to this UTxO set.
    pub fn apply_babbage_tx(
        &mut self,
        tx_id: [u8; 32],
        body: &BabbageTxBody,
        current_slot: u64,
    ) -> Result<(), LedgerError> {
        self.apply_babbage_tx_withdrawals(tx_id, body, current_slot, 0)
    }

    /// Applies a Babbage transaction body with a pre-validated withdrawal total.
    pub fn apply_babbage_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &BabbageTxBody,
        current_slot: u64,
        withdrawal_total: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;

        if let Some(ttl) = body.ttl {
            if current_slot > ttl {
                return Err(LedgerError::TxExpired {
                    ttl,
                    slot: current_slot,
                });
            }
        }
        if let Some(start) = body.validity_interval_start {
            if current_slot < start {
                return Err(LedgerError::TxNotYetValid {
                    start,
                    slot: current_slot,
                });
            }
        }

        let consumed = self.sum_consumed_coin(&body.inputs)?;
        let produced: u64 = body
            .outputs
            .iter()
            .map(|o| o.amount.coin())
            .fold(0u64, u64::saturating_add);
        check_coin_preservation(consumed.saturating_add(withdrawal_total), produced, body.fee)?;

        let consumed_ma = self.sum_consumed_multi_asset(&body.inputs);
        let produced_ma = sum_output_multi_asset_babbage(&body.outputs);
        check_multi_asset_preservation(&consumed_ma, &produced_ma, &body.mint)?;

        self.remove_inputs(&body.inputs);
        for (idx, output) in body.outputs.iter().enumerate() {
            let txin = ShelleyTxIn {
                transaction_id: tx_id,
                index: idx as u16,
            };
            self.entries
                .insert(txin, MultiEraTxOut::Babbage(output.clone()));
        }

        Ok(())
    }

    /// Applies a Conway transaction body to this UTxO set.
    ///
    /// Conway uses the same output type as Babbage (`BabbageTxOut`).
    pub fn apply_conway_tx(
        &mut self,
        tx_id: [u8; 32],
        body: &ConwayTxBody,
        current_slot: u64,
    ) -> Result<(), LedgerError> {
        self.apply_conway_tx_withdrawals(tx_id, body, current_slot, 0)
    }

    /// Applies a Conway transaction body with a pre-validated withdrawal total.
    pub fn apply_conway_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &ConwayTxBody,
        current_slot: u64,
        withdrawal_total: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;

        if let Some(ttl) = body.ttl {
            if current_slot > ttl {
                return Err(LedgerError::TxExpired {
                    ttl,
                    slot: current_slot,
                });
            }
        }
        if let Some(start) = body.validity_interval_start {
            if current_slot < start {
                return Err(LedgerError::TxNotYetValid {
                    start,
                    slot: current_slot,
                });
            }
        }

        let consumed = self.sum_consumed_coin(&body.inputs)?;
        let produced: u64 = body
            .outputs
            .iter()
            .map(|o| o.amount.coin())
            .fold(0u64, u64::saturating_add);
        check_coin_preservation(consumed.saturating_add(withdrawal_total), produced, body.fee)?;

        let consumed_ma = self.sum_consumed_multi_asset(&body.inputs);
        let produced_ma = sum_output_multi_asset_babbage(&body.outputs);
        check_multi_asset_preservation(&consumed_ma, &produced_ma, &body.mint)?;

        self.remove_inputs(&body.inputs);
        for (idx, output) in body.outputs.iter().enumerate() {
            let txin = ShelleyTxIn {
                transaction_id: tx_id,
                index: idx as u16,
            };
            self.entries
                .insert(txin, MultiEraTxOut::Babbage(output.clone()));
        }

        Ok(())
    }

    // -- Private helpers ----------------------------------------------------

    /// Sum the coin (lovelace) of all consumed inputs.
    fn sum_consumed_coin(&self, inputs: &[ShelleyTxIn]) -> Result<u64, LedgerError> {
        let mut total: u64 = 0;
        for input in inputs {
            let entry = self.entries.get(input).ok_or(LedgerError::InputNotInUtxo)?;
            total = total.saturating_add(entry.coin());
        }
        Ok(total)
    }

    /// Sum the multi-asset bundle of all consumed inputs.
    fn sum_consumed_multi_asset(&self, inputs: &[ShelleyTxIn]) -> MultiAsset {
        let mut total = MultiAsset::new();
        for input in inputs {
            if let Some(entry) = self.entries.get(input) {
                let value = entry.value();
                if let Some(ma) = value.multi_asset() {
                    add_multi_asset(&mut total, ma);
                }
            }
        }
        total
    }

    /// Remove all inputs from the UTxO set.
    fn remove_inputs(&mut self, inputs: &[ShelleyTxIn]) {
        for input in inputs {
            self.entries.remove(input);
        }
    }
}

// ---------------------------------------------------------------------------
// Free-standing helpers
// ---------------------------------------------------------------------------

/// Validates that inputs and outputs are non-empty.
fn validate_nonempty<I, O>(inputs: &[I], outputs: &[O]) -> Result<(), LedgerError> {
    if inputs.is_empty() {
        return Err(LedgerError::NoInputs);
    }
    if outputs.is_empty() {
        return Err(LedgerError::NoOutputs);
    }
    Ok(())
}

/// Checks coin-level value preservation: `consumed == produced + fee`.
fn check_coin_preservation(consumed: u64, produced: u64, fee: u64) -> Result<(), LedgerError> {
    if consumed != produced.saturating_add(fee) {
        return Err(LedgerError::ValueNotPreserved {
            consumed,
            produced,
            fee,
        });
    }
    Ok(())
}

/// Sums the multi-asset bundle across Mary-era outputs.
fn sum_output_multi_asset_mary(outputs: &[MaryTxOut]) -> MultiAsset {
    let mut total = MultiAsset::new();
    for output in outputs {
        if let Some(ma) = output.amount.multi_asset() {
            add_multi_asset(&mut total, ma);
        }
    }
    total
}

/// Sums the multi-asset bundle across Alonzo-era outputs.
fn sum_output_multi_asset_alonzo(outputs: &[AlonzoTxOut]) -> MultiAsset {
    let mut total = MultiAsset::new();
    for output in outputs {
        if let Some(ma) = output.amount.multi_asset() {
            add_multi_asset(&mut total, ma);
        }
    }
    total
}

/// Sums the multi-asset bundle across Babbage/Conway-era outputs.
fn sum_output_multi_asset_babbage(outputs: &[BabbageTxOut]) -> MultiAsset {
    let mut total = MultiAsset::new();
    for output in outputs {
        if let Some(ma) = output.amount.multi_asset() {
            add_multi_asset(&mut total, ma);
        }
    }
    total
}

/// Adds all entries from `src` into `dst`.
fn add_multi_asset(dst: &mut MultiAsset, src: &MultiAsset) {
    for (policy, assets) in src {
        let entry = dst.entry(*policy).or_default();
        for (name, qty) in assets {
            *entry.entry(name.clone()).or_insert(0) += qty;
        }
    }
}

/// Checks that consumed multi-assets plus minted/burned equals produced.
///
/// For each policy+asset: `consumed + minted == produced`.
/// `MintAsset` uses `i64` so negative values represent burning.
fn check_multi_asset_preservation(
    consumed: &MultiAsset,
    produced: &MultiAsset,
    mint: &Option<MintAsset>,
) -> Result<(), LedgerError> {
    // Merge consumed + mint into expected.
    let mut expected = consumed.clone();
    if let Some(minted) = mint {
        for (policy, assets) in minted {
            let entry = expected.entry(*policy).or_default();
            for (name, qty) in assets {
                // MintAsset has i64 quantities; convert to u64 for balance.
                let current = entry.entry(name.clone()).or_insert(0);
                // Apply signed mint: positive = mint, negative = burn.
                if *qty >= 0 {
                    *current = current.saturating_add(*qty as u64);
                } else {
                    *current = current.saturating_sub(qty.unsigned_abs());
                }
            }
        }
    }

    // Compare expected vs produced for every policy+asset that appears
    // in either map.
    let mut all_policies: std::collections::BTreeSet<&[u8; 28]> =
        std::collections::BTreeSet::new();
    for k in expected.keys() {
        all_policies.insert(k);
    }
    for k in produced.keys() {
        all_policies.insert(k);
    }

    for policy in all_policies {
        let expected_assets = expected.get(policy);
        let produced_assets = produced.get(policy);

        let mut all_names: std::collections::BTreeSet<&Vec<u8>> =
            std::collections::BTreeSet::new();
        if let Some(ea) = expected_assets {
            for k in ea.keys() {
                all_names.insert(k);
            }
        }
        if let Some(pa) = produced_assets {
            for k in pa.keys() {
                all_names.insert(k);
            }
        }

        for name in all_names {
            let exp = expected_assets.and_then(|a| a.get(name)).copied().unwrap_or(0);
            let prod = produced_assets.and_then(|a| a.get(name)).copied().unwrap_or(0);
            if exp != prod {
                return Err(LedgerError::MultiAssetNotPreserved {
                    policy: *policy,
                    asset_name: name.clone(),
                    expected: exp,
                    produced: prod,
                });
            }
        }
    }

    Ok(())
}
