//! Byron-era block envelope types.
//!
//! Byron blocks use a fundamentally different wire format from Shelley+
//! eras. They come in two variants:
//!
//! - **Epoch Boundary Block (EBB)** — era tag 0 in the outer envelope.
//!   Header consensus data: `[epoch, chain_difficulty]`.
//! - **Main Block** — era tag 1 in the outer envelope.
//!   Header consensus data: `[[epoch, slot_in_epoch], pubkey,
//!   difficulty, signature]`.
//!
//! This module extracts the epoch, slot, `chain_difficulty` (block
//! number), previous hash, and raw header byte ranges needed for
//! header hash computation.
//!
//! ## Header hash computation
//!
//! Byron header hashes are **not** computed over the full wire block.
//! Instead they are `Blake2b-256(prefix ++ raw_header_cbor_bytes)` where
//! the prefix encodes the variant discriminator:
//!
//! - EBB:  `0x82 0x00` (`[2-array, 0]`)
//! - Main: `0x82 0x01` (`[2-array, 1]`)
//!
//! Reference:
//! <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/byron>

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::error::LedgerError;
use crate::types::HeaderHash;

pub const BYRON_NAME: &str = "Byron";

/// Number of slots per Byron epoch on mainnet.
pub const BYRON_SLOTS_PER_EPOCH: u64 = 21600;

/// Prefix prepended to EBB header bytes before hashing.
///
/// `0x82 0x00` = CBOR 2-element array, tag 0.
const EBB_HASH_PREFIX: [u8; 2] = [0x82, 0x00];

/// Prefix prepended to main-block header bytes before hashing.
///
/// `0x82 0x01` = CBOR 2-element array, tag 1.
const MAIN_HASH_PREFIX: [u8; 2] = [0x82, 0x01];

// ---------------------------------------------------------------------------
// Byron transaction types
// ---------------------------------------------------------------------------

/// A Byron-era transaction input referencing a previous output.
///
/// Byron TxIn wire format: `[u8_type, #6.24(bytes .cbor [txid, u32])]`
/// where `u8_type = 0` for a regular input.
///
/// Reference: `Cardano.Chain.UTxO.TxIn` from `cardano-ledger-byron`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ByronTxIn {
    /// Blake2b-256 hash of the referenced transaction.
    pub txid: [u8; 32],
    /// Output index within that transaction.
    pub index: u32,
}

impl CborDecode for ByronTxIn {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let type_tag = dec.unsigned()?;
        if type_tag != 0 {
            return Err(LedgerError::CborTypeMismatch {
                expected: 0,
                actual: type_tag as u8,
            });
        }

        // The inner payload is CBOR tag 24 wrapping a byte string that itself
        // contains CBOR-encoded [txid, index].
        let tag = dec.tag()?;
        if tag != 24 {
            return Err(LedgerError::CborTypeMismatch {
                expected: 24,
                actual: tag as u8,
            });
        }
        let inner_bytes = dec.bytes()?;
        let mut inner_dec = Decoder::new(inner_bytes);
        let inner_len = inner_dec.array()?;
        if inner_len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: inner_len as usize,
            });
        }
        let txid_raw = inner_dec.bytes()?;
        let txid: [u8; 32] =
            txid_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: txid_raw.len(),
                })?;
        let index = inner_dec.unsigned()? as u32;
        Ok(Self { txid, index })
    }
}

impl CborEncode for ByronTxIn {
    fn encode_cbor(&self, enc: &mut Encoder) {
        // Encode inner payload: [txid, index]
        let mut inner = Encoder::new();
        inner.array(2).bytes(&self.txid).unsigned(u64::from(self.index));
        let inner_bytes = inner.into_bytes();
        // Outer: [0, #6.24(inner_bytes)]
        enc.array(2).unsigned(0).tag(24).bytes(&inner_bytes);
    }
}

/// A Byron-era transaction output: an address receiving a lovelace amount.
///
/// Byron TxOut wire format: `[address_raw, coin]`.
/// The address is opaque CBOR bytes (Byron addresses use CBOR-in-CBOR
/// with CRC32 checksums internally).
///
/// Reference: `Cardano.Chain.UTxO.TxOut` from `cardano-ledger-byron`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByronTxOut {
    /// Raw address bytes (Byron CBOR-in-CBOR address with CRC32 checksum).
    pub address: Vec<u8>,
    /// Amount in lovelace.
    pub amount: u64,
}

impl CborDecode for ByronTxOut {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        // Address: capture the raw CBOR bytes.
        let addr_start = dec.position();
        dec.skip()?;
        let addr_end = dec.position();
        let address = dec.slice(addr_start, addr_end)?.to_vec();
        let amount = dec.unsigned()?;
        Ok(Self { address, amount })
    }
}

impl CborEncode for ByronTxOut {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2).raw(&self.address).unsigned(self.amount);
    }
}

/// A Byron-era transaction.
///
/// Byron Tx wire format: `[inputs : [+ TxIn], outputs : [+ TxOut], attributes : {}]`.
/// The `attributes` map is typically empty and is captured as raw CBOR.
///
/// Reference: `Cardano.Chain.UTxO.Tx` from `cardano-ledger-byron`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByronTx {
    /// Transaction inputs.
    pub inputs: Vec<ByronTxIn>,
    /// Transaction outputs.
    pub outputs: Vec<ByronTxOut>,
    /// Attributes map captured as raw CBOR bytes (usually empty `{}`).
    pub attributes: Vec<u8>,
}

impl CborDecode for ByronTx {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            });
        }

        // inputs: [+ TxIn]
        let n_inputs = dec.array()?;
        let mut inputs = Vec::with_capacity(n_inputs as usize);
        for _ in 0..n_inputs {
            inputs.push(ByronTxIn::decode_cbor(dec)?);
        }

        // outputs: [+ TxOut]
        let n_outputs = dec.array()?;
        let mut outputs = Vec::with_capacity(n_outputs as usize);
        for _ in 0..n_outputs {
            outputs.push(ByronTxOut::decode_cbor(dec)?);
        }

        // attributes: map (captured as raw CBOR)
        let attr_start = dec.position();
        dec.skip()?;
        let attr_end = dec.position();
        let attributes = dec.slice(attr_start, attr_end)?.to_vec();

        Ok(Self {
            inputs,
            outputs,
            attributes,
        })
    }
}

impl CborEncode for ByronTx {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(3);

        enc.array(self.inputs.len() as u64);
        for input in &self.inputs {
            input.encode_cbor(enc);
        }

        enc.array(self.outputs.len() as u64);
        for output in &self.outputs {
            output.encode_cbor(enc);
        }

        enc.raw(&self.attributes);
    }
}

impl ByronTx {
    /// Compute the transaction identifier (Blake2b-256 of the CBOR-encoded Tx).
    ///
    /// Reference: `Cardano.Chain.UTxO.TxId` — `txid = hash(encode(tx))`.
    pub fn tx_id(&self) -> [u8; 32] {
        yggdrasil_crypto::hash_bytes_256(&self.to_cbor_bytes()).0
    }
}

/// A Byron-era transaction witness (signature).
///
/// Wire format: `[u8_type, #6.24(bytes .cbor witness_data)]`.
/// Type 0 = PkWitness `[public_key, signature]`.
/// Type 2 = RedeemWitness `[redeem_public_key, redeem_signature]`.
///
/// Reference: `Cardano.Chain.UTxO.TxWitness` from `cardano-ledger-byron`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByronTxWitness {
    /// Witness type tag (0 = PkWitness, 2 = RedeemWitness).
    pub witness_type: u8,
    /// The CBOR-in-CBOR inner payload (after unwrapping tag 24).
    pub payload: Vec<u8>,
}

impl CborDecode for ByronTxWitness {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let witness_type = dec.unsigned()? as u8;
        let tag = dec.tag()?;
        if tag != 24 {
            return Err(LedgerError::CborTypeMismatch {
                expected: 24,
                actual: tag as u8,
            });
        }
        let payload = dec.bytes()?.to_vec();
        Ok(Self {
            witness_type,
            payload,
        })
    }
}

impl CborEncode for ByronTxWitness {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2)
            .unsigned(u64::from(self.witness_type))
            .tag(24)
            .bytes(&self.payload);
    }
}

/// A Byron transaction with its witnesses (TxAux).
///
/// Wire format: `[Tx, [witnesses...]]`.
///
/// Reference: `Cardano.Chain.UTxO.TxAux` from `cardano-ledger-byron`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByronTxAux {
    /// The transaction body.
    pub tx: ByronTx,
    /// Witness signatures.
    pub witnesses: Vec<ByronTxWitness>,
}

impl CborDecode for ByronTxAux {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: len as usize,
            });
        }
        let tx = ByronTx::decode_cbor(dec)?;
        let n_witnesses = dec.array()?;
        let mut witnesses = Vec::with_capacity(n_witnesses as usize);
        for _ in 0..n_witnesses {
            witnesses.push(ByronTxWitness::decode_cbor(dec)?);
        }
        Ok(Self { tx, witnesses })
    }
}

impl CborEncode for ByronTxAux {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(2);
        self.tx.encode_cbor(enc);
        enc.array(self.witnesses.len() as u64);
        for w in &self.witnesses {
            w.encode_cbor(enc);
        }
    }
}

// ---------------------------------------------------------------------------
// Byron block variant
// ---------------------------------------------------------------------------

/// A decoded Byron block envelope carrying header-level metadata,
/// decoded transactions, and the raw header annotation bytes needed
/// for correct header hash computation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ByronBlock {
    /// An Epoch Boundary Block (EBB), marking an epoch transition.
    EpochBoundary {
        /// The epoch number.
        epoch: u64,
        /// `ChainDifficulty` value (serves as block number).
        chain_difficulty: u64,
        /// Previous block hash (32 bytes).
        prev_hash: [u8; 32],
        /// Raw CBOR bytes of the header element (for hash computation).
        raw_header: Vec<u8>,
    },
    /// A regular Byron main block.
    MainBlock {
        /// The epoch number.
        epoch: u64,
        /// The slot index within the epoch (0..21599 on mainnet).
        slot_in_epoch: u64,
        /// `ChainDifficulty` value (serves as block number).
        chain_difficulty: u64,
        /// Previous block hash (32 bytes).
        prev_hash: [u8; 32],
        /// PBFT issuer verification key (first 32 bytes of the 64-byte
        /// extended Ed25519 public key from consensus data).
        issuer_vkey: [u8; 32],
        /// Raw CBOR bytes of the header element (for hash computation).
        raw_header: Vec<u8>,
        /// Decoded transactions (TxAux entries from the block body).
        transactions: Vec<ByronTxAux>,
    },
}

impl ByronBlock {
    /// Returns the epoch number.
    pub fn epoch(&self) -> u64 {
        match self {
            Self::EpochBoundary { epoch, .. } => *epoch,
            Self::MainBlock { epoch, .. } => *epoch,
        }
    }

    /// Returns the `ChainDifficulty` value, which serves as the block
    /// number in Byron.
    ///
    /// EBBs share the block number of their predecessor (they do **not**
    /// increment the difficulty counter).
    pub fn chain_difficulty(&self) -> u64 {
        match self {
            Self::EpochBoundary {
                chain_difficulty, ..
            } => *chain_difficulty,
            Self::MainBlock {
                chain_difficulty, ..
            } => *chain_difficulty,
        }
    }

    /// Returns `true` for Epoch Boundary Blocks.
    pub fn is_ebb(&self) -> bool {
        matches!(self, Self::EpochBoundary { .. })
    }

    /// Computes the absolute slot number.
    ///
    /// EBBs are treated as occupying the first slot of their epoch.
    /// Main blocks use `epoch * slots_per_epoch + slot_in_epoch`.
    pub fn absolute_slot(&self, slots_per_epoch: u64) -> u64 {
        match self {
            Self::EpochBoundary { epoch, .. } => epoch * slots_per_epoch,
            Self::MainBlock {
                epoch,
                slot_in_epoch,
                ..
            } => epoch * slots_per_epoch + slot_in_epoch,
        }
    }

    /// Returns the previous block hash.
    pub fn prev_hash(&self) -> &[u8; 32] {
        match self {
            Self::EpochBoundary { prev_hash, .. } => prev_hash,
            Self::MainBlock { prev_hash, .. } => prev_hash,
        }
    }

    /// Returns the PBFT issuer verification key (32 bytes).
    ///
    /// For MainBlocks this is the first 32 bytes of the extended Ed25519
    /// public key from the consensus data.  EBBs have no issuer and
    /// return all zeros.
    pub fn issuer_vkey(&self) -> [u8; 32] {
        match self {
            Self::EpochBoundary { .. } => [0u8; 32],
            Self::MainBlock { issuer_vkey, .. } => *issuer_vkey,
        }
    }

    /// Returns the decoded transactions.
    ///
    /// EBBs carry no transactions; main blocks carry the decoded
    /// `TxAux` entries from the block body's tx_payload.
    pub fn transactions(&self) -> &[ByronTxAux] {
        match self {
            Self::EpochBoundary { .. } => &[],
            Self::MainBlock { transactions, .. } => transactions,
        }
    }

    /// Computes the Byron header hash.
    ///
    /// The hash is `Blake2b-256(prefix ++ raw_header_cbor)` where:
    /// - EBB prefix  = `0x82 0x00`
    /// - Main prefix = `0x82 0x01`
    ///
    /// This matches the upstream `headerHashAnnotated` /
    /// `boundaryHeaderHashAnnotated` from `cardano-ledger-byron`.
    pub fn header_hash(&self) -> HeaderHash {
        let (prefix, raw_header) = match self {
            Self::EpochBoundary { raw_header, .. } => (&EBB_HASH_PREFIX[..], raw_header),
            Self::MainBlock { raw_header, .. } => (&MAIN_HASH_PREFIX[..], raw_header),
        };
        let mut buf = Vec::with_capacity(prefix.len() + raw_header.len());
        buf.extend_from_slice(prefix);
        buf.extend_from_slice(raw_header);
        let digest = yggdrasil_crypto::hash_bytes_256(&buf);
        HeaderHash(digest.0)
    }

    /// Decode a Byron EBB from raw CBOR bytes (the block body inside
    /// the `[0, body]` outer envelope).
    ///
    /// EBB structure: `[header, body, extra]`
    /// Header: `[protocol_magic, prev_hash, body_proof,
    ///   consensus_data, extra_data]`
    /// Consensus data: `[epoch, chain_difficulty]`
    ///
    /// `chain_difficulty` is CBOR-encoded as `[Word64]` (a 1-element
    /// array wrapping the difficulty value).
    pub fn decode_ebb(raw: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(raw);

        // Outer: [header, body, extra]
        let outer_len = dec.array()?;
        if outer_len < 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: outer_len as usize,
            });
        }

        // Capture the header byte range for hash computation.
        let hdr_start = dec.position();

        // Header: [protocol_magic, prev_hash, body_proof,
        //          consensus_data, extra_data]
        let hdr_len = dec.array()?;
        if hdr_len < 5 {
            return Err(LedgerError::CborInvalidLength {
                expected: 5,
                actual: hdr_len as usize,
            });
        }

        // protocol_magic
        dec.skip()?;

        // prev_hash
        let prev_raw = dec.bytes()?;
        let prev_hash: [u8; 32] =
            prev_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: prev_raw.len(),
                })?;

        // body_proof
        dec.skip()?;

        // consensus_data: [epoch, chain_difficulty]
        let cd_len = dec.array()?;
        if cd_len < 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: cd_len as usize,
            });
        }
        let epoch = dec.unsigned()?;

        // chain_difficulty: CBOR [Word64] — 1-element array.
        let diff_len = dec.array()?;
        if diff_len < 1 {
            return Err(LedgerError::CborInvalidLength {
                expected: 1,
                actual: diff_len as usize,
            });
        }
        let chain_difficulty = dec.unsigned()?;

        // Skip remaining header fields (extra_data).
        dec.skip()?;

        let hdr_end = dec.position();
        let raw_header = raw[hdr_start..hdr_end].to_vec();

        Ok(Self::EpochBoundary {
            epoch,
            chain_difficulty,
            prev_hash,
            raw_header,
        })
    }

    /// Decode a Byron main block from raw CBOR bytes (the block body
    /// inside the `[1, body]` outer envelope).
    ///
    /// Main block structure: `[header, body, extra]`
    /// Header: `[protocol_magic, prev_hash, body_proof,
    ///   consensus_data, extra_data]`
    /// Consensus data: `[slot_id, pubkey, difficulty, signature]`
    /// Slot id: `[epoch, slot_in_epoch]`
    ///
    /// `difficulty` is CBOR-encoded as `[Word64]` (a 1-element array
    /// wrapping the difficulty value).
    pub fn decode_main(raw: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(raw);

        // Outer: [header, body, extra]
        let outer_len = dec.array()?;
        if outer_len < 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: outer_len as usize,
            });
        }

        // Capture the header byte range for hash computation.
        let hdr_start = dec.position();

        // Header: [protocol_magic, prev_hash, body_proof,
        //          consensus_data, extra_data]
        let hdr_len = dec.array()?;
        if hdr_len < 5 {
            return Err(LedgerError::CborInvalidLength {
                expected: 5,
                actual: hdr_len as usize,
            });
        }

        // protocol_magic
        dec.skip()?;

        // prev_hash
        let prev_raw = dec.bytes()?;
        let prev_hash: [u8; 32] =
            prev_raw
                .try_into()
                .map_err(|_| LedgerError::CborInvalidLength {
                    expected: 32,
                    actual: prev_raw.len(),
                })?;

        // body_proof
        dec.skip()?;

        // consensus_data: [slot_id, pubkey, difficulty, signature]
        let cd_len = dec.array()?;
        if cd_len < 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: cd_len as usize,
            });
        }

        // slot_id: [epoch, slot_in_epoch]
        let slot_len = dec.array()?;
        if slot_len != 2 {
            return Err(LedgerError::CborInvalidLength {
                expected: 2,
                actual: slot_len as usize,
            });
        }
        let epoch = dec.unsigned()?;
        let slot_in_epoch = dec.unsigned()?;

        // pubkey — 64-byte extended Ed25519 public key
        let pubkey_raw = dec.bytes()?;
        let issuer_vkey: [u8; 32] = if pubkey_raw.len() >= 32 {
            pubkey_raw[..32].try_into().map_err(|_| LedgerError::CborInvalidLength {
                expected: 32,
                actual: pubkey_raw.len(),
            })?
        } else {
            return Err(LedgerError::CborInvalidLength {
                expected: 32,
                actual: pubkey_raw.len(),
            });
        };

        // difficulty: [Word64] — 1-element array.
        let diff_len = dec.array()?;
        if diff_len < 1 {
            return Err(LedgerError::CborInvalidLength {
                expected: 1,
                actual: diff_len as usize,
            });
        }
        let chain_difficulty = dec.unsigned()?;

        // Skip remaining consensus fields (signature) + extra_data.
        dec.skip()?;
        dec.skip()?;

        let hdr_end = dec.position();
        let raw_header = raw[hdr_start..hdr_end].to_vec();

        // ---------------------------------------------------------------
        // Block body: [tx_payload, ssc_payload, dlg_payload, upd_payload]
        // tx_payload: [[TxAux, ...]]
        // ---------------------------------------------------------------
        let body_len = dec.array()?;
        if body_len < 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: body_len as usize,
            });
        }

        // tx_payload is a 1-element array wrapping the list of TxAux.
        let tx_payload_len = dec.array()?;
        let transactions = if tx_payload_len == 0 {
            Vec::new()
        } else {
            let n_txs = dec.array()?;
            let mut txs = Vec::with_capacity(n_txs as usize);
            for _ in 0..n_txs {
                txs.push(ByronTxAux::decode_cbor(&mut dec)?);
            }
            // Skip any remaining elements of the tx_payload envelope.
            for _ in 1..tx_payload_len {
                dec.skip()?;
            }
            txs
        };

        Ok(Self::MainBlock {
            epoch,
            slot_in_epoch,
            chain_difficulty,
            prev_hash,
            issuer_vkey,
            raw_header,
            transactions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ByronTxIn ──────────────────────────────────────────────────────

    #[test]
    fn txin_round_trip() {
        let txin = ByronTxIn { txid: [0xAA; 32], index: 0 };
        let decoded = ByronTxIn::from_cbor_bytes(&txin.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, txin);
    }

    #[test]
    fn txin_different_index_round_trip() {
        let txin = ByronTxIn { txid: [0x11; 32], index: 42 };
        let decoded = ByronTxIn::from_cbor_bytes(&txin.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, txin);
    }

    #[test]
    fn txin_different_indices_differ() {
        let a = ByronTxIn { txid: [0x00; 32], index: 0 };
        let b = ByronTxIn { txid: [0x00; 32], index: 1 };
        assert_ne!(a.to_cbor_bytes(), b.to_cbor_bytes());
    }

    // ── ByronTxOut ─────────────────────────────────────────────────────

    #[test]
    fn txout_round_trip() {
        // Build a minimal valid CBOR address (just raw CBOR bytes)
        let mut addr_enc = Encoder::new();
        addr_enc.bytes(&[0x01; 28]);
        let addr = addr_enc.into_bytes();

        let txout = ByronTxOut { address: addr, amount: 5_000_000 };
        let decoded = ByronTxOut::from_cbor_bytes(&txout.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, txout);
    }

    #[test]
    fn txout_different_amounts_differ() {
        let mut addr_enc = Encoder::new();
        addr_enc.bytes(&[0x01; 28]);
        let addr = addr_enc.into_bytes();

        let a = ByronTxOut { address: addr.clone(), amount: 100 };
        let b = ByronTxOut { address: addr, amount: 200 };
        assert_ne!(a.to_cbor_bytes(), b.to_cbor_bytes());
    }

    // ── ByronTxWitness ─────────────────────────────────────────────────

    #[test]
    fn tx_witness_pk_round_trip() {
        let w = ByronTxWitness {
            witness_type: 0,
            payload: vec![0xCC; 64],
        };
        let decoded = ByronTxWitness::from_cbor_bytes(&w.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, w);
    }

    #[test]
    fn tx_witness_redeem_round_trip() {
        let w = ByronTxWitness {
            witness_type: 2,
            payload: vec![0xDD; 96],
        };
        let decoded = ByronTxWitness::from_cbor_bytes(&w.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, w);
    }

    // ── ByronTx ────────────────────────────────────────────────────────

    fn mk_addr() -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.bytes(&[0x01; 28]);
        enc.into_bytes()
    }

    fn mk_attrs() -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.map(0);
        enc.into_bytes()
    }

    #[test]
    fn tx_round_trip() {
        let tx = ByronTx {
            inputs: vec![ByronTxIn { txid: [0xAA; 32], index: 0 }],
            outputs: vec![ByronTxOut { address: mk_addr(), amount: 1_000_000 }],
            attributes: mk_attrs(),
        };
        let decoded = ByronTx::from_cbor_bytes(&tx.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, tx);
    }

    #[test]
    fn tx_id_is_32_bytes() {
        let tx = ByronTx {
            inputs: vec![ByronTxIn { txid: [0xBB; 32], index: 1 }],
            outputs: vec![ByronTxOut { address: mk_addr(), amount: 500 }],
            attributes: mk_attrs(),
        };
        assert_eq!(tx.tx_id().len(), 32);
    }

    #[test]
    fn tx_id_deterministic() {
        let tx = ByronTx {
            inputs: vec![ByronTxIn { txid: [0xCC; 32], index: 0 }],
            outputs: vec![ByronTxOut { address: mk_addr(), amount: 100 }],
            attributes: mk_attrs(),
        };
        assert_eq!(tx.tx_id(), tx.tx_id());
    }

    #[test]
    fn tx_id_different_inputs_differ() {
        let tx1 = ByronTx {
            inputs: vec![ByronTxIn { txid: [0x01; 32], index: 0 }],
            outputs: vec![ByronTxOut { address: mk_addr(), amount: 100 }],
            attributes: mk_attrs(),
        };
        let tx2 = ByronTx {
            inputs: vec![ByronTxIn { txid: [0x02; 32], index: 0 }],
            outputs: vec![ByronTxOut { address: mk_addr(), amount: 100 }],
            attributes: mk_attrs(),
        };
        assert_ne!(tx1.tx_id(), tx2.tx_id());
    }

    // ── ByronTxAux ─────────────────────────────────────────────────────

    #[test]
    fn tx_aux_round_trip() {
        let aux = ByronTxAux {
            tx: ByronTx {
                inputs: vec![ByronTxIn { txid: [0xDD; 32], index: 0 }],
                outputs: vec![ByronTxOut { address: mk_addr(), amount: 1_000_000 }],
                attributes: mk_attrs(),
            },
            witnesses: vec![ByronTxWitness { witness_type: 0, payload: vec![0xEE; 64] }],
        };
        let decoded = ByronTxAux::from_cbor_bytes(&aux.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, aux);
    }

    #[test]
    fn tx_aux_empty_witnesses_round_trip() {
        let aux = ByronTxAux {
            tx: ByronTx {
                inputs: vec![ByronTxIn { txid: [0xFF; 32], index: 0 }],
                outputs: vec![ByronTxOut { address: mk_addr(), amount: 500 }],
                attributes: mk_attrs(),
            },
            witnesses: vec![],
        };
        let decoded = ByronTxAux::from_cbor_bytes(&aux.to_cbor_bytes()).unwrap();
        assert_eq!(decoded, aux);
    }

    // ── ByronBlock helpers ─────────────────────────────────────────────

    #[test]
    fn ebb_accessors() {
        let blk = ByronBlock::EpochBoundary {
            epoch: 10,
            chain_difficulty: 100,
            prev_hash: [0xAB; 32],
            raw_header: vec![],
        };
        assert_eq!(blk.epoch(), 10);
        assert_eq!(blk.chain_difficulty(), 100);
        assert!(blk.is_ebb());
        assert_eq!(blk.absolute_slot(BYRON_SLOTS_PER_EPOCH), 10 * BYRON_SLOTS_PER_EPOCH);
        assert_eq!(blk.prev_hash(), &[0xAB; 32]);
        assert_eq!(blk.issuer_vkey(), [0u8; 32]);
        assert!(blk.transactions().is_empty());
    }

    #[test]
    fn main_block_accessors() {
        let blk = ByronBlock::MainBlock {
            epoch: 5,
            slot_in_epoch: 100,
            chain_difficulty: 50,
            prev_hash: [0xCD; 32],
            issuer_vkey: [0xEF; 32],
            raw_header: vec![],
            transactions: vec![],
        };
        assert_eq!(blk.epoch(), 5);
        assert_eq!(blk.chain_difficulty(), 50);
        assert!(!blk.is_ebb());
        assert_eq!(blk.absolute_slot(BYRON_SLOTS_PER_EPOCH), 5 * BYRON_SLOTS_PER_EPOCH + 100);
        assert_eq!(blk.prev_hash(), &[0xCD; 32]);
        assert_eq!(blk.issuer_vkey(), [0xEF; 32]);
        assert!(blk.transactions().is_empty());
    }

    #[test]
    fn ebb_header_hash_deterministic() {
        let blk = ByronBlock::EpochBoundary {
            epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0x00; 32],
            raw_header: vec![0x83, 0x01, 0x02, 0x03],
        };
        assert_eq!(blk.header_hash(), blk.header_hash());
    }

    #[test]
    fn main_header_hash_deterministic() {
        let blk = ByronBlock::MainBlock {
            epoch: 1,
            slot_in_epoch: 10,
            chain_difficulty: 5,
            prev_hash: [0x00; 32],
            issuer_vkey: [0x00; 32],
            raw_header: vec![0x84, 0x01, 0x02, 0x03, 0x04],
            transactions: vec![],
        };
        assert_eq!(blk.header_hash(), blk.header_hash());
    }

    #[test]
    fn ebb_vs_main_header_hash_prefix_differ() {
        let raw = vec![0x83, 0x01, 0x02, 0x03];
        let ebb = ByronBlock::EpochBoundary {
            epoch: 0, chain_difficulty: 0,
            prev_hash: [0x00; 32], raw_header: raw.clone(),
        };
        let main = ByronBlock::MainBlock {
            epoch: 0, slot_in_epoch: 0, chain_difficulty: 0,
            prev_hash: [0x00; 32], issuer_vkey: [0x00; 32],
            raw_header: raw, transactions: vec![],
        };
        assert_ne!(ebb.header_hash(), main.header_hash());
    }

    #[test]
    fn header_hash_is_32_bytes() {
        let blk = ByronBlock::EpochBoundary {
            epoch: 0, chain_difficulty: 0,
            prev_hash: [0x00; 32], raw_header: vec![0x01],
        };
        assert_eq!(blk.header_hash().0.len(), 32);
    }
}
