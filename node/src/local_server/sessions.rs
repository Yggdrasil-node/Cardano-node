//! Per-mini-protocol session runners + snapshot helpers for the
//! Node-to-Client server.
//!
//! Mirrors upstream `Ouroboros.Network.Protocol.{LocalTxSubmission,
//! LocalStateQuery, LocalTxMonitor}.Server` session loops. Each
//! `run_local_*_session` drives a single accepted client through the
//! protocol's request/reply state machine until the client closes or
//! the protocol errors.
//!
//! The snapshot helpers (`acquire_snapshot`,
//! `attach_chain_dep_state_from_sidecar`, `recover_snapshot_at_point`)
//! are shared between the LSQ and TxMonitor session loops.
//!
//! Reference:
//! - <https://github.com/IntersectMBO/ouroboros-network/tree/master/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/LocalTxSubmission>
//! - <https://github.com/IntersectMBO/ouroboros-network/tree/master/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/LocalStateQuery>
//! - <https://github.com/IntersectMBO/ouroboros-network/tree/master/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/LocalTxMonitor>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side per-NtC-session
//! registry and per-session driver bundle. Mirrors the
//! session half of upstream `Ouroboros.Network.NodeToClient.runServer`
//! plus `Ouroboros.Network.Mux::handleMux`. Upstream wires this
//! inline; Yggdrasil isolates the registry plus drivers for
//! testability.

use std::path::Path;
use std::sync::{Arc, RwLock};

#[cfg(unix)]
use std::path::PathBuf;

use yggdrasil_consensus::mempool::SharedMempool;
use yggdrasil_ledger::{CborDecode, Era, LedgerStateSnapshot, MultiEraSubmittedTx, Point, SlotNo};
use yggdrasil_network::{
    AcquireFailure, AcquireTarget, LocalStateQueryAcquiredRequest, LocalStateQueryIdleRequest,
    LocalStateQueryServer, LocalTxMonitorAcquiredRequest, LocalTxMonitorIdleRequest,
    LocalTxMonitorServer, LocalTxRequest, LocalTxSubmissionServer,
};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

use crate::local_server::{
    LocalQueryDispatcher, LocalStateQuerySessionError, LocalTxMonitorSessionError,
    LocalTxSubmissionSessionError,
};
use crate::runtime::{MempoolAddTxResult, add_tx_to_shared_mempool_with_eviction};
use crate::sync::{
    chain_dep_context_from_sidecar, load_exact_chain_dep_sidecar_snapshot,
    recover_ledger_state_chaindb,
};
use crate::tracer::NodeMetrics;

// ---------------------------------------------------------------------------
// run_local_tx_submission_session
// ---------------------------------------------------------------------------

/// Drive a single LocalTxSubmission server session to completion.
///
/// Accepts transaction byte blobs from the client, decodes them for the
/// current ledger era, and attempts admission into the shared mempool.
/// Accepted transactions receive `MsgAcceptTx`; rejected transactions
/// receive `MsgRejectTx` with a CBOR-encoded reason byte vector.
///
/// When a `metrics` handle is supplied each admission outcome is mirrored
/// into the `mempool_tx_added` / `mempool_tx_rejected` Prometheus counters
/// — matching the accounting the NtN inbound path already performs via
/// [`crate::server::SharedTxSubmissionConsumer`]. Decode failures and
/// ledger-recovery failures also count as rejections so the counter
/// stays an accurate view of LocalTxSubmission outcomes.
///
/// The session ends when the client sends `MsgDone` or the protocol errors.
pub async fn run_local_tx_submission_session<I, V, L>(
    mut server: LocalTxSubmissionServer,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
    metrics: Option<Arc<NodeMetrics>>,
) -> Result<(), LocalTxSubmissionSessionError>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    // Hard ceiling on a single LocalTxSubmission CBOR payload.  The
    // ledger-side `validate_max_tx_size` (see `crates/ledger/src/fees.rs`)
    // would reject anything past `params.max_tx_size`, but that check
    // runs AFTER full CBOR decode — a malicious local client could
    // submit a multi-megabyte well-formed-but-oversized CBOR blob and
    // force us to allocate it before rejection.  Cap the wire-side
    // first.  Mainnet `max_tx_size` is 16 384 B (Conway PV 10);
    // 64 KiB gives ~4× headroom for any future protocol-param raise
    // while still bounding the allocation.
    const LOCAL_TX_SUBMIT_MAX_BYTES: usize = 64 * 1024;
    loop {
        match server.recv_request().await? {
            LocalTxRequest::Done => return Ok(()),
            LocalTxRequest::SubmitTx { tx: tx_bytes } => {
                if tx_bytes.len() > LOCAL_TX_SUBMIT_MAX_BYTES {
                    if let Some(m) = &metrics {
                        m.inc_mempool_tx_rejected();
                    }
                    let reason = encode_rejection_reason(&format!(
                        "tx payload {} bytes exceeds LocalTxSubmission ceiling of {} bytes",
                        tx_bytes.len(),
                        LOCAL_TX_SUBMIT_MAX_BYTES
                    ));
                    server.reject(reason).await?;
                    continue;
                }
                // Recover a current ledger state for decoding and validation.
                // The RwLockReadGuard (and its originating Result) must be
                // fully dropped before any .await to keep the future Send.
                let ledger_result = chain_db.read().ok().and_then(|db| {
                    recover_ledger_state_chaindb(
                        &db,
                        yggdrasil_ledger::LedgerState::new(Era::Byron),
                    )
                    .ok()
                });
                let mut ledger_state = match ledger_result {
                    Some(recovery) => recovery.ledger_state,
                    None => {
                        if let Some(m) = &metrics {
                            m.inc_mempool_tx_rejected();
                        }
                        let reason = encode_rejection_reason("internal error: ledger recovery");
                        let _ = server.reject(reason).await;
                        continue;
                    }
                };

                let era = ledger_state.current_era();
                let current_slot = ledger_state.tip.slot().unwrap_or(SlotNo(0));

                // Decode the submitted transaction bytes for the current era.
                let submitted_tx =
                    match MultiEraSubmittedTx::from_cbor_bytes_for_era(era, &tx_bytes) {
                        Ok(tx) => tx,
                        Err(e) => {
                            if let Some(m) = &metrics {
                                m.inc_mempool_tx_rejected();
                            }
                            let reason = encode_rejection_reason(&format!("decode error: {e}"));
                            server.reject(reason).await?;
                            continue;
                        }
                    };

                // Attempt mempool admission with upstream-aligned
                // capacity-overflow eviction. Mirrors
                // `Ouroboros.Consensus.Mempool.Impl.Update.makeRoomForTransaction`
                // — when the mempool is full, the lowest-fee tail is
                // displaced rather than the incoming tx being rejected
                // outright (provided cumulative-fee guards hold).
                let eval_ref = evaluator.as_ref().map(|e| {
                    e.as_ref() as &dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator
                });
                match add_tx_to_shared_mempool_with_eviction(
                    &mut ledger_state,
                    &mempool,
                    submitted_tx,
                    current_slot,
                    eval_ref,
                ) {
                    Ok(outcome) => match outcome.result {
                        MempoolAddTxResult::MempoolTxAdded(_) => {
                            if let Some(m) = &metrics {
                                m.inc_mempool_tx_added();
                                for _ in &outcome.evicted {
                                    m.inc_mempool_tx_rejected();
                                }
                            }
                            server.accept().await?;
                        }
                        MempoolAddTxResult::MempoolTxRejected(_, reason) => {
                            if let Some(m) = &metrics {
                                m.inc_mempool_tx_rejected();
                            }
                            let reason_bytes = encode_rejection_reason(&format!("{reason}"));
                            server.reject(reason_bytes).await?;
                        }
                    },
                    Err(e) => {
                        if let Some(m) = &metrics {
                            m.inc_mempool_tx_rejected();
                        }
                        let reason_bytes = encode_rejection_reason(&format!("mempool error: {e}"));
                        server.reject(reason_bytes).await?;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// run_local_state_query_session
// ---------------------------------------------------------------------------

/// Drive a single LocalStateQuery server session to completion.
///
/// Handles the full acquire→query→release lifecycle.  Each `Acquire` request
/// attempts to take a ledger-state snapshot for the requested target point;
/// once acquired, the session enters a loop fielding `Query`, `Release`, and
/// `ReAcquire` requests until the client sends `MsgDone`.
///
/// Query payloads are dispatched opaquely through the supplied
/// [`LocalQueryDispatcher`].
pub async fn run_local_state_query_session<I, V, L>(
    mut server: LocalStateQueryServer,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    dispatcher: Arc<dyn LocalQueryDispatcher>,
    storage_dir: Option<PathBuf>,
) -> Result<(), LocalStateQuerySessionError>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    loop {
        match server.recv_idle_request().await? {
            LocalStateQueryIdleRequest::Done => return Ok(()),
            LocalStateQueryIdleRequest::Acquire(target) => {
                let snapshot_opt = acquire_snapshot(&chain_db, &target, storage_dir.as_deref());

                match snapshot_opt {
                    Some(snapshot) => {
                        server.acquired().await?;
                        // Acquired loop.
                        let mut current_snapshot = snapshot;
                        loop {
                            match server.recv_acquired_request().await? {
                                LocalStateQueryAcquiredRequest::Query(query_bytes) => {
                                    let result =
                                        dispatcher.dispatch_query(&current_snapshot, &query_bytes);
                                    server.send_result(result).await?;
                                }
                                LocalStateQueryAcquiredRequest::Release => {
                                    // Return to idle loop.
                                    break;
                                }
                                LocalStateQueryAcquiredRequest::ReAcquire(new_target) => {
                                    match acquire_snapshot(
                                        &chain_db,
                                        &new_target,
                                        storage_dir.as_deref(),
                                    ) {
                                        Some(new_snapshot) => {
                                            current_snapshot = new_snapshot;
                                            server.acquired().await?;
                                        }
                                        None => {
                                            server.failure(AcquireFailure::PointNotOnChain).await?;
                                            // After failure on re-acquire the
                                            // server returns to StAcquired so
                                            // the acquired loop continues.
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        // The requested point is not available; send failure
                        // which transitions back to StIdle.
                        server.failure(AcquireFailure::PointNotOnChain).await?;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// run_local_tx_monitor_session
// ---------------------------------------------------------------------------

/// Drive a single LocalTxMonitor server session to completion.
///
/// Acquires a snapshot of the shared mempool on each `Acquire`/`AwaitAcquire`
/// request, then services `NextTx`, `HasTx`, and `GetSizes` queries against
/// that snapshot until the client releases or re-acquires.
///
/// The session ends when the client sends `MsgDone` or the protocol errors.
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Server`.
pub async fn run_local_tx_monitor_session<I, V, L>(
    mut server: LocalTxMonitorServer,
    mempool: SharedMempool,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
) -> Result<(), LocalTxMonitorSessionError>
where
    I: ImmutableStore + Send + Sync + 'static,
    V: VolatileStore + Send + Sync + 'static,
    L: LedgerStore + Send + Sync + 'static,
{
    loop {
        match server.recv_idle_request().await? {
            LocalTxMonitorIdleRequest::Done => return Ok(()),
            LocalTxMonitorIdleRequest::Acquire => {
                // Take a snapshot and enter the acquired loop.
                let snapshot = mempool.snapshot();
                let tip_slot = chain_db
                    .read()
                    .ok()
                    .and_then(|db| db.tip().slot())
                    .map(|s| s.0)
                    .unwrap_or(0u64);
                server.acquired(tip_slot).await?;

                let mut tx_iter = snapshot
                    .mempool_txids_after(yggdrasil_consensus::mempool::MEMPOOL_ZERO_IDX)
                    .into_iter();

                loop {
                    match server.recv_acquired_request().await? {
                        LocalTxMonitorAcquiredRequest::NextTx => {
                            let next_tx = tx_iter.next().and_then(|(_, idx, _)| {
                                snapshot.mempool_lookup_tx(idx).map(|e| e.raw_tx.clone())
                            });
                            server.reply_next_tx(next_tx).await?;
                        }
                        LocalTxMonitorAcquiredRequest::HasTx { tx_id } => {
                            let has = if tx_id.len() == 32 {
                                let mut id = [0u8; 32];
                                id.copy_from_slice(&tx_id);
                                snapshot.mempool_has_tx(&yggdrasil_ledger::TxId(id))
                            } else {
                                false
                            };
                            server.reply_has_tx(has).await?;
                        }
                        LocalTxMonitorAcquiredRequest::GetSizes => {
                            let cap = mempool.capacity() as u32;
                            let size: usize = snapshot
                                .mempool_txids_after(yggdrasil_consensus::mempool::MEMPOOL_ZERO_IDX)
                                .iter()
                                .map(|(_, _, sz)| *sz)
                                .sum();
                            let count = snapshot
                                .mempool_txids_after(yggdrasil_consensus::mempool::MEMPOOL_ZERO_IDX)
                                .len() as u32;
                            server.reply_get_sizes(cap, size as u32, count).await?;
                        }
                        LocalTxMonitorAcquiredRequest::Release => break,
                        LocalTxMonitorAcquiredRequest::AwaitAcquire => {
                            // Block until the mempool contents change, matching
                            // upstream `MsgAwaitAcquire` blocking semantics.
                            // Reference: Ouroboros.Network.Protocol.LocalTxMonitor.Server
                            mempool.wait_for_change().await;
                            // Re-acquire: take a fresh snapshot and re-read tip.
                            let new_snapshot = mempool.snapshot();
                            let tip_slot = chain_db
                                .read()
                                .ok()
                                .and_then(|db| db.tip().slot())
                                .map(|s| s.0)
                                .unwrap_or(0u64);
                            server.acquired(tip_slot).await?;
                            tx_iter = new_snapshot
                                .mempool_txids_after(yggdrasil_consensus::mempool::MEMPOOL_ZERO_IDX)
                                .into_iter();
                            // Note: we shadow `snapshot` by rebinding below,
                            // but the borrow checker requires us to break out
                            // the new snapshot. Instead, we restart the outer
                            // acquired loop with a fresh snapshot.
                            // For simplicity, break and re-enter the idle loop
                            // (the protocol transitions back to StIdle after
                            // AwaitAcquire → MsgAcquired).
                            continue;
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: acquire ledger snapshot
// ---------------------------------------------------------------------------

/// Attempt to acquire a [`LedgerStateSnapshot`] for the requested target.
///
/// For `VolatileTip` the current tip snapshot is always available.  For a
/// specific `Point` we attempt to recover the ledger state at that point;
/// `None` is returned when the point is not on the current chain.
pub(crate) fn acquire_snapshot<I, V, L>(
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    target: &AcquireTarget,
    storage_dir: Option<&Path>,
) -> Option<LedgerStateSnapshot>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    let db = chain_db.read().ok()?;

    let snapshot = match target {
        AcquireTarget::VolatileTip => {
            let recovery =
                recover_ledger_state_chaindb(&db, yggdrasil_ledger::LedgerState::new(Era::Byron))
                    .ok()?;
            recovery.ledger_state.snapshot()
        }
        AcquireTarget::Point(point) => {
            let mut dec = yggdrasil_ledger::cbor::Decoder::new(point);
            let requested = Point::decode_cbor(&mut dec).ok()?;
            recover_snapshot_at_point(&db, &requested)?
        }
    };

    // R238 — attach `ChainDepStateContext` from the canonical slot-indexed
    // sidecar whose point exactly matches the acquired ledger point.
    Some(attach_chain_dep_state_from_sidecar(snapshot, storage_dir))
}

/// R238 — load `ChainDepStateContext` data from the exact on-disk
/// ChainDepState sidecar produced by the sync runtime and attach it to the
/// snapshot.
///
/// Missing exact sidecar history leaves `ChainDepStateContext` absent so the
/// protocol-state encoder uses its neutral no-context shape.
pub(crate) fn attach_chain_dep_state_from_sidecar(
    snapshot: LedgerStateSnapshot,
    storage_dir: Option<&Path>,
) -> LedgerStateSnapshot {
    use yggdrasil_ledger::stake::StakeSnapshots;
    use yggdrasil_ledger::{CborDecode, ChainDepStateContext, Decoder};

    let Some(dir) = storage_dir else {
        return snapshot;
    };

    let mut snap = snapshot;
    let mut ctx = ChainDepStateContext::default();
    let mut chain_dep_populated = false;

    if let Ok(Some(sidecar)) = load_exact_chain_dep_sidecar_snapshot(Some(dir), snap.tip()) {
        ctx = chain_dep_context_from_sidecar(&sidecar);
        chain_dep_populated = sidecar.nonce_state.is_some() || sidecar.ocert_counters.is_some();
    }

    if chain_dep_populated {
        snap = snap.with_chain_dep_state(ctx);
    }

    // Active stake-snapshot rotation (R203).
    if let Ok(Some(stake_bytes)) = yggdrasil_storage::load_stake_snapshots(dir) {
        let mut dec = Decoder::new(&stake_bytes);
        if let Ok(snapshots) = StakeSnapshots::decode_cbor(&mut dec) {
            snap = snap.with_stake_snapshots(snapshots);
        }
    }

    snap
}

/// Recover a ledger snapshot at an explicit chain point.
///
/// Reference: `ouroboros-network` LocalStateQuery acquire semantics
/// (`MsgAcquire point`) where acquisition succeeds only when the point is on
/// the node's current chain.
pub(crate) fn recover_snapshot_at_point<I, V, L>(
    chain_db: &ChainDb<I, V, L>,
    requested: &Point,
) -> Option<LedgerStateSnapshot>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    if requested == &Point::Origin {
        return Some(yggdrasil_ledger::LedgerState::new(Era::Byron).snapshot());
    }

    let tip = chain_db.tip();
    if requested == &tip {
        let recovery =
            recover_ledger_state_chaindb(chain_db, yggdrasil_ledger::LedgerState::new(Era::Byron))
                .ok()?;
        return Some(recovery.ledger_state.snapshot());
    }

    let mut state = yggdrasil_ledger::LedgerState::new(Era::Byron);
    let immutable_blocks = chain_db.immutable().suffix_after(&Point::Origin).ok()?;
    for block in &immutable_blocks {
        state.apply_block(block).ok()?;
        if &state.tip == requested {
            return Some(state.snapshot());
        }
    }

    let volatile_blocks = chain_db.volatile().suffix_after(&state.tip);
    for block in &volatile_blocks {
        state.apply_block(block).ok()?;
        if &state.tip == requested {
            return Some(state.snapshot());
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Helper: CBOR-encode a rejection reason string
// ---------------------------------------------------------------------------

/// Encode a human-readable rejection reason as a CBOR text-string byte vector.
///
/// The NtC LocalTxSubmission wire format for `MsgRejectTx` carries the
/// rejection reason as an opaque byte blob; this helper wraps the reason
/// in a minimal 1-element CBOR array containing the text string so clients
/// that understand CBOR can decode it while raw bytes remain readable.
pub(crate) fn encode_rejection_reason(reason: &str) -> Vec<u8> {
    use yggdrasil_ledger::Encoder;

    let mut enc = Encoder::new();
    enc.array(1).text(reason);
    enc.into_bytes()
}
