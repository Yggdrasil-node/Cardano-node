//! Sync orchestration helpers for node-to-node ChainSync + BlockFetch.
//!
//! This module provides a thin runtime coordination layer between the
//! `ChainSyncClient` and `BlockFetchClient` drivers from `yggdrasil-network`.
//! It intentionally keeps ledger and consensus validation out of the node
//! crate and focuses only on protocol sequencing.

use std::time::Duration;

use yggdrasil_consensus::{ConsensusError, Header as ConsensusHeader, HeaderBody as ConsensusHeaderBody, OpCert as ConsensusOpCert, verify_header};
use yggdrasil_crypto::blake2b::hash_bytes_256;
use yggdrasil_crypto::ed25519::{Signature as Ed25519Signature, VerificationKey};
use yggdrasil_crypto::sum_kes::{SumKesSignature, SumKesVerificationKey};
use yggdrasil_crypto::vrf::VrfVerificationKey;
use yggdrasil_network::{
    BatchResponse, BlockFetchClient, BlockFetchClientError, ChainRange, ChainSyncClient,
    ChainSyncClientError, IntersectResponse, KeepAliveClient, KeepAliveClientError, NextResponse,
};
use yggdrasil_ledger::{
    AlonzoBlock, BabbageBlock, Block, BlockHeader, BlockNo, CborDecode, CborEncode, ConwayBlock,
    Decoder, Era, HeaderHash, LedgerError, Point, PraosHeader, PraosHeaderBody, ShelleyBlock,
    ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, SlotNo, Tx, TxId,
    compute_block_body_hash,
};
use yggdrasil_mempool::Mempool;
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
        .map(|tx_body| {
            let raw = tx_body.to_cbor_bytes();
            Tx {
                id: compute_tx_id(&raw),
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
    /// An opaque Byron-era block. Full structural decode is not yet
    /// implemented; the raw CBOR bytes are preserved.
    Byron {
        /// Raw CBOR bytes of the Byron block.
        raw: Vec<u8>,
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
        era_tag::SHELLEY | era_tag::ALLEGRA | era_tag::MARY => {
            // Shelley/Allegra/Mary blocks are 4-element CBOR arrays.
            let body_start = dec.position();
            dec.skip().map_err(SyncError::LedgerDecode)?;
            let body_bytes = &raw[body_start..dec.position()];
            let block = ShelleyBlock::from_cbor_bytes(body_bytes)
                .map_err(SyncError::LedgerDecode)?;
            Ok(MultiEraBlock::Shelley(Box::new(block)))
        }
        era_tag::ALONZO => {
            // Alonzo blocks are 5-element CBOR arrays (added invalid_transactions).
            let body_start = dec.position();
            dec.skip().map_err(SyncError::LedgerDecode)?;
            let body_bytes = &raw[body_start..dec.position()];
            let block = AlonzoBlock::from_cbor_bytes(body_bytes)
                .map_err(SyncError::LedgerDecode)?;
            Ok(MultiEraBlock::Alonzo(Box::new(block)))
        }
        era_tag::BABBAGE => {
            let body_start = dec.position();
            dec.skip().map_err(SyncError::LedgerDecode)?;
            let body_bytes = &raw[body_start..dec.position()];
            let block = BabbageBlock::from_cbor_bytes(body_bytes)
                .map_err(SyncError::LedgerDecode)?;
            Ok(MultiEraBlock::Babbage(Box::new(block)))
        }
        era_tag::CONWAY => {
            let body_start = dec.position();
            dec.skip().map_err(SyncError::LedgerDecode)?;
            let body_bytes = &raw[body_start..dec.position()];
            let block = ConwayBlock::from_cbor_bytes(body_bytes)
                .map_err(SyncError::LedgerDecode)?;
            Ok(MultiEraBlock::Conway(Box::new(block)))
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

// ---------------------------------------------------------------------------
// Phase 37: Verified multi-era sync pipeline
// ---------------------------------------------------------------------------

/// Convert a `MultiEraBlock` into the generic ledger `Block` wrapper.
///
/// All Shelley-family eras (Shelley/Allegra/Mary/Alonzo, Babbage, Conway)
/// are fully decoded using the common block envelope. Byron blocks produce
/// a minimal `Block` whose header hash is the Blake2b-256 of the raw CBOR
/// envelope; other header fields are zeroed because Byron structural
/// decode is not yet implemented.
pub fn multi_era_block_to_block(block: &MultiEraBlock) -> Block {
    match block {
        MultiEraBlock::Shelley(shelley) => shelley_block_to_block(shelley),
        MultiEraBlock::Alonzo(alonzo) => alonzo_block_to_block(alonzo),
        MultiEraBlock::Babbage(babbage) => babbage_block_to_block(babbage),
        MultiEraBlock::Conway(conway) => conway_block_to_block(conway),
        MultiEraBlock::Byron { raw } => Block {
            era: Era::Byron,
            header: BlockHeader {
                hash: HeaderHash(hash_bytes_256(raw).0),
                prev_hash: HeaderHash([0u8; 32]),
                slot_no: SlotNo(0),
                block_no: BlockNo(0),
                issuer_vkey: [0u8; 32],
            },
            transactions: vec![],
        },
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
        .map(|tx_body| {
            let raw = tx_body.to_cbor_bytes();
            Tx {
                id: compute_tx_id(&raw),
                body: raw,
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
        .map(|tx_body| {
            let raw = tx_body.to_cbor_bytes();
            Tx {
                id: compute_tx_id(&raw),
                body: raw,
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
        .map(|tx_body| {
            let raw = tx_body.to_cbor_bytes();
            Tx {
                id: compute_tx_id(&raw),
                body: raw,
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
    from_point: Vec<u8>,
) -> Result<MultiEraSyncStep, SyncError> {
    let step = sync_step(chain_sync, block_fetch, from_point).await?;
    match step {
        SyncStep::RollForward {
            header,
            tip,
            blocks,
        } => Ok(MultiEraSyncStep::RollForward {
            raw_header: header,
            tip: decode_point(&tip)?,
            blocks: decode_multi_era_blocks(&blocks)?,
        }),
        SyncStep::RollBackward { point, tip } => Ok(MultiEraSyncStep::RollBackward {
            point: decode_point(&point)?,
            tip: decode_point(&tip)?,
        }),
    }
}

/// Apply one multi-era sync step into a volatile store.
///
/// Roll-forward blocks are converted to generic `Block` values and appended.
/// Roll-backward steps trigger a store rollback to the given point.
pub fn apply_multi_era_step_to_volatile<S: VolatileStore>(
    store: &mut S,
    step: &MultiEraSyncStep,
) -> Result<(), SyncError> {
    match step {
        MultiEraSyncStep::RollForward { blocks, .. } => {
            for b in blocks {
                store.add_block(multi_era_block_to_block(b))?;
            }
        }
        MultiEraSyncStep::RollBackward { point, .. } => {
            store.rollback_to(point);
        }
    }
    Ok(())
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
    mut from_point: Point,
    batch_size: usize,
    verification: Option<&VerificationConfig>,
) -> Result<MultiEraSyncProgress, SyncError> {
    let mut steps = Vec::new();
    let mut fetched_blocks = 0usize;
    let mut rollback_count = 0usize;

    for _ in 0..batch_size {
        let raw_step = sync_step(
            chain_sync,
            block_fetch,
            from_point.to_cbor_bytes(),
        )
        .await?;

        let me_step = match raw_step {
            SyncStep::RollForward {
                header,
                tip,
                blocks: raw_blocks,
            } => {
                // Body-hash verification on raw bytes (before decode).
                if let Some(config) = verification {
                    if config.verify_body_hash {
                        for raw in &raw_blocks {
                            verify_block_body_hash(raw)?;
                        }
                    }
                }

                let decoded_tip = decode_point(&tip)?;
                let decoded_blocks = decode_multi_era_blocks(&raw_blocks)?;

                // Header verification on decoded blocks.
                if let Some(config) = verification {
                    for block in &decoded_blocks {
                        verify_multi_era_block(block, config)?;
                    }
                }

                from_point = decoded_tip;
                fetched_blocks += decoded_blocks.len();

                MultiEraSyncStep::RollForward {
                    raw_header: header,
                    tip: decoded_tip,
                    blocks: decoded_blocks,
                }
            }
            SyncStep::RollBackward { point, tip } => {
                let decoded_point = decode_point(&point)?;
                from_point = decoded_point;
                rollback_count += 1;

                MultiEraSyncStep::RollBackward {
                    point: decoded_point,
                    tip: decode_point(&tip)?,
                }
            }
        };

        apply_multi_era_step_to_volatile(store, &me_step)?;
        steps.push(me_step);
    }

    Ok(MultiEraSyncProgress {
        current_point: from_point,
        steps,
        fetched_blocks,
        rollback_count,
    })
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
