//! Node runtime — wires networking, storage, and protocol client drivers
//! into a cohesive sync lifecycle.
//!
//! Reference: `cardano-node/src/Cardano/Node/Run.hs`.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::config::load_peer_snapshot_file;
#[cfg(test)]
use crate::sync::VerifiedSyncServiceConfig;
use crate::sync::{
    LedgerCheckpointTracking, LedgerCheckpointUpdateOutcome, SyncError, TypedIntersectResult,
    typed_find_intersect,
};
use crate::tracer::{NodeTracer, trace_fields};
use serde_json::Value;
use serde_json::json;
use yggdrasil_consensus::{ChainState, EpochSchedule, SecurityParam};
use yggdrasil_ledger::{EpochBoundaryEvent, LedgerState, Point};
use yggdrasil_network::{
    AfterSlot, ChainSyncClient, LedgerPeerSnapshot, LedgerPeerUseDecision, LedgerStateJudgement,
    LiveLedgerPeerRefreshObservation, PeerAccessPoint, PeerRegistry, PeerSnapshotFreshness,
    TopologyConfig, UseLedgerPeers, always_eligible_snapshot_peers, derive_peer_snapshot_freshness,
    eligible_ledger_peer_candidates, live_refresh_ledger_peer_registry_observed,
    merge_ledger_peer_snapshots, resolve_peer_access_points,
};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

/// Notification used to wake ChainSync servers when the chain tip advances.
/// This is the Rust equivalent of the upstream ChainDB follower notification
/// mechanism, allowing servers to block efficiently instead of busy-polling.
pub type ChainTipNotify = Arc<tokio::sync::Notify>;

pub mod block_producer_config;
pub use block_producer_config::{
    RuntimeBlockProducerConfig, SharedBlockProducerState, update_bp_state_nonce,
    update_bp_state_sigma,
};

pub mod governor_config;
pub use governor_config::RuntimeGovernorConfig;

pub mod peer_management;
#[cfg(test)]
use peer_management::{
    ManagedWarmPeer, ordered_reconnect_fallback_peers, preferred_hot_peer_from_registry,
    reconnect_preferred_peer, reconnect_preferred_peer_with_source,
};
use peer_management::{
    OutboundPeerManager, RuntimeRootPeerSources, apply_control_close, peer_share_request_amount,
    preferred_hot_peer_handoff_target, prepare_reconnect_attempt_state, reconnect_storage_tip,
    registry_reserve_bootstrap_attempt_peers, reserve_bootstrap_sync_peers,
};
pub use peer_management::{
    SharedFetchWorkerPool, local_root_targets_from_config, new_shared_fetch_worker_pool,
    seed_peer_registry,
};

pub mod cm_actions;
#[cfg(test)]
use cm_actions::direct_sync_bootstrap_pending;
use cm_actions::{
    apply_cm_actions, governor_action_name, governor_action_peer, outbound_cm_local_addr,
    retire_failed_outbound_peer, split_timeout_cm_actions_for_governor,
    suppress_outbound_promotions_while_bootstrap_pending, update_registry_status_from_cm,
};

pub mod forge;
#[cfg(test)]
use forge::kes_expiry_warning_from_periods;
use forge::{
    kes_expiry_warning, mempool_entries_for_forging, self_validate_forged_block,
    tip_context_from_chain_db,
};

pub mod ledger_judgement;
pub use ledger_judgement::LedgerJudgementSettings;

pub mod ledger_peer_source;
use ledger_peer_source::{
    ChainDbConsensusLedgerSource, FilePeerSnapshotSource, block_producer_ledger_state_judgement,
};
#[cfg(test)]
use ledger_peer_source::{derive_judgement_at, wall_clock_unix_secs};

fn refresh_ledger_peer_sources_from_chain_db<I, V, L>(
    registry: &mut PeerRegistry,
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    base_ledger_state: &LedgerState,
    topology: &TopologyConfig,
    tracer: &NodeTracer,
    judgement_settings: LedgerJudgementSettings,
    epoch_schedule: Option<EpochSchedule>,
) -> LiveLedgerPeerRefreshObservation
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    if !topology.use_ledger_peers.enabled() {
        return LiveLedgerPeerRefreshObservation {
            update: yggdrasil_network::LedgerPeerRegistryUpdate {
                decision: LedgerPeerUseDecision::Disabled,
                changed: false,
            },
            latest_slot: None,
            judgement: LedgerStateJudgement::Unavailable,
            peer_snapshot_freshness: PeerSnapshotFreshness::NotConfigured,
        };
    }

    let mut consensus_source = ChainDbConsensusLedgerSource {
        chain_db,
        base_ledger_state,
        tracer,
        system_start_unix_secs: judgement_settings.system_start_unix_secs,
        slot_length_secs: judgement_settings.slot_length_secs,
        max_ledger_state_age_secs: judgement_settings.max_ledger_state_age_secs,
        epoch_schedule,
    };
    let mut snapshot_source = FilePeerSnapshotSource {
        path: topology.peer_snapshot_file.as_deref(),
        tracer,
    };

    let observation = live_refresh_ledger_peer_registry_observed(
        registry,
        topology.use_ledger_peers,
        &mut consensus_source,
        &mut snapshot_source,
    );

    if observation.update.changed {
        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "ledger peer registry refreshed",
            trace_fields([(
                "decision",
                json!(format!("{:?}", observation.update.decision)),
            )]),
        );
    }

    observation
}

pub mod block_producer_loop;
pub use block_producer_loop::run_block_producer_loop;

pub mod governor_loop;
pub use governor_loop::run_governor_loop;

pub mod mempool_helpers;
pub use mempool_helpers::{
    MempoolAddTxError, MempoolAddTxOutcome, MempoolAddTxResult, add_tx_to_mempool,
    add_tx_to_shared_mempool, add_tx_to_shared_mempool_with_eviction, add_txs_to_mempool,
    add_txs_to_shared_mempool, add_txs_to_shared_mempool_with_eviction,
};

pub mod tx_submission_service;
pub use tx_submission_service::{
    TxSubmissionServiceError, TxSubmissionServiceOutcome, run_txsubmission_service,
    run_txsubmission_service_shared, serve_txsubmission_request_from_mempool,
    serve_txsubmission_request_from_reader,
};
pub mod peer_session;
pub use peer_session::{
    NodeConfig, PeerSession, ReconnectingSyncServiceOutcome, ReconnectingVerifiedSyncRequest,
    ResumeReconnectingVerifiedSyncRequest, ResumedSyncServiceOutcome,
};

pub mod bootstrap;
pub use bootstrap::{bootstrap, bootstrap_with_attempt_state, bootstrap_with_fallbacks};

pub mod reconnecting_sync;
#[cfg(test)]
pub(crate) use reconnecting_sync::{
    recover_ledger_state_for_runtime, stake_snapshots_for_recovered_point,
};
pub use reconnecting_sync::{
    resume_reconnecting_verified_sync_service_chaindb,
    resume_reconnecting_verified_sync_service_chaindb_with_tracer,
    resume_reconnecting_verified_sync_service_shared_chaindb,
    resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer,
    run_reconnecting_verified_sync_service, run_reconnecting_verified_sync_service_chaindb,
    run_reconnecting_verified_sync_service_chaindb_with_tracer,
    run_reconnecting_verified_sync_service_with_tracer,
};

type CheckpointTracking = LedgerCheckpointTracking;

fn shared_chaindb_lock_error() -> SyncError {
    SyncError::Recovery("shared ChainDb lock poisoned".to_owned())
}

mod reconnecting;
#[cfg(test)]
use reconnecting::cache_confirmed_entries;
use reconnecting::{BatchErrorDisposition, BatchTraceExtras, ReconnectingRunState};
#[cfg(test)]
use reconnecting::{re_admit_rolled_back_tx_ids, record_verified_batch_progress};

mod tracing;
use tracing::{
    peer_point_trace_fields, session_established_trace_fields, sync_error_trace_fields,
    verified_sync_batch_trace_fields,
};

fn trace_shutdown_before_bootstrap(tracer: &NodeTracer) {
    tracer.trace_runtime(
        "Node.Shutdown",
        "Notice",
        "shutdown requested before bootstrap completed",
        BTreeMap::new(),
    );
}

fn trace_shutdown_during_session(tracer: &NodeTracer, peer_addr: SocketAddr, current_point: Point) {
    tracer.trace_runtime(
        "Node.Shutdown",
        "Notice",
        "shutdown requested during sync session",
        peer_point_trace_fields(peer_addr, current_point),
    );
}

fn trace_session_established(
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
/// When `from_point` is [`Point::Origin`] the call is a no-op because the
/// peer's default read pointer is already at Origin.  Otherwise this issues a
/// single-point intersection request; on `Found` the local point is preserved,
/// on `NotFound` the local `from_point` is reset to [`Point::Origin`] so the
/// next batch starts a fresh sync from genesis (matching upstream behaviour
/// when no chain points are recognised by the peer).
async fn synchronize_chain_sync_to_point(
    chain_sync: &mut ChainSyncClient,
    from_point: &mut Point,
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
) -> Result<(), SyncError> {
    if matches!(from_point, Point::Origin) {
        return Ok(());
    }
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

fn trace_reconnectable_sync_error(
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

mod keep_alive;
use keep_alive::trace_sync_failure;

fn handle_reconnect_batch_error(
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
    // `node/src/main.rs::evaluate_ledger_derived_startup_fallbacks` for the
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

fn trace_epoch_boundary_events(tracer: &NodeTracer, events: &[EpochBoundaryEvent]) {
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

/// Polymorphic seed of the volatile-window `ChainState` that works whether
/// the caller holds the chain DB as `&mut ChainDb<I, V, L>` (the
/// non-shared variant) or `&Arc<RwLock<ChainDb<I, V, L>>>` (the shared
/// variant).  Without this, the post-restart `ChainState` was always
/// `ChainState::new(k)` — empty — and the next ChainSync session's
/// `RollBackward(recovered_tip)` confirmation failed with
/// `RollbackPointNotFound` (surfaced by §6 restart-resilience cycle 2).
fn seed_chain_state_via_chain_db<S: ChainDbVolatileAccess>(
    chain_db: &S,
    security_param: Option<SecurityParam>,
) -> Option<ChainState> {
    security_param
        .map(|k| chain_db.with_volatile(|v| crate::sync::seed_chain_state_from_volatile(v, k)))
}

/// Trait abstracting "give me a borrow of the volatile store" across the
/// two ChainDb access modes used by the reconnecting sync entry points.
trait ChainDbVolatileAccess {
    fn with_volatile<R>(&self, f: impl FnOnce(&dyn VolatileStore) -> R) -> R;
    fn best_tip(&self) -> Point;
}

impl<I, V, L> ChainDbVolatileAccess for ChainDb<I, V, L>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    fn with_volatile<R>(&self, f: impl FnOnce(&dyn VolatileStore) -> R) -> R {
        f(self.volatile())
    }

    fn best_tip(&self) -> Point {
        self.tip()
    }
}

impl<I, V, L> ChainDbVolatileAccess for std::sync::Arc<std::sync::RwLock<ChainDb<I, V, L>>>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    fn with_volatile<R>(&self, f: impl FnOnce(&dyn VolatileStore) -> R) -> R {
        let guard = self.read().expect("chain db lock poisoned");
        f(guard.volatile())
    }

    fn best_tip(&self) -> Point {
        let guard = self.read().expect("chain db lock poisoned");
        guard.tip()
    }
}

#[cfg(test)]
mod tests;
