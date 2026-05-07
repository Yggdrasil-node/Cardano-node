//! Node runtime — wires networking, storage, and protocol client drivers
//! into a cohesive sync lifecycle.
//!
//! Reference: `cardano-node/src/Cardano/Node/Run.hs`.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::block_producer::{BlockProducerCredentials, ForgedBlock, serialize_forged_block_cbor};
use crate::config::load_peer_snapshot_file;
#[cfg(test)]
use crate::sync::VerifiedSyncServiceConfig;
use crate::sync::{
    LedgerCheckpointTracking, LedgerCheckpointUpdateOutcome, SyncError, TypedIntersectResult,
    decode_multi_era_block, multi_era_block_to_block, recover_ledger_state_chaindb,
    recover_ledger_state_chaindb_epoch_boundary, typed_find_intersect, validate_block_body_size,
    validate_block_protocol_version, verify_block_body_hash,
};
use crate::tracer::{NodeTracer, trace_fields};
use serde_json::Value;
use serde_json::json;
use yggdrasil_consensus::mempool::{MEMPOOL_ZERO_IDX, MempoolEntry, SharedMempool};
use yggdrasil_consensus::{ChainState, EpochSchedule, SecurityParam, kes_period_of_slot};
use yggdrasil_ledger::{
    BlockNo, Decoder, EpochBoundaryEvent, HeaderHash, LedgerState, Point, SlotNo,
};
use yggdrasil_network::{
    AfterSlot, ChainSyncClient, ConsensusLedgerPeerInputs, ConsensusLedgerPeerSource,
    LedgerPeerSnapshot, LedgerPeerUseDecision, LedgerStateJudgement,
    LiveLedgerPeerRefreshObservation, PeerAccessPoint, PeerRegistry, PeerSnapshotFileObservation,
    PeerSnapshotFileSource, PeerSnapshotFreshness, TopologyConfig, UseLedgerPeers,
    always_eligible_snapshot_peers, derive_peer_snapshot_freshness,
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
    OutboundPeerManager, RuntimeRootPeerSources, apply_control_close,
    ledger_peer_snapshot_from_ledger_state, peer_share_request_amount, point_slot,
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

fn tip_context_from_chain_db<I, V, L>(
    chain_db: &ChainDb<I, V, L>,
) -> (Option<SlotNo>, Option<BlockNo>, Option<HeaderHash>)
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    match chain_db.tip() {
        Point::Origin => (None, None, None),
        Point::BlockPoint(slot, hash) => {
            let block_no = chain_db
                .volatile()
                .get_block(&hash)
                .or_else(|| chain_db.immutable().get_block(&hash))
                .map(|block| block.header.block_no);
            (Some(slot), block_no, Some(hash))
        }
    }
}

fn mempool_entries_for_forging(mempool: &SharedMempool) -> Vec<MempoolEntry> {
    let snapshot = mempool.snapshot();
    let mut entries = snapshot
        .mempool_txids_after(MEMPOOL_ZERO_IDX)
        .into_iter()
        .filter_map(|(_, idx, _)| snapshot.mempool_lookup_tx(idx).cloned())
        .collect::<Vec<_>>();
    // Keep forge-body assembly deterministic and fee-ordered (descending).
    entries.sort_by_key(|e| std::cmp::Reverse(e.fee));
    entries
}

fn extract_inner_block_bytes(raw_envelope: &[u8]) -> Result<&[u8], SyncError> {
    let mut dec = Decoder::new(raw_envelope);
    let _ = dec.array().map_err(SyncError::LedgerDecode)?;
    dec.skip().map_err(SyncError::LedgerDecode)?;
    let body_start = dec.position();
    dec.skip().map_err(SyncError::LedgerDecode)?;
    let body_end = dec.position();
    dec.slice(body_start, body_end)
        .map_err(SyncError::LedgerDecode)
}

fn self_validate_forged_block(forged: &ForgedBlock) -> Result<(), SyncError> {
    let raw_envelope = serialize_forged_block_cbor(forged);
    let decoded = decode_multi_era_block(&raw_envelope)?;

    validate_block_protocol_version(&decoded)?;
    verify_block_body_hash(&raw_envelope)?;

    let raw_inner_block = extract_inner_block_bytes(&raw_envelope)?;
    validate_block_body_size(&decoded, raw_inner_block)?;

    let decoded_block = multi_era_block_to_block(&decoded, &raw_envelope);
    if decoded_block.header.hash != forged.header_hash {
        return Err(SyncError::Recovery(
            "forged header hash mismatch".to_owned(),
        ));
    }
    if decoded_block.header.slot_no != forged.slot {
        return Err(SyncError::Recovery("forged slot mismatch".to_owned()));
    }
    if decoded_block.header.block_no != forged.block_number {
        return Err(SyncError::Recovery(
            "forged block number mismatch".to_owned(),
        ));
    }

    Ok(())
}

/// Emit a warning when the operational certificate is close to KES expiry.
///
/// Upstream reference: `praosCheckCanForge` / `KESInfo` style operator
/// observability around certificate validity windows.
const KES_EXPIRY_WARNING_THRESHOLD_PERIODS: u64 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KesExpiryWarning {
    current_period: u64,
    cert_start_period: u64,
    cert_end_period: u64,
    remaining_periods: u64,
    remaining_slots: u64,
}

fn kes_expiry_warning(
    creds: &BlockProducerCredentials,
    current_slot: SlotNo,
) -> Option<KesExpiryWarning> {
    let current_period = kes_period_of_slot(current_slot.0, creds.slots_per_kes_period).ok()?;
    kes_expiry_warning_from_periods(
        current_period,
        creds.operational_cert.kes_period,
        creds.max_kes_evolutions,
        creds.slots_per_kes_period,
    )
}

fn kes_expiry_warning_from_periods(
    current_period: u64,
    cert_start_period: u64,
    max_kes_evolutions: u64,
    slots_per_kes_period: u64,
) -> Option<KesExpiryWarning> {
    let cert_end_period = cert_start_period.checked_add(max_kes_evolutions)?;
    let remaining_periods = cert_end_period.saturating_sub(current_period);
    if remaining_periods > KES_EXPIRY_WARNING_THRESHOLD_PERIODS {
        return None;
    }

    Some(KesExpiryWarning {
        current_period,
        cert_start_period,
        cert_end_period,
        remaining_periods,
        remaining_slots: remaining_periods.saturating_mul(slots_per_kes_period),
    })
}

/// Live consensus-fed ledger-peer source backed by `ChainDb`.
///
/// Implements the network crate's `ConsensusLedgerPeerSource` trait so the
/// network-owned `live_refresh_ledger_peer_registry` orchestration can pull
/// authoritative `(latest_slot, judgement, ledger_snapshot)` inputs from the
/// node's storage layer without the network crate depending on storage types.
///
/// Carries the genesis timing inputs (`system_start_unix_secs`,
/// `slot_length_secs`) plus the configured `max_ledger_state_age_secs`
/// threshold so each `observe()` call can derive a real
/// [`LedgerStateJudgement`] from the recovered tip's wall-clock age,
/// matching upstream `mkLedgerStateJudgement` from
/// `Cardano.Node.Diffusion.Configuration` instead of hardcoding
/// `YoungEnough`.
struct ChainDbConsensusLedgerSource<'a, I, V, L>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    chain_db: &'a Arc<RwLock<ChainDb<I, V, L>>>,
    base_ledger_state: &'a LedgerState,
    tracer: &'a NodeTracer,
    /// Seconds since the Unix epoch of `ShelleyGenesis.system_start`.
    /// `None` falls back to the legacy `YoungEnough` behaviour to keep
    /// no-genesis test paths working.
    system_start_unix_secs: Option<f64>,
    /// Slot duration in seconds from `ShelleyGenesis.slot_length`.
    /// `None` falls back to the legacy `YoungEnough` behaviour.
    slot_length_secs: Option<f64>,
    /// Maximum tolerated tip age in seconds before the judgement flips to
    /// `TooOld`. Upstream uses `stabilityWindow * slotLength` (≈
    /// `3 * k / f * slotLength`).
    max_ledger_state_age_secs: f64,
    /// Era-aware epoch schedule for boundary-aware ChainDb recovery.
    epoch_schedule: Option<EpochSchedule>,
}

impl<I, V, L> ConsensusLedgerPeerSource for ChainDbConsensusLedgerSource<'_, I, V, L>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    fn observe(&mut self) -> ConsensusLedgerPeerInputs {
        let chain_db = self.chain_db.read().expect("chain db lock poisoned");
        let tip = chain_db.recovery().tip;
        let recovery_result = match self.epoch_schedule {
            Some(epoch_schedule) => recover_ledger_state_chaindb_epoch_boundary(
                &chain_db,
                self.base_ledger_state.clone(),
                epoch_schedule,
                None,
            ),
            None => recover_ledger_state_chaindb(&chain_db, self.base_ledger_state.clone()),
        };
        match recovery_result {
            Ok(recovery) => {
                let latest_slot = point_slot(&recovery.point).or_else(|| point_slot(&tip));
                let judgement = derive_judgement_for_observe(
                    latest_slot,
                    self.system_start_unix_secs,
                    self.slot_length_secs,
                    self.max_ledger_state_age_secs,
                );
                ConsensusLedgerPeerInputs {
                    latest_slot,
                    judgement,
                    ledger_snapshot: ledger_peer_snapshot_from_ledger_state(&recovery.ledger_state),
                }
            }
            Err(err) => {
                self.tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to recover ledger peers from chain db",
                    trace_fields([("error", json!(err.to_string()))]),
                );
                ConsensusLedgerPeerInputs {
                    latest_slot: point_slot(&tip),
                    judgement: LedgerStateJudgement::Unavailable,
                    ledger_snapshot: LedgerPeerSnapshot::default(),
                }
            }
        }
    }
}

/// Derives a [`LedgerStateJudgement`] for [`ChainDbConsensusLedgerSource::observe`].
///
/// Falls back to `YoungEnough` (the historical pre-slice behaviour) when
/// either of the genesis timing inputs is `None`, so tests and other
/// non-production paths that don't configure genesis aren't disturbed.
/// When both inputs are present, delegates to
/// [`yggdrasil_network::judge_ledger_state_age`] for the upstream-aligned
/// comparison.
fn derive_judgement_for_observe(
    tip_slot: Option<u64>,
    system_start_unix_secs: Option<f64>,
    slot_length_secs: Option<f64>,
    max_age_secs: f64,
) -> LedgerStateJudgement {
    derive_judgement_at(
        tip_slot,
        system_start_unix_secs,
        slot_length_secs,
        max_age_secs,
        wall_clock_unix_secs(),
    )
}

/// Pure variant of [`derive_judgement_for_observe`] that takes an explicit
/// `now_unix_secs` for deterministic testing. The production helper above
/// is a thin wrapper that supplies the real wall-clock value.
pub(crate) fn derive_judgement_at(
    tip_slot: Option<u64>,
    system_start_unix_secs: Option<f64>,
    slot_length_secs: Option<f64>,
    max_age_secs: f64,
    now_unix_secs: f64,
) -> LedgerStateJudgement {
    if system_start_unix_secs.is_none() || slot_length_secs.is_none() {
        return LedgerStateJudgement::YoungEnough;
    }
    yggdrasil_network::judge_ledger_state_age(yggdrasil_network::LedgerStateAgeInputs {
        tip_slot,
        system_start_unix_secs,
        slot_length_secs,
        max_age_secs,
        now_unix_secs,
    })
}

fn wall_clock_unix_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn block_producer_ledger_state_judgement(
    tip_slot: Option<SlotNo>,
    config: &RuntimeBlockProducerConfig,
) -> LedgerStateJudgement {
    match config.max_ledger_state_age_secs {
        Some(max_age_secs) => derive_judgement_at(
            tip_slot.map(|slot| slot.0),
            config.system_start_unix_secs,
            Some(config.slot_length.as_secs_f64()),
            max_age_secs,
            wall_clock_unix_secs(),
        ),
        None => LedgerStateJudgement::YoungEnough,
    }
}

/// Live `peerSnapshotFile` source that re-reads the configured snapshot path
/// each tick.
struct FilePeerSnapshotSource<'a> {
    path: Option<&'a str>,
    tracer: &'a NodeTracer,
}

impl PeerSnapshotFileSource for FilePeerSnapshotSource<'_> {
    fn observe(&mut self) -> PeerSnapshotFileObservation {
        let Some(path) = self.path else {
            return PeerSnapshotFileObservation::not_configured();
        };

        match load_peer_snapshot_file(Path::new(path)) {
            Ok(loaded_snapshot) => {
                PeerSnapshotFileObservation::loaded(loaded_snapshot.slot, loaded_snapshot.snapshot)
            }
            Err(err) => {
                self.tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to refresh configured peer snapshot",
                    trace_fields([
                        ("snapshotPath", json!(path)),
                        ("error", json!(err.to_string())),
                    ]),
                );
                PeerSnapshotFileObservation::unavailable()
            }
        }
    }
}

pub mod ledger_judgement;
pub use ledger_judgement::LedgerJudgementSettings;

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
