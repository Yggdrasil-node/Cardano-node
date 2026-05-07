//! Node runtime — wires networking, storage, and protocol client drivers
//! into a cohesive sync lifecycle.
//!
//! Reference: `cardano-node/src/Cardano/Node/Run.hs`.

use std::sync::{Arc, RwLock};

use crate::sync::LedgerCheckpointTracking;
#[cfg(test)]
use crate::sync::VerifiedSyncServiceConfig;
use crate::tracer::{NodeTracer, trace_fields};
use serde_json::json;
use yggdrasil_consensus::{ChainState, EpochSchedule, SecurityParam};
use yggdrasil_ledger::{LedgerState, Point};
use yggdrasil_network::{
    LedgerPeerUseDecision, LedgerStateJudgement, LiveLedgerPeerRefreshObservation, PeerRegistry,
    PeerSnapshotFreshness, TopologyConfig, live_refresh_ledger_peer_registry_observed,
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

pub mod sync_session;
#[cfg(test)]
use sync_session::{CheckpointPersistenceOutcome, checkpoint_trace_fields};
use sync_session::{
    handle_reconnect_batch_error, refresh_chain_db_reconnect_fallback_peers,
    shared_chaindb_lock_error, synchronize_chain_sync_to_point, trace_checkpoint_outcome,
    trace_epoch_boundary_events, trace_reconnectable_sync_error, trace_session_established,
    trace_shutdown_before_bootstrap, trace_shutdown_during_session,
};

mod reconnecting;
#[cfg(test)]
use reconnecting::cache_confirmed_entries;
#[cfg(test)]
use reconnecting::{
    BatchErrorDisposition, re_admit_rolled_back_tx_ids, record_verified_batch_progress,
};
use reconnecting::{BatchTraceExtras, ReconnectingRunState};

mod tracing;
#[cfg(test)]
use tracing::session_established_trace_fields;
use tracing::{sync_error_trace_fields, verified_sync_batch_trace_fields};
mod keep_alive;

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
