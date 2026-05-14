//! Shelley-era block, header, and point CBOR decoders.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side adapter functions that
//! bridge node-to-node ChainSync / BlockFetch payloads into the
//! typed ledger surface from `yggdrasil_ledger`. Surfaces upstream
//! Shelley decoding logic from
//! `Ouroboros.Consensus.Shelley.Ledger.Block` (`decodeShelleyBlock`,
//! `decodeShelleyHeader`) plus the byte-span preserving conversion
//! to the storage `Block` wrapper that is byte-exact-fee-correct
//! per `Cardano.Ledger.Shelley.Tx.minfee`.
//!
//! Public functions moved from `node/src/sync.rs`:
//!
//! - `shelley_block_to_block` — full conversion with span extraction.
//! - `shelley_block_to_block_with_spans` — hot-path variant taking
//!   pre-extracted byte spans.
//! - `decode_shelley_blocks` — list of BlockFetch payloads.
//! - `decode_shelley_header` — ChainSync header payload.
//! - `decode_point` — ChainSync point / tip payload.
//!
//! Plus the `compute_tx_id` helper, promoted to `pub(super)` so
//! Phase-35 multi-era decoders and Phase-40 mempool eviction (still
//! resident in `sync.rs`) keep working.
//!
//! Extracted from `node/src/sync.rs` in R499 (sync.rs R-arc, 2nd
//! slice). See `docs/operational-runs/2026-05-12-round-498-plan-sync-rs-split-arc.md`
//! for the multi-round plan.

use yggdrasil_crypto::blake2b::hash_bytes_256;
use yggdrasil_ledger::{
    Block, BlockHeader, BlockNo, BlockTxRawSpans, CborDecode, CborEncode, Era, HeaderHash, Point,
    ShelleyBlock, ShelleyHeader, SlotNo, Tx, TxId,
};

use super::{SyncError, apply_raw_header_hash_override};

/// Compute a `TxId` as the Blake2b-256 hash of the CBOR-encoded transaction
/// body, matching the upstream Cardano transaction identifier.
///
/// Reference: `Cardano.Ledger.TxIn` — `TxId`.
pub(super) fn compute_tx_id(body: &[u8]) -> TxId {
    TxId(hash_bytes_256(body).0)
}

/// Convert a typed Shelley block into the generic ledger `Block` wrapper used
/// by storage traits.
///
/// `raw_block_bytes` is the original on-wire CBOR encoding of the block
/// (as received from BlockFetch).  Each transaction's `body` and
/// `witnesses` slots are populated with the **exact on-wire byte spans**
/// extracted from this buffer rather than re-serialised from the typed
/// `ShelleyTxBody` / `ShelleyWitnessSet` values.  Re-serialisation is
/// byte-canonical CBOR but does not always agree byte-for-byte with the
/// block author's original encoding (set vs array, definite vs
/// indefinite length, integer-width canonicalisation), and the linear
/// fee formula `min_fee = a · txSize + b` is sensitive to that
/// difference.
///
/// Reference: `Cardano.Ledger.Shelley.Tx.minfee`,
/// `Cardano.Ledger.Core.txIdTxBody`.
pub fn shelley_block_to_block(block: &ShelleyBlock, raw_block_bytes: &[u8]) -> Block {
    let spans = yggdrasil_ledger::extract_block_tx_byte_spans(raw_block_bytes).unwrap_or_default();
    apply_raw_header_hash_override(
        shelley_block_to_block_with_spans(block, &spans),
        raw_block_bytes,
    )
}

/// Variant of [`shelley_block_to_block`] that consumes pre-extracted
/// `BlockTxRawSpans` instead of re-walking the block CBOR.
///
/// Use this on the sync hot path when spans are already cached on the
/// `MultiEraSyncStep::RollForward.block_spans` field — saves one CBOR
/// walk per block.
pub fn shelley_block_to_block_with_spans(block: &ShelleyBlock, spans: &BlockTxRawSpans) -> Block {
    let body = &block.header.body;
    let hash = block.header_hash();
    let prev_hash = HeaderHash(body.prev_hash.unwrap_or([0u8; 32]));

    let transactions: Vec<Tx> = block
        .transaction_bodies
        .iter()
        .enumerate()
        .zip(
            block
                .transaction_witness_sets
                .iter()
                .map(Some)
                .chain(std::iter::repeat(None)),
        )
        .map(|((idx, tx_body), ws)| {
            // Prefer the on-wire byte span; fall back to a typed
            // re-encoding only if span extraction failed (test paths).
            let raw_body = spans
                .bodies
                .get(idx)
                .cloned()
                .unwrap_or_else(|| tx_body.to_cbor_bytes());
            let raw_witnesses = spans
                .witness_sets
                .get(idx)
                .cloned()
                .or_else(|| ws.map(|w| w.to_cbor_bytes()));
            Tx {
                id: compute_tx_id(&raw_body),
                body: raw_body,
                witnesses: raw_witnesses,
                auxiliary_data: block.transaction_metadata_set.get(&(idx as u64)).cloned(),
                is_valid: None,
            }
        })
        .collect();

    Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash,
            prev_hash,
            slot_no: SlotNo(body.slot),
            block_no: BlockNo(body.block_number),
            issuer_vkey: body.issuer_vkey,
            protocol_version: Some(body.protocol_version),
        },
        transactions,
        raw_cbor: None,
        header_cbor_size: Some(block.header.to_cbor_bytes().len()),
    }
}

/// Decode a list of raw BlockFetch payloads into Shelley blocks.
pub fn decode_shelley_blocks(raw_blocks: &[Vec<u8>]) -> Result<Vec<ShelleyBlock>, SyncError> {
    raw_blocks
        .iter()
        .map(|raw| ShelleyBlock::from_cbor_bytes(raw))
        .collect::<Result<Vec<_>, _>>()
        .map_err(SyncError::LedgerDecode)
}

/// Decode a raw ChainSync header payload into a typed Shelley header.
pub fn decode_shelley_header(raw_header: &[u8]) -> Result<ShelleyHeader, SyncError> {
    ShelleyHeader::from_cbor_bytes(raw_header).map_err(SyncError::LedgerDecode)
}

/// Decode a raw ChainSync point/tip payload into a typed ledger `Point`.
pub fn decode_point(raw_point: &[u8]) -> Result<Point, SyncError> {
    Point::from_cbor_bytes(raw_point).map_err(SyncError::LedgerDecode)
}
