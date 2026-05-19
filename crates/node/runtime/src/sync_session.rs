//! Sync-session helpers: per-session traces, reconnect orchestration,
//! ChainDb fallback-peer refresh, and checkpoint / epoch-boundary
//! trace events.
//!
//! Mirrors the runtime-side glue around the upstream sync session
//! lifecycle — `Ouroboros.Consensus.Node.Run.runWith` shutdown traces,
//! `Ouroboros.Network.Protocol.ChainSync.Client::chainSyncClientPeer`
//! intersection synchronisation, and the upstream
//! `Cardano.Node.Tracers::TraceLedgerEvent` checkpoint / epoch-boundary
//! observability.
//!
//! Twelve items move from `runtime.rs` here:
//!
//! - `shared_chaindb_lock_error` — uniform `SyncError::Recovery` for
//!   poisoned `Arc<RwLock<ChainDb>>` locks.
//! - `trace_shutdown_before_bootstrap`, `trace_shutdown_during_session`,
//!   `trace_session_established` — lifecycle trace helpers.
//! - `synchronize_chain_sync_to_point` — typed-ChainSync `MsgFindIntersect`
//!   synchronisation around the locally-tracked chain point.
//! - `trace_reconnectable_sync_error` — uniform reconnect trace.
//! - `handle_reconnect_batch_error` — three-way disposition
//!   (Reconnect / Fail / Continue) for sync errors.
//! - `extend_unique_socket_addrs` — uniqueness-preserving append helper.
//! - `refresh_chain_db_reconnect_fallback_peers` — re-derives the
//!   reconnect fallback peer list from current ChainDb / topology /
//!   peer-snapshot state and seeds the post-restart `ChainState`.
//! - `checkpoint_trace_fields`, `trace_checkpoint_outcome`,
//!   `trace_epoch_boundary_events` — checkpoint persistence + epoch-
//!   boundary trace event surface.
//!
//! Extracted from `runtime.rs` in R271r (Phase γ §R271 eighteenth slice).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side per-session helpers
//! (traces, reconnect orchestration, fallback-peer refresh,
//! epoch-boundary trace events). Mirrors glue around upstream
//! `Ouroboros.Consensus.Node.Run.runWith` shutdown traces,
//! `Ouroboros.Network.Protocol.ChainSync.Client.chainSyncClientPeer`
//! intersection sync, and ChainDb tip refresh; Haskell wires this
//! inline, Yggdrasil isolates the runtime-side glue.

use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;

use yggdrasil_ledger::{EpochBoundaryEvent, Point};
use yggdrasil_network::{
    AfterSlot, ChainSyncClient, LedgerPeerSnapshot, LedgerPeerUseDecision, LedgerStateJudgement,
    PeerAccessPoint, PeerSnapshotFreshness, UseLedgerPeers, always_eligible_snapshot_peers,
    derive_peer_snapshot_freshness, eligible_ledger_peer_candidates, merge_ledger_peer_snapshots,
    resolve_peer_access_points,
};

use yggdrasil_node_config::load_peer_snapshot_file;
use yggdrasil_node_sync::{
    LedgerCheckpointTracking, LedgerCheckpointUpdateOutcome, SyncError, TypedIntersectResult,
    typed_find_intersect,
};
use yggdrasil_node_tracer::{NodeTracer, trace_fields};

use super::keep_alive::trace_sync_failure;
use super::reconnecting::BatchErrorDisposition;
use super::tracing::{
    peer_point_trace_fields, session_established_trace_fields, sync_error_trace_fields,
};

type CheckpointTracking = LedgerCheckpointTracking;

pub(super) fn shared_chaindb_lock_error() -> SyncError {
    SyncError::Recovery("shared ChainDb lock poisoned".to_owned())
}

pub(super) fn trace_shutdown_before_bootstrap(tracer: &NodeTracer) {
    tracer.trace_runtime(
        "Node.Shutdown",
        "Notice",
        "shutdown requested before bootstrap completed",
        BTreeMap::new(),
    );
}

pub(super) fn trace_shutdown_during_session(
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
    current_point: Point,
) {
    tracer.trace_runtime(
        "Node.Shutdown",
        "Notice",
        "shutdown requested during sync session",
        peer_point_trace_fields(peer_addr, current_point),
    );
}

pub(super) fn trace_session_established(
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
    reconnect_count: usize,
    from_point: Point,
) {
    tracer.trace_runtime(
        "Net.ConnectionManager.Remote",
        "Notice",
        if reconnect_count == 0 {
            "verified sync session established"
        } else {
            "verified sync session re-established"
        },
        session_established_trace_fields(peer_addr, reconnect_count, from_point),
    );
}

/// Synchronize a freshly-connected ChainSync client to the locally-tracked
/// chain point by issuing `MsgFindIntersect`.
///
/// Upstream typed ChainSync requires the client to send `MsgFindIntersect`
/// before `MsgRequestNext`; otherwise the peer's read pointer stays at its
/// default position (Origin) and the client is rolled back to genesis on the
/// first `RollBackward` reply.  Reference:
/// `Ouroboros.Network.Protocol.ChainSync.Client.chainSyncClientPeer` and
/// `Ouroboros.Consensus.Network.NodeToNode` (typed ChainSync codec).
///
/// This also sends an explicit `MsgFindIntersect [Origin]` when `from_point`
/// is [`Point::Origin`]. Mainnet peers can close fresh sessions that skip
/// intersection and jump straight to `MsgRequestNext`, while upstream clients
/// position the server cursor with ChainSync intersection before streaming.
/// On `Found` the local point is preserved; on `NotFound` the local
/// `from_point` is reset to [`Point::Origin`] so the next batch starts a fresh
/// sync from genesis (matching upstream behaviour when no chain points are
/// recognised by the peer).
pub(super) async fn synchronize_chain_sync_to_point(
    chain_sync: &mut ChainSyncClient,
    from_point: &mut Point,
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
) -> Result<(), SyncError> {
    let candidates = vec![*from_point];
    let result = typed_find_intersect(chain_sync, &candidates).await?;
    match result {
        TypedIntersectResult::Found { point, tip } => {
            tracer.trace_runtime(
                "ChainSync.Client.FindIntersect",
                "Info",
                "intersection found with peer",
                trace_fields([
                    ("peer", json!(peer_addr.to_string())),
                    ("intersectionPoint", json!(format!("{point:?}"))),
                    ("peerTip", json!(format!("{tip:?}"))),
                ]),
            );
        }
        TypedIntersectResult::NotFound { tip } => {
            tracer.trace_runtime(
                "ChainSync.Client.FindIntersect",
                "Warning",
                "no intersection found with peer; restarting from Origin",
                trace_fields([
                    ("peer", json!(peer_addr.to_string())),
                    ("requestedPoint", json!(format!("{from_point:?}"))),
                    ("peerTip", json!(format!("{tip:?}"))),
                ]),
            );
            *from_point = Point::Origin;
        }
    }
    Ok(())
}

pub(super) fn trace_reconnectable_sync_error(
    tracer: &NodeTracer,
    namespace: &'static str,
    message: &'static str,
    peer_addr: SocketAddr,
    error: &impl ToString,
    current_point: Point,
) {
    tracer.trace_runtime(
        namespace,
        "Warning",
        message,
        sync_error_trace_fields(peer_addr, error, current_point),
    );
}

pub(super) fn handle_reconnect_batch_error(
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
    current_point: Point,
    error: &SyncError,
) -> BatchErrorDisposition {
    // Peer-attributable validation failures: the block itself (or its
    // header) failed verification.  Upstream this enacts
    // `InvalidBlockPunishment` which throws
    // `PeerSentAnInvalidBlockException` to the BlockFetch client thread.
    //
    // We reconnect to a different peer and emit a punishment trace event
    // so the governor can demote the offending peer.
    //
    // Reference: `Ouroboros.Consensus.MiniProtocol.BlockFetch.ClientInterface`
    // `mkAddFetchedBlock_` (~line 188–240).
    if error.is_peer_attributable() {
        tracer.trace_runtime(
            "ChainDB.AddBlockEvent.InvalidBlock",
            "Error",
            "peer sent an invalid block; disconnecting",
            sync_error_trace_fields(peer_addr, error, current_point),
        );
        return BatchErrorDisposition::ReconnectAndPunish;
    }

    match error {
        SyncError::ChainSync(err) => {
            trace_reconnectable_sync_error(
                tracer,
                "ChainSync.Client",
                "chainsync connectivity lost; reconnecting",
                peer_addr,
                err,
                current_point,
            );
            BatchErrorDisposition::Reconnect
        }
        SyncError::BlockFetch(err) => {
            trace_reconnectable_sync_error(
                tracer,
                "BlockFetch.Client.CompletedBlockFetch",
                "blockfetch connectivity lost; reconnecting",
                peer_addr,
                err,
                current_point,
            );
            BatchErrorDisposition::Reconnect
        }
        _ => {
            trace_sync_failure(tracer, peer_addr, error, current_point);
            BatchErrorDisposition::Fail
        }
    }
}

pub(super) fn extend_unique_socket_addrs(
    target: &mut Vec<SocketAddr>,
    peers: impl IntoIterator<Item = SocketAddr>,
) {
    for peer in peers {
        if !target.contains(&peer) {
            target.push(peer);
        }
    }
}

pub(super) fn refresh_chain_db_reconnect_fallback_peers(
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
    let latest_slot = checkpoint_tracking
        .ledger_state
        .tip
        .slot()
        .map(|slot| slot.0);
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
        for access_point in checkpoint_tracking
            .ledger_state
            .pool_state()
            .relay_access_points()
        {
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
    let mut snapshot_overlay = None;

    if let Some(peer_snapshot_path) = peer_snapshot_path {
        match load_peer_snapshot_file(peer_snapshot_path) {
            Ok(loaded_snapshot) => {
                snapshot_slot = loaded_snapshot.slot;
                snapshot_available = true;
                snapshot_overlay = Some(loaded_snapshot.snapshot);
            }
            Err(err) => {
                snapshot_available = false;
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to refresh reconnect peer snapshot",
                    trace_fields([
                        (
                            "snapshotPath",
                            json!(peer_snapshot_path.display().to_string()),
                        ),
                        ("error", json!(err.to_string())),
                    ]),
                );
            }
        }
    }

    // R250 — split snapshot-overlay path from live-ledger path so snapshot
    // peers (loaded from `peerSnapshotFile`) are eligible immediately at
    // reconnect time, while live-ledger-derived peers continue to wait for
    // the `useLedgerAfterSlot` gate. Upstream parity: see
    // `crates/node/yggdrasil-node/src/main.rs::evaluate_ledger_derived_startup_fallbacks` for the
    // companion change at startup, and
    // `crates/network/src/ledger_peers_provider.rs::always_eligible_snapshot_peers`
    // for the underlying primitive.
    let live_snapshot = LedgerPeerSnapshot::new(ledger_peers, Vec::new());
    let snapshot_overlay_for_always = snapshot_overlay.clone();
    let snapshot = merge_ledger_peer_snapshots(&live_snapshot, snapshot_overlay);
    let freshness: PeerSnapshotFreshness = derive_peer_snapshot_freshness(
        use_ledger_peers,
        peer_snapshot_path.is_some(),
        snapshot_slot,
        latest_slot,
        snapshot_available,
    );
    let mut blocked_peers = refreshed.clone();
    blocked_peers.push(primary_peer);

    // Live-ledger eligibility (gated by useLedgerAfterSlot).
    let (decision, live_eligible_peers) = eligible_ledger_peer_candidates(
        &live_snapshot,
        &blocked_peers,
        use_ledger_peers,
        latest_slot,
        LedgerStateJudgement::YoungEnough,
        freshness,
    );

    // Snapshot-overlay eligibility (always, no gate).
    let snapshot_eligible_peers =
        always_eligible_snapshot_peers(snapshot_overlay_for_always.as_ref(), &blocked_peers);

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
            (
                "snapshotEligibleCount",
                json!(snapshot_eligible_peers.len()),
            ),
            ("liveLedgerEligibleCount", json!(live_eligible_peers.len())),
        ]),
    );

    // Always extend with snapshot peers; live peers only when gate is open.
    extend_unique_socket_addrs(&mut refreshed, snapshot_eligible_peers);
    if decision == LedgerPeerUseDecision::Eligible {
        extend_unique_socket_addrs(&mut refreshed, live_eligible_peers);
    }
    refreshed
}

pub(super) type CheckpointPersistenceOutcome = LedgerCheckpointUpdateOutcome;

pub(super) fn checkpoint_trace_fields(
    outcome: &CheckpointPersistenceOutcome,
    policy: &yggdrasil_node_sync::LedgerCheckpointPolicy,
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

pub(super) fn trace_checkpoint_outcome(
    tracer: &NodeTracer,
    outcome: &CheckpointPersistenceOutcome,
    policy: &yggdrasil_node_sync::LedgerCheckpointPolicy,
) {
    let (severity, message) = match outcome {
        CheckpointPersistenceOutcome::Persisted { .. } => ("Info", "ledger checkpoint persisted"),
        CheckpointPersistenceOutcome::Skipped { .. } => ("Info", "ledger checkpoint skipped"),
        CheckpointPersistenceOutcome::ClearedDisabled => (
            "Notice",
            "ledger checkpoints cleared because persistence is disabled",
        ),
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

pub(super) fn trace_epoch_boundary_events(tracer: &NodeTracer, events: &[EpochBoundaryEvent]) {
    for ev in events {
        tracer.trace_runtime(
            "Ledger.EpochBoundary",
            "Notice",
            "epoch boundary transition applied",
            trace_fields([
                ("newEpoch", json!(ev.new_epoch.0)),
                ("pparamUpdatesApplied", json!(ev.pparam_updates_applied)),
                ("poolsRetired", json!(ev.pools_retired)),
                ("poolDepositRefunds", json!(ev.pool_deposit_refunds)),
                ("unclaimedPoolDeposits", json!(ev.unclaimed_pool_deposits)),
                ("rewardsDistributed", json!(ev.rewards_distributed)),
                ("treasuryDelta", json!(ev.treasury_delta)),
                ("unclaimedRewards", json!(ev.unclaimed_rewards)),
                ("deltaReserves", json!(ev.delta_reserves)),
                ("accountsRewarded", json!(ev.accounts_rewarded)),
                (
                    "governanceActionsExpired",
                    json!(ev.governance_actions_expired),
                ),
                (
                    "governanceDepositRefunds",
                    json!(ev.governance_deposit_refunds),
                ),
                ("drepsExpired", json!(ev.dreps_expired)),
                (
                    "governanceActionsEnacted",
                    json!(ev.governance_actions_enacted),
                ),
                ("enactedDepositRefunds", json!(ev.enacted_deposit_refunds)),
                (
                    "unclaimedGovernanceDeposits",
                    json!(ev.unclaimed_governance_deposits),
                ),
                ("donationsTransferred", json!(ev.donations_transferred)),
            ]),
        );
    }
}
