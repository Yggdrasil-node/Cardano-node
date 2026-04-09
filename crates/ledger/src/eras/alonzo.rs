//! Alonzo-era transaction types with Plutus script support.
//!
//! Alonzo introduces Plutus smart contracts, adding the following to the
//! Mary-era foundation:
//! - `transaction_output` gains an optional `datum_hash`.
//! - `transaction_body` gains keys 11 (`script_data_hash`), 13
//!   (`collateral`), 14 (`required_signers`), 15 (`network_id`).
//! - `transaction_witness_set` gains Plutus scripts (key 3), datums
//!   (key 4), and redeemers (key 5).
//! - The `transaction` tuple becomes 4-element with an `is_valid` flag.
//!
//! This module models new Alonzo-specific types. Plutus data is typed
//! using the `PlutusData` AST defined in `plutus.rs`.
//!
//! Reference:
//! <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/alonzo/impl/cddl>

use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::mary::{MintAsset, Value, decode_mint_asset, encode_mint_asset};
use crate::eras::shelley::{ShelleyHeader, ShelleyTxIn, ShelleyWitnessSet};
use crate::eras::shelley::ShelleyUpdate;
use crate::error::LedgerError;
use crate::plutus::PlutusData;
use crate::types::{DCert, HeaderHash, RewardAccount};

pub const ALONZO_NAME: &str = "Alonzo";

// ---------------------------------------------------------------------------
// Execution units
// ---------------------------------------------------------------------------

/// Computational budget for Plutus script execution.
///
/// CDDL: `ex_units = [mem : uint, steps : uint]`
///
/// Reference: `Cardano.Ledger.Alonzo.Plutus.TxInfo` — `ExUnits`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExUnits {
    /// Memory units consumed.
    pub mem: u64,
    /// CPU step units consumed.
    pub steps: u64,
}

impl CborEncode for ExUnits {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2).unsigned(self.mem).unsigned(self.steps);
    }
}

impl CborDecode for ExUnits {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let mem = dec.unsigned()?;
        let steps = dec.unsigned()?;
        Ok(Self { mem, steps })
    }
}

// ---------------------------------------------------------------------------
// Redeemer
// ---------------------------------------------------------------------------

/// A redeemer that supplies execution context to a Plutus script.
///
/// CDDL: `redeemer = [tag : redeemer_tag, index : uint,
///          data : plutus_data, ex_units : ex_units]`
///
/// Reference: `Cardano.Ledger.Alonzo.TxWits` — `Redeemer`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Redeemer {
    /// Redeemer purpose tag: 0 = spend, 1 = mint, 2 = cert, 3 = reward.
    pub tag: u8,
    /// Index into the relevant sorted set (inputs, policies, etc.).
    pub index: u64,
    /// Typed Plutus data payload.
    pub data: PlutusData,
    /// Execution budget for this redeemer.
    pub ex_units: ExUnits,
}

impl CborEncode for Redeemer {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4)
            .unsigned(u64::from(self.tag))
            .unsigned(self.index);
        self.data.encode_cbor(enc);
        self.ex_units.encode_cbor(enc);
    }
}

impl CborDecode for Redeemer {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }
        let tag = dec.unsigned()? as u8;
        let index = dec.unsigned()?;
        let data = PlutusData::decode_cbor(dec)?;
        let ex_units = ExUnits::decode_cbor(dec)?;
        Ok(Self {
            tag,
            index,
            data,
            ex_units,
        })
    }
}

// ---------------------------------------------------------------------------
// Alonzo transaction output
// ---------------------------------------------------------------------------

/// An Alonzo-era transaction output with optional datum hash.
///
/// CDDL: `transaction_output = [address, amount : value,
///          ? datum_hash : hash32]`
///
/// Reference: `Cardano.Ledger.Alonzo.TxOut`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlonzoTxOut {
    /// Raw address bytes.
    pub address: Vec<u8>,
    /// Output value (coin or coin + multi-asset).
    pub amount: Value,
    /// Optional datum hash locking the output to a Plutus script.
    pub datum_hash: Option<[u8; 32]>,
}

impl CborEncode for AlonzoTxOut {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let len = if self.datum_hash.is_some() { 3 } else { 2 };
        enc.array(len).bytes(&self.address);
        self.amount.encode_cbor(enc);
        if let Some(hash) = &self.datum_hash {
            enc.bytes(hash);
        }
    }
}

impl CborDecode for AlonzoTxOut {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 && len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let address = dec.bytes()?.to_vec();
        let amount = Value::decode_cbor(dec)?;
        let datum_hash = if len == 3 {
            let raw = dec.bytes()?;
            let hash: [u8; 32] =
                raw.try_into()
                    .map_err(|_| LedgerError::CborInvalidLength {
                        expected: 32,
                        actual: raw.len(),
                    })?;
            Some(hash)
        } else {
            None
        };
        Ok(Self {
            address,
            amount,
            datum_hash,
        })
    }
}

// ---------------------------------------------------------------------------
// Alonzo transaction body
// ---------------------------------------------------------------------------

/// Alonzo-era transaction body.
///
/// Extends Mary by adding:
/// - Key 11: `script_data_hash` — hash of redeemers, datums, and cost models.
/// - Key 13: `collateral` — set of inputs pledged as collateral.
/// - Key 14: `required_signers` — set of key hashes that must sign.
/// - Key 15: `network_id` — 0 (testnet) or 1 (mainnet).
///
/// ```text
/// transaction_body =
///   { 0  : set<transaction_input>
///   , 1  : [* transaction_output]
///   , 2  : coin
///   , ? 3  : slot
///   , ? 4  : [* certificate]
///   , ? 5  : withdrawals
///   , ? 6  : update
///   , ? 7  : auxiliary_data_hash
///   , ? 8  : slot
///   , ? 9  : mint
///   , ? 11 : script_data_hash      ; NEW
///   , ? 13 : set<transaction_input> ; NEW (collateral)
///   , ? 14 : required_signers       ; NEW
///   , ? 15 : network_id             ; NEW
///   }
/// ```
///
/// Only modeled optional keys: 3, 4, 5, 6, 7, 8, 9, 11, 13, 14, 15.
///
/// Reference: `Cardano.Ledger.Alonzo.TxBody`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlonzoTxBody {
    /// Set of transaction inputs (CDDL key 0).
    pub inputs: Vec<ShelleyTxIn>,
    /// Sequence of transaction outputs (CDDL key 1).
    pub outputs: Vec<AlonzoTxOut>,
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
}

impl CborEncode for AlonzoTxBody {
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
    }
}

impl CborDecode for AlonzoTxBody {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let map_len = dec.map()?;

        let mut inputs: Option<Vec<ShelleyTxIn>> = None;
        let mut outputs: Option<Vec<AlonzoTxOut>> = None;
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

        for _ in 0..map_len {
            let key = dec.unsigned()?;
            match key {
                0 => {
                    let count = dec.array_or_set()?;
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
                        outs.push(AlonzoTxOut::decode_cbor(dec)?);
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
                    let count = dec.array_or_set()?;
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
                    let count = dec.array_or_set()?;
                    let mut cols = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        cols.push(ShelleyTxIn::decode_cbor(dec)?);
                    }
                    collateral = Some(cols);
                }
                14 => {
                    let count = dec.array_or_set()?;
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
        })
    }
}

// ---------------------------------------------------------------------------
// Alonzo block
// ---------------------------------------------------------------------------

/// An Alonzo-era full block.
///
/// Alonzo changed the block envelope from 4 elements (Shelley/Allegra/Mary)
/// to 5 elements, adding `invalid_transactions` — a list of transaction
/// indices whose Phase-2 Plutus scripts failed validation.  The block still
/// uses the Shelley-era 15-element `ShelleyHeader` (with `nonce_vrf` and
/// `leader_vrf`), since the TPraos protocol was active until the Babbage
/// hard fork.
///
/// CDDL:
/// ```text
/// block = [
///   header,
///   transaction_bodies       : [* transaction_body],
///   transaction_witness_sets : [* transaction_witness_set],
///   auxiliary_data_set       : {* transaction_index => auxiliary_data},
///   invalid_transactions     : [* transaction_index],
/// ]
/// ```
///
/// Reference: `Ouroboros.Consensus.Shelley.Ledger.Block`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlonzoBlock {
    /// The signed block header (Shelley/TPraos format, 15-element body).
    pub header: ShelleyHeader,
    /// Transaction bodies decoded with Alonzo-era key-map CBOR.
    pub transaction_bodies: Vec<AlonzoTxBody>,
    /// Witness sets (parallel to transaction_bodies).
    pub transaction_witness_sets: Vec<ShelleyWitnessSet>,
    /// Auxiliary data map: transaction index → raw CBOR auxiliary data bytes.
    pub auxiliary_data_set: HashMap<u64, Vec<u8>>,
    /// Indices of transactions whose Phase-2 scripts failed validation.
    pub invalid_transactions: Vec<u64>,
}

impl AlonzoBlock {
    /// Compute the Blake2b-256 header hash for this block.
    pub fn header_hash(&self) -> HeaderHash {
        self.header.header_hash()
    }
}

impl CborEncode for AlonzoBlock {
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
        for (&idx, aux) in &self.auxiliary_data_set {
            enc.unsigned(idx);
            enc.raw(aux);
        }

        enc.array(self.invalid_transactions.len() as u64);
        for &idx in &self.invalid_transactions {
            enc.unsigned(idx);
        }
    }
}

impl CborDecode for AlonzoBlock {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 5 {
            return Err(LedgerError::CborInvalidLength {
                expected: 5,
                actual: len as usize,
            });
        }

        let header = ShelleyHeader::decode_cbor(dec)?;

        let tb_count = dec.array()?;
        let mut transaction_bodies = Vec::with_capacity(tb_count as usize);
        for _ in 0..tb_count {
            transaction_bodies.push(AlonzoTxBody::decode_cbor(dec)?);
        }

        let ws_count = dec.array()?;
        let mut witness_sets = Vec::with_capacity(ws_count as usize);
        for _ in 0..ws_count {
            witness_sets.push(ShelleyWitnessSet::decode_cbor(dec)?);
        }

        let meta_count = dec.map()?;
        let mut auxiliary_data_set = HashMap::with_capacity(meta_count as usize);
        for _ in 0..meta_count {
            let idx = dec.unsigned()?;
            let start = dec.position();
            dec.skip()?;
            let end = dec.position();
            let raw = dec.slice(start, end)?.to_vec();
            auxiliary_data_set.insert(idx, raw);
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
            auxiliary_data_set,
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

    fn mk_alonzo_txout() -> AlonzoTxOut {
        AlonzoTxOut {
            address: vec![0x61; 29],
            amount: Value::Coin(2_000_000),
            datum_hash: None,
        }
    }

    // ── ExUnits ────────────────────────────────────────────────────────

    #[test]
    fn ex_units_round_trip() {
        let eu = ExUnits { mem: 1_000_000, steps: 2_000_000 };
        let decoded = ExUnits::from_cbor_bytes(&eu.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, eu);
    }

    #[test]
    fn ex_units_zero_round_trip() {
        let eu = ExUnits { mem: 0, steps: 0 };
        let decoded = ExUnits::from_cbor_bytes(&eu.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, eu);
    }

    // ── Redeemer ───────────────────────────────────────────────────────

    #[test]
    fn redeemer_spend_round_trip() {
        let r = Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Integer(42),
            ex_units: ExUnits { mem: 100, steps: 200 },
        };
        let decoded = Redeemer::from_cbor_bytes(&r.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, r);
    }

    #[test]
    fn redeemer_mint_round_trip() {
        let r = Redeemer {
            tag: 1,
            index: 3,
            data: PlutusData::Bytes(vec![0xDE, 0xAD]),
            ex_units: ExUnits { mem: 500, steps: 600 },
        };
        let decoded = Redeemer::from_cbor_bytes(&r.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, r);
    }

    #[test]
    fn redeemer_cert_round_trip() {
        let r = Redeemer {
            tag: 2,
            index: 0,
            data: PlutusData::List(vec![PlutusData::Integer(1)]),
            ex_units: ExUnits { mem: 10, steps: 20 },
        };
        let decoded = Redeemer::from_cbor_bytes(&r.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, r);
    }

    #[test]
    fn redeemer_reward_round_trip() {
        let r = Redeemer {
            tag: 3,
            index: 1,
            data: PlutusData::Constr(0, vec![]),
            ex_units: ExUnits { mem: 300, steps: 400 },
        };
        let decoded = Redeemer::from_cbor_bytes(&r.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, r);
    }

    // ── AlonzoTxOut ────────────────────────────────────────────────────

    #[test]
    fn txout_no_datum_hash_round_trip() {
        let out = mk_alonzo_txout();
        let decoded = AlonzoTxOut::from_cbor_bytes(&out.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, out);
    }

    #[test]
    fn txout_with_datum_hash_round_trip() {
        let out = AlonzoTxOut {
            address: vec![0x01; 57],
            amount: Value::Coin(5_000_000),
            datum_hash: Some([0xCC; 32]),
        };
        let decoded = AlonzoTxOut::from_cbor_bytes(&out.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, out);
    }

    #[test]
    fn txout_datum_hash_absent_vs_present_differ() {
        let without = mk_alonzo_txout();
        let with = AlonzoTxOut {
            datum_hash: Some([0xFF; 32]),
            ..without.clone()
        };
        assert_ne!(without.to_cbor_bytes(), with.to_cbor_bytes());
    }

    // ── AlonzoTxBody ───────────────────────────────────────────────────

    fn mk_alonzo_body() -> AlonzoTxBody {
        AlonzoTxBody {
            inputs: vec![mk_txin(0)],
            outputs: vec![mk_alonzo_txout()],
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
        }
    }

    #[test]
    fn tx_body_minimal_round_trip() {
        let body = mk_alonzo_body();
        let decoded = AlonzoTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_with_alonzo_fields_round_trip() {
        let body = AlonzoTxBody {
            script_data_hash: Some([0x11; 32]),
            collateral: Some(vec![mk_txin(1)]),
            required_signers: Some(vec![[0x22; 28]]),
            network_id: Some(1),
            ..mk_alonzo_body()
        };
        let decoded = AlonzoTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_with_all_optional_round_trip() {
        let body = AlonzoTxBody {
            ttl: Some(500),
            auxiliary_data_hash: Some([0x33; 32]),
            validity_interval_start: Some(10),
            script_data_hash: Some([0x44; 32]),
            collateral: Some(vec![mk_txin(2)]),
            required_signers: Some(vec![[0x55; 28], [0x66; 28]]),
            network_id: Some(0),
            ..mk_alonzo_body()
        };
        let decoded = AlonzoTxBody::from_cbor_bytes(&body.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn tx_body_no_collateral_vs_with_collateral_differ() {
        let base = mk_alonzo_body();
        let with_col = AlonzoTxBody {
            collateral: Some(vec![mk_txin(5)]),
            ..base.clone()
        };
        assert_ne!(base.to_cbor_bytes(), with_col.to_cbor_bytes());
    }
}
