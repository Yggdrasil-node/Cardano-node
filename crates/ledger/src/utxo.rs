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

/// Maximum total reference-script size (in bytes) per transaction.
///
/// Hardcoded upstream constant (not a governable protocol parameter).
/// Reference: `Cardano.Ledger.Conway.PParams` — `ppMaxRefScriptSizePerTxG`.
pub const MAX_REF_SCRIPT_SIZE_PER_TX: usize = 204_800;
use crate::eras::alonzo::{AlonzoTxBody, AlonzoTxOut};
use crate::eras::babbage::{BabbageTxBody, BabbageTxOut};
use crate::eras::byron::ByronTx;
use crate::eras::conway::ConwayTxBody;
use crate::eras::mary::{MaryTxBody, MaryTxOut, MintAsset, MultiAsset, Value};
use crate::eras::shelley::{ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, ShelleyUtxo};
use crate::plutus::ScriptRef;
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

    /// Returns the inline reference script, if present (Babbage+ only).
    pub fn script_ref(&self) -> Option<&ScriptRef> {
        match self {
            Self::Babbage(o) => o.script_ref.as_ref(),
            _ => None,
        }
    }

    /// Returns the datum hash attached to this output, if any.
    ///
    /// - Shelley/Mary: always `None` (no datum support).
    /// - Alonzo: returns `AlonzoTxOut.datum_hash`.
    /// - Babbage: returns `Some(h)` for `DatumOption::Hash(h)`, `None` for
    ///   inline datums and absent datum options.
    ///
    /// This matches upstream `dataHashTxOutL` which returns `SNothing` for
    /// inline datums (they are not counted as "datum hash" outputs).
    pub fn datum_hash(&self) -> Option<[u8; 32]> {
        match self {
            Self::Shelley(_) | Self::Mary(_) => None,
            Self::Alonzo(o) => o.datum_hash,
            Self::Babbage(o) => match &o.datum_option {
                Some(crate::eras::babbage::DatumOption::Hash(h)) => Some(*h),
                _ => None,
            },
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
        /// Returns the consumed and produced UTxO entries for a transaction.
        ///
        /// - `inputs`: the set of inputs consumed by the transaction
        /// - `outputs`: the set of outputs produced by the transaction (as (ShelleyTxIn, MultiEraTxOut) pairs)
        ///
        /// Returns a tuple: (consumed_utxos, produced_utxos)
        pub fn utxo_delta_for_tx<'a>(
            &'a self,
            inputs: &[ShelleyTxIn],
            outputs: &'a [(ShelleyTxIn, MultiEraTxOut)],
        ) -> (Vec<(&'a ShelleyTxIn, &'a MultiEraTxOut)>, Vec<(&'a ShelleyTxIn, &'a MultiEraTxOut)>) {
            let consumed: Vec<_> = inputs
                .iter()
                .filter_map(|txin| self.entries.get_key_value(txin))
                .collect();
            let produced: Vec<_> = outputs.iter().map(|(k, v)| (k, v)).collect();
            (consumed, produced)
        }
    /// Creates an empty UTxO set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuilds a multi-era UTxO set from a Shelley-only UTxO snapshot.
    pub fn from_shelley_utxo(utxo: &ShelleyUtxo) -> Self {
        let mut entries = HashMap::with_capacity(utxo.len());
        for (txin, txout) in utxo.iter() {
            entries.insert(txin.clone(), MultiEraTxOut::Shelley(txout.clone()));
        }
        Self { entries }
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

    /// Applies a Byron transaction to this UTxO set.
    ///
    /// Byron transactions have no TTL, no certificates, no withdrawals,
    /// and no multi-asset values.  The fee is implicit: `consumed - produced`.
    /// Validates: non-empty inputs/outputs, input existence, and
    /// non-negative implicit fee (`consumed >= produced`).
    ///
    /// Byron `ByronTxIn` (u32 index) is converted to `ShelleyTxIn` (u16
    /// index) for unified UTxO set storage.  Byron outputs are stored as
    /// `MultiEraTxOut::Shelley` since their shape is identical.
    ///
    /// Reference: `Cardano.Chain.UTxO.TxValidation` from `cardano-ledger-byron`.
    pub fn apply_byron_tx(
        &mut self,
        tx: &ByronTx,
    ) -> Result<(), LedgerError> {
        if tx.inputs.is_empty() {
            return Err(LedgerError::NoInputs);
        }
        if tx.outputs.is_empty() {
            return Err(LedgerError::NoOutputs);
        }

        // Convert Byron inputs to Shelley-compatible keys and validate existence.
        let shelley_inputs: Vec<ShelleyTxIn> = tx.inputs.iter().map(|i| ShelleyTxIn {
            transaction_id: i.txid,
            index: i.index as u16,
        }).collect();

        let consumed = self.sum_consumed_coin(&shelley_inputs)?;
        let produced: u64 = tx.outputs
            .iter()
            .map(|o| o.amount)
            .fold(0u64, u64::saturating_add);

        // Byron fee is implicit: consumed - produced.  Must be non-negative.
        if consumed < produced {
            return Err(LedgerError::ValueNotPreserved {
                consumed,
                produced,
                fee: 0,
            });
        }

        // State update: remove consumed inputs, insert produced outputs.
        self.remove_inputs(&shelley_inputs);
        let tx_id = tx.tx_id();
        for (idx, output) in tx.outputs.iter().enumerate() {
            let txin = ShelleyTxIn {
                transaction_id: tx_id,
                index: idx as u16,
            };
            self.entries.insert(txin, MultiEraTxOut::Shelley(ShelleyTxOut {
                address: output.address.clone(),
                amount: output.amount,
            }));
        }

        Ok(())
    }

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
        self.apply_shelley_tx_withdrawals(tx_id, body, current_slot, 0, 0, 0)
    }

    /// Applies a Shelley transaction body with a pre-validated withdrawal total.
    pub fn apply_shelley_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &ShelleyTxBody,
        current_slot: u64,
        withdrawal_total: u64,
        deposits: u64,
        refunds: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;
        validate_no_duplicate_inputs(&body.inputs)?;

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
        // consumed + withdrawals + refunds = produced + fee + deposits
        let produced: u64 = body
            .outputs
            .iter()
            .map(|o| o.amount)
            .fold(0u64, u64::saturating_add);
        check_coin_preservation(
            consumed.saturating_add(withdrawal_total).saturating_add(refunds),
            produced,
            body.fee.saturating_add(deposits),
        )?;

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
        self.apply_allegra_tx_withdrawals(tx_id, body, current_slot, 0, 0, 0)
    }

    /// Applies an Allegra transaction body with a pre-validated withdrawal total.
    pub fn apply_allegra_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &AllegraTxBody,
        current_slot: u64,
        withdrawal_total: u64,
        deposits: u64,
        refunds: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;
        validate_no_duplicate_inputs(&body.inputs)?;

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
        check_coin_preservation(
            consumed.saturating_add(withdrawal_total).saturating_add(refunds),
            produced,
            body.fee.saturating_add(deposits),
        )?;

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
        self.apply_mary_tx_withdrawals(tx_id, body, current_slot, 0, 0, 0)
    }

    /// Applies a Mary transaction body with a pre-validated withdrawal total.
    pub fn apply_mary_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &MaryTxBody,
        current_slot: u64,
        withdrawal_total: u64,
        deposits: u64,
        refunds: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;
        validate_no_duplicate_inputs(&body.inputs)?;

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
        check_coin_preservation(
            consumed.saturating_add(withdrawal_total).saturating_add(refunds),
            produced,
            body.fee.saturating_add(deposits),
        )?;

        // Multi-asset preservation.
        validate_no_ada_in_mint(&body.mint)?;
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
        self.apply_alonzo_tx_withdrawals(tx_id, body, current_slot, 0, 0, 0)
    }

    /// Applies an Alonzo transaction body with a pre-validated withdrawal total.
    pub fn apply_alonzo_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &AlonzoTxBody,
        current_slot: u64,
        withdrawal_total: u64,
        deposits: u64,
        refunds: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;
        validate_no_duplicate_inputs(&body.inputs)?;

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
        check_coin_preservation(
            consumed.saturating_add(withdrawal_total).saturating_add(refunds),
            produced,
            body.fee.saturating_add(deposits),
        )?;

        validate_no_ada_in_mint(&body.mint)?;
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
        self.apply_babbage_tx_withdrawals(tx_id, body, current_slot, 0, 0, 0)
    }

    /// Applies a Babbage transaction body with a pre-validated withdrawal total.
    pub fn apply_babbage_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &BabbageTxBody,
        current_slot: u64,
        withdrawal_total: u64,
        deposits: u64,
        refunds: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;
        validate_no_duplicate_inputs(&body.inputs)?;

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
        check_coin_preservation(
            consumed.saturating_add(withdrawal_total).saturating_add(refunds),
            produced,
            body.fee.saturating_add(deposits),
        )?;

        validate_no_ada_in_mint(&body.mint)?;
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
        self.apply_conway_tx_withdrawals(tx_id, body, current_slot, 0, 0, 0)
    }

    /// Applies a Conway transaction body with a pre-validated withdrawal total.
    pub fn apply_conway_tx_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &ConwayTxBody,
        current_slot: u64,
        withdrawal_total: u64,
        deposits: u64,
        refunds: u64,
    ) -> Result<(), LedgerError> {
        validate_nonempty(&body.inputs, &body.outputs)?;
        validate_no_duplicate_inputs(&body.inputs)?;

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
        // Conway UTXO rule: treasury_donation is on the "produced" side of
        // the value preservation equation.
        //
        // Reference: `Cardano.Ledger.Conway.Rules.Utxo` — produced:
        //   `balance (outs txb) + txfee txb + deposits + txDonation txb`
        let fee_plus_deposits_donation = body
            .fee
            .saturating_add(deposits)
            .saturating_add(body.treasury_donation.unwrap_or(0));
        check_coin_preservation(
            consumed.saturating_add(withdrawal_total).saturating_add(refunds),
            produced,
            fee_plus_deposits_donation,
        )?;

        validate_no_ada_in_mint(&body.mint)?;
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

    /// Validates that every reference input exists in the UTxO set.
    ///
    /// Reference inputs (CIP-31, Babbage+) are read but never consumed.
    /// This check mirrors the upstream UTXO rule: all referenced inputs must
    /// be present in the UTxO.
    pub fn validate_reference_inputs(
        &self,
        ref_inputs: &[ShelleyTxIn],
    ) -> Result<(), LedgerError> {
        for input in ref_inputs {
            if !self.entries.contains_key(input) {
                return Err(LedgerError::ReferenceInputNotInUtxo);
            }
        }
        Ok(())
    }

    /// Validates that spending inputs and reference inputs are disjoint.
    ///
    /// Babbage+ UTXO rule: the sets of spending and reference inputs must not
    /// overlap.  Upstream: `disjoint txins refInputs`.
    pub fn validate_reference_input_disjointness(
        inputs: &[ShelleyTxIn],
        ref_inputs: &[ShelleyTxIn],
    ) -> Result<(), LedgerError> {
        for ri in ref_inputs {
            for inp in inputs {
                if ri == inp {
                    return Err(LedgerError::ReferenceInputContention);
                }
            }
        }
        Ok(())
    }

    /// Computes the total serialized reference script size across all given
    /// UTxO entries referenced by a transaction (both spending and reference
    /// inputs).
    ///
    /// The upstream function `txNonDistinctRefScriptsSize` sums sizes without
    /// deduplication — if two inputs reference the same UTxO, the script is
    /// counted twice.
    ///
    /// Reference: `Cardano.Ledger.Conway.UTxO` — `txNonDistinctRefScriptsSize`.
    pub fn total_ref_scripts_size(
        &self,
        spending_inputs: &[ShelleyTxIn],
        ref_inputs: Option<&[ShelleyTxIn]>,
    ) -> usize {
        let mut total = 0usize;
        for input in spending_inputs.iter().chain(ref_inputs.unwrap_or(&[]).iter()) {
            if let Some(txout) = self.entries.get(input) {
                if let Some(sr) = txout.script_ref() {
                    total = total.saturating_add(sr.0.binary_size());
                }
            }
        }
        total
    }

    /// Validates that the total reference-script size for a transaction does
    /// not exceed `MAX_REF_SCRIPT_SIZE_PER_TX` (Conway+ LEDGER rule).
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Ledger` —
    /// `ConwayTxRefScriptsSizeTooBig`.
    pub fn validate_tx_ref_scripts_size(
        &self,
        spending_inputs: &[ShelleyTxIn],
        ref_inputs: Option<&[ShelleyTxIn]>,
    ) -> Result<(), LedgerError> {
        let actual = self.total_ref_scripts_size(spending_inputs, ref_inputs);
        if actual > MAX_REF_SCRIPT_SIZE_PER_TX {
            return Err(LedgerError::TxRefScriptsSizeTooBig {
                actual,
                max_allowed: MAX_REF_SCRIPT_SIZE_PER_TX,
            });
        }
        Ok(())
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

/// Validates that spending inputs contain no duplicates.
///
/// Upstream Shelley UTXO rules convert the input list to a set; any difference
/// in cardinality implies duplicates.
///
/// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `BadInputsUTxO`.
pub(crate) fn validate_no_duplicate_inputs(inputs: &[ShelleyTxIn]) -> Result<(), LedgerError> {
    use std::collections::HashSet;
    let set: HashSet<_> = inputs.iter().collect();
    if set.len() != inputs.len() {
        return Err(LedgerError::DuplicateInput);
    }
    Ok(())
}

/// Applies the collateral-only UTxO transition for an `is_valid = false`
/// transaction in a block (Alonzo+).
///
/// Removes all collateral inputs from the UTxO set and, for Babbage+,
/// adds the `collateral_return` output if present.
///
/// Reference: `Cardano.Ledger.Alonzo.TxSeq` — `applyPlutusScriptFailure`.
pub(crate) fn apply_collateral_only(
    utxo: &mut MultiEraUtxo,
    tx_id: [u8; 32],
    collateral: Option<&[ShelleyTxIn]>,
    collateral_return: Option<&BabbageTxOut>,
) {
    // Remove collateral inputs.
    if let Some(collateral_inputs) = collateral {
        for input in collateral_inputs {
            utxo.entries.remove(input);
        }
    }
    // Add collateral return output (Babbage+).
    if let Some(ret) = collateral_return {
        let ret_txin = ShelleyTxIn {
            transaction_id: tx_id,
            // The collateral return uses output index = 2^16 - 1 (65535)
            // as a sentinel, matching upstream `CollRet` semantics.
            index: u16::MAX,
        };
        utxo.insert(ret_txin, MultiEraTxOut::Babbage(ret.clone()));
    }
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

/// The ADA policy ID: 28 zero bytes.
///
/// The formal ledger spec defines `adaPolicy` as the empty/zero script hash.
/// No transaction may mint or burn tokens under this policy ID.
const ADA_POLICY_ID: [u8; 28] = [0u8; 28];

/// Validates that the transaction's `mint` field does not contain the ADA
/// policy ID (`[0u8; 28]`).
///
/// Reference: formal spec predicate `adaPolicy ∉ supp mint tx`
/// (`Cardano.Ledger.Mary.Rules.Utxo` — Mary through Conway).
fn validate_no_ada_in_mint(mint: &Option<MintAsset>) -> Result<(), LedgerError> {
    if let Some(minted) = mint {
        if minted.contains_key(&ADA_POLICY_ID) {
            return Err(LedgerError::TriesToForgeADA);
        }
    }
    Ok(())
}

/// Checks that consumed multi-assets plus minted/burned equals produced.
///
/// For each policy+asset: `consumed + minted == produced`.
/// `MintAsset` uses `i64` so negative values represent burning.
#[allow(clippy::type_complexity)]
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

// ─────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::eras::byron::{ByronTx, ByronTxIn, ByronTxOut};

    fn sample_txin(id_byte: u8, index: u16) -> ShelleyTxIn {
        ShelleyTxIn {
            transaction_id: [id_byte; 32],
            index,
        }
    }

    fn sample_shelley_out(amount: u64) -> ShelleyTxOut {
        ShelleyTxOut {
            address: vec![0x61; 29],
            amount,
        }
    }

    fn sample_multi_era_out(amount: u64) -> MultiEraTxOut {
        MultiEraTxOut::Shelley(sample_shelley_out(amount))
    }

    // ── MultiEraUtxo basic operations ──────────────────────────────────

    #[test]
    fn new_utxo_is_empty() {
        let utxo = MultiEraUtxo::new();
        assert!(utxo.is_empty());
        assert_eq!(utxo.len(), 0);
    }

    #[test]
    fn insert_and_get() {
        let mut utxo = MultiEraUtxo::new();
        let txin = sample_txin(0x01, 0);
        let txout = sample_multi_era_out(1_000_000);
        utxo.insert(txin.clone(), txout.clone());
        assert_eq!(utxo.len(), 1);
        assert!(!utxo.is_empty());
        assert_eq!(utxo.get(&txin), Some(&txout));
    }

    #[test]
    fn insert_shelley_convenience() {
        let mut utxo = MultiEraUtxo::new();
        let txin = sample_txin(0x02, 0);
        let txout = sample_shelley_out(500_000);
        utxo.insert_shelley(txin.clone(), txout.clone());
        assert_eq!(utxo.get(&txin), Some(&MultiEraTxOut::Shelley(txout)));
    }

    #[test]
    fn get_missing_returns_none() {
        let utxo = MultiEraUtxo::new();
        assert!(utxo.get(&sample_txin(0xff, 0)).is_none());
    }

    #[test]
    fn from_shelley_utxo() {
        let mut shelley = ShelleyUtxo::new();
        let txin = sample_txin(0x01, 0);
        let txout = sample_shelley_out(2_000_000);
        shelley.insert(txin.clone(), txout.clone());
        let multi = MultiEraUtxo::from_shelley_utxo(&shelley);
        assert_eq!(multi.len(), 1);
        assert_eq!(multi.get(&txin), Some(&MultiEraTxOut::Shelley(txout)));
    }

    #[test]
    fn iter_yields_all_entries() {
        let mut utxo = MultiEraUtxo::new();
        utxo.insert(sample_txin(0x01, 0), sample_multi_era_out(100));
        utxo.insert(sample_txin(0x02, 0), sample_multi_era_out(200));
        let collected: Vec<_> = utxo.iter().collect();
        assert_eq!(collected.len(), 2);
    }

    // ── MultiEraTxOut accessors ────────────────────────────────────────

    #[test]
    fn multi_era_txout_coin_shelley() {
        let out = MultiEraTxOut::Shelley(sample_shelley_out(3_000_000));
        assert_eq!(out.coin(), 3_000_000);
    }

    #[test]
    fn multi_era_txout_address() {
        let out = sample_multi_era_out(100);
        assert_eq!(out.address(), &[0x61; 29]);
    }

    #[test]
    fn multi_era_txout_script_ref_none_for_shelley() {
        let out = sample_multi_era_out(100);
        assert!(out.script_ref().is_none());
    }

    // ── CBOR round-trips ───────────────────────────────────────────────

    #[test]
    fn multi_era_txout_shelley_cbor_round_trip() {
        let out = sample_multi_era_out(5_000_000);
        let bytes = out.to_cbor_bytes();
        let decoded = MultiEraTxOut::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, out);
    }

    #[test]
    fn multi_era_utxo_cbor_round_trip() {
        let mut utxo = MultiEraUtxo::new();
        utxo.insert(sample_txin(0x01, 0), sample_multi_era_out(1_000));
        utxo.insert(sample_txin(0x02, 1), sample_multi_era_out(2_000));
        let bytes = utxo.to_cbor_bytes();
        let decoded = MultiEraUtxo::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, utxo);
    }

    #[test]
    fn multi_era_utxo_empty_cbor_round_trip() {
        let utxo = MultiEraUtxo::new();
        let bytes = utxo.to_cbor_bytes();
        let decoded = MultiEraUtxo::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, utxo);
    }

    // ── apply_shelley_tx ───────────────────────────────────────────────

    fn seed_utxo_shelley(id_byte: u8, amount: u64) -> (MultiEraUtxo, ShelleyTxIn) {
        let mut utxo = MultiEraUtxo::new();
        let txin = sample_txin(id_byte, 0);
        utxo.insert_shelley(txin.clone(), sample_shelley_out(amount));
        (utxo, txin)
    }

    #[test]
    fn apply_shelley_tx_valid() {
        let (mut utxo, input) = seed_utxo_shelley(0x01, 3_000_000);
        let body = ShelleyTxBody {
            inputs: vec![input],
            outputs: vec![sample_shelley_out(2_800_000)],
            fee: 200_000,
            ttl: 100,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        let result = utxo.apply_shelley_tx([0xaa; 32], &body, 50);
        assert!(result.is_ok());
        assert_eq!(utxo.len(), 1);
        let new_txin = ShelleyTxIn { transaction_id: [0xaa; 32], index: 0 };
        assert_eq!(utxo.get(&new_txin).unwrap().coin(), 2_800_000);
    }

    #[test]
    fn apply_shelley_tx_expired() {
        let (mut utxo, input) = seed_utxo_shelley(0x01, 3_000_000);
        let body = ShelleyTxBody {
            inputs: vec![input],
            outputs: vec![sample_shelley_out(2_800_000)],
            fee: 200_000,
            ttl: 10,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        let err = utxo.apply_shelley_tx([0xaa; 32], &body, 50);
        assert!(matches!(err, Err(LedgerError::TxExpired { ttl: 10, slot: 50 })));
    }

    #[test]
    fn apply_shelley_tx_value_not_preserved() {
        let (mut utxo, input) = seed_utxo_shelley(0x01, 3_000_000);
        let body = ShelleyTxBody {
            inputs: vec![input],
            outputs: vec![sample_shelley_out(3_000_000)], // no room for fee
            fee: 200_000,
            ttl: 100,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        let err = utxo.apply_shelley_tx([0xaa; 32], &body, 50);
        assert!(matches!(err, Err(LedgerError::ValueNotPreserved { .. })));
    }

    #[test]
    fn apply_shelley_tx_missing_input() {
        let mut utxo = MultiEraUtxo::new();
        let body = ShelleyTxBody {
            inputs: vec![sample_txin(0xff, 0)],
            outputs: vec![sample_shelley_out(1_000_000)],
            fee: 100,
            ttl: 100,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        assert!(matches!(
            utxo.apply_shelley_tx([0xaa; 32], &body, 50),
            Err(LedgerError::InputNotInUtxo)
        ));
    }

    #[test]
    fn apply_shelley_tx_no_inputs() {
        let mut utxo = MultiEraUtxo::new();
        let body = ShelleyTxBody {
            inputs: vec![],
            outputs: vec![sample_shelley_out(1_000_000)],
            fee: 100,
            ttl: 100,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        assert!(matches!(
            utxo.apply_shelley_tx([0xaa; 32], &body, 50),
            Err(LedgerError::NoInputs)
        ));
    }

    #[test]
    fn apply_shelley_tx_no_outputs() {
        let (mut utxo, input) = seed_utxo_shelley(0x01, 3_000_000);
        let body = ShelleyTxBody {
            inputs: vec![input],
            outputs: vec![],
            fee: 3_000_000,
            ttl: 100,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        assert!(matches!(
            utxo.apply_shelley_tx([0xaa; 32], &body, 50),
            Err(LedgerError::NoOutputs)
        ));
    }

    // ── apply_allegra_tx ───────────────────────────────────────────────

    #[test]
    fn apply_allegra_tx_optional_ttl_none() {
        let (mut utxo, input) = seed_utxo_shelley(0x01, 2_000_000);
        let body = AllegraTxBody {
            inputs: vec![input],
            outputs: vec![sample_shelley_out(1_800_000)],
            fee: 200_000,
            ttl: None,
            validity_interval_start: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        assert!(utxo.apply_allegra_tx([0xbb; 32], &body, 999_999).is_ok());
    }

    #[test]
    fn apply_allegra_tx_not_yet_valid() {
        let (mut utxo, input) = seed_utxo_shelley(0x01, 2_000_000);
        let body = AllegraTxBody {
            inputs: vec![input],
            outputs: vec![sample_shelley_out(1_800_000)],
            fee: 200_000,
            ttl: None,
            validity_interval_start: Some(100),
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        let err = utxo.apply_allegra_tx([0xbb; 32], &body, 50);
        assert!(matches!(err, Err(LedgerError::TxNotYetValid { start: 100, slot: 50 })));
    }

    // ── apply_byron_tx ─────────────────────────────────────────────────

    #[test]
    fn apply_byron_tx_valid() {
        let mut utxo = MultiEraUtxo::new();
        let seed_txid = [0x01; 32];
        let txin = ShelleyTxIn { transaction_id: seed_txid, index: 0 };
        utxo.insert_shelley(txin, sample_shelley_out(5_000_000));

        let byron_tx = ByronTx {
            inputs: vec![ByronTxIn { txid: seed_txid, index: 0 }],
            outputs: vec![ByronTxOut { address: vec![0x82; 10], amount: 4_500_000 }],
            attributes: vec![0xa0], // empty map
        };
        assert!(utxo.apply_byron_tx(&byron_tx).is_ok());
        assert_eq!(utxo.len(), 1); // old input removed, new output added
    }

    #[test]
    fn apply_byron_tx_empty_inputs() {
        let mut utxo = MultiEraUtxo::new();
        let byron_tx = ByronTx {
            inputs: vec![],
            outputs: vec![ByronTxOut { address: vec![0x82; 10], amount: 100 }],
            attributes: vec![0xa0],
        };
        assert!(matches!(utxo.apply_byron_tx(&byron_tx), Err(LedgerError::NoInputs)));
    }

    #[test]
    fn apply_byron_tx_empty_outputs() {
        let mut utxo = MultiEraUtxo::new();
        let txin = ShelleyTxIn { transaction_id: [0x01; 32], index: 0 };
        utxo.insert_shelley(txin, sample_shelley_out(1_000));
        let byron_tx = ByronTx {
            inputs: vec![ByronTxIn { txid: [0x01; 32], index: 0 }],
            outputs: vec![],
            attributes: vec![0xa0],
        };
        assert!(matches!(utxo.apply_byron_tx(&byron_tx), Err(LedgerError::NoOutputs)));
    }

    #[test]
    fn apply_byron_tx_negative_fee_rejected() {
        let mut utxo = MultiEraUtxo::new();
        let txin = ShelleyTxIn { transaction_id: [0x01; 32], index: 0 };
        utxo.insert_shelley(txin, sample_shelley_out(1_000));
        let byron_tx = ByronTx {
            inputs: vec![ByronTxIn { txid: [0x01; 32], index: 0 }],
            outputs: vec![ByronTxOut { address: vec![0x82; 10], amount: 2_000 }],
            attributes: vec![0xa0],
        };
        assert!(matches!(utxo.apply_byron_tx(&byron_tx), Err(LedgerError::ValueNotPreserved { .. })));
    }

    // ── validate_reference_inputs ──────────────────────────────────────

    #[test]
    fn validate_reference_inputs_all_present() {
        let mut utxo = MultiEraUtxo::new();
        let txin = sample_txin(0x01, 0);
        utxo.insert(txin.clone(), sample_multi_era_out(100));
        assert!(utxo.validate_reference_inputs(&[txin]).is_ok());
    }

    #[test]
    fn validate_reference_inputs_missing() {
        let utxo = MultiEraUtxo::new();
        let txin = sample_txin(0x01, 0);
        assert!(matches!(
            utxo.validate_reference_inputs(&[txin]),
            Err(LedgerError::ReferenceInputNotInUtxo)
        ));
    }

    #[test]
    fn validate_reference_inputs_empty_is_ok() {
        let utxo = MultiEraUtxo::new();
        assert!(utxo.validate_reference_inputs(&[]).is_ok());
    }

    // ── utxo_delta_for_tx ──────────────────────────────────────────────

    #[test]
    fn utxo_delta_consumed_and_produced() {
        let mut utxo = MultiEraUtxo::new();
        let txin = sample_txin(0x01, 0);
        utxo.insert(txin.clone(), sample_multi_era_out(1_000));

        let new_txin = sample_txin(0x02, 0);
        let new_txout = sample_multi_era_out(900);
        let outputs = vec![(new_txin.clone(), new_txout.clone())];

        let (consumed, produced) = utxo.utxo_delta_for_tx(&[txin.clone()], &outputs);
        assert_eq!(consumed.len(), 1);
        assert_eq!(consumed[0].0, &txin);
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0, &new_txin);
    }

    // ── free-standing helpers ──────────────────────────────────────────

    #[test]
    fn check_coin_preservation_valid() {
        assert!(check_coin_preservation(1_000_000, 800_000, 200_000).is_ok());
    }

    #[test]
    fn check_coin_preservation_mismatch() {
        assert!(check_coin_preservation(1_000_000, 800_000, 100_000).is_err());
    }

    #[test]
    fn validate_nonempty_both_empty() {
        let empty_inputs: Vec<ShelleyTxIn> = vec![];
        let outputs = vec![sample_shelley_out(100)];
        assert!(matches!(validate_nonempty(&empty_inputs, &outputs), Err(LedgerError::NoInputs)));
    }

    #[test]
    fn validate_nonempty_outputs_empty() {
        let inputs = vec![sample_txin(0x01, 0)];
        let empty_outputs: Vec<ShelleyTxOut> = vec![];
        assert!(matches!(validate_nonempty(&inputs, &empty_outputs), Err(LedgerError::NoOutputs)));
    }

    #[test]
    fn check_multi_asset_preservation_balanced() {
        let mut consumed = MultiAsset::new();
        consumed.entry([0xaa; 28]).or_default().insert(b"token".to_vec(), 100);
        let mut produced = MultiAsset::new();
        produced.entry([0xaa; 28]).or_default().insert(b"token".to_vec(), 100);
        assert!(check_multi_asset_preservation(&consumed, &produced, &None).is_ok());
    }

    #[test]
    fn check_multi_asset_preservation_with_mint() {
        let consumed = MultiAsset::new();
        let mut produced = MultiAsset::new();
        produced.entry([0xaa; 28]).or_default().insert(b"token".to_vec(), 50);
        let mut mint = std::collections::BTreeMap::new();
        let mut policy_assets = std::collections::BTreeMap::new();
        policy_assets.insert(b"token".to_vec(), 50i64);
        mint.insert([0xaa; 28], policy_assets);
        assert!(check_multi_asset_preservation(&consumed, &produced, &Some(mint)).is_ok());
    }

    #[test]
    fn check_multi_asset_preservation_imbalanced() {
        let consumed = MultiAsset::new();
        let mut produced = MultiAsset::new();
        produced.entry([0xaa; 28]).or_default().insert(b"token".to_vec(), 50);
        assert!(check_multi_asset_preservation(&consumed, &produced, &None).is_err());
    }

    #[test]
    fn check_multi_asset_preservation_burn() {
        let mut consumed = MultiAsset::new();
        consumed.entry([0xaa; 28]).or_default().insert(b"token".to_vec(), 100);
        let mut produced = MultiAsset::new();
        produced.entry([0xaa; 28]).or_default().insert(b"token".to_vec(), 70);
        let mut mint = std::collections::BTreeMap::new();
        let mut policy_assets = std::collections::BTreeMap::new();
        policy_assets.insert(b"token".to_vec(), -30i64);
        mint.insert([0xaa; 28], policy_assets);
        assert!(check_multi_asset_preservation(&consumed, &produced, &Some(mint)).is_ok());
    }

    // ── TriesToForgeADA (validate_no_ada_in_mint) ──────────────────────

    #[test]
    fn no_ada_in_mint_none_is_ok() {
        assert!(validate_no_ada_in_mint(&None).is_ok());
    }

    #[test]
    fn no_ada_in_mint_empty_map_is_ok() {
        let mint: MintAsset = std::collections::BTreeMap::new();
        assert!(validate_no_ada_in_mint(&Some(mint)).is_ok());
    }

    #[test]
    fn no_ada_in_mint_non_ada_policy_is_ok() {
        let mut mint: MintAsset = std::collections::BTreeMap::new();
        let mut assets = std::collections::BTreeMap::new();
        assets.insert(b"token".to_vec(), 100i64);
        mint.insert([0xaa; 28], assets);
        assert!(validate_no_ada_in_mint(&Some(mint)).is_ok());
    }

    #[test]
    fn no_ada_in_mint_ada_policy_rejected() {
        let mut mint: MintAsset = std::collections::BTreeMap::new();
        let mut assets = std::collections::BTreeMap::new();
        assets.insert(b"ada".to_vec(), 1_000_000i64);
        mint.insert(ADA_POLICY_ID, assets);
        assert!(matches!(
            validate_no_ada_in_mint(&Some(mint)),
            Err(LedgerError::TriesToForgeADA)
        ));
    }

    #[test]
    fn no_ada_in_mint_ada_policy_with_burn_rejected() {
        let mut mint: MintAsset = std::collections::BTreeMap::new();
        let mut assets = std::collections::BTreeMap::new();
        assets.insert(b"ada".to_vec(), -500i64);
        mint.insert(ADA_POLICY_ID, assets);
        assert!(matches!(
            validate_no_ada_in_mint(&Some(mint)),
            Err(LedgerError::TriesToForgeADA)
        ));
    }

    #[test]
    fn no_ada_in_mint_mixed_policies_with_ada_rejected() {
        let mut mint: MintAsset = std::collections::BTreeMap::new();
        let mut good_assets = std::collections::BTreeMap::new();
        good_assets.insert(b"token".to_vec(), 50i64);
        mint.insert([0xbb; 28], good_assets);
        let mut bad_assets = std::collections::BTreeMap::new();
        bad_assets.insert(b"fake_ada".to_vec(), 1i64);
        mint.insert(ADA_POLICY_ID, bad_assets);
        assert!(matches!(
            validate_no_ada_in_mint(&Some(mint)),
            Err(LedgerError::TriesToForgeADA)
        ));
    }
}
