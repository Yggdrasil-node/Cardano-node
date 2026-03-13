//! Sync orchestration helpers for node-to-node ChainSync + BlockFetch.
//!
//! This module provides a thin runtime coordination layer between the
//! `ChainSyncClient` and `BlockFetchClient` drivers from `yggdrasil-network`.
//! It intentionally keeps ledger and consensus validation out of the node
//! crate and focuses only on protocol sequencing.

use std::time::Duration;

use yggdrasil_consensus::{ConsensusError, Header as ConsensusHeader, HeaderBody as ConsensusHeaderBody, OpCert as ConsensusOpCert, verify_header};
use yggdrasil_crypto::ed25519::{Signature as Ed25519Signature, VerificationKey};
use yggdrasil_crypto::sum_kes::{SumKesSignature, SumKesVerificationKey};
use yggdrasil_crypto::vrf::VrfVerificationKey;
use yggdrasil_network::{
    BatchResponse, BlockFetchClient, BlockFetchClientError, ChainRange, ChainSyncClient,
    ChainSyncClientError, IntersectResponse, KeepAliveClient, KeepAliveClientError, NextResponse,
};
use yggdrasil_ledger::{
    Block, BlockHeader, BlockNo, CborDecode, CborEncode, Era, HeaderHash, LedgerError, Point,
    ShelleyBlock, ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, SlotNo, Tx, TxId,
};
use yggdrasil_storage::{StorageError, VolatileStore};

/// Error type for sync orchestration operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
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

/// Build a deterministic placeholder `TxId` from an opaque transaction body.
///
/// This is a temporary bridge until full transaction-id hashing parity is
/// wired through the node integration path.
fn tx_id_from_body_placeholder(body: &[u8]) -> TxId {
    let mut out = [0u8; 32];
    let n = usize::min(32, body.len());
    out[..n].copy_from_slice(&body[..n]);
    TxId(out)
}

/// Convert a typed Shelley block into the generic ledger `Block` wrapper used
/// by storage traits.
pub fn shelley_block_to_block(block: &ShelleyBlock) -> Block {
    let body = &block.header.body;
    let hash = HeaderHash(body.body_hash);
    let prev_hash = HeaderHash(body.prev_hash.unwrap_or([0u8; 32]));

    let transactions: Vec<Tx> = block
        .transaction_bodies
        .iter()
        .map(|tx_body| {
            let raw = tx_body.to_cbor_bytes();
            Tx {
                id: tx_id_from_body_placeholder(&raw),
                body: raw,
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
        },
        transactions,
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

async fn fetch_range_blocks(
    block_fetch: &mut BlockFetchClient,
    lower: Vec<u8>,
    upper: Vec<u8>,
) -> Result<Vec<Vec<u8>>, SyncError> {
    let mut blocks = Vec::new();
    let range = ChainRange { lower, upper };

    match block_fetch.request_range(range).await? {
        BatchResponse::NoBlocks => Ok(blocks),
        BatchResponse::StartedBatch => {
            while let Some(block) = block_fetch.recv_block().await? {
                blocks.push(block);
            }
            Ok(blocks)
        }
    }
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
            let blocks = fetch_range_blocks(block_fetch, from_point, tip.clone()).await?;
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
    let step = sync_step(chain_sync, block_fetch, from_point).await?;
    match step {
        SyncStep::RollForward {
            header,
            tip,
            blocks,
        } => Ok(DecodedSyncStep::RollForward {
            header,
            tip,
            blocks: decode_shelley_blocks(&blocks)?,
        }),
        SyncStep::RollBackward { point, tip } => Ok(DecodedSyncStep::RollBackward { point, tip }),
    }
}

/// Execute one sync step and decode all ChainSync and BlockFetch payloads
/// into typed ledger values.
pub async fn sync_step_typed(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    from_point: Vec<u8>,
) -> Result<TypedSyncStep, SyncError> {
    let step = sync_step(chain_sync, block_fetch, from_point).await?;
    match step {
        SyncStep::RollForward {
            header,
            tip,
            blocks,
        } => Ok(TypedSyncStep::RollForward {
            header: Box::new(decode_shelley_header(&header)?),
            tip: decode_point(&tip)?,
            blocks: decode_shelley_blocks(&blocks)?,
        }),
        SyncStep::RollBackward { point, tip } => Ok(TypedSyncStep::RollBackward {
            point: decode_point(&point)?,
            tip: decode_point(&tip)?,
        }),
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
        let step = sync_step_typed(
            chain_sync,
            block_fetch,
            from_point.to_cbor_bytes(),
        )
        .await?;

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

        let step = sync_step_typed(
            chain_sync,
            block_fetch,
            from_point.to_cbor_bytes(),
        )
        .await?;

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
        TypedSyncStep::RollForward { blocks, .. } => {
            for b in blocks {
                store.add_block(shelley_block_to_block(b))?;
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
    let encoded: Vec<Vec<u8>> = points.iter().map(|p| p.to_cbor_bytes()).collect();

    match chain_sync.find_intersect(encoded).await? {
        IntersectResponse::Found { point, tip } => Ok(TypedIntersectResult::Found {
            point: decode_point(&point)?,
            tip: decode_point(&tip)?,
        }),
        IntersectResponse::NotFound { tip } => Ok(TypedIntersectResult::NotFound {
            tip: decode_point(&tip)?,
        }),
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
        hot_vk: SumKesVerificationKey::from_bytes(opcert.hot_vkey),
        cert_counter: opcert.sequence_number,
        kes_period: opcert.kes_period,
        sigma: Ed25519Signature::from_bytes(opcert.sigma),
    }
}

/// Convert a ledger `ShelleyHeaderBody` into a consensus `HeaderBody` for
/// verification.
pub fn shelley_header_body_to_consensus(body: &ShelleyHeaderBody) -> ConsensusHeaderBody {
    ConsensusHeaderBody {
        block_no: BlockNo(body.block_number),
        slot_no: SlotNo(body.slot),
        prev_hash: body.prev_hash.map(HeaderHash),
        issuer_vk: VerificationKey::from_bytes(body.issuer_vkey),
        vrf_vk: VrfVerificationKey::from_bytes(body.vrf_vkey),
        body_size: body.body_size,
        body_hash: body.body_hash,
        opcert: shelley_opcert_to_consensus(&body.opcert),
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
    let kes_sig = SumKesSignature::from_bytes(SHELLEY_KES_DEPTH, &header.signature)
        .map_err(|_| SyncError::LedgerDecode(LedgerError::CborInvalidLength {
            expected: SumKesSignature::expected_size(SHELLEY_KES_DEPTH),
            actual: header.signature.len(),
        }))?;

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

// ---------------------------------------------------------------------------
// Phase 35: Multi-era block decode
// ---------------------------------------------------------------------------

/// A decoded block from any supported era.
///
/// As era support expands, new variants are added here. Currently only
/// Shelley is fully decoded; Byron blocks pass through as opaque bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultiEraBlock {
    /// An opaque Byron-era block. Full structural decode is not yet
    /// implemented; the raw CBOR bytes are preserved.
    Byron {
        /// Raw CBOR bytes of the Byron block.
        raw: Vec<u8>,
    },
    /// A fully decoded Shelley-era block (also covers Allegra/Mary/Alonzo
    /// which share the Shelley block envelope in the wire format).
    Shelley(ShelleyBlock),
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
/// kept as opaque bytes. Shelley-family blocks (tags 2–5) are decoded
/// using the Shelley block codec.
///
/// Tags 6 (Babbage) and 7 (Conway) are not yet supported and return a
/// decode error.
pub fn decode_multi_era_block(raw: &[u8]) -> Result<MultiEraBlock, SyncError> {
    // Peek at the structure: expect a 2-element array [tag, body].
    use yggdrasil_ledger::cbor::Decoder;
    let mut dec = Decoder::new(raw);
    let arr_len = dec.array().map_err(SyncError::LedgerDecode)?;
    if arr_len != 2 {
        return Err(SyncError::LedgerDecode(LedgerError::CborInvalidLength {
            expected: 2,
            actual: arr_len as usize,
        }));
    }

    let tag = dec.unsigned().map_err(SyncError::LedgerDecode)?;

    match tag {
        era_tag::BYRON_EBB | era_tag::BYRON_MAIN => {
            Ok(MultiEraBlock::Byron { raw: raw.to_vec() })
        }
        era_tag::SHELLEY | era_tag::ALLEGRA | era_tag::MARY | era_tag::ALONZO => {
            // The body is the next CBOR item; capture its raw bytes.
            let body_start = dec.position();
            dec.skip().map_err(SyncError::LedgerDecode)?;
            let body_bytes = &raw[body_start..dec.position()];
            let block = ShelleyBlock::from_cbor_bytes(body_bytes)
                .map_err(SyncError::LedgerDecode)?;
            Ok(MultiEraBlock::Shelley(block))
        }
        unsupported => {
            Err(SyncError::LedgerDecode(LedgerError::CborTypeMismatch {
                expected: 2, // Shelley era tag
                actual: unsupported as u8,
            }))
        }
    }
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
