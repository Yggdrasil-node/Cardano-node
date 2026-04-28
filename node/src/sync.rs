//! Sync orchestration helpers for node-to-node ChainSync + BlockFetch.
//!
//! This module provides a thin runtime coordination layer between the
//! `ChainSyncClient` and `BlockFetchClient` drivers from `yggdrasil-network`.
//! It intentionally keeps ledger and consensus validation out of the node
//! crate and focuses only on protocol sequencing.

use std::time::Duration;

use std::collections::BTreeMap;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use yggdrasil_consensus::{
    ActiveSlotCoeff, ChainEntry, ChainState, ClockSkew, ConsensusError, EpochSchedule, EpochSize,
    FutureSlotJudgement, Header as ConsensusHeader, HeaderBody as ConsensusHeaderBody,
    NonceDerivation, NonceEvolutionConfig, NonceEvolutionState, OcertCounters,
    OpCert as ConsensusOpCert, SecurityParam, TentativeState, VrfMode, judge_header_slot,
    verify_header, verify_leader_proof, verify_nonce_proof,
};
use yggdrasil_crypto::blake2b::hash_bytes_256;
use yggdrasil_crypto::ed25519::{Signature as Ed25519Signature, VerificationKey};
use yggdrasil_crypto::sum_kes::{SumKesSignature, SumKesVerificationKey};
use yggdrasil_crypto::vrf::VrfVerificationKey;
use yggdrasil_ledger::{
    AlonzoBlock, BYRON_SLOTS_PER_EPOCH, BabbageBlock, Block, BlockHeader, BlockNo, ByronBlock,
    CborDecode, CborEncode, ConwayBlock, Decoder, EpochBoundaryEvent, Era, HeaderHash, LedgerError,
    LedgerState, Nonce, Point, PoolKeyHash, PraosHeader, PraosHeaderBody, ShelleyBlock,
    ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyTxIn, SlotNo, StakeSnapshots, Tx, TxId,
    UnitInterval, apply_epoch_boundary, compute_block_body_hash,
};
use yggdrasil_mempool::Mempool;
use yggdrasil_network::{
    BlockFetchClient, BlockFetchClientError, BlockFetchInstrumentation, ChainRange,
    ChainSyncClient, ChainSyncClientError, DecodedHeaderNextResponse, KeepAliveClient,
    KeepAliveClientError, NextResponse, PeerError, TypedIntersectResponse, TypedNextResponse,
};
use yggdrasil_plutus::CostModel;
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, StorageError, VolatileStore};

pub use yggdrasil_storage::LedgerRecoveryOutcome;

/// Error type for sync orchestration operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Peer bootstrap or handshake error before protocol sync begins.
    #[error("peer error: {0}")]
    Peer(#[from] PeerError),

    /// ChainSync protocol error while requesting next chain update.
    #[error("chainsync error: {0}")]
    ChainSync(#[from] ChainSyncClientError),

    /// BlockFetch protocol error while fetching blocks for a roll-forward.
    #[error("blockfetch error: {0}")]
    BlockFetch(#[from] BlockFetchClientError),

    /// Ledger decode error while deserializing fetched block bytes.
    #[error("ledger decode error: {0}")]
    LedgerDecode(#[from] LedgerError),

    /// Storage error while applying decoded sync results.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    /// KeepAlive protocol error during heartbeat.
    #[error("keepalive error: {0}")]
    KeepAlive(#[from] KeepAliveClientError),

    /// Consensus validation error (header verification failure).
    #[error("consensus error: {0}")]
    Consensus(#[from] ConsensusError),

    /// Recovery failed because the available storage state could not be
    /// reconstructed into a usable ledger tip.
    #[error("recovery error: {0}")]
    Recovery(String),

    /// Block body hash in the header does not match the actual block body.
    #[error("block body hash mismatch")]
    BlockBodyHashMismatch,

    /// A received block's slot is beyond the tolerable clock skew.
    ///
    /// Reference: `InFutureHeaderExceedsClockSkew` in
    /// `Ouroboros.Consensus.MiniProtocol.ChainSync.Client.InFutureCheck`.
    #[error("block from far future: slot {slot} is {excess_slots} slots ahead of wall clock")]
    BlockFromFuture {
        /// The block's slot number.
        slot: u64,
        /// How many slots ahead of the wall-clock the block is.
        excess_slots: u64,
    },

    /// The declared block body size in the header does not match the actual
    /// serialized body size.
    ///
    /// Reference: `WrongBlockBodySizeBBODY` in
    /// `Cardano.Ledger.Shelley.Rules.Bbody`.
    #[error(
        "wrong block body size: header declares {declared} bytes, \
         actual body is {actual} bytes"
    )]
    WrongBlockBodySize {
        /// The `block_body_size` field from the block header.
        declared: u32,
        /// The actual serialized size of the block body.
        actual: u32,
    },

    /// The block header's protocol version is outside the acceptable range
    /// for the era it claims to be in.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Chain` — era/protocol
    /// version consistency check.
    #[error(
        "protocol version mismatch: block in era {era:?} carries version \
         ({major}, {minor}), expected major in {expected_range}"
    )]
    ProtocolVersionMismatch {
        /// The era of the block.
        era: Era,
        /// Declared major version.
        major: u64,
        /// Declared minor version.
        minor: u64,
        /// Human-readable expected range string.
        expected_range: String,
    },

    /// The block header's major protocol version exceeds the maximum
    /// major version configured for this node.
    ///
    /// Reference: `MaxMajorProtVer` in
    /// `Ouroboros.Consensus.Shelley.Ledger.Block`.
    #[error(
        "protocol version too high: block major version {major} exceeds \
         node maximum {max}"
    )]
    ProtocolVersionTooHigh {
        /// Declared major version from the block header.
        major: u64,
        /// The node's configured `MaxMajorProtVer`.
        max: u64,
    },

    /// Block header major protocol version exceeds
    /// `pp.protocolVersion.major + 1`.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Bbody` —
    /// `HeaderProtVerTooHigh`.
    #[error(
        "header protocol version too high: major {header_major} > pp major \
         {pp_major} + 1"
    )]
    HeaderProtVerTooHigh {
        /// Major version declared in the block header.
        header_major: u64,
        /// Current protocol-parameter major version.
        pp_major: u64,
    },
}

impl SyncError {
    /// Returns `true` when the error is attributable to the remote peer
    /// sending data that fails validation (invalid block body hash,
    /// consensus header verification failure, or a block that breaks
    /// ledger rules).
    ///
    /// These errors indicate a misbehaving or broken peer and should be
    /// handled by reconnecting to a different peer rather than stopping
    /// the sync service.  Local infrastructure errors (`Storage`) and
    /// protocol framing errors (`ChainSync`, `BlockFetch`) are not
    /// peer-attributable validation failures.
    ///
    /// Reference: upstream `InvalidBlockPunishment` in
    /// `Ouroboros.Consensus.Storage.ChainDB.API.Types.InvalidBlockPunishment`
    /// — errors that result in `throwTo PeerSentAnInvalidBlockException`.
    pub fn is_peer_attributable(&self) -> bool {
        matches!(
            self,
            SyncError::Consensus(_)
                | SyncError::BlockBodyHashMismatch
                | SyncError::LedgerDecode(_)
                | SyncError::BlockFromFuture { .. }
                | SyncError::WrongBlockBodySize { .. }
                | SyncError::ProtocolVersionMismatch { .. }
                | SyncError::ProtocolVersionTooHigh { .. }
                | SyncError::HeaderProtVerTooHigh { .. }
        )
    }
}

/// Result of a single sync step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncStep {
    /// The peer rolled forward; any fetched blocks for the announced range are
    /// included in `blocks`.
    RollForward {
        /// Opaque header payload from ChainSync.
        header: Vec<u8>,
        /// Opaque tip/point payload from ChainSync.
        tip: Vec<u8>,
        /// Serialized blocks returned by BlockFetch for this step.
        blocks: Vec<Vec<u8>>,
    },

    /// The peer rolled backward to `point`.
    RollBackward {
        /// Opaque rollback target point from ChainSync.
        point: Vec<u8>,
        /// Opaque current tip from ChainSync.
        tip: Vec<u8>,
    },
}

/// Aggregate output from running multiple sync steps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncProgress {
    /// The latest known chain point after applying all steps.
    pub current_point: Vec<u8>,
    /// Ordered sequence of step outcomes.
    pub steps: Vec<SyncStep>,
    /// Total number of fetched blocks across all roll-forward steps.
    pub fetched_blocks: usize,
}

/// Result of a sync step where roll-forward blocks are decoded into
/// `ShelleyBlock` structures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedSyncStep {
    /// The peer rolled forward and all fetched blocks decoded successfully.
    RollForward {
        /// Opaque header payload from ChainSync.
        header: Vec<u8>,
        /// Opaque tip/point payload from ChainSync.
        tip: Vec<u8>,
        /// Decoded Shelley blocks fetched for this step.
        blocks: Vec<ShelleyBlock>,
    },

    /// The peer rolled backward to `point`.
    RollBackward {
        /// Opaque rollback target point from ChainSync.
        point: Vec<u8>,
        /// Opaque current tip from ChainSync.
        tip: Vec<u8>,
    },
}

/// Result of a sync step where ChainSync payloads are decoded into typed
/// ledger values and roll-forward blocks are decoded into `ShelleyBlock`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedSyncStep {
    /// Fully decoded roll-forward step.
    RollForward {
        /// Decoded Shelley header from ChainSync.
        header: Box<ShelleyHeader>,
        /// Decoded tip point from ChainSync.
        tip: Point,
        /// Decoded Shelley blocks fetched via BlockFetch.
        blocks: Vec<ShelleyBlock>,
        /// Original on-wire CBOR bytes of each block, parallel-indexed with
        /// `blocks`.  Required for byte-exact fee validation and tx-id
        /// computation per upstream `Cardano.Ledger.Shelley.Tx.minfee`.
        raw_blocks: Vec<Vec<u8>>,
    },

    /// Fully decoded roll-backward step.
    RollBackward {
        /// Decoded rollback target point.
        point: Point,
        /// Decoded current tip point.
        tip: Point,
    },
}

/// Aggregate output from running multiple typed sync steps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedSyncProgress {
    /// The latest known typed point after applying all steps.
    pub current_point: Point,
    /// Ordered sequence of typed step outcomes.
    pub steps: Vec<TypedSyncStep>,
    /// Total number of fetched blocks across all roll-forward steps.
    pub fetched_blocks: usize,
    /// Number of rollback steps observed.
    pub rollback_count: usize,
}

/// Compute a `TxId` as the Blake2b-256 hash of the CBOR-encoded transaction
/// body, matching the upstream Cardano transaction identifier.
///
/// Reference: `Cardano.Ledger.TxIn` — `TxId`.
fn compute_tx_id(body: &[u8]) -> TxId {
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
    shelley_block_to_block_with_spans(block, &spans)
}

/// Variant of [`shelley_block_to_block`] that consumes pre-extracted
/// `BlockTxRawSpans` instead of re-walking the block CBOR.
///
/// Use this on the sync hot path when spans are already cached on the
/// `MultiEraSyncStep::RollForward.block_spans` field — saves one CBOR
/// walk per block.
pub fn shelley_block_to_block_with_spans(
    block: &ShelleyBlock,
    spans: &yggdrasil_ledger::BlockTxRawSpans,
) -> Block {
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

fn map_blockfetch_error(err: BlockFetchClientError) -> SyncError {
    match err {
        BlockFetchClientError::BlockDecode(err) => SyncError::LedgerDecode(err),
        other => SyncError::BlockFetch(other),
    }
}

async fn fetch_range_blocks(
    block_fetch: &mut BlockFetchClient,
    lower: Vec<u8>,
    upper: Vec<u8>,
) -> Result<Vec<Vec<u8>>, SyncError> {
    block_fetch
        .request_range_collect(ChainRange { lower, upper })
        .await
        .map_err(SyncError::BlockFetch)
}

async fn fetch_range_blocks_typed(
    block_fetch: &mut BlockFetchClient,
    lower: Point,
    upper: Point,
) -> Result<Vec<(Vec<u8>, ShelleyBlock)>, SyncError> {
    block_fetch
        .request_range_collect_points_raw_with(lower, upper, ShelleyBlock::from_cbor_bytes)
        .await
        .map_err(map_blockfetch_error)
}

pub(crate) async fn fetch_range_blocks_multi_era_raw_decoded(
    block_fetch: &mut BlockFetchClient,
    lower: Point,
    upper: Point,
) -> Result<Vec<(Vec<u8>, MultiEraBlock)>, SyncError> {
    block_fetch
        .request_range_collect_points_raw_with(lower, upper, decode_multi_era_block_ledger)
        .await
        .map_err(map_blockfetch_error)
}

async fn fetch_range_blocks_decoded(
    block_fetch: &mut BlockFetchClient,
    lower: Vec<u8>,
    upper: Vec<u8>,
) -> Result<Vec<ShelleyBlock>, SyncError> {
    block_fetch
        .request_range_collect_decoded::<ShelleyBlock>(ChainRange { lower, upper })
        .await
        .map_err(map_blockfetch_error)
}

fn normalize_blockfetch_range_points(lower: Point, upper: Point) -> Option<(Point, Point)> {
    let upper_is_block = matches!(upper, Point::BlockPoint(_, _));
    if !upper_is_block {
        return None;
    }

    if let (Point::BlockPoint(lower_slot, _), Point::BlockPoint(upper_slot, _)) = (lower, upper) {
        if upper_slot < lower_slot {
            return None;
        }
    }

    // Upstream BlockFetch (Ouroboros.Network.BlockFetch.Server) cannot resolve
    // `Point::Origin` as a lower bound — genesis is a virtual point with no
    // fetchable header. When the caller has no prior known tip we collapse the
    // range to `[upper, upper]` so the wire `MsgRequestRange` requests just the
    // single block at `upper`. Callers that need to detect this case and avoid
    // dropping the fetched block must inspect the *original* `from_point`
    // before normalization (see the dedup gate in
    // `sync_batch_verified_with_tentative`).
    let normalized_lower = if matches!(lower, Point::Origin) {
        upper
    } else {
        lower
    };

    Some((normalized_lower, upper))
}

fn normalize_blockfetch_range_bytes(lower: Vec<u8>, upper: Vec<u8>) -> Option<(Vec<u8>, Vec<u8>)> {
    // When both bounds decode as proper Points, apply upstream Origin->upper
    // normalization. Otherwise (synthetic/opaque payloads used by lower-level
    // tests and pass-through call sites) hand the raw bytes through unchanged
    // so the BlockFetch wire request still carries the caller's intent.
    match (
        Point::from_cbor_bytes(&lower).ok(),
        Point::from_cbor_bytes(&upper).ok(),
    ) {
        (Some(lower_point), Some(upper_point)) => {
            let (normalized_lower, normalized_upper) =
                normalize_blockfetch_range_points(lower_point, upper_point)?;
            Some((
                normalized_lower.to_cbor_bytes(),
                normalized_upper.to_cbor_bytes(),
            ))
        }
        _ => Some((lower, upper)),
    }
}

fn point_from_raw_header(raw_header: &[u8]) -> Option<Point> {
    fn decode_slot_from_with_origin(value: &[u8]) -> Option<u64> {
        let mut dec = Decoder::new(value);
        if let Ok(2) = dec.array() {
            let tag = dec.unsigned().ok()?;
            if tag != 0 && tag != 1 {
                return None;
            }
            let slot = dec.unsigned().ok()?;
            if dec.is_empty() {
                return Some(slot);
            }
        }
        None
    }

    fn decode_cbor_in_cbor_bytes(value: &[u8]) -> Option<Vec<u8>> {
        let mut dec = Decoder::new(value);
        if let Ok(unwrapped) = dec.wrapped() {
            if dec.is_empty() {
                return Some(unwrapped.to_vec());
            }
        }

        let mut dec = Decoder::new(value);
        if let Ok(bytes) = dec.bytes() {
            if dec.is_empty() {
                return Some(bytes.to_vec());
            }
        }

        None
    }

    fn byron_main_header_hash(raw_header: &[u8]) -> HeaderHash {
        // Byron main header hash uses prefix ++ raw annotated header bytes.
        const MAIN_HASH_PREFIX: [u8; 2] = [0x82, 0x01];
        let mut bytes = Vec::with_capacity(MAIN_HASH_PREFIX.len() + raw_header.len());
        bytes.extend_from_slice(&MAIN_HASH_PREFIX);
        bytes.extend_from_slice(raw_header);
        HeaderHash(hash_bytes_256(&bytes).0)
    }

    fn decode_point_from_byron_raw_header(raw_header: &[u8]) -> Option<Point> {
        let mut dec = Decoder::new(raw_header);
        if dec.array().ok()? < 5 {
            return None;
        }

        // protocol_magic, prev_hash, body_proof
        dec.skip().ok()?;
        dec.skip().ok()?;
        dec.skip().ok()?;

        let consensus_len = dec.array().ok()?;
        if consensus_len >= 4 {
            // Main block header consensus_data: [slot_id, pubkey, difficulty, signature]
            let slot_id_len = dec.array().ok()?;
            if slot_id_len != 2 {
                return None;
            }

            let epoch = dec.unsigned().ok()?;
            let slot_in_epoch = dec.unsigned().ok()?;
            let slot = epoch
                .checked_mul(BYRON_SLOTS_PER_EPOCH)
                .and_then(|base| base.checked_add(slot_in_epoch))?;
            return Some(Point::BlockPoint(
                SlotNo(slot),
                byron_main_header_hash(raw_header),
            ));
        }

        if consensus_len >= 2 {
            // EBB headers don't carry slot-in-epoch in the same way as main
            // headers; prefer the outer with-origin slot for those envelopes.
            return None;
        }

        None
    }

    fn decode_header_point_bytes(bytes: &[u8]) -> Option<Point> {
        if let Ok(header) = ShelleyHeader::from_cbor_bytes(bytes) {
            return Some(Point::BlockPoint(
                SlotNo(header.body.slot),
                header.header_hash(),
            ));
        }

        if let Ok(header) = PraosHeader::from_cbor_bytes(bytes) {
            return Some(Point::BlockPoint(
                SlotNo(header.body.slot),
                header.header_hash(),
            ));
        }

        None
    }

    fn decode_point_bytes(bytes: &[u8]) -> Option<Point> {
        Point::from_cbor_bytes(bytes).ok()
    }

    fn decode_point_from_serialised_header(bytes: &[u8]) -> Option<Point> {
        let mut dec = Decoder::new(bytes);
        if let Ok(2) = dec.array() {
            let first = dec.raw_value().ok()?;
            let second = dec.raw_value().ok()?;
            if !dec.is_empty() {
                return None;
            }

            // Common serialised-header form: [point, header-bytes]
            if let Ok(point) = Point::from_cbor_bytes(first) {
                return Some(point);
            }

            // Some serialised forms store the header in CBOR-in-CBOR.
            let mut second_dec = Decoder::new(second);
            if let Ok(unwrapped) = second_dec.wrapped() {
                if second_dec.is_empty() {
                    if let Some(point) = decode_header_point_bytes(unwrapped) {
                        return Some(point);
                    }
                }
            }

            // Fallback: second field might already be a direct header payload.
            if let Some(point) = decode_header_point_bytes(second) {
                return Some(point);
            }

            // Byron serialised-header envelope observed in preprod:
            // [[withOriginSlot], tag24(rawHeaderBytes)]
            let raw_header_bytes = decode_cbor_in_cbor_bytes(second)?;
            if let Some(point) = decode_point_from_byron_raw_header(&raw_header_bytes) {
                return Some(point);
            }

            // Last-resort fallback for envelope variants where only slot is
            // recoverable from the outer with-origin field.
            let slot = decode_slot_from_with_origin(first)?;
            Some(Point::BlockPoint(
                SlotNo(slot),
                byron_main_header_hash(&raw_header_bytes),
            ))
        } else {
            None
        }
    }

    if let Some(point) = decode_header_point_bytes(raw_header) {
        return Some(point);
    }

    // ChainSync roll-forward headers may arrive in a 2-element envelope. In
    // practice we see multiple layouts depending on the negotiated header
    // flavor (typed header, serialised header, multi-era envelope), so try a
    // small set of upstream-compatible heuristics.
    let mut dec = Decoder::new(raw_header);
    if let Ok(2) = dec.array() {
        if let (Ok(first), Ok(second)) = (dec.raw_value(), dec.raw_value()) {
            if dec.is_empty() {
                // Layout A: [eraTag, headerPayload]
                let mut first_dec = Decoder::new(first);
                if first_dec.unsigned().is_ok() && first_dec.is_empty() {
                    if let Some(point) = decode_point_from_serialised_header(second) {
                        return Some(point);
                    }
                    if let Some(point) = decode_header_point_bytes(second) {
                        return Some(point);
                    }
                    // NTN ChainSync wraps the actual header body as
                    // tag(24, bytes(<header-cbor>)) (CBOR-in-CBOR). Unwrap
                    // and retry Shelley/Praos point extraction. Without
                    // this, post-Byron headers fail to decode and the
                    // BlockFetch upper falls back to the chain tip, which
                    // upstream rejects as an unfetchable range.
                    if let Some(unwrapped) = decode_cbor_in_cbor_bytes(second) {
                        if let Some(point) = decode_header_point_bytes(&unwrapped) {
                            return Some(point);
                        }
                        if let Some(point) = decode_point_from_byron_raw_header(&unwrapped) {
                            return Some(point);
                        }
                    }
                }

                // Layout B: [point, serialised-header-bytes]
                if let Some(point) = decode_point_bytes(first) {
                    return Some(point);
                }

                // Layout C: [serialised-header-bytes, point]
                if let Some(point) = decode_point_bytes(second) {
                    return Some(point);
                }

                // Layout D: [x, headerPayload] or [headerPayload, x]
                if let Some(point) = decode_point_from_serialised_header(first) {
                    return Some(point);
                }
                if let Some(point) = decode_point_from_serialised_header(second) {
                    return Some(point);
                }
                if let Some(point) = decode_header_point_bytes(first) {
                    return Some(point);
                }
                if let Some(point) = decode_header_point_bytes(second) {
                    return Some(point);
                }
            }
        }
    }

    None
}

fn point_bytes_from_raw_header_or_tip(raw_header: &[u8], tip: Vec<u8>) -> Vec<u8> {
    point_from_raw_header(raw_header)
        .map(|point| point.to_cbor_bytes())
        .unwrap_or(tip)
}

/// Execute one sync step:
/// 1. Request the next ChainSync update.
/// 2. If roll-forward, request matching blocks via BlockFetch.
///
/// `from_point` is used as the lower bound for BlockFetch range requests.
pub async fn sync_step(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    from_point: Vec<u8>,
) -> Result<SyncStep, SyncError> {
    let next = chain_sync.request_next().await?;

    match next {
        NextResponse::RollForward { header, tip }
        | NextResponse::AwaitRollForward { header, tip } => {
            let range_upper = point_bytes_from_raw_header_or_tip(&header, tip.clone());
            let blocks = if let Some((lower, upper)) =
                normalize_blockfetch_range_bytes(from_point, range_upper)
            {
                fetch_range_blocks(block_fetch, lower, upper).await?
            } else {
                Vec::new()
            };
            Ok(SyncStep::RollForward {
                header,
                tip,
                blocks,
            })
        }
        NextResponse::RollBackward { point, tip }
        | NextResponse::AwaitRollBackward { point, tip } => {
            Ok(SyncStep::RollBackward { point, tip })
        }
    }
}

/// Execute one sync step and decode any roll-forward block payloads into
/// typed `ShelleyBlock` values.
pub async fn sync_step_decoded(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    from_point: Vec<u8>,
) -> Result<DecodedSyncStep, SyncError> {
    let next = chain_sync.request_next().await?;
    match next {
        NextResponse::RollForward { header, tip }
        | NextResponse::AwaitRollForward { header, tip } => Ok(DecodedSyncStep::RollForward {
            blocks: {
                let range_upper = point_bytes_from_raw_header_or_tip(&header, tip.clone());
                if let Some((lower, upper)) =
                    normalize_blockfetch_range_bytes(from_point, range_upper)
                {
                    fetch_range_blocks_decoded(block_fetch, lower, upper).await?
                } else {
                    Vec::new()
                }
            },
            header,
            tip: tip.clone(),
        }),
        NextResponse::RollBackward { point, tip }
        | NextResponse::AwaitRollBackward { point, tip } => {
            Ok(DecodedSyncStep::RollBackward { point, tip })
        }
    }
}

/// Execute one sync step and decode all ChainSync and BlockFetch payloads
/// into typed ledger values.
pub async fn sync_step_typed(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    from_point: Point,
) -> Result<TypedSyncStep, SyncError> {
    let next = chain_sync
        .request_next_decoded_header::<ShelleyHeader>()
        .await?;

    match next {
        DecodedHeaderNextResponse::RollForward { header, tip }
        | DecodedHeaderNextResponse::AwaitRollForward { header, tip } => {
            let header_point = Point::BlockPoint(SlotNo(header.body.slot), header.header_hash());
            let pairs = if let Some((lower, upper)) =
                normalize_blockfetch_range_points(from_point, header_point)
            {
                fetch_range_blocks_typed(block_fetch, lower, upper).await?
            } else {
                Vec::new()
            };
            let (raw_blocks, blocks): (Vec<Vec<u8>>, Vec<ShelleyBlock>) = pairs.into_iter().unzip();
            Ok(TypedSyncStep::RollForward {
                header: Box::new(header),
                tip,
                blocks,
                raw_blocks,
            })
        }
        DecodedHeaderNextResponse::RollBackward { point, tip }
        | DecodedHeaderNextResponse::AwaitRollBackward { point, tip } => {
            Ok(TypedSyncStep::RollBackward { point, tip })
        }
    }
}

/// Execute `count` consecutive sync steps, carrying forward the latest chain
/// point between each step.
///
/// Point update rules:
/// - `RollForward`: `current_point = tip`
/// - `RollBackward`: `current_point = point`
pub async fn sync_steps(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    mut from_point: Vec<u8>,
    count: usize,
) -> Result<SyncProgress, SyncError> {
    let mut steps = Vec::with_capacity(count);
    let mut fetched_blocks = 0usize;

    for _ in 0..count {
        let step = sync_step(chain_sync, block_fetch, from_point.clone()).await?;
        match &step {
            SyncStep::RollForward { tip, blocks, .. } => {
                from_point = tip.clone();
                fetched_blocks += blocks.len();
            }
            SyncStep::RollBackward { point, .. } => {
                from_point = point.clone();
            }
        }
        steps.push(step);
    }

    Ok(SyncProgress {
        current_point: from_point,
        steps,
        fetched_blocks,
    })
}

/// Execute `count` consecutive typed sync steps, carrying forward the latest
/// typed chain point between each step.
pub async fn sync_steps_typed(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    mut from_point: Point,
    count: usize,
) -> Result<TypedSyncProgress, SyncError> {
    let mut steps = Vec::with_capacity(count);
    let mut fetched_blocks = 0usize;
    let mut rollback_count = 0usize;

    for _ in 0..count {
        let step = sync_step_typed(chain_sync, block_fetch, from_point).await?;

        match &step {
            TypedSyncStep::RollForward { tip, blocks, .. } => {
                from_point = *tip;
                fetched_blocks += blocks.len();
            }
            TypedSyncStep::RollBackward { point, .. } => {
                from_point = *point;
                rollback_count += 1;
            }
        }

        steps.push(step);
    }

    Ok(TypedSyncProgress {
        current_point: from_point,
        steps,
        fetched_blocks,
        rollback_count,
    })
}

/// Run typed sync for up to `max_steps` transitions, optionally stopping early
/// once `stop_at` is reached as the current point.
pub async fn sync_until_typed(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    mut from_point: Point,
    max_steps: usize,
    stop_at: Option<Point>,
) -> Result<TypedSyncProgress, SyncError> {
    let mut steps = Vec::new();
    let mut fetched_blocks = 0usize;
    let mut rollback_count = 0usize;

    for _ in 0..max_steps {
        if stop_at.is_some_and(|target| target == from_point) {
            break;
        }

        let step = sync_step_typed(chain_sync, block_fetch, from_point).await?;

        match &step {
            TypedSyncStep::RollForward { tip, blocks, .. } => {
                from_point = *tip;
                fetched_blocks += blocks.len();
            }
            TypedSyncStep::RollBackward { point, .. } => {
                from_point = *point;
                rollback_count += 1;
            }
        }

        steps.push(step);

        if stop_at.is_some_and(|target| target == from_point) {
            break;
        }
    }

    Ok(TypedSyncProgress {
        current_point: from_point,
        steps,
        fetched_blocks,
        rollback_count,
    })
}

/// Apply one typed sync step into a volatile store.
///
/// Roll-forward blocks are converted and appended in order; roll-backward
/// steps trigger store rollback to the provided point.
pub fn apply_typed_step_to_volatile<S: VolatileStore>(
    store: &mut S,
    step: &TypedSyncStep,
) -> Result<(), SyncError> {
    match step {
        TypedSyncStep::RollForward {
            blocks, raw_blocks, ..
        } => {
            for (b, raw) in blocks.iter().zip(raw_blocks.iter()) {
                store.add_block(shelley_block_to_block(b, raw))?;
            }
        }
        TypedSyncStep::RollBackward { point, .. } => {
            store.rollback_to(point);
        }
    }
    Ok(())
}

/// Apply a typed sync progress bundle into a volatile store.
pub fn apply_typed_progress_to_volatile<S: VolatileStore>(
    store: &mut S,
    progress: &TypedSyncProgress,
) -> Result<(), SyncError> {
    for step in &progress.steps {
        apply_typed_step_to_volatile(store, step)?;
    }
    Ok(())
}

/// Result of a typed intersection query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedIntersectResult {
    /// The peer found a common intersection at `point`.
    Found {
        /// The intersection point agreed upon by the peer.
        point: Point,
        /// The peer's current tip.
        tip: Point,
    },
    /// The peer did not find any of the proposed points in its chain.
    NotFound {
        /// The peer's current tip.
        tip: Point,
    },
}

/// Find the intersection between our known chain and the peer's chain using
/// typed `Point` values.
///
/// Encodes the candidate points, calls `ChainSyncClient::find_intersect`, and
/// decodes the response into typed ledger values.
pub async fn typed_find_intersect(
    chain_sync: &mut ChainSyncClient,
    points: &[Point],
) -> Result<TypedIntersectResult, SyncError> {
    match chain_sync.find_intersect_points(points.to_vec()).await? {
        TypedIntersectResponse::Found { point, tip } => {
            Ok(TypedIntersectResult::Found { point, tip })
        }
        TypedIntersectResponse::NotFound { tip } => Ok(TypedIntersectResult::NotFound { tip }),
    }
}

/// Execute one batch of typed sync and apply the results to a volatile store.
///
/// Combines `sync_until_typed` with `apply_typed_progress_to_volatile` into
/// a single composable step. Returns the updated current point.
pub async fn sync_batch_apply<S: VolatileStore>(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    store: &mut S,
    from_point: Point,
    batch_size: usize,
    stop_at: Option<Point>,
) -> Result<TypedSyncProgress, SyncError> {
    let progress =
        sync_until_typed(chain_sync, block_fetch, from_point, batch_size, stop_at).await?;
    apply_typed_progress_to_volatile(store, &progress)?;
    Ok(progress)
}

/// Run keepalive heartbeats at the given interval until an error occurs.
///
/// Uses sequential cookies starting from 1. Returns the first error
/// encountered (typically a connection close or mux error).
pub async fn keepalive_heartbeat(
    keep_alive: &mut KeepAliveClient,
    interval: Duration,
) -> SyncError {
    let mut cookie: u16 = 1;
    loop {
        tokio::time::sleep(interval).await;
        if let Err(e) = keep_alive.keep_alive(cookie).await {
            return SyncError::KeepAlive(e);
        }
        cookie = cookie.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Phase 33: Managed sync service — continuous batch loop with shutdown
// ---------------------------------------------------------------------------

/// Configuration for the managed sync service.
#[derive(Clone, Debug)]
pub struct SyncServiceConfig {
    /// Number of typed sync steps per batch iteration.
    pub batch_size: usize,
    /// KeepAlive heartbeat interval. `None` disables heartbeats.
    pub keepalive_interval: Option<Duration>,
}

/// Outcome returned when the managed sync service finishes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncServiceOutcome {
    /// Final chain point when the service stopped.
    pub final_point: Point,
    /// Total blocks fetched across all batches.
    pub total_blocks: usize,
    /// Total rollback events across all batches.
    pub total_rollbacks: usize,
    /// Number of batch iterations completed.
    pub batches_completed: usize,
}

/// Run a continuous sync loop that batches typed sync steps into volatile
/// storage until `shutdown` is signalled.
///
/// The service:
/// 1. Calls `sync_batch_apply` with `config.batch_size` per iteration.
/// 2. Loops until the `shutdown` future resolves or a protocol error occurs.
/// 3. Returns `SyncServiceOutcome` summarizing the full run.
///
/// The `shutdown` parameter is a future that resolves when the service should
/// stop. Typically this is a `tokio::sync::oneshot::Receiver` or similar
/// cancellation signal.
///
/// # Errors
///
/// Returns `SyncError` if a protocol, decode, or storage error occurs during
/// any batch. Shutdown-triggered termination returns `Ok`.
pub async fn run_sync_service<S, F>(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    store: &mut S,
    mut from_point: Point,
    config: &SyncServiceConfig,
    shutdown: F,
) -> Result<SyncServiceOutcome, SyncError>
where
    S: VolatileStore,
    F: std::future::Future<Output = ()>,
{
    tokio::pin!(shutdown);

    let mut total_blocks = 0usize;
    let mut total_rollbacks = 0usize;
    let mut batches_completed = 0usize;

    loop {
        let batch_fut = sync_batch_apply(
            chain_sync,
            block_fetch,
            store,
            from_point,
            config.batch_size,
            None,
        );

        tokio::select! {
            biased;

            () = &mut shutdown => {
                return Ok(SyncServiceOutcome {
                    final_point: from_point,
                    total_blocks,
                    total_rollbacks,
                    batches_completed,
                });
            }

            result = batch_fut => {
                let progress = result?;
                from_point = progress.current_point;
                total_blocks += progress.fetched_blocks;
                total_rollbacks += progress.rollback_count;
                batches_completed += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 37: Verified sync service with nonce evolution tracking
// ---------------------------------------------------------------------------

/// Configuration for the verified managed sync service.
///
/// Extends the basic `SyncServiceConfig` with header/body verification and
/// optional epoch nonce tracking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerCheckpointPolicy {
    /// Minimum slot delta between automatic checkpoint writes.
    ///
    /// `0` preserves the previous behavior and writes after every successful
    /// batch. Rollback batches always force a refreshed checkpoint when
    /// checkpointing is enabled.
    pub min_slot_delta: u64,
    /// Maximum number of typed ledger checkpoints to retain.
    ///
    /// `0` disables persisted ledger checkpoints and clears any retained
    /// snapshots during live sync.
    pub max_snapshots: usize,
}

impl Default for LedgerCheckpointPolicy {
    fn default() -> Self {
        Self {
            min_slot_delta: 2160,
            max_snapshots: 8,
        }
    }
}

impl LedgerCheckpointPolicy {
    pub(crate) fn should_persist(
        &self,
        previous_point: &Point,
        current_point: &Point,
        forced: bool,
    ) -> bool {
        if self.max_snapshots == 0 {
            return false;
        }

        if forced {
            return *current_point != Point::Origin;
        }

        match (previous_point, current_point) {
            (Point::Origin, Point::BlockPoint(_, _)) => true,
            (
                Point::BlockPoint(previous_slot, previous_hash),
                Point::BlockPoint(current_slot, current_hash),
            ) => {
                (*current_slot == *previous_slot && *current_hash != *previous_hash)
                    || current_slot.0.saturating_sub(previous_slot.0) >= self.min_slot_delta
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VerifiedSyncServiceConfig {
    /// Number of multi-era sync steps per batch iteration.
    pub batch_size: usize,
    /// Header and body hash verification parameters.
    pub verification: VerificationConfig,
    /// Nonce evolution parameters.  When set, the service tracks the epoch
    /// nonce across all processed blocks.  `None` disables nonce tracking.
    pub nonce_config: Option<NonceEvolutionConfig>,
    /// Ouroboros security parameter `k`.  When set, the service maintains
    /// a [`ChainState`] that tracks chain topology and enforces rollback
    /// depth limits.  `None` disables chain state tracking.
    pub security_param: Option<SecurityParam>,
    /// Ledger checkpoint write cadence and retention policy for coordinated
    /// storage runs.
    pub checkpoint_policy: LedgerCheckpointPolicy,
    /// Optional calibrated CEK cost model used when applying blocks through
    /// phase-2 Plutus validation during checkpoint-tracked ledger replay.
    /// When absent, `CekPlutusEvaluator` falls back to `CostModel::default()`.
    pub plutus_cost_model: Option<CostModel>,
    /// Whether to verify VRF leader eligibility proofs against stake
    /// distribution during epoch-boundary-aware block application.
    ///
    /// Requires `nonce_config` to be set (so epoch nonce and stake snapshots
    /// are tracked).  When the `set` snapshot is non-empty and the epoch
    /// nonce is available, each Shelley-family block's VRF proof is checked
    /// against the pool's relative stake.
    ///
    /// Defaults to `false` for backward compatibility and because VRF
    /// verification is computationally expensive during initial sync.
    pub verify_vrf: bool,
    /// Active slot coefficient `f` from genesis, required when `verify_vrf`
    /// is `true`.  Ignored when VRF verification is disabled.
    pub active_slot_coeff: Option<ActiveSlotCoeff>,
    /// Slot duration in seconds from Shelley genesis (`slotLength`).
    /// Defaults to 1.0 when unset.
    pub slot_length_secs: Option<f64>,
    /// Seconds since Unix epoch of the network genesis moment.
    ///
    /// Parsed from `ShelleyGenesis.system_start`.  When set together with
    /// `slot_length_secs`, the Plutus evaluator converts slot numbers to
    /// POSIX milliseconds in the `POSIXTimeRange` field of ScriptContext
    /// (upstream `transVITime`).
    pub system_start_unix_secs: Option<f64>,
    /// Era-aware epoch schedule used for epoch-boundary detection during
    /// sync.  When `None`, the schedule degenerates to a fixed-length
    /// epoch using `nonce_config.epoch_size`, which is incorrect on
    /// networks with a Byron prefix (mainnet, preprod).
    pub epoch_schedule: Option<EpochSchedule>,
    /// Optional shared [`BlockFetchInstrumentation`] handle.  When set, the
    /// verified-sync batch loop records per-peer fetch dispatch / success /
    /// failure into the shared [`crate::sync::BlockFetchInstrumentation`]
    /// pool so the BlockFetch decision engine has live per-peer accounting
    /// across reconnects.  When `None`, the verified-sync path runs
    /// unchanged with no instrumentation overhead.
    ///
    /// The pool is currently single-peer-equivalent: concurrency is gated
    /// by the existing single-session pipeline.  A future slice (see
    /// `docs/PARITY_PLAN.md` Phase 3 item 5) will lift this to N≥2 peers
    /// via [`crate::sync::BlockFetchInstrumentation`]-driven scheduling.
    pub block_fetch_pool: Option<BlockFetchInstrumentation>,
    /// Operator-configured upper bound on concurrent BlockFetch peers.
    ///
    /// Sourced from `NodeConfigFile.max_concurrent_block_fetch_peers`
    /// (`node/src/config.rs:285`).  Defaults to `1` (legacy single-peer
    /// dispatch).  The runtime computes the effective per-tick concurrency
    /// via [`effective_block_fetch_concurrency`] which clamps this knob
    /// against the actual peer slice length, so any value `> 1` parses
    /// safely even when only one session is currently active.
    ///
    /// Closes the audit gap "config knob is read by no production path"
    /// (Slice E in `docs/AUDIT_VERIFICATION_2026Q2.md`).
    ///
    /// Reference: `Ouroboros.Network.BlockFetch.Decision` —
    /// `bfcMaxConcurrencyDeadline = 1`, `bfcMaxConcurrencyBulkSync = 2`.
    pub max_concurrent_block_fetch_peers: u8,
    /// Optional shared per-peer ChainSync header-density registry
    /// (`Slice GD-RT`).  When set, every observed RollForward header
    /// pushes its slot into the peer's `DensityWindow` so the network
    /// governor can read chain-quality density on its next tick.  When
    /// `None`, density observation is a no-op and the runtime keeps
    /// pre-Slice-GD behaviour.
    ///
    /// Reference: `Ouroboros.Consensus.Genesis.Governor` — density
    /// updates per ChainSync `RollForward`.
    pub density_registry: Option<DensityRegistry>,
    /// Optional shared per-peer BlockFetch worker pool reachable
    /// from both the governor (writer: register on promote,
    /// unregister on demote) and the sync loop (reader: dispatch
    /// fetch plans).  When populated and
    /// `effective_block_fetch_concurrency(pool.len()) > 1`, the
    /// sync loop's BlockFetch path goes through the pool's
    /// `dispatch_plan` instead of the direct `block_fetch_mut()`
    /// call — matching upstream
    /// `Ouroboros.Network.BlockFetch.ClientRegistry` semantics.
    ///
    /// Cloned at runtime startup from
    /// [`crate::runtime::new_shared_fetch_worker_pool`] and threaded
    /// into both [`crate::runtime::RuntimeGovernorConfig`] (governor
    /// side) and this config (sync side) so register/unregister
    /// from the governor task is visible to the sync task on the
    /// next read.
    pub shared_fetch_worker_pool: Option<crate::runtime::SharedFetchWorkerPool>,
    /// Round 151 — shared candidate-fragment registry that the
    /// verified-sync loop populates on each `MsgRollForward` and
    /// the BlockFetch dispatcher reads to resolve `split_range`'s
    /// placeholder hashes.  When `Some`, multi-peer dispatch issues
    /// real-hash `MsgRequestRange` plans (the upstream
    /// `Ouroboros.Network.BlockFetch.Decision.fetchDecisions`
    /// analogue); when `None`, the placeholder-collapse fallback
    /// runs so single-chunk dispatch stays correct.  Cloned at
    /// runtime startup from
    /// [`crate::chainsync_worker::new_shared_chainsync_worker_pool`].
    pub shared_chainsync_worker_pool: Option<crate::chainsync_worker::SharedChainSyncWorkerPool>,
}

impl VerifiedSyncServiceConfig {
    /// Effective per-tick BlockFetch concurrency for a peer slice of size
    /// `n_peers`.  Thin wrapper around [`effective_block_fetch_concurrency`]
    /// that callers use as the production-side read of the
    /// `max_concurrent_block_fetch_peers` configuration knob, satisfying
    /// the audit gap "config knob read by no production path" (Slice E).
    pub fn effective_block_fetch_concurrency(&self, n_peers: usize) -> usize {
        effective_block_fetch_concurrency(self.max_concurrent_block_fetch_peers, n_peers)
    }

    /// Build a `CekPlutusEvaluator` from this config's cost model and
    /// time-conversion parameters.
    pub(crate) fn build_plutus_evaluator(&self) -> crate::plutus_eval::CekPlutusEvaluator {
        match (&self.plutus_cost_model, self.system_start_unix_secs) {
            (Some(cm), Some(start)) => {
                crate::plutus_eval::CekPlutusEvaluator::with_time_conversion(
                    cm.clone(),
                    start,
                    self.slot_length_secs.unwrap_or(1.0),
                )
            }
            (Some(cm), None) => crate::plutus_eval::CekPlutusEvaluator::with_cost_model(cm.clone()),
            (None, Some(start)) => crate::plutus_eval::CekPlutusEvaluator {
                system_start_unix_secs: Some(start),
                slot_length_secs: self.slot_length_secs.unwrap_or(1.0),
                ..Default::default()
            },
            (None, None) => crate::plutus_eval::CekPlutusEvaluator::default(),
        }
    }
}

/// Outcome returned when the verified sync service finishes.
///
/// Includes the final `NonceEvolutionState` so the caller can persist or
/// inspect the current epoch nonce, and the `ChainState` for rollback
/// tracking context.
#[derive(Clone, Debug)]
pub struct VerifiedSyncServiceOutcome {
    /// Final chain point when the service stopped.
    pub final_point: Point,
    /// Total blocks fetched across all batches.
    pub total_blocks: usize,
    /// Total rollback events across all batches.
    pub total_rollbacks: usize,
    /// Number of batch iterations completed.
    pub batches_completed: usize,
    /// Final nonce evolution state (present when `nonce_config` was set).
    pub nonce_state: Option<NonceEvolutionState>,
    /// Final chain state (present when `security_param` was set).
    pub chain_state: Option<ChainState>,
    /// Total number of blocks that crossed the stability window during
    /// the service run (eligible for immutable promotion).
    pub stable_block_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct LedgerCheckpointTracking {
    pub(crate) base_ledger_state: LedgerState,
    pub(crate) ledger_state: LedgerState,
    pub(crate) last_persisted_point: Point,
    pub(crate) plutus_evaluator: crate::plutus_eval::CekPlutusEvaluator,
    /// Stake snapshots for epoch boundary processing.  When present,
    /// block application detects epoch transitions and applies the
    /// NEWEPOCH / SNAP / RUPD sequence before the first block of each
    /// new epoch.
    pub(crate) stake_snapshots: Option<StakeSnapshots>,
    /// Epoch schedule (era-aware slots per epoch) for epoch boundary
    /// detection.  Required when `stake_snapshots` is `Some`.
    pub(crate) epoch_size: Option<EpochSchedule>,
    /// Per-pool block production counts for the current epoch.
    ///
    /// At each epoch boundary, these counts are converted to performance
    /// ratios (`blocks_produced / expected_blocks`) and passed to
    /// `apply_epoch_boundary`, then reset for the next epoch.
    pub(crate) pool_block_counts: BTreeMap<PoolKeyHash, u64>,
    /// Storage directory under which the OpCert counter sidecar
    /// (`ocert_counters.cbor`) is persisted whenever a ledger checkpoint
    /// is written. When `None`, no sidecar persistence happens — the
    /// counters remain process-local, matching pre-slice behavior.
    /// Reference: `PraosState.csCounters` in
    /// `Ouroboros.Consensus.Protocol.Praos`.
    pub(crate) ocert_persist_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LedgerCheckpointUpdateOutcome {
    ClearedDisabled,
    ClearedOrigin,
    Persisted {
        slot: SlotNo,
        retained_snapshots: usize,
        pruned_snapshots: usize,
        rollback_count: usize,
    },
    Skipped {
        slot: SlotNo,
        rollback_count: usize,
        since_last_slot_delta: u64,
    },
}

pub(crate) fn for_each_roll_forward_block<E, F>(
    progress: &MultiEraSyncProgress,
    mut f: F,
) -> Result<(), E>
where
    F: FnMut(&MultiEraBlock, &[u8], &yggdrasil_ledger::BlockTxRawSpans) -> Result<(), E>,
{
    let empty_spans = yggdrasil_ledger::BlockTxRawSpans::default();
    for step in &progress.steps {
        if let MultiEraSyncStep::RollForward {
            blocks,
            raw_blocks,
            block_spans,
            ..
        } = step
        {
            // Both `raw_blocks` and `block_spans` are parallel to `blocks`
            // in production; synthetic test progress may pass shorter
            // slices, in which case the missing indices fall back to
            // empty bytes / empty spans and the typed converter
            // re-encodes (see `extract_tx_ids` for the fallback contract).
            for (idx, block) in blocks.iter().enumerate() {
                let raw: &[u8] = raw_blocks.get(idx).map(|v| v.as_slice()).unwrap_or(&[]);
                let spans = block_spans.get(idx).unwrap_or(&empty_spans);
                f(block, raw, spans)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn advance_ledger_state_with_progress(
    ledger_state: &mut LedgerState,
    progress: &MultiEraSyncProgress,
    evaluator: Option<&dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator>,
) -> Result<(), SyncError> {
    for_each_roll_forward_block(progress, |block, _raw, spans| {
        ledger_state.apply_block_validated(
            &multi_era_block_to_block_with_spans(block, spans),
            evaluator,
        )?;
        Ok(())
    })
}

/// Advances the ledger state block-by-block, detecting epoch transitions
/// and applying the NEWEPOCH / SNAP / RUPD boundary rules before the
/// first block of each new epoch.
///
/// Returns the list of epoch boundary events that fired during this batch.
/// When no epoch transition occurs, the returned vec is empty and the
/// behavior is identical to [`advance_ledger_state_with_progress`].
/// Compute per-pool performance ratios from accumulated block counts and
/// the `set` stake snapshot.
///
/// Performance for each pool is `blocks_produced / expected_blocks` where
/// `expected_blocks = σ_pool * total_blocks`. When the snapshot has no
/// stake data (initial sync epochs), returns an empty map which causes
/// `apply_epoch_boundary` to fall back to perfect performance for all pools.
///
/// Reference: `Cardano.Ledger.Shelley.LedgerState` — `completeRupd`.
pub(crate) fn compute_pool_performance(
    pool_block_counts: &BTreeMap<PoolKeyHash, u64>,
    set_snapshot: &yggdrasil_ledger::StakeSnapshot,
    _epoch_size: EpochSize,
) -> BTreeMap<PoolKeyHash, UnitInterval> {
    let stake_dist = set_snapshot.pool_stake_distribution();
    let total_stake = stake_dist.total_active_stake();
    if total_stake == 0 || pool_block_counts.is_empty() {
        return BTreeMap::new();
    }

    let total_blocks: u64 = pool_block_counts.values().sum();
    if total_blocks == 0 {
        return BTreeMap::new();
    }

    let mut performance = BTreeMap::new();
    for (pool_hash, &blocks_produced) in pool_block_counts {
        let pool_stake = stake_dist.pool_stake(pool_hash);
        if pool_stake == 0 {
            // Pool has no stake in the set snapshot — skip (defaults to perfect).
            continue;
        }
        // performance = blocks_produced / (σ * total_blocks)
        //             = blocks_produced * total_stake / (pool_stake * total_blocks)
        let numerator = blocks_produced.saturating_mul(total_stake);
        let denominator = pool_stake.saturating_mul(total_blocks);
        if denominator > 0 {
            performance.insert(
                *pool_hash,
                UnitInterval {
                    numerator,
                    denominator,
                },
            );
        }
    }
    performance
}

/// Optional VRF verification context for epoch-boundary-aware block application.
///
/// When provided, each block's VRF leader eligibility proof is checked
/// against the pool's relative stake using the `set` snapshot.
pub(crate) struct VrfVerificationContext<'a> {
    /// Current epoch nonce from nonce evolution tracking.
    pub nonce_state: &'a NonceEvolutionState,
    /// Active slot coefficient from genesis.
    pub active_slot_coeff: &'a ActiveSlotCoeff,
}

pub(crate) fn advance_ledger_with_epoch_boundary(
    ledger_state: &mut LedgerState,
    snapshots: &mut StakeSnapshots,
    epoch_schedule: EpochSchedule,
    progress: &MultiEraSyncProgress,
    evaluator: Option<&dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator>,
    vrf_ctx: Option<&VrfVerificationContext<'_>>,
    pool_block_counts: &mut BTreeMap<PoolKeyHash, u64>,
) -> Result<Vec<EpochBoundaryEvent>, SyncError> {
    let mut events = Vec::new();
    let shelley_epoch_size = epoch_schedule.shelley_epoch_size();
    for_each_roll_forward_block(progress, |block, _raw, spans| -> Result<(), SyncError> {
        let converted = multi_era_block_to_block_with_spans(block, spans);
        let block_slot = converted.header.slot_no;

        // Detect epoch transition relative to the current ledger tip.
        let prev_slot = match ledger_state.tip {
            Point::BlockPoint(s, _) => Some(s),
            Point::Origin => None,
        };
        if epoch_schedule.is_new_epoch(prev_slot, block_slot) {
            let new_epoch = epoch_schedule.slot_to_epoch(block_slot);
            // Compute pool performance ratios from accumulated block counts.
            // Performance = blocks_produced / expected_blocks where
            // expected_blocks ≈ σ_pool * epoch_size * f.  When the set
            // snapshot has stake data we use it; otherwise fall back to
            // the previous behavior of treating all pools as perfect.
            let pool_performance =
                compute_pool_performance(pool_block_counts, &snapshots.set, shelley_epoch_size);
            apply_epoch_boundary(ledger_state, new_epoch, snapshots, &pool_performance)
                .map(|event| events.push(event))
                .map_err(SyncError::LedgerDecode)?;
            // Reset counts for the new epoch.
            pool_block_counts.clear();
        }

        // Track pool block production.
        if let Some(issuer_vkey) = block_issuer_vkey(block) {
            let pool_hash = yggdrasil_crypto::blake2b::hash_bytes_224(&issuer_vkey).0;
            *pool_block_counts.entry(pool_hash).or_insert(0) += 1;
        }

        // VRF leader eligibility check using the `set` snapshot.
        if let Some(ctx) = vrf_ctx {
            let stake_dist = snapshots.set.pool_stake_distribution();
            if stake_dist.total_active_stake() > 0 {
                let valid = verify_block_vrf_with_stake(
                    block,
                    ctx.nonce_state.epoch_nonce,
                    &stake_dist,
                    ctx.active_slot_coeff,
                )?;
                if !valid {
                    return Err(SyncError::Consensus(ConsensusError::VrfLeaderCheckFailed));
                }
            }
        }

        ledger_state.apply_block_validated(&converted, evaluator)?;

        // Accumulate declared transaction fees into the snapshot fee pot.
        // This feeds `compute_epoch_rewards()` at the next epoch boundary
        // so that rewards reflect actual on-chain fee revenue.
        // Byron fees are implicit and pre-date the Shelley reward system,
        // so `total_transaction_fees()` returns 0 for Byron blocks.
        let block_fees = block.total_transaction_fees();
        if block_fees > 0 {
            snapshots.accumulate_fees(block_fees);
        }
        Ok(())
    })?;
    Ok(events)
}

pub(crate) fn apply_nonce_evolution_to_progress(
    nonce_state: &mut NonceEvolutionState,
    progress: &MultiEraSyncProgress,
    nonce_cfg: &NonceEvolutionConfig,
) {
    let _ = for_each_roll_forward_block(progress, |block, _raw, _spans| {
        apply_nonce_evolution(nonce_state, block, nonce_cfg);
        Ok::<(), core::convert::Infallible>(())
    });
}

pub(crate) fn update_ledger_checkpoint_after_progress<I, V, L>(
    chain_db: &mut ChainDb<I, V, L>,
    tracking: &mut LedgerCheckpointTracking,
    progress: &MultiEraSyncProgress,
    policy: &LedgerCheckpointPolicy,
    vrf_ctx: Option<&VrfVerificationContext<'_>>,
    mut ocert_counters: Option<&mut OcertCounters>,
) -> Result<(LedgerCheckpointUpdateOutcome, Vec<EpochBoundaryEvent>), SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    let mut epoch_events = Vec::new();

    if progress.rollback_count > 0 {
        chain_db.truncate_ledger_checkpoints_after_point(&progress.current_point)?;

        // Round 166 — initial-sync rollback fast path.
        //
        // Every fresh ChainSync session begins with the server confirming the
        // requested intersect by sending `MsgRollBackward` to that point.  When
        // the requested intersect is `Origin`, the resulting batch progress
        // looks like `[RollBackward(Origin), RollForward(blocks)]` and reports
        // `rollback_count = 1` even though the chain has not actually
        // rewound.  Detect this case and skip the heavy
        // `recover_ledger_state_chaindb` call (which replays the volatile
        // suffix via `apply_block` without epoch-boundary detection — leading
        // to `PPUP wrong epoch` failures whenever a single batch crosses a
        // Byron→Shelley or any per-epoch boundary).  Instead, reset to the
        // base state and let the boundary-aware forward path apply the
        // RollForward portion of progress.
        //
        // Reference: `Ouroboros.Network.Protocol.ChainSync.Server` —
        // `RollBackward` confirmation behaviour at session start.
        let initial_sync_rollback_to_origin = matches!(
            progress.steps.iter().find_map(|step| match step {
                MultiEraSyncStep::RollBackward { point, .. } => Some(*point),
                _ => None,
            }),
            Some(Point::Origin)
        ) && tracking.base_ledger_state.tip == Point::Origin;

        if initial_sync_rollback_to_origin {
            tracking.ledger_state = tracking.base_ledger_state.clone();
        } else {
            tracking.ledger_state =
                recover_ledger_state_chaindb(chain_db, tracking.base_ledger_state.clone())?
                    .ledger_state;

            // Round 167 — post-recovery epoch fixup for mid-sync rollback.
            //
            // `recover_ledger_state` replays the volatile/immutable suffix
            // via `LedgerState::apply_block` without firing epoch-boundary
            // processing.  When the rollback target sits in a later epoch
            // than the latest restored checkpoint (e.g. a deep rollback
            // crossing an epoch boundary), `current_epoch` would otherwise
            // remain at the checkpoint's epoch — breaking PPUP validation
            // for any subsequent block whose proposal targets the actual
            // tip's epoch.  Fix this by forcing `current_epoch` to match
            // the recovered tip's slot.  Reward distribution is **not**
            // redone (it already happened during the original live sync,
            // and re-firing `apply_epoch_boundary` here would require
            // reconstructing the historical stake snapshots) — the
            // recovered ledger state stays as it was at the checkpoint
            // for everything except `current_epoch`.
            //
            // Reference: `Cardano.Ledger.Shelley.Rules.NewEpoch` —
            // `current_epoch` is the only field PPUP validation reads.
            if let (Some(epoch_schedule), Point::BlockPoint(slot, _)) =
                (tracking.epoch_size, tracking.ledger_state.tip)
            {
                let actual_epoch = epoch_schedule.slot_to_epoch(slot);
                if actual_epoch.0 > tracking.ledger_state.current_epoch().0 {
                    tracking.ledger_state.set_current_epoch(actual_epoch);
                }
            }
        }
        // After rollback recovery, stake snapshots are stale — reset them
        // so epoch boundary processing restarts cleanly.
        if tracking.stake_snapshots.is_some() {
            tracking.stake_snapshots = Some(StakeSnapshots::new());
        }
        // Pool block counts are epoch-relative and stale after rollback.
        tracking.pool_block_counts.clear();
        // Per-pool OpCert counters are part of upstream `PraosState.csCounters`
        // (rolled back via `ChainDepState` snapshot at the rollback restore
        // point). Reset them here so a fork that legitimately includes
        // lower-sequence OpCerts from the same pool is accepted as
        // "first-seen" via `OcertCounters::validate_and_update`'s permissive
        // initialisation rule, instead of being rejected as
        // `OcertCounterTooOld` against a stale pre-rollback high-water mark.
        // The persisted sidecar will be overwritten with the post-reset
        // map at the next checkpoint persistence below.
        if let Some(counters) = ocert_counters.as_mut() {
            counters.clear();
        }

        // For initial-sync rollback to Origin, the heavy recovery was
        // skipped — apply the forward portion of progress now via the
        // boundary-aware path so PPUP / NEWEPOCH transitions fire correctly.
        if initial_sync_rollback_to_origin {
            if let (Some(snapshots), Some(epoch_size)) =
                (tracking.stake_snapshots.as_mut(), tracking.epoch_size)
            {
                epoch_events = advance_ledger_with_epoch_boundary(
                    &mut tracking.ledger_state,
                    snapshots,
                    epoch_size,
                    progress,
                    Some(&tracking.plutus_evaluator),
                    vrf_ctx,
                    &mut tracking.pool_block_counts,
                )?;
            } else {
                advance_ledger_state_with_progress(
                    &mut tracking.ledger_state,
                    progress,
                    Some(&tracking.plutus_evaluator),
                )?;
            }
        }
    } else if let (Some(snapshots), Some(epoch_size)) =
        (tracking.stake_snapshots.as_mut(), tracking.epoch_size)
    {
        epoch_events = advance_ledger_with_epoch_boundary(
            &mut tracking.ledger_state,
            snapshots,
            epoch_size,
            progress,
            Some(&tracking.plutus_evaluator),
            vrf_ctx,
            &mut tracking.pool_block_counts,
        )?;
    } else {
        advance_ledger_state_with_progress(
            &mut tracking.ledger_state,
            progress,
            Some(&tracking.plutus_evaluator),
        )?;
    }

    if policy.max_snapshots == 0 {
        chain_db.clear_ledger_checkpoints()?;
        tracking.last_persisted_point = Point::Origin;
        return Ok((LedgerCheckpointUpdateOutcome::ClearedDisabled, epoch_events));
    }

    let current_point = tracking.ledger_state.tip;
    match current_point {
        Point::Origin => {
            chain_db.clear_ledger_checkpoints()?;
            tracking.last_persisted_point = Point::Origin;
            Ok((LedgerCheckpointUpdateOutcome::ClearedOrigin, epoch_events))
        }
        Point::BlockPoint(slot, _) => {
            if policy.should_persist(
                &tracking.last_persisted_point,
                &current_point,
                progress.rollback_count > 0,
            ) {
                let retention = chain_db.persist_ledger_checkpoint(
                    &current_point,
                    &tracking.ledger_state.checkpoint(),
                    policy.max_snapshots,
                )?;
                // Persist the OpCert counter sidecar atomically alongside
                // the ledger checkpoint. Mirrors `PraosState.csCounters`
                // durability in upstream `Ouroboros.Consensus.Protocol.Praos`,
                // which is part of the persistent `ChainDepState`. Without
                // this, a restart resets per-pool monotonicity high-water
                // marks to zero and a peer can replay an old block whose
                // OpCert sequence number is below the true on-chain value.
                if let (Some(dir), Some(counters)) = (
                    tracking.ocert_persist_dir.as_ref(),
                    ocert_counters.as_deref(),
                ) {
                    let encoded = counters.to_cbor_bytes();
                    yggdrasil_storage::save_ocert_counters(dir, &encoded)
                        .map_err(SyncError::Storage)?;
                }
                tracking.last_persisted_point = current_point;
                Ok((
                    LedgerCheckpointUpdateOutcome::Persisted {
                        slot,
                        retained_snapshots: retention.retained_snapshots,
                        pruned_snapshots: retention.pruned_snapshots,
                        rollback_count: progress.rollback_count,
                    },
                    epoch_events,
                ))
            } else {
                let since_last_slot_delta = match tracking.last_persisted_point {
                    Point::BlockPoint(previous_slot, _) => slot.0.saturating_sub(previous_slot.0),
                    Point::Origin => slot.0,
                };
                Ok((
                    LedgerCheckpointUpdateOutcome::Skipped {
                        slot,
                        rollback_count: progress.rollback_count,
                        since_last_slot_delta,
                    },
                    epoch_events,
                ))
            }
        }
    }
}

pub(crate) fn default_checkpoint_tracking<I, V, L>(
    chain_db: &ChainDb<I, V, L>,
    base_ledger_state: LedgerState,
    config: &VerifiedSyncServiceConfig,
) -> Result<LedgerCheckpointTracking, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    let recovery = recover_ledger_state_chaindb(chain_db, base_ledger_state)?;
    Ok(LedgerCheckpointTracking {
        base_ledger_state: recovery.ledger_state.clone(),
        ledger_state: recovery.ledger_state,
        last_persisted_point: recovery.point,
        plutus_evaluator: config.build_plutus_evaluator(),
        stake_snapshots: None,
        epoch_size: None,
        pool_block_counts: BTreeMap::new(),
        ocert_persist_dir: None,
    })
}

/// Restore a ledger state from the latest typed ChainDb checkpoint and replay
/// any remaining volatile suffix.
///
/// This helper restores from the latest available typed checkpoint, then
/// replays immutable blocks after that checkpoint followed by the remaining
/// volatile suffix.
pub fn recover_ledger_state_chaindb<I, V, L>(
    chain_db: &ChainDb<I, V, L>,
    base_state: LedgerState,
) -> Result<LedgerRecoveryOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    chain_db
        .recover_ledger_state(base_state)
        .map_err(|error| match error {
            StorageError::Recovery(message) => SyncError::Recovery(message),
            other => SyncError::Storage(other),
        })
}
/// Run a continuous verified sync loop with multi-era block decoding,
/// header/body verification, and optional epoch nonce tracking.
///
/// The service:
/// 1. Calls [`sync_batch_apply_verified`] per iteration with full
///    header + body-hash verification.
/// 2. After each batch, applies [`apply_nonce_evolution`] to every
///    roll-forward block (when nonce tracking is enabled).
/// 3. Loops until `shutdown` resolves or a protocol error occurs.
/// 4. Returns [`VerifiedSyncServiceOutcome`] including the final nonce
///    state.
///
/// ## Nonce evolution and rollbacks
///
/// During initial chain sync, rollbacks are rare and typically shallow.
/// Nonce evolution is forward-only — a rollback does **not** revert the
/// nonce state.  This is safe for initial sync but will need epoch-boundary
/// checkpointing when handling live chain forks.
///
/// # Errors
///
/// Returns `SyncError` on protocol, decode, verification, or storage
/// errors.  Shutdown-triggered termination returns `Ok`.
pub async fn run_verified_sync_service<S, F>(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    store: &mut S,
    mut from_point: Point,
    config: &VerifiedSyncServiceConfig,
    mut nonce_state: Option<NonceEvolutionState>,
    shutdown: F,
) -> Result<VerifiedSyncServiceOutcome, SyncError>
where
    S: VolatileStore,
    F: std::future::Future<Output = ()>,
{
    tokio::pin!(shutdown);

    let mut total_blocks = 0usize;
    let mut total_rollbacks = 0usize;
    let mut batches_completed = 0usize;
    let mut total_stable = 0usize;
    let mut chain_state = config
        .security_param
        .map(|k| seed_chain_state_from_volatile(store, k));
    let mut ocert_counters = config.verification.ocert_counters.clone();

    loop {
        let batch_fut = sync_batch_apply_verified(
            chain_sync,
            block_fetch,
            store,
            from_point,
            config.batch_size,
            Some(&config.verification),
            &mut ocert_counters,
            None,
        );

        tokio::select! {
            biased;

            () = &mut shutdown => {
                return Ok(VerifiedSyncServiceOutcome {
                    final_point: from_point,
                    total_blocks,
                    total_rollbacks,
                    batches_completed,
                    nonce_state,
                    chain_state,
                    stable_block_count: total_stable,
                });
            }

            result = batch_fut => {
                let progress = result?;
                from_point = progress.current_point;
                total_blocks += progress.fetched_blocks;
                total_rollbacks += progress.rollback_count;
                batches_completed += 1;

                // Track chain topology in ChainState.
                if let Some(ref mut cs) = chain_state {
                    for step in &progress.steps {
                        total_stable += track_chain_state(cs, step)?;
                    }
                }

                // Apply nonce evolution to all roll-forward blocks.
                if let Some((ref mut state, nonce_cfg)) =
                    nonce_state.as_mut().zip(config.nonce_config.as_ref())
                {
                    apply_nonce_evolution_to_progress(state, &progress, nonce_cfg);
                }
            }
        }
    }
}

/// Run a continuous verified sync loop while coordinating storage through
/// [`ChainDb`].
///
/// This variant mirrors [`run_verified_sync_service`] but promotes stable
/// volatile prefixes into immutable storage as soon as the tracked
/// [`ChainState`] drains them.
#[allow(clippy::too_many_arguments)]
pub async fn run_verified_sync_service_chaindb<I, V, L, F>(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    chain_db: &mut ChainDb<I, V, L>,
    base_ledger_state: LedgerState,
    mut from_point: Point,
    config: &VerifiedSyncServiceConfig,
    mut nonce_state: Option<NonceEvolutionState>,
    shutdown: F,
) -> Result<VerifiedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: std::future::Future<Output = ()>,
{
    tokio::pin!(shutdown);

    let mut total_blocks = 0usize;
    let mut total_rollbacks = 0usize;
    let mut batches_completed = 0usize;
    let mut total_stable = 0usize;
    let mut chain_state = config
        .security_param
        .map(|k| seed_chain_state_from_volatile(chain_db.volatile(), k));
    let mut ocert_counters = config.verification.ocert_counters.clone();
    let mut checkpoint_tracking = default_checkpoint_tracking(chain_db, base_ledger_state, config)?;

    // Enable epoch boundary processing when nonce config provides epoch size.
    if let Some(ref nonce_cfg) = config.nonce_config {
        checkpoint_tracking.stake_snapshots = Some(StakeSnapshots::new());
        checkpoint_tracking.epoch_size = Some(
            config
                .epoch_schedule
                .unwrap_or_else(|| EpochSchedule::fixed(nonce_cfg.epoch_size)),
        );
    }

    loop {
        let batch_fut = sync_batch_verified(
            chain_sync,
            block_fetch,
            from_point,
            config.batch_size,
            Some(&config.verification),
            &mut ocert_counters,
            None,
        );

        tokio::select! {
            biased;

            () = &mut shutdown => {
                return Ok(VerifiedSyncServiceOutcome {
                    final_point: from_point,
                    total_blocks,
                    total_rollbacks,
                    batches_completed,
                    nonce_state,
                    chain_state,
                    stable_block_count: total_stable,
                });
            }

            result = batch_fut => {
                let progress = result?;

                // Build VRF context when verification is enabled and nonce
                // tracking is active.  The nonce state must be read before
                // this batch's nonce evolution update.
                let vrf_ctx = if config.verify_vrf {
                    nonce_state.as_ref().zip(config.active_slot_coeff.as_ref()).map(
                        |(ns, asc)| VrfVerificationContext {
                            nonce_state: ns,
                            active_slot_coeff: asc,
                        },
                    )
                } else {
                    None
                };

                let applied = apply_verified_progress_to_chaindb(
                    chain_db,
                    &progress,
                    chain_state.as_mut(),
                    Some(&mut checkpoint_tracking),
                    &config.checkpoint_policy,
                    vrf_ctx.as_ref(),
                    ocert_counters.as_mut(),
                )?;
                from_point = progress.current_point;
                total_blocks += progress.fetched_blocks;
                total_rollbacks += progress.rollback_count;
                batches_completed += 1;
                total_stable += applied.stable_block_count;

                if let Some((ref mut state, nonce_cfg)) =
                    nonce_state.as_mut().zip(config.nonce_config.as_ref())
                {
                    apply_nonce_evolution_to_progress(state, &progress, nonce_cfg);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 34: Consensus header verification bridge
// ---------------------------------------------------------------------------

/// The KES depth used for Shelley-era block header signatures.
///
/// `SumKES_6` yields a 448-byte signature = 64 + 6*64.
/// Reference: `MaxKESEvo = 62` and `SumKES_6` in Shelley genesis.
pub const SHELLEY_KES_DEPTH: u32 = 6;

/// Convert a ledger `ShelleyOpCert` into a consensus `OpCert` for
/// verification.
pub fn shelley_opcert_to_consensus(opcert: &ShelleyOpCert) -> ConsensusOpCert {
    ConsensusOpCert {
        hot_vkey: SumKesVerificationKey::from_bytes(opcert.hot_vkey),
        sequence_number: opcert.sequence_number,
        kes_period: opcert.kes_period,
        sigma: Ed25519Signature::from_bytes(opcert.sigma),
    }
}

/// Convert a ledger `ShelleyHeaderBody` into a consensus `HeaderBody` for
/// verification.
pub fn shelley_header_body_to_consensus(body: &ShelleyHeaderBody) -> ConsensusHeaderBody {
    ConsensusHeaderBody {
        block_number: BlockNo(body.block_number),
        slot: SlotNo(body.slot),
        prev_hash: body.prev_hash.map(HeaderHash),
        issuer_vkey: VerificationKey::from_bytes(body.issuer_vkey),
        vrf_vkey: VrfVerificationKey::from_bytes(body.vrf_vkey),
        leader_vrf_output: body.leader_vrf.output.clone(),
        leader_vrf_proof: body.leader_vrf.proof,
        nonce_vrf_output: Some(body.nonce_vrf.output.clone()),
        nonce_vrf_proof: Some(body.nonce_vrf.proof),
        block_body_size: body.block_body_size,
        block_body_hash: body.block_body_hash,
        operational_cert: shelley_opcert_to_consensus(&body.operational_cert),
        protocol_version: body.protocol_version,
    }
}

/// Convert a ledger `ShelleyHeader` into a consensus `Header` for
/// cryptographic verification.
///
/// # Errors
///
/// Returns `SyncError::LedgerDecode` if the KES signature bytes cannot be
/// parsed at `SHELLEY_KES_DEPTH`.
pub fn shelley_header_to_consensus(header: &ShelleyHeader) -> Result<ConsensusHeader, SyncError> {
    let kes_sig =
        SumKesSignature::from_bytes(SHELLEY_KES_DEPTH, &header.signature).map_err(|_| {
            SyncError::LedgerDecode(LedgerError::CborInvalidLength {
                expected: SumKesSignature::expected_size(SHELLEY_KES_DEPTH),
                actual: header.signature.len(),
            })
        })?;

    Ok(ConsensusHeader {
        body: shelley_header_body_to_consensus(&header.body),
        kes_signature: kes_sig,
    })
}

/// Verify a Shelley block header using the consensus verification pipeline.
///
/// Converts the ledger-typed header into consensus types and runs
/// `verify_header` which checks:
/// 1. OpCert cold-key signature
/// 2. KES period validity window
/// 3. KES signature over the header body
///
/// # Parameters
///
/// * `header` — the decoded Shelley header to verify.
/// * `slots_per_kes_period` — Shelley genesis parameter (mainnet: 129600).
/// * `max_kes_evolutions` — maximum KES evolutions (mainnet: 62).
pub fn verify_shelley_header(
    header: &ShelleyHeader,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<(), SyncError> {
    let consensus_hdr = shelley_header_to_consensus(header)?;
    verify_header(&consensus_hdr, slots_per_kes_period, max_kes_evolutions)?;
    Ok(())
}

/// Convert a ledger `PraosHeaderBody` into a consensus `HeaderBody` for
/// verification.
pub fn praos_header_body_to_consensus(body: &PraosHeaderBody) -> ConsensusHeaderBody {
    ConsensusHeaderBody {
        block_number: BlockNo(body.block_number),
        slot: SlotNo(body.slot),
        prev_hash: body.prev_hash.map(HeaderHash),
        issuer_vkey: VerificationKey::from_bytes(body.issuer_vkey),
        vrf_vkey: VrfVerificationKey::from_bytes(body.vrf_vkey),
        leader_vrf_output: body.vrf_result.output.clone(),
        leader_vrf_proof: body.vrf_result.proof,
        nonce_vrf_output: None,
        nonce_vrf_proof: None,
        block_body_size: body.block_body_size,
        block_body_hash: body.block_body_hash,
        operational_cert: shelley_opcert_to_consensus(&body.operational_cert),
        protocol_version: body.protocol_version,
    }
}

/// Convert a ledger `PraosHeader` into a consensus `Header` for
/// cryptographic verification.
///
/// # Errors
///
/// Returns `SyncError::LedgerDecode` if the KES signature bytes cannot be
/// parsed at `SHELLEY_KES_DEPTH`.
pub fn praos_header_to_consensus(header: &PraosHeader) -> Result<ConsensusHeader, SyncError> {
    let kes_sig =
        SumKesSignature::from_bytes(SHELLEY_KES_DEPTH, &header.signature).map_err(|_| {
            SyncError::LedgerDecode(LedgerError::CborInvalidLength {
                expected: SumKesSignature::expected_size(SHELLEY_KES_DEPTH),
                actual: header.signature.len(),
            })
        })?;

    Ok(ConsensusHeader {
        body: praos_header_body_to_consensus(&header.body),
        kes_signature: kes_sig,
    })
}

/// Verify a Praos-era block header (Babbage/Conway) using the consensus
/// verification pipeline.
///
/// Identical verification logic to `verify_shelley_header` but operates on
/// the Praos header format (14-element body with single `vrf_result`).
pub fn verify_praos_header(
    header: &PraosHeader,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<(), SyncError> {
    let consensus_hdr = praos_header_to_consensus(header)?;
    verify_header(&consensus_hdr, slots_per_kes_period, max_kes_evolutions)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 35: Multi-era block decode
// ---------------------------------------------------------------------------

/// A decoded block from any supported era.
///
/// Each Shelley-family era has its own variant carrying a typed block with
/// era-appropriate transaction body types. Byron blocks pass through as
/// opaque bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultiEraBlock {
    /// A decoded Byron-era block carrying header-level metadata
    /// extracted from the raw CBOR envelope.
    Byron {
        /// Structurally decoded Byron block with header metadata and
        /// raw header annotation for hash computation.
        block: ByronBlock,
        /// The era tag from the outer wire envelope (0 = EBB, 1 = main).
        era_tag: u64,
    },
    /// A fully decoded Shelley-era block (also covers Allegra/Mary
    /// which share the Shelley 4-element block envelope and tx body format).
    Shelley(Box<ShelleyBlock>),
    /// A fully decoded Alonzo-era block with `AlonzoTxBody` entries.
    /// Alonzo introduced the 5-element block format (adding
    /// `invalid_transactions`) while keeping the Shelley 15-element
    /// header body (TPraos, two VRF certs).
    Alonzo(Box<AlonzoBlock>),
    /// A fully decoded Babbage-era block with `BabbageTxBody` entries.
    Babbage(Box<BabbageBlock>),
    /// A fully decoded Conway-era block with `ConwayTxBody` entries.
    Conway(Box<ConwayBlock>),
}

impl MultiEraBlock {
    /// Return the slot number of this block.
    ///
    /// Byron blocks compute the absolute slot from
    /// epoch_times_slots_per_epoch_plus_slot_in_epoch using the Byron
    /// constant (21600 slots/epoch).
    /// Shelley through Conway blocks extract it from the typed header body.
    pub fn slot(&self) -> SlotNo {
        match self {
            Self::Byron { block, .. } => SlotNo(block.absolute_slot(BYRON_SLOTS_PER_EPOCH)),
            Self::Shelley(b) => SlotNo(b.header.body.slot),
            Self::Alonzo(b) => SlotNo(b.header.body.slot),
            Self::Babbage(b) => SlotNo(b.header.body.slot),
            Self::Conway(b) => SlotNo(b.header.body.slot),
        }
    }

    /// Sum the declared transaction fees across all transactions in this block.
    ///
    /// Byron blocks return 0 because their fees are implicit (input sum
    /// minus output sum) and pre-date the Shelley reward system.  For
    /// Shelley through Conway every transaction body carries an explicit
    /// `fee` field that is summed here.
    pub fn total_transaction_fees(&self) -> u64 {
        match self {
            Self::Byron { .. } => 0,
            Self::Shelley(b) => b.transaction_bodies.iter().map(|tx| tx.fee).sum(),
            Self::Alonzo(b) => b.transaction_bodies.iter().map(|tx| tx.fee).sum(),
            Self::Babbage(b) => b.transaction_bodies.iter().map(|tx| tx.fee).sum(),
            Self::Conway(b) => b.transaction_bodies.iter().map(|tx| tx.fee).sum(),
        }
    }

    /// Return the `block_body_size` declared in the header.
    ///
    /// Byron blocks do not carry a declared body size in the same way as
    /// Shelley-family blocks, so they return `None`.
    pub fn declared_body_size(&self) -> Option<u32> {
        match self {
            Self::Byron { .. } => None,
            Self::Shelley(b) => Some(b.header.body.block_body_size),
            Self::Alonzo(b) => Some(b.header.body.block_body_size),
            Self::Babbage(b) => Some(b.header.body.block_body_size),
            Self::Conway(b) => Some(b.header.body.block_body_size),
        }
    }

    /// Return the `protocol_version` `(major, minor)` from the header body.
    ///
    /// Byron blocks do not carry an in-header protocol version.
    pub fn protocol_version(&self) -> Option<(u64, u64)> {
        match self {
            Self::Byron { .. } => None,
            Self::Shelley(b) => Some(b.header.body.protocol_version),
            Self::Alonzo(b) => Some(b.header.body.protocol_version),
            Self::Babbage(b) => Some(b.header.body.protocol_version),
            Self::Conway(b) => Some(b.header.body.protocol_version),
        }
    }

    /// Return the era of this block as the ledger `Era` enum value.
    pub fn era(&self) -> Era {
        match self {
            Self::Byron { .. } => Era::Byron,
            Self::Shelley(b) => {
                // Shelley/Allegra/Mary share the ShelleyBlock structure.
                // Distinguish by protocol version: Shelley=(2,x), Allegra=(3,x), Mary=(4,x).
                match b.header.body.protocol_version.0 {
                    3 => Era::Allegra,
                    4 => Era::Mary,
                    _ => Era::Shelley,
                }
            }
            Self::Alonzo(_) => Era::Alonzo,
            Self::Babbage(_) => Era::Babbage,
            Self::Conway(_) => Era::Conway,
        }
    }
}

/// Cardano mainnet era tags used in the multi-era block envelope.
///
/// On the wire, a multi-era block is encoded as `[era_tag, block_body]`
/// where `era_tag` is a small integer.
///
/// Reference: `CardanoBlock` in `Ouroboros.Consensus.Cardano.Block`.
#[allow(dead_code)]
mod era_tag {
    pub const BYRON_EBB: u64 = 0;
    pub const BYRON_MAIN: u64 = 1;
    pub const SHELLEY: u64 = 2;
    pub const ALLEGRA: u64 = 3;
    pub const MARY: u64 = 4;
    pub const ALONZO: u64 = 5;
    pub const BABBAGE: u64 = 6;
    pub const CONWAY: u64 = 7;
}

/// Attempt to decode a raw block payload into a `MultiEraBlock`.
///
/// The block is expected to be CBOR-encoded in the Cardano multi-era
/// envelope format: `[era_tag, block_body]`. Byron blocks (tags 0–1) are
/// kept as opaque bytes. Shelley/Allegra/Mary (tags 2–4) use the 4-element
/// Shelley block codec. Alonzo (tag 5) uses the 5-element Alonzo block
/// codec. Babbage (tag 6) and Conway (tag 7) use their own 5-element
/// block codecs with era-appropriate transaction body types.
fn decode_multi_era_block_ledger(raw: &[u8]) -> Result<MultiEraBlock, LedgerError> {
    fn decode_impl(raw: &[u8]) -> Result<MultiEraBlock, LedgerError> {
        // Peek at the structure: expect a 2-element array [tag, body].
        use yggdrasil_ledger::cbor::Decoder;
        let mut dec = Decoder::new(raw);
        let arr_len = dec.array_begin()?;
        if let Some(len) = arr_len {
            if len != 2 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 2,
                    actual: len as usize,
                });
            }
        }

        let tag = dec.unsigned()?;

        let decoded = match tag {
            era_tag::BYRON_EBB | era_tag::BYRON_MAIN => {
                let body_start = dec.position();
                dec.skip()?;
                let body_bytes = &raw[body_start..dec.position()];
                let byron = if tag == era_tag::BYRON_EBB {
                    ByronBlock::decode_ebb(body_bytes)?
                } else {
                    ByronBlock::decode_main(body_bytes)?
                };
                Ok(MultiEraBlock::Byron {
                    block: byron,
                    era_tag: tag,
                })
            }
            era_tag::SHELLEY | era_tag::ALLEGRA | era_tag::MARY => {
                // Shelley/Allegra/Mary blocks are 4-element CBOR arrays.
                let body_start = dec.position();
                dec.skip()?;
                let body_bytes = &raw[body_start..dec.position()];
                let block = ShelleyBlock::from_cbor_bytes(body_bytes)?;
                Ok(MultiEraBlock::Shelley(Box::new(block)))
            }
            era_tag::ALONZO => {
                // Alonzo blocks are 5-element CBOR arrays (added invalid_transactions).
                let body_start = dec.position();
                dec.skip()?;
                let body_bytes = &raw[body_start..dec.position()];
                let block = AlonzoBlock::from_cbor_bytes(body_bytes)?;
                Ok(MultiEraBlock::Alonzo(Box::new(block)))
            }
            era_tag::BABBAGE => {
                let body_start = dec.position();
                dec.skip()?;
                let body_bytes = &raw[body_start..dec.position()];
                let block = BabbageBlock::from_cbor_bytes(body_bytes)?;
                Ok(MultiEraBlock::Babbage(Box::new(block)))
            }
            era_tag::CONWAY => {
                let body_start = dec.position();
                dec.skip()?;
                let body_bytes = &raw[body_start..dec.position()];
                let block = ConwayBlock::from_cbor_bytes(body_bytes)?;
                Ok(MultiEraBlock::Conway(Box::new(block)))
            }
            unsupported => {
                Err(LedgerError::CborTypeMismatch {
                    expected: 2, // Shelley era tag
                    actual: unsupported as u8,
                })
            }
        };

        if arr_len.is_none() {
            dec.consume_break()?;
        }

        decoded
    }

    match decode_impl(raw) {
        Ok(block) => Ok(block),
        Err(err) => {
            if sync_debug_enabled() {
                let era_tag = {
                    let mut d = yggdrasil_ledger::cbor::Decoder::new(raw);
                    d.array_begin().and_then(|_| d.unsigned()).ok()
                };
                let preview_len = raw.len().min(64);
                let preview = bytes_to_hex(&raw[..preview_len]);
                let full_hex = if raw.len() <= 4096 {
                    Some(bytes_to_hex(raw))
                } else {
                    let _ = fs::create_dir_all("tmp");
                    let _ = fs::write("tmp/last-decode-fail.cbor", raw);
                    let _ = fs::write("tmp/last-decode-fail.hex", bytes_to_hex(raw));
                    None
                };
                eprintln!(
                    "[ygg-sync-debug] decode-multi-era-failed err={err} era_tag={:?} raw_len={} raw_preview={} raw_hex={}",
                    era_tag,
                    raw.len(),
                    preview,
                    full_hex.unwrap_or_else(|| "<omitted>".to_string())
                );
            }
            Err(err)
        }
    }
}

pub fn decode_multi_era_block(raw: &[u8]) -> Result<MultiEraBlock, SyncError> {
    decode_multi_era_block_ledger(raw).map_err(SyncError::LedgerDecode)
}

/// Decode a list of raw block payloads into multi-era blocks.
///
/// Each block is individually decoded using `decode_multi_era_block`.
pub fn decode_multi_era_blocks(raw_blocks: &[Vec<u8>]) -> Result<Vec<MultiEraBlock>, SyncError> {
    raw_blocks
        .iter()
        .map(|raw| decode_multi_era_block(raw))
        .collect()
}

// ---------------------------------------------------------------------------
// Phase 37: Verified multi-era sync pipeline
// ---------------------------------------------------------------------------

/// Convert a `MultiEraBlock` into the generic ledger `Block` wrapper.
///
/// `raw_block_bytes` is the original on-wire CBOR encoding (as received
/// from BlockFetch).  The Shelley-family converters use it to capture
/// each transaction's exact on-wire byte span for fee validation and
/// tx-id computation; see [`shelley_block_to_block`] for the rationale.
/// Byron blocks ignore the parameter because Byron block decode already
/// preserves per-tx raw byte spans internally.
///
/// All Shelley-family eras (Shelley/Allegra/Mary/Alonzo, Babbage, Conway)
/// are fully decoded using the common block envelope. Byron blocks
/// populate real header fields from structural decode:
/// - `hash`: `Blake2b-256(prefix ++ raw_header_cbor)`
/// - `prev_hash`: from Byron consensus data
/// - `slot_no`: absolute slot via `epoch * 21600 + slot_in_epoch`
/// - `block_no`: `chain_difficulty` from consensus data
/// - `issuer_vkey`: PBFT issuer key from consensus data (MainBlock) or
///   zeroed (EBB)
/// - `transactions`: decoded from block body tx_payload
pub fn multi_era_block_to_block(block: &MultiEraBlock, raw_block_bytes: &[u8]) -> Block {
    let spans = match block {
        MultiEraBlock::Byron { .. } => yggdrasil_ledger::BlockTxRawSpans::default(),
        _ => yggdrasil_ledger::extract_block_tx_byte_spans(raw_block_bytes).unwrap_or_default(),
    };
    multi_era_block_to_block_with_spans(block, &spans)
}

/// Variant of [`multi_era_block_to_block`] that consumes pre-extracted
/// `BlockTxRawSpans` instead of re-walking the block CBOR.
///
/// On the sync hot path, the dispatcher caches one `BlockTxRawSpans` per
/// block at step construction (`extract_spans_per_block`), and both the
/// eviction path (`extract_tx_ids`) and the apply path
/// (`apply_multi_era_step_to_volatile`) read from that cache via this
/// entry point.  The Byron arm ignores `spans` (Byron tx-id derivation
/// runs over per-tx `ByronTxAux::raw_tx_cbor`, not block-level spans).
pub fn multi_era_block_to_block_with_spans(
    block: &MultiEraBlock,
    spans: &yggdrasil_ledger::BlockTxRawSpans,
) -> Block {
    match block {
        MultiEraBlock::Shelley(shelley) => shelley_block_to_block_with_spans(shelley, spans),
        MultiEraBlock::Alonzo(alonzo) => alonzo_block_to_block_with_spans(alonzo, spans),
        MultiEraBlock::Babbage(babbage) => babbage_block_to_block_with_spans(babbage, spans),
        MultiEraBlock::Conway(conway) => conway_block_to_block_with_spans(conway, spans),
        MultiEraBlock::Byron { block: byron, .. } => {
            let transactions: Vec<Tx> = byron
                .transactions()
                .iter()
                .map(|tx_aux| {
                    // Byron tx_id MUST be computed over the on-wire CBOR bytes
                    // captured during decoding — re-encoding from the decoded
                    // structure can produce different bytes (definite vs
                    // indefinite arrays, attributes ordering) and yield a
                    // wrong tx_id which would later cause every spend of that
                    // output to fail with InputNotFound.
                    let raw = tx_aux.raw_tx_cbor.clone();
                    let id = compute_tx_id(&raw);
                    Tx {
                        id,
                        body: raw,
                        witnesses: None,
                        auxiliary_data: None,
                        is_valid: None,
                    }
                })
                .collect();
            Block {
                era: Era::Byron,
                header: BlockHeader {
                    hash: byron.header_hash(),
                    prev_hash: HeaderHash(*byron.prev_hash()),
                    slot_no: SlotNo(byron.absolute_slot(BYRON_SLOTS_PER_EPOCH)),
                    block_no: BlockNo(byron.chain_difficulty()),
                    issuer_vkey: byron.issuer_vkey(),
                    protocol_version: None,
                },
                transactions,
                raw_cbor: None,
                header_cbor_size: None, // Byron headers not checked
            }
        }
    }
}

/// Convert a typed Alonzo block into the generic ledger `Block` wrapper.
///
/// See [`shelley_block_to_block`] for the rationale on `raw_block_bytes`.
pub fn alonzo_block_to_block(block: &AlonzoBlock, raw_block_bytes: &[u8]) -> Block {
    let spans = yggdrasil_ledger::extract_block_tx_byte_spans(raw_block_bytes).unwrap_or_default();
    alonzo_block_to_block_with_spans(block, &spans)
}

/// Generate a `*_block_to_block_with_spans` function for an Alonzo-family
/// era (Alonzo / Babbage / Conway).  These three eras share the same wire
/// shape (5-element block, `invalid_transactions` array, `auxiliary_data_set`
/// map, per-tx `is_valid` flag); only the era tag and the typed block
/// struct differ.  See [`shelley_block_to_block_with_spans`] for the
/// Shelley-only variant (different metadata-set field, no `is_valid`).
macro_rules! alonzo_family_block_to_block_with_spans {
    ($vis:vis fn $name:ident, $block_ty:ty, $era:expr) => {
        $vis fn $name(
            block: &$block_ty,
            spans: &yggdrasil_ledger::BlockTxRawSpans,
        ) -> Block {
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
                    let valid = !block.invalid_transactions.contains(&(idx as u64));
                    Tx {
                        id: compute_tx_id(&raw_body),
                        body: raw_body,
                        witnesses: raw_witnesses,
                        auxiliary_data: block.auxiliary_data_set.get(&(idx as u64)).cloned(),
                        is_valid: Some(valid),
                    }
                })
                .collect();

            Block {
                era: $era,
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
    };
}

alonzo_family_block_to_block_with_spans!(
    pub fn alonzo_block_to_block_with_spans, AlonzoBlock, Era::Alonzo
);
alonzo_family_block_to_block_with_spans!(
    fn babbage_block_to_block_with_spans, BabbageBlock, Era::Babbage
);
alonzo_family_block_to_block_with_spans!(
    fn conway_block_to_block_with_spans, ConwayBlock, Era::Conway
);

/// Verification parameters for Shelley-family header validation.
///
/// These correspond to Shelley genesis parameters and are used by
/// `verify_multi_era_block` and the verified sync pipeline.
///
/// Reference: `shelleyGenesisConfig` in `cardano-node` configuration.
#[derive(Clone, Debug)]
pub struct VerificationConfig {
    /// Number of slots per KES period (mainnet: 129600).
    pub slots_per_kes_period: u64,
    /// Maximum number of KES evolutions (mainnet: 62).
    pub max_kes_evolutions: u64,
    /// Whether to verify the block body hash against the header.
    pub verify_body_hash: bool,
    /// Maximum major protocol version the node can understand.
    ///
    /// Blocks whose header protocol version major exceeds this value
    /// are rejected outright — preventing the node from attempting
    /// to validate blocks from a future hard fork it does not support.
    ///
    /// Upstream default for Conway-era nodes: 10.
    ///
    /// Reference: `MaxMajorProtVer` in
    /// `Ouroboros.Consensus.Shelley.Ledger.Block`.
    pub max_major_protocol_version: Option<u64>,
    /// Optional future-block check.  When `Some`, decoded blocks are
    /// compared against the given current wall-clock slot and rejected
    /// if they exceed `clock_skew` tolerance.
    ///
    /// Reference: `InFutureCheck.realHeaderInFutureCheck` in
    /// `ouroboros-consensus`.
    pub future_check: Option<FutureBlockCheckConfig>,
    /// Optional operational-certificate counter tracker.  When `Some`,
    /// each block's OpCert `sequence_number` is validated against the
    /// per-pool monotonic counter state before acceptance.
    ///
    /// The stake distribution must be threaded alongside so that first-
    /// seen pools can be recognized (upstream `currentIssueNo` fallthrough).
    ///
    /// Reference: `PraosState.csCounters` in
    /// `Ouroboros.Consensus.Protocol.Praos`.
    pub ocert_counters: Option<OcertCounters>,
    /// Current protocol-parameter major version from the live ledger state.
    ///
    /// When present, block headers are rejected if their major version
    /// exceeds `pp_major_protocol_version + 1` (Conway BBODY rule
    /// `HeaderProtVerTooHigh`).
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Bbody` —
    /// `pvMajor(bhprotver hdr) > succVersion(pvMajor pp)`.
    pub pp_major_protocol_version: Option<u64>,
}

/// Configuration for the blocks-from-the-future check.
///
/// Reference: `Ouroboros.Consensus.MiniProtocol.ChainSync.Client.InFutureCheck`.
#[derive(Clone, Copy, Debug)]
pub struct FutureBlockCheckConfig {
    /// Shelley genesis `systemStart` as Unix seconds.
    pub system_start_unix_secs: f64,
    /// Shelley genesis slot length in seconds.
    pub slot_length_secs: f64,
    /// Maximum tolerable clock skew.
    pub clock_skew: ClockSkew,
}

fn near_future_wait_duration_until_slot_at(
    now_secs: f64,
    system_start_unix_secs: f64,
    slot_length_secs: f64,
    target_slot: SlotNo,
) -> Option<std::time::Duration> {
    if slot_length_secs <= 0.0 {
        return None;
    }

    // Wait until the start boundary of `target_slot`.
    let target_secs = system_start_unix_secs + (target_slot.0 as f64 * slot_length_secs);
    let wait_secs = target_secs - now_secs;
    if wait_secs <= 0.0 {
        return None;
    }

    Some(std::time::Duration::from_secs_f64(wait_secs))
}

fn near_future_wait_duration(
    system_start_unix_secs: f64,
    slot_length_secs: f64,
    target_slot: SlotNo,
) -> Option<std::time::Duration> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(system_start_unix_secs);
    near_future_wait_duration_until_slot_at(
        now_secs,
        system_start_unix_secs,
        slot_length_secs,
        target_slot,
    )
}

impl FutureBlockCheckConfig {
    /// Compute the current wall-clock slot from `systemStart` and slot length.
    ///
    /// This mirrors upstream `InFutureCheck` behavior where "now" is
    /// re-evaluated for each header arrival, instead of freezing a startup
    /// snapshot for the lifetime of the sync service.
    pub fn current_wall_slot(self) -> SlotNo {
        if self.slot_length_secs <= 0.0 {
            return SlotNo(0);
        }

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(self.system_start_unix_secs);

        if now_secs <= self.system_start_unix_secs {
            return SlotNo(0);
        }

        let elapsed = now_secs - self.system_start_unix_secs;
        SlotNo((elapsed / self.slot_length_secs).floor() as u64)
    }
}

/// Parameters required for VRF leader-eligibility verification.
///
/// VRF verification is intentionally separate from basic header verification
/// because it requires epoch-level protocol state (the epoch nonce) and
/// stake-distribution context (the issuer's relative stake and the active
/// slot coefficient) that are not available during initial chain sync.
///
/// Reference: `validateVRFSignature` in
/// `Ouroboros.Consensus.Protocol.Praos`.
#[derive(Clone, Debug)]
pub struct VrfVerificationParams {
    /// Epoch nonce for the current epoch.
    pub epoch_nonce: Nonce,
    /// Numerator of relative stake (σ) of the block issuer.
    pub sigma_num: u64,
    /// Denominator of relative stake (σ) of the block issuer.
    pub sigma_den: u64,
    /// Active slot coefficient `f` from genesis.
    pub active_slot_coeff: ActiveSlotCoeff,
}

/// Verify the VRF leader-eligibility proof in a multi-era block header.
///
/// Checks that the VRF proof in the header is valid for the block's slot
/// and the given epoch nonce, and that the VRF output meets the leadership
/// threshold for the issuer's relative stake.
///
/// Expects standard (draft-03) 80-byte VRF proofs per CDDL
/// `vrf_cert = [bytes, bytes .size 80]`.
///
/// Byron blocks are skipped (no VRF).
///
/// # Returns
///
/// * `Ok(true)` — VRF proof is valid and output meets leader threshold.
/// * `Ok(false)` — VRF proof is valid but output does not meet threshold.
/// * `Err` — VRF proof is malformed or verification failed.
pub fn verify_block_vrf(
    block: &MultiEraBlock,
    params: &VrfVerificationParams,
) -> Result<bool, SyncError> {
    // Extract VRF fields per era.  TPraos blocks carry two proofs (leader + nonce);
    // Praos blocks carry a single unified proof.
    let (vrf_vkey_bytes, leader_proof, nonce_proof, slot, mode) = match block {
        MultiEraBlock::Shelley(s) => (
            s.header.body.vrf_vkey,
            &s.header.body.leader_vrf.proof,
            Some(&s.header.body.nonce_vrf.proof),
            SlotNo(s.header.body.slot),
            VrfMode::TPraos,
        ),
        MultiEraBlock::Alonzo(a) => (
            a.header.body.vrf_vkey,
            &a.header.body.leader_vrf.proof,
            Some(&a.header.body.nonce_vrf.proof),
            SlotNo(a.header.body.slot),
            VrfMode::TPraos,
        ),
        MultiEraBlock::Babbage(b) => (
            b.header.body.vrf_vkey,
            &b.header.body.vrf_result.proof,
            None,
            SlotNo(b.header.body.slot),
            VrfMode::Praos,
        ),
        MultiEraBlock::Conway(c) => (
            c.header.body.vrf_vkey,
            &c.header.body.vrf_result.proof,
            None,
            SlotNo(c.header.body.slot),
            VrfMode::Praos,
        ),
        MultiEraBlock::Byron { .. } => return Ok(true),
    };

    let vk = VrfVerificationKey::from_bytes(vrf_vkey_bytes);

    // 1. Verify leader VRF proof and check leader threshold.
    let leader_ok = verify_leader_proof(
        &vk,
        slot,
        params.epoch_nonce,
        leader_proof,
        params.sigma_num,
        params.sigma_den,
        &params.active_slot_coeff,
        mode,
    )
    .map_err(SyncError::Consensus)?;

    // 2. For TPraos blocks, also verify the nonce VRF proof (upstream `vrfChecks`
    //    verifies both `bheaderEta` and `bheaderL`).
    if let Some(np) = nonce_proof {
        verify_nonce_proof(&vk, slot, params.epoch_nonce, np).map_err(SyncError::Consensus)?;
    }

    Ok(leader_ok)
}

/// Extract the issuer's cold verification key bytes from a multi-era block.
///
/// Returns `None` for Byron blocks (which use PBFT, not VRF).
pub fn block_issuer_vkey(block: &MultiEraBlock) -> Option<[u8; 32]> {
    match block {
        MultiEraBlock::Shelley(s) => Some(s.header.body.issuer_vkey),
        MultiEraBlock::Alonzo(a) => Some(a.header.body.issuer_vkey),
        MultiEraBlock::Babbage(b) => Some(b.header.body.issuer_vkey),
        MultiEraBlock::Conway(c) => Some(c.header.body.issuer_vkey),
        MultiEraBlock::Byron { .. } => None,
    }
}

/// Extracts the raw VRF verification key bytes from a block header.
///
/// Returns `None` for Byron blocks (no VRF).
pub fn block_vrf_vkey(block: &MultiEraBlock) -> Option<[u8; 32]> {
    match block {
        MultiEraBlock::Shelley(s) => Some(s.header.body.vrf_vkey),
        MultiEraBlock::Alonzo(a) => Some(a.header.body.vrf_vkey),
        MultiEraBlock::Babbage(b) => Some(b.header.body.vrf_vkey),
        MultiEraBlock::Conway(c) => Some(c.header.body.vrf_vkey),
        MultiEraBlock::Byron { .. } => None,
    }
}

/// Extracts the OpCert sequence number from a multi-era block header.
///
/// Returns `None` for Byron blocks (no OpCert).
pub fn block_opcert_sequence_number(block: &MultiEraBlock) -> Option<u64> {
    match block {
        MultiEraBlock::Shelley(s) => Some(s.header.body.operational_cert.sequence_number),
        MultiEraBlock::Alonzo(a) => Some(a.header.body.operational_cert.sequence_number),
        MultiEraBlock::Babbage(b) => Some(b.header.body.operational_cert.sequence_number),
        MultiEraBlock::Conway(c) => Some(c.header.body.operational_cert.sequence_number),
        MultiEraBlock::Byron { .. } => None,
    }
}

/// Validates a block's OpCert sequence number against the per-pool counter
/// state, updating the counters on success.
///
/// This implements the upstream `currentIssueNo` check from
/// `Ouroboros.Consensus.Protocol.Praos`.  Byron blocks are skipped.
///
/// # Arguments
///
/// * `block` — The multi-era block to validate.
/// * `counters` — Mutable reference to the per-pool counter state.
/// * `stake_dist` — The current stake distribution (used to recognize
///   first-seen pools that are not yet in the counter map).
pub fn validate_block_opcert_counter(
    block: &MultiEraBlock,
    counters: &mut OcertCounters,
    stake_dist: &yggdrasil_ledger::PoolStakeDistribution,
) -> Result<(), SyncError> {
    let issuer_vkey_bytes = match block_issuer_vkey(block) {
        Some(vk) => vk,
        None => return Ok(()), // Byron
    };
    let new_seq = match block_opcert_sequence_number(block) {
        Some(s) => s,
        None => return Ok(()), // Byron (redundant, but defensive)
    };

    let pool_hash = yggdrasil_crypto::blake2b::hash_bytes_224(&issuer_vkey_bytes).0;
    let pool_in_dist = stake_dist.contains_pool(&pool_hash);

    counters
        .validate_and_update(pool_hash, new_seq, pool_in_dist)
        .map_err(SyncError::Consensus)
}

/// Validate a block's OpCert counter in permissive mode.
///
/// This variant always treats the issuer pool as "in distribution",
/// which means any first-seen pool is accepted and tracked.  Once
/// tracked, the standard monotonicity rules apply (same or +1).
///
/// Use this during initial sync when the full stake distribution is
/// not yet available.  When a stake distribution is available, prefer
/// [`validate_block_opcert_counter`] for full upstream fidelity.
fn validate_block_opcert_counter_permissive(
    block: &MultiEraBlock,
    counters: &mut OcertCounters,
) -> Result<(), SyncError> {
    let issuer_vkey_bytes = match block_issuer_vkey(block) {
        Some(vk) => vk,
        None => return Ok(()), // Byron
    };
    let new_seq = match block_opcert_sequence_number(block) {
        Some(s) => s,
        None => return Ok(()), // Byron
    };

    let pool_hash = yggdrasil_crypto::blake2b::hash_bytes_224(&issuer_vkey_bytes).0;

    counters
        .validate_and_update(pool_hash, new_seq, /* pool_in_dist */ true)
        .map_err(SyncError::Consensus)
}

/// Verify a block's VRF leader eligibility proof using the pool stake
/// distribution from the ledger's `set` snapshot.
///
/// This function:
/// 1. Extracts the issuer's cold key from the block header.
/// 2. Hashes it (Blake2b-224) to obtain the pool operator key hash.
/// 3. Looks up the pool's relative stake `σ = pool_stake / total_stake`
///    from the stake distribution.
/// 4. Verifies the VRF proof and checks the output against the leader
///    threshold `φ_f(σ) = 1 − (1 − f)^σ`.
///
/// Byron blocks are always `Ok(true)` (no VRF).  If the pool is unknown
/// (not in the stake distribution), `sigma` defaults to `(0, 1)` which will
/// fail the leader check unless the VRF output is exactly zero (impossible
/// in practice).
///
/// Reference: `validateVRFSignature` in
/// `Ouroboros.Consensus.Protocol.Praos`.
pub fn verify_block_vrf_with_stake(
    block: &MultiEraBlock,
    epoch_nonce: Nonce,
    stake_dist: &yggdrasil_ledger::PoolStakeDistribution,
    active_slot_coeff: &ActiveSlotCoeff,
) -> Result<bool, SyncError> {
    let issuer_vkey_bytes = match block_issuer_vkey(block) {
        Some(vk) => vk,
        None => return Ok(true), // Byron
    };

    // Derive pool key hash = Blake2b-224(issuer_vkey).
    let pool_hash = yggdrasil_crypto::blake2b::hash_bytes_224(&issuer_vkey_bytes).0;

    // ── VRF key hash cross-check ─────────────────────────────────────
    //
    // The VRF verification key in the block header must hash to the same
    // value that the pool registered in its `PoolParams.vrf_keyhash`.
    //
    // Reference: `doValidateVRFSignature` in
    // `Ouroboros.Consensus.Protocol.Praos`:
    //   vrfHKBlock = hashVerKeyVRF (vrfKBlock)
    //   vrfHKStake = IndividualPoolStake.iPoolInfoVRF (from PoolDistr)
    //   when vrfHKStake /= vrfHKBlock → VRFKeyBadNonce / VRFKeyBadLeaderValue
    if let Some(vrf_vkey_bytes) = block_vrf_vkey(block) {
        let vrf_hash_block = yggdrasil_crypto::blake2b::hash_bytes_256(&vrf_vkey_bytes).0;
        // Pool not in stake distribution → no VRF cross-check possible;
        // the leader threshold check below will reject anyway (sigma = 0).
        if let Some(registered_vrf_hash) = stake_dist.pool_vrf_key_hash(&pool_hash)
            && vrf_hash_block != *registered_vrf_hash
        {
            return Err(SyncError::Consensus(ConsensusError::VrfKeyMismatch {
                expected: *registered_vrf_hash,
                actual: vrf_hash_block,
            }));
        }
    }

    let (sigma_num, sigma_den) = stake_dist.relative_stake(&pool_hash);

    let params = VrfVerificationParams {
        epoch_nonce,
        sigma_num,
        sigma_den,
        active_slot_coeff: active_slot_coeff.clone(),
    };

    verify_block_vrf(block, &params)
}

/// Applies a multi-era block to the nonce evolution state machine.
///
/// Extracts the VRF nonce contribution and `prev_hash` from the block header
/// and feeds them to [`NonceEvolutionState::apply_block`].
///
/// - TPraos (Shelley–Alonzo): uses the dedicated `nonce_vrf` output with
///   `Blake2b-256(output)` derivation (`hashVerifiedVRF`).
/// - Praos (Babbage/Conway): uses the single `vrf_result` output with
///   `Blake2b-256(Blake2b-256("N" || output))` derivation (`vrfNonceValue`).
/// - Byron blocks are skipped (no VRF).
///
/// After this call, the state's `epoch_nonce` reflects any epoch transition
/// that may have occurred at the block's slot.
///
/// Reference: `vrfNonceValue` in `Ouroboros.Consensus.Protocol.Praos.VRF`,
/// `hashVerifiedVRF` in `Cardano.Ledger.BaseTypes`.
pub fn apply_nonce_evolution(
    state: &mut NonceEvolutionState,
    block: &MultiEraBlock,
    config: &NonceEvolutionConfig,
) {
    match block {
        MultiEraBlock::Shelley(s) => {
            let slot = SlotNo(s.header.body.slot);
            let prev_hash = s.header.body.prev_hash.map(HeaderHash);
            state.apply_block(
                slot,
                &s.header.body.nonce_vrf.output,
                prev_hash,
                config,
                NonceDerivation::TPraos,
            );
        }
        MultiEraBlock::Alonzo(a) => {
            let slot = SlotNo(a.header.body.slot);
            let prev_hash = a.header.body.prev_hash.map(HeaderHash);
            state.apply_block(
                slot,
                &a.header.body.nonce_vrf.output,
                prev_hash,
                config,
                NonceDerivation::TPraos,
            );
        }
        MultiEraBlock::Babbage(b) => {
            let slot = SlotNo(b.header.body.slot);
            let prev_hash = b.header.body.prev_hash.map(HeaderHash);
            state.apply_block(
                slot,
                &b.header.body.vrf_result.output,
                prev_hash,
                config,
                NonceDerivation::Praos,
            );
        }
        MultiEraBlock::Conway(c) => {
            let slot = SlotNo(c.header.body.slot);
            let prev_hash = c.header.body.prev_hash.map(HeaderHash);
            state.apply_block(
                slot,
                &c.header.body.vrf_result.output,
                prev_hash,
                config,
                NonceDerivation::Praos,
            );
        }
        MultiEraBlock::Byron { .. } => {
            // Byron blocks have no VRF; skip nonce evolution.
        }
    }
}

/// Verify that the block body hash declared in the header matches the actual
/// block body content.
///
/// `raw_envelope` is the raw CBOR `[era_tag, inner_block]` envelope as
/// received on the wire.  Byron blocks (era tags 0–1) are skipped because
/// they use a different header format.
///
/// Steps:
/// 1. Peel the 2-element envelope to extract the inner block bytes.
/// 2. Compute the Blake2b-256 hash of the body elements (via
///    `compute_block_body_hash`).
/// 3. Parse the header-body to extract the declared `block_body_hash`
///    (field 8 for 15-element Shelley headers, field 7 for 14-element
///    Praos headers).
/// 4. Compare — mismatch yields `SyncError::BlockBodyHashMismatch`.
pub fn verify_block_body_hash(raw_envelope: &[u8]) -> Result<(), SyncError> {
    let mut dec = Decoder::new(raw_envelope);
    let _arr_len = dec.array()?;
    let era_tag = dec.unsigned()?;
    // Byron blocks use a different header layout — skip them.
    if era_tag <= 1 {
        return Ok(());
    }
    let inner_start = dec.position();
    dec.skip()?;
    let inner_end = dec.position();
    let inner_bytes = dec.slice(inner_start, inner_end)?;

    let computed = compute_block_body_hash(inner_bytes)?;
    let declared = extract_header_block_body_hash(inner_bytes)?;

    if computed != declared {
        return Err(SyncError::BlockBodyHashMismatch);
    }
    Ok(())
}

/// Extract the `block_body_hash` field from the header of a raw inner block.
///
/// The inner block is a CBOR array whose first element is the header,
/// which is itself `[header_body, kes_signature]`.  `header_body` has:
/// - 15 elements for Shelley through Alonzo (`block_body_hash` at index 8)
/// - 14 elements for Babbage/Conway (`block_body_hash` at index 7)
fn extract_header_block_body_hash(inner_block: &[u8]) -> Result<[u8; 32], LedgerError> {
    let mut dec = Decoder::new(inner_block);
    let _block_len = dec.array()?;
    // Element 0: header = [header_body, kes_sig]
    let _hdr_len = dec.array()?;
    let hb_len = dec.array()?;
    let skip_count = match hb_len {
        15 => 8, // Shelley: block_body_hash at index 8
        14 => 7, // Praos (Babbage/Conway): block_body_hash at index 7
        _ => {
            return Err(LedgerError::CborInvalidLength {
                expected: 15,
                actual: hb_len as usize,
            });
        }
    };
    for _ in 0..skip_count {
        dec.skip()?;
    }
    let hash_bytes = dec.bytes()?;
    let mut result = [0u8; 32];
    if hash_bytes.len() != 32 {
        return Err(LedgerError::CborInvalidLength {
            expected: 32,
            actual: hash_bytes.len(),
        });
    }
    result.copy_from_slice(hash_bytes);
    Ok(result)
}

/// Verify the header of a multi-era block.
///
/// Shelley-family blocks (Shelley through Alonzo, including the separate
/// Alonzo variant) use `verify_shelley_header` (15-element header body
/// with two VRF certs).  Babbage and Conway use `verify_praos_header`
/// (14-element header body with single `vrf_result`).  Byron blocks pass
/// through without verification.
///
/// Additionally performs BBODY-level checks:
/// - Protocol version is within the expected range for the block's era
///   (reference: hard-fork combinator era transitions).
pub fn verify_multi_era_block(
    block: &MultiEraBlock,
    config: &VerificationConfig,
) -> Result<(), SyncError> {
    verify_multi_era_block_with_raw(block, None, config)
}

/// Variant of [`verify_multi_era_block`] that accepts the original raw
/// multi-era block CBOR bytes.
///
/// When provided, the raw bytes are used to extract the *exact* CBOR
/// encoding of the header body (`BHBody`).  Those bytes are passed to
/// `verify_header_with_signed_bytes` so the KES signature is verified
/// against the same message upstream `verifyHeader` uses (an annotated
/// CBOR slice — re-encoding the decoded body cannot reproduce it
/// deterministically, see `Cardano.Protocol.Praos.Header.verifyHeader`).
///
/// When `raw_block` is `None`, falls back to the synthetic
/// `to_signable_bytes()` layout used by self-produced blocks and tests.
pub fn verify_multi_era_block_with_raw(
    block: &MultiEraBlock,
    raw_block: Option<&[u8]>,
    config: &VerificationConfig,
) -> Result<(), SyncError> {
    // BBODY/BHEAD-level protocol-version check (Shelley+ only).
    validate_block_protocol_version_with_max(block, config.max_major_protocol_version)?;

    // Conway BBODY: HeaderProtVerTooHigh — header major must be
    // ≤ pp.protocolVersion.major + 1.
    if let Some(pp_major) = config.pp_major_protocol_version {
        if let Some((header_major, _)) = block.protocol_version() {
            if header_major > pp_major + 1 {
                return Err(SyncError::HeaderProtVerTooHigh {
                    header_major,
                    pp_major,
                });
            }
        }
    }

    // Extract the canonical signed CBOR bytes for the header body, when
    // we have the raw block to slice from.
    let body_cbor: Option<Vec<u8>> =
        raw_block.and_then(|raw| extract_header_body_cbor_from_raw_block(raw).ok().flatten());

    match block {
        MultiEraBlock::Shelley(shelley) => verify_shelley_header_with_body_cbor(
            &shelley.header,
            body_cbor.as_deref(),
            config.slots_per_kes_period,
            config.max_kes_evolutions,
        ),
        MultiEraBlock::Alonzo(alonzo) => verify_shelley_header_with_body_cbor(
            &alonzo.header,
            body_cbor.as_deref(),
            config.slots_per_kes_period,
            config.max_kes_evolutions,
        ),
        MultiEraBlock::Babbage(babbage) => verify_praos_header_with_body_cbor(
            &babbage.header,
            body_cbor.as_deref(),
            config.slots_per_kes_period,
            config.max_kes_evolutions,
        ),
        MultiEraBlock::Conway(conway) => verify_praos_header_with_body_cbor(
            &conway.header,
            body_cbor.as_deref(),
            config.slots_per_kes_period,
            config.max_kes_evolutions,
        ),
        MultiEraBlock::Byron { .. } => Ok(()),
    }
}

/// Extract the raw CBOR bytes of the header body (`BHBody`) from a raw
/// multi-era block CBOR payload.
///
/// The wire format is `[era_tag, [header, ...body_segments...]]` where the
/// `header` itself is `[header_body, kes_signature]`.  This function walks
/// that envelope using a CBOR decoder and returns the byte slice of
/// `header_body` exactly as it appeared on the wire — which is the message
/// over which the KES signature was computed by the producer.
///
/// Returns `Ok(None)` for Byron-era blocks (no Shelley-style header body)
/// or any era tag we do not recognise; returns `Err` if the structure does
/// not match the expected envelope.
pub fn extract_header_body_cbor_from_raw_block(raw: &[u8]) -> Result<Option<Vec<u8>>, SyncError> {
    use yggdrasil_ledger::cbor::Decoder;
    let mut dec = Decoder::new(raw);
    let outer_len = dec.array_begin().map_err(SyncError::LedgerDecode)?;
    if let Some(len) = outer_len {
        if len != 2 {
            return Ok(None);
        }
    }
    let era_tag = dec.unsigned().map_err(SyncError::LedgerDecode)?;
    match era_tag {
        era_tag::SHELLEY
        | era_tag::ALLEGRA
        | era_tag::MARY
        | era_tag::ALONZO
        | era_tag::BABBAGE
        | era_tag::CONWAY => {}
        _ => return Ok(None),
    }

    // Inner block array: [header, ...body_segments...]
    let inner_arr_start = dec.position();
    let _inner_len = dec.array_begin().map_err(SyncError::LedgerDecode)?;
    // Element 0: header = [header_body, kes_sig]
    let header_arr_start = dec.position();
    let _header_len = dec.array_begin().map_err(SyncError::LedgerDecode)?;
    // Element 0 of header: header_body
    let body_start = dec.position();
    dec.skip().map_err(SyncError::LedgerDecode)?;
    let body_end = dec.position();
    let _ = (inner_arr_start, header_arr_start);
    Ok(Some(raw[body_start..body_end].to_vec()))
}

/// Variant of [`verify_shelley_header`] that accepts the canonical
/// CBOR-encoded header body for KES verification.  See
/// [`verify_multi_era_block_with_raw`] for context.
pub fn verify_shelley_header_with_body_cbor(
    header: &ShelleyHeader,
    body_cbor: Option<&[u8]>,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<(), SyncError> {
    let consensus_hdr = shelley_header_to_consensus(header)?;
    yggdrasil_consensus::verify_header_with_signed_bytes(
        &consensus_hdr,
        slots_per_kes_period,
        max_kes_evolutions,
        body_cbor,
    )?;
    Ok(())
}

/// Variant of [`verify_praos_header`] that accepts the canonical
/// CBOR-encoded header body for KES verification.  See
/// [`verify_multi_era_block_with_raw`] for context.
pub fn verify_praos_header_with_body_cbor(
    header: &PraosHeader,
    body_cbor: Option<&[u8]>,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<(), SyncError> {
    let consensus_hdr = praos_header_to_consensus(header)?;
    yggdrasil_consensus::verify_header_with_signed_bytes(
        &consensus_hdr,
        slots_per_kes_period,
        max_kes_evolutions,
        body_cbor,
    )?;
    Ok(())
}

/// Validate that the declared `block_body_size` in the header matches the
/// actual serialized size of the block's transaction bodies.
///
/// Upstream reference: `validateBlockBodySize` in
/// `Cardano.Ledger.Shelley.Rules.Bbody` — `WrongBlockBodySizeBBODY`.
///
/// This check is applied at the node layer because the full typed header
/// (carrying `block_body_size`) is available here, while the simplified
/// ledger `Block` wrapper does not carry this field.
///
/// `raw_inner_block` is the CBOR-encoded inner block (the body element of
/// the `[era_tag, block_body]` envelope).
pub fn validate_block_body_size(
    block: &MultiEraBlock,
    raw_inner_block: &[u8],
) -> Result<(), SyncError> {
    let declared = match block.declared_body_size() {
        Some(d) => d,
        None => return Ok(()), // Byron — no declared size in header
    };

    // Compute actual body size from the raw inner-block CBOR.
    // The inner block is an N-element CBOR array; element 0 is the header,
    // and the remaining elements (transaction_bodies, witness_sets, etc.)
    // collectively form the "body" whose serialized size must match.
    //
    // Upstream defines body size as the serialized size of the TxSeq
    // (all transaction-related elements after the header).
    let actual = compute_actual_body_size(raw_inner_block)?;

    if declared != actual {
        return Err(SyncError::WrongBlockBodySize { declared, actual });
    }
    Ok(())
}

/// Compute the serialized body size from a raw inner-block CBOR.
///
/// The body is defined as everything after the header element in the
/// block CBOR array.  This matches upstream `bBodySize` which is the
/// serialized size of the `TxSeq` payload.
fn compute_actual_body_size(raw_inner_block: &[u8]) -> Result<u32, SyncError> {
    let mut dec = Decoder::new(raw_inner_block);
    let _arr_len = dec.array().map_err(SyncError::LedgerDecode)?;
    // Skip element 0 (header).
    dec.skip().map_err(SyncError::LedgerDecode)?;
    let body_start = dec.position();
    // The remaining elements are the body.
    let body_byte_count = raw_inner_block.len() - body_start;
    Ok(body_byte_count as u32)
}

/// Validate that the protocol version in the block header is within the
/// expected range for the block's era.
///
/// Each Cardano era corresponds to its intra-era major versions PLUS
/// the next era's transition major (the upstream hard-fork combinator
/// bumps PV major *within* era N to signal that era N+1 will activate
/// at the next epoch boundary, so the last block of era N and the
/// first block of era N+1 both carry the same major):
///
/// | Era     | Accepted major versions          |
/// |---------|----------------------------------|
/// | Shelley | 2 (intra), 3 (Allegra signal)    |
/// | Allegra | 3 (intra), 4 (Mary signal)       |
/// | Mary    | 4 (intra), 5 (Alonzo signal)     |
/// | Alonzo  | 5, 6 (intra), 7 (Babbage signal) |
/// | Babbage | 7, 8 (intra), 9 (Conway signal)  |
/// | Conway  | 9+ (intra)                       |
///
/// Byron blocks do not carry an in-header protocol version and are skipped.
///
/// Reference: `shelleyTransition` / `allegraTransition` /
/// `maryTransition` / `alonzoTransition` / `babbageTransition` /
/// `conwayTransition` ProtVer values in
/// `Ouroboros.Consensus.Cardano.CanHardFork`.
pub fn validate_block_protocol_version(block: &MultiEraBlock) -> Result<(), SyncError> {
    validate_block_protocol_version_with_max(block, None)
}

/// Validate block protocol version constraints with an optional global
/// maximum major-version guard.
///
/// The optional maximum major-version guard mirrors upstream
/// `MaxMajorProtVer` behavior from
/// `Ouroboros.Consensus.Shelley.Ledger.Block`.
fn validate_block_protocol_version_with_max(
    block: &MultiEraBlock,
    max_major_protocol_version: Option<u64>,
) -> Result<(), SyncError> {
    let (major, minor) = match block.protocol_version() {
        Some(v) => v,
        None => return Ok(()), // Byron
    };

    validate_protocol_version_for_era(block.era(), major, minor, max_major_protocol_version)
}

/// Validate protocol-version constraints for a specific era.
///
/// This helper enforces both:
/// 1. Era-local major-version ranges.
/// 2. Optional global `MaxMajorProtVer` cap.
fn validate_protocol_version_for_era(
    era: Era,
    major: u64,
    minor: u64,
    max_major_protocol_version: Option<u64>,
) -> Result<(), SyncError> {
    // Delegate the `MaxMajorProtVer` ceiling check to the consensus-crate
    // helper (slice 43) so the canonical PRTCL rule from
    // `Cardano.Protocol.Praos.Rules.Prtcl.headerView` is the single source
    // of truth. Converting `ConsensusError::ObsoleteNode` back into
    // `SyncError::ProtocolVersionTooHigh` preserves the existing sync-layer
    // error surface and keeps peer-attribution semantics intact.
    if let Some(max) = max_major_protocol_version {
        if let Err(yggdrasil_consensus::ConsensusError::ObsoleteNode {
            header_major,
            max_major,
        }) = yggdrasil_consensus::check_header_protocol_version(major, max)
        {
            return Err(SyncError::ProtocolVersionTooHigh {
                major: header_major,
                max: max_major,
            });
        }
    }

    // Each era's CBOR codec admits its own intra-era major-version
    // range PLUS the next era's "transition" major.  Upstream's
    // hard-fork combinator bumps the protocol-version major via an
    // in-band protocol-parameters update WITHIN era N to signal
    // that era N+1 will activate at the next epoch boundary — so
    // the LAST block of era N and the FIRST block of era N+1 both
    // carry the same major.  Preview's `Test*HardForkAtEpoch=0`
    // configuration produces this transition-state at chain
    // genesis (Alonzo-codec block with PV major=7 = Babbage
    // signal); rejecting it here was a yggdrasil-specific bug.
    //
    // Reference: `Ouroboros.Consensus.Cardano.CanHardFork`'s
    // `shelleyTransition` / `allegraTransition` / `maryTransition`
    // / `alonzoTransition` / `babbageTransition` / `conwayTransition`
    // ProtVer values.
    let (valid, expected_range) = match era {
        Era::Byron => return Ok(()),
        Era::Shelley => (major == 2 || major == 3, "2..=3"),
        Era::Allegra => (major == 3 || major == 4, "3..=4"),
        Era::Mary => (major == 4 || major == 5, "4..=5"),
        Era::Alonzo => ((5..=7).contains(&major), "5..=7"),
        Era::Babbage => ((7..=9).contains(&major), "7..=9"),
        Era::Conway => (major >= 9, "9+"),
    };

    if !valid {
        return Err(SyncError::ProtocolVersionMismatch {
            era,
            major,
            minor,
            expected_range: expected_range.to_string(),
        });
    }
    Ok(())
}

/// A typed sync step with multi-era block decoding.
///
/// Unlike `TypedSyncStep` (which always decodes as Shelley), this variant
/// preserves the per-block era tag and supports Byron + Shelley-family blocks.
#[derive(Clone, Debug)]
pub enum MultiEraSyncStep {
    /// Roll forward with decoded multi-era blocks.
    RollForward {
        /// Raw header bytes as announced by the peer.
        raw_header: Vec<u8>,
        /// Decoded chain tip.
        tip: Point,
        /// Decoded multi-era blocks.
        blocks: Vec<MultiEraBlock>,
        /// Original wire-format bytes for each block, **parallel to `blocks`**
        /// (same length, same order).
        ///
        /// Stored alongside the decoded `Block` so the inbound server can
        /// re-serve the block over BlockFetch byte-for-byte.  Synthetic
        /// test fakes may pass `Vec::new()` when neither the eviction nor
        /// apply paths are exercised.
        raw_blocks: Vec<Vec<u8>>,
        /// Pre-extracted CBOR byte spans for each Shelley-family block,
        /// parallel to `blocks` and `raw_blocks`.  Populated once at
        /// construction so both the eviction path (`extract_tx_ids`) and
        /// the apply path (`multi_era_block_to_block`) can read tx body
        /// and witness spans without re-walking the block CBOR per
        /// consumer.  Byron blocks store an empty `BlockTxRawSpans`
        /// (Byron envelope is not understood by the span extractor and
        /// has different tx-id derivation rules anyway).
        ///
        /// Synthetic test fakes may pass `Vec::new()` (or fewer entries
        /// than `blocks`); the missing indices trigger the
        /// `extract_tx_ids` typed-re-encode fallback.
        block_spans: Vec<yggdrasil_ledger::BlockTxRawSpans>,
    },
    /// Roll backward to a given point.
    RollBackward {
        /// Decoded rollback target point.
        point: Point,
        /// Decoded chain tip.
        tip: Point,
    },
}

/// Perform a single sync step with multi-era block decoding.
///
/// Like `sync_step_typed` but uses `decode_multi_era_blocks` instead of
/// `decode_shelley_blocks`, preserving era-specific block wrappers.
pub async fn sync_step_multi_era(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    from_point: Point,
) -> Result<MultiEraSyncStep, SyncError> {
    let next = chain_sync.request_next_typed().await?;
    match next {
        TypedNextResponse::RollForward { header, tip }
        | TypedNextResponse::AwaitRollForward { header, tip } => {
            let range_upper = point_from_raw_header(&header).unwrap_or(tip);
            let pairs = if let Some((lower, upper)) =
                normalize_blockfetch_range_points(from_point, range_upper)
            {
                fetch_range_blocks_multi_era_raw_decoded(block_fetch, lower, upper).await?
            } else {
                Vec::new()
            };
            let (raw_blocks, blocks): (Vec<Vec<u8>>, Vec<MultiEraBlock>) =
                pairs.into_iter().unzip();
            let block_spans = extract_spans_per_block(&blocks, &raw_blocks);
            Ok(MultiEraSyncStep::RollForward {
                raw_header: header,
                tip,
                blocks,
                raw_blocks,
                block_spans,
            })
        }
        TypedNextResponse::RollBackward { point, tip }
        | TypedNextResponse::AwaitRollBackward { point, tip } => {
            Ok(MultiEraSyncStep::RollBackward { point, tip })
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 42: ChainState integration helpers
// ---------------------------------------------------------------------------

/// Extract a [`ChainEntry`] from a [`MultiEraBlock`] for chain state tracking.
///
/// All eras including Byron now return `Some`, enabling full chain state
/// tracking from genesis.
pub fn multi_era_block_to_chain_entry(block: &MultiEraBlock) -> Option<ChainEntry> {
    match block {
        MultiEraBlock::Byron { block: byron, .. } => {
            let prev = match byron {
                yggdrasil_ledger::eras::byron::ByronBlock::EpochBoundary { prev_hash, .. }
                | yggdrasil_ledger::eras::byron::ByronBlock::MainBlock { prev_hash, .. } => {
                    Some(HeaderHash(*prev_hash))
                }
            };
            Some(ChainEntry {
                hash: byron.header_hash(),
                slot: SlotNo(byron.absolute_slot(BYRON_SLOTS_PER_EPOCH)),
                block_no: BlockNo(byron.chain_difficulty()),
                prev_hash: prev,
            })
        }
        MultiEraBlock::Shelley(b) => Some(ChainEntry {
            hash: b.header_hash(),
            slot: SlotNo(b.header.body.slot),
            block_no: BlockNo(b.header.body.block_number),
            prev_hash: b.header.body.prev_hash.map(HeaderHash),
        }),
        MultiEraBlock::Alonzo(b) => Some(ChainEntry {
            hash: b.header_hash(),
            slot: SlotNo(b.header.body.slot),
            block_no: BlockNo(b.header.body.block_number),
            prev_hash: b.header.body.prev_hash.map(HeaderHash),
        }),
        MultiEraBlock::Babbage(b) => Some(ChainEntry {
            hash: b.header_hash(),
            slot: SlotNo(b.header.body.slot),
            block_no: BlockNo(b.header.body.block_number),
            prev_hash: b.header.body.prev_hash.map(HeaderHash),
        }),
        MultiEraBlock::Conway(b) => Some(ChainEntry {
            hash: b.header_hash(),
            slot: SlotNo(b.header.body.slot),
            block_no: BlockNo(b.header.body.block_number),
            prev_hash: b.header.body.prev_hash.map(HeaderHash),
        }),
    }
}

/// Seed a fresh [`ChainState`] from the recovered volatile-store contents.
///
/// On node restart, [`ChainState::new`] returns an empty volatile window.
/// Without seeding, the **next ChainSync session's
/// `RollBackward(recovered_tip)` confirmation fails with
/// `RollbackPointNotFound`** because the rollback target — the recovered
/// tip — isn't present in `entries`.  This was observed in the operator
/// restart-resilience rehearsal as a cycle-2 crash:
///
/// ```text
/// Notice Node.Recovery recovered ledger state … point=BlockPoint(88840, …)
/// Notice ConnectionManager verified sync session established fromPoint=BlockPoint(88840, …)
/// Error  Node.Sync rollback point not found: slot 88840 …
/// ```
///
/// Reading every volatile block via `suffix_after(&Point::Origin)` and
/// seeding the chain state with `seed_from_entries` (which trims to `k`
/// internally) closes the gap.  Reference: upstream
/// `Ouroboros.Consensus.Storage.ChainDB.Init.getCurrentChain` rebuilds
/// the in-memory chain fragment from the volatile DB on start-up.
pub fn seed_chain_state_from_volatile(
    volatile: &dyn VolatileStore,
    k: SecurityParam,
) -> ChainState {
    let mut chain_state = ChainState::new(k);
    let blocks = volatile.suffix_after(&Point::Origin);
    if blocks.is_empty() {
        return chain_state;
    }
    let entries: Vec<ChainEntry> = blocks
        .iter()
        .map(|b| ChainEntry {
            hash: b.header.hash,
            slot: b.header.slot_no,
            block_no: b.header.block_no,
            prev_hash: Some(b.header.prev_hash),
        })
        .collect();
    chain_state.seed_from_entries(entries);
    chain_state
}

/// Apply one multi-era sync step to a [`ChainState`].
///
/// Roll-forward blocks are converted to [`ChainEntry`] values and
/// `roll_forward`-ed.  All eras including Byron are tracked.
/// Roll-backward steps trigger `roll_backward` on the chain state.
///
/// After roll-forward processing, stable entries are drained so the
/// volatile window stays bounded to the security parameter `k`.
///
/// # Returns
///
/// The newly stable entries drained from the chain state.
pub fn track_chain_state_entries(
    chain_state: &mut ChainState,
    step: &MultiEraSyncStep,
) -> Result<Vec<ChainEntry>, SyncError> {
    match step {
        MultiEraSyncStep::RollForward { blocks, .. } => {
            for block in blocks {
                if let Some(entry) = multi_era_block_to_chain_entry(block) {
                    chain_state.roll_forward(entry)?;
                }
            }
        }
        MultiEraSyncStep::RollBackward { point, .. } => {
            chain_state.roll_backward(point)?;
        }
    }
    Ok(chain_state.drain_stable())
}

/// Apply one multi-era sync step to a [`ChainState`] and return the number of
/// newly stable entries drained from the chain state.
pub fn track_chain_state(
    chain_state: &mut ChainState,
    step: &MultiEraSyncStep,
) -> Result<usize, SyncError> {
    Ok(track_chain_state_entries(chain_state, step)?.len())
}

/// Promote blocks that have crossed the stability window from volatile
/// storage into immutable storage.
///
/// For each stable [`ChainEntry`], the corresponding block is looked up
/// in the volatile store and appended to the immutable store.  Only
/// entries whose block is still present in volatile are promoted —
/// missing entries are silently skipped.
///
/// # Returns
///
/// The number of blocks successfully promoted.
pub fn promote_stable_blocks<V: VolatileStore, I: ImmutableStore>(
    stable_entries: &[ChainEntry],
    volatile: &V,
    immutable: &mut I,
) -> Result<usize, SyncError> {
    let mut promoted = 0;
    for entry in stable_entries {
        if let Some(block) = volatile.get_block(&entry.hash) {
            immutable.append_block(block.clone())?;
            promoted += 1;
        }
    }
    Ok(promoted)
}

/// Apply one multi-era sync step into a volatile store.
///
/// Roll-forward blocks are converted to generic `Block` values and appended.
/// When `raw_blocks` is present, original wire-format bytes are preserved on
/// each stored block so the inbound server can re-serve them over BlockFetch.
/// Roll-backward steps trigger a store rollback to the given point.
pub fn apply_multi_era_step_to_volatile<S: VolatileStore>(
    store: &mut S,
    step: &MultiEraSyncStep,
) -> Result<(), SyncError> {
    match step {
        MultiEraSyncStep::RollForward {
            blocks,
            raw_blocks,
            block_spans,
            ..
        } => {
            let empty_spans = yggdrasil_ledger::BlockTxRawSpans::default();
            for (i, b) in blocks.iter().enumerate() {
                let spans = block_spans.get(i).unwrap_or(&empty_spans);
                let mut block = multi_era_block_to_block_with_spans(b, spans);
                block.raw_cbor = raw_blocks.get(i).cloned().map(std::sync::Arc::from);
                // BlockFetch ranges can overlap at boundaries across peers.
                // Treat already-present hashes as idempotent replays.
                if store.get_block(&block.header.hash).is_some() {
                    continue;
                }
                store.add_block(block)?;
            }
        }
        MultiEraSyncStep::RollBackward { point, .. } => {
            store.rollback_to(point);
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct AppliedVerifiedProgress {
    pub stable_block_count: usize,
    pub checkpoint_outcome: Option<LedgerCheckpointUpdateOutcome>,
    /// Transaction ids collected from blocks discarded during rollback steps.
    pub rolled_back_tx_ids: Vec<TxId>,
    /// Epoch boundary events emitted during ledger advancement.
    pub epoch_boundary_events: Vec<EpochBoundaryEvent>,
}

pub(crate) fn apply_verified_progress_to_chaindb<I, V, L>(
    chain_db: &mut ChainDb<I, V, L>,
    progress: &MultiEraSyncProgress,
    chain_state: Option<&mut ChainState>,
    checkpoint_tracking: Option<&mut LedgerCheckpointTracking>,
    checkpoint_policy: &LedgerCheckpointPolicy,
    vrf_ctx: Option<&VrfVerificationContext<'_>>,
    ocert_counters: Option<&mut OcertCounters>,
) -> Result<AppliedVerifiedProgress, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    let mut rolled_back_tx_ids = Vec::new();
    for step in &progress.steps {
        if let MultiEraSyncStep::RollBackward { point, .. } = step {
            rolled_back_tx_ids.extend(collect_rolled_back_tx_ids(chain_db.volatile(), point));
        }
        apply_multi_era_step_to_volatile(chain_db.volatile_mut(), step)?;
    }

    let mut total_stable = 0usize;
    if let Some(chain_state) = chain_state {
        for step in &progress.steps {
            let stable_entries = track_chain_state_entries(chain_state, step)?;
            total_stable += stable_entries.len();
            if let Some(last_stable) = stable_entries.last() {
                let point = Point::BlockPoint(last_stable.slot, last_stable.hash);
                chain_db.promote_volatile_prefix(&point)?;
            }
        }
    }

    let (checkpoint_outcome, epoch_boundary_events) = checkpoint_tracking
        .map(|tracking| {
            update_ledger_checkpoint_after_progress(
                chain_db,
                tracking,
                progress,
                checkpoint_policy,
                vrf_ctx,
                ocert_counters,
            )
        })
        .transpose()?
        .map(|(outcome, events)| (Some(outcome), events))
        .unwrap_or((None, Vec::new()));

    Ok(AppliedVerifiedProgress {
        stable_block_count: total_stable,
        checkpoint_outcome,
        rolled_back_tx_ids,
        epoch_boundary_events,
    })
}

fn tentative_header_from_raw_header(
    raw_header: &[u8],
) -> Option<(ConsensusHeaderBody, SlotNo, HeaderHash)> {
    if let Ok(header) = ShelleyHeader::from_cbor_bytes(raw_header) {
        let hash = header.header_hash();
        let slot = SlotNo(header.body.slot);
        let consensus = shelley_header_to_consensus(&header).ok()?;
        return Some((consensus.body, slot, hash));
    }

    if let Ok(header) = PraosHeader::from_cbor_bytes(raw_header) {
        let hash = header.header_hash();
        let slot = SlotNo(header.body.slot);
        let consensus = praos_header_to_consensus(&header).ok()?;
        return Some((consensus.body, slot, hash));
    }

    None
}

fn try_set_tentative_header(
    tentative_state: &Arc<RwLock<TentativeState>>,
    raw_header: &[u8],
) -> bool {
    let Some((header_body, slot, hash)) = tentative_header_from_raw_header(raw_header) else {
        return false;
    };

    let Ok(mut state) = tentative_state.write() else {
        return false;
    };

    state
        .try_set_tentative(&header_body, slot, hash, raw_header.to_vec())
        .is_some()
}

fn clear_tentative_adopted(tentative_state: &Arc<RwLock<TentativeState>>) {
    if let Ok(mut state) = tentative_state.write() {
        let _ = state.clear_adopted();
    }
}

fn clear_tentative_trap(tentative_state: &Arc<RwLock<TentativeState>>) {
    if let Ok(mut state) = tentative_state.write() {
        let _ = state.clear_trap();
    }
}

fn sync_debug_enabled() -> bool {
    std::env::var("YGG_SYNC_DEBUG").is_ok_and(|v| v != "0")
}

/// Lowercase hex-encode a byte slice without per-byte `format!` allocations.
///
/// Used by the optional `YGG_SYNC_DEBUG` trace path to render raw header /
/// block CBOR for diagnostic comparison against upstream `cardano-cli debug`
/// output. Mirrors the helper in `crates/storage/src/file_immutable.rs`.
fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

pub(crate) async fn sync_batch_verified(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    from_point: Point,
    batch_size: usize,
    verification: Option<&VerificationConfig>,
    ocert_counters: &mut Option<OcertCounters>,
    pool_instr: Option<(&BlockFetchInstrumentation, SocketAddr)>,
) -> Result<MultiEraSyncProgress, SyncError> {
    sync_batch_verified_with_tentative(
        chain_sync,
        Some(block_fetch),
        from_point,
        batch_size,
        verification,
        None,
        ocert_counters,
        pool_instr,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn sync_batch_verified_with_tentative(
    chain_sync: &mut ChainSyncClient,
    block_fetch: Option<&mut BlockFetchClient>,
    mut from_point: Point,
    batch_size: usize,
    verification: Option<&VerificationConfig>,
    tentative_state: Option<&Arc<RwLock<TentativeState>>>,
    ocert_counters: &mut Option<OcertCounters>,
    pool_instr: Option<(&BlockFetchInstrumentation, SocketAddr)>,
    density_instr: Option<(&DensityRegistry, SocketAddr)>,
    multi_peer_dispatch: Option<MultiPeerDispatchContext<'_>>,
) -> Result<MultiEraSyncProgress, SyncError> {
    let mut block_fetch = block_fetch;
    let mut steps = Vec::new();
    let mut fetched_blocks = 0usize;
    let mut rollback_count = 0usize;

    for _ in 0..batch_size {
        let next = chain_sync.request_next_typed().await?;

        let me_step = match next {
            TypedNextResponse::RollForward { header, tip }
            | TypedNextResponse::AwaitRollForward { header, tip } => {
                let header_point = point_from_raw_header(&header);
                let range_upper = header_point.unwrap_or(tip);
                let effective_range = normalize_blockfetch_range_points(from_point, range_upper);
                let skip_fetch = header_point.is_some() && effective_range.is_none();
                // Slice GD-RT — Genesis density observation hook.
                // Push the observed header slot into the per-peer
                // `DensityWindow` so the governor can read chain-quality
                // density on its next tick.  No-op when registry is None.
                if let (Some(Point::BlockPoint(slot, _)), Some((registry, peer))) =
                    (header_point, density_instr)
                {
                    let _ = observe_chain_sync_header_density(peer, slot, registry);
                }
                // Round 151 — publish the observed RollForward `(slot,
                // hash)` to the shared `ChainSyncWorkerPool` candidate
                // fragment for the verified-sync session's peer.  This
                // gives `partition_fetch_range_with_candidate_fragments`
                // real intermediate-boundary hashes for multi-peer
                // BlockFetch dispatch.  No-op when the pool isn't
                // wired through.  Reference: Finding A foundation in
                // `node/src/chainsync_worker.rs`.
                if let (Some(Point::BlockPoint(slot, hash)), Some(ctx)) =
                    (header_point, multi_peer_dispatch.as_ref())
                {
                    if let Some(chainsync_pool) = ctx.chainsync_pool.as_ref() {
                        let peer = pool_instr.map(|(_, p)| p).unwrap_or_else(|| {
                            // Fallback synthetic addr — never reached in
                            // production where pool_instr is wired.
                            std::net::SocketAddr::from(([0, 0, 0, 0], 0))
                        });
                        crate::chainsync_worker::publish_announced_header(
                            chainsync_pool,
                            peer,
                            slot,
                            hash,
                        )
                        .await;
                    }
                }
                if sync_debug_enabled() {
                    let header_hex = bytes_to_hex(&header);
                    eprintln!(
                        "[ygg-sync-debug] blockfetch-range lower={:?} upper={:?} tip={:?} header_point_decoded={} range_valid={} skip_fetch={} raw_header_len={} raw_header_hex={}",
                        from_point,
                        range_upper,
                        tip,
                        header_point.is_some(),
                        effective_range.is_some(),
                        skip_fetch,
                        header.len(),
                        header_hex
                    );
                }
                let tentative_set =
                    tentative_state.is_some_and(|state| try_set_tentative_header(state, &header));

                let raw_and_decoded = if skip_fetch {
                    Vec::new()
                } else {
                    match effective_range {
                        Some((lower, upper)) => {
                            // Phase 6 — multi-peer dispatch branch.
                            // Active when the runtime opted in via
                            // `max_concurrent_block_fetch_peers > 1`
                            // AND the shared worker pool has registered
                            // at least one worker.  Reads the pool under
                            // a brief read-lock; dispatches through the
                            // upstream-style per-peer worker tasks
                            // (mirrors `BlockFetch.ClientRegistry`
                            // semantics).
                            //
                            // Genesis bootstrap (`from_point = Origin`)
                            // is handled inside [`FetchWorkerPool::dispatch_plan`]
                            // — `split_range` already returns a single
                            // chunk for Origin lower, and the dispatcher
                            // seeds its `ReorderBuffer` so the chunk
                            // releases cleanly.  Multi-chunk Origin
                            // plans (programmer error) are rejected
                            // upfront.  Reference: `docs/MANUAL_TEST_RUNBOOK.md`
                            // §6.5a "Round 91 Gap BN" closure (Round 144).
                            let multi_peer_result = if let Some(ctx) = &multi_peer_dispatch {
                                let pool_guard = ctx.pool.read().await;
                                let n_workers = pool_guard.len();
                                let effective = effective_block_fetch_concurrency(
                                    ctx.max_concurrent_knob,
                                    n_workers,
                                );
                                if effective > 1 {
                                    let peer_addrs = pool_guard.peer_addrs();
                                    // Round 151 — when a candidate-fragment
                                    // pool is wired through, attempt to
                                    // resolve `split_range`'s placeholder
                                    // hashes against per-peer announcements
                                    // for a real-hash multi-chunk plan.
                                    // Falls back to the placeholder-collapse
                                    // single-chunk path when fragments don't
                                    // have the required hashes.
                                    let plan = if let Some(cs_pool) = ctx.chainsync_pool.as_ref() {
                                        // Drop the BlockFetch read-lock
                                        // before awaiting on the
                                        // ChainSync pool to avoid lock
                                        // ordering issues.
                                        drop(pool_guard);
                                        let resolved =
                                            partition_fetch_range_with_candidate_fragments(
                                                lower,
                                                upper,
                                                &peer_addrs,
                                                ctx.max_concurrent_knob,
                                                cs_pool,
                                            )
                                            .await;
                                        resolved.unwrap_or_else(|| {
                                            partition_fetch_range_across_peers(
                                                lower,
                                                upper,
                                                &peer_addrs,
                                                ctx.max_concurrent_knob,
                                            )
                                        })
                                    } else {
                                        partition_fetch_range_across_peers(
                                            lower,
                                            upper,
                                            &peer_addrs,
                                            ctx.max_concurrent_knob,
                                        )
                                    };
                                    let pool_guard = ctx.pool.read().await;
                                    Some(
                                        pool_guard
                                            .dispatch_plan(
                                                &plan,
                                                from_point,
                                                pool_instr.map(|(p, _)| p),
                                            )
                                            .await,
                                    )
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            // If the multi-peer branch was taken, use
                            // its result.  Otherwise fall through to
                            // the legacy single-peer fetch.
                            if let Some(result) = multi_peer_result {
                                match result {
                                    Ok(mut blocks) => {
                                        // Symmetric `lower_hash` dedup with the
                                        // legacy single-peer branch below.  The
                                        // BlockFetch wire protocol returns the
                                        // closed interval `[lower, upper]`; when
                                        // the caller already has the block at
                                        // `lower` applied (from the previous
                                        // batch's `from_point` advancement), the
                                        // returned vector starts with a
                                        // duplicate.  `apply_multi_era_step_to_volatile`
                                        // tolerates a hash-already-present
                                        // replay idempotently, but
                                        // `track_chain_state_entries` enforces a
                                        // strict block_number contiguity check
                                        // (`expected N, got N-1`) that fires when
                                        // the duplicate is fed in — so the dedup
                                        // must run on both paths.  Missing this
                                        // branch was the second half of Round
                                        // 91 Gap BN: with my placeholder-hash
                                        // collapse the worker now delivers blocks
                                        // correctly, but the un-deduped front
                                        // entry caused
                                        // `consensus error: non-contiguous
                                        // block` on every batch after the first.
                                        // Reference: `docs/MANUAL_TEST_RUNBOOK.md`
                                        // §6.5a Round 144 closure.
                                        if let (Point::BlockPoint(_, lower_hash), true) =
                                            (lower, matches!(from_point, Point::BlockPoint(_, _)))
                                        {
                                            while let Some((first_raw, first)) = blocks.first() {
                                                let first_hash =
                                                    multi_era_block_to_block(first, first_raw)
                                                        .header
                                                        .hash;
                                                if first_hash == lower_hash {
                                                    blocks.remove(0);
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                        blocks
                                    }
                                    Err(err) => {
                                        if tentative_set {
                                            if let Some(state) = tentative_state {
                                                clear_tentative_trap(state);
                                            }
                                        }
                                        return Err(err);
                                    }
                                }
                            } else {
                                // Pool instrumentation: record dispatch synchronously
                                // so per-peer in-flight accounting reflects the
                                // outstanding fetch.  Mirrors upstream
                                // `bumpFetchClientStateVars` in
                                // `Ouroboros.Network.BlockFetch.ClientState`.
                                if let Some((pool, peer)) = pool_instr {
                                    if let Ok(mut g) = pool.lock() {
                                        g.note_dispatch(peer);
                                    }
                                }
                                let bf = block_fetch.as_deref_mut().expect(
                                "legacy single-peer fetch path requires Some(BlockFetchClient); \
                                 caller must provide the leader's BlockFetch handle when no \
                                 multi-peer dispatch context is active",
                            );
                                match fetch_range_blocks_multi_era_raw_decoded(bf, lower, upper)
                                    .await
                                {
                                    Ok(mut blocks) => {
                                        // Only deduplicate against `lower_hash` when the
                                        // caller actually had a known prior tip
                                        // (`from_point` was a BlockPoint).  When syncing
                                        // from Origin, `normalize_blockfetch_range_points`
                                        // sets `lower = upper`, and dropping blocks that
                                        // match `lower_hash` would erase the very first
                                        // block we just fetched.
                                        if let (Point::BlockPoint(_, lower_hash), true) =
                                            (lower, matches!(from_point, Point::BlockPoint(_, _)))
                                        {
                                            while let Some((first_raw, first)) = blocks.first() {
                                                let first_hash =
                                                    multi_era_block_to_block(first, first_raw)
                                                        .header
                                                        .hash;
                                                if first_hash == lower_hash {
                                                    blocks.remove(0);
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                        blocks
                                    }
                                    Err(err) => {
                                        if let Some((pool, peer)) = pool_instr {
                                            if let Ok(mut g) = pool.lock() {
                                                g.note_failure(peer);
                                            }
                                        }
                                        if tentative_set {
                                            if let Some(state) = tentative_state {
                                                clear_tentative_trap(state);
                                            }
                                        }
                                        return Err(err);
                                    }
                                }
                            }
                        }
                        None => Vec::new(),
                    }
                };

                // Pool instrumentation: record success after fetch completes
                // (and any caller-side dedup is applied above).
                if let Some((pool, peer)) = pool_instr {
                    if let Ok(mut g) = pool.lock() {
                        let n_blocks = raw_and_decoded.len() as u64;
                        let n_bytes: u64 = raw_and_decoded
                            .iter()
                            .map(|(raw, _)| raw.len() as u64)
                            .sum();
                        g.note_success(peer, n_blocks, n_bytes, Instant::now());
                    }
                }

                if let Some(config) = verification {
                    if config.verify_body_hash {
                        for (raw, _) in &raw_and_decoded {
                            if let Err(err) = verify_block_body_hash(raw) {
                                if tentative_set {
                                    if let Some(state) = tentative_state {
                                        clear_tentative_trap(state);
                                    }
                                }
                                return Err(err);
                            }
                        }
                    }
                }

                let (raw_bytes, decoded_blocks): (Vec<Vec<u8>>, Vec<MultiEraBlock>) =
                    raw_and_decoded.into_iter().unzip();

                if let Some(config) = verification {
                    for (raw, block) in raw_bytes.iter().zip(decoded_blocks.iter()) {
                        if let Err(err) =
                            verify_multi_era_block_with_raw(block, Some(raw.as_slice()), config)
                        {
                            if tentative_set {
                                if let Some(state) = tentative_state {
                                    clear_tentative_trap(state);
                                }
                            }
                            return Err(err);
                        }
                    }

                    // Blocks-from-the-future check: reject blocks whose
                    // slot exceeds the tolerable clock skew window.
                    //
                    // Near-future blocks (within skew) are tolerated after
                    // waiting until their slot is no longer in the future,
                    // matching upstream `InFutureCheck` behavior.
                    //
                    // Far-future blocks trigger a peer-attributable error
                    // (see `SyncError::is_peer_attributable`) which causes
                    // the runtime to disconnect and reconnect to another
                    // peer.
                    //
                    // Reference: `InFutureCheck.handleHeaderArrival` in
                    // `ouroboros-consensus`.
                    if let Some(ref fc) = config.future_check {
                        let current_wall_slot = fc.current_wall_slot();
                        let mut max_near_future_slot: Option<SlotNo> = None;
                        for block in &decoded_blocks {
                            let block_slot = block.slot();
                            match judge_header_slot(block_slot, current_wall_slot, fc.clock_skew) {
                                FutureSlotJudgement::NotFuture => {}
                                FutureSlotJudgement::NearFuture { .. } => {
                                    max_near_future_slot = Some(
                                        max_near_future_slot
                                            .map(|s| std::cmp::max(s, block_slot))
                                            .unwrap_or(block_slot),
                                    );
                                }
                                FutureSlotJudgement::FarFuture { excess_slots } => {
                                    if tentative_set {
                                        if let Some(state) = tentative_state {
                                            clear_tentative_trap(state);
                                        }
                                    }
                                    return Err(SyncError::BlockFromFuture {
                                        slot: block_slot.0,
                                        excess_slots,
                                    });
                                }
                            }
                        }

                        if let Some(wait) = max_near_future_slot.and_then(|slot| {
                            near_future_wait_duration(
                                fc.system_start_unix_secs,
                                fc.slot_length_secs,
                                slot,
                            )
                        }) {
                            tokio::time::sleep(wait).await;
                        }
                    }
                }

                // OpCert counter validation: each Shelley-family block's
                // OpCert sequence number must be ≥ the stored counter for
                // its issuer pool and ≤ stored + 1.  First-seen pools are
                // accepted permissively (without stake distribution lookup).
                //
                // Reference: `PraosState.csCounters` in
                // `Ouroboros.Consensus.Protocol.Praos`.
                if let Some(ref mut counters) = *ocert_counters {
                    for block in &decoded_blocks {
                        if let Err(err) = validate_block_opcert_counter_permissive(block, counters)
                        {
                            if tentative_set {
                                if let Some(state) = tentative_state {
                                    clear_tentative_trap(state);
                                }
                            }
                            return Err(err);
                        }
                    }
                }

                if tentative_set {
                    if let Some(state) = tentative_state {
                        clear_tentative_adopted(state);
                    }
                }

                if let Some((_, upper)) = effective_range {
                    from_point = upper;
                }
                fetched_blocks += decoded_blocks.len();

                {
                    let block_spans = extract_spans_per_block(&decoded_blocks, &raw_bytes);
                    MultiEraSyncStep::RollForward {
                        raw_header: header,
                        tip,
                        blocks: decoded_blocks,
                        raw_blocks: raw_bytes,
                        block_spans,
                    }
                }
            }
            TypedNextResponse::RollBackward { point, tip }
            | TypedNextResponse::AwaitRollBackward { point, tip } => {
                from_point = point;
                rollback_count += 1;

                MultiEraSyncStep::RollBackward { point, tip }
            }
        };

        steps.push(me_step);
    }

    Ok(MultiEraSyncProgress {
        current_point: from_point,
        steps,
        fetched_blocks,
        rollback_count,
    })
}

/// Execute one batch of verified multi-era sync and apply results to storage.
///
/// This combines `sync_step` with optional body-hash and header verification
/// (via `verify_block_body_hash` and `verify_multi_era_block`) and
/// `apply_multi_era_step_to_volatile` into a single composable batch.
///
/// When `verification` is `Some`:
/// - If `verify_body_hash` is set, each raw block envelope is checked against
///   its declared header body hash before decoding.
/// - Every Shelley-family block header is KES-verified after decoding.
///
/// Byron blocks pass through both checks without verification.
#[allow(clippy::too_many_arguments)]
pub async fn sync_batch_apply_verified<S: VolatileStore>(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    store: &mut S,
    from_point: Point,
    batch_size: usize,
    verification: Option<&VerificationConfig>,
    ocert_counters: &mut Option<OcertCounters>,
    pool_instr: Option<(&BlockFetchInstrumentation, SocketAddr)>,
) -> Result<MultiEraSyncProgress, SyncError> {
    let progress = sync_batch_verified(
        chain_sync,
        block_fetch,
        from_point,
        batch_size,
        verification,
        ocert_counters,
        pool_instr,
    )
    .await?;

    for step in &progress.steps {
        apply_multi_era_step_to_volatile(store, step)?;
    }

    Ok(progress)
}

/// Progress summary from a multi-era sync batch.
#[derive(Clone, Debug)]
pub struct MultiEraSyncProgress {
    /// Current chain point after all steps in this batch.
    pub current_point: Point,
    /// Individual multi-era steps in order of execution.
    pub steps: Vec<MultiEraSyncStep>,
    /// Total number of fetched blocks across all roll-forward steps.
    pub fetched_blocks: usize,
    /// Number of rollback steps observed.
    pub rollback_count: usize,
}

impl MultiEraSyncProgress {
    /// Return the block number of the last block in the last roll-forward
    /// step, if any.
    ///
    /// Walks steps in reverse to find the final roll-forward and extracts
    /// the block number from its last decoded block.
    pub fn latest_block_number(&self) -> Option<u64> {
        for step in self.steps.iter().rev() {
            if let MultiEraSyncStep::RollForward { blocks, .. } = step {
                if let Some(entry) = blocks.last().and_then(multi_era_block_to_chain_entry) {
                    return Some(entry.block_no.0);
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Phase 40: Mempool sync eviction
// ---------------------------------------------------------------------------

/// Pre-extract `BlockTxRawSpans` for every block in a roll-forward step.
///
/// Run once at sync-step construction so the eviction path
/// (`extract_tx_ids`) and the apply path (`multi_era_block_to_block`) can
/// share the cached spans instead of each re-walking the block CBOR.
/// Byron blocks and indexes whose `raw_blocks` slot is missing or fails
/// span extraction get a `BlockTxRawSpans::default()` (empty bodies and
/// witnesses) — consumers detect that as "no spans available" and fall
/// back to typed re-encoding.
pub fn extract_spans_per_block(
    blocks: &[MultiEraBlock],
    raw_blocks: &[Vec<u8>],
) -> Vec<yggdrasil_ledger::BlockTxRawSpans> {
    blocks
        .iter()
        .enumerate()
        .map(|(idx, block)| {
            if matches!(block, MultiEraBlock::Byron { .. }) {
                return yggdrasil_ledger::BlockTxRawSpans::default();
            }
            let raw = match raw_blocks.get(idx) {
                Some(b) if !b.is_empty() => b.as_slice(),
                _ => return yggdrasil_ledger::BlockTxRawSpans::default(),
            };
            yggdrasil_ledger::extract_block_tx_byte_spans(raw).unwrap_or_default()
        })
        .collect()
}

/// Extract transaction IDs from a multi-era block, given pre-extracted
/// `BlockTxRawSpans`.
///
/// Pass `Some(spans)` for Shelley-family blocks where the cached spans are
/// available (the production sync path computes them once at step
/// construction, see [`extract_spans_per_block`]).  Pass `None` to fall
/// back to typed re-encoding of each tx body — which can diverge from the
/// wallet's original on-wire bytes and silently miss mempool entries, so
/// production paths must never use the fallback.
///
/// Returned `TxId`s are `blake2b-256(on-wire body)` per upstream
/// `Cardano.Ledger.Core.txIdTxBody`.  The Byron arm ignores `spans`
/// (different envelope; tx-id derivation runs over the typed `ByronTx`).
pub fn extract_tx_ids(
    block: &MultiEraBlock,
    spans: Option<&yggdrasil_ledger::BlockTxRawSpans>,
) -> Vec<TxId> {
    fn id_at<B: CborEncode>(
        spans: Option<&yggdrasil_ledger::BlockTxRawSpans>,
        idx: usize,
        body: &B,
    ) -> TxId {
        match spans.and_then(|s| s.bodies.get(idx)) {
            Some(raw) => compute_tx_id(raw),
            None => compute_tx_id(&body.to_cbor_bytes()),
        }
    }
    macro_rules! shelley_family_ids {
        ($txs:expr, $spans:expr) => {
            $txs.iter()
                .enumerate()
                .map(|(idx, body)| id_at($spans, idx, body))
                .collect()
        };
    }
    // A `Default::default()` BlockTxRawSpans (empty bodies/witnesses) is
    // semantically the same as None for our purposes — span lookup
    // misses, fallback fires.  Treat it as None.
    let s = spans.filter(|s| !s.bodies.is_empty());
    match block {
        MultiEraBlock::Shelley(b) => shelley_family_ids!(b.transaction_bodies, s),
        MultiEraBlock::Alonzo(b) => shelley_family_ids!(b.transaction_bodies, s),
        MultiEraBlock::Babbage(b) => shelley_family_ids!(b.transaction_bodies, s),
        MultiEraBlock::Conway(b) => shelley_family_ids!(b.transaction_bodies, s),
        MultiEraBlock::Byron { block, .. } => match block {
            ByronBlock::MainBlock { transactions, .. } => transactions
                .iter()
                .map(|tx_aux| TxId(tx_aux.tx.tx_id()))
                .collect(),
            ByronBlock::EpochBoundary { .. } => vec![],
        },
    }
}

/// Collect all UTxO inputs consumed by the transactions in a block.
///
/// This extracts `ShelleyTxIn` inputs from all Shelley-family era blocks.
/// Byron blocks are skipped (Byron transactions are not in the mempool).
///
/// Used for mempool conflict detection: after applying a block, any mempool
/// transaction that also spends one of these inputs is invalid and must be
/// evicted.
///
/// Reference: `Ouroboros.Consensus.Mempool.Impl.Update` —
/// `revalidateTxsFor` implicitly catches consumed inputs via re-apply.
pub fn extract_consumed_inputs(block: &MultiEraBlock) -> Vec<ShelleyTxIn> {
    match block {
        MultiEraBlock::Shelley(shelley) => shelley
            .transaction_bodies
            .iter()
            .flat_map(|body| body.inputs.iter().cloned())
            .collect(),
        MultiEraBlock::Alonzo(alonzo) => alonzo
            .transaction_bodies
            .iter()
            .flat_map(|body| body.inputs.iter().cloned())
            .collect(),
        MultiEraBlock::Babbage(babbage) => babbage
            .transaction_bodies
            .iter()
            .flat_map(|body| body.inputs.iter().cloned())
            .collect(),
        MultiEraBlock::Conway(conway) => conway
            .transaction_bodies
            .iter()
            .flat_map(|body| body.inputs.iter().cloned())
            .collect(),
        MultiEraBlock::Byron { .. } => vec![],
    }
}

/// Evict confirmed transactions from the mempool after a sync step.
///
/// For roll-forward steps, every transaction included in the new blocks is
/// removed from the mempool via `remove_confirmed`. Expired entries are
/// then purged using the tip slot.
///
/// Roll-backward steps do not modify the mempool — re-admission of
/// rolled-back transactions is handled separately.
///
/// Returns the total number of entries evicted (confirmed + expired).
pub fn evict_confirmed_from_mempool(mempool: &mut Mempool, step: &MultiEraSyncStep) -> usize {
    match step {
        MultiEraSyncStep::RollForward {
            blocks,
            block_spans,
            tip,
            ..
        } => {
            let confirmed_ids: Vec<TxId> = blocks
                .iter()
                .enumerate()
                .flat_map(|(i, b)| extract_tx_ids(b, block_spans.get(i)))
                .collect();
            let removed = mempool.remove_confirmed(&confirmed_ids);
            let tip_slot = tip.slot().unwrap_or(SlotNo(0));
            let purged = mempool.purge_expired(tip_slot);
            removed + purged
        }
        MultiEraSyncStep::RollBackward { .. } => 0,
    }
}

/// Collect transaction IDs from rolled-back blocks so they can be
/// considered for re-admission.
///
/// Before a volatile store rollback is applied, this reads the blocks
/// that will be discarded (the suffix *after* `target`) and returns
/// their transaction IDs. Callers can then re-admit any of these
/// transactions that remain valid under the new chain state.
///
/// Reference: `Ouroboros.Consensus.Mempool.Impl.Common` — post-rollback
/// re-addition of rolled-back transactions.
pub fn collect_rolled_back_tx_ids<V: VolatileStore>(store: &V, target: &Point) -> Vec<TxId> {
    store
        .suffix_after(target)
        .iter()
        .flat_map(|block| block.transactions.iter().map(|tx| tx.id))
        .collect()
}

// ---------------------------------------------------------------------------
// Slice GD-RT — ChainSync header density observation runtime hook
// ---------------------------------------------------------------------------
//
// Wires the consensus-side `DensityWindow` primitive (`Slice GD`,
// `crates/consensus/src/genesis_density.rs`) into the runtime ChainSync
// path so the network governor can read per-peer chain density as a
// hot-demotion signal.  Mirrors upstream
// `Ouroboros.Consensus.Genesis.Governor` density tracking — observed
// header slots feed a per-peer sliding window, the governor consults
// the resulting density when scoring hot peers.

/// Per-peer ChainSync header-density registry.
///
/// Wraps a `BTreeMap<SocketAddr, DensityWindow>` in `Arc<RwLock<>>` so
/// the runtime sync loops, the governor, and any future scheduler can
/// share a single density view across peer sessions.  Construct with
/// [`new_density_registry`]; consume read-only via the governor's
/// hot-demotion bias.
pub type DensityRegistry = Arc<RwLock<BTreeMap<SocketAddr, yggdrasil_consensus::DensityWindow>>>;

/// Construct an empty [`DensityRegistry`] suitable for
/// [`VerifiedSyncServiceConfig::density_registry`].
pub fn new_density_registry() -> DensityRegistry {
    Arc::new(RwLock::new(BTreeMap::new()))
}

/// Observe a ChainSync header at `slot` against `peer`'s density window.
///
/// Creates the per-peer window on first observation (using the upstream
/// default [`yggdrasil_consensus::DEFAULT_SLOT_WINDOW`] = 6 480 slots).
/// Returns `true` if the header was admitted, `false` if rejected as a
/// slot regression (the runtime is responsible for not double-counting
/// rolled-back headers; this guard is a defensive secondary).
///
/// Reference: `Ouroboros.Consensus.Genesis.Governor` density updates
/// per ChainSync `RollForward`.
pub fn observe_chain_sync_header_density(
    peer: SocketAddr,
    slot: SlotNo,
    registry: &DensityRegistry,
) -> bool {
    let Ok(mut guard) = registry.write() else {
        // Poisoned lock: skip observation rather than propagate panic
        // up through the sync loop.  The governor will simply read a
        // stale density on the next tick, which is the same fallback
        // the upstream `bracketWithLock` pattern uses on contention.
        return false;
    };
    let window = guard
        .entry(peer)
        .or_insert_with(yggdrasil_consensus::DensityWindow::new);
    window.observe_header(slot)
}

/// Read the current density for `peer`, or `0.0` if no window exists.
/// Read-only; intended for governor consumption during hot-demotion
/// scoring (`density < DEFAULT_LOW_DENSITY_THRESHOLD` ⇒ bias demotion).
pub fn read_peer_density(peer: SocketAddr, registry: &DensityRegistry) -> f64 {
    let Ok(guard) = registry.read() else {
        return 0.0;
    };
    guard.get(&peer).map_or(0.0, |w| w.density())
}

/// Forget a peer's density window.  Called by the runtime when a peer
/// disconnects so stale density is not carried into a future
/// connection.
pub fn forget_peer_density(peer: SocketAddr, registry: &DensityRegistry) {
    let Ok(mut guard) = registry.write() else {
        return;
    };
    guard.remove(&peer);
}

// ---------------------------------------------------------------------------
// Slice E — Multi-peer concurrent BlockFetch dispatch primitives
// ---------------------------------------------------------------------------
//
// These helpers translate the `max_concurrent_block_fetch_peers`
// configuration knob (`node/src/config.rs:285`) into per-peer fetch
// assignments using the existing `BlockFetchPool` / `split_range` /
// `ReorderBuffer` foundation in `crates/network/src/blockfetch_pool.rs`.
//
// The runtime call site in `sync_batch_verified_with_tentative` operates
// on a single `BlockFetchClient` per `session`, so production wiring of
// the actual parallel dispatch is gated on a follow-up that maintains
// multiple sessions concurrently.  The helpers below are the primitive
// layer that future work can drive directly: they are pure, synchronous,
// fully tested, and exercise the config knob from a public API path so
// the audit gap "max_concurrent_block_fetch_peers is read by no
// production path yet" is closed at the API surface.
//
// Reference: upstream `Ouroboros.Network.BlockFetch.Decision` —
// `bfcMaxConcurrencyDeadline = 1`, `bfcMaxConcurrencyBulkSync = 2`
// (upstream typically caps at 2 per fetch-mode).

/// Bound the configured `max_concurrent_block_fetch_peers` knob to the
/// peer slice in a way the dispatcher can blindly map onto a peer index.
///
/// Returns `1` (the legacy single-peer path, no behavioural change) when
/// either:
/// - the knob is `0` or `1`; or
/// - there is at most one peer.
///
/// Otherwise returns `min(knob as usize, n_peers)`, capped at the peer
/// slice length so callers cannot index past the end.
///
/// Reference: upstream `bfcMaxConcurrency{Deadline,BulkSync}` clamping.
pub fn effective_block_fetch_concurrency(max_knob: u8, n_peers: usize) -> usize {
    let knob = max_knob as usize;
    if n_peers == 0 {
        return 1;
    }
    if knob <= 1 {
        return 1;
    }
    knob.min(n_peers)
}

/// Per-peer fetch assignment produced by [`partition_fetch_range_across_peers`].
///
/// Each assignment is a self-contained instruction: `peer` will fetch the
/// block range `[lower, upper]` (inclusive) using its own
/// `BlockFetchClient`.  Assignments are returned in chain (slot-ascending)
/// order so a downstream `ReorderBuffer::insert(lower, upper, blocks)`
/// drains them in the same order they were dispatched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockFetchAssignment {
    /// Peer that will execute this BlockFetch range request.
    pub peer: SocketAddr,
    /// Lower bound of the range (inclusive).  The first assignment
    /// always carries the original `lower` from the planner input.
    pub lower: Point,
    /// Upper bound of the range (inclusive).  The last assignment
    /// always carries the original `upper`.
    pub upper: Point,
}

/// Partition a fetch range across a peer slice for parallel BlockFetch
/// dispatch, honouring the `max_concurrent_block_fetch_peers` knob.
///
/// The algorithm:
///
/// 1. Compute `n = effective_block_fetch_concurrency(max_knob, peers.len())`.
/// 2. Call `crates/network::blockfetch_pool::split_range(lower, upper, n)`
///    to obtain `n` chunk ranges in slot order.
/// 3. Pair the i-th chunk with the i-th peer in the input slice.
///
/// The first peer always receives the chunk containing the original
/// `lower`, and the last peer (within the effective concurrency window)
/// receives the chunk containing `upper`.  Peers beyond the window are
/// not assigned in this round; the runtime can rotate the slice across
/// rounds for fair distribution.
///
/// Returns an empty `Vec` only when `peers` is empty.
///
/// **Placeholder-hash guard (Round 144 follow-up — closes the runtime
/// half of Round 91 Gap BN):** when `split_range` produces multi-chunk
/// output, intermediate boundaries carry a synthesised
/// `HeaderHash([0; 32])` placeholder.  The runtime currently has no
/// candidate-fragment lookup to resolve those placeholders to real
/// chain points before issuing `MsgRequestRange`, so peers receive a
/// fetch request with an unknown upper-bound hash and respond with
/// `NoBlocks` — every batch returns zero blocks, volatile storage
/// stays empty, and the node livelocks re-syncing from Origin.  Until
/// multi-peer ChainSync candidate fragments land (`Ouroboros.Network.BlockFetch.Decision.fetchDecisions`
/// equivalent), collapse any plan containing a placeholder boundary
/// to a single-chunk plan against `peers[0]`.  The single-chunk plan
/// hits `FetchWorkerPool::dispatch_plan`'s fast path which bypasses
/// the `ReorderBuffer` entirely, so storage populates correctly on
/// the multi-peer path even when the worker pool has multiple peers.
/// Cross-batch peer rotation is preserved because successive batches
/// see a fresh `peer_addrs()` snapshot from the pool's BTreeMap and
/// the runtime advances `from_point` regardless of which peer served
/// each batch.  Reference: `docs/MANUAL_TEST_RUNBOOK.md` §6.5a.
pub fn partition_fetch_range_across_peers(
    lower: Point,
    upper: Point,
    peers: &[SocketAddr],
    max_knob: u8,
) -> Vec<BlockFetchAssignment> {
    if peers.is_empty() {
        return Vec::new();
    }
    let n = effective_block_fetch_concurrency(max_knob, peers.len());
    let chunks = yggdrasil_network::blockfetch_pool::split_range(lower, upper, n);
    if chunks
        .iter()
        .any(|(l, u)| point_carries_placeholder_hash(l) || point_carries_placeholder_hash(u))
    {
        return vec![BlockFetchAssignment {
            peer: peers[0],
            lower,
            upper,
        }];
    }
    chunks
        .into_iter()
        .zip(peers.iter().take(n))
        .map(|((chunk_lower, chunk_upper), addr)| BlockFetchAssignment {
            peer: *addr,
            lower: chunk_lower,
            upper: chunk_upper,
        })
        .collect()
}

/// Round 150 — Finding A foundation.  Multi-peer-aware variant of
/// [`partition_fetch_range_across_peers`] that resolves
/// `split_range`'s placeholder hashes against per-peer candidate
/// fragments populated by the [`crate::chainsync_worker::ChainSyncWorkerPool`].
///
/// When `split_range` produces a multi-chunk plan, each intermediate
/// boundary slot is looked up in the candidate-fragment registry; a
/// successful lookup replaces the synthesised `[0; 32]` hash with the
/// peer-announced real hash, allowing the upstream BlockFetch wire
/// `MsgRequestRange` to be served by any peer whose chain includes
/// that point (mirrors upstream
/// `Ouroboros.Network.BlockFetch.Decision.fetchDecisions`).
///
/// Returns `Some(plan)` if every intermediate boundary was resolvable;
/// returns `None` if any boundary remained a placeholder, signalling
/// the caller to fall back to the single-chunk path
/// (`partition_fetch_range_across_peers`).
pub async fn partition_fetch_range_with_candidate_fragments(
    lower: Point,
    upper: Point,
    peers: &[SocketAddr],
    max_knob: u8,
    chainsync_pool: &crate::chainsync_worker::SharedChainSyncWorkerPool,
) -> Option<Vec<BlockFetchAssignment>> {
    if peers.is_empty() {
        return None;
    }
    let n = effective_block_fetch_concurrency(max_knob, peers.len());
    let mut chunks = yggdrasil_network::blockfetch_pool::split_range(lower, upper, n);

    // Walk every chunk endpoint; replace placeholder hashes with
    // per-peer candidate-fragment lookups.
    let pool_guard = chainsync_pool.read().await;
    for (chunk_lower, chunk_upper) in chunks.iter_mut() {
        if let Point::BlockPoint(slot, hash) = *chunk_lower {
            if hash.0 == [0u8; 32] {
                match pool_guard.resolve_slot_to_hash(slot).await {
                    Some(real_hash) => *chunk_lower = Point::BlockPoint(slot, real_hash),
                    None => return None,
                }
            }
        }
        if let Point::BlockPoint(slot, hash) = *chunk_upper {
            if hash.0 == [0u8; 32] {
                match pool_guard.resolve_slot_to_hash(slot).await {
                    Some(real_hash) => *chunk_upper = Point::BlockPoint(slot, real_hash),
                    None => return None,
                }
            }
        }
    }
    drop(pool_guard);

    Some(
        chunks
            .into_iter()
            .zip(peers.iter().take(n))
            .map(|((chunk_lower, chunk_upper), addr)| BlockFetchAssignment {
                peer: *addr,
                lower: chunk_lower,
                upper: chunk_upper,
            })
            .collect(),
    )
}

/// Returns `true` if `p` carries the all-zeros placeholder
/// [`HeaderHash`] that `yggdrasil_network::blockfetch_pool::split_range`
/// synthesises for intermediate chunk boundaries.  Used by
/// [`partition_fetch_range_across_peers`] to detect plans that the
/// runtime cannot dispatch on the wire (peers respond with `NoBlocks`
/// for unknown-hash bounds, producing the operational livelock
/// described in `docs/MANUAL_TEST_RUNBOOK.md` §6.5a).
fn point_carries_placeholder_hash(p: &Point) -> bool {
    matches!(p, Point::BlockPoint(_, hash) if hash.0 == [0u8; 32])
}

/// Execute a multi-peer BlockFetch plan in parallel and return blocks
/// in chain (slot-ascending) order.
///
/// `fetch_one` is the per-(peer, range) fetch closure — typically wraps
/// `fetch_range_blocks_multi_era_raw_decoded(block_fetch, lower, upper)`
/// for a real `BlockFetchClient`, but is generic so tests can drive it
/// with synthetic in-memory data.  Each closure invocation returns
/// `Vec<(raw_block_bytes, MultiEraBlock)>` for the assigned chunk.
///
/// Concurrency model:
/// - All assignments dispatch concurrently via `tokio::JoinSet`.
/// - On any chunk error, sibling tasks are aborted via `abort_all()`
///   and the first observed error is propagated; pool failures are
///   recorded against the offending peer.
/// - Successful chunks are buffered through a `ReorderBuffer` keyed
///   on chunk lower-bound so the validator receives blocks in chain
///   order even when peers respond out-of-order.
/// - Pool instrumentation receives `note_dispatch` per assignment up
///   front, then `note_success(peer, n_blocks, n_bytes, now)` /
///   `note_failure(peer)` per outcome — mirroring the single-peer
///   accounting in `sync_batch_verified_with_tentative`.
///
/// Tentative-header timing: this primitive intentionally does NOT
/// touch [`yggdrasil_consensus::TentativeState`].  The caller is
/// responsible for `try_set_tentative_header` BEFORE invoking this
/// function and `clear_tentative_trap` on `Err` — same contract as
/// the single-peer path.  This keeps the consensus-correctness boundary
/// in one place (the caller's `sync_batch_verified_*` function) and
/// avoids spreading tentative-state mutation across multiple async
/// tasks.
///
/// Reference: upstream `Ouroboros.Network.BlockFetch.State.completeBlockDownload`
/// ordering invariants; `Ouroboros.Network.BlockFetch.ClientRegistry`
/// per-peer dispatch.
pub async fn execute_multi_peer_blockfetch_plan<B, F, Fut>(
    plan: &[BlockFetchAssignment],
    from_point: Point,
    fetch_one: F,
    pool_instr: Option<&BlockFetchInstrumentation>,
) -> Result<Vec<(Vec<u8>, B)>, SyncError>
where
    B: Send + 'static,
    F: Fn(SocketAddr, Point, Point) -> Fut + Send + Sync + Clone + 'static,
    Fut: std::future::Future<Output = Result<Vec<(Vec<u8>, B)>, SyncError>> + Send + 'static,
{
    use yggdrasil_network::blockfetch_pool::ReorderBuffer;

    if plan.is_empty() {
        return Ok(Vec::new());
    }

    // The `ReorderBuffer` only releases chunks whose lower-slot is
    // strictly past its head, and treats `Point::Origin` as
    // "never releasable".  At genesis (`from_point = Origin`), the
    // multi-peer reassembly path therefore cannot drain.  Reject
    // genesis multi-peer plans explicitly so the caller routes to
    // the single-peer path for initial sync; collapse-by-truncation
    // would silently drop chunks past plan[0], which is wrong.
    if plan.len() > 1 && matches!(from_point, Point::Origin) {
        return Err(SyncError::Recovery(
            "multi-peer BlockFetch dispatch requires non-Origin from_point; \
             genesis bootstrap must use single-peer path"
                .to_owned(),
        ));
    }

    // Single-peer fast path: the legacy single-element plan is
    // bit-identical to a direct fetch — no JoinSet machinery, no
    // reorder buffer.
    if plan.len() == 1 {
        let asn = plan[0];
        if let Some(pool) = pool_instr {
            if let Ok(mut g) = pool.lock() {
                g.note_dispatch(asn.peer);
            }
        }
        let result = fetch_one(asn.peer, asn.lower, asn.upper).await;
        match &result {
            Ok(blocks) => {
                if let Some(pool) = pool_instr {
                    if let Ok(mut g) = pool.lock() {
                        let n = blocks.len() as u64;
                        let bytes: u64 = blocks.iter().map(|(raw, _)| raw.len() as u64).sum();
                        g.note_success(asn.peer, n, bytes, Instant::now());
                    }
                }
            }
            Err(_) => {
                if let Some(pool) = pool_instr {
                    if let Ok(mut g) = pool.lock() {
                        g.note_failure(asn.peer);
                    }
                }
            }
        }
        return result;
    }

    // Multi-peer dispatch.  Record dispatch up front so per-peer
    // in-flight accounting reflects all outstanding fetches before any
    // future polls.
    if let Some(pool) = pool_instr {
        if let Ok(mut g) = pool.lock() {
            for asn in plan {
                g.note_dispatch(asn.peer);
            }
        }
    }

    let mut joinset = tokio::task::JoinSet::new();
    for asn in plan {
        let asn = *asn;
        let f = fetch_one.clone();
        joinset.spawn(async move {
            let res = f(asn.peer, asn.lower, asn.upper).await;
            (asn, res)
        });
    }

    // The `ReorderBuffer` releases chunks whose lower-slot is STRICTLY
    // greater than the head's slot.  `from_point` is the last applied
    // block (its slot equals the first chunk's lower slot), so seed
    // the buffer with `previous_point(from_point)` so the first chunk
    // releases naturally.  At genesis the multi-peer plan already
    // returned an explicit error above, so `from_point` is guaranteed
    // to be a real `BlockPoint` here.
    let head_seed = match from_point {
        Point::Origin => Point::Origin,
        Point::BlockPoint(slot, hash) => {
            if slot.0 == 0 {
                Point::BlockPoint(yggdrasil_ledger::SlotNo(0), hash)
            } else {
                Point::BlockPoint(yggdrasil_ledger::SlotNo(slot.0 - 1), hash)
            }
        }
    };
    let mut buffer: ReorderBuffer<(Vec<u8>, B)> = ReorderBuffer::new(head_seed);

    while let Some(joined) = joinset.join_next().await {
        let (asn, result) = match joined {
            Ok(pair) => pair,
            Err(join_err) => {
                joinset.abort_all();
                return Err(SyncError::Recovery(format!(
                    "multi-peer fetch task panicked: {join_err}"
                )));
            }
        };
        match result {
            Ok(blocks) => {
                if let Some(pool) = pool_instr {
                    if let Ok(mut g) = pool.lock() {
                        let n = blocks.len() as u64;
                        let bytes: u64 = blocks.iter().map(|(raw, _)| raw.len() as u64).sum();
                        g.note_success(asn.peer, n, bytes, Instant::now());
                    }
                }
                buffer.insert(asn.lower, asn.upper, blocks);
            }
            Err(err) => {
                if let Some(pool) = pool_instr {
                    if let Ok(mut g) = pool.lock() {
                        g.note_failure(asn.peer);
                    }
                }
                joinset.abort_all();
                return Err(err);
            }
        }
    }

    // Drain in chain (slot-ascending) order.
    let mut out = Vec::new();
    for (_lower, _upper, blocks) in buffer.drain_releasable() {
        out.extend(blocks);
    }
    Ok(out)
}

/// Sequential variant of [`execute_multi_peer_blockfetch_plan`] that
/// dispatches assignments inline (no `tokio::spawn`).  Trades parallel
/// throughput for borrow-checker simplicity: the closure may be
/// `FnMut` and may capture mutable references into a borrowed peer
/// slice, so callers can pass `&mut [(SocketAddr, &mut BlockFetchClient)]`
/// directly without `Arc<tokio::sync::Mutex<BlockFetchClient>>` wrappers
/// or per-peer worker tasks.
///
/// Behaviour matches the parallel dispatcher's contract:
/// - Empty plan → `Ok(empty)`.
/// - Multi-element plan with `from_point = Origin` → explicit error
///   (genesis bootstrap must use the single-peer path; the
///   `ReorderBuffer` cannot release with `head = Origin`).
/// - On any chunk error: short-circuit, propagate the error, record
///   `note_failure(peer)` against the offending peer.  Subsequent
///   assignments are skipped — the inline variant has no spawned
///   tasks to abort.
/// - On success: returns blocks reassembled in chain order via
///   [`yggdrasil_network::blockfetch_pool::ReorderBuffer`].
///
/// This is the runtime-friendly executor that Phase 6 step 2 of
/// [`docs/ARCHITECTURE.md`] will consume from
/// `sync_batch_verified_with_tentative` once the sync-loop branching
/// lands.  Real parallelism (Phase 6 step 3) comes when the runtime
/// adopts the per-peer worker-task pattern; the API surface here
/// stays as the simpler-by-default path operators can opt into
/// without restructuring connection lifecycle.
///
/// Reference: same `Ouroboros.Network.BlockFetch.State.completeBlockDownload`
/// ordering invariants as the parallel dispatcher; the inline form
/// mirrors upstream `BlockFetch.Client.fetchClient` running directly
/// against per-peer `FetchClientStateVars` without an intermediate
/// scheduler thread.
pub async fn execute_multi_peer_blockfetch_plan_inline<B, F, Fut>(
    plan: &[BlockFetchAssignment],
    from_point: Point,
    mut fetch_one: F,
    pool_instr: Option<&BlockFetchInstrumentation>,
) -> Result<Vec<(Vec<u8>, B)>, SyncError>
where
    F: FnMut(SocketAddr, Point, Point) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<(Vec<u8>, B)>, SyncError>>,
{
    use yggdrasil_network::blockfetch_pool::ReorderBuffer;

    if plan.is_empty() {
        return Ok(Vec::new());
    }

    if plan.len() > 1 && matches!(from_point, Point::Origin) {
        return Err(SyncError::Recovery(
            "multi-peer BlockFetch dispatch requires non-Origin from_point; \
             genesis bootstrap must use single-peer path"
                .to_owned(),
        ));
    }

    if plan.len() == 1 {
        let asn = plan[0];
        if let Some(pool) = pool_instr {
            if let Ok(mut g) = pool.lock() {
                g.note_dispatch(asn.peer);
            }
        }
        let result = fetch_one(asn.peer, asn.lower, asn.upper).await;
        match &result {
            Ok(blocks) => {
                if let Some(pool) = pool_instr {
                    if let Ok(mut g) = pool.lock() {
                        let n = blocks.len() as u64;
                        let bytes: u64 = blocks.iter().map(|(raw, _)| raw.len() as u64).sum();
                        g.note_success(asn.peer, n, bytes, Instant::now());
                    }
                }
            }
            Err(_) => {
                if let Some(pool) = pool_instr {
                    if let Ok(mut g) = pool.lock() {
                        g.note_failure(asn.peer);
                    }
                }
            }
        }
        return result;
    }

    // Sequential multi-peer dispatch.  Each chunk is awaited fully
    // before moving to the next; the dispatcher remains correct in
    // the face of borrowed-state closures and out-of-order arrival
    // (the iteration order matches plan order, which matches chain
    // order, so the buffer trivially releases each chunk on insert).
    let head_seed = match from_point {
        Point::Origin => Point::Origin,
        Point::BlockPoint(slot, hash) => {
            if slot.0 == 0 {
                Point::BlockPoint(yggdrasil_ledger::SlotNo(0), hash)
            } else {
                Point::BlockPoint(yggdrasil_ledger::SlotNo(slot.0 - 1), hash)
            }
        }
    };
    let mut buffer: ReorderBuffer<(Vec<u8>, B)> = ReorderBuffer::new(head_seed);

    for asn in plan {
        if let Some(pool) = pool_instr {
            if let Ok(mut g) = pool.lock() {
                g.note_dispatch(asn.peer);
            }
        }
        match fetch_one(asn.peer, asn.lower, asn.upper).await {
            Ok(blocks) => {
                if let Some(pool) = pool_instr {
                    if let Ok(mut g) = pool.lock() {
                        let n = blocks.len() as u64;
                        let bytes: u64 = blocks.iter().map(|(raw, _)| raw.len() as u64).sum();
                        g.note_success(asn.peer, n, bytes, Instant::now());
                    }
                }
                buffer.insert(asn.lower, asn.upper, blocks);
            }
            Err(err) => {
                if let Some(pool) = pool_instr {
                    if let Ok(mut g) = pool.lock() {
                        g.note_failure(asn.peer);
                    }
                }
                return Err(err);
            }
        }
    }

    let mut out = Vec::new();
    for (_lower, _upper, blocks) in buffer.drain_releasable() {
        out.extend(blocks);
    }
    Ok(out)
}

/// Runtime-side dispatch context for the multi-peer BlockFetch path.
///
/// When passed to `sync_batch_verified_with_tentative` as
/// `Some(...)`, AND the underlying [`crate::runtime::SharedFetchWorkerPool`]
/// has at least two registered workers, AND
/// `effective_block_fetch_concurrency(workers, max_knob) > 1`, the
/// per-RollForward fetch dispatches through the pool's `dispatch_plan`
/// instead of the direct `block_fetch` reference.  Otherwise the
/// legacy single-peer path runs unchanged.
///
/// This is the runtime-level wire of Phase 6 (see
/// `docs/ARCHITECTURE.md`).  The pool is populated by the governor
/// task via `OutboundPeerManager::migrate_session_to_worker` and
/// read here under a brief `tokio::sync::RwLock::read` guard.
pub struct MultiPeerDispatchContext<'a> {
    /// Shared per-peer worker pool.  Cloned `Arc` from runtime
    /// startup; both the governor (writer) and this context (reader)
    /// hold their own clones.
    pub pool: &'a crate::runtime::SharedFetchWorkerPool,
    /// Operator-configured upper bound on concurrent BlockFetch
    /// peers (`max_concurrent_block_fetch_peers` from
    /// `NodeConfigFile`).  When `<= 1`, the multi-peer branch is
    /// not taken even if the pool has multiple workers.
    pub max_concurrent_knob: u8,
    /// Round 151 — shared candidate-fragment registry populated by
    /// the verified-sync loop's RollForward observations.  When
    /// `Some`, `partition_fetch_range_with_candidate_fragments`
    /// resolves `split_range`'s placeholder hashes against
    /// per-peer announced points before issuing
    /// `MsgRequestRange` — the upstream
    /// `Ouroboros.Network.BlockFetch.Decision.fetchDecisions`
    /// analogue.  When `None`, the existing placeholder-collapse
    /// fallback in `partition_fetch_range_across_peers` runs.
    pub chainsync_pool: Option<&'a crate::chainsync_worker::SharedChainSyncWorkerPool>,
}

/// Per-RollForward integration helper that binds the dispatcher
/// (`execute_multi_peer_blockfetch_plan`) to the consensus-correctness
/// invariants of the legacy single-peer path
/// (`sync_batch_verified_with_tentative`).
///
/// Workflow:
///
/// 1. If `tentative_state` is `Some` and `header` decodes to a
///    `Point::BlockPoint`, call `try_set_tentative_header(state, header)`.
///    Record whether the announcement actually happened so the cleanup
///    branch fires only when a trap was set.
/// 2. Compute the per-peer plan via
///    [`partition_fetch_range_across_peers`].
/// 3. Dispatch via [`execute_multi_peer_blockfetch_plan`].
/// 4. On `Err`: if the tentative was set, call `clear_tentative_trap`
///    so the chain-selection state machine reverts to the pre-announce
///    state. Then propagate the error.
/// 5. On `Ok`: return the chain-ordered blocks.
///
/// The function is generic over the block type `B` so unit tests use
/// `u64` placeholders without needing real `BlockFetchClient` mocking.
/// Production callers parameterise as `MultiEraBlock` and pass a real
/// fetch closure that wraps `fetch_range_blocks_multi_era_raw_decoded`.
///
/// Reference: same tentative-header timing contract as
/// `sync_batch_verified_with_tentative`.  The dispatcher itself is
/// tentative-state-agnostic so the announce/cleanup pair is enforced
/// in this single layer rather than fanned out across async tasks.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_range_with_tentative<B, F, Fut>(
    header: &[u8],
    tip: Point,
    from_point: Point,
    peers: &[SocketAddr],
    max_concurrent_knob: u8,
    tentative_state: Option<&Arc<RwLock<TentativeState>>>,
    pool_instr: Option<&BlockFetchInstrumentation>,
    fetch_one: F,
) -> Result<Vec<(Vec<u8>, B)>, SyncError>
where
    B: Send + 'static,
    F: Fn(SocketAddr, Point, Point) -> Fut + Send + Sync + Clone + 'static,
    Fut: std::future::Future<Output = Result<Vec<(Vec<u8>, B)>, SyncError>> + Send + 'static,
{
    // Resolve the upper-bound point: prefer the decoded header point,
    // fall back to the ChainSync `tip` if the header doesn't carry one
    // — same precedence as `sync_batch_verified_with_tentative`.
    let header_point = point_from_raw_header(header);
    let range_upper = header_point.unwrap_or(tip);
    let effective_range = normalize_blockfetch_range_points(from_point, range_upper);

    // Announce the tentative header before any fetch dispatches.
    // Mirrors upstream `cdbTentativeHeader` semantics where the trap
    // is set on the candidate header, then cleared if the body fetch
    // fails.
    let tentative_set =
        tentative_state.is_some_and(|state| try_set_tentative_header(state, header));

    let Some((lower, upper)) = effective_range else {
        // Empty range — nothing to fetch, no tentative state changes
        // needed beyond the announcement (which the caller treats as
        // adopted on the next ChainSync iteration).
        return Ok(Vec::new());
    };

    let plan = partition_fetch_range_across_peers(lower, upper, peers, max_concurrent_knob);
    let result = execute_multi_peer_blockfetch_plan(&plan, from_point, fetch_one, pool_instr).await;

    if result.is_err() && tentative_set {
        if let Some(state) = tentative_state {
            clear_tentative_trap(state);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_consensus::EpochSize;
    use yggdrasil_ledger::{
        PoolKeyHash, PoolParams, RewardAccount, StakeCredential, StakeSnapshot, UnitInterval,
    };

    fn pool_hash(seed: u8) -> PoolKeyHash {
        [seed; 28]
    }

    fn stake_cred(seed: u8) -> StakeCredential {
        StakeCredential::AddrKeyHash([seed; 28])
    }

    /// Build a snapshot where each pool has the specified stake via a
    /// dedicated fake credential.
    fn make_snapshot_with_pools(pools: &[(PoolKeyHash, u64)]) -> StakeSnapshot {
        let mut snapshot = StakeSnapshot::default();
        for (i, (hash, amount)) in pools.iter().enumerate() {
            let cred = stake_cred(100 + i as u8);
            let params = PoolParams {
                operator: *hash,
                vrf_keyhash: [0u8; 32],
                pledge: 0,
                cost: 0,
                margin: UnitInterval {
                    numerator: 0,
                    denominator: 1,
                },
                reward_account: RewardAccount {
                    network: 0,
                    credential: StakeCredential::AddrKeyHash([0u8; 28]),
                },
                pool_owners: vec![],
                relays: vec![],
                pool_metadata: None,
            };
            snapshot.pool_params.insert(*hash, params);
            snapshot.delegations.insert(cred, *hash);
            snapshot.stake.add(cred, *amount);
        }
        snapshot
    }

    #[test]
    fn near_future_wait_duration_until_slot_at_returns_delta_to_boundary() {
        // system_start=1000, slot_length=2s, target slot 8 starts at 1016.
        let wait = near_future_wait_duration_until_slot_at(1010.5, 1000.0, 2.0, SlotNo(8))
            .expect("wait duration");
        assert_eq!(wait, std::time::Duration::from_secs_f64(5.5));
    }

    // -----------------------------------------------------------------------
    // Slice E — Multi-peer concurrent BlockFetch dispatch primitives
    //
    // Reference: upstream `Ouroboros.Network.BlockFetch.Decision` and the
    // existing `crates/network/src/blockfetch_pool.rs::split_range` helper.
    // -----------------------------------------------------------------------

    fn test_addr(port: u16) -> SocketAddr {
        SocketAddr::V4(std::net::SocketAddrV4::new(
            std::net::Ipv4Addr::LOCALHOST,
            port,
        ))
    }

    fn block_point(slot: u64) -> Point {
        // Non-zero placeholder so the point can never be confused with
        // the all-zeros sentinel that
        // `yggdrasil_network::blockfetch_pool::split_range` synthesises
        // for intermediate chunk boundaries (see
        // `point_carries_placeholder_hash`).  Derived deterministically
        // from `slot` so distinct slots produce distinct hashes; the
        // first byte is forced non-zero to guarantee `hash != [0; 32]`
        // even at slot 0.
        let mut hash = [0u8; 32];
        let bytes = slot.to_le_bytes();
        hash[..bytes.len()].copy_from_slice(&bytes);
        hash[31] = 0xff;
        Point::BlockPoint(SlotNo(slot), HeaderHash(hash))
    }

    #[test]
    fn effective_concurrency_zero_knob_returns_one() {
        // knob = 0 must collapse to single-peer dispatch — preserves
        // the legacy behaviour for operators who explicitly disable.
        assert_eq!(effective_block_fetch_concurrency(0, 5), 1);
    }

    #[test]
    fn effective_concurrency_default_knob_is_one() {
        // The shipped default (`max_concurrent_block_fetch_peers = 1`)
        // must keep single-peer dispatch.  Pinned because changing the
        // default elsewhere without re-anchoring this test would silently
        // start parallelising production fetches.
        assert_eq!(effective_block_fetch_concurrency(1, 5), 1);
    }

    #[test]
    fn effective_concurrency_clamps_to_peer_count() {
        // knob > peers.len() must clamp to peers.len() so the dispatcher
        // can never index past the slice end.
        assert_eq!(effective_block_fetch_concurrency(10, 3), 3);
    }

    #[test]
    fn effective_concurrency_uses_full_knob_within_peer_count() {
        // knob ≤ peers.len() returns the knob unchanged.
        assert_eq!(effective_block_fetch_concurrency(2, 5), 2);
    }

    #[test]
    fn effective_concurrency_with_no_peers_returns_one() {
        // Empty peer slice falls back to the single-peer code path
        // (callers must check assignments are non-empty before
        // dispatching).
        assert_eq!(effective_block_fetch_concurrency(5, 0), 1);
    }

    #[test]
    fn partition_with_no_peers_is_empty() {
        let assignments =
            partition_fetch_range_across_peers(block_point(100), block_point(200), &[], 5);
        assert!(assignments.is_empty());
    }

    #[test]
    fn partition_with_single_peer_returns_one_assignment() {
        let peers = vec![test_addr(1001)];
        let assignments =
            partition_fetch_range_across_peers(block_point(100), block_point(200), &peers, 5);
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].peer, peers[0]);
        assert_eq!(assignments[0].lower, block_point(100));
        assert_eq!(assignments[0].upper, block_point(200));
    }

    #[test]
    fn partition_default_knob_uses_first_peer_only() {
        // Default knob (1) with three peers must produce a single
        // assignment to the *first* peer carrying the full range —
        // matches the legacy single-peer code path bit-for-bit.
        let peers = vec![test_addr(1001), test_addr(1002), test_addr(1003)];
        let assignments =
            partition_fetch_range_across_peers(block_point(100), block_point(200), &peers, 1);
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].peer, peers[0]);
        assert_eq!(assignments[0].lower, block_point(100));
        assert_eq!(assignments[0].upper, block_point(200));
    }

    #[test]
    fn partition_with_two_peers_collapses_to_single_chunk_when_split_produces_placeholder_hashes() {
        // Round 144 follow-up — closes the runtime half of Round 91
        // Gap BN.  `split_range(BlockPoint(100), BlockPoint(200), 2)`
        // returns two chunks with synthesised `[0; 32]` placeholder
        // hashes on the intermediate boundary; the runtime cannot
        // resolve them to real chain points, and peers respond
        // `NoBlocks` for unknown-hash bounds, so the multi-peer
        // dispatch path silently drops every block.  The guard in
        // `partition_fetch_range_across_peers` collapses the plan to
        // a single chunk against `peers[0]` whenever any chunk
        // boundary carries the placeholder hash; the original
        // endpoints are preserved exactly so `MsgRequestRange` still
        // requests the full range from one peer.
        //
        // Reference: `docs/MANUAL_TEST_RUNBOOK.md` §6.5a operational
        // confirmation that the wire-level request body
        // `8300821853...821904635820 0000...` (placeholder upper
        // hash) was returning `NoBlocks` and silently producing
        // empty volatile storage.
        let peers = vec![test_addr(1001), test_addr(1002)];
        let assignments =
            partition_fetch_range_across_peers(block_point(100), block_point(200), &peers, 2);
        assert_eq!(
            assignments.len(),
            1,
            "split_range produces a placeholder boundary hash for any \
             multi-chunk plan whose intermediate slot is not a real chain \
             point — the runtime collapses to a single chunk so peers see \
             only known-hash bounds",
        );
        assert_eq!(assignments[0].peer, peers[0]);
        assert_eq!(
            assignments[0].lower,
            block_point(100),
            "lower endpoint preserved exactly",
        );
        assert_eq!(
            assignments[0].upper,
            block_point(200),
            "upper endpoint preserved exactly",
        );
    }

    #[test]
    fn partition_collapses_only_when_chunks_actually_carry_placeholders() {
        // Sanity pin: when `n_chunks == 1` the helper does NOT trigger
        // (single-chunk output uses real lower/upper from the input,
        // no placeholder synthesis).  This guards against an
        // overzealous future refactor that always collapses.
        let peers = vec![test_addr(1001), test_addr(1002)];
        let assignments =
            partition_fetch_range_across_peers(block_point(100), block_point(200), &peers, 1);
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].peer, peers[0]);
        assert!(
            !point_carries_placeholder_hash(&assignments[0].lower),
            "single-chunk lower must be the real input point",
        );
        assert!(
            !point_carries_placeholder_hash(&assignments[0].upper),
            "single-chunk upper must be the real input point",
        );
    }

    /// Round 150 — `partition_fetch_range_with_candidate_fragments`
    /// resolves `split_range`'s synthetic placeholder hashes against
    /// per-peer candidate fragments from a
    /// [`ChainSyncWorkerPool`](crate::chainsync_worker::ChainSyncWorkerPool).
    /// When every intermediate boundary is announced by at least one
    /// peer, the planner returns a real-hash multi-chunk plan that
    /// the BlockFetch wire layer can dispatch in parallel.
    #[tokio::test]
    async fn partition_with_candidate_fragments_resolves_placeholder_hashes() {
        use crate::chainsync_worker::{ChainSyncWorkerHandle, new_shared_chainsync_worker_pool};
        use yggdrasil_ledger::HeaderHash;
        let p1 = test_addr(1001);
        let p2 = test_addr(1002);
        let peers = vec![p1, p2];

        // Build a pool where each peer has announced the would-be
        // placeholder slots.
        let pool = new_shared_chainsync_worker_pool();
        {
            let mut g = pool.write().await;
            let h1 = ChainSyncWorkerHandle::spawn(p1, |_| async { None });
            let h2 = ChainSyncWorkerHandle::spawn(p2, |_| async { None });
            // Pre-seed candidate fragments with the slot 150 boundary
            // that `split_range(100, 200, 2)` would synthesise as a
            // placeholder.
            h1.fragment()
                .write()
                .await
                .push_announced(SlotNo(150), HeaderHash([0xaa; 32]));
            h1.fragment()
                .write()
                .await
                .push_announced(SlotNo(151), HeaderHash([0xbb; 32]));
            h2.fragment()
                .write()
                .await
                .push_announced(SlotNo(150), HeaderHash([0xaa; 32]));
            g.register(h1);
            g.register(h2);
        }

        let assignments = partition_fetch_range_with_candidate_fragments(
            block_point(100),
            block_point(200),
            &peers,
            2,
            &pool,
        )
        .await
        .expect("placeholder slots are resolvable");
        assert_eq!(assignments.len(), 2);
        // Every boundary in the resulting plan must have a real
        // (non-zero) hash.
        for asn in &assignments {
            for endpoint in [asn.lower, asn.upper] {
                if let yggdrasil_ledger::Point::BlockPoint(_, hash) = endpoint {
                    assert_ne!(
                        hash.0, [0u8; 32],
                        "candidate-fragment lookup must replace placeholder hash"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn partition_with_candidate_fragments_falls_back_when_unresolvable() {
        use crate::chainsync_worker::new_shared_chainsync_worker_pool;
        let pool = new_shared_chainsync_worker_pool();
        let peers = vec![test_addr(1001), test_addr(1002)];
        // Empty pool — no peer has announced anything; placeholder
        // slots cannot be resolved.
        let result = partition_fetch_range_with_candidate_fragments(
            block_point(100),
            block_point(200),
            &peers,
            2,
            &pool,
        )
        .await;
        assert!(
            result.is_none(),
            "empty pool can't resolve placeholders → caller must use single-chunk fallback",
        );
    }

    #[test]
    fn point_carries_placeholder_hash_recognises_split_range_synthetic_boundary() {
        // The placeholder is the exact hash that
        // `yggdrasil_network::blockfetch_pool::split_range` produces
        // for intermediate chunk boundaries.  Real chain hashes
        // (Origin or BlockPoint with non-zero hash) must never match.
        use yggdrasil_ledger::HeaderHash;
        let placeholder = Point::BlockPoint(SlotNo(1234), HeaderHash([0u8; 32]));
        let real = Point::BlockPoint(SlotNo(1234), HeaderHash([0xab; 32]));
        assert!(point_carries_placeholder_hash(&placeholder));
        assert!(!point_carries_placeholder_hash(&real));
        assert!(!point_carries_placeholder_hash(&Point::Origin));
    }

    // -----------------------------------------------------------------------
    // Slice GD-RT — ChainSync header density observation runtime hook
    // -----------------------------------------------------------------------

    #[test]
    fn new_density_registry_starts_empty() {
        let r = new_density_registry();
        let guard = r.read().unwrap();
        assert!(guard.is_empty());
    }

    #[test]
    fn observe_creates_window_on_first_call() {
        let r = new_density_registry();
        let peer = test_addr(2001);
        assert!(observe_chain_sync_header_density(peer, SlotNo(100), &r));
        let guard = r.read().unwrap();
        let w = guard.get(&peer).expect("window should be created");
        assert_eq!(w.headers_seen(), 1);
        assert_eq!(w.last_slot(), Some(SlotNo(100)));
    }

    #[test]
    fn observe_accumulates_into_existing_window() {
        let r = new_density_registry();
        let peer = test_addr(2002);
        observe_chain_sync_header_density(peer, SlotNo(10), &r);
        observe_chain_sync_header_density(peer, SlotNo(20), &r);
        observe_chain_sync_header_density(peer, SlotNo(30), &r);
        let guard = r.read().unwrap();
        let w = guard.get(&peer).expect("window present");
        assert_eq!(w.headers_seen(), 3);
        assert_eq!(w.last_slot(), Some(SlotNo(30)));
    }

    #[test]
    fn observe_isolates_peers() {
        // Independent peers must accumulate independently — chain density
        // is a per-peer signal.
        let r = new_density_registry();
        let p1 = test_addr(2101);
        let p2 = test_addr(2102);
        observe_chain_sync_header_density(p1, SlotNo(5), &r);
        observe_chain_sync_header_density(p2, SlotNo(7), &r);
        observe_chain_sync_header_density(p2, SlotNo(8), &r);
        let guard = r.read().unwrap();
        assert_eq!(guard.get(&p1).unwrap().headers_seen(), 1);
        assert_eq!(guard.get(&p2).unwrap().headers_seen(), 2);
    }

    #[test]
    fn observe_rejects_slot_regression() {
        let r = new_density_registry();
        let peer = test_addr(2003);
        observe_chain_sync_header_density(peer, SlotNo(50), &r);
        // Lower slot must be rejected.
        assert!(!observe_chain_sync_header_density(peer, SlotNo(40), &r));
        let guard = r.read().unwrap();
        let w = guard.get(&peer).unwrap();
        assert_eq!(w.headers_seen(), 1);
        assert_eq!(w.last_slot(), Some(SlotNo(50)));
    }

    #[test]
    fn read_peer_density_returns_zero_for_unknown_peer() {
        let r = new_density_registry();
        assert_eq!(read_peer_density(test_addr(9999), &r), 0.0);
    }

    #[test]
    fn read_peer_density_matches_window_density() {
        let r = new_density_registry();
        let peer = test_addr(2004);
        // 100 observations against the default 6480-slot window:
        // density = 100 / 6480 ≈ 0.0154.
        for s in 0..100 {
            observe_chain_sync_header_density(peer, SlotNo(s), &r);
        }
        let d = read_peer_density(peer, &r);
        assert!(d > 0.0 && d < 0.05, "density should be roughly 100/6480");
    }

    #[test]
    fn forget_peer_density_removes_window() {
        let r = new_density_registry();
        let peer = test_addr(2005);
        observe_chain_sync_header_density(peer, SlotNo(10), &r);
        forget_peer_density(peer, &r);
        let guard = r.read().unwrap();
        assert!(guard.get(&peer).is_none());
    }

    #[test]
    fn forget_peer_density_unknown_peer_is_noop() {
        // Forgetting a peer that never had a window must be a safe no-op
        // — the runtime calls forget_peer_density on every disconnect,
        // including ones that never produced any RollForward headers.
        let r = new_density_registry();
        forget_peer_density(test_addr(9999), &r);
        let guard = r.read().unwrap();
        assert!(guard.is_empty());
    }

    // -----------------------------------------------------------------------
    // Slice E-Dispatch — execute_multi_peer_blockfetch_plan
    // -----------------------------------------------------------------------

    /// Synthetic block placeholder for dispatcher tests — the
    /// dispatcher only counts and orders by chunk lower-bound, so the
    /// block contents are irrelevant.  Generic `B = u64` keeps the
    /// tests free of any real block-decoding ceremony.
    fn fake_block(slot: u64) -> (Vec<u8>, u64) {
        (vec![slot as u8; 4], slot)
    }

    type SynthFut = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<(Vec<u8>, u64)>, SyncError>> + Send>,
    >;

    fn synthetic_fetch_one(
        contents: std::collections::BTreeMap<SocketAddr, Vec<(Point, Point, Vec<u64>)>>,
    ) -> impl Fn(SocketAddr, Point, Point) -> SynthFut + Clone + Send + Sync + 'static {
        let contents = std::sync::Arc::new(contents);
        move |peer, lower, upper| {
            let contents = contents.clone();
            Box::pin(async move {
                let entries = contents.get(&peer).cloned().unwrap_or_default();
                for (l, u, slots) in entries {
                    if l == lower && u == upper {
                        return Ok(slots.into_iter().map(fake_block).collect());
                    }
                }
                Ok(Vec::new())
            })
        }
    }

    fn failing_fetch_one(
        target_peer: SocketAddr,
    ) -> impl Fn(SocketAddr, Point, Point) -> SynthFut + Clone + Send + Sync + 'static {
        move |peer, _lower, _upper| {
            let fail = peer == target_peer;
            Box::pin(async move {
                if fail {
                    Err(SyncError::Recovery(format!(
                        "synthetic peer {peer} failure"
                    )))
                } else {
                    // Slow path so failure can race ahead — used by the
                    // sibling-cancellation test.
                    tokio::task::yield_now().await;
                    Ok(Vec::new())
                }
            })
        }
    }

    #[tokio::test]
    async fn empty_plan_yields_empty_output() {
        let empty = std::collections::BTreeMap::<SocketAddr, Vec<(Point, Point, Vec<u64>)>>::new();
        let result = execute_multi_peer_blockfetch_plan(
            &[],
            block_point(100),
            synthetic_fetch_one(empty),
            None,
        )
        .await
        .expect("empty plan succeeds");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn genesis_multi_peer_returns_explicit_error() {
        // Multi-element plan from Origin must reject — caller should
        // route initial sync to single-peer.
        let plan = vec![
            BlockFetchAssignment {
                peer: test_addr(1001),
                lower: Point::Origin,
                upper: block_point(50),
            },
            BlockFetchAssignment {
                peer: test_addr(1002),
                lower: block_point(51),
                upper: block_point(100),
            },
        ];
        let empty = std::collections::BTreeMap::<SocketAddr, Vec<(Point, Point, Vec<u64>)>>::new();
        let err = execute_multi_peer_blockfetch_plan(
            &plan,
            Point::Origin,
            synthetic_fetch_one(empty),
            None,
        )
        .await
        .expect_err("genesis multi-peer must error");
        assert!(matches!(err, SyncError::Recovery(_)));
    }

    #[tokio::test]
    async fn single_peer_plan_is_bit_identical_to_direct_fetch() {
        // Single-element plan must take the legacy fast path — no
        // ReorderBuffer involvement.  The output equals what the
        // closure returned for that single chunk.
        let peer = test_addr(2001);
        let plan = vec![BlockFetchAssignment {
            peer,
            lower: block_point(100),
            upper: block_point(150),
        }];

        let mut contents = std::collections::BTreeMap::new();
        contents.insert(
            peer,
            vec![(block_point(100), block_point(150), vec![101, 102, 103])],
        );

        let result = execute_multi_peer_blockfetch_plan(
            &plan,
            block_point(99),
            synthetic_fetch_one(contents),
            None,
        )
        .await
        .expect("single-peer plan succeeds");
        assert_eq!(result.len(), 3);
    }

    #[tokio::test]
    async fn two_peer_plan_releases_blocks_in_chain_order() {
        let p1 = test_addr(2101);
        let p2 = test_addr(2102);
        let plan = vec![
            BlockFetchAssignment {
                peer: p1,
                lower: block_point(100),
                upper: block_point(150),
            },
            BlockFetchAssignment {
                peer: p2,
                lower: block_point(151),
                upper: block_point(200),
            },
        ];
        let mut contents = std::collections::BTreeMap::new();
        contents.insert(
            p1,
            vec![(block_point(100), block_point(150), vec![101, 110, 150])],
        );
        contents.insert(
            p2,
            vec![(block_point(151), block_point(200), vec![160, 180, 200])],
        );

        let result = execute_multi_peer_blockfetch_plan(
            &plan,
            block_point(99),
            synthetic_fetch_one(contents),
            None,
        )
        .await
        .expect("two-peer plan succeeds");
        // 6 blocks total, drain order is chain-ascending.
        assert_eq!(result.len(), 6);
    }

    #[tokio::test]
    async fn any_chunk_failure_propagates_and_aborts_siblings() {
        // p2 returns Err immediately; p1 yields and returns Ok.  The
        // dispatcher must surface p2's error and abort p1.
        let p1 = test_addr(2201);
        let p2 = test_addr(2202);
        let plan = vec![
            BlockFetchAssignment {
                peer: p1,
                lower: block_point(100),
                upper: block_point(150),
            },
            BlockFetchAssignment {
                peer: p2,
                lower: block_point(151),
                upper: block_point(200),
            },
        ];
        let err =
            execute_multi_peer_blockfetch_plan(&plan, block_point(99), failing_fetch_one(p2), None)
                .await
                .expect_err("p2 failure must propagate");
        assert!(matches!(err, SyncError::Recovery(_)));
    }

    // -----------------------------------------------------------------------
    // Slice E-Tentative — dispatch_range_with_tentative timing tests
    //
    // Pin the consensus-correctness invariant: tentative is set BEFORE
    // dispatch, and the trap is cleared on ANY fetch error so the
    // chain-selection state machine doesn't carry a stale candidate
    // forward.
    // -----------------------------------------------------------------------

    fn fake_byron_header_with_slot(slot: u64) -> Vec<u8> {
        // For the dispatch tests we want `point_from_raw_header` to
        // return `None` so the caller's `tip` argument drives the
        // range-upper.  A single-byte break code (`0xff`) is invalid
        // as a CBOR data item header, causing every decode branch in
        // `point_from_raw_header` to fail cleanly.
        let _ = slot;
        vec![0xff]
    }

    #[tokio::test]
    async fn dispatch_range_clears_tentative_on_fetch_error() {
        // Failing fetch must trigger `clear_tentative_trap` so the
        // chain-selection state doesn't carry the stale candidate.
        let state = Arc::new(RwLock::new(TentativeState::default()));
        let p1 = test_addr(2401);
        let p2 = test_addr(2402);
        let peers = vec![p1, p2];

        // Use a fake header that doesn't decode (point_from_raw_header
        // returns None), so range_upper falls back to `tip`.  The
        // tentative announcement returns false in that case
        // (try_set_tentative_header gates on a decoded point), and
        // the cleanup branch correctly skips clearing — matching the
        // existing single-peer path behaviour for un-decodable
        // headers.  We verify that the dispatch error still
        // propagates.
        //
        // After the Round 144 follow-up, the two-peer plan collapses
        // to a single chunk against `peers[0]` because `split_range`
        // would otherwise produce placeholder hashes; target p1 (the
        // peer that actually receives the dispatch) so the failing
        // closure trips.
        let header = fake_byron_header_with_slot(150);
        let result: Result<Vec<(Vec<u8>, u64)>, SyncError> = dispatch_range_with_tentative(
            &header,
            block_point(150),
            block_point(99),
            &peers,
            2,
            Some(&state),
            None,
            failing_fetch_one(p1),
        )
        .await;

        assert!(matches!(result, Err(SyncError::Recovery(_))));
    }

    #[tokio::test]
    async fn dispatch_range_returns_blocks_in_order_on_success() {
        // After the Round 144 follow-up, two-peer plans whose
        // intermediate `split_range` boundary would carry a placeholder
        // hash collapse to a single-chunk plan against `peers[0]`
        // covering the full `(lower, upper)` range.  Verify the
        // integration: dispatch_range_with_tentative routes the full
        // range to peers[0]'s synthetic closure and returns every
        // delivered block.
        let p1 = test_addr(2501);
        let p2 = test_addr(2502);
        let peers = vec![p1, p2];

        let mut contents = std::collections::BTreeMap::new();
        contents.insert(
            p1,
            vec![(block_point(50), block_point(200), vec![60, 100, 150, 200])],
        );

        let header = fake_byron_header_with_slot(200);
        let result = dispatch_range_with_tentative::<u64, _, _>(
            &header,
            block_point(200),
            block_point(50),
            &peers,
            2,
            None,
            None,
            synthetic_fetch_one(contents),
        )
        .await
        .expect("two-peer collapsed-to-single-chunk dispatch succeeds");
        assert_eq!(result.len(), 4);
    }

    #[tokio::test]
    async fn dispatch_range_with_no_state_skips_tentative_handling() {
        // tentative_state = None must be a complete no-op for the
        // tentative-handling branch.  Verifies the helper doesn't
        // panic when the consensus-correctness invariant doesn't
        // apply (test environments without a chain-selection state
        // machine).
        let p1 = test_addr(2601);
        let mut contents = std::collections::BTreeMap::new();
        // Single-peer plan: the partition covers the full requested
        // range as one chunk, so the closure key matches the inputs
        // directly.
        contents.insert(
            p1,
            vec![(block_point(99), block_point(150), vec![100, 110])],
        );

        let header = fake_byron_header_with_slot(150);
        let result = dispatch_range_with_tentative::<u64, _, _>(
            &header,
            block_point(150),
            block_point(99),
            &[p1],
            1,
            None,
            None,
            synthetic_fetch_one(contents),
        )
        .await
        .expect("single-peer no-state dispatch");
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn inline_dispatcher_empty_plan_yields_empty_output() {
        let result: Result<Vec<(Vec<u8>, u64)>, SyncError> =
            execute_multi_peer_blockfetch_plan_inline(
                &[],
                block_point(100),
                |_addr, _lower, _upper| async { Ok(Vec::new()) },
                None,
            )
            .await;
        assert!(matches!(result, Ok(blocks) if blocks.is_empty()));
    }

    #[tokio::test]
    async fn inline_dispatcher_single_peer_works() {
        let p1 = test_addr(2801);
        let plan = vec![BlockFetchAssignment {
            peer: p1,
            lower: block_point(100),
            upper: block_point(200),
        }];
        let result = execute_multi_peer_blockfetch_plan_inline::<u64, _, _>(
            &plan,
            block_point(99),
            |_addr, _lower, _upper| async { Ok(vec![fake_block(150), fake_block(180)]) },
            None,
        )
        .await
        .expect("single-peer inline succeeds");
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn inline_dispatcher_multi_peer_releases_in_chain_order() {
        let p1 = test_addr(2901);
        let p2 = test_addr(2902);
        let plan = vec![
            BlockFetchAssignment {
                peer: p1,
                lower: block_point(50),
                upper: block_point(125),
            },
            BlockFetchAssignment {
                peer: p2,
                lower: block_point(126),
                upper: block_point(200),
            },
        ];

        // FnMut closure: capture a counter to verify each call is
        // distinct and the iteration is deterministic.
        let mut call_count = 0u32;
        let result = execute_multi_peer_blockfetch_plan_inline::<u64, _, _>(
            &plan,
            block_point(50),
            |peer, _lower, _upper| {
                call_count += 1;
                let blocks = if peer == p1 {
                    vec![fake_block(60), fake_block(100)]
                } else {
                    vec![fake_block(150), fake_block(200)]
                };
                async move { Ok(blocks) }
            },
            None,
        )
        .await
        .expect("multi-peer inline succeeds");
        assert_eq!(result.len(), 4);
        assert_eq!(call_count, 2);
    }

    #[tokio::test]
    async fn inline_dispatcher_short_circuits_on_error() {
        // First peer succeeds, second fails — the inline variant must
        // propagate the error without invoking subsequent peers.
        let p1 = test_addr(2950);
        let p2 = test_addr(2951);
        let p3 = test_addr(2952);
        let plan = vec![
            BlockFetchAssignment {
                peer: p1,
                lower: block_point(50),
                upper: block_point(100),
            },
            BlockFetchAssignment {
                peer: p2,
                lower: block_point(101),
                upper: block_point(150),
            },
            BlockFetchAssignment {
                peer: p3,
                lower: block_point(151),
                upper: block_point(200),
            },
        ];

        let mut call_log: Vec<SocketAddr> = Vec::new();
        let result: Result<Vec<(Vec<u8>, u64)>, SyncError> =
            execute_multi_peer_blockfetch_plan_inline(
                &plan,
                block_point(50),
                |peer, _lower, _upper| {
                    call_log.push(peer);
                    let fail = peer == p2;
                    async move {
                        if fail {
                            Err(SyncError::Recovery(format!("peer {peer} failed")))
                        } else {
                            Ok(vec![fake_block(0)])
                        }
                    }
                },
                None,
            )
            .await;

        assert!(matches!(result, Err(SyncError::Recovery(_))));
        // p3 must NOT have been invoked — short-circuit on p2's error.
        assert_eq!(call_log, vec![p1, p2]);
    }

    #[tokio::test]
    async fn inline_dispatcher_genesis_multi_peer_returns_explicit_error() {
        let plan = vec![
            BlockFetchAssignment {
                peer: test_addr(2980),
                lower: Point::Origin,
                upper: block_point(100),
            },
            BlockFetchAssignment {
                peer: test_addr(2981),
                lower: block_point(101),
                upper: block_point(200),
            },
        ];
        let err: Result<Vec<(Vec<u8>, u64)>, SyncError> =
            execute_multi_peer_blockfetch_plan_inline(
                &plan,
                Point::Origin,
                |_a, _l, _u| async { Ok(Vec::new()) },
                None,
            )
            .await;
        assert!(matches!(err, Err(SyncError::Recovery(_))));
    }

    #[tokio::test]
    async fn execute_plan_with_first_chunk_at_from_point_releases() {
        // Pinpoint test for the head_seed fix: with from_point at the
        // same slot as the first chunk's lower, the buffer must still
        // release.  This exercises the `previous_point(from_point)`
        // seed used by `execute_multi_peer_blockfetch_plan`.
        let p1 = test_addr(2701);
        let p2 = test_addr(2702);
        let plan = vec![
            BlockFetchAssignment {
                peer: p1,
                lower: block_point(50),
                upper: block_point(125),
            },
            BlockFetchAssignment {
                peer: p2,
                lower: block_point(126),
                upper: block_point(200),
            },
        ];
        let mut contents = std::collections::BTreeMap::new();
        contents.insert(p1, vec![(block_point(50), block_point(125), vec![60, 100])]);
        contents.insert(
            p2,
            vec![(block_point(126), block_point(200), vec![150, 200])],
        );

        let result = execute_multi_peer_blockfetch_plan(
            &plan,
            block_point(50),
            synthetic_fetch_one(contents),
            None,
        )
        .await
        .expect("plan must succeed");
        assert_eq!(result.len(), 4);
    }

    #[tokio::test]
    async fn dispatch_range_with_no_peers_returns_empty() {
        // Empty peer slice means no plan, which the dispatcher
        // returns as `Ok(empty)`.
        let empty = std::collections::BTreeMap::<SocketAddr, Vec<(Point, Point, Vec<u64>)>>::new();
        let header = fake_byron_header_with_slot(150);
        let result = dispatch_range_with_tentative::<u64, _, _>(
            &header,
            block_point(150),
            block_point(99),
            &[],
            1,
            None,
            None,
            synthetic_fetch_one(empty),
        )
        .await
        .expect("no-peers dispatch");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn three_peer_plan_handles_out_of_order_arrival() {
        // Block fetch closures complete in reverse order (p3 first, p1
        // last) — the ReorderBuffer must still produce chain-ascending
        // output.
        let p1 = test_addr(2301);
        let p2 = test_addr(2302);
        let p3 = test_addr(2303);
        let plan = vec![
            BlockFetchAssignment {
                peer: p1,
                lower: block_point(100),
                upper: block_point(150),
            },
            BlockFetchAssignment {
                peer: p2,
                lower: block_point(151),
                upper: block_point(200),
            },
            BlockFetchAssignment {
                peer: p3,
                lower: block_point(201),
                upper: block_point(250),
            },
        ];
        let mut contents = std::collections::BTreeMap::new();
        contents.insert(p1, vec![(block_point(100), block_point(150), vec![100])]);
        contents.insert(p2, vec![(block_point(151), block_point(200), vec![151])]);
        contents.insert(p3, vec![(block_point(201), block_point(250), vec![201])]);

        let result = execute_multi_peer_blockfetch_plan(
            &plan,
            block_point(99),
            synthetic_fetch_one(contents),
            None,
        )
        .await
        .expect("three-peer plan succeeds");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn partition_clamps_to_available_peers() {
        // `effective_block_fetch_concurrency(10, 2) == 2` — peer count
        // wins over knob.  After the Round 144 follow-up that guards
        // against `split_range` placeholder hashes, two-peer multi-chunk
        // plans collapse to a single chunk targeting `peers[0]`; assert
        // the effective concurrency cap directly so this test still
        // covers the clamp logic without depending on the
        // placeholder-aware partition collapse.
        assert_eq!(effective_block_fetch_concurrency(10, 2), 2);
        assert_eq!(effective_block_fetch_concurrency(10, 3), 3);
        assert_eq!(effective_block_fetch_concurrency(2, 5), 2);
    }

    #[test]
    fn near_future_wait_duration_until_slot_at_none_when_past_or_invalid() {
        assert!(near_future_wait_duration_until_slot_at(1020.0, 1000.0, 2.0, SlotNo(8)).is_none());
        assert!(near_future_wait_duration_until_slot_at(1010.0, 1000.0, 0.0, SlotNo(8)).is_none());
        assert!(near_future_wait_duration_until_slot_at(1010.0, 1000.0, -1.0, SlotNo(8)).is_none());
    }

    /// Pins the rollback-aware reset of `OcertCounters` in
    /// `update_ledger_checkpoint_after_progress`. Mirrors upstream
    /// `Cardano.Protocol.TPraos.API` `tickChainDepState` semantics
    /// where `PraosState.csCounters` is restored from the rollback's
    /// `ChainDepState` snapshot — without the reset, an alt chain that
    /// legitimately includes lower-sequence OpCerts from the same pool
    /// would be rejected as `OcertCounterTooOld`.
    #[test]
    fn update_ledger_checkpoint_after_progress_clears_ocert_counters_on_rollback() {
        use crate::plutus_eval::CekPlutusEvaluator;
        use yggdrasil_consensus::OcertCounters;
        use yggdrasil_ledger::Era;
        use yggdrasil_storage::{
            ChainDb, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile,
        };

        // Pre-loaded counter map: pool advanced to seq 5.
        let pool = [0xAA; 28];
        let mut counters = OcertCounters::new();
        for seq in 0..=5 {
            counters.validate_and_update(pool, seq, true).unwrap();
        }
        assert_eq!(counters.get(&pool), Some(5));

        // Minimal in-memory ChainDb + base ledger state.
        let mut chain_db = ChainDb::new(
            InMemoryImmutable::default(),
            InMemoryVolatile::default(),
            InMemoryLedgerStore::default(),
        );
        let base = LedgerState::new(Era::Byron);
        let mut tracking = LedgerCheckpointTracking {
            base_ledger_state: base.clone(),
            ledger_state: base,
            last_persisted_point: Point::Origin,
            plutus_evaluator: CekPlutusEvaluator::default(),
            stake_snapshots: None,
            epoch_size: None,
            pool_block_counts: BTreeMap::new(),
            ocert_persist_dir: None,
        };

        // A progress with a rollback-only batch (no real blocks needed —
        // the helper inspects `progress.rollback_count` for the reset
        // branch).
        let progress = MultiEraSyncProgress {
            current_point: Point::Origin,
            steps: Vec::new(),
            fetched_blocks: 0,
            rollback_count: 1,
        };
        let policy = LedgerCheckpointPolicy {
            min_slot_delta: 0,
            max_snapshots: 0,
        };

        // Run the helper. Pre-this-slice, counters would still hold
        // pool → 5 after the call.
        update_ledger_checkpoint_after_progress(
            &mut chain_db,
            &mut tracking,
            &progress,
            &policy,
            None,
            Some(&mut counters),
        )
        .expect("rollback path");

        // The reset must have cleared the counters so the next OpCert
        // from the same pool is treated as first-seen.
        assert!(counters.is_empty());
        assert_eq!(counters.get(&pool), None);
    }

    #[test]
    fn future_block_check_current_wall_slot_advances_with_time() {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time before epoch")
            .as_secs_f64();
        let cfg = FutureBlockCheckConfig {
            system_start_unix_secs: now_secs - 20.0,
            slot_length_secs: 1.0,
            clock_skew: ClockSkew::default_for_slot_length(std::time::Duration::from_secs(1)),
        };

        let a = cfg.current_wall_slot().0;
        let b = cfg.current_wall_slot().0;
        assert!(b >= a);
        assert!(a >= 19, "wall slot should be near elapsed seconds");
    }

    #[test]
    fn future_block_check_current_wall_slot_is_zero_before_system_start() {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time before epoch")
            .as_secs_f64();
        let cfg = FutureBlockCheckConfig {
            system_start_unix_secs: now_secs + 3600.0,
            slot_length_secs: 1.0,
            clock_skew: ClockSkew::default_for_slot_length(std::time::Duration::from_secs(1)),
        };
        assert_eq!(cfg.current_wall_slot(), SlotNo(0));
    }

    #[test]
    fn pool_performance_empty_counts_returns_empty_map() {
        let snapshot = make_snapshot_with_pools(&[(pool_hash(1), 500)]);
        let counts = BTreeMap::new();
        let perf = compute_pool_performance(&counts, &snapshot, EpochSize(432000));
        assert!(perf.is_empty());
    }

    #[test]
    fn pool_performance_no_stake_returns_empty_map() {
        let snapshot = StakeSnapshot::default();
        let mut counts = BTreeMap::new();
        counts.insert(pool_hash(1), 10);
        let perf = compute_pool_performance(&counts, &snapshot, EpochSize(432000));
        assert!(perf.is_empty());
    }

    #[test]
    fn pool_performance_single_pool_perfect() {
        let pool_a = pool_hash(1);
        let snapshot = make_snapshot_with_pools(&[(pool_a, 1000)]);
        let mut counts = BTreeMap::new();
        counts.insert(pool_a, 100);

        let perf = compute_pool_performance(&counts, &snapshot, EpochSize(432000));
        let p = perf.get(&pool_a).unwrap();
        // numerator = 100 * 1000, denominator = 1000 * 100 → 1/1
        assert_eq!(p.numerator, p.denominator);
    }

    #[test]
    fn pool_performance_two_pools_proportional() {
        let pool_a = pool_hash(1);
        let pool_b = pool_hash(2);
        let snapshot = make_snapshot_with_pools(&[(pool_a, 500), (pool_b, 500)]);
        let mut counts = BTreeMap::new();
        counts.insert(pool_a, 50);
        counts.insert(pool_b, 50);

        let perf = compute_pool_performance(&counts, &snapshot, EpochSize(432000));
        let pa = perf.get(&pool_a).unwrap();
        let pb = perf.get(&pool_b).unwrap();
        assert_eq!(pa.numerator, pa.denominator);
        assert_eq!(pb.numerator, pb.denominator);
    }

    #[test]
    fn pool_performance_underperforming_pool() {
        let pool_a = pool_hash(1);
        let pool_b = pool_hash(2);
        let snapshot = make_snapshot_with_pools(&[(pool_a, 500), (pool_b, 500)]);
        let mut counts = BTreeMap::new();
        counts.insert(pool_a, 25);
        counts.insert(pool_b, 75);

        let perf = compute_pool_performance(&counts, &snapshot, EpochSize(432000));
        let pa = perf.get(&pool_a).unwrap();
        // numerator = 25 * 1000, denominator = 500 * 100 → 25000/50000 = 0.5
        assert_eq!(pa.numerator * 2, pa.denominator);
    }

    #[test]
    fn pool_performance_pool_without_stake_skipped() {
        let pool_a = pool_hash(1);
        let pool_c = pool_hash(3);
        let snapshot = make_snapshot_with_pools(&[(pool_a, 1000)]);
        let mut counts = BTreeMap::new();
        counts.insert(pool_a, 90);
        counts.insert(pool_c, 10);

        let perf = compute_pool_performance(&counts, &snapshot, EpochSize(432000));
        assert!(perf.contains_key(&pool_a));
        assert!(!perf.contains_key(&pool_c));
    }

    // -- SyncError Display-message content tests --
    //
    // Mirrors the `ConsensusError` pattern: dedicated assertions on each
    // variant's Display message, ensuring the diagnostic fields survive
    // future refactors of the `#[error(...)]` format strings.

    #[test]
    fn display_block_from_future_names_slot_and_excess() {
        let e = SyncError::BlockFromFuture {
            slot: 12_345,
            excess_slots: 42,
        };
        let s = format!("{e}");
        assert!(s.contains("12345") || s.contains("12_345"));
        assert!(s.contains("42"));
    }

    #[test]
    fn display_wrong_block_body_size_names_both_sizes() {
        let e = SyncError::WrongBlockBodySize {
            declared: 1_000,
            actual: 1_500,
        };
        let s = format!("{e}");
        assert!(s.contains("1000") || s.contains("1_000"));
        assert!(s.contains("1500") || s.contains("1_500"));
    }

    #[test]
    fn display_protocol_version_mismatch_names_era_and_versions() {
        let e = SyncError::ProtocolVersionMismatch {
            era: Era::Alonzo,
            major: 4,
            minor: 0,
            expected_range: "5..=6".to_owned(),
        };
        let s = format!("{e}");
        assert!(s.contains("Alonzo"), "must name the offending era: {s}");
        assert!(s.contains('4'), "must name the declared major: {s}");
        assert!(s.contains("5..=6"), "must name the expected range: {s}",);
    }

    #[test]
    fn display_protocol_version_too_high_names_both_majors() {
        let e = SyncError::ProtocolVersionTooHigh { major: 99, max: 10 };
        let s = format!("{e}");
        assert!(s.contains("99"), "must name the offending major: {s}");
        assert!(s.contains("10"), "must name the configured ceiling: {s}");
    }

    #[test]
    fn display_header_prot_ver_too_high_names_both_majors() {
        let e = SyncError::HeaderProtVerTooHigh {
            header_major: 12,
            pp_major: 10,
        };
        let s = format!("{e}");
        assert!(s.contains("12"), "must name the header major: {s}");
        assert!(s.contains("10"), "must name the pp major: {s}");
    }

    // -- is_peer_attributable classification tests --

    #[test]
    fn sync_error_peer_attributable_for_validation_failures() {
        assert!(SyncError::BlockBodyHashMismatch.is_peer_attributable());
        assert!(
            SyncError::Consensus(yggdrasil_consensus::ConsensusError::InvalidKesSignature)
                .is_peer_attributable()
        );
        assert!(SyncError::LedgerDecode(LedgerError::CborTrailingBytes(1)).is_peer_attributable());
        assert!(
            SyncError::BlockFromFuture {
                slot: 999,
                excess_slots: 100,
            }
            .is_peer_attributable()
        );
        assert!(
            SyncError::WrongBlockBodySize {
                declared: 100,
                actual: 200,
            }
            .is_peer_attributable()
        );
        assert!(
            SyncError::ProtocolVersionMismatch {
                era: Era::Alonzo,
                major: 4,
                minor: 0,
                expected_range: "5..=6".to_owned(),
            }
            .is_peer_attributable()
        );
        assert!(SyncError::ProtocolVersionTooHigh { major: 99, max: 10 }.is_peer_attributable());
        assert!(
            SyncError::HeaderProtVerTooHigh {
                header_major: 15,
                pp_major: 10,
            }
            .is_peer_attributable()
        );
    }

    #[test]
    fn sync_error_not_peer_attributable_for_local_errors() {
        assert!(!SyncError::Recovery("test".to_owned()).is_peer_attributable());
        assert!(
            !SyncError::Storage(StorageError::Serialization("test".to_owned()))
                .is_peer_attributable()
        );
        assert!(
            !SyncError::KeepAlive(yggdrasil_network::KeepAliveClientError::ConnectionClosed)
                .is_peer_attributable()
        );
    }

    /// Drift-detection invariant: every `SyncError` variant must have an
    /// explicit classification decision in `is_peer_attributable`. The
    /// exhaustive match guard below forces any new variant to appear here,
    /// which in turn forces the author to think through whether the new
    /// error should be attributed to the peer (reconnect) or the local
    /// node (propagate up). Without this guard, a future variant would
    /// default to the `_ =>` fall-through inside `matches!` and silently
    /// be classified as non-peer-attributable — masking a real peer
    /// misbehavior if the variant represents validation failure.
    ///
    /// Each arm calls `.is_peer_attributable()` so the classification
    /// decision is exercised and a flipped boolean between the match arm
    /// here and the `matches!` in `is_peer_attributable` shows as a test
    /// failure rather than a silent misclassification.
    #[test]
    fn every_sync_error_variant_has_explicit_peer_attributable_decision() {
        use yggdrasil_network::{ChainSyncClientError, PeerError};
        // Construct one representative of every variant. The `match` over
        // a fictional SyncError acts as a compile-time exhaustiveness gate:
        // adding a new variant without extending this list is a hard compile
        // error.
        let all: Vec<SyncError> = vec![
            SyncError::Peer(PeerError::NoCompatibleVersion),
            SyncError::ChainSync(ChainSyncClientError::ConnectionClosed),
            SyncError::BlockFetch(BlockFetchClientError::ConnectionClosed),
            SyncError::LedgerDecode(LedgerError::CborTrailingBytes(1)),
            SyncError::Storage(StorageError::Serialization("x".to_owned())),
            SyncError::KeepAlive(yggdrasil_network::KeepAliveClientError::ConnectionClosed),
            SyncError::Consensus(yggdrasil_consensus::ConsensusError::InvalidKesSignature),
            SyncError::Recovery("x".to_owned()),
            SyncError::BlockBodyHashMismatch,
            SyncError::BlockFromFuture {
                slot: 1,
                excess_slots: 1,
            },
            SyncError::WrongBlockBodySize {
                declared: 1,
                actual: 2,
            },
            SyncError::ProtocolVersionMismatch {
                era: Era::Conway,
                major: 1,
                minor: 0,
                expected_range: "9+".to_owned(),
            },
            SyncError::ProtocolVersionTooHigh { major: 20, max: 10 },
            SyncError::HeaderProtVerTooHigh {
                header_major: 20,
                pp_major: 10,
            },
        ];

        for err in &all {
            // Compiler-enforced exhaustive match: if a new SyncError
            // variant is added without being classified here, this match
            // will fail to compile.
            let expected_peer_attributable = match err {
                SyncError::Peer(_) => false,
                SyncError::ChainSync(_) => false,
                SyncError::BlockFetch(_) => false,
                SyncError::LedgerDecode(_) => true,
                SyncError::Storage(_) => false,
                SyncError::KeepAlive(_) => false,
                SyncError::Consensus(_) => true,
                SyncError::Recovery(_) => false,
                SyncError::BlockBodyHashMismatch => true,
                SyncError::BlockFromFuture { .. } => true,
                SyncError::WrongBlockBodySize { .. } => true,
                SyncError::ProtocolVersionMismatch { .. } => true,
                SyncError::ProtocolVersionTooHigh { .. } => true,
                SyncError::HeaderProtVerTooHigh { .. } => true,
            };
            assert_eq!(
                err.is_peer_attributable(),
                expected_peer_attributable,
                "classification mismatch for {err:?}: test expected \
                 {expected_peer_attributable}, implementation returned \
                 {}. Review the `is_peer_attributable` matches! list \
                 against this test's expected map.",
                err.is_peer_attributable(),
            );
        }
    }

    #[test]
    fn protocol_version_constraints_enforce_alonzo_era_gate() {
        // Alonzo accepts intra-era major 5 and 6 PLUS the Babbage
        // transition major 7 (per upstream's hard-fork combinator
        // signalling — last block of Alonzo can carry the next
        // era's transition major).
        assert!(validate_protocol_version_for_era(Era::Alonzo, 5, 0, None).is_ok());
        assert!(validate_protocol_version_for_era(Era::Alonzo, 6, 2, None).is_ok());
        assert!(
            validate_protocol_version_for_era(Era::Alonzo, 7, 0, None).is_ok(),
            "Alonzo must accept PV major=7 — Babbage transition signal \
             emitted by `Test*HardForkAtEpoch=0` testnets at chain genesis",
        );

        // Pre-Alonzo majors are still rejected.
        assert!(matches!(
            validate_protocol_version_for_era(Era::Alonzo, 4, 3, None),
            Err(SyncError::ProtocolVersionMismatch {
                era: Era::Alonzo,
                major: 4,
                ..
            })
        ));
        // Post-transition (Babbage's intra-era 8) is rejected for Alonzo.
        assert!(matches!(
            validate_protocol_version_for_era(Era::Alonzo, 8, 0, None),
            Err(SyncError::ProtocolVersionMismatch {
                era: Era::Alonzo,
                major: 8,
                ..
            })
        ));
    }

    /// Round 154 — Babbage admits intra-era 7/8 PLUS the Conway
    /// transition major 9.  Pre-Babbage and post-Conway-transition
    /// majors are rejected.
    #[test]
    fn protocol_version_constraints_enforce_babbage_era_gate() {
        assert!(validate_protocol_version_for_era(Era::Babbage, 7, 0, None).is_ok());
        assert!(validate_protocol_version_for_era(Era::Babbage, 8, 0, None).is_ok());
        assert!(
            validate_protocol_version_for_era(Era::Babbage, 9, 0, None).is_ok(),
            "Babbage must accept PV major=9 — Conway transition signal",
        );
        assert!(matches!(
            validate_protocol_version_for_era(Era::Babbage, 6, 0, None),
            Err(SyncError::ProtocolVersionMismatch {
                era: Era::Babbage,
                major: 6,
                ..
            })
        ));
        assert!(matches!(
            validate_protocol_version_for_era(Era::Babbage, 10, 0, None),
            Err(SyncError::ProtocolVersionMismatch {
                era: Era::Babbage,
                major: 10,
                ..
            })
        ));
    }

    #[test]
    fn protocol_version_constraints_enforce_max_major_guard() {
        // Conway-era major 10 is accepted when max is 10.
        assert!(validate_protocol_version_for_era(Era::Conway, 10, 0, Some(10)).is_ok());

        // Future major versions are rejected by MaxMajorProtVer.
        assert!(matches!(
            validate_protocol_version_for_era(Era::Conway, 11, 0, Some(10)),
            Err(SyncError::ProtocolVersionTooHigh { major: 11, max: 10 })
        ));

        // Guard is global: it applies even when era-local range would otherwise fail.
        assert!(matches!(
            validate_protocol_version_for_era(Era::Alonzo, 7, 0, Some(6)),
            Err(SyncError::ProtocolVersionTooHigh { major: 7, max: 6 })
        ));
    }

    #[test]
    fn max_major_guard_delegates_to_consensus_obsolete_node_rule() {
        // Cross-check that the sync-layer ceiling enforcement uses the
        // consensus-crate `check_header_protocol_version` helper (slice 43)
        // rather than a duplicate inline comparison. The canonical PRTCL
        // rule uses `<=` at the boundary; this test pins the two layers
        // together so a future refactor that inlines the check cannot
        // silently drift to a stricter `<` comparison (off-by-one at the
        // era boundary slot of a hard fork).
        // Above ceiling at the sync layer ↔ ObsoleteNode at the consensus layer.
        assert!(matches!(
            validate_protocol_version_for_era(Era::Conway, 15, 0, Some(10)),
            Err(SyncError::ProtocolVersionTooHigh { major: 15, max: 10 })
        ));
        match yggdrasil_consensus::check_header_protocol_version(15, 10) {
            Err(yggdrasil_consensus::ConsensusError::ObsoleteNode {
                header_major,
                max_major,
            }) => {
                assert_eq!(header_major, 15);
                assert_eq!(max_major, 10);
            }
            other => panic!("consensus helper should reject 15 > 10, got {other:?}"),
        }
        // Equal-to-ceiling at the sync layer ↔ Ok at the consensus layer.
        assert!(validate_protocol_version_for_era(Era::Conway, 10, 0, Some(10)).is_ok());
        assert!(yggdrasil_consensus::check_header_protocol_version(10, 10).is_ok());
    }

    #[test]
    fn point_from_raw_header_decodes_observed_byron_serialised_header_envelope() {
        // Captured from preprod ChainSync roll-forward (YGG_SYNC_DEBUG).
        let raw_header: Vec<u8> = vec![
            0x82, 0x00, 0x82, 0x82, 0x00, 0x18, 0x53, 0xd8, 0x18, 0x58, 0x4c, 0x85, 0x01, 0x58,
            0x20, 0xd4, 0xb8, 0xde, 0x7a, 0x11, 0xd9, 0x29, 0xa3, 0x23, 0x37, 0x3c, 0xba, 0xb6,
            0xc1, 0xa9, 0xbd, 0xc9, 0x31, 0xbe, 0xff, 0xff, 0x11, 0xdb, 0x11, 0x1c, 0xf9, 0xd5,
            0x73, 0x56, 0xee, 0x19, 0x37, 0x58, 0x20, 0xaf, 0xc0, 0xda, 0x64, 0x18, 0x3b, 0xf2,
            0x66, 0x4f, 0x3d, 0x4e, 0xec, 0x72, 0x38, 0xd5, 0x24, 0xba, 0x60, 0x7f, 0xae, 0xea,
            0xb2, 0x4f, 0xc1, 0x00, 0xeb, 0x86, 0x1d, 0xba, 0x69, 0x97, 0x1b, 0x82, 0x00, 0x81,
            0x00, 0x81, 0xa0,
        ];

        let expected_hash = HeaderHash(
            hash_bytes_256(
                &[
                    [0x82, 0x01].as_slice(),
                    &[
                        0x85, 0x01, 0x58, 0x20, 0xd4, 0xb8, 0xde, 0x7a, 0x11, 0xd9, 0x29, 0xa3,
                        0x23, 0x37, 0x3c, 0xba, 0xb6, 0xc1, 0xa9, 0xbd, 0xc9, 0x31, 0xbe, 0xff,
                        0xff, 0x11, 0xdb, 0x11, 0x1c, 0xf9, 0xd5, 0x73, 0x56, 0xee, 0x19, 0x37,
                        0x58, 0x20, 0xaf, 0xc0, 0xda, 0x64, 0x18, 0x3b, 0xf2, 0x66, 0x4f, 0x3d,
                        0x4e, 0xec, 0x72, 0x38, 0xd5, 0x24, 0xba, 0x60, 0x7f, 0xae, 0xea, 0xb2,
                        0x4f, 0xc1, 0x00, 0xeb, 0x86, 0x1d, 0xba, 0x69, 0x97, 0x1b, 0x82, 0x00,
                        0x81, 0x00, 0x81, 0xa0,
                    ],
                ]
                .concat(),
            )
            .0,
        );

        let point = point_from_raw_header(&raw_header);
        assert_eq!(point, Some(Point::BlockPoint(SlotNo(83), expected_hash)));
    }
}
