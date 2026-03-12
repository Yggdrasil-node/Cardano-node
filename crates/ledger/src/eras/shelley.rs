//! Shelley-era transaction and block types.
//!
//! Types match the Shelley CDDL specification:
//! <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/cddl/data/shelley.cddl>

use std::collections::HashMap;

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::error::LedgerError;

pub const SHELLEY_NAME: &str = "Shelley";

// ---------------------------------------------------------------------------
// Transaction input
// ---------------------------------------------------------------------------

/// A reference to a UTxO entry: a transaction ID and output index.
///
/// CDDL: `transaction_input = [id : transaction_id, index : uint .size 2]`
///
/// Reference: `Cardano.Ledger.TxIn` — `TxIn`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ShelleyTxIn {
    /// Blake2b-256 hash of the transaction body that created this output.
    pub transaction_id: [u8; 32],
    /// Output index within that transaction.
    pub index: u16,
}

impl CborEncode for ShelleyTxIn {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2)
            .bytes(&self.transaction_id)
            .unsigned(u64::from(self.index));
    }
}

impl CborDecode for ShelleyTxIn {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let raw = dec.bytes()?;
        let transaction_id: [u8; 32] =
            raw.try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: raw.len(),
                })?;
        let index = dec.unsigned()? as u16;
        Ok(Self {
            transaction_id,
            index,
        })
    }
}

// ---------------------------------------------------------------------------
// Transaction output
// ---------------------------------------------------------------------------

/// A Shelley transaction output: an address receiving a lovelace amount.
///
/// CDDL: `transaction_output = [address, amount : coin]`
///
/// Reference: `Cardano.Ledger.Shelley.TxOut`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyTxOut {
    /// Raw address bytes (encoding varies by address type).
    pub address: Vec<u8>,
    /// Amount in lovelace.
    pub amount: u64,
}

impl CborEncode for ShelleyTxOut {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2).bytes(&self.address).unsigned(self.amount);
    }
}

impl CborDecode for ShelleyTxOut {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let address = dec.bytes()?.to_vec();
        let amount = dec.unsigned()?;
        Ok(Self { address, amount })
    }
}

// ---------------------------------------------------------------------------
// Transaction body
// ---------------------------------------------------------------------------

/// The body of a Shelley-era transaction — the content that is signed.
///
/// CDDL:
/// ```text
/// transaction_body =
///   {   0 : set<transaction_input>
///   ,   1 : [* transaction_output]
///   ,   2 : coin
///   ,   3 : slot
///   , ? 4 : [* certificate]
///   , ? 5 : withdrawals
///   , ? 6 : update
///   , ? 7 : auxiliary_data_hash
///   }
/// ```
///
/// Only the required fields (0–3) and optional metadata hash (7) are
/// modeled in this initial slice.  Certificates, withdrawals, and updates
/// will be added when the corresponding types land.
///
/// Reference: `Cardano.Ledger.Shelley.TxBody`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyTxBody {
    /// Set of transaction inputs (CDDL key 0).
    pub inputs: Vec<ShelleyTxIn>,
    /// Sequence of transaction outputs (CDDL key 1).
    pub outputs: Vec<ShelleyTxOut>,
    /// Transaction fee in lovelace (CDDL key 2).
    pub fee: u64,
    /// Time-to-live — slot after which this transaction is invalid (CDDL key 3).
    pub ttl: u64,
    /// Optional auxiliary data hash (CDDL key 7).
    pub auxiliary_data_hash: Option<[u8; 32]>,
}

impl CborEncode for ShelleyTxBody {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let field_count: u64 = 4 + u64::from(self.auxiliary_data_hash.is_some());
        enc.map(field_count);

        // Key 0: inputs (set encoded as array).
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
        enc.unsigned(3).unsigned(self.ttl);

        // Key 7: auxiliary_data_hash (optional).
        if let Some(hash) = &self.auxiliary_data_hash {
            enc.unsigned(7).bytes(hash);
        }
    }
}

impl CborDecode for ShelleyTxBody {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let map_len = dec.map()?;

        let mut inputs: Option<Vec<ShelleyTxIn>> = None;
        let mut outputs: Option<Vec<ShelleyTxOut>> = None;
        let mut fee: Option<u64> = None;
        let mut ttl: Option<u64> = None;
        let mut auxiliary_data_hash: Option<[u8; 32]> = None;

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
                        outs.push(ShelleyTxOut::decode_cbor(dec)?);
                    }
                    outputs = Some(outs);
                }
                2 => {
                    fee = Some(dec.unsigned()?);
                }
                3 => {
                    ttl = Some(dec.unsigned()?);
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
                _ => {
                    // Skip unknown fields for forward compatibility.
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
            ttl: ttl.ok_or(LedgerError::CborInvalidLength {
                expected: 1,
                actual: 0,
            })?,
            auxiliary_data_hash,
        })
    }
}

// ---------------------------------------------------------------------------
// VKey witness
// ---------------------------------------------------------------------------

/// A verification-key witness: a public key and its signature.
///
/// CDDL: `vkeywitness = [vkey, signature]`
///
/// Reference: `Cardano.Ledger.Shelley.TxWits` — `WitVKey`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyVkeyWitness {
    /// 32-byte Ed25519 verification key.
    pub vkey: [u8; 32],
    /// 64-byte Ed25519 signature.
    pub signature: [u8; 64],
}

impl CborEncode for ShelleyVkeyWitness {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2).bytes(&self.vkey).bytes(&self.signature);
    }
}

impl CborDecode for ShelleyVkeyWitness {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let vkey_raw = dec.bytes()?;
        let vkey: [u8; 32] =
            vkey_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: vkey_raw.len(),
                })?;
        let sig_raw = dec.bytes()?;
        let signature: [u8; 64] =
            sig_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 64,
                    actual: sig_raw.len(),
                })?;
        Ok(Self { vkey, signature })
    }
}

// ---------------------------------------------------------------------------
// Transaction witness set
// ---------------------------------------------------------------------------

/// The witness set for a Shelley-era transaction.
///
/// CDDL: `transaction_witness_set = {? 0 : [* vkeywitness], ? 1 : [* native_script], ? 2 : [* bootstrap_witness]}`
///
/// Only VKey witnesses are modeled in this initial slice.
///
/// Reference: `Cardano.Ledger.Shelley.TxWits`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyWitnessSet {
    /// VKey witnesses (CDDL key 0).
    pub vkey_witnesses: Vec<ShelleyVkeyWitness>,
    /// Bootstrap witnesses are stored as opaque bytes for now (CDDL key 2).
    pub bootstrap_witnesses: Vec<Vec<u8>>,
}

impl CborEncode for ShelleyWitnessSet {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let mut count: u64 = 0;
        if !self.vkey_witnesses.is_empty() {
            count += 1;
        }
        if !self.bootstrap_witnesses.is_empty() {
            count += 1;
        }
        enc.map(count);

        if !self.vkey_witnesses.is_empty() {
            enc.unsigned(0).array(self.vkey_witnesses.len() as u64);
            for w in &self.vkey_witnesses {
                w.encode_cbor(enc);
            }
        }

        if !self.bootstrap_witnesses.is_empty() {
            enc.unsigned(2).array(self.bootstrap_witnesses.len() as u64);
            for bw in &self.bootstrap_witnesses {
                enc.bytes(bw);
            }
        }
    }
}

impl CborDecode for ShelleyWitnessSet {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let map_len = dec.map()?;
        let mut vkey_witnesses = Vec::new();
        let mut bootstrap_witnesses = Vec::new();

        for _ in 0..map_len {
            let key = dec.unsigned()?;
            match key {
                0 => {
                    let count = dec.array()?;
                    for _ in 0..count {
                        vkey_witnesses.push(ShelleyVkeyWitness::decode_cbor(dec)?);
                    }
                }
                2 => {
                    let count = dec.array()?;
                    for _ in 0..count {
                        bootstrap_witnesses.push(dec.bytes()?.to_vec());
                    }
                }
                _ => {
                    dec.skip()?;
                }
            }
        }

        Ok(Self {
            vkey_witnesses,
            bootstrap_witnesses,
        })
    }
}

// ---------------------------------------------------------------------------
// Full Shelley transaction
// ---------------------------------------------------------------------------

/// A complete Shelley-era transaction: body + witnesses + optional metadata.
///
/// CDDL: `transaction = [transaction_body, transaction_witness_set, metadata / nil]`
///
/// Metadata is stored as opaque CBOR bytes in this initial slice.
///
/// Reference: `Cardano.Ledger.Shelley.Tx`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyTx {
    /// The transaction body (signed content).
    pub body: ShelleyTxBody,
    /// Witness set (signatures, scripts, etc.).
    pub witness_set: ShelleyWitnessSet,
    /// Optional auxiliary data (metadata), stored as raw CBOR bytes.
    pub auxiliary_data: Option<Vec<u8>>,
}

impl CborEncode for ShelleyTx {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(3);
        self.body.encode_cbor(enc);
        self.witness_set.encode_cbor(enc);
        match &self.auxiliary_data {
            Some(data) => {
                enc.raw(data);
            }
            None => {
                enc.null();
            }
        }
    }
}

impl CborDecode for ShelleyTx {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            });
        }
        let body = ShelleyTxBody::decode_cbor(dec)?;
        let witness_set = ShelleyWitnessSet::decode_cbor(dec)?;

        let auxiliary_data = if dec.peek_major()? == 7 {
            // null
            dec.null()?;
            None
        } else {
            // Capture raw CBOR for metadata.
            let start = dec.position();
            dec.skip()?;
            let end = dec.position();
            Some(dec.slice(start, end)?.to_vec())
        };

        Ok(Self {
            body,
            witness_set,
            auxiliary_data,
        })
    }
}

// ---------------------------------------------------------------------------
// UTxO set and state transition
// ---------------------------------------------------------------------------

/// The Shelley-era UTxO set: unspent transaction outputs indexed by their
/// producing input reference.
///
/// Reference: `Cardano.Ledger.UTxO` — `UTxO`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShelleyUtxo {
    entries: HashMap<ShelleyTxIn, ShelleyTxOut>,
}

impl ShelleyUtxo {
    /// Creates an empty UTxO set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a UTxO entry.
    pub fn insert(&mut self, txin: ShelleyTxIn, txout: ShelleyTxOut) {
        self.entries.insert(txin, txout);
    }

    /// Looks up a UTxO entry.
    pub fn get(&self, txin: &ShelleyTxIn) -> Option<&ShelleyTxOut> {
        self.entries.get(txin)
    }

    /// Returns the number of entries in the UTxO set.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when the set is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Applies a Shelley transaction body to this UTxO set.
    ///
    /// Validates the Shelley UTXO transition rules (simplified — no
    /// certificates, withdrawals, or deposit accounting in this slice):
    ///
    /// 1. The transaction must have at least one input and one output.
    /// 2. TTL check: `current_slot ≤ tx.ttl`.
    /// 3. All inputs must exist in the UTxO set.
    /// 4. Value preservation: `sum(consumed inputs) = sum(outputs) + fee`.
    /// 5. On success: inputs removed, new outputs added.
    ///
    /// `tx_id` is the hash that identifies this transaction (normally
    /// Blake2b-256 of the serialized body).
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — UTXO STS.
    pub fn apply_tx(
        &mut self,
        tx_id: [u8; 32],
        body: &ShelleyTxBody,
        current_slot: u64,
    ) -> Result<(), LedgerError> {
        // 1. Non-empty inputs / outputs.
        if body.inputs.is_empty() {
            return Err(LedgerError::NoInputs);
        }
        if body.outputs.is_empty() {
            return Err(LedgerError::NoOutputs);
        }

        // 2. TTL check.
        if current_slot > body.ttl {
            return Err(LedgerError::TxExpired {
                ttl: body.ttl,
                slot: current_slot,
            });
        }

        // 3. Input existence and consumed value.
        let mut consumed: u64 = 0;
        for input in &body.inputs {
            let utxo_entry = self.get(input).ok_or(LedgerError::InputNotInUtxo)?;
            consumed = consumed.saturating_add(utxo_entry.amount);
        }

        // 4. Value preservation.
        let produced: u64 = body
            .outputs
            .iter()
            .map(|o| o.amount)
            .fold(0u64, u64::saturating_add);
        if consumed != produced.saturating_add(body.fee) {
            return Err(LedgerError::ValueNotPreserved {
                consumed,
                produced,
                fee: body.fee,
            });
        }

        // 5. State update: remove inputs, add outputs.
        for input in &body.inputs {
            self.entries.remove(input);
        }
        for (idx, output) in body.outputs.iter().enumerate() {
            let new_txin = ShelleyTxIn {
                transaction_id: tx_id,
                index: idx as u16,
            };
            self.entries.insert(new_txin, output.clone());
        }

        Ok(())
    }
}
