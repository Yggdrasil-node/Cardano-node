//! Node runtime — wires networking, storage, and protocol client drivers
//! into a cohesive sync lifecycle.
//!
//! Reference: `cardano-node/src/Cardano/Node/Run.hs`.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use crate::config::{derive_peer_snapshot_freshness, load_peer_snapshot_file};
use crate::sync::{
    LedgerCheckpointTracking, LedgerCheckpointUpdateOutcome, LedgerRecoveryOutcome, SyncError,
    VerifiedSyncServiceConfig, apply_nonce_evolution_to_progress,
    recover_ledger_state_chaindb, sync_batch_apply_verified, track_chain_state,
    track_chain_state_entries, update_ledger_checkpoint_after_progress,
};
use crate::tracer::{NodeTracer, trace_fields};
use serde_json::json;
use serde_json::Value;
use yggdrasil_consensus::{ChainState, NonceEvolutionState};
use yggdrasil_network::{
    AfterSlot, BlockFetchClient, ChainSyncClient, HandshakeVersion, KeepAliveClient,
    LedgerPeerSnapshot, LedgerPeerUseDecision, LedgerStateJudgement, MiniProtocolNum,
    NodeToNodeVersionData, PeerAccessPoint, PeerConnection, PeerError,
    PeerSnapshotFreshness, PeerAttemptState, TxIdAndSize, TxServerRequest,
    TxSubmissionClient, TxSubmissionClientError, UseLedgerPeers, judge_ledger_peer_usage,
    peer_attempt_state, resolve_peer_access_points,
};
use yggdrasil_ledger::{LedgerError, LedgerState, MultiEraSubmittedTx, Point, SlotNo, TxId};
use yggdrasil_mempool::{
    Mempool, MempoolEntry, MempoolError, MempoolIdx, MempoolSnapshot,
    SharedMempool, MEMPOOL_ZERO_IDX, SharedTxSubmissionMempoolReader,
    TxSubmissionMempoolReader,
};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

// ---------------------------------------------------------------------------
// TxSubmission mempool integration
// ---------------------------------------------------------------------------

/// Result of attempting to add a single transaction to the mempool.
///
/// This mirrors the upstream `MempoolAddTxResult` split between accepted and
/// rejected transactions while keeping queue-level failures separate.
#[derive(Debug, Eq, PartialEq)]
pub enum MempoolAddTxResult {
    /// The transaction was validated and added to the mempool.
    MempoolTxAdded(TxId),
    /// The transaction was rejected by ledger validation and not added.
    MempoolTxRejected(TxId, LedgerError),
}

/// Queue-level failures encountered while adding a transaction to the mempool.
#[derive(Debug, thiserror::Error)]
pub enum MempoolAddTxError {
    /// Underlying mempool capacity, duplicate, or TTL error.
    #[error("mempool admission error: {0}")]
    Mempool(#[from] MempoolError),
}

fn admitted_entry(tx: MultiEraSubmittedTx) -> MempoolEntry {
    let fee = tx.fee();
    let ttl = tx.expires_at().unwrap_or(SlotNo(u64::MAX));
    MempoolEntry::from_multi_era_submitted_tx(tx, fee, ttl)
}

fn add_tx_with<F>(
    ledger: &mut LedgerState,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
    mut insert_entry: F,
) -> Result<MempoolAddTxResult, MempoolAddTxError>
where
    F: FnMut(MempoolEntry) -> Result<(), MempoolError>,
{
    let tx_id = tx.tx_id();
    let mut staged_ledger = ledger.clone();
    match staged_ledger.apply_submitted_tx(&tx, current_slot) {
        Ok(()) => {
            insert_entry(admitted_entry(tx))?;
            *ledger = staged_ledger;
            Ok(MempoolAddTxResult::MempoolTxAdded(tx_id))
        }
        Err(err) => Ok(MempoolAddTxResult::MempoolTxRejected(tx_id, err)),
    }
}

/// Validate and add a single transaction to the mempool.
///
/// The transaction is first applied to a staged clone of the caller-provided
/// ledger state. If ledger validation fails, the ledger and mempool remain
/// unchanged and the result is `MempoolTxRejected`. If validation succeeds, the
/// transaction is inserted into the mempool and the staged ledger state is
/// committed.
pub fn add_tx_to_mempool(
    ledger: &mut LedgerState,
    mempool: &mut Mempool,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
) -> Result<MempoolAddTxResult, MempoolAddTxError> {
    add_tx_with(ledger, tx, current_slot, |entry| {
        mempool.insert_checked(entry, current_slot)
    })
}

/// Validate and add a single transaction to a shared mempool.
///
/// This is the shared-handle variant of [`add_tx_to_mempool`]. Accepted
/// transactions update the caller's ledger state only after the shared mempool
/// insert succeeds.
pub fn add_tx_to_shared_mempool(
    ledger: &mut LedgerState,
    mempool: &SharedMempool,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
) -> Result<MempoolAddTxResult, MempoolAddTxError> {
    add_tx_with(ledger, tx, current_slot, |entry| {
        mempool.insert_checked(entry, current_slot)
    })
}

/// Validate and add a sequence of transactions to the mempool in order.
///
/// This mirrors the upstream `addTxs` semantics: each transaction is checked
/// against the ledger state produced by all previously accepted transactions in
/// the same batch. Rejected transactions do not advance the staged ledger
/// state. Queue-level failures stop the batch and return an error.
pub fn add_txs_to_mempool<I>(
    ledger: &mut LedgerState,
    mempool: &mut Mempool,
    txs: I,
    current_slot: SlotNo,
) -> Result<Vec<MempoolAddTxResult>, MempoolAddTxError>
where
    I: IntoIterator<Item = MultiEraSubmittedTx>,
{
    txs.into_iter()
        .map(|tx| add_tx_to_mempool(ledger, mempool, tx, current_slot))
        .collect()
}

/// Validate and add a sequence of transactions to a shared mempool in order.
///
/// Accepted transactions update the caller's ledger state one by one so later
/// transactions in the batch can depend on earlier accepted outputs.
pub fn add_txs_to_shared_mempool<I>(
    ledger: &mut LedgerState,
    mempool: &SharedMempool,
    txs: I,
    current_slot: SlotNo,
) -> Result<Vec<MempoolAddTxResult>, MempoolAddTxError>
where
    I: IntoIterator<Item = MultiEraSubmittedTx>,
{
    txs.into_iter()
        .map(|tx| add_tx_to_shared_mempool(ledger, mempool, tx, current_slot))
        .collect()
}

/// Errors from serving TxSubmission requests out of a mempool snapshot.
#[derive(Debug, thiserror::Error)]
pub enum TxSubmissionServiceError {
    /// Underlying TxSubmission protocol client error.
    #[error("tx-submission client error: {0}")]
    Client(#[from] TxSubmissionClientError),
}

/// Outcome returned when the managed TxSubmission service finishes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TxSubmissionServiceOutcome {
    /// Number of TxSubmission requests handled by the service.
    pub handled_requests: usize,
    /// `true` when the protocol terminated normally via `MsgDone`, `false`
    /// when the service stopped due to shutdown.
    pub terminated_by_protocol: bool,
}

trait TxSubmissionSnapshotReader {
    fn mempool_get_snapshot(&self) -> MempoolSnapshot;
}

impl TxSubmissionSnapshotReader for TxSubmissionMempoolReader<'_> {
    fn mempool_get_snapshot(&self) -> MempoolSnapshot {
        self.mempool_get_snapshot()
    }
}

impl TxSubmissionSnapshotReader for SharedTxSubmissionMempoolReader {
    fn mempool_get_snapshot(&self) -> MempoolSnapshot {
        self.mempool_get_snapshot()
    }
}

/// Serve a single TxSubmission request using the current mempool contents.
///
/// Tx ids are advertised from a TxSubmission mempool snapshot using the
/// monotonic `last_idx` cursor expected by the outbound side. For blocking
/// requests with no available transactions after `last_idx`, the helper
/// terminates the mini-protocol with `MsgDone` and returns `Ok(false)`.
async fn serve_txsubmission_request_from_snapshot_reader<R>(
    client: &mut TxSubmissionClient,
    reader: &R,
    last_idx: &mut MempoolIdx,
) -> Result<bool, TxSubmissionServiceError>
where
    R: TxSubmissionSnapshotReader,
{
    match client.recv_request().await? {
        TxServerRequest::RequestTxIds { blocking, req, .. } => {
            let snapshot = reader.mempool_get_snapshot();
            let txids = snapshot
                .mempool_txids_after(*last_idx)
                .into_iter()
                .take(req as usize)
                .map(|(txid, idx, size_bytes)| {
                    *last_idx = idx;
                    TxIdAndSize {
                        txid,
                        size: size_bytes.min(u32::MAX as usize) as u32,
                    }
                })
                .collect::<Vec<_>>();

            if txids.is_empty() && blocking {
                client.send_done().await?;
                Ok(false)
            } else {
                client.reply_tx_ids(txids).await?;
                Ok(true)
            }
        }
        TxServerRequest::RequestTxs { txids } => {
            let snapshot = reader.mempool_get_snapshot();
            let txs = txids
                .into_iter()
                .filter_map(|txid| snapshot.mempool_lookup_tx_by_id(&txid))
                .map(|entry| entry.raw_tx.clone())
                .collect::<Vec<_>>();
            client.reply_txs(txs).await?;
            Ok(true)
        }
    }
}

pub async fn serve_txsubmission_request_from_reader(
    client: &mut TxSubmissionClient,
    reader: &TxSubmissionMempoolReader<'_>,
    last_idx: &mut MempoolIdx,
) -> Result<bool, TxSubmissionServiceError> {
    serve_txsubmission_request_from_snapshot_reader(client, reader, last_idx).await
}

/// Run a managed TxSubmission loop backed by a shared mempool snapshot source
/// until shutdown or protocol termination.
///
/// This variant allows concurrent mempool updates while the service is
/// running. Each request takes a fresh snapshot from the shared handle and
/// continues advertising from the previously served `last_idx` position.
pub async fn run_txsubmission_service_shared<F>(
    client: &mut TxSubmissionClient,
    mempool: &SharedMempool,
    shutdown: F,
) -> Result<TxSubmissionServiceOutcome, TxSubmissionServiceError>
where
    F: Future<Output = ()>,
{
    client.init().await?;
    tokio::pin!(shutdown);

    let mut handled_requests = 0usize;
    let reader = mempool.txsubmission_mempool_reader();
    let mut last_idx = MEMPOOL_ZERO_IDX;

    loop {
        let serve_fut =
            serve_txsubmission_request_from_snapshot_reader(client, &reader, &mut last_idx);

        tokio::select! {
            biased;

            () = &mut shutdown => {
                return Ok(TxSubmissionServiceOutcome {
                    handled_requests,
                    terminated_by_protocol: false,
                });
            }

            result = serve_fut => {
                handled_requests += 1;
                let should_continue = result?;
                if !should_continue {
                    return Ok(TxSubmissionServiceOutcome {
                        handled_requests,
                        terminated_by_protocol: true,
                    });
                }
            }
        }
    }
}

/// Serve a single TxSubmission request using the current mempool contents.
///
/// Tx ids are advertised in the mempool's existing fee-descending order. For
/// blocking requests with no available transactions, the helper terminates the
/// mini-protocol with `MsgDone` and returns `Ok(false)`.
pub async fn serve_txsubmission_request_from_mempool(
    client: &mut TxSubmissionClient,
    mempool: &Mempool,
) -> Result<bool, TxSubmissionServiceError> {
    match client.recv_request().await? {
        TxServerRequest::RequestTxIds { blocking, req, .. } => {
            let txids = mempool
                .iter()
                .take(req as usize)
                .map(|entry| TxIdAndSize {
                    txid: entry.tx_id,
                    size: entry.size_bytes.min(u32::MAX as usize) as u32,
                })
                .collect::<Vec<_>>();

            if txids.is_empty() && blocking {
                client.send_done().await?;
                Ok(false)
            } else {
                client.reply_tx_ids(txids).await?;
                Ok(true)
            }
        }
        TxServerRequest::RequestTxs { txids } => {
            let txs = txids
                .into_iter()
                .filter_map(|txid| mempool.iter().find(|entry| entry.tx_id == txid))
                .map(|entry| entry.raw_tx.clone())
                .collect::<Vec<_>>();
            client.reply_txs(txs).await?;
            Ok(true)
        }
    }
}

/// Run a managed TxSubmission loop backed by the current mempool snapshot
/// until shutdown or protocol termination.
///
/// The service sends `MsgInit` once, then repeatedly serves incoming
/// TxSubmission requests from the provided mempool. If a blocking request
/// arrives while the mempool is empty, the helper terminates the protocol with
/// `MsgDone` and returns an outcome marked as protocol-terminated.
pub async fn run_txsubmission_service<F>(
    client: &mut TxSubmissionClient,
    mempool: &Mempool,
    shutdown: F,
) -> Result<TxSubmissionServiceOutcome, TxSubmissionServiceError>
where
    F: Future<Output = ()>,
{
    client.init().await?;
    tokio::pin!(shutdown);

    let mut handled_requests = 0usize;
    let reader = mempool.txsubmission_mempool_reader();
    let mut last_idx = MEMPOOL_ZERO_IDX;

    loop {
        let serve_fut =
            serve_txsubmission_request_from_snapshot_reader(client, &reader, &mut last_idx);

        tokio::select! {
            biased;

            () = &mut shutdown => {
                return Ok(TxSubmissionServiceOutcome {
                    handled_requests,
                    terminated_by_protocol: false,
                });
            }

            result = serve_fut => {
                handled_requests += 1;
                let should_continue = result?;
                if !should_continue {
                    return Ok(TxSubmissionServiceOutcome {
                        handled_requests,
                        terminated_by_protocol: true,
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// NodeConfig
// ---------------------------------------------------------------------------

/// Minimal configuration for establishing a node-to-node connection.
///
/// This covers the subset needed for initial sync bootstrapping.
pub struct NodeConfig {
    /// Address of the upstream peer to connect to.
    pub peer_addr: SocketAddr,
    /// The network magic for the target network (e.g. mainnet = 764824073).
    pub network_magic: u32,
    /// Protocol versions to propose during handshake, ordered by preference.
    pub protocol_versions: Vec<HandshakeVersion>,
}

// ---------------------------------------------------------------------------
// PeerSession — result of bootstrapping a connection
// ---------------------------------------------------------------------------

/// A fully-negotiated peer session with typed protocol drivers ready for use.
///
/// Owns the [`PeerConnection`]'s mux handle and exposes each data-protocol
/// client as a named field.
pub struct PeerSession {
    /// Upstream peer address that completed the handshake.
    pub connected_peer_addr: SocketAddr,
    /// ChainSync client driver.
    pub chain_sync: ChainSyncClient,
    /// BlockFetch client driver.
    pub block_fetch: BlockFetchClient,
    /// KeepAlive client driver.
    pub keep_alive: KeepAliveClient,
    /// TxSubmission client driver.
    pub tx_submission: TxSubmissionClient,
    /// Mux handle — abort to tear down the connection.
    pub mux: yggdrasil_network::MuxHandle,
    /// Negotiated protocol version.
    pub version: HandshakeVersion,
    /// Agreed-upon version data.
    pub version_data: NodeToNodeVersionData,
}

/// Outcome returned when the reconnecting verified sync runner stops.
#[derive(Clone, Debug)]
pub struct ReconnectingSyncServiceOutcome {
    /// Final chain point when the service stopped.
    pub final_point: Point,
    /// Total blocks fetched across all batches.
    pub total_blocks: usize,
    /// Total rollback events across all batches.
    pub total_rollbacks: usize,
    /// Number of batch iterations completed.
    pub batches_completed: usize,
    /// Final nonce evolution state (present when nonce tracking was enabled).
    pub nonce_state: Option<NonceEvolutionState>,
    /// Final chain state (present when chain tracking was enabled).
    pub chain_state: Option<ChainState>,
    /// Total number of blocks that crossed the stability window during the run.
    pub stable_block_count: usize,
    /// Number of reconnects performed after the initial successful session.
    pub reconnect_count: usize,
    /// The most recent peer that successfully completed bootstrap.
    pub last_connected_peer_addr: Option<SocketAddr>,
}

/// Outcome returned when a coordinated-storage sync run first restores ledger
/// state from `ChainDb` recovery data and then starts reconnecting sync.
#[derive(Clone, Debug)]
pub struct ResumedSyncServiceOutcome {
    /// Ledger recovery state rebuilt before live syncing begins.
    pub recovery: LedgerRecoveryOutcome,
    /// Outcome from the reconnecting live sync loop started at the recovered point.
    pub sync: ReconnectingSyncServiceOutcome,
}

/// Request parameters for reconnecting verified sync runners.
pub struct ReconnectingVerifiedSyncRequest<'a> {
    /// Node-to-node bootstrap configuration.
    pub node_config: &'a NodeConfig,
    /// Ordered fallback peers tried after the primary peer.
    pub fallback_peer_addrs: &'a [SocketAddr],
    /// Chain point from which live sync should begin.
    pub from_point: Point,
    /// Verified sync policy and batch configuration.
    pub config: &'a VerifiedSyncServiceConfig,
    /// Optional nonce-evolution state to carry through the run.
    pub nonce_state: Option<NonceEvolutionState>,
    /// Optional ledger-peer policy for refreshing ChainDb reconnect targets.
    pub use_ledger_peers: Option<UseLedgerPeers>,
    /// Optional resolved peer snapshot file path for reconnect-time refresh.
    pub peer_snapshot_path: Option<PathBuf>,
}

/// Request parameters for coordinated-storage reconnecting sync resumption.
pub struct ResumeReconnectingVerifiedSyncRequest<'a> {
    /// Node-to-node bootstrap configuration.
    pub node_config: &'a NodeConfig,
    /// Ordered fallback peers tried after the primary peer.
    pub fallback_peer_addrs: &'a [SocketAddr],
    /// Base ledger state used before replaying persisted recovery data.
    pub base_ledger_state: LedgerState,
    /// Verified sync policy and batch configuration.
    pub config: &'a VerifiedSyncServiceConfig,
    /// Optional nonce-evolution state to carry through the resumed run.
    pub nonce_state: Option<NonceEvolutionState>,
    /// Optional ledger-peer policy for refreshing ChainDb reconnect targets.
    pub use_ledger_peers: Option<UseLedgerPeers>,
    /// Optional resolved peer snapshot file path for reconnect-time refresh.
    pub peer_snapshot_path: Option<PathBuf>,
}

type CheckpointTracking = LedgerCheckpointTracking;

struct ReconnectingVerifiedSyncContext<'a> {
    node_config: &'a NodeConfig,
    fallback_peer_addrs: &'a [SocketAddr],
    use_ledger_peers: Option<UseLedgerPeers>,
    peer_snapshot_path: Option<&'a Path>,
    config: &'a VerifiedSyncServiceConfig,
    tracer: &'a NodeTracer,
}

struct ReconnectingVerifiedSyncState {
    from_point: Point,
    nonce_state: Option<NonceEvolutionState>,
    checkpoint_tracking: Option<CheckpointTracking>,
}

fn extend_unique_socket_addrs(
    target: &mut Vec<SocketAddr>,
    peers: impl IntoIterator<Item = SocketAddr>,
) {
    for peer in peers {
        if !target.contains(&peer) {
            target.push(peer);
        }
    }
}

fn refresh_chain_db_reconnect_fallback_peers(
    primary_peer: SocketAddr,
    fallback_peer_addrs: &[SocketAddr],
    checkpoint_tracking: Option<&CheckpointTracking>,
    use_ledger_peers: Option<UseLedgerPeers>,
    peer_snapshot_path: Option<&Path>,
    tracer: &NodeTracer,
) -> Vec<SocketAddr> {
    let mut refreshed = fallback_peer_addrs.to_vec();

    let Some(checkpoint_tracking) = checkpoint_tracking else {
        return refreshed;
    };

    let use_ledger_peers = use_ledger_peers.unwrap_or(UseLedgerPeers::DontUseLedgerPeers);
    let latest_slot = checkpoint_tracking.ledger_state.tip.slot().map(|slot| slot.0);
    let ledger_allowed = match use_ledger_peers {
        UseLedgerPeers::DontUseLedgerPeers => false,
        UseLedgerPeers::UseLedgerPeers(AfterSlot::Always) => true,
        UseLedgerPeers::UseLedgerPeers(AfterSlot::After(after_slot)) => checkpoint_tracking
            .ledger_state
            .tip
            .slot()
            .is_some_and(|slot| slot.0 >= after_slot),
    };

    let mut ledger_peers = Vec::new();
    if ledger_allowed {
        for access_point in checkpoint_tracking.ledger_state.pool_state().relay_access_points() {
            let peer_access_point = PeerAccessPoint {
                address: access_point.address,
                port: access_point.port,
            };
            extend_unique_socket_addrs(
                &mut ledger_peers,
                resolve_peer_access_points(&peer_access_point),
            );
        }
    }

    let mut snapshot_slot = None;
    let mut snapshot_available = peer_snapshot_path.is_none();
    let mut snapshot = LedgerPeerSnapshot::new(ledger_peers, Vec::new());

    if let Some(peer_snapshot_path) = peer_snapshot_path {
        match load_peer_snapshot_file(peer_snapshot_path) {
            Ok(loaded_snapshot) => {
                snapshot_slot = loaded_snapshot.slot;
                snapshot_available = true;
                let mut merged_ledger_peers = snapshot.ledger_peers;
                extend_unique_socket_addrs(&mut merged_ledger_peers, loaded_snapshot.snapshot.ledger_peers);

                snapshot = LedgerPeerSnapshot::new(
                    merged_ledger_peers,
                    loaded_snapshot.snapshot.big_ledger_peers,
                );
            }
            Err(err) => {
                snapshot_available = false;
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to refresh reconnect peer snapshot",
                    trace_fields([
                        ("snapshotPath", json!(peer_snapshot_path.display().to_string())),
                        ("error", json!(err.to_string())),
                    ]),
                );
            }
        }
    }

    let freshness: PeerSnapshotFreshness = derive_peer_snapshot_freshness(
        use_ledger_peers,
        peer_snapshot_path.is_some(),
        snapshot_slot,
        latest_slot,
        snapshot_available,
    );
    let decision = judge_ledger_peer_usage(
        use_ledger_peers,
        latest_slot,
        LedgerStateJudgement::YoungEnough,
        freshness,
    );

    tracer.trace_runtime(
        "Net.PeerSelection",
        "Info",
        "evaluated reconnect ledger-derived peers",
        trace_fields([
            ("decision", json!(format!("{decision:?}"))),
            ("latestSlot", json!(latest_slot)),
            ("snapshotSlot", json!(snapshot_slot)),
            ("ledgerPeerCount", json!(snapshot.ledger_peers.len())),
            ("bigLedgerPeerCount", json!(snapshot.big_ledger_peers.len())),
            ("peerSnapshotFreshness", json!(format!("{freshness:?}"))),
        ]),
    );

    if decision != LedgerPeerUseDecision::Eligible {
        return refreshed;
    }

    extend_unique_socket_addrs(
        &mut refreshed,
        snapshot
            .ledger_peers
            .into_iter()
            .chain(snapshot.big_ledger_peers)
            .filter(|peer| *peer != primary_peer),
    );
    refreshed
}

type CheckpointPersistenceOutcome = LedgerCheckpointUpdateOutcome;

fn checkpoint_trace_fields(
    outcome: &CheckpointPersistenceOutcome,
    policy: &crate::sync::LedgerCheckpointPolicy,
) -> BTreeMap<String, Value> {
    match outcome {
        CheckpointPersistenceOutcome::ClearedDisabled => trace_fields([
            ("action", json!("cleared-disabled")),
            ("checkpointIntervalSlots", json!(policy.min_slot_delta)),
            ("maxLedgerSnapshots", json!(policy.max_snapshots)),
        ]),
        CheckpointPersistenceOutcome::ClearedOrigin => trace_fields([
            ("action", json!("cleared-origin")),
            ("checkpointIntervalSlots", json!(policy.min_slot_delta)),
            ("maxLedgerSnapshots", json!(policy.max_snapshots)),
        ]),
        CheckpointPersistenceOutcome::Persisted {
            slot,
            retained_snapshots,
            pruned_snapshots,
            rollback_count,
        } => trace_fields([
            ("action", json!("persisted")),
            ("slot", json!(slot.0)),
            ("retainedSnapshots", json!(retained_snapshots)),
            ("prunedSnapshots", json!(pruned_snapshots)),
            ("rollbackCount", json!(rollback_count)),
            ("checkpointIntervalSlots", json!(policy.min_slot_delta)),
            ("maxLedgerSnapshots", json!(policy.max_snapshots)),
        ]),
        CheckpointPersistenceOutcome::Skipped {
            slot,
            rollback_count,
            since_last_slot_delta,
        } => trace_fields([
            ("action", json!("skipped")),
            ("slot", json!(slot.0)),
            ("rollbackCount", json!(rollback_count)),
            ("sinceLastSlotDelta", json!(since_last_slot_delta)),
            ("checkpointIntervalSlots", json!(policy.min_slot_delta)),
            ("maxLedgerSnapshots", json!(policy.max_snapshots)),
        ]),
    }
}

fn trace_checkpoint_outcome(
    tracer: &NodeTracer,
    outcome: &CheckpointPersistenceOutcome,
    policy: &crate::sync::LedgerCheckpointPolicy,
) {
    let (severity, message) = match outcome {
        CheckpointPersistenceOutcome::Persisted { .. } => ("Info", "ledger checkpoint persisted"),
        CheckpointPersistenceOutcome::Skipped { .. } => ("Info", "ledger checkpoint skipped"),
        CheckpointPersistenceOutcome::ClearedDisabled => {
            ("Notice", "ledger checkpoints cleared because persistence is disabled")
        }
        CheckpointPersistenceOutcome::ClearedOrigin => {
            ("Notice", "ledger checkpoints cleared at origin")
        }
    };

    tracer.trace_runtime(
        "Node.Recovery.Checkpoint",
        severity,
        message,
        checkpoint_trace_fields(outcome, policy),
    );
}

async fn run_reconnecting_verified_sync_service_chaindb_inner<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    context: ReconnectingVerifiedSyncContext<'_>,
    state: ReconnectingVerifiedSyncState,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncContext {
        node_config,
        fallback_peer_addrs,
        use_ledger_peers,
        peer_snapshot_path,
        config,
        tracer,
    } = context;
    let ReconnectingVerifiedSyncState {
        mut from_point,
        mut nonce_state,
        mut checkpoint_tracking,
    } = state;

    tokio::pin!(shutdown);

    let mut total_blocks = 0usize;
    let mut total_rollbacks = 0usize;
    let mut batches_completed = 0usize;
    let mut total_stable = 0usize;
    let mut reconnect_count = 0usize;
    let mut last_connected_peer_addr = None;
    let mut chain_state = config.security_param.map(ChainState::new);
    let mut had_session = false;
    let mut preferred_peer = None;

    loop {
        let refreshed_fallback_peers = refresh_chain_db_reconnect_fallback_peers(
            node_config.peer_addr,
            fallback_peer_addrs,
            checkpoint_tracking.as_ref(),
            use_ledger_peers,
            peer_snapshot_path,
            tracer,
        );
        let mut attempt_state = peer_attempt_state(node_config.peer_addr, &refreshed_fallback_peers);
        if let Some(peer_addr) = preferred_peer {
            attempt_state.record_success(peer_addr);
        }

        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "refreshed reconnect peer candidates",
            trace_fields([
                ("fallbackPeerCount", json!(refreshed_fallback_peers.len())),
                (
                    "latestSlot",
                    json!(checkpoint_tracking
                        .as_ref()
                        .and_then(|tracking| tracking.ledger_state.tip.slot().map(|slot| slot.0))),
                ),
                ("useLedgerPeers", json!(use_ledger_peers.map(|policy| format!("{policy:?}")))),
            ]),
        );

        let mut session = tokio::select! {
            biased;

            () = &mut shutdown => {
                tracer.trace_runtime(
                    "Node.Shutdown",
                    "Notice",
                    "shutdown requested before bootstrap completed",
                    BTreeMap::new(),
                );
                return Ok(ReconnectingSyncServiceOutcome {
                    final_point: from_point,
                    total_blocks,
                    total_rollbacks,
                    batches_completed,
                    nonce_state,
                    chain_state,
                    stable_block_count: total_stable,
                    reconnect_count,
                    last_connected_peer_addr,
                });
            }

            result = bootstrap_with_attempt_state(node_config, &mut attempt_state, tracer) => result?,
        };

        if had_session {
            reconnect_count += 1;
        } else {
            had_session = true;
        }
        last_connected_peer_addr = Some(session.connected_peer_addr);
        preferred_peer = Some(session.connected_peer_addr);

        tracer.trace_runtime(
            "Net.ConnectionManager.Remote",
            "Notice",
            if reconnect_count == 0 {
                "verified sync session established"
            } else {
                "verified sync session re-established"
            },
            trace_fields([
                ("peer", json!(session.connected_peer_addr.to_string())),
                ("reconnectCount", json!(reconnect_count)),
                ("fromPoint", json!(format!("{:?}", from_point))),
            ]),
        );

        loop {
            let batch_fut = sync_batch_apply_verified(
                &mut session.chain_sync,
                &mut session.block_fetch,
                chain_db.volatile_mut(),
                from_point,
                config.batch_size,
                Some(&config.verification),
            );

            tokio::select! {
                biased;

                () = &mut shutdown => {
                    tracer.trace_runtime(
                        "Node.Shutdown",
                        "Notice",
                        "shutdown requested during sync session",
                        trace_fields([
                            ("peer", json!(session.connected_peer_addr.to_string())),
                            ("currentPoint", json!(format!("{:?}", from_point))),
                        ]),
                    );
                    session.mux.abort();
                    return Ok(ReconnectingSyncServiceOutcome {
                        final_point: from_point,
                        total_blocks,
                        total_rollbacks,
                        batches_completed,
                        nonce_state,
                        chain_state,
                        stable_block_count: total_stable,
                        reconnect_count,
                        last_connected_peer_addr,
                    });
                }

                result = batch_fut => {
                    match result {
                        Ok(progress) => {
                            from_point = progress.current_point;
                            total_blocks += progress.fetched_blocks;
                            total_rollbacks += progress.rollback_count;
                            batches_completed += 1;

                            if let Some(ref mut cs) = chain_state {
                                for step in &progress.steps {
                                    let stable_entries = track_chain_state_entries(cs, step)?;
                                    total_stable += stable_entries.len();
                                    if let Some(last_stable) = stable_entries.last() {
                                        let point = Point::BlockPoint(last_stable.slot, last_stable.hash);
                                        chain_db.promote_volatile_prefix(&point)?;
                                    }
                                }
                            }

                            if let Some((ref mut state, nonce_cfg)) =
                                nonce_state.as_mut().zip(config.nonce_config.as_ref())
                            {
                                apply_nonce_evolution_to_progress(state, &progress, nonce_cfg);
                            }

                            if let Some(ref mut tracking) = checkpoint_tracking {
                                let checkpoint_outcome = update_ledger_checkpoint_after_progress(
                                    chain_db,
                                    tracking,
                                    &progress,
                                    &config.checkpoint_policy,
                                )?;
                                trace_checkpoint_outcome(
                                    tracer,
                                    &checkpoint_outcome,
                                    &config.checkpoint_policy,
                                );
                            }

                            tracer.trace_runtime(
                                "ChainSync.Client",
                                "Info",
                                "verified sync batch applied",
                                trace_fields([
                                    ("peer", json!(session.connected_peer_addr.to_string())),
                                    ("currentPoint", json!(format!("{:?}", from_point))),
                                    ("batchFetchedBlocks", json!(progress.fetched_blocks)),
                                    ("batchRollbacks", json!(progress.rollback_count)),
                                    ("totalBlocks", json!(total_blocks)),
                                    ("batchesCompleted", json!(batches_completed)),
                                    ("stableBlocks", json!(total_stable)),
                                    ("checkpointTracked", json!(checkpoint_tracking.is_some())),
                                ]),
                            );
                        }
                        Err(SyncError::ChainSync(err)) => {
                            tracer.trace_runtime(
                                "ChainSync.Client",
                                "Warning",
                                "chainsync connectivity lost; reconnecting",
                                trace_fields([
                                    ("peer", json!(session.connected_peer_addr.to_string())),
                                    ("error", json!(err.to_string())),
                                    ("currentPoint", json!(format!("{:?}", from_point))),
                                ]),
                            );
                            session.mux.abort();
                            break;
                        }
                        Err(SyncError::BlockFetch(err)) => {
                            tracer.trace_runtime(
                                "BlockFetch.Client.CompletedBlockFetch",
                                "Warning",
                                "blockfetch connectivity lost; reconnecting",
                                trace_fields([
                                    ("peer", json!(session.connected_peer_addr.to_string())),
                                    ("error", json!(err.to_string())),
                                    ("currentPoint", json!(format!("{:?}", from_point))),
                                ]),
                            );
                            session.mux.abort();
                            break;
                        }
                        Err(err) => {
                            tracer.trace_runtime(
                                "Node.Sync",
                                "Error",
                                "verified sync service failed",
                                trace_fields([
                                    ("peer", json!(session.connected_peer_addr.to_string())),
                                    ("error", json!(err.to_string())),
                                    ("currentPoint", json!(format!("{:?}", from_point))),
                                ]),
                            );
                            session.mux.abort();
                            return Err(err);
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// bootstrap
// ---------------------------------------------------------------------------

/// Connect to an upstream peer and set up all protocol client drivers.
///
/// This is the main runtime entry point for syncing from a remote node.
///
/// # Errors
///
/// Returns `PeerError` if the TCP connection or handshake fails.
pub async fn bootstrap(config: &NodeConfig) -> Result<PeerSession, PeerError> {
    bootstrap_with_fallbacks(config, &[]).await
}

/// Connect to the primary upstream peer, retrying ordered fallbacks on failure.
///
/// The primary address in [`NodeConfig`] is always attempted first. Fallback
/// peers are then tried in the provided order, skipping duplicates.
pub async fn bootstrap_with_fallbacks(
    config: &NodeConfig,
    fallback_peer_addrs: &[SocketAddr],
) -> Result<PeerSession, PeerError> {
    let tracer = NodeTracer::disabled();
    let mut attempt_state = peer_attempt_state(config.peer_addr, fallback_peer_addrs);
    bootstrap_with_attempt_state(config, &mut attempt_state, &tracer).await
}

async fn bootstrap_with_attempt_state(
    config: &NodeConfig,
    attempt_state: &mut PeerAttemptState,
    tracer: &NodeTracer,
) -> Result<PeerSession, PeerError> {
    let proposals: Vec<(HandshakeVersion, NodeToNodeVersionData)> = config
        .protocol_versions
        .iter()
        .map(|v| {
            (
                *v,
                NodeToNodeVersionData {
                    network_magic: config.network_magic,
                    initiator_only_diffusion_mode: false,
                    peer_sharing: 0,
                    query: false,
                },
            )
        })
        .collect();

    let candidate_peer_addrs = attempt_state.attempt_order();

    let mut last_error = None;
    let mut connected_peer_addr = config.peer_addr;
    let mut conn_opt = None;

    for (attempt_index, peer_addr) in candidate_peer_addrs.into_iter().enumerate() {
        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "attempting bootstrap peer",
            trace_fields([
                ("attempt", json!(attempt_index + 1)),
                ("peer", json!(peer_addr.to_string())),
                ("networkMagic", json!(config.network_magic)),
            ]),
        );

        match yggdrasil_network::peer_connect(peer_addr, proposals.clone()).await {
            Ok(conn) => {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Info",
                    "bootstrap peer connected",
                    trace_fields([
                        ("attempt", json!(attempt_index + 1)),
                        ("peer", json!(peer_addr.to_string())),
                    ]),
                );
                connected_peer_addr = peer_addr;
                attempt_state.record_success(peer_addr);
                conn_opt = Some(conn);
                break;
            }
            Err(err) => {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "bootstrap peer failed",
                    trace_fields([
                        ("attempt", json!(attempt_index + 1)),
                        ("peer", json!(peer_addr.to_string())),
                        ("error", json!(err.to_string())),
                    ]),
                );
                last_error = Some(err);
            }
        }
    }

    let mut conn: PeerConnection = match conn_opt {
        Some(conn) => conn,
        None => return Err(last_error.expect("at least one peer candidate")),
    };

    let cs = conn
        .protocols
        .remove(&MiniProtocolNum::CHAIN_SYNC)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing ChainSync protocol handle".into(),
        })?;
    let bf = conn
        .protocols
        .remove(&MiniProtocolNum::BLOCK_FETCH)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing BlockFetch protocol handle".into(),
        })?;
    let ka = conn
        .protocols
        .remove(&MiniProtocolNum::KEEP_ALIVE)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing KeepAlive protocol handle".into(),
        })?;
    let tx = conn
        .protocols
        .remove(&MiniProtocolNum::TX_SUBMISSION)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing TxSubmission protocol handle".into(),
        })?;

    Ok(PeerSession {
        connected_peer_addr,
        chain_sync: ChainSyncClient::new(cs),
        block_fetch: BlockFetchClient::new(bf),
        keep_alive: KeepAliveClient::new(ka),
        tx_submission: TxSubmissionClient::new(tx),
        mux: conn.mux,
        version: conn.version,
        version_data: conn.version_data,
    })
}

/// Run the verified sync loop, reconnecting through ordered bootstrap peers
/// when protocol connectivity is lost.
///
/// The runner preserves the current chain point, nonce evolution state, and
/// optional chain state across reconnects. Only bootstrap, ChainSync, and
/// BlockFetch failures trigger reconnection; decode, verification, and storage
/// failures still return immediately.
pub async fn run_reconnecting_verified_sync_service<S, F>(
    store: &mut S,
    request: ReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    S: VolatileStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    run_reconnecting_verified_sync_service_with_tracer(store, request, &tracer, shutdown).await
}

/// Run the verified sync loop, reconnecting through ordered bootstrap peers
/// while coordinating storage through [`ChainDb`].
pub async fn run_reconnecting_verified_sync_service_chaindb<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    run_reconnecting_verified_sync_service_chaindb_with_tracer(chain_db, request, &tracer, shutdown)
        .await
}

/// Recover ledger state from coordinated storage and then run reconnecting
/// verified sync from the recovered point.
pub async fn resume_reconnecting_verified_sync_service_chaindb<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    resume_reconnecting_verified_sync_service_chaindb_with_tracer(chain_db, request, &tracer, shutdown)
        .await
}

/// Run the reconnecting verified sync loop while emitting runtime trace events.
///
/// Trace emission is driven by the node config-derived [`NodeTracer`] and stays
/// within the node integration layer: bootstrap attempts, successful session
/// establishment, connectivity-triggered reconnects, batch completion, and
/// graceful shutdown are traced, while decode, verification, and storage
/// failures still return immediately.
pub async fn run_reconnecting_verified_sync_service_with_tracer<S, F>(
    store: &mut S,
    request: ReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    S: VolatileStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        mut from_point,
        config,
        mut nonce_state,
        use_ledger_peers: _,
        peer_snapshot_path: _,
    } = request;

    tokio::pin!(shutdown);

    let mut total_blocks = 0usize;
    let mut total_rollbacks = 0usize;
    let mut batches_completed = 0usize;
    let mut total_stable = 0usize;
    let mut reconnect_count = 0usize;
    let mut last_connected_peer_addr = None;
    let mut chain_state = config.security_param.map(ChainState::new);
    let mut had_session = false;
    let mut attempt_state = peer_attempt_state(node_config.peer_addr, fallback_peer_addrs);

    loop {
        let mut session = tokio::select! {
            biased;

            () = &mut shutdown => {
                tracer.trace_runtime(
                    "Node.Shutdown",
                    "Notice",
                    "shutdown requested before bootstrap completed",
                    BTreeMap::new(),
                );
                return Ok(ReconnectingSyncServiceOutcome {
                    final_point: from_point,
                    total_blocks,
                    total_rollbacks,
                    batches_completed,
                    nonce_state,
                    chain_state,
                    stable_block_count: total_stable,
                    reconnect_count,
                    last_connected_peer_addr,
                });
            }

            result = bootstrap_with_attempt_state(node_config, &mut attempt_state, tracer) => result?,
        };

        if had_session {
            reconnect_count += 1;
        } else {
            had_session = true;
        }
        last_connected_peer_addr = Some(session.connected_peer_addr);

        tracer.trace_runtime(
            "Net.ConnectionManager.Remote",
            "Notice",
            if reconnect_count == 0 {
                "verified sync session established"
            } else {
                "verified sync session re-established"
            },
            trace_fields([
                ("peer", json!(session.connected_peer_addr.to_string())),
                ("reconnectCount", json!(reconnect_count)),
                ("fromPoint", json!(format!("{:?}", from_point))),
            ]),
        );

        loop {
            let batch_fut = sync_batch_apply_verified(
                &mut session.chain_sync,
                &mut session.block_fetch,
                store,
                from_point,
                config.batch_size,
                Some(&config.verification),
            );

            tokio::select! {
                biased;

                () = &mut shutdown => {
                    tracer.trace_runtime(
                        "Node.Shutdown",
                        "Notice",
                        "shutdown requested during sync session",
                        trace_fields([
                            ("peer", json!(session.connected_peer_addr.to_string())),
                            ("currentPoint", json!(format!("{:?}", from_point))),
                        ]),
                    );
                    session.mux.abort();
                    return Ok(ReconnectingSyncServiceOutcome {
                        final_point: from_point,
                        total_blocks,
                        total_rollbacks,
                        batches_completed,
                        nonce_state,
                        chain_state,
                        stable_block_count: total_stable,
                        reconnect_count,
                        last_connected_peer_addr,
                    });
                }

                result = batch_fut => {
                    match result {
                        Ok(progress) => {
                            from_point = progress.current_point;
                            total_blocks += progress.fetched_blocks;
                            total_rollbacks += progress.rollback_count;
                            batches_completed += 1;

                            if let Some(ref mut cs) = chain_state {
                                for step in &progress.steps {
                                    total_stable += track_chain_state(cs, step)?;
                                }
                            }

                            if let Some((ref mut state, nonce_cfg)) =
                                nonce_state.as_mut().zip(config.nonce_config.as_ref())
                            {
                                apply_nonce_evolution_to_progress(state, &progress, nonce_cfg);
                            }

                            tracer.trace_runtime(
                                "ChainSync.Client",
                                "Info",
                                "verified sync batch applied",
                                trace_fields([
                                    ("peer", json!(session.connected_peer_addr.to_string())),
                                    ("currentPoint", json!(format!("{:?}", from_point))),
                                    ("batchFetchedBlocks", json!(progress.fetched_blocks)),
                                    ("batchRollbacks", json!(progress.rollback_count)),
                                    ("totalBlocks", json!(total_blocks)),
                                    ("batchesCompleted", json!(batches_completed)),
                                ]),
                            );
                        }
                        Err(SyncError::ChainSync(err)) => {
                            tracer.trace_runtime(
                                "ChainSync.Client",
                                "Warning",
                                "chainsync connectivity lost; reconnecting",
                                trace_fields([
                                    ("peer", json!(session.connected_peer_addr.to_string())),
                                    ("error", json!(err.to_string())),
                                    ("currentPoint", json!(format!("{:?}", from_point))),
                                ]),
                            );
                            session.mux.abort();
                            break;
                        }
                        Err(SyncError::BlockFetch(err)) => {
                            tracer.trace_runtime(
                                "BlockFetch.Client.CompletedBlockFetch",
                                "Warning",
                                "blockfetch connectivity lost; reconnecting",
                                trace_fields([
                                    ("peer", json!(session.connected_peer_addr.to_string())),
                                    ("error", json!(err.to_string())),
                                    ("currentPoint", json!(format!("{:?}", from_point))),
                                ]),
                            );
                            session.mux.abort();
                            break;
                        }
                        Err(err) => {
                            tracer.trace_runtime(
                                "Node.Sync",
                                "Error",
                                "verified sync service failed",
                                trace_fields([
                                    ("peer", json!(session.connected_peer_addr.to_string())),
                                    ("error", json!(err.to_string())),
                                    ("currentPoint", json!(format!("{:?}", from_point))),
                                ]),
                            );
                            session.mux.abort();
                            return Err(err);
                        }
                    }
                }
            }
        }
    }
}

/// Recover ledger state from coordinated storage and then run reconnecting
/// verified sync while emitting runtime trace events.
pub async fn resume_reconnecting_verified_sync_service_chaindb_with_tracer<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ResumeReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        base_ledger_state,
        config,
        nonce_state,
        use_ledger_peers,
        peer_snapshot_path,
    } = request;

    let recovery = recover_ledger_state_chaindb(chain_db, base_ledger_state)?;
    tracer.trace_runtime(
        "Node.Recovery",
        "Notice",
        "recovered ledger state from coordinated storage",
        trace_fields([
            ("point", json!(format!("{:?}", recovery.point))),
            ("checkpointSlot", json!(recovery.checkpoint_slot.map(|slot| slot.0))),
            ("replayedVolatileBlocks", json!(recovery.replayed_volatile_blocks)),
        ]),
    );

    let checkpoint_tracking = LedgerCheckpointTracking {
        base_ledger_state: recovery.ledger_state.clone(),
        ledger_state: recovery.ledger_state.clone(),
        last_persisted_point: recovery.point,
    };

    let sync = run_reconnecting_verified_sync_service_chaindb_inner(
        chain_db,
        ReconnectingVerifiedSyncContext {
            node_config,
            fallback_peer_addrs,
            use_ledger_peers,
            peer_snapshot_path: peer_snapshot_path.as_deref(),
            config,
            tracer,
        },
        ReconnectingVerifiedSyncState {
            from_point: recovery.point,
            nonce_state,
            checkpoint_tracking: Some(checkpoint_tracking),
        },
        shutdown,
    )
    .await?;

    Ok(ResumedSyncServiceOutcome { recovery, sync })
}

/// Run the reconnecting verified sync loop over coordinated storage while
/// emitting runtime trace events.
pub async fn run_reconnecting_verified_sync_service_chaindb_with_tracer<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        from_point,
        config,
        nonce_state,
        use_ledger_peers,
        peer_snapshot_path,
    } = request;
    let checkpoint_tracking = Some(crate::sync::default_checkpoint_tracking(chain_db)?);

    run_reconnecting_verified_sync_service_chaindb_inner(
        chain_db,
        ReconnectingVerifiedSyncContext {
            node_config,
            fallback_peer_addrs,
            use_ledger_peers,
            peer_snapshot_path: peer_snapshot_path.as_deref(),
            config,
            tracer,
        },
        ReconnectingVerifiedSyncState {
            from_point,
            nonce_state,
            checkpoint_tracking,
        },
        shutdown,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{CheckpointPersistenceOutcome, checkpoint_trace_fields};
    use crate::sync::LedgerCheckpointPolicy;
    use serde_json::json;
    use yggdrasil_ledger::SlotNo;

    #[test]
    fn checkpoint_trace_fields_include_persisted_prune_counts() {
        let policy = LedgerCheckpointPolicy {
            min_slot_delta: 2160,
            max_snapshots: 8,
        };
        let fields = checkpoint_trace_fields(
            &CheckpointPersistenceOutcome::Persisted {
                slot: SlotNo(4320),
                retained_snapshots: 8,
                pruned_snapshots: 2,
                rollback_count: 1,
            },
            &policy,
        );

        assert_eq!(fields.get("action"), Some(&json!("persisted")));
        assert_eq!(fields.get("slot"), Some(&json!(4320)));
        assert_eq!(fields.get("retainedSnapshots"), Some(&json!(8)));
        assert_eq!(fields.get("prunedSnapshots"), Some(&json!(2)));
        assert_eq!(fields.get("rollbackCount"), Some(&json!(1)));
        assert_eq!(fields.get("checkpointIntervalSlots"), Some(&json!(2160)));
        assert_eq!(fields.get("maxLedgerSnapshots"), Some(&json!(8)));
    }

    #[test]
    fn checkpoint_trace_fields_include_skip_delta() {
        let policy = LedgerCheckpointPolicy {
            min_slot_delta: 2160,
            max_snapshots: 8,
        };
        let fields = checkpoint_trace_fields(
            &CheckpointPersistenceOutcome::Skipped {
                slot: SlotNo(1200),
                rollback_count: 0,
                since_last_slot_delta: 1200,
            },
            &policy,
        );

        assert_eq!(fields.get("action"), Some(&json!("skipped")));
        assert_eq!(fields.get("slot"), Some(&json!(1200)));
        assert_eq!(fields.get("sinceLastSlotDelta"), Some(&json!(1200)));
        assert_eq!(fields.get("rollbackCount"), Some(&json!(0)));
    }
}
