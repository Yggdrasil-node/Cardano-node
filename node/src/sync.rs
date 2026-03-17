//! Sync orchestration helpers for node-to-node ChainSync + BlockFetch.
//!
//! This module provides a thin runtime coordination layer between the
//! `ChainSyncClient` and `BlockFetchClient` drivers from `yggdrasil-network`.
//! It intentionally keeps ledger and consensus validation out of the node
//! crate and focuses only on protocol sequencing.

use std::time::Duration;

use std::collections::BTreeMap;

use yggdrasil_consensus::{ActiveSlotCoeff, ChainEntry, ChainState, ConsensusError, EpochSize, Header as ConsensusHeader, HeaderBody as ConsensusHeaderBody, NonceEvolutionConfig, NonceEvolutionState, OpCert as ConsensusOpCert, SecurityParam, is_new_epoch, slot_to_epoch, verify_header, verify_leader_proof};
use yggdrasil_crypto::blake2b::hash_bytes_256;
use yggdrasil_crypto::ed25519::{Signature as Ed25519Signature, VerificationKey};
use yggdrasil_crypto::sum_kes::{SumKesSignature, SumKesVerificationKey};
use yggdrasil_crypto::vrf::VrfVerificationKey;
use yggdrasil_network::{
    BlockFetchClient, BlockFetchClientError, ChainRange, ChainSyncClient,
    ChainSyncClientError, DecodedHeaderNextResponse, KeepAliveClient, KeepAliveClientError, NextResponse,
    PeerError, TypedIntersectResponse, TypedNextResponse,
};
use yggdrasil_ledger::{
    AlonzoBlock, BabbageBlock, Block, BlockHeader, BlockNo, ByronBlock, BYRON_SLOTS_PER_EPOCH,
    CborDecode, CborEncode, ConwayBlock,
    Decoder, Era, EpochBoundaryEvent, HeaderHash, LedgerError, LedgerState, Nonce, Point,
    PraosHeader, PraosHeaderBody, ShelleyBlock, ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert,
    SlotNo, StakeSnapshots, Tx, TxId,
    apply_epoch_boundary, compute_block_body_hash,
};
use yggdrasil_mempool::Mempool;
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

/// Compute a `TxId` as the Blake2b-256 hash of the CBOR-encoded transaction
/// body, matching the upstream Cardano transaction identifier.
///
/// Reference: `Cardano.Ledger.TxIn` — `TxId`.
fn compute_tx_id(body: &[u8]) -> TxId {
    TxId(hash_bytes_256(body).0)
}

/// Convert a typed Shelley block into the generic ledger `Block` wrapper used
/// by storage traits.
pub fn shelley_block_to_block(block: &ShelleyBlock) -> Block {
    let body = &block.header.body;
    let hash = block.header_hash();
    let prev_hash = HeaderHash(body.prev_hash.unwrap_or([0u8; 32]));

    let transactions: Vec<Tx> = block
        .transaction_bodies
        .iter()
        .zip(
            block
                .transaction_witness_sets
                .iter()
                .map(Some)
                .chain(std::iter::repeat(None)),
        )
        .map(|(tx_body, ws)| {
            let raw = tx_body.to_cbor_bytes();
            Tx {
                id: compute_tx_id(&raw),
                body: raw,
                witnesses: ws.map(|w| w.to_cbor_bytes()),
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
        raw_cbor: None,
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
) -> Result<Vec<ShelleyBlock>, SyncError> {
    block_fetch
        .request_range_collect_points_decoded::<ShelleyBlock>(lower, upper)
        .await
        .map_err(map_blockfetch_error)
}

async fn fetch_range_blocks_multi_era(
    block_fetch: &mut BlockFetchClient,
    lower: Point,
    upper: Point,
) -> Result<Vec<MultiEraBlock>, SyncError> {
    block_fetch
        .request_range_collect_points_with(lower, upper, decode_multi_era_block_ledger)
        .await
        .map_err(map_blockfetch_error)
}

async fn fetch_range_blocks_multi_era_raw_decoded(
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
    let next = chain_sync.request_next().await?;
    match next {
        NextResponse::RollForward { header, tip }
        | NextResponse::AwaitRollForward { header, tip } => Ok(DecodedSyncStep::RollForward {
            header,
            tip: tip.clone(),
            blocks: fetch_range_blocks_decoded(block_fetch, from_point, tip).await?,
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
    let next = chain_sync.request_next_decoded_header::<ShelleyHeader>().await?;

    match next {
        DecodedHeaderNextResponse::RollForward { header, tip }
        | DecodedHeaderNextResponse::AwaitRollForward { header, tip } => {
            let blocks = fetch_range_blocks_typed(block_fetch, from_point, tip).await?;
            Ok(TypedSyncStep::RollForward {
                header: Box::new(header),
                tip,
                blocks,
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
        let step = sync_step_typed(
            chain_sync,
            block_fetch,
            from_point,
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
            from_point,
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
    match chain_sync.find_intersect_points(points.to_vec()).await? {
        TypedIntersectResponse::Found { point, tip } => Ok(TypedIntersectResult::Found {
            point,
            tip,
        }),
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
            (Point::BlockPoint(previous_slot, previous_hash), Point::BlockPoint(current_slot, current_hash)) => {
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
    /// Stake snapshots for epoch boundary processing.  When present,
    /// block application detects epoch transitions and applies the
    /// NEWEPOCH / SNAP / RUPD sequence before the first block of each
    /// new epoch.
    pub(crate) stake_snapshots: Option<StakeSnapshots>,
    /// Epoch size (slots per epoch) for epoch boundary detection.
    /// Required when `stake_snapshots` is `Some`.
    pub(crate) epoch_size: Option<EpochSize>,
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
    F: FnMut(&MultiEraBlock) -> Result<(), E>,
{
    for step in &progress.steps {
        if let MultiEraSyncStep::RollForward { blocks, .. } = step {
            for block in blocks {
                f(block)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn advance_ledger_state_with_progress(
    ledger_state: &mut LedgerState,
    progress: &MultiEraSyncProgress,
) -> Result<(), SyncError> {
    for_each_roll_forward_block(progress, |block| {
        ledger_state.apply_block(&multi_era_block_to_block(block))?;
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
pub(crate) fn advance_ledger_with_epoch_boundary(
    ledger_state: &mut LedgerState,
    snapshots: &mut StakeSnapshots,
    epoch_size: EpochSize,
    progress: &MultiEraSyncProgress,
) -> Result<Vec<EpochBoundaryEvent>, SyncError> {
    let mut events = Vec::new();
    for_each_roll_forward_block(progress, |block| -> Result<(), SyncError> {
        let converted = multi_era_block_to_block(block);
        let block_slot = converted.header.slot_no;

        // Detect epoch transition relative to the current ledger tip.
        let prev_slot = match ledger_state.tip {
            Point::BlockPoint(s, _) => Some(s),
            Point::Origin => None,
        };
        if is_new_epoch(prev_slot, block_slot, epoch_size) {
            let new_epoch = slot_to_epoch(block_slot, epoch_size);
            // Pool performance is not yet tracked; treat all pools as
            // having ideal performance (empty map → perfect σ/σ̂ ratio).
            apply_epoch_boundary(ledger_state, new_epoch, snapshots, &BTreeMap::new())
                .map(|event| events.push(event))
                .map_err(SyncError::LedgerDecode)?;
        }

        ledger_state.apply_block(&converted)?;
        Ok(())
    })?;
    Ok(events)
}

pub(crate) fn apply_nonce_evolution_to_progress(
    nonce_state: &mut NonceEvolutionState,
    progress: &MultiEraSyncProgress,
    nonce_cfg: &NonceEvolutionConfig,
) {
    let _ = for_each_roll_forward_block(progress, |block| {
        apply_nonce_evolution(nonce_state, block, nonce_cfg);
        Ok::<(), core::convert::Infallible>(())
    });
}

pub(crate) fn update_ledger_checkpoint_after_progress<I, V, L>(
    chain_db: &mut ChainDb<I, V, L>,
    tracking: &mut LedgerCheckpointTracking,
    progress: &MultiEraSyncProgress,
    policy: &LedgerCheckpointPolicy,
) -> Result<LedgerCheckpointUpdateOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    if progress.rollback_count > 0 {
        chain_db.truncate_ledger_checkpoints_after_point(&progress.current_point)?;

        tracking.ledger_state = recover_ledger_state_chaindb(
            chain_db,
            tracking.base_ledger_state.clone(),
        )?
        .ledger_state;
        // After rollback recovery, stake snapshots are stale — reset them
        // so epoch boundary processing restarts cleanly.
        if tracking.stake_snapshots.is_some() {
            tracking.stake_snapshots = Some(StakeSnapshots::new());
        }
    } else if let (Some(snapshots), Some(epoch_size)) =
        (tracking.stake_snapshots.as_mut(), tracking.epoch_size)
    {
        let _events = advance_ledger_with_epoch_boundary(
            &mut tracking.ledger_state,
            snapshots,
            epoch_size,
            progress,
        )?;
    } else {
        advance_ledger_state_with_progress(&mut tracking.ledger_state, progress)?;
    }

    if policy.max_snapshots == 0 {
        chain_db.clear_ledger_checkpoints()?;
        tracking.last_persisted_point = Point::Origin;
        return Ok(LedgerCheckpointUpdateOutcome::ClearedDisabled);
    }

    let current_point = tracking.ledger_state.tip;
    match current_point {
        Point::Origin => {
            chain_db.clear_ledger_checkpoints()?;
            tracking.last_persisted_point = Point::Origin;
            Ok(LedgerCheckpointUpdateOutcome::ClearedOrigin)
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
                tracking.last_persisted_point = current_point;
                Ok(LedgerCheckpointUpdateOutcome::Persisted {
                    slot,
                    retained_snapshots: retention.retained_snapshots,
                    pruned_snapshots: retention.pruned_snapshots,
                    rollback_count: progress.rollback_count,
                })
            } else {
                let since_last_slot_delta = match tracking.last_persisted_point {
                    Point::BlockPoint(previous_slot, _) => {
                        slot.0.saturating_sub(previous_slot.0)
                    }
                    Point::Origin => slot.0,
                };
                Ok(LedgerCheckpointUpdateOutcome::Skipped {
                    slot,
                    rollback_count: progress.rollback_count,
                    since_last_slot_delta,
                })
            }
        }
    }
}

pub(crate) fn default_checkpoint_tracking<I, V, L>(
    chain_db: &ChainDb<I, V, L>,
) -> Result<LedgerCheckpointTracking, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    let recovery = recover_ledger_state_chaindb(chain_db, LedgerState::new(Era::Byron))?;
    Ok(LedgerCheckpointTracking {
        base_ledger_state: recovery.ledger_state.clone(),
        ledger_state: recovery.ledger_state,
        last_persisted_point: recovery.point,
        stake_snapshots: None,
        epoch_size: None,
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
    chain_db.recover_ledger_state(base_state).map_err(|error| match error {
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
    let mut chain_state = config.security_param.map(ChainState::new);

    loop {
        let batch_fut = sync_batch_apply_verified(
            chain_sync,
            block_fetch,
            store,
            from_point,
            config.batch_size,
            Some(&config.verification),
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
pub async fn run_verified_sync_service_chaindb<I, V, L, F>(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    chain_db: &mut ChainDb<I, V, L>,
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
    let mut chain_state = config.security_param.map(ChainState::new);
    let mut checkpoint_tracking = default_checkpoint_tracking(chain_db)?;

    // Enable epoch boundary processing when nonce config provides epoch size.
    if let Some(ref nonce_cfg) = config.nonce_config {
        checkpoint_tracking.stake_snapshots = Some(StakeSnapshots::new());
        checkpoint_tracking.epoch_size = Some(nonce_cfg.epoch_size);
    }

    loop {
        let batch_fut = sync_batch_verified(
            chain_sync,
            block_fetch,
            from_point,
            config.batch_size,
            Some(&config.verification),
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
                let applied = apply_verified_progress_to_chaindb(
                    chain_db,
                    &progress,
                    chain_state.as_mut(),
                    Some(&mut checkpoint_tracking),
                    &config.checkpoint_policy,
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
    let kes_sig = SumKesSignature::from_bytes(SHELLEY_KES_DEPTH, &header.signature)
        .map_err(|_| SyncError::LedgerDecode(LedgerError::CborInvalidLength {
            expected: SumKesSignature::expected_size(SHELLEY_KES_DEPTH),
            actual: header.signature.len(),
        }))?;

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
    // Peek at the structure: expect a 2-element array [tag, body].
    use yggdrasil_ledger::cbor::Decoder;
    let mut dec = Decoder::new(raw);
    let arr_len = dec.array()?;
    if arr_len != 2 {
        return Err(LedgerError::CborInvalidLength {
            expected: 2,
            actual: arr_len as usize,
        });
    }

    let tag = dec.unsigned()?;

    match tag {
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
/// All Shelley-family eras (Shelley/Allegra/Mary/Alonzo, Babbage, Conway)
/// are fully decoded using the common block envelope. Byron blocks
/// populate real header fields from structural decode:
/// - `hash`: `Blake2b-256(prefix ++ raw_header_cbor)`
/// - `prev_hash`: from Byron consensus data
/// - `slot_no`: absolute slot via `epoch * 21600 + slot_in_epoch`
/// - `block_no`: `chain_difficulty` from consensus data
/// - `issuer_vkey`: zeroed (Byron uses a different signature scheme)
/// - `transactions`: decoded from block body tx_payload
pub fn multi_era_block_to_block(block: &MultiEraBlock) -> Block {
    match block {
        MultiEraBlock::Shelley(shelley) => shelley_block_to_block(shelley),
        MultiEraBlock::Alonzo(alonzo) => alonzo_block_to_block(alonzo),
        MultiEraBlock::Babbage(babbage) => babbage_block_to_block(babbage),
        MultiEraBlock::Conway(conway) => conway_block_to_block(conway),
        MultiEraBlock::Byron { block: byron, .. } => {
            let transactions: Vec<Tx> = byron
                .transactions()
                .iter()
                .map(|tx_aux| {
                    let raw = tx_aux.tx.to_cbor_bytes();
                    Tx {
                        id: compute_tx_id(&raw),
                        body: raw,
                        witnesses: None,
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
                    issuer_vkey: [0u8; 32],
                },
                transactions,
                raw_cbor: None,
            }
        }
    }
}

/// Convert a typed Alonzo block into the generic ledger `Block` wrapper.
pub fn alonzo_block_to_block(block: &AlonzoBlock) -> Block {
    let body = &block.header.body;
    let hash = block.header_hash();
    let prev_hash = HeaderHash(body.prev_hash.unwrap_or([0u8; 32]));

    let transactions: Vec<Tx> = block
        .transaction_bodies
        .iter()
        .zip(
            block
                .transaction_witness_sets
                .iter()
                .map(Some)
                .chain(std::iter::repeat(None)),
        )
        .map(|(tx_body, ws)| {
            let raw = tx_body.to_cbor_bytes();
            Tx {
                id: compute_tx_id(&raw),
                body: raw,
                witnesses: ws.map(|w| w.to_cbor_bytes()),
            }
        })
        .collect();

    Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash,
            prev_hash,
            slot_no: SlotNo(body.slot),
            block_no: BlockNo(body.block_number),
            issuer_vkey: body.issuer_vkey,
        },
        transactions,
        raw_cbor: None,
    }
}

/// Convert a typed Babbage block into the generic ledger `Block` wrapper.
fn babbage_block_to_block(block: &BabbageBlock) -> Block {
    let body = &block.header.body;
    let hash = block.header_hash();
    let prev_hash = HeaderHash(body.prev_hash.unwrap_or([0u8; 32]));

    let transactions: Vec<Tx> = block
        .transaction_bodies
        .iter()
        .zip(
            block
                .transaction_witness_sets
                .iter()
                .map(Some)
                .chain(std::iter::repeat(None)),
        )
        .map(|(tx_body, ws)| {
            let raw = tx_body.to_cbor_bytes();
            Tx {
                id: compute_tx_id(&raw),
                body: raw,
                witnesses: ws.map(|w| w.to_cbor_bytes()),
            }
        })
        .collect();

    Block {
        era: Era::Babbage,
        header: BlockHeader {
            hash,
            prev_hash,
            slot_no: SlotNo(body.slot),
            block_no: BlockNo(body.block_number),
            issuer_vkey: body.issuer_vkey,
        },
        transactions,
        raw_cbor: None,
    }
}

/// Convert a typed Conway block into the generic ledger `Block` wrapper.
fn conway_block_to_block(block: &ConwayBlock) -> Block {
    let body = &block.header.body;
    let hash = block.header_hash();
    let prev_hash = HeaderHash(body.prev_hash.unwrap_or([0u8; 32]));

    let transactions: Vec<Tx> = block
        .transaction_bodies
        .iter()
        .zip(
            block
                .transaction_witness_sets
                .iter()
                .map(Some)
                .chain(std::iter::repeat(None)),
        )
        .map(|(tx_body, ws)| {
            let raw = tx_body.to_cbor_bytes();
            Tx {
                id: compute_tx_id(&raw),
                body: raw,
                witnesses: ws.map(|w| w.to_cbor_bytes()),
            }
        })
        .collect();

    Block {
        era: Era::Conway,
        header: BlockHeader {
            hash,
            prev_hash,
            slot_no: SlotNo(body.slot),
            block_no: BlockNo(body.block_number),
            issuer_vkey: body.issuer_vkey,
        },
        transactions,
        raw_cbor: None,
    }
}

/// Verification parameters for Shelley-family header validation.
///
/// These correspond to Shelley genesis parameters and are used by
/// `verify_multi_era_block` and the verified sync pipeline.
///
/// Reference: `shelleyGenesisConfig` in `cardano-node` configuration.
#[derive(Clone, Copy, Debug)]
pub struct VerificationConfig {
    /// Number of slots per KES period (mainnet: 129600).
    pub slots_per_kes_period: u64,
    /// Maximum number of KES evolutions (mainnet: 62).
    pub max_kes_evolutions: u64,
    /// Whether to verify the block body hash against the header.
    pub verify_body_hash: bool,
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
    let (vrf_vkey_bytes, leader_proof, slot) = match block {
        MultiEraBlock::Shelley(s) => (
            s.header.body.vrf_vkey,
            &s.header.body.leader_vrf.proof,
            SlotNo(s.header.body.slot),
        ),
        MultiEraBlock::Alonzo(a) => (
            a.header.body.vrf_vkey,
            &a.header.body.leader_vrf.proof,
            SlotNo(a.header.body.slot),
        ),
        MultiEraBlock::Babbage(b) => (
            b.header.body.vrf_vkey,
            &b.header.body.vrf_result.proof,
            SlotNo(b.header.body.slot),
        ),
        MultiEraBlock::Conway(c) => (
            c.header.body.vrf_vkey,
            &c.header.body.vrf_result.proof,
            SlotNo(c.header.body.slot),
        ),
        MultiEraBlock::Byron { .. } => return Ok(true),
    };

    let vk = VrfVerificationKey::from_bytes(vrf_vkey_bytes);
    verify_leader_proof(
        &vk,
        slot,
        params.epoch_nonce,
        leader_proof,
        params.sigma_num,
        params.sigma_den,
        &params.active_slot_coeff,
    )
    .map_err(SyncError::Consensus)
}

/// Applies a multi-era block to the nonce evolution state machine.
///
/// Extracts the VRF nonce contribution and `prev_hash` from the block header
/// and feeds them to [`NonceEvolutionState::apply_block`].
///
/// - TPraos (Shelley–Alonzo): uses the dedicated `nonce_vrf` output.
/// - Praos (Babbage/Conway): uses the single `vrf_result` output.
/// - Byron blocks are skipped (no VRF).
///
/// After this call, the state's `epoch_nonce` reflects any epoch transition
/// that may have occurred at the block's slot.
pub fn apply_nonce_evolution(
    state: &mut NonceEvolutionState,
    block: &MultiEraBlock,
    config: &NonceEvolutionConfig,
) {
    match block {
        MultiEraBlock::Shelley(s) => {
            let slot = SlotNo(s.header.body.slot);
            let prev_hash = s.header.body.prev_hash.map(HeaderHash);
            state.apply_block(slot, &s.header.body.nonce_vrf.output, prev_hash, config);
        }
        MultiEraBlock::Alonzo(a) => {
            let slot = SlotNo(a.header.body.slot);
            let prev_hash = a.header.body.prev_hash.map(HeaderHash);
            state.apply_block(slot, &a.header.body.nonce_vrf.output, prev_hash, config);
        }
        MultiEraBlock::Babbage(b) => {
            let slot = SlotNo(b.header.body.slot);
            let prev_hash = b.header.body.prev_hash.map(HeaderHash);
            state.apply_block(slot, &b.header.body.vrf_result.output, prev_hash, config);
        }
        MultiEraBlock::Conway(c) => {
            let slot = SlotNo(c.header.body.slot);
            let prev_hash = c.header.body.prev_hash.map(HeaderHash);
            state.apply_block(slot, &c.header.body.vrf_result.output, prev_hash, config);
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
pub fn verify_multi_era_block(
    block: &MultiEraBlock,
    config: &VerificationConfig,
) -> Result<(), SyncError> {
    match block {
        MultiEraBlock::Shelley(shelley) => {
            verify_shelley_header(
                &shelley.header,
                config.slots_per_kes_period,
                config.max_kes_evolutions,
            )
        }
        MultiEraBlock::Alonzo(alonzo) => {
            verify_shelley_header(
                &alonzo.header,
                config.slots_per_kes_period,
                config.max_kes_evolutions,
            )
        }
        MultiEraBlock::Babbage(babbage) => {
            verify_praos_header(
                &babbage.header,
                config.slots_per_kes_period,
                config.max_kes_evolutions,
            )
        }
        MultiEraBlock::Conway(conway) => {
            verify_praos_header(
                &conway.header,
                config.slots_per_kes_period,
                config.max_kes_evolutions,
            )
        }
        MultiEraBlock::Byron { .. } => Ok(()),
    }
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
        /// Original wire-format bytes for each block, parallel to `blocks`.
        ///
        /// When present, these are stored alongside the decoded `Block` so
        /// the inbound server can re-serve the block over BlockFetch.
        raw_blocks: Option<Vec<Vec<u8>>>,
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
            Ok(MultiEraSyncStep::RollForward {
                raw_header: header,
                tip,
                blocks: fetch_range_blocks_multi_era(block_fetch, from_point, tip).await?,
                raw_blocks: None,
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
        MultiEraBlock::Byron { block: byron, .. } => Some(ChainEntry {
            hash: byron.header_hash(),
            slot: SlotNo(byron.absolute_slot(BYRON_SLOTS_PER_EPOCH)),
            block_no: BlockNo(byron.chain_difficulty()),
        }),
        MultiEraBlock::Shelley(b) => Some(ChainEntry {
            hash: b.header_hash(),
            slot: SlotNo(b.header.body.slot),
            block_no: BlockNo(b.header.body.block_number),
        }),
        MultiEraBlock::Alonzo(b) => Some(ChainEntry {
            hash: b.header_hash(),
            slot: SlotNo(b.header.body.slot),
            block_no: BlockNo(b.header.body.block_number),
        }),
        MultiEraBlock::Babbage(b) => Some(ChainEntry {
            hash: b.header_hash(),
            slot: SlotNo(b.header.body.slot),
            block_no: BlockNo(b.header.body.block_number),
        }),
        MultiEraBlock::Conway(b) => Some(ChainEntry {
            hash: b.header_hash(),
            slot: SlotNo(b.header.body.slot),
            block_no: BlockNo(b.header.body.block_number),
        }),
    }
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
        MultiEraSyncStep::RollForward { blocks, raw_blocks, .. } => {
            let raws = raw_blocks.as_deref();
            for (i, b) in blocks.iter().enumerate() {
                let mut block = multi_era_block_to_block(b);
                block.raw_cbor = raws.and_then(|r| r.get(i)).cloned();
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
}

pub(crate) fn apply_verified_progress_to_chaindb<I, V, L>(
    chain_db: &mut ChainDb<I, V, L>,
    progress: &MultiEraSyncProgress,
    chain_state: Option<&mut ChainState>,
    checkpoint_tracking: Option<&mut LedgerCheckpointTracking>,
    checkpoint_policy: &LedgerCheckpointPolicy,
) -> Result<AppliedVerifiedProgress, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    for step in &progress.steps {
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

    let checkpoint_outcome = checkpoint_tracking
        .map(|tracking| {
            update_ledger_checkpoint_after_progress(
                chain_db,
                tracking,
                progress,
                checkpoint_policy,
            )
        })
        .transpose()?;

    Ok(AppliedVerifiedProgress {
        stable_block_count: total_stable,
        checkpoint_outcome,
    })
}

pub(crate) async fn sync_batch_verified(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    mut from_point: Point,
    batch_size: usize,
    verification: Option<&VerificationConfig>,
) -> Result<MultiEraSyncProgress, SyncError> {
    let mut steps = Vec::new();
    let mut fetched_blocks = 0usize;
    let mut rollback_count = 0usize;

    for _ in 0..batch_size {
        let next = chain_sync.request_next_typed().await?;

        let me_step = match next {
            TypedNextResponse::RollForward { header, tip }
            | TypedNextResponse::AwaitRollForward { header, tip } => {
                let raw_and_decoded =
                    fetch_range_blocks_multi_era_raw_decoded(block_fetch, from_point, tip).await?;

                if let Some(config) = verification {
                    if config.verify_body_hash {
                        for (raw, _) in &raw_and_decoded {
                            verify_block_body_hash(raw)?;
                        }
                    }
                }

                let (raw_bytes, decoded_blocks): (Vec<Vec<u8>>, Vec<MultiEraBlock>) =
                    raw_and_decoded.into_iter().unzip();

                if let Some(config) = verification {
                    for block in &decoded_blocks {
                        verify_multi_era_block(block, config)?;
                    }
                }

                from_point = tip;
                fetched_blocks += decoded_blocks.len();

                MultiEraSyncStep::RollForward {
                    raw_header: header,
                    tip,
                    blocks: decoded_blocks,
                    raw_blocks: Some(raw_bytes),
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
pub async fn sync_batch_apply_verified<S: VolatileStore>(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    store: &mut S,
    from_point: Point,
    batch_size: usize,
    verification: Option<&VerificationConfig>,
) -> Result<MultiEraSyncProgress, SyncError> {
    let progress = sync_batch_verified(
        chain_sync,
        block_fetch,
        from_point,
        batch_size,
        verification,
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

// ---------------------------------------------------------------------------
// Phase 40: Mempool sync eviction
// ---------------------------------------------------------------------------

/// Extract transaction IDs from a multi-era block.
///
/// For all Shelley-family eras, each transaction body is CBOR-encoded and
/// hashed (Blake2b-256) to derive the canonical `TxId`. Byron blocks
/// return an empty list since structural decode is not yet implemented.
pub fn extract_tx_ids(block: &MultiEraBlock) -> Vec<TxId> {
    match block {
        MultiEraBlock::Shelley(shelley) => shelley
            .transaction_bodies
            .iter()
            .map(|body| compute_tx_id(&body.to_cbor_bytes()))
            .collect(),
        MultiEraBlock::Alonzo(alonzo) => alonzo
            .transaction_bodies
            .iter()
            .map(|body| compute_tx_id(&body.to_cbor_bytes()))
            .collect(),
        MultiEraBlock::Babbage(babbage) => babbage
            .transaction_bodies
            .iter()
            .map(|body| compute_tx_id(&body.to_cbor_bytes()))
            .collect(),
        MultiEraBlock::Conway(conway) => conway
            .transaction_bodies
            .iter()
            .map(|body| compute_tx_id(&body.to_cbor_bytes()))
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
pub fn evict_confirmed_from_mempool(
    mempool: &mut Mempool,
    step: &MultiEraSyncStep,
) -> usize {
    match step {
        MultiEraSyncStep::RollForward { blocks, tip, .. } => {
            let confirmed_ids: Vec<TxId> = blocks
                .iter()
                .flat_map(extract_tx_ids)
                .collect();
            let removed = mempool.remove_confirmed(&confirmed_ids);
            let tip_slot = tip.slot().unwrap_or(SlotNo(0));
            let purged = mempool.purge_expired(tip_slot);
            removed + purged
        }
        MultiEraSyncStep::RollBackward { .. } => 0,
    }
}
