//! Connection-manager / governor-action plumbing.
//!
//! Mirrors upstream `Ouroboros.Network.ConnectionManager.Core` per-peer
//! state machine and the network crate's `GovernorAction` dispatch glue
//! that bridges the peer-selection governor's target-state decisions to
//! mux registry updates.
//!
//! Eleven items move from `runtime.rs` here:
//!
//! - `governor_action_name`, `governor_action_peer` — trace-fields / peer
//!   target lookup helpers for `GovernorAction` enum variants.
//! - `direct_sync_bootstrap_pending`,
//!   `suppress_outbound_promotions_while_bootstrap_pending` — runtime-side
//!   bootstrap-coordination helpers that gate governor promotions while the
//!   verified-sync bootstrap has not completed.
//! - `outbound_cm_local_addr`, `data_flow_from_version_data`,
//!   `peer_status_from_cm_state`, `update_registry_status_from_cm` —
//!   per-peer mux-registry state mapping helpers.
//! - `retire_failed_outbound_peer` — async cleanup that demotes a failed
//!   peer through the connection manager and unregisters its mux.
//! - `apply_cm_actions` — async dispatch that applies a batch of
//!   `CmAction`s against the connection manager.
//! - `split_timeout_cm_actions_for_governor` — partitions timeout-driven
//!   `CmAction`s by whether they currently apply to a governor-known peer.
//!
//! Extracted from `runtime.rs` in R271o (Phase γ §R271 fifteenth slice).

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use serde_json::json;

use yggdrasil_network::{
    AbstractState, CmAction, ConnectionManagerState, DataFlow, GovernorAction, GovernorState,
    NodeToNodeVersionData, PeerRegistry, PeerSource, PeerStatus,
};

use crate::tracer::{NodeMetrics, NodeTracer, trace_fields};

use super::peer_management::OutboundPeerManager;
use super::peer_session::NodeConfig;

pub(super) fn governor_action_name(action: &GovernorAction) -> &'static str {
    match action {
        GovernorAction::PromoteToWarm(_) => "PromoteToWarm",
        GovernorAction::PromoteToHot(_) => "PromoteToHot",
        GovernorAction::DemoteToWarm(_) => "DemoteToWarm",
        GovernorAction::DemoteToCold(_) => "DemoteToCold",
        GovernorAction::ForgetPeer(_) => "ForgetPeer",
        GovernorAction::ShareRequest(_) => "ShareRequest",
        GovernorAction::RequestPublicRoots => "RequestPublicRoots",
        GovernorAction::RequestBigLedgerPeers => "RequestBigLedgerPeers",
        GovernorAction::AdoptInboundPeer(_) => "AdoptInboundPeer",
    }
}

pub(super) fn governor_action_peer(action: &GovernorAction) -> Option<SocketAddr> {
    match action {
        GovernorAction::PromoteToWarm(peer)
        | GovernorAction::PromoteToHot(peer)
        | GovernorAction::DemoteToWarm(peer)
        | GovernorAction::DemoteToCold(peer)
        | GovernorAction::ForgetPeer(peer)
        | GovernorAction::ShareRequest(peer)
        | GovernorAction::AdoptInboundPeer(peer) => Some(*peer),
        GovernorAction::RequestPublicRoots | GovernorAction::RequestBigLedgerPeers => None,
    }
}

pub(super) fn direct_sync_bootstrap_pending(registry: &PeerRegistry) -> bool {
    let has_direct_sync_hot_peer = registry
        .iter()
        .any(|(_, entry)| entry.status == PeerStatus::PeerHot);
    if has_direct_sync_hot_peer {
        return false;
    }

    registry.iter().any(|(_, entry)| {
        entry.status == PeerStatus::PeerCooling
            && entry.sources.contains(&PeerSource::PeerSourceBootstrap)
    })
}

pub(super) fn suppress_outbound_promotions_while_bootstrap_pending(
    registry: &PeerRegistry,
    actions: &mut Vec<GovernorAction>,
) -> usize {
    if !direct_sync_bootstrap_pending(registry) {
        return 0;
    }

    let before = actions.len();
    actions.retain(|action| {
        !matches!(
            action,
            GovernorAction::PromoteToWarm(_) | GovernorAction::PromoteToHot(_)
        )
    });
    before - actions.len()
}

pub(super) fn outbound_cm_local_addr() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 0))
}

pub(super) fn data_flow_from_version_data(version_data: &NodeToNodeVersionData) -> DataFlow {
    if version_data.initiator_only_diffusion_mode {
        DataFlow::Unidirectional
    } else {
        DataFlow::Duplex
    }
}

pub(super) fn peer_status_from_cm_state(state: AbstractState) -> PeerStatus {
    match state {
        AbstractState::OutboundUniSt
        | AbstractState::OutboundDupSt(_)
        | AbstractState::InboundSt(_)
        | AbstractState::DuplexSt => PeerStatus::PeerWarm,
        _ => PeerStatus::PeerCold,
    }
}

pub(super) fn update_registry_status_from_cm(
    connection_manager: &Arc<RwLock<ConnectionManagerState>>,
    peer_registry: &Arc<RwLock<PeerRegistry>>,
    peer: SocketAddr,
) -> bool {
    let state = {
        let cm = connection_manager
            .read()
            .expect("connection manager lock poisoned");
        cm.abstract_state_of(&peer)
    };
    let mut registry = peer_registry.write().expect("peer registry lock poisoned");
    registry.set_status(peer, peer_status_from_cm_state(state))
}

pub(super) async fn retire_failed_outbound_peer(
    peer_manager: &mut OutboundPeerManager,
    peer_registry: &Arc<RwLock<PeerRegistry>>,
    connection_manager: &Arc<RwLock<ConnectionManagerState>>,
    governor_state: &mut GovernorState,
    peer: SocketAddr,
    reason: &'static str,
    tracer: &NodeTracer,
) -> bool {
    governor_state.clear_in_flight_warm(&peer);
    governor_state.clear_in_flight_hot(&peer);
    governor_state.clear_in_flight_demote_warm(&peer);
    governor_state.clear_in_flight_demote_hot(&peer);

    let connection_changed = peer_manager.demote_to_cold(peer).await;
    let cm_changed = {
        let mut cm = connection_manager
            .write()
            .expect("connection manager lock poisoned");
        let marked = cm.mark_terminating(peer, Some(reason.to_owned())).is_some();
        let expired = cm.time_wait_expired(peer).is_ok();
        let removed = cm.remove_terminated(&peer);
        marked || expired || removed
    };
    let (status_changed, preserved_bootstrap_hot) = {
        let mut registry = peer_registry.write().expect("peer registry lock poisoned");
        let preserve_bootstrap_hot = registry.get(&peer).is_some_and(|entry| {
            entry.status == PeerStatus::PeerHot
                && entry.sources.contains(&PeerSource::PeerSourceBootstrap)
        });
        if preserve_bootstrap_hot {
            (registry.set_hot_tip_slot(peer, None), true)
        } else {
            (registry.set_status(peer, PeerStatus::PeerCold), false)
        }
    };

    let changed = connection_changed || cm_changed || status_changed;
    if changed {
        tracer.trace_runtime(
            "Net.Governor",
            "Info",
            "outbound peer retired after protocol failure",
            trace_fields([
                ("peer", json!(peer.to_string())),
                ("reason", json!(reason)),
                ("connectionChanged", json!(connection_changed)),
                ("connectionManagerChanged", json!(cm_changed)),
                ("statusChanged", json!(status_changed)),
                ("preservedBootstrapHot", json!(preserved_bootstrap_hot)),
            ]),
        );
    }
    changed
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn apply_cm_actions(
    peer_manager: &mut OutboundPeerManager,
    peer_registry: &Arc<RwLock<PeerRegistry>>,
    connection_manager: &Arc<RwLock<ConnectionManagerState>>,
    governor_state: &mut GovernorState,
    node_config: &NodeConfig,
    actions: Vec<CmAction>,
    tracer: &NodeTracer,
    max_concurrent_block_fetch_peers: u8,
    metrics: Option<&Arc<NodeMetrics>>,
) -> bool {
    let mut changed = false;
    for cm_action in actions {
        match cm_action {
            CmAction::StartConnect(peer) => {
                if peer_manager
                    .promote_to_warm(node_config, peer, governor_state, tracer)
                    .await
                {
                    // Phase 6 — operator opt-in: when
                    // `max_concurrent_block_fetch_peers > 1`, migrate
                    // the freshly-promoted session's BlockFetchClient
                    // into a per-peer worker so the sync loop's
                    // multi-peer branch can dispatch through the
                    // shared `FetchWorkerPool`.  At knob == 1 the
                    // session keeps direct ownership of its
                    // BlockFetchClient (legacy single-peer path).
                    //
                    // Mirrors upstream `bracketSyncWithFetchClient`:
                    // the per-peer fetch state is owned by a dedicated
                    // task for the connection's lifetime.
                    if max_concurrent_block_fetch_peers > 1 {
                        let migrated = peer_manager.migrate_session_to_worker(peer).await;
                        if migrated {
                            if let Some(m) = metrics {
                                m.inc_blockfetch_workers_migrated();
                            }
                            tracer.trace_runtime(
                                "Net.BlockFetch.Worker",
                                "Info",
                                "BlockFetch migrated to per-peer worker",
                                trace_fields([
                                    ("peer", json!(peer.to_string())),
                                    ("maxConcurrent", json!(max_concurrent_block_fetch_peers)),
                                ]),
                            );
                        }
                    }

                    let data_flow = peer_manager
                        .warm_peers
                        .get(&peer)
                        .map(|managed| data_flow_from_version_data(&managed.session.version_data))
                        .unwrap_or(DataFlow::Duplex);

                    let handshake_result = {
                        let mut cm = connection_manager
                            .write()
                            .expect("connection manager lock poisoned");
                        cm.outbound_handshake_done(outbound_cm_local_addr(), peer, data_flow)
                    };

                    match handshake_result {
                        Ok(_) => {
                            changed |= update_registry_status_from_cm(
                                connection_manager,
                                peer_registry,
                                peer,
                            );
                        }
                        Err(err) => {
                            let _ = peer_manager.demote_to_cold(peer).await;
                            let mut cm = connection_manager
                                .write()
                                .expect("connection manager lock poisoned");
                            let _ = cm.outbound_connect_failed(peer);
                            governor_state.record_failure(peer);
                            tracer.trace_runtime(
                                "Net.Governor",
                                "Warning",
                                "connection-manager outbound handshake transition failed",
                                trace_fields([
                                    ("peer", json!(peer.to_string())),
                                    ("error", json!(err.to_string())),
                                ]),
                            );
                        }
                    }
                } else {
                    let mut cm = connection_manager
                        .write()
                        .expect("connection manager lock poisoned");
                    let _ = cm.outbound_connect_failed(peer);
                }
            }
            CmAction::TerminateConnection(conn_id) => {
                let peer = conn_id.remote;
                let connection_changed = peer_manager.demote_to_cold(peer).await;
                let status_changed = {
                    let mut registry = peer_registry.write().expect("peer registry lock poisoned");
                    registry.set_status(peer, PeerStatus::PeerCold)
                };
                changed |= connection_changed || status_changed;
            }
            CmAction::StartResponderTimeout(conn_id) => {
                tracer.trace_runtime(
                    "Net.Governor",
                    "Debug",
                    "connection-manager responder timeout requested",
                    trace_fields([("peer", json!(conn_id.remote.to_string()))]),
                );
            }
            CmAction::PruneConnections(peers) => {
                for peer in peers {
                    let connection_changed = peer_manager.demote_to_cold(peer).await;
                    let status_changed = {
                        let mut registry =
                            peer_registry.write().expect("peer registry lock poisoned");
                        registry.set_status(peer, PeerStatus::PeerCold)
                    };
                    changed |= connection_changed || status_changed;
                }
            }
        }
    }

    changed
}

/// Split timeout-driven CM actions into those the governor can execute
/// directly and those that should be handled by the inbound loop.
///
/// The inbound accept loop owns the abort-handle registry for inbound mux
/// sessions, so inbound prune/terminate effects are deferred there.
pub(super) fn split_timeout_cm_actions_for_governor(
    peer_manager: &OutboundPeerManager,
    actions: Vec<CmAction>,
) -> (Vec<CmAction>, usize) {
    let mut applicable = Vec::new();
    let mut deferred = 0usize;

    for action in actions {
        match &action {
            CmAction::PruneConnections(_) | CmAction::StartResponderTimeout(_) => {
                deferred += 1;
            }
            CmAction::TerminateConnection(conn_id)
                if !peer_manager.warm_peers.contains_key(&conn_id.remote) =>
            {
                deferred += 1;
            }
            _ => applicable.push(action),
        }
    }

    (applicable, deferred)
}
