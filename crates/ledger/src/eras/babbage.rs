//! Babbage-era transaction types with inline datums and reference scripts.
//!
//! Babbage extends Alonzo with:
//! - `transaction_output` supports both the legacy array format
//!   (`pre_babbage_transaction_output`) and the new map format
//!   (`post_alonzo_transaction_output`).
//! - `datum_option`: tag 0 for datum hash, tag 1 for inline datum
//!   (`#6.24(bytes .cbor plutus_data)`).
//! - `script_ref`: `#6.24(bytes .cbor script)` — an inline script
//!   reference carried in the output.
//! - `transaction_body` gains keys 16 (`collateral_return`),
//!   17 (`total_collateral`), and 18 (`reference_inputs`).
//!
//! Reference:
//! <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/babbage/impl/cddl-files>

use std::collections::{BTreeMap, HashMap};

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::mary::{MintAsset, Value, decode_mint_asset, encode_mint_asset};
use crate::eras::shelley::{PraosHeader, ShelleyTxIn, ShelleyUpdate, ShelleyWitnessSet};
use crate::error::LedgerError;
use crate::plutus::{PlutusData, ScriptRef};
use crate::types::{DCert, HeaderHash, RewardAccount};

pub const BABBAGE_NAME: &str = "Babbage";

// ---------------------------------------------------------------------------
// Datum option
// ---------------------------------------------------------------------------

/// Datum option: either a hash reference or inline datum.
///
/// CDDL: `datum_option = [ 0, $hash32 // 1, data ]`
///
/// Inline datum data is a typed `PlutusData` value wrapped in
/// `#6.24(bytes .cbor plutus_data)` double encoding.
///
/// Reference: `Cardano.Ledger.Babbage.TxBody` — `Datum`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatumOption {
    /// Datum hash reference (tag 0).
    Hash([u8; 32]),
    /// Inline datum as typed PlutusData (tag 1).
    Inline(PlutusData),
}

impl CborEncode for DatumOption {
    fn encode_cbor(&self, enc: &mut Encoder) {
        match self {
            Self::Hash(hash) => {
                enc.array(2).unsigned(0).bytes(hash);
            }
            Self::Inline(pd) => {
                enc.array(2).unsigned(1);
                let mut inner = Encoder::new();
                pd.encode_cbor(&mut inner);
                enc.tag(24).bytes(&inner.into_bytes());
            }
        }
    }
}

impl CborDecode for DatumOption {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let tag = dec.unsigned()?;
        match tag {
            0 => {
                let raw = dec.bytes()?;
                let hash: [u8; 32] =
                    raw.try_into()
                        .map_err(|_| LedgerError::CborInvalidLength {
                            expected: 32,
                            actual: raw.len(),
                        })?;
                Ok(Self::Hash(hash))
            }
            1 => {
                let cbor_tag = dec.tag()?;
                if cbor_tag != 24 {
                    return Err(LedgerError::CborTypeMismatch {
                        expected: 24,
                        actual: cbor_tag as u8,
                    });
                }
                let inner_bytes = dec.bytes()?;
                let mut inner_dec = Decoder::new(inner_bytes);
                let pd = PlutusData::decode_cbor(&mut inner_dec)?;
                Ok(Self::Inline(pd))
            }
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 0,
                actual: tag as u8,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Babbage transaction output
// ---------------------------------------------------------------------------

/// Babbage-era transaction output supporting both legacy array and
/// post-Alonzo map formats.
///
/// ```text
/// transaction_output = pre_babbage_transaction_output
///                    / post_alonzo_transaction_output
///
/// pre_babbage_transaction_output = [address, amount : value, ? datum_hash]
///
/// post_alonzo_transaction_output =
///   { 0 : address, 1 : value, ? 2 : datum_option, ? 3 : script_ref }
/// ```
///
/// On encode, the canonical post-Alonzo map format is used.
/// On decode, both formats are accepted by peeking at the CBOR major type.
///
/// Reference: `Cardano.Ledger.Babbage.TxOut`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BabbageTxOut {
    /// Raw address bytes.
    pub address: Vec<u8>,
    /// Output value (coin or coin + multi-asset).
    pub amount: Value,
    /// Optional datum (hash or inline).
    pub datum_option: Option<DatumOption>,
    /// Optional inline script reference.
    pub script_ref: Option<ScriptRef>,
}

impl CborEncode for BabbageTxOut {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let mut field_count: u64 = 2; // keys 0, 1
        if self.datum_option.is_some() {
            field_count += 1;
        }
        if self.script_ref.is_some() {
            field_count += 1;
        }
        enc.map(field_count);

        // Key 0: address.
        enc.unsigned(0).bytes(&self.address);

        // Key 1: value.
        enc.unsigned(1);
        self.amount.encode_cbor(enc);

        // Key 2: datum_option.
        if let Some(datum) = &self.datum_option {
            enc.unsigned(2);
            datum.encode_cbor(enc);
        }

        // Key 3: script_ref.
        if let Some(sref) = &self.script_ref {
            enc.unsigned(3);
            sref.encode_cbor(enc);
        }
    }
}

impl CborDecode for BabbageTxOut {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let major = dec.peek_major()?;
        match major {
            // Major type 4 = array → pre-Babbage format.
            4 => decode_pre_babbage_txout(dec),
            // Major type 5 = map → post-Alonzo format.
            5 => decode_post_alonzo_txout(dec),
            _ => Err(LedgerError::CborTypeMismatch {
                expected: 4,
                actual: major,
            }),
        }
    }
}

/// Decode a pre-Babbage (Alonzo-style) array transaction output.
fn decode_pre_babbage_txout(dec: &mut Decoder<'_>) -> Result<BabbageTxOut, LedgerError> {
    let len = dec.array()?;
    if len != 2 && len != 3 {
        return Err(LedgerError::CborInvalidLength {
            expected: 2,
            actual: len as usize,
        });
    }
    let address = dec.bytes()?.to_vec();
    let amount = Value::decode_cbor(dec)?;
    let datum_option = if len == 3 {
        let raw = dec.bytes()?;
        let hash: [u8; 32] = raw.try_into().map_err(|_| LedgerError::CborInvalidLength {
            expected: 32,
            actual: raw.len(),
        })?;
        Some(DatumOption::Hash(hash))
    } else {
        None
    };
    Ok(BabbageTxOut {
        address,
        amount,
        datum_option,
        script_ref: None,
    })
}

/// Decode a post-Alonzo map-format transaction output.
fn decode_post_alonzo_txout(dec: &mut Decoder<'_>) -> Result<BabbageTxOut, LedgerError> {
    let map_len = dec.map()?;
    let mut address: Option<Vec<u8>> = None;
    let mut amount: Option<Value> = None;
    let mut datum_option: Option<DatumOption> = None;
    let mut script_ref: Option<ScriptRef> = None;

    for _ in 0..map_len {
        let key = dec.unsigned()?;
        match key {
            0 => {
                address = Some(dec.bytes()?.to_vec());
            }
            1 => {
                amount = Some(Value::decode_cbor(dec)?);
            }
            2 => {
                datum_option = Some(DatumOption::decode_cbor(dec)?);
            }
            3 => {
                script_ref = Some(ScriptRef::decode_cbor(dec)?);
            }
            _ => {
                dec.skip()?;
            }
        }
    }

    Ok(BabbageTxOut {
        address: address.ok_or(LedgerError::CborInvalidLength {
            expected: 1,
            actual: 0,
        })?,
        amount: amount.ok_or(LedgerError::CborInvalidLength {
            expected: 1,
            actual: 0,
        })?,
        datum_option,
        script_ref,
    })
}

// ---------------------------------------------------------------------------
// Babbage transaction body
// ---------------------------------------------------------------------------

/// Babbage-era transaction body.
///
/// Extends Alonzo by adding:
/// - Key 16: `collateral_return` — output returned when collateral is
///   consumed but the transaction is valid.
/// - Key 17: `total_collateral` — explicit total collateral amount.
/// - Key 18: `reference_inputs` — inputs used for reading but not spent.
///
/// ```text
/// transaction_body =
///   { 0  : set<transaction_input>
///   , 1  : [* transaction_output]
///   , 2  : coin
///   , ? 3  : uint                        ; ttl
///   , ? 4  : [* certificate]
///   , ? 5  : withdrawals
///   , ? 6  : update
///   , ? 7  : auxiliary_data_hash
///   , ? 8  : uint                        ; validity interval start
///   , ? 9  : mint
///   , ? 11 : script_data_hash
///   , ? 13 : set<transaction_input>      ; collateral inputs
///   , ? 14 : required_signers
///   , ? 15 : network_id
///   , ? 16 : transaction_output          ; collateral return   (NEW)
///   , ? 17 : coin                        ; total collateral    (NEW)
///   , ? 18 : set<transaction_input>      ; reference inputs    (NEW)
///   }
/// ```
///
/// Certificates (4), withdrawals (5), and update (6) are now modeled.
///
/// Reference: `Cardano.Ledger.Babbage.TxBody`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BabbageTxBody {
    /// Set of transaction inputs (CDDL key 0).
    pub inputs: Vec<ShelleyTxIn>,
    /// Sequence of transaction outputs (CDDL key 1).
    pub outputs: Vec<BabbageTxOut>,
    /// Transaction fee in lovelace (CDDL key 2).
    pub fee: u64,
    /// Optional TTL slot (CDDL key 3).
    pub ttl: Option<u64>,
    /// Optional certificates (CDDL key 4).
    pub certificates: Option<Vec<DCert>>,
    /// Optional withdrawals: reward-account → lovelace (CDDL key 5).
    pub withdrawals: Option<BTreeMap<RewardAccount, u64>>,
    /// Optional protocol-parameter update proposal (CDDL key 6).
    pub update: Option<ShelleyUpdate>,
    /// Optional auxiliary data hash (CDDL key 7).
    pub auxiliary_data_hash: Option<[u8; 32]>,
    /// Optional validity interval start (CDDL key 8).
    pub validity_interval_start: Option<u64>,
    /// Optional mint field for native tokens (CDDL key 9).
    pub mint: Option<MintAsset>,
    /// Optional hash of script integrity data (CDDL key 11).
    pub script_data_hash: Option<[u8; 32]>,
    /// Optional collateral inputs (CDDL key 13).
    pub collateral: Option<Vec<ShelleyTxIn>>,
    /// Optional required signer key hashes (CDDL key 14).
    pub required_signers: Option<Vec<[u8; 28]>>,
    /// Optional network ID: 0 = testnet, 1 = mainnet (CDDL key 15).
    pub network_id: Option<u8>,
    /// Optional collateral return output (CDDL key 16).
    pub collateral_return: Option<BabbageTxOut>,
    /// Optional total collateral in lovelace (CDDL key 17).
    pub total_collateral: Option<u64>,
    /// Optional reference inputs (CDDL key 18).
    pub reference_inputs: Option<Vec<ShelleyTxIn>>,
}

impl CborEncode for BabbageTxBody {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let mut field_count: u64 = 3; // keys 0, 1, 2
        if self.ttl.is_some() {
            field_count += 1;
        }
        if self.certificates.is_some() {
            field_count += 1;
        }
        if self.withdrawals.is_some() {
            field_count += 1;
        }
        if self.update.is_some() {
            field_count += 1;
        }
        if self.auxiliary_data_hash.is_some() {
            field_count += 1;
        }
        if self.validity_interval_start.is_some() {
            field_count += 1;
        }
        if self.mint.is_some() {
            field_count += 1;
        }
        if self.script_data_hash.is_some() {
            field_count += 1;
        }
        if self.collateral.is_some() {
            field_count += 1;
        }
        if self.required_signers.is_some() {
            field_count += 1;
        }
        if self.network_id.is_some() {
            field_count += 1;
        }
        if self.collateral_return.is_some() {
            field_count += 1;
        }
        if self.total_collateral.is_some() {
            field_count += 1;
        }
        if self.reference_inputs.is_some() {
            field_count += 1;
        }
        enc.map(field_count);

        // Key 0: inputs.
        enc.unsigned(0).array(self.inputs.len() as u64);
        for input in &self.inputs {
            input.encode_cbor(enc);
        }

        // Key 1: outputs.
        enc.unsigned(1).array(self.outputs.len() as u64);
        for output in &self.outputs {
            output.encode_cbor(enc);
        }

        // Key 2: fee.
        enc.unsigned(2).unsigned(self.fee);

        // Key 3: ttl.
        if let Some(ttl) = self.ttl {
            enc.unsigned(3).unsigned(ttl);
        }

        // Key 4: certificates.
        if let Some(certs) = &self.certificates {
            enc.unsigned(4).array(certs.len() as u64);
            for cert in certs {
                cert.encode_cbor(enc);
            }
        }

        // Key 5: withdrawals.
        if let Some(withdrawals) = &self.withdrawals {
            enc.unsigned(5).map(withdrawals.len() as u64);
            for (acct, coin) in withdrawals {
                acct.encode_cbor(enc);
                enc.unsigned(*coin);
            }
        }

        // Key 6: update.
        if let Some(update) = &self.update {
            enc.unsigned(6);
            update.encode_cbor(enc);
        }

        // Key 7: auxiliary_data_hash.
        if let Some(hash) = &self.auxiliary_data_hash {
            enc.unsigned(7).bytes(hash);
        }

        // Key 8: validity_interval_start.
        if let Some(start) = self.validity_interval_start {
            enc.unsigned(8).unsigned(start);
        }

        // Key 9: mint.
        if let Some(mint) = &self.mint {
            enc.unsigned(9);
            encode_mint_asset(enc, mint);
        }

        // Key 11: script_data_hash.
        if let Some(hash) = &self.script_data_hash {
            enc.unsigned(11).bytes(hash);
        }

        // Key 13: collateral.
        if let Some(collateral) = &self.collateral {
            enc.unsigned(13).array(collateral.len() as u64);
            for input in collateral {
                input.encode_cbor(enc);
            }
        }

        // Key 14: required_signers.
        if let Some(signers) = &self.required_signers {
            enc.unsigned(14).array(signers.len() as u64);
            for signer in signers {
                enc.bytes(signer);
            }
        }

        // Key 15: network_id.
        if let Some(nid) = self.network_id {
            enc.unsigned(15).unsigned(u64::from(nid));
        }

        // Key 16: collateral_return.
        if let Some(ret) = &self.collateral_return {
            enc.unsigned(16);
            ret.encode_cbor(enc);
        }

        // Key 17: total_collateral.
        if let Some(total) = self.total_collateral {
            enc.unsigned(17).unsigned(total);
        }

        // Key 18: reference_inputs.
        if let Some(refs) = &self.reference_inputs {
            enc.unsigned(18).array(refs.len() as u64);
            for input in refs {
                input.encode_cbor(enc);
            }
        }
    }
}

impl CborDecode for BabbageTxBody {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let map_len = dec.map()?;

        let mut inputs: Option<Vec<ShelleyTxIn>> = None;
        let mut outputs: Option<Vec<BabbageTxOut>> = None;
        let mut fee: Option<u64> = None;
        let mut ttl: Option<u64> = None;
        let mut certificates: Option<Vec<DCert>> = None;
        let mut withdrawals: Option<BTreeMap<RewardAccount, u64>> = None;
        let mut update: Option<ShelleyUpdate> = None;
        let mut auxiliary_data_hash: Option<[u8; 32]> = None;
        let mut validity_interval_start: Option<u64> = None;
        let mut mint: Option<MintAsset> = None;
        let mut script_data_hash: Option<[u8; 32]> = None;
        let mut collateral: Option<Vec<ShelleyTxIn>> = None;
        let mut required_signers: Option<Vec<[u8; 28]>> = None;
        let mut network_id: Option<u8> = None;
        let mut collateral_return: Option<BabbageTxOut> = None;
        let mut total_collateral: Option<u64> = None;
        let mut reference_inputs: Option<Vec<ShelleyTxIn>> = None;

        for _ in 0..map_len {
            let key = dec.unsigned()?;
            match key {
                0 => {
                    let count = dec.array()?;
                    let mut ins = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        ins.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    inputs = Some(ins);
                }
                1 => {
                    let count = dec.array()?;
                    let mut outs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        outs.push(BabbageTxOut::decode_cbor(dec)?);
                    }
                    outputs = Some(outs);
                }
                2 => {
                    fee = Some(dec.unsigned()?);
                }
                3 => {
                    ttl = Some(dec.unsigned()?);
                }
                4 => {
                    let count = dec.array()?;
                    let mut certs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        certs.push(DCert::decode_cbor(dec)?);
                    }
                    certificates = Some(certs);
                }
                5 => {
                    let count = dec.map()?;
                    let mut wdrl = BTreeMap::new();
                    for _ in 0..count {
                        let acct = RewardAccount::decode_cbor(dec)?;
                        let coin = dec.unsigned()?;
                        wdrl.insert(acct, coin);
                    }
                    withdrawals = Some(wdrl);
                }
                6 => {
                    update = Some(ShelleyUpdate::decode_cbor(dec)?);
                }
                7 => {
                    let raw = dec.bytes()?;
                    let hash: [u8; 32] =
                        raw.try_into()
                            .map_err(|_| LedgerError::CborInvalidLength {
                                expected: 32,
                                actual: raw.len(),
                            })?;
                    auxiliary_data_hash = Some(hash);
                }
                8 => {
                    validity_interval_start = Some(dec.unsigned()?);
                }
                9 => {
                    mint = Some(decode_mint_asset(dec)?);
                }
                11 => {
                    let raw = dec.bytes()?;
                    let hash: [u8; 32] =
                        raw.try_into()
                            .map_err(|_| LedgerError::CborInvalidLength {
                                expected: 32,
                                actual: raw.len(),
                            })?;
                    script_data_hash = Some(hash);
                }
                13 => {
                    let count = dec.array()?;
                    let mut cols = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        cols.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    collateral = Some(cols);
                }
                14 => {
                    let count = dec.array()?;
                    let mut sigs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        let raw = dec.bytes()?;
                        let hash: [u8; 28] =
                            raw.try_into()
                                .map_err(|_| LedgerError::CborInvalidLength {
                                    expected: 28,
                                    actual: raw.len(),
                                })?;
                        sigs.push(hash);
                    }
                    required_signers = Some(sigs);
                }
                15 => {
                    network_id = Some(dec.unsigned()? as u8);
                }
                16 => {
                    collateral_return = Some(BabbageTxOut::decode_cbor(dec)?);
                }
                17 => {
                    total_collateral = Some(dec.unsigned()?);
                }
                18 => {
                    let count = dec.array()?;
                    let mut refs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        refs.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    reference_inputs = Some(refs);
                }
                _ => {
                    dec.skip()?;
                }
            }
        }

        Ok(Self {
            inputs: inputs.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            outputs: outputs.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            fee: fee.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            ttl,
            certificates,
            withdrawals,
            update,
            auxiliary_data_hash,
            validity_interval_start,
            mint,
            script_data_hash,
            collateral,
            required_signers,
            network_id,
            collateral_return,
            total_collateral,
            reference_inputs,
        })
    }
}

// ---------------------------------------------------------------------------
// Block envelope
// ---------------------------------------------------------------------------

/// A complete Babbage-era block as it appears on the wire.
///
/// Uses the Praos header format (14-element body with single `vrf_result`)
/// instead of the Shelley header (15-element body with `nonce_vrf` +
/// `leader_vrf`).
///
/// CDDL:
/// ```text
/// block = [
///   header,
///   transaction_bodies       : [* transaction_body],
///   transaction_witness_sets : [* transaction_witness_set],
///   auxiliary_data_set       : {* transaction_index => auxiliary_data},
///   invalid_transactions     : [* transaction_index]
/// ]
/// ```
///
/// Reference: `Cardano.Ledger.Babbage.TxBody` and
/// `Ouroboros.Consensus.Shelley.Ledger.Block`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BabbageBlock {
    /// The signed block header (Praos format).
    pub header: PraosHeader,
    /// Transaction bodies decoded with Babbage-era key-map CBOR.
    pub transaction_bodies: Vec<BabbageTxBody>,
    /// Witness sets (parallel to transaction_bodies).
    pub transaction_witness_sets: Vec<ShelleyWitnessSet>,
    /// Auxiliary data map: transaction index → raw CBOR auxiliary data bytes.
    pub auxiliary_data_set: HashMap<u64, Vec<u8>>,
    /// Indices of transactions whose Phase-2 scripts failed validation.
    pub invalid_transactions: Vec<u64>,
}

impl BabbageBlock {
    /// Compute the Blake2b-256 header hash for this block.
    pub fn header_hash(&self) -> HeaderHash {
        self.header.header_hash()
    }
}

impl CborEncode for BabbageBlock {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(5);
        self.header.encode_cbor(enc);

        enc.array(self.transaction_bodies.len() as u64);
        for body in &self.transaction_bodies {
            body.encode_cbor(enc);
        }

        enc.array(self.transaction_witness_sets.len() as u64);
        for ws in &self.transaction_witness_sets {
            ws.encode_cbor(enc);
        }

        enc.map(self.auxiliary_data_set.len() as u64);
        for (&idx, meta) in &self.auxiliary_data_set {
            enc.unsigned(idx);
            enc.raw(meta);
        }

        enc.array(self.invalid_transactions.len() as u64);
        for &idx in &self.invalid_transactions {
            enc.unsigned(idx);
        }
    }
}

impl CborDecode for BabbageBlock {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 5 {
            return Err(LedgerError::CborInvalidLength {
                expected: 5,
                actual: len as usize,
            });
        }

        let header = PraosHeader::decode_cbor(dec)?;

        let tb_count = dec.array()?;
        let mut transaction_bodies = Vec::with_capacity(tb_count as usize);
        for _ in 0..tb_count {
            transaction_bodies.push(BabbageTxBody::decode_cbor(dec)?);
        }

        let ws_count = dec.array()?;
        let mut witness_sets = Vec::with_capacity(ws_count as usize);
        for _ in 0..ws_count {
            witness_sets.push(ShelleyWitnessSet::decode_cbor(dec)?);
        }

        let meta_count = dec.map()?;
        let mut transaction_metadata = HashMap::with_capacity(meta_count as usize);
        for _ in 0..meta_count {
            let idx = dec.unsigned()?;
            let start = dec.position();
            dec.skip()?;
            let end = dec.position();
            let raw = dec.slice(start, end)?.to_vec();
            transaction_metadata.insert(idx, raw);
        }

        let inv_count = dec.array()?;
        let mut invalid_transactions = Vec::with_capacity(inv_count as usize);
        for _ in 0..inv_count {
            invalid_transactions.push(dec.unsigned()?);
        }

        Ok(Self {
            header,
            transaction_bodies,
            transaction_witness_sets: witness_sets,
            auxiliary_data_set: transaction_metadata,
            invalid_transactions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eras::mary::Value;

    fn mk_txin(idx: u16) -> ShelleyTxIn {
        ShelleyTxIn { transaction_id: [0xAA; 32], index: idx }
    }

    fn mk_babbage_txout() -> BabbageTxOut {
        BabbageTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        }
    }

    // ── DatumOption ────────────────────────────────────────────────────

    #[test]
    fn datum_option_hash_round_trip() {
        let d = DatumOption::Hash([0xCC; 32]);
        let decoded = DatumOption::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn datum_option_inline_round_trip() {
        let d = DatumOption::Inline(PlutusData::Integer(42));
        let decoded = DatumOption::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn datum_option_inline_complex_round_trip() {
        let d = DatumOption::Inline(PlutusData::Constr(0, vec![
            PlutusData::Bytes(vec![0x01, 0x02]),
            PlutusData::Integer(-1),
        ]));
        let decoded = DatumOption::from_cbor_bytes(&d.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn datum_option_hash_vs_inline_differ() {
        let h = DatumOption::Hash([0xDD; 32]);
        let i = DatumOption::Inline(PlutusData::Bytes(vec![0xDD; 32]));
        assert_ne!(h.to_cbor_bytes(), i.to_cbor_bytes());
    }

    // ── BabbageTxOut ───────────────────────────────────────────────────

    #[test]
    fn txout_minimal_round_trip() {
        let out = mk_babbage_txout();
        let decoded = BabbageTxOut::from_cbor_bytes(&out.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, out);
    }

    #[test]
    fn txout_with_datum_option_round_trip() {
        let out = BabbageTxOut {
            datum_option: Some(DatumOption::Hash([0xEE; 32])),
            ..mk_babbage_txout()
        };
        let decoded = BabbageTxOut::from_cbor_bytes(&out.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, out);
    }

    #[test]
    fn txout_with_script_ref_round_trip() {
        use crate::plutus::{Script, ScriptRef};
        let out = BabbageTxOut {
            script_ref: Some(ScriptRef(Script::PlutusV2(vec![0x01, 0x02, 0x03]))),
            ..mk_babbage_txout()
        };
        let decoded = BabbageTxOut::from_cbor_bytes(&out.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, out);
    }

    #[test]
    fn txout_with_all_optional_round_trip() {
        use crate::plutus::{Script, ScriptRef};
        let out = BabbageTxOut {
            address: vec![0x01; 57],
            amount: Value::Coin(10_000_000),
            datum_option: Some(DatumOption::Inline(PlutusData::Integer(99))),
            script_ref: Some(ScriptRef(Script::PlutusV1(vec![0xAB]))),
        };
        let decoded = BabbageTxOut::from_cbor_bytes(&out.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, out);
    }

    #[test]
    fn txout_datum_option_absent_vs_present_differ() {
        let without = mk_babbage_txout();
        let with = BabbageTxOut {
            datum_option: Some(DatumOption::Hash([0xFF; 32])),
            ..without.clone()
        };
        assert_ne!(without.to_cbor_bytes(), with.to_cbor_bytes());
    }

    // ── BabbageTxBody ──────────────────────────────────────────────────

    fn mk_babbage_body() -> BabbageTxBody {
        BabbageTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![mk_babbage_txout()],
            fee: 200_000,
            ttl: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
            script_data_hash: None,
            collateral: None,
            required_signers: None,
            network_id: None,
            collateral_return: None,
            total_collateral: None,
            reference_inputs: None,
        }
    }

    #[test]
    fn tx_body_minimal_round_trip() {
        let body = mk_babbage_body();
        let decoded = BabbageTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_with_babbage_fields_round_trip() {
        let body = BabbageTxBody {
            collateral_return: Some(mk_babbage_txout()),
            total_collateral: Some(5_000_000),
            reference_inputs: Some(vec![mk_txin(2)]),
            ..mk_babbage_body()
        };
        let decoded = BabbageTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_with_all_optional_round_trip() {
        let body = BabbageTxBody {
            ttl: Some(1000),
            auxiliary_data_hash: Some([0x11; 32]),
            validity_interval_start: Some(100),
            script_data_hash: Some([0x22; 32]),
            collateral: Some(vec![mk_txin(3)]),
            required_signers: Some(vec![[0x33; 28]]),
            network_id: Some(1),
            collateral_return: Some(mk_babbage_txout()),
            total_collateral: Some(3_000_000),
            reference_inputs: Some(vec![mk_txin(4)]),
            ..mk_babbage_body()
        };
        let decoded = BabbageTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_no_ref_inputs_vs_with_differ() {
        let base = mk_babbage_body();
        let with_refs = BabbageTxBody {
            reference_inputs: Some(vec![mk_txin(9)]),
            ..base.clone()
        };
        assert_ne!(base.to_cbor_bytes(), with_refs.to_cbor_bytes());
    }
}
