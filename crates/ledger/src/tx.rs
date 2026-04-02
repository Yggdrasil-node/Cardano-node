use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::Era;
use crate::eras::{
    AllegraTxBody, AlonzoTxBody, BabbageTxBody, ConwayTxBody, ExUnits,
    MaryTxBody, ShelleyTx, ShelleyTxIn, ShelleyWitnessSet,
};
use crate::error::LedgerError;
use crate::types::{BlockNo, HeaderHash, SlotNo, TxId};

/// Compute a `TxId` as the Blake2b-256 hash of a CBOR-encoded transaction body.
///
/// Reference: `Cardano.Ledger.Core` — `txIdTxBody`.
pub fn compute_tx_id(body_bytes: &[u8]) -> TxId {
    TxId(yggdrasil_crypto::hash_bytes_256(body_bytes).0)
}

/// A transaction identified by its body hash.
///
/// The `body` field holds the transaction's opaque serialized payload until
/// typed CBOR codec work lands. The `id` is the Blake2b-256 hash of that
/// payload.
///
/// Reference: `Cardano.Ledger.Core` — `Tx` / `TxId`.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tx {
    /// Blake2b-256 hash of the serialized transaction body.
    pub id: TxId,
    /// Opaque serialized transaction body (to be replaced by typed payload).
    pub body: Vec<u8>,
    /// Optional serialized witness set (CBOR-encoded `ShelleyWitnessSet`).
    ///
    /// Populated when the block carries witness data alongside transaction
    /// bodies (all Shelley+ eras). Used by the ledger to perform VKey
    /// witness sufficiency validation during block application.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witnesses: Option<Vec<u8>>,
    /// Optional auxiliary data (metadata) carried by this transaction,
    /// stored as raw CBOR bytes from the block-level auxiliary data map.
    ///
    /// Populated during block conversion from era-specific blocks.
    /// Used by the ledger to validate `auxiliary_data_hash` integrity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auxiliary_data: Option<Vec<u8>>,
    /// Phase-2 validation flag (Alonzo+).
    ///
    /// `None` for pre-Alonzo eras (Byron/Shelley/Allegra/Mary).
    /// `Some(true)` when the block producer asserts all Phase-2 scripts passed.
    /// `Some(false)` when Phase-2 validation failed — the ledger applies a
    /// collateral-only transition instead of the normal UTxO transition.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Tx` — `isValid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_valid: Option<bool>,
}

impl Tx {
    /// Compute the full serialized transaction size matching the on-wire CBOR
    /// array encoding.
    ///
    /// Pre-Alonzo (Shelley/Allegra/Mary): `[body, witnesses, auxiliary_data / null]` (3 elements).
    /// Alonzo+: `[body, witnesses, is_valid, auxiliary_data / null]` (4 elements).
    ///
    /// Upstream `validateMaxTxSizeUTxO` and the linear fee formula both use this
    /// full serialized size, not just the body bytes.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `validateMaxTxSizeUTxO`.
    pub fn serialized_size(&self) -> usize {
        let alonzo_plus = self.is_valid.is_some();
        let n_elems: u8 = if alonzo_plus { 4 } else { 3 };
        // CBOR array header: 1 byte for array(3) or array(4) (both < 24)
        let header_size: usize = 1;
        let body_size = self.body.len();
        let witness_size = self.witnesses.as_ref().map_or(1, |w| w.len()); // null = 1 byte
        let is_valid_size: usize = if alonzo_plus { 1 } else { 0 }; // CBOR bool = 1 byte
        let aux_data_size = self.auxiliary_data.as_ref().map_or(1, |a| a.len()); // null = 1 byte
        let _ = n_elems; // used conceptually for documentation
        header_size + body_size + witness_size + is_valid_size + aux_data_size
    }
}

/// A submitted transaction using the 3-element Shelley-family wire shape:
/// body, witness set, and optional auxiliary data.
///
/// This shape is shared by Shelley, Allegra, and Mary transactions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShelleyCompatibleSubmittedTx<TxBody> {
    /// The typed transaction body.
    pub body: TxBody,
    /// Witness set carried alongside the body.
    pub witness_set: ShelleyWitnessSet,
    /// Optional auxiliary data captured as raw CBOR.
    pub auxiliary_data: Option<Vec<u8>>,
    /// Exact CBOR bytes of the submitted transaction when built via the
    /// provided constructors or decoder.
    pub raw_cbor: Vec<u8>,
    /// Original wire CBOR bytes of the transaction body.
    ///
    /// Upstream `Cardano.Ledger.Core.txIdTxBody` hashes the on-wire body
    /// bytes, not a re-serialized representation.  Preserving the original
    /// bytes ensures that TxId computation is correct even for
    /// non-canonically encoded transactions.
    pub raw_body: Vec<u8>,
}

impl<TxBody> ShelleyCompatibleSubmittedTx<TxBody>
where
    TxBody: CborEncode,
{
    /// Build a Shelley-family submitted transaction from typed parts.
    pub fn new(body: TxBody, witness_set: ShelleyWitnessSet, auxiliary_data: Option<Vec<u8>>) -> Self {
        let raw_body = body.to_cbor_bytes();
        let raw_cbor = encode_shelley_family_tx(&body, &witness_set, &auxiliary_data);
        Self {
            body,
            witness_set,
            auxiliary_data,
            raw_cbor,
            raw_body,
        }
    }

    /// Return the canonical transaction identifier derived from the
    /// original on-wire CBOR body bytes.
    ///
    /// Reference: upstream `Cardano.Ledger.Core` — `txIdTxBody` uses
    /// `originalBytes` of the body, not a re-serialized form.
    pub fn tx_id(&self) -> TxId {
        compute_tx_id(&self.raw_body)
    }
}

impl<TxBody> CborEncode for ShelleyCompatibleSubmittedTx<TxBody>
where
    TxBody: CborEncode,
{
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.raw(&encode_shelley_family_tx(
            &self.body,
            &self.witness_set,
            &self.auxiliary_data,
        ));
    }
}

impl<TxBody> CborDecode for ShelleyCompatibleSubmittedTx<TxBody>
where
    TxBody: CborDecode + CborEncode,
{
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let start = dec.position();
        let len = dec.array()?;
        if len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            });
        }

        let body_start = dec.position();
        let body = TxBody::decode_cbor(dec)?;
        let body_end = dec.position();
        let raw_body = dec.slice(body_start, body_end)?.to_vec();

        let witness_set = ShelleyWitnessSet::decode_cbor(dec)?;
        let auxiliary_data = decode_optional_raw_cbor(dec)?;
        let end = dec.position();

        Ok(Self {
            body,
            witness_set,
            auxiliary_data,
            raw_cbor: dec.slice(start, end)?.to_vec(),
            raw_body,
        })
    }
}

/// A submitted transaction using the 4-element Alonzo-family wire shape:
/// body, witness set, `is_valid`, and optional auxiliary data.
///
/// This shape is shared by Alonzo, Babbage, and Conway transactions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlonzoCompatibleSubmittedTx<TxBody> {
    /// The typed transaction body.
    pub body: TxBody,
    /// Witness set carried alongside the body.
    pub witness_set: ShelleyWitnessSet,
    /// Phase-2 validation flag present in Alonzo-family transactions.
    pub is_valid: bool,
    /// Optional auxiliary data captured as raw CBOR.
    pub auxiliary_data: Option<Vec<u8>>,
    /// Exact CBOR bytes of the submitted transaction when built via the
    /// provided constructors or decoder.
    pub raw_cbor: Vec<u8>,
    /// Original wire CBOR bytes of the transaction body.
    ///
    /// Upstream `Cardano.Ledger.Core.txIdTxBody` hashes the on-wire body
    /// bytes, not a re-serialized representation.  Preserving the original
    /// bytes ensures that TxId computation is correct even for
    /// non-canonically encoded transactions.
    pub raw_body: Vec<u8>,
}

impl<TxBody> AlonzoCompatibleSubmittedTx<TxBody>
where
    TxBody: CborEncode,
{
    /// Build an Alonzo-family submitted transaction from typed parts.
    pub fn new(
        body: TxBody,
        witness_set: ShelleyWitnessSet,
        is_valid: bool,
        auxiliary_data: Option<Vec<u8>>,
    ) -> Self {
        let raw_body = body.to_cbor_bytes();
        let raw_cbor = encode_alonzo_family_tx(&body, &witness_set, is_valid, &auxiliary_data);
        Self {
            body,
            witness_set,
            is_valid,
            auxiliary_data,
            raw_cbor,
            raw_body,
        }
    }

    /// Return the canonical transaction identifier derived from the
    /// original on-wire CBOR body bytes.
    ///
    /// Reference: upstream `Cardano.Ledger.Core` — `txIdTxBody` uses
    /// `originalBytes` of the body, not a re-serialized form.
    pub fn tx_id(&self) -> TxId {
        compute_tx_id(&self.raw_body)
    }
}

impl<TxBody> CborEncode for AlonzoCompatibleSubmittedTx<TxBody>
where
    TxBody: CborEncode,
{
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.raw(&encode_alonzo_family_tx(
            &self.body,
            &self.witness_set,
            self.is_valid,
            &self.auxiliary_data,
        ));
    }
}

impl<TxBody> CborDecode for AlonzoCompatibleSubmittedTx<TxBody>
where
    TxBody: CborDecode + CborEncode,
{
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let start = dec.position();
        let len = dec.array()?;
        if len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }

        let body_start = dec.position();
        let body = TxBody::decode_cbor(dec)?;
        let body_end = dec.position();
        let raw_body = dec.slice(body_start, body_end)?.to_vec();

        let witness_set = ShelleyWitnessSet::decode_cbor(dec)?;
        let is_valid = dec.bool()?;
        let auxiliary_data = decode_optional_raw_cbor(dec)?;
        let end = dec.position();

        Ok(Self {
            body,
            witness_set,
            is_valid,
            auxiliary_data,
            raw_cbor: dec.slice(start, end)?.to_vec(),
            raw_body,
        })
    }
}

/// A typed submitted transaction spanning all supported Shelley-based eras.
///
/// Byron transactions are not modeled here because the current submission work
/// targets the Shelley-based node-to-node relay path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultiEraSubmittedTx {
    /// Shelley-era submitted transaction.
    Shelley(ShelleyTx),
    /// Allegra-era submitted transaction.
    Allegra(ShelleyCompatibleSubmittedTx<AllegraTxBody>),
    /// Mary-era submitted transaction.
    Mary(ShelleyCompatibleSubmittedTx<MaryTxBody>),
    /// Alonzo-era submitted transaction.
    Alonzo(AlonzoCompatibleSubmittedTx<AlonzoTxBody>),
    /// Babbage-era submitted transaction.
    Babbage(AlonzoCompatibleSubmittedTx<BabbageTxBody>),
    /// Conway-era submitted transaction.
    Conway(AlonzoCompatibleSubmittedTx<ConwayTxBody>),
}

impl MultiEraSubmittedTx {
    /// Decode a submitted transaction using the transaction shape for the
    /// specified era.
    pub fn from_cbor_bytes_for_era(era: Era, data: &[u8]) -> Result<Self, LedgerError> {
        match era {
            Era::Byron => Err(LedgerError::UnsupportedEra(Era::Byron)),
            Era::Shelley => ShelleyTx::from_cbor_bytes(data).map(Self::Shelley),
            Era::Allegra => {
                ShelleyCompatibleSubmittedTx::<AllegraTxBody>::from_cbor_bytes(data)
                    .map(Self::Allegra)
            }
            Era::Mary => {
                ShelleyCompatibleSubmittedTx::<MaryTxBody>::from_cbor_bytes(data).map(Self::Mary)
            }
            Era::Alonzo => {
                AlonzoCompatibleSubmittedTx::<AlonzoTxBody>::from_cbor_bytes(data)
                    .map(Self::Alonzo)
            }
            Era::Babbage => {
                AlonzoCompatibleSubmittedTx::<BabbageTxBody>::from_cbor_bytes(data)
                    .map(Self::Babbage)
            }
            Era::Conway => {
                AlonzoCompatibleSubmittedTx::<ConwayTxBody>::from_cbor_bytes(data)
                    .map(Self::Conway)
            }
        }
    }

    /// Return the era associated with this submitted transaction.
    pub fn era(&self) -> Era {
        match self {
            Self::Shelley(_) => Era::Shelley,
            Self::Allegra(_) => Era::Allegra,
            Self::Mary(_) => Era::Mary,
            Self::Alonzo(_) => Era::Alonzo,
            Self::Babbage(_) => Era::Babbage,
            Self::Conway(_) => Era::Conway,
        }
    }

    /// Return the canonical transaction identifier derived from the CBOR body.
    pub fn tx_id(&self) -> TxId {
        match self {
            Self::Shelley(tx) => compute_tx_id(&tx.body.to_cbor_bytes()),
            Self::Allegra(tx) => tx.tx_id(),
            Self::Mary(tx) => tx.tx_id(),
            Self::Alonzo(tx) => tx.tx_id(),
            Self::Babbage(tx) => tx.tx_id(),
            Self::Conway(tx) => tx.tx_id(),
        }
    }

    /// Return the transaction fee declared by the submitted transaction body.
    pub fn fee(&self) -> u64 {
        match self {
            Self::Shelley(tx) => tx.body.fee,
            Self::Allegra(tx) => tx.body.fee,
            Self::Mary(tx) => tx.body.fee,
            Self::Alonzo(tx) => tx.body.fee,
            Self::Babbage(tx) => tx.body.fee,
            Self::Conway(tx) => tx.body.fee,
        }
    }

    /// Return the upper validity bound, if the era carries one.
    pub fn expires_at(&self) -> Option<SlotNo> {
        match self {
            Self::Shelley(tx) => Some(SlotNo(tx.body.ttl)),
            Self::Allegra(tx) => tx.body.ttl.map(SlotNo),
            Self::Mary(tx) => tx.body.ttl.map(SlotNo),
            Self::Alonzo(tx) => tx.body.ttl.map(SlotNo),
            Self::Babbage(tx) => tx.body.ttl.map(SlotNo),
            Self::Conway(tx) => tx.body.ttl.map(SlotNo),
        }
    }

    /// Return the UTxO inputs consumed by this transaction.
    ///
    /// This is used by the mempool for double-spend conflict detection:
    /// two transactions conflict if their input sets overlap, meaning both
    /// attempt to spend the same UTxO output.
    ///
    /// Reference: `Cardano.Ledger.Core` — `inputs txb`.
    pub fn inputs(&self) -> Vec<ShelleyTxIn> {
        match self {
            Self::Shelley(tx) => tx.body.inputs.clone(),
            Self::Allegra(tx) => tx.body.inputs.clone(),
            Self::Mary(tx) => tx.body.inputs.clone(),
            Self::Alonzo(tx) => tx.body.inputs.clone(),
            Self::Babbage(tx) => tx.body.inputs.clone(),
            Self::Conway(tx) => tx.body.inputs.clone(),
        }
    }

    /// Return the canonical CBOR bytes of the transaction body.
    pub fn body_cbor(&self) -> Vec<u8> {
        match self {
            Self::Shelley(tx) => tx.body.to_cbor_bytes(),
            Self::Allegra(tx) => tx.body.to_cbor_bytes(),
            Self::Mary(tx) => tx.body.to_cbor_bytes(),
            Self::Alonzo(tx) => tx.body.to_cbor_bytes(),
            Self::Babbage(tx) => tx.body.to_cbor_bytes(),
            Self::Conway(tx) => tx.body.to_cbor_bytes(),
        }
    }

    /// Return the aggregate script execution units declared by redeemers.
    ///
    /// Shelley, Allegra, and Mary do not carry redeemers, so this returns
    /// `None`. Alonzo-family eras return `Some` when at least one redeemer is
    /// present.
    pub fn total_ex_units(&self) -> Option<ExUnits> {
        fn sum_redeemers(witness_set: &ShelleyWitnessSet) -> Option<ExUnits> {
            if witness_set.redeemers.is_empty() {
                return None;
            }

            let mut mem = 0u64;
            let mut steps = 0u64;
            for redeemer in &witness_set.redeemers {
                mem = mem.saturating_add(redeemer.ex_units.mem);
                steps = steps.saturating_add(redeemer.ex_units.steps);
            }

            Some(ExUnits { mem, steps })
        }

        match self {
            Self::Shelley(_) | Self::Allegra(_) | Self::Mary(_) => None,
            Self::Alonzo(tx) => sum_redeemers(&tx.witness_set),
            Self::Babbage(tx) => sum_redeemers(&tx.witness_set),
            Self::Conway(tx) => sum_redeemers(&tx.witness_set),
        }
    }

    /// Return the exact or reconstructed CBOR bytes for this submitted
    /// transaction.
    pub fn raw_cbor(&self) -> Vec<u8> {
        match self {
            Self::Shelley(tx) => tx.to_cbor_bytes(),
            Self::Allegra(tx) => tx.raw_cbor.clone(),
            Self::Mary(tx) => tx.raw_cbor.clone(),
            Self::Alonzo(tx) => tx.raw_cbor.clone(),
            Self::Babbage(tx) => tx.raw_cbor.clone(),
            Self::Conway(tx) => tx.raw_cbor.clone(),
        }
    }
}

fn encode_shelley_family_tx<TxBody: CborEncode>(
    body: &TxBody,
    witness_set: &ShelleyWitnessSet,
    auxiliary_data: &Option<Vec<u8>>,
) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    body.encode_cbor(&mut enc);
    witness_set.encode_cbor(&mut enc);
    encode_optional_raw_cbor(&mut enc, auxiliary_data);
    enc.into_bytes()
}

fn encode_alonzo_family_tx<TxBody: CborEncode>(
    body: &TxBody,
    witness_set: &ShelleyWitnessSet,
    is_valid: bool,
    auxiliary_data: &Option<Vec<u8>>,
) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(4);
    body.encode_cbor(&mut enc);
    witness_set.encode_cbor(&mut enc);
    enc.bool(is_valid);
    encode_optional_raw_cbor(&mut enc, auxiliary_data);
    enc.into_bytes()
}

fn encode_optional_raw_cbor(enc: &mut Encoder, auxiliary_data: &Option<Vec<u8>>) {
    match auxiliary_data {
        Some(raw) => {
            enc.raw(raw);
        }
        None => {
            enc.null();
        }
    }
}

fn decode_optional_raw_cbor(dec: &mut Decoder<'_>) -> Result<Option<Vec<u8>>, LedgerError> {
    if dec.peek_major()? == 7 {
        dec.null()?;
        Ok(None)
    } else {
        let start = dec.position();
        dec.skip()?;
        let end = dec.position();
        Ok(Some(dec.slice(start, end)?.to_vec()))
    }
}

/// A block header containing the essential chain-indexing fields.
///
/// Reference: upstream `HeaderBody` in `cardano-ledger`.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockHeader {
    /// Hash of this header (Blake2b-256).
    pub hash: HeaderHash,
    /// Hash of the previous block header (`[0u8; 32]` for genesis successor).
    pub prev_hash: HeaderHash,
    /// Slot in which this block was issued.
    pub slot_no: SlotNo,
    /// Block height.
    pub block_no: BlockNo,
    /// Issuer verification key (opaque bytes, 32-byte Ed25519 vkey).
    pub issuer_vkey: [u8; 32],
}

/// A block carrying its header and a body of transactions.
///
/// Reference: `Ouroboros.Consensus.Block.Abstract` — `Block`.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Block {
    /// The era this block belongs to.
    pub era: Era,
    /// Block header with chain-indexing fields.
    pub header: BlockHeader,
    /// Transactions included in this block.
    pub transactions: Vec<Tx>,
    /// Original wire-format CBOR bytes received from the network.
    ///
    /// When present, these bytes can be served directly via BlockFetch
    /// without re-encoding. Populated during sync when blocks arrive
    /// from the network; absent for blocks constructed programmatically
    /// or recovered from legacy storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_cbor: Option<Vec<u8>>,
    /// Serialized size of the block header in bytes (CBOR wire format).
    ///
    /// When present, `apply_block()` checks this against the
    /// `max_block_header_size` protocol parameter.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Bbody` — `bHeaderSize`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_cbor_size: Option<usize>,
}

// ─────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::eras::{ExUnits, Redeemer};
    use crate::eras::ShelleyTxBody;
    use crate::plutus::PlutusData;

    // ── compute_tx_id ──────────────────────────────────────────────────

    #[test]
    fn compute_tx_id_deterministic() {
        let body = b"some_body_bytes";
        let id1 = compute_tx_id(body);
        let id2 = compute_tx_id(body);
        assert_eq!(id1, id2);
    }

    #[test]
    fn compute_tx_id_different_inputs_differ() {
        let id1 = compute_tx_id(b"body_a");
        let id2 = compute_tx_id(b"body_b");
        assert_ne!(id1, id2);
    }

    #[test]
    fn compute_tx_id_is_32_bytes() {
        let id = compute_tx_id(b"payload");
        assert_eq!(id.0.len(), 32);
    }

    // ── Tx struct ──────────────────────────────────────────────────────

    #[test]
    fn tx_struct_fields() {
        let body = b"test_body".to_vec();
        let id = compute_tx_id(&body);
        let tx = Tx {
            id,
            body: body.clone(),
            witnesses: None,
            auxiliary_data: None,
            is_valid: None,
        };
        assert_eq!(tx.id, id);
        assert_eq!(tx.body, body);
        assert!(tx.witnesses.is_none());
        assert!(tx.auxiliary_data.is_none());
    }

    #[test]
    fn tx_with_witnesses_and_aux_data() {
        let body = b"test_body".to_vec();
        let tx = Tx {
            id: compute_tx_id(&body),
            body,
            witnesses: Some(vec![0xa0]),
            auxiliary_data: Some(vec![0xa1, 0x01, 0x02]),
            is_valid: None,
        };
        assert!(tx.witnesses.is_some());
        assert!(tx.auxiliary_data.is_some());
    }

    #[test]
    fn serialized_size_pre_alonzo() {
        // Pre-Alonzo: [body, witnesses, aux_data/null] — 3-element array
        let body = vec![0xa2, 0x00, 0x01, 0x01, 0x02]; // 5 bytes body
        let witnesses = vec![0xa0]; // 1 byte empty map
        let tx = Tx {
            id: compute_tx_id(&body),
            body: body.clone(),
            witnesses: Some(witnesses.clone()),
            auxiliary_data: None,
            is_valid: None, // pre-Alonzo: no is_valid
        };
        // 1 (array header) + 5 (body) + 1 (witnesses) + 1 (null for aux_data) = 8
        assert_eq!(tx.serialized_size(), 8);
    }

    #[test]
    fn serialized_size_alonzo_plus() {
        // Alonzo+: [body, witnesses, is_valid, aux_data/null] — 4-element array
        let body = vec![0xa2, 0x00, 0x01, 0x01, 0x02]; // 5 bytes
        let witnesses = vec![0xa0]; // 1 byte
        let aux_data = vec![0xa1, 0x00, 0x01]; // 3 bytes
        let tx = Tx {
            id: compute_tx_id(&body),
            body: body.clone(),
            witnesses: Some(witnesses.clone()),
            auxiliary_data: Some(aux_data.clone()),
            is_valid: Some(true), // Alonzo+ has is_valid
        };
        // 1 (header) + 5 (body) + 1 (witnesses) + 1 (is_valid bool) + 3 (aux_data) = 11
        assert_eq!(tx.serialized_size(), 11);
    }

    #[test]
    fn serialized_size_larger_than_body_only() {
        let body = vec![0xa2, 0x00, 0x01]; // 3 bytes body
        let witnesses = vec![0xa2, 0x00, 0x81, 0x01, 0x01, 0x02]; // 6 bytes
        let tx = Tx {
            id: compute_tx_id(&body),
            body: body.clone(),
            witnesses: Some(witnesses.clone()),
            auxiliary_data: None,
            is_valid: Some(true),
        };
        // Full size should be strictly larger than body-only
        assert!(tx.serialized_size() > body.len());
        // 1 (header) + 3 (body) + 6 (witnesses) + 1 (is_valid) + 1 (null) = 12
        assert_eq!(tx.serialized_size(), 12);
    }

    // ── ShelleyCompatibleSubmittedTx ───────────────────────────────────

    #[test]
    fn shelley_submitted_tx_round_trip() {
        let body = ShelleyTxBody {
            inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
            outputs: vec![crate::eras::shelley::ShelleyTxOut {
                address: vec![0x61; 29],
                amount: 2_000_000,
            }],
            fee: 200_000,
            ttl: 100,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let stx = ShelleyCompatibleSubmittedTx::new(body.clone(), ws.clone(), None);
        let tx_id = stx.tx_id();
        assert_eq!(tx_id, compute_tx_id(&body.to_cbor_bytes()));

        // CBOR round-trip
        let encoded = stx.to_cbor_bytes();
        let decoded =
            ShelleyCompatibleSubmittedTx::<ShelleyTxBody>::from_cbor_bytes(&encoded).unwrap();
        assert_eq!(decoded.body, body);
        assert_eq!(decoded.auxiliary_data, None);
    }

    // ── AlonzoCompatibleSubmittedTx ────────────────────────────────────

    #[test]
    fn alonzo_submitted_tx_round_trip() {
        let body = AlonzoTxBody {
            inputs: vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }],
            outputs: vec![crate::eras::alonzo::AlonzoTxOut {
                address: vec![0x61; 29],
                amount: crate::eras::mary::Value::Coin(1_000_000),
                datum_hash: None,
            }],
            fee: 300_000,
            ttl: None,
            validity_interval_start: None,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            mint: None,
            script_data_hash: None,
            collateral: None,
            required_signers: None,
            network_id: None,
        };
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let stx = AlonzoCompatibleSubmittedTx::new(body.clone(), ws, true, None);
        assert!(stx.is_valid);
        let tx_id = stx.tx_id();
        assert_eq!(tx_id, compute_tx_id(&body.to_cbor_bytes()));

        let encoded = stx.to_cbor_bytes();
        let decoded =
            AlonzoCompatibleSubmittedTx::<AlonzoTxBody>::from_cbor_bytes(&encoded).unwrap();
        assert_eq!(decoded.body, body);
        assert!(decoded.is_valid);
    }

    #[test]
    fn alonzo_submitted_tx_invalid_array_length_rejected() {
        // Construct a 3-element array (wrong for Alonzo which requires 4)
        let mut enc = crate::cbor::Encoder::new();
        enc.array(3).unsigned(0).unsigned(0).null();
        let bytes = enc.into_bytes();
        let result = AlonzoCompatibleSubmittedTx::<AlonzoTxBody>::from_cbor_bytes(&bytes);
        assert!(result.is_err());
    }

    // ── MultiEraSubmittedTx ────────────────────────────────────────────

    #[test]
    fn multi_era_submitted_tx_byron_unsupported() {
        let result = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Byron, &[0x00]);
        assert!(matches!(result, Err(LedgerError::UnsupportedEra(Era::Byron))));
    }

    #[test]
    fn multi_era_submitted_tx_era_accessor() {
        // Build a minimal Shelley submitted tx
        let body = ShelleyTxBody {
            inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
            outputs: vec![crate::eras::shelley::ShelleyTxOut {
                address: vec![0x61; 29],
                amount: 1_000_000,
            }],
            fee: 100,
            ttl: 100,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let stx = ShelleyCompatibleSubmittedTx::new(body, ws, None);
        let cbor = stx.to_cbor_bytes();
        let mstx = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Shelley, &cbor).unwrap();
        assert_eq!(mstx.era(), Era::Shelley);
    }

    #[test]
    fn multi_era_submitted_tx_fee_and_inputs() {
        let body = ShelleyTxBody {
            inputs: vec![
                ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
                ShelleyTxIn { transaction_id: [0x02; 32], index: 1 },
            ],
            outputs: vec![crate::eras::shelley::ShelleyTxOut {
                address: vec![0x61; 29],
                amount: 1_000_000,
            }],
            fee: 175_000,
            ttl: 200,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        let ws = ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        };
        let stx = ShelleyCompatibleSubmittedTx::new(body, ws, None);
        let cbor = stx.to_cbor_bytes();
        let mstx = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Shelley, &cbor).unwrap();
        assert_eq!(mstx.fee(), 175_000);
        assert_eq!(mstx.inputs().len(), 2);
        assert_eq!(mstx.expires_at(), Some(SlotNo(200)));
    }

    #[test]
    fn multi_era_submitted_tx_total_ex_units_none_without_redeemers() {
        let tx = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
            AlonzoTxBody {
                inputs: vec![],
                outputs: vec![],
                fee: 1,
                ttl: None,
                validity_interval_start: None,
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash: None,
                mint: None,
                script_data_hash: None,
                collateral: None,
                required_signers: None,
                network_id: None,
            },
            ShelleyWitnessSet {
                vkey_witnesses: vec![],
                native_scripts: vec![],
                bootstrap_witnesses: vec![],
                plutus_v1_scripts: vec![],
                plutus_data: vec![],
                redeemers: vec![],
                plutus_v2_scripts: vec![],
                plutus_v3_scripts: vec![],
            },
            true,
            None,
        ));

        assert_eq!(tx.total_ex_units(), None);
    }

    #[test]
    fn multi_era_submitted_tx_total_ex_units_sums_redeemers() {
        let tx = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
            AlonzoTxBody {
                inputs: vec![],
                outputs: vec![],
                fee: 1,
                ttl: None,
                validity_interval_start: None,
                certificates: None,
                withdrawals: None,
                update: None,
                auxiliary_data_hash: None,
                mint: None,
                script_data_hash: None,
                collateral: None,
                required_signers: None,
                network_id: None,
            },
            ShelleyWitnessSet {
                vkey_witnesses: vec![],
                native_scripts: vec![],
                bootstrap_witnesses: vec![],
                plutus_v1_scripts: vec![],
                plutus_data: vec![],
                redeemers: vec![
                    Redeemer {
                        tag: 0,
                        index: 0,
                        data: PlutusData::Integer(0),
                        ex_units: ExUnits {
                            mem: 10,
                            steps: 20,
                        },
                    },
                    Redeemer {
                        tag: 1,
                        index: 0,
                        data: PlutusData::Integer(1),
                        ex_units: ExUnits {
                            mem: 30,
                            steps: 40,
                        },
                    },
                ],
                plutus_v2_scripts: vec![],
                plutus_v3_scripts: vec![],
            },
            true,
            None,
        ));

        assert_eq!(
            tx.total_ex_units(),
            Some(ExUnits {
                mem: 40,
                steps: 60,
            })
        );
    }

    // ── BlockHeader / Block ────────────────────────────────────────────

    #[test]
    fn block_header_fields() {
        let header = BlockHeader {
            hash: HeaderHash([0x01; 32]),
            prev_hash: HeaderHash([0x00; 32]),
            slot_no: SlotNo(42),
            block_no: BlockNo(1),
            issuer_vkey: [0xab; 32],
        };
        assert_eq!(header.slot_no, SlotNo(42));
        assert_eq!(header.block_no, BlockNo(1));
    }

    #[test]
    fn block_struct() {
        let block = Block {
            era: Era::Shelley,
            header: BlockHeader {
                hash: HeaderHash([0x01; 32]),
                prev_hash: HeaderHash([0x00; 32]),
                slot_no: SlotNo(1),
                block_no: BlockNo(1),
                issuer_vkey: [0x00; 32],
            },
            transactions: vec![],
            raw_cbor: None,
            header_cbor_size: None,
        };
        assert_eq!(block.era, Era::Shelley);
        assert!(block.transactions.is_empty());
        assert!(block.raw_cbor.is_none());
    }

    // ── encode/decode helpers ──────────────────────────────────────────

    #[test]
    fn encode_optional_raw_cbor_none() {
        let mut enc = crate::cbor::Encoder::new();
        encode_optional_raw_cbor(&mut enc, &None);
        let bytes = enc.into_bytes();
        // Should be CBOR null
        assert_eq!(bytes, [0xf6]);
    }

    #[test]
    fn encode_optional_raw_cbor_some() {
        let raw = vec![0x01, 0x02];
        let mut enc = crate::cbor::Encoder::new();
        encode_optional_raw_cbor(&mut enc, &Some(raw.clone()));
        let bytes = enc.into_bytes();
        assert_eq!(bytes, raw);
    }

    #[test]
    fn decode_optional_raw_cbor_null() {
        let bytes = [0xf6]; // CBOR null
        let mut dec = crate::cbor::Decoder::new(&bytes);
        assert_eq!(decode_optional_raw_cbor(&mut dec).unwrap(), None);
    }

    #[test]
    fn decode_optional_raw_cbor_present() {
        let mut enc = crate::cbor::Encoder::new();
        enc.unsigned(42);
        let bytes = enc.into_bytes();
        let mut dec = crate::cbor::Decoder::new(&bytes);
        let result = decode_optional_raw_cbor(&mut dec).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), bytes);
    }
}
