#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Node runtime — wires networking, storage, and protocol client drivers
//! into a cohesive sync lifecycle.
//!
//! Reference: `cardano-node/src/Cardano/Node/Run.hs`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side runtime shell that
//! re-exports the sub-modules under `runtime/` and exposes the
//! verified-sync service entry points the CLI consumes. R271-
//! arc rounds reduced this file from a 7,269-line monolith to
//! a thin re-export shell. Upstream wires the equivalent
//! across `Ouroboros.Consensus.Node.Run`, `Cardano.Node.Run`,
//! and `Cardano.Node.Diffusion`; Yggdrasil isolates each
//! concern in its own runtime/* sub-module.

use std::sync::Arc;

use yggdrasil_node_sync::LedgerCheckpointTracking;
#[cfg(all(test, feature = "forge"))]
use yggdrasil_node_sync::VerifiedSyncServiceConfig;

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
#[cfg(all(test, feature = "forge"))]
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
#[cfg(all(test, feature = "forge"))]
use cm_actions::direct_sync_bootstrap_pending;
use cm_actions::{
    apply_cm_actions, governor_action_name, governor_action_peer, outbound_cm_local_addr,
    retire_failed_outbound_peer, split_timeout_cm_actions_for_governor,
    suppress_outbound_promotions_while_bootstrap_pending, update_registry_status_from_cm,
};

#[cfg(feature = "forge")]
pub mod forge;
#[cfg(all(feature = "forge", test))]
use forge::kes_expiry_warning_from_periods;
#[cfg(feature = "forge")]
use forge::{
    kes_expiry_warning, mempool_entries_for_forging, self_validate_forged_block,
    tip_context_from_chain_db,
};

pub mod ledger_judgement;
pub use ledger_judgement::LedgerJudgementSettings;

pub mod ledger_peer_source;
#[cfg(feature = "forge")]
use ledger_peer_source::block_producer_ledger_state_judgement;
use ledger_peer_source::refresh_ledger_peer_sources_from_chain_db;
#[cfg(all(test, feature = "forge"))]
use ledger_peer_source::{derive_judgement_at, wall_clock_unix_secs};

#[cfg(feature = "forge")]
pub mod block_producer_loop;
#[cfg(feature = "forge")]
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
#[cfg(all(test, feature = "forge"))]
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
#[cfg(all(test, feature = "forge"))]
use sync_session::{CheckpointPersistenceOutcome, checkpoint_trace_fields};
use sync_session::{
    handle_reconnect_batch_error, refresh_chain_db_reconnect_fallback_peers,
    shared_chaindb_lock_error, synchronize_chain_sync_to_point, trace_checkpoint_outcome,
    trace_epoch_boundary_events, trace_reconnectable_sync_error, trace_session_established,
    trace_shutdown_before_bootstrap, trace_shutdown_during_session,
};

mod reconnecting;
#[cfg(all(test, feature = "forge"))]
use reconnecting::cache_confirmed_entries;
#[cfg(all(test, feature = "forge"))]
use reconnecting::{
    BatchErrorDisposition, re_admit_rolled_back_tx_ids, record_verified_batch_progress,
};
use reconnecting::{BatchTraceExtras, ReconnectingRunState};

mod tracing;
#[cfg(all(test, feature = "forge"))]
use tracing::session_established_trace_fields;
use tracing::{sync_error_trace_fields, verified_sync_batch_trace_fields};
mod keep_alive;

// Tests gated behind `forge` because the bulk of them (sample
// forged block, KES window, body-hash self-validation, block-
// producer ledger judgement) need the `yggdrasil-node-block-producer`
// crate that the `forge` feature gates. Relay-only `--no-default-
// features` builds skip the test target entirely; the default
// build runs all tests as before.
#[cfg(all(test, feature = "forge"))]
mod tests;
