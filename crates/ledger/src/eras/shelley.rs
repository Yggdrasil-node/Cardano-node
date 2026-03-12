//! Shelley-era transaction and block types.
//!
//! Types match the Shelley CDDL specification:
//! <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/cddl/data/shelley.cddl>

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
