//! Shelley-era transaction and block types.
//!
//! Types match the Shelley CDDL specification:
//! <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/cddl/data/shelley.cddl>

use std::collections::{BTreeMap, HashMap};

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::allegra::NativeScript;
use crate::eras::alonzo::{ExUnits, Redeemer};
use crate::error::LedgerError;
use crate::plutus::PlutusData;
use crate::types::{DCert, RewardAccount};

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
// ShelleyUpdate (typed update proposal)
// ---------------------------------------------------------------------------

/// A Shelley-era protocol parameter update proposal.
///
/// CDDL:
/// ```text
/// update = [ proposed_protocol_parameter_updates, epoch ]
/// proposed_protocol_parameter_updates = { * genesis_hash => protocol_param_update }
/// ```
///
/// The inner `protocol_param_update` values remain as opaque CBOR bytes
/// since the map has 16+ optional keys with complex nested types.
///
/// Reference: `Cardano.Ledger.Shelley.PParams.Update`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyUpdate {
    /// Map from genesis delegate key hash to proposed parameter updates
    /// (each value is opaque CBOR encoding of `protocol_param_update`).
    pub proposed_protocol_parameter_updates: BTreeMap<[u8; 28], Vec<u8>>,
    /// Epoch in which the update takes effect.
    pub epoch: u64,
}

impl CborEncode for ShelleyUpdate {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        enc.map(self.proposed_protocol_parameter_updates.len() as u64);
        for (hash, param_update) in &self.proposed_protocol_parameter_updates {
            enc.bytes(hash);
            enc.raw(param_update);
        }
        enc.unsigned(self.epoch);
    }
}

impl CborDecode for ShelleyUpdate {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let arr_len = dec.array()?;
        if arr_len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: arr_len as usize,
            });
        }
        let map_len = dec.map()?;
        let mut proposed = BTreeMap::new();
        for _ in 0..map_len {
            let raw_hash = dec.bytes()?;
            let hash: [u8; 28] =
                raw_hash
                    .try_into()
                    .map_err(|_| LedgerError::CborInvalidLength {
                        expected: 28,
                        actual: raw_hash.len(),
                    })?;
            let start = dec.position();
            dec.skip()?;
            let end = dec.position();
            let param_update = dec.slice(start, end)?.to_vec();
            proposed.insert(hash, param_update);
        }
        let epoch = dec.unsigned()?;
        Ok(Self {
            proposed_protocol_parameter_updates: proposed,
            epoch,
        })
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
    /// Optional certificates (CDDL key 4).
    pub certificates: Option<Vec<DCert>>,
    /// Optional withdrawals: reward-account → lovelace (CDDL key 5).
    pub withdrawals: Option<BTreeMap<RewardAccount, u64>>,
    /// Optional protocol-parameter update proposal (CDDL key 6).
    pub update: Option<ShelleyUpdate>,
    /// Optional auxiliary data hash (CDDL key 7).
    pub auxiliary_data_hash: Option<[u8; 32]>,
}

impl CborEncode for ShelleyTxBody {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let mut field_count: u64 = 4; // keys 0, 1, 2, 3
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
        let mut certificates: Option<Vec<DCert>> = None;
        let mut withdrawals: Option<BTreeMap<RewardAccount, u64>> = None;
        let mut update: Option<ShelleyUpdate> = None;
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
            certificates,
            withdrawals,
            update,
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
// Bootstrap witness
// ---------------------------------------------------------------------------

/// A bootstrap witness used for Byron-era addresses in post-Byron transactions.
///
/// CDDL: `bootstrap_witness = [public_key : vkey, signature : signature,
///         chain_code : bytes .size 32, attributes : bytes]`
///
/// Reference: `Cardano.Ledger.Shelley.TxWits` — `BootstrapWitness`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapWitness {
    /// 32-byte Ed25519 verification key.
    pub public_key: [u8; 32],
    /// 64-byte Ed25519 signature.
    pub signature: [u8; 64],
    /// 32-byte chain code from Byron HD key derivation.
    pub chain_code: [u8; 32],
    /// Byron-era address attributes (serialized CBOR).
    pub attributes: Vec<u8>,
}

impl CborEncode for BootstrapWitness {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4)
            .bytes(&self.public_key)
            .bytes(&self.signature)
            .bytes(&self.chain_code)
            .bytes(&self.attributes);
    }
}

impl CborDecode for BootstrapWitness {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }
        let pk_raw = dec.bytes()?;
        let public_key: [u8; 32] =
            pk_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: pk_raw.len(),
                })?;
        let sig_raw = dec.bytes()?;
        let signature: [u8; 64] =
            sig_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 64,
                    actual: sig_raw.len(),
                })?;
        let cc_raw = dec.bytes()?;
        let chain_code: [u8; 32] =
            cc_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: cc_raw.len(),
                })?;
        let attributes = dec.bytes()?.to_vec();
        Ok(Self {
            public_key,
            signature,
            chain_code,
            attributes,
        })
    }
}

// ---------------------------------------------------------------------------
// Transaction witness set
// ---------------------------------------------------------------------------

/// The witness set for a transaction (all eras Shelley through Conway).
///
/// CDDL (Conway superset):
/// ```text
/// transaction_witness_set = {
///   ? 0 : [* vkeywitness],
///   ? 1 : [* native_script],
///   ? 2 : [* bootstrap_witness],
///   ? 3 : [* plutus_v1_script],
///   ? 4 : [* plutus_data],
///   ? 5 : redeemers,
///   ? 6 : [* plutus_v2_script],
///   ? 7 : [* plutus_v3_script],
/// }
/// ```
///
/// Reference: `Cardano.Ledger.Shelley.TxWits`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyWitnessSet {
    /// VKey witnesses (CDDL key 0).
    pub vkey_witnesses: Vec<ShelleyVkeyWitness>,
    /// Native scripts (CDDL key 1, Allegra+).
    pub native_scripts: Vec<NativeScript>,
    /// Bootstrap witnesses for Byron-era addresses (CDDL key 2).
    pub bootstrap_witnesses: Vec<BootstrapWitness>,
    /// Plutus V1 scripts as opaque bytes (CDDL key 3, Alonzo+).
    pub plutus_v1_scripts: Vec<Vec<u8>>,
    /// Typed Plutus data items (CDDL key 4, Alonzo+).
    pub plutus_data: Vec<PlutusData>,
    /// Redeemers (CDDL key 5, Alonzo+).
    pub redeemers: Vec<Redeemer>,
    /// Plutus V2 scripts as opaque bytes (CDDL key 6, Babbage+).
    pub plutus_v2_scripts: Vec<Vec<u8>>,
    /// Plutus V3 scripts as opaque bytes (CDDL key 7, Conway+).
    pub plutus_v3_scripts: Vec<Vec<u8>>,
}

impl CborEncode for ShelleyWitnessSet {
    fn encode_cbor(&self, enc: &mut Encoder) {
        let mut count: u64 = 0;
        if !self.vkey_witnesses.is_empty() {
            count += 1;
        }
        if !self.native_scripts.is_empty() {
            count += 1;
        }
        if !self.bootstrap_witnesses.is_empty() {
            count += 1;
        }
        if !self.plutus_v1_scripts.is_empty() {
            count += 1;
        }
        if !self.plutus_data.is_empty() {
            count += 1;
        }
        if !self.redeemers.is_empty() {
            count += 1;
        }
        if !self.plutus_v2_scripts.is_empty() {
            count += 1;
        }
        if !self.plutus_v3_scripts.is_empty() {
            count += 1;
        }
        enc.map(count);

        // Key 0: vkey witnesses
        if !self.vkey_witnesses.is_empty() {
            enc.unsigned(0).array(self.vkey_witnesses.len() as u64);
            for w in &self.vkey_witnesses {
                w.encode_cbor(enc);
            }
        }

        // Key 1: native scripts
        if !self.native_scripts.is_empty() {
            enc.unsigned(1).array(self.native_scripts.len() as u64);
            for s in &self.native_scripts {
                s.encode_cbor(enc);
            }
        }

        // Key 2: bootstrap witnesses
        if !self.bootstrap_witnesses.is_empty() {
            enc.unsigned(2).array(self.bootstrap_witnesses.len() as u64);
            for bw in &self.bootstrap_witnesses {
                bw.encode_cbor(enc);
            }
        }

        // Key 3: plutus v1 scripts
        if !self.plutus_v1_scripts.is_empty() {
            enc.unsigned(3).array(self.plutus_v1_scripts.len() as u64);
            for s in &self.plutus_v1_scripts {
                enc.bytes(s);
            }
        }

        // Key 4: plutus data (typed PlutusData items)
        if !self.plutus_data.is_empty() {
            enc.unsigned(4).array(self.plutus_data.len() as u64);
            for d in &self.plutus_data {
                d.encode_cbor(enc);
            }
        }

        // Key 5: redeemers (encoded as legacy array format)
        if !self.redeemers.is_empty() {
            enc.unsigned(5).array(self.redeemers.len() as u64);
            for r in &self.redeemers {
                r.encode_cbor(enc);
            }
        }

        // Key 6: plutus v2 scripts
        if !self.plutus_v2_scripts.is_empty() {
            enc.unsigned(6).array(self.plutus_v2_scripts.len() as u64);
            for s in &self.plutus_v2_scripts {
                enc.bytes(s);
            }
        }

        // Key 7: plutus v3 scripts
        if !self.plutus_v3_scripts.is_empty() {
            enc.unsigned(7).array(self.plutus_v3_scripts.len() as u64);
            for s in &self.plutus_v3_scripts {
                enc.bytes(s);
            }
        }
    }
}

impl CborDecode for ShelleyWitnessSet {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let map_len = dec.map()?;
        let mut vkey_witnesses = Vec::new();
        let mut native_scripts = Vec::new();
        let mut bootstrap_witnesses = Vec::new();
        let mut plutus_v1_scripts = Vec::new();
        let mut plutus_data = Vec::new();
        let mut redeemers = Vec::new();
        let mut plutus_v2_scripts = Vec::new();
        let mut plutus_v3_scripts = Vec::new();

        for _ in 0..map_len {
            let key = dec.unsigned()?;
            match key {
                0 => {
                    let count = dec.array()?;
                    for _ in 0..count {
                        vkey_witnesses.push(ShelleyVkeyWitness::decode_cbor(dec)?);
                    }
                }
                1 => {
                    let count = dec.array()?;
                    for _ in 0..count {
                        native_scripts.push(NativeScript::decode_cbor(dec)?);
                    }
                }
                2 => {
                    let count = dec.array()?;
                    for _ in 0..count {
                        bootstrap_witnesses.push(BootstrapWitness::decode_cbor(dec)?);
                    }
                }
                3 => {
                    let count = dec.array()?;
                    for _ in 0..count {
                        plutus_v1_scripts.push(dec.bytes()?.to_vec());
                    }
                }
                4 => {
                    let count = dec.array()?;
                    for _ in 0..count {
                        plutus_data.push(PlutusData::decode_cbor(dec)?);
                    }
                }
                5 => {
                    // Redeemers: array format [* redeemer] (Alonzo/Babbage) or
                    // map format { [tag, index] => [data, ex_units] } (Conway).
                    let major = dec.peek_major()?;
                    if major == 4 {
                        let count = dec.array()?;
                        for _ in 0..count {
                            redeemers.push(Redeemer::decode_cbor(dec)?);
                        }
                    } else if major == 5 {
                        let count = dec.map()?;
                        for _ in 0..count {
                            let kl = dec.array()?;
                            if kl != 2 {
                                return Err(LedgerError::CborInvalidLength {
                                    expected: 2,
                                    actual: kl as usize,
                                });
                            }
                            let tag = dec.unsigned()? as u8;
                            let index = dec.unsigned()?;
                            let vl = dec.array()?;
                            if vl != 2 {
                                return Err(LedgerError::CborInvalidLength {
                                    expected: 2,
                                    actual: vl as usize,
                                });
                            }
                            let data = PlutusData::decode_cbor(dec)?;
                            let ex_units = ExUnits::decode_cbor(dec)?;
                            redeemers.push(Redeemer {
                                tag,
                                index,
                                data,
                                ex_units,
                            });
                        }
                    } else {
                        return Err(LedgerError::CborTypeMismatch {
                            expected: 4,
                            actual: major,
                        });
                    }
                }
                6 => {
                    let count = dec.array()?;
                    for _ in 0..count {
                        plutus_v2_scripts.push(dec.bytes()?.to_vec());
                    }
                }
                7 => {
                    let count = dec.array()?;
                    for _ in 0..count {
                        plutus_v3_scripts.push(dec.bytes()?.to_vec());
                    }
                }
                _ => {
                    dec.skip()?;
                }
            }
        }

        Ok(Self {
            vkey_witnesses,
            native_scripts,
            bootstrap_witnesses,
            plutus_v1_scripts,
            plutus_data,
            redeemers,
            plutus_v2_scripts,
            plutus_v3_scripts,
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

impl CborEncode for ShelleyUtxo {
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

impl CborDecode for ShelleyUtxo {
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
            let txout = ShelleyTxOut::decode_cbor(dec)?;
            entries.insert(txin, txout);
        }

        Ok(Self { entries })
    }
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

    /// Iterates over all UTxO entries.
    pub fn iter(&self) -> impl Iterator<Item = (&ShelleyTxIn, &ShelleyTxOut)> {
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
        self.apply_tx_with_withdrawals(tx_id, body, current_slot, 0)
    }

    /// Applies a Shelley transaction body with a pre-validated withdrawal total.
    pub fn apply_tx_with_withdrawals(
        &mut self,
        tx_id: [u8; 32],
        body: &ShelleyTxBody,
        current_slot: u64,
        withdrawal_total: u64,
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
        let available = consumed.saturating_add(withdrawal_total);
        if available != produced.saturating_add(body.fee) {
            return Err(LedgerError::ValueNotPreserved {
                consumed: available,
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

// ---------------------------------------------------------------------------
// VRF certificate (output + proof)
// ---------------------------------------------------------------------------

/// A VRF certificate: the output hash followed by the 80-byte VRF proof.
///
/// CDDL: `vrf_cert = [bytes, bytes .size 80]`
///
/// Reference: `Cardano.Protocol.TPraos.BHeader` — `CertifiedVRF`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyVrfCert {
    /// VRF output (hash of the VRF proof — typically 32 bytes).
    pub output: Vec<u8>,
    /// VRF proof (80 bytes for ECVRF).
    pub proof: [u8; 80],
}

impl CborEncode for ShelleyVrfCert {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2).bytes(&self.output).bytes(&self.proof);
    }
}

impl CborDecode for ShelleyVrfCert {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let output = dec.bytes()?.to_vec();
        let proof_raw = dec.bytes()?;
        let proof: [u8; 80] =
            proof_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 80,
                    actual: proof_raw.len(),
                })?;
        Ok(Self { output, proof })
    }
}

// ---------------------------------------------------------------------------
// Operational certificate (wire format)
// ---------------------------------------------------------------------------

/// A Shelley-era operational certificate in wire format.
///
/// CDDL (inlined group in header_body):
/// ```text
/// operational_cert = (
///   hot_vkey    : kes_vkey,          ; 32 bytes
///   sequence_number : uint .size 8,
///   kes_period  : uint .size 8,
///   sigma       : ed25519_signature  ; 64 bytes
/// )
/// ```
///
/// Reference: `Cardano.Ledger.OCert`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyOpCert {
    /// Hot KES verification key (32 bytes).
    pub hot_vkey: [u8; 32],
    /// Monotonically increasing counter.
    pub sequence_number: u64,
    /// KES period in which the certificate starts.
    pub kes_period: u64,
    /// Ed25519 signature of (hot_vkey || sequence_number || kes_period)
    /// by the cold key.
    pub sigma: [u8; 64],
}

impl ShelleyOpCert {
    /// Encode the group fields into a parent array encoder (no array
    /// header — group is inlined).
    pub fn encode_fields(&self, enc: &mut Encoder) {
        enc.bytes(&self.hot_vkey)
            .unsigned(self.sequence_number)
            .unsigned(self.kes_period)
            .bytes(&self.sigma);
    }

    /// Decode the group fields from a parent array decoder (no array
    /// header — group is inlined).
    pub fn decode_fields(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let hot_raw = dec.bytes()?;
        let hot_vkey: [u8; 32] =
            hot_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: hot_raw.len(),
                })?;
        let sequence_number = dec.unsigned()?;
        let kes_period = dec.unsigned()?;
        let sig_raw = dec.bytes()?;
        let sigma: [u8; 64] =
            sig_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 64,
                    actual: sig_raw.len(),
                })?;
        Ok(Self {
            hot_vkey,
            sequence_number,
            kes_period,
            sigma,
        })
    }
}

// ---------------------------------------------------------------------------
// Shelley header body
// ---------------------------------------------------------------------------

/// The body of a Shelley-era block header — all chain-indexing fields.
///
/// CDDL:
/// ```text
/// header_body = [
///   block_number, slot, prev_hash,
///   issuer_vkey, vrf_vkey,
///   nonce_vrf, leader_vrf,
///   block_body_size, block_body_hash,
///   operational_cert,  ; 4 inlined group fields
///   protocol_version   ; 2 inlined group fields
/// ]
/// ```
///
/// Total: 15 elements in the CBOR array.
///
/// Reference: `Cardano.Protocol.TPraos.BHeader` — `BHBody`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyHeaderBody {
    /// Block height.
    pub block_number: u64,
    /// Slot in which this block was issued.
    pub slot: u64,
    /// Hash of the previous block header (`None` for genesis successor).
    pub prev_hash: Option<[u8; 32]>,
    /// Block issuer's Ed25519 verification key (32 bytes).
    pub issuer_vkey: [u8; 32],
    /// Block issuer's VRF verification key (32 bytes).
    pub vrf_vkey: [u8; 32],
    /// VRF certificate for the nonce contribution.
    pub nonce_vrf: ShelleyVrfCert,
    /// VRF certificate for the leader election proof.
    pub leader_vrf: ShelleyVrfCert,
    /// Size of the block body in bytes.
    pub block_body_size: u32,
    /// Blake2b-256 hash of the block body.
    pub block_body_hash: [u8; 32],
    /// Operational certificate.
    pub operational_cert: ShelleyOpCert,
    /// Protocol version (major, minor).
    pub protocol_version: (u64, u64),
}

impl CborEncode for ShelleyHeaderBody {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(15);

        enc.unsigned(self.block_number);
        enc.unsigned(self.slot);

        // prev_hash: hash32 / nil
        match &self.prev_hash {
            Some(h) => { enc.bytes(h); }
            None => { enc.null(); }
        }

        enc.bytes(&self.issuer_vkey);
        enc.bytes(&self.vrf_vkey);

        self.nonce_vrf.encode_cbor(enc);
        self.leader_vrf.encode_cbor(enc);

        enc.unsigned(u64::from(self.block_body_size));
        enc.bytes(&self.block_body_hash);

        // operational_cert group (4 fields inlined)
        self.operational_cert.encode_fields(enc);

        // protocol_version group (2 fields inlined)
        enc.unsigned(self.protocol_version.0);
        enc.unsigned(self.protocol_version.1);
    }
}

impl CborDecode for ShelleyHeaderBody {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 15 {
            return Err(LedgerError::CborInvalidLength {
                expected: 15,
                actual: len as usize,
            });
        }

        let block_number = dec.unsigned()?;
        let slot = dec.unsigned()?;

        // prev_hash: hash32 / nil
        let prev_hash = if dec.peek_major()? == 7 {
            dec.null()?;
            None
        } else {
            let raw = dec.bytes()?;
            let h: [u8; 32] =
                raw.try_into()
                    .map_err(|_| LedgerError::CborInvalidLength {
                        expected: 32,
                        actual: raw.len(),
                    })?;
            Some(h)
        };

        let iv_raw = dec.bytes()?;
        let issuer_vkey: [u8; 32] =
            iv_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: iv_raw.len(),
                })?;

        let vv_raw = dec.bytes()?;
        let vrf_vkey: [u8; 32] =
            vv_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: vv_raw.len(),
                })?;

        let nonce_vrf = ShelleyVrfCert::decode_cbor(dec)?;
        let leader_vrf = ShelleyVrfCert::decode_cbor(dec)?;

        let body_size = dec.unsigned()? as u32;

        let bh_raw = dec.bytes()?;
        let body_hash: [u8; 32] =
            bh_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: bh_raw.len(),
                })?;

        let opcert = ShelleyOpCert::decode_fields(dec)?;

        let proto_major = dec.unsigned()?;
        let proto_minor = dec.unsigned()?;

        Ok(Self {
            block_number,
            slot,
            prev_hash,
            issuer_vkey,
            vrf_vkey,
            nonce_vrf,
            leader_vrf,
            block_body_size: body_size,
            block_body_hash: body_hash,
            operational_cert: opcert,
            protocol_version: (proto_major, proto_minor),
        })
    }
}

// ---------------------------------------------------------------------------
// Shelley header (header_body + KES signature)
// ---------------------------------------------------------------------------

/// A signed Shelley-era block header: the body plus a KES signature.
///
/// CDDL: `header = [header_body, body_signature : kes_signature]`
///
/// The KES signature is stored as opaque bytes (448 bytes for depth-6
/// SumKES) because the verification logic lives in the consensus crate.
///
/// Reference: `Cardano.Protocol.TPraos.BHeader` — `BHeader`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyHeader {
    /// The header body (chain-indexing + VRF + opcert + version).
    pub body: ShelleyHeaderBody,
    /// KES signature bytes over the serialized header body.
    pub signature: Vec<u8>,
}

impl CborEncode for ShelleyHeader {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        self.body.encode_cbor(enc);
        enc.bytes(&self.signature);
    }
}

impl CborDecode for ShelleyHeader {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let body = ShelleyHeaderBody::decode_cbor(dec)?;
        let signature = dec.bytes()?.to_vec();
        Ok(Self { body, signature })
    }
}

impl ShelleyHeader {
    /// Compute the Blake2b-256 hash of this header's CBOR encoding.
    ///
    /// This is the canonical block identifier used in `Point(slot, hash)`
    /// and the `HeaderHash` type. The hash covers the full header including
    /// the KES signature.
    ///
    /// Reference: `Ouroboros.Consensus.Block.Abstract` — `HeaderHash`.
    pub fn header_hash(&self) -> crate::types::HeaderHash {
        let cbor = self.to_cbor_bytes();
        let digest = yggdrasil_crypto::hash_bytes_256(&cbor);
        crate::types::HeaderHash(digest.0)
    }
}

// ---------------------------------------------------------------------------
// Praos header body (Babbage / Conway)
// ---------------------------------------------------------------------------

/// The body of a Praos-era block header (Babbage and Conway).
///
/// In Babbage/Conway, the two separate VRF certificates (`nonce_vrf` and
/// `leader_vrf`) from the Shelley-era header are consolidated into a single
/// `vrf_result`.  This yields a 14-element CBOR array (versus 15 in
/// Shelley).
///
/// CDDL:
/// ```text
/// header_body = [
///   block_number, slot, prev_hash,
///   issuer_vkey, vrf_vkey,
///   vrf_result,
///   block_body_size, block_body_hash,
///   operational_cert,  ; 4 inlined group fields
///   protocol_version   ; 2 inlined group fields
/// ]
/// ```
///
/// Total: 14 elements in the CBOR array.
///
/// Reference: `Cardano.Ledger.Block` — `HeaderBody`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PraosHeaderBody {
    /// Block height.
    pub block_number: u64,
    /// Slot in which this block was issued.
    pub slot: u64,
    /// Hash of the previous block header (`None` for genesis successor).
    pub prev_hash: Option<[u8; 32]>,
    /// Block issuer's Ed25519 verification key (32 bytes).
    pub issuer_vkey: [u8; 32],
    /// Block issuer's VRF verification key (32 bytes).
    pub vrf_vkey: [u8; 32],
    /// Combined VRF result (output + proof).
    pub vrf_result: ShelleyVrfCert,
    /// Size of the block body in bytes.
    pub block_body_size: u32,
    /// Blake2b-256 hash of the block body.
    pub block_body_hash: [u8; 32],
    /// Operational certificate.
    pub operational_cert: ShelleyOpCert,
    /// Protocol version (major, minor).
    pub protocol_version: (u64, u64),
}

impl CborEncode for PraosHeaderBody {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(14);

        enc.unsigned(self.block_number);
        enc.unsigned(self.slot);

        // prev_hash: hash32 / nil
        match &self.prev_hash {
            Some(h) => { enc.bytes(h); }
            None => { enc.null(); }
        }

        enc.bytes(&self.issuer_vkey);
        enc.bytes(&self.vrf_vkey);

        self.vrf_result.encode_cbor(enc);

        enc.unsigned(u64::from(self.block_body_size));
        enc.bytes(&self.block_body_hash);

        // operational_cert group (4 fields inlined)
        self.operational_cert.encode_fields(enc);

        // protocol_version group (2 fields inlined)
        enc.unsigned(self.protocol_version.0);
        enc.unsigned(self.protocol_version.1);
    }
}

impl CborDecode for PraosHeaderBody {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 14 {
            return Err(LedgerError::CborInvalidLength {
                expected: 14,
                actual: len as usize,
            });
        }

        let block_number = dec.unsigned()?;
        let slot = dec.unsigned()?;

        // prev_hash: hash32 / nil
        let prev_hash = if dec.peek_major()? == 7 {
            dec.null()?;
            None
        } else {
            let raw = dec.bytes()?;
            let h: [u8; 32] =
                raw.try_into()
                    .map_err(|_| LedgerError::CborInvalidLength {
                        expected: 32,
                        actual: raw.len(),
                    })?;
            Some(h)
        };

        let iv_raw = dec.bytes()?;
        let issuer_vkey: [u8; 32] =
            iv_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: iv_raw.len(),
                })?;

        let vv_raw = dec.bytes()?;
        let vrf_vkey: [u8; 32] =
            vv_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: vv_raw.len(),
                })?;

        let vrf_result = ShelleyVrfCert::decode_cbor(dec)?;

        let body_size = dec.unsigned()? as u32;

        let bh_raw = dec.bytes()?;
        let body_hash: [u8; 32] =
            bh_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: bh_raw.len(),
                })?;

        let opcert = ShelleyOpCert::decode_fields(dec)?;

        let proto_major = dec.unsigned()?;
        let proto_minor = dec.unsigned()?;

        Ok(Self {
            block_number,
            slot,
            prev_hash,
            issuer_vkey,
            vrf_vkey,
            vrf_result,
            block_body_size: body_size,
            block_body_hash: body_hash,
            operational_cert: opcert,
            protocol_version: (proto_major, proto_minor),
        })
    }
}

// ---------------------------------------------------------------------------
// Praos header (Babbage / Conway)
// ---------------------------------------------------------------------------

/// A signed Praos-era block header: the body plus a KES signature.
///
/// CDDL: `header = [header_body, body_signature : kes_signature]`
///
/// Reference: `Cardano.Ledger.Block` — `Header`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PraosHeader {
    /// The header body (chain-indexing + VRF result + opcert + version).
    pub body: PraosHeaderBody,
    /// KES signature bytes over the serialized header body.
    pub signature: Vec<u8>,
}

impl CborEncode for PraosHeader {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        self.body.encode_cbor(enc);
        enc.bytes(&self.signature);
    }
}

impl CborDecode for PraosHeader {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let body = PraosHeaderBody::decode_cbor(dec)?;
        let signature = dec.bytes()?.to_vec();
        Ok(Self { body, signature })
    }
}

impl PraosHeader {
    /// Compute the Blake2b-256 hash of this header's CBOR encoding.
    pub fn header_hash(&self) -> crate::types::HeaderHash {
        let cbor = self.to_cbor_bytes();
        let digest = yggdrasil_crypto::hash_bytes_256(&cbor);
        crate::types::HeaderHash(digest.0)
    }
}

impl ShelleyBlock {
    /// Compute the Blake2b-256 header hash for this block.
    ///
    /// Equivalent to `self.header.header_hash()`.
    pub fn header_hash(&self) -> crate::types::HeaderHash {
        self.header.header_hash()
    }
}

// ---------------------------------------------------------------------------
// Block body hash computation
// ---------------------------------------------------------------------------

/// Compute the Blake2b-256 block body hash from raw block CBOR bytes.
///
/// In Cardano, the block body hash covers all block array elements after
/// the header. The hash is computed over the raw serialized CBOR bytes of
/// those elements concatenated:
///
/// - Shelley/Allegra/Mary (4-element block):
///   `Blake2b-256(txBodies_bytes || witnesses_bytes || metadata_bytes)`
/// - Alonzo/Babbage/Conway (5-element block):
///   `Blake2b-256(txBodies_bytes || witnesses_bytes || auxData_bytes || invalidTxs_bytes)`
///
/// The `block_bytes` parameter is the raw CBOR of the inner block (after
/// the multi-era `[era_tag, block_body]` envelope has been peeled).
///
/// Reference: `Cardano.Ledger.Shelley.BlockChain.bbHash` (Shelley) and
/// `Cardano.Ledger.Alonzo.TxSeq.hashAnnotated` (Alonzo onwards).
pub fn compute_block_body_hash(block_bytes: &[u8]) -> Result<[u8; 32], crate::LedgerError> {
    let mut dec = crate::cbor::Decoder::new(block_bytes);
    let arr_len = dec.array()?;
    if !(4..=5).contains(&arr_len) {
        return Err(crate::LedgerError::CborInvalidLength {
            expected: 4,
            actual: arr_len as usize,
        });
    }

    // Skip element 0 (header).
    dec.skip()?;

    // Elements 1..arr_len are the block body.
    let body_start = dec.position();
    for _ in 1..arr_len {
        dec.skip()?;
    }
    let body_end = dec.position();

    let body_bytes = dec.slice(body_start, body_end)?;
    Ok(yggdrasil_crypto::hash_bytes_256(body_bytes).0)
}

// ---------------------------------------------------------------------------
// Full Shelley block
// ---------------------------------------------------------------------------

/// A complete Shelley-era block as it appears on the wire.
///
/// CDDL:
/// ```text
/// block = [
///   header,
///   transaction_bodies   : [* transaction_body],
///   transaction_witness_sets : [* transaction_witness_set],
///   transaction_metadata_set : {* uint => metadata}
/// ]
/// ```
///
/// Metadata is stored opaquely per-index for now.
///
/// Reference: `Cardano.Ledger.Shelley.Block`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyBlock {
    /// The signed block header.
    pub header: ShelleyHeader,
    /// Transaction bodies (parallel to witness_sets).
    pub transaction_bodies: Vec<ShelleyTxBody>,
    /// Witness sets (parallel to transaction_bodies).
    pub transaction_witness_sets: Vec<ShelleyWitnessSet>,
    /// Metadata map: transaction index → raw CBOR metadata bytes.
    pub transaction_metadata_set: HashMap<u64, Vec<u8>>,
}

impl CborEncode for ShelleyBlock {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4);
        self.header.encode_cbor(enc);

        // transaction_bodies
        enc.array(self.transaction_bodies.len() as u64);
        for body in &self.transaction_bodies {
            body.encode_cbor(enc);
        }

        // transaction_witness_sets
        enc.array(self.transaction_witness_sets.len() as u64);
        for ws in &self.transaction_witness_sets {
            ws.encode_cbor(enc);
        }

        // transaction_metadata_set
        enc.map(self.transaction_metadata_set.len() as u64);
        for (&idx, meta) in &self.transaction_metadata_set {
            enc.unsigned(idx);
            enc.raw(meta);
        }
    }
}

impl CborDecode for ShelleyBlock {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }

        let header = ShelleyHeader::decode_cbor(dec)?;

        // transaction_bodies
        let tb_count = dec.array()?;
        let mut transaction_bodies = Vec::with_capacity(tb_count as usize);
        for _ in 0..tb_count {
            transaction_bodies.push(ShelleyTxBody::decode_cbor(dec)?);
        }

        // transaction_witness_sets
        let ws_count = dec.array()?;
        let mut witness_sets = Vec::with_capacity(ws_count as usize);
        for _ in 0..ws_count {
            witness_sets.push(ShelleyWitnessSet::decode_cbor(dec)?);
        }

        // transaction_metadata_set
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

        Ok(Self {
            header,
            transaction_bodies,
            transaction_witness_sets: witness_sets,
            transaction_metadata_set: transaction_metadata,
        })
    }
}
