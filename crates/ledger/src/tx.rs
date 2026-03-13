use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::eras::Era;
use crate::eras::{
    AllegraTxBody, AlonzoTxBody, BabbageTxBody, ConwayTxBody, MaryTxBody, ShelleyTx,
    ShelleyWitnessSet,
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
}

impl<TxBody> ShelleyCompatibleSubmittedTx<TxBody>
where
    TxBody: CborEncode,
{
    /// Build a Shelley-family submitted transaction from typed parts.
    pub fn new(body: TxBody, witness_set: ShelleyWitnessSet, auxiliary_data: Option<Vec<u8>>) -> Self {
        let raw_cbor = encode_shelley_family_tx(&body, &witness_set, &auxiliary_data);
        Self {
            body,
            witness_set,
            auxiliary_data,
            raw_cbor,
        }
    }

    /// Return the canonical transaction identifier derived from the CBOR body.
    pub fn tx_id(&self) -> TxId {
        compute_tx_id(&self.body.to_cbor_bytes())
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

        let body = TxBody::decode_cbor(dec)?;
        let witness_set = ShelleyWitnessSet::decode_cbor(dec)?;
        let auxiliary_data = decode_optional_raw_cbor(dec)?;
        let end = dec.position();

        Ok(Self {
            body,
            witness_set,
            auxiliary_data,
            raw_cbor: dec.slice(start, end)?.to_vec(),
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
        let raw_cbor = encode_alonzo_family_tx(&body, &witness_set, is_valid, &auxiliary_data);
        Self {
            body,
            witness_set,
            is_valid,
            auxiliary_data,
            raw_cbor,
        }
    }

    /// Return the canonical transaction identifier derived from the CBOR body.
    pub fn tx_id(&self) -> TxId {
        compute_tx_id(&self.body.to_cbor_bytes())
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

        let body = TxBody::decode_cbor(dec)?;
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
}
