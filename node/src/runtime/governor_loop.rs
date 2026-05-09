//! Peer governor slot-tick loop.
//!
//! Mirrors upstream `Ouroboros.Network.PeerSelection.Governor.peerSelectionGovernor`
//! plus `Cardano.Node.Diffusion`'s outer wiring loop. Each tick:
//! 1. Refreshes root peers from DNS-backed providers (local roots,
//!    bootstrap, public roots).
//! 2. Refreshes ledger peers from the current ChainDb recovery view
//!    plus optional peer-snapshot file overlay.
//! 3. Drives warm-peer KeepAlive heartbeats.
//! 4. Reads density observations from the per-peer ChainSync registry.
//! 5. Computes governor decisions via [`super::evaluate_*`] (in
//!    `crate::governor::state`) and applies them through the
//!    [`super::OutboundPeerManager`] effect layer.
//! 6. Exports counters/gauges for `/metrics` and emits per-action traces.
//!
//! Extracted from `runtime.rs` in R271l.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side async slot-tick body
//! for the peer governor. Mirrors the loop body of upstream
//! `Ouroboros.Network.PeerSelection.Governor.peerSelectionGovernor`
//! plus `Cardano.Node.Diffusion`'s outer wiring loop. Haskell wires
//! the governor loop inline; Yggdrasil isolates it for testability
//! and per-tick instrumentation.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde_json::json;
use yggdrasil_consensus::mempool::SharedMempool;
use yggdrasil_ledger::{LedgerState, SlotNo};
use yggdrasil_network::{
    AcquireOutboundResult, ConnectionManagerState, ConsensusMode, GovernorAction, GovernorState,
    NodePeerSharing, PeerRegistry, PeerSelectionCounters, PeerSelectionTimeouts, PeerSource,
    PeerStateAction, PeerStatus, ReleaseOutboundResult, TopologyConfig, churn_mode_from_fetch_mode,
    compute_association_mode, fetch_mode_from_judgement, governor_action_to_peer_state_action,
    peer_selection_mode, pick_churn_regime,
};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

use crate::tracer::{NodeMetrics, NodeTracer, trace_fields};

use super::governor_config::RuntimeGovernorConfig;
use super::peer_session::NodeConfig;
use super::{
    OutboundPeerManager, RuntimeRootPeerSources, apply_cm_actions, apply_control_close,
    governor_action_name, governor_action_peer, outbound_cm_local_addr, peer_share_request_amount,
    refresh_ledger_peer_sources_from_chain_db, reserve_bootstrap_sync_peers,
    retire_failed_outbound_peer, split_timeout_cm_actions_for_governor,
    suppress_outbound_promotions_while_bootstrap_pending, update_registry_status_from_cm,
};

/// Run the peer governor loop until shutdown.
///
/// The loop periodically refreshes root peers from DNS-backed providers,
/// refreshes ledger peers from the current ChainDb recovery view plus optional
/// peer snapshot file, drives warm-peer KeepAlive traffic, and then executes
/// governor actions against the shared peer registry and outbound warm sessions.
#[allow(clippy::too_many_arguments)]
pub async fn run_governor_loop<I, V, L, F>(
    node_config: NodeConfig,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    peer_registry: Arc<RwLock<PeerRegistry>>,
    connection_manager: Arc<RwLock<ConnectionManagerState>>,
    mut governor_state: GovernorState,
    config: RuntimeGovernorConfig,
    topology: TopologyConfig,
    base_ledger_state: LedgerState,
    mempool: Option<SharedMempool>,
    inbound_peers: Option<Arc<RwLock<BTreeMap<SocketAddr, NodePeerSharing>>>>,
    tracer: NodeTracer,
    metrics: Option<Arc<NodeMetrics>>,
    shutdown: F,
) where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let mut interval = tokio::time::interval(config.tick_interval);
    // Phase 6 — when the runtime caller has provided a shared
    // `FetchWorkerPool`, use it so the sync loop observes peer
    // worker registrations made by promote_to_warm/migrate.
    // Otherwise fall back to a private per-governor pool.
    let mut peer_manager = match &config.shared_fetch_worker_pool {
        Some(pool) => OutboundPeerManager::with_fetch_worker_pool(pool.clone()),
        None => OutboundPeerManager::new(),
    };
    let mut root_sources = RuntimeRootPeerSources::new(&topology);
    let timeouts = PeerSelectionTimeouts::default();
    governor_state.enable_root_big_ledger_requests = true;
    governor_state.inbound_peers_retry_delay = timeouts.inbound_peers_retry_delay;
    governor_state.max_inbound_peers = timeouts.max_inbound_peers;
    tokio::pin!(shutdown);

    {
        let mut registry = peer_registry.write().expect("peer registry lock poisoned");
        root_sources.sync_registry(&mut registry);
        let _ = reserve_bootstrap_sync_peers(
            &mut registry,
            std::iter::once(node_config.peer_addr).chain(root_sources.bootstrap_peer_addrs()),
        );
        let _ = refresh_ledger_peer_sources_from_chain_db(
            &mut registry,
            &chain_db,
            &base_ledger_state,
            &topology,
            &tracer,
            config.ledger_judgement_settings,
            config.epoch_schedule,
        );
    }

    tracer.trace_runtime(
        "Net.Governor",
        "Notice",
        "peer governor started",
        trace_fields([
            ("tickIntervalSecs", json!(config.tick_interval.as_secs())),
            ("peerSharing", json!(config.peer_sharing.is_enabled())),
            (
                "consensusMode",
                json!(match config.consensus_mode {
                    ConsensusMode::PraosMode => "PraosMode",
                    ConsensusMode::GenesisMode => "GenesisMode",
                }),
            ),
            ("targetKnown", json!(config.targets.target_known)),
            (
                "targetEstablished",
                json!(config.targets.target_established),
            ),
            ("targetActive", json!(config.targets.target_active)),
        ]),
    );

    loop {
        tokio::select! {
            biased;

            () = &mut shutdown => {
                // -- Graceful outbound drain (upstream governor shutdown) --
                // Phase 1: Signal all outbound peers to terminate their
                // mini-protocol bundles via ControlMessage::Terminate.
                let outbound_peers: Vec<SocketAddr> =
                    peer_manager.warm_peers.keys().copied().collect();
                for peer in &outbound_peers {
                    if let Some(managed) = peer_manager.warm_peers.get_mut(peer) {
                        apply_control_close(&mut managed.control);
                    }
                }

                tracer.trace_runtime(
                    "Net.Governor",
                    "Info",
                    "outbound shutdown: signalled terminate to all peers",
                    trace_fields([("peerCount", json!(outbound_peers.len()))]),
                );

                // Phase 2: Release connections through CM and clean up.
                let mut drained = 0usize;
                for peer in outbound_peers {
                    let (release_result, cm_actions) = {
                        let mut cm = connection_manager
                            .write()
                            .expect("connection manager lock poisoned");
                        cm.release_outbound_connection(peer)
                    };
                    let changed = apply_cm_actions(
                        &mut peer_manager,
                        &peer_registry,
                        &connection_manager,
                        &mut governor_state,
                        &node_config,
                        cm_actions,
                        &tracer,
                        config.max_concurrent_block_fetch_peers,
                    metrics.as_ref(),
                    )
                    .await;

                    match release_result {
                        ReleaseOutboundResult::DemotedToColdLocal(_)
                        | ReleaseOutboundResult::Noop(_) => {
                            let _ = update_registry_status_from_cm(
                                &connection_manager,
                                &peer_registry,
                                peer,
                            );
                            if changed {
                                drained += 1;
                            }
                        }
                        ReleaseOutboundResult::Error(err) => {
                            tracer.trace_runtime(
                                "Net.Governor",
                                "Warning",
                                "connection-manager shutdown drain release failed",
                                trace_fields([
                                    ("peer", json!(peer.to_string())),
                                    ("error", json!(err.to_string())),
                                ]),
                            );
                        }
                    }
                }

                tracer.trace_runtime(
                    "Net.Governor",
                    "Notice",
                    "peer governor stopped",
                    trace_fields([("drainedPeers", json!(drained))]),
                );
                return;
            }

            _ = interval.tick() => {
                {
                    let timeout_actions = {
                        let mut cm = connection_manager
                            .write()
                            .expect("connection manager lock poisoned");
                        cm.timeout_tick(Instant::now())
                    };
                    if !timeout_actions.is_empty() {
                        let action_count = timeout_actions.len();
                        let (applicable_actions, deferred_actions) =
                            split_timeout_cm_actions_for_governor(
                                &peer_manager,
                                timeout_actions,
                            );

                        if deferred_actions > 0 {
                            tracer.trace_runtime(
                                "Net.Governor",
                                "Debug",
                                "connection-manager timeout actions deferred to inbound loop",
                                trace_fields([("deferredActions", json!(deferred_actions))]),
                            );
                        }

                        if applicable_actions.is_empty() {
                            continue;
                        }

                        let changed = apply_cm_actions(
                            &mut peer_manager,
                            &peer_registry,
                            &connection_manager,
                            &mut governor_state,
                            &node_config,
                            applicable_actions,
                            &tracer,
                            config.max_concurrent_block_fetch_peers,
                        metrics.as_ref(),
                        )
                        .await;
                        tracer.trace_runtime(
                            "Net.Governor",
                            "Debug",
                            "connection-manager timeout tick applied",
                            trace_fields([
                                ("actions", json!(action_count)),
                                ("appliedActions", json!(action_count - deferred_actions)),
                                ("deferredActions", json!(deferred_actions)),
                                ("changed", json!(changed)),
                            ]),
                        );
                    }
                }

                {
                    let mut registry = peer_registry.write().expect("peer registry lock poisoned");
                    root_sources.refresh(&mut registry, &tracer);
                }

                let failed_keepalive_peers = peer_manager
                    .drive_keepalives(config.keepalive_interval, &mut governor_state, &tracer)
                    .await;
                for peer in failed_keepalive_peers {
                    let _ = retire_failed_outbound_peer(
                        &mut peer_manager,
                        &peer_registry,
                        &connection_manager,
                        &mut governor_state,
                        peer,
                        "keepalive failure",
                        &tracer,
                    )
                    .await;
                }

                // Peer sharing is now governor-driven via ShareRequest actions
                // dispatched to specific target peers, matching upstream behavior.

                let ledger_observation = {
                    let mut registry = peer_registry.write().expect("peer registry lock poisoned");
                    refresh_ledger_peer_sources_from_chain_db(
                        &mut registry,
                        &chain_db,
                        &base_ledger_state,
                        &topology,
                        &tracer,
                        config.ledger_judgement_settings,
                        config.epoch_schedule,
                    )
                };

                let failed_tip_peers = peer_manager
                    .refresh_hot_peer_tips(&peer_registry, &mut governor_state, &tracer)
                    .await;
                for peer in failed_tip_peers {
                    let _ = retire_failed_outbound_peer(
                        &mut peer_manager,
                        &peer_registry,
                        &connection_manager,
                        &mut governor_state,
                        peer,
                        "hot peer tip query failure",
                        &tracer,
                    )
                    .await;
                }

                if let Some(best_peer) = peer_manager.best_hot_peer() {
                    if let Some(slot) = peer_manager
                        .warm_peers
                        .get(&best_peer)
                        .and_then(|managed| managed.last_known_tip.as_ref())
                        .and_then(|tip| tip.slot())
                    {
                        governor_state.metrics.record_fetchyness(best_peer, slot.0);
                    }
                    tracer.trace_runtime(
                        "Net.Governor",
                        "Debug",
                        "best hot peer selected",
                        trace_fields([("peer", json!(best_peer.to_string()))]),
                    );
                }

                // Purge expired mempool entries using the current chain tip slot.
                if let Some(ref mempool) = mempool {
                    let tip_slot = {
                        let db = chain_db.read().expect("chain_db lock poisoned");
                        db.volatile().tip().slot().unwrap_or(SlotNo(0))
                    };
                    let purged = mempool.purge_expired(tip_slot);
                    if purged > 0 {
                        tracer.trace_runtime(
                            "Mempool",
                            "Info",
                            "expired transactions purged",
                            trace_fields([
                                ("purged", json!(purged)),
                                ("tipSlot", json!(tip_slot.0)),
                            ]),
                        );
                    }
                    // Update mempool gauge metrics.
                    if let Some(ref m) = metrics {
                        m.set_mempool_gauges(mempool.len() as u64, mempool.size_bytes() as u64);
                    }
                }

                let local_root_groups = root_sources.local_root_targets();
                let ledger_state_judgement = ledger_observation.judgement;

                let selection_mode = peer_selection_mode(
                    &topology.bootstrap_peers,
                    ledger_state_judgement,
                );
                let association_mode = compute_association_mode(
                    &topology.bootstrap_peers,
                    &topology.use_ledger_peers,
                    config.peer_sharing,
                    ledger_state_judgement,
                );
                governor_state.fetch_mode =
                    fetch_mode_from_judgement(ledger_state_judgement);
                // Propagate the unified `FetchMode` signal into the
                // BlockFetch pool's per-peer concurrency cap. Mirrors
                // upstream `mkReadFetchMode` from
                // `Ouroboros.Network.BlockFetch.ConsensusInterface`,
                // which feeds `LedgerStateJudgement` into the BlockFetch
                // decision policy via `bfcMaxConcurrency{BulkSync,
                // Deadline}`. Without this, the pool stayed in whatever
                // mode it was constructed with regardless of how the
                // node's ledger judgement evolved.
                if let Some(pool) = config.block_fetch_pool.as_ref() {
                    if let Ok(mut pool) = pool.lock() {
                        pool.set_mode(governor_state.fetch_mode);
                    }
                }
                governor_state.churn_regime = pick_churn_regime(
                    churn_mode_from_fetch_mode(governor_state.fetch_mode),
                    &topology.bootstrap_peers,
                    config.consensus_mode,
                );

                if let Some(shared_inbound_peers) = inbound_peers.as_ref() {
                    let inbound_snapshot = {
                        let peers = shared_inbound_peers
                            .read()
                            .expect("inbound peers lock poisoned");
                        peers.iter().map(|(peer, mode)| (*peer, *mode)).collect::<Vec<_>>()
                    };
                    governor_state.set_inbound_peers(inbound_snapshot);
                }
                let (actions, suppressed_bootstrap_promotions) = {
                    let registry = peer_registry.read().expect("peer registry lock poisoned");
                    // Slice GD-Final — propagate per-peer ChainSync density
                    // from the runtime registry into governor_state.metrics
                    // so hot-demotion scoring applies the density-aware
                    // bonus on this tick.  No-op when no registry is wired.
                    if let Some(density_registry) = config.density_registry.as_ref() {
                        for (addr, _) in registry.iter() {
                            let d = crate::sync::read_peer_density(*addr, density_registry);
                            governor_state.metrics.set_density(*addr, d);
                        }
                    }
                    let mut actions = governor_state.tick(
                        &registry,
                        &config.targets,
                        &local_root_groups,
                        selection_mode,
                        association_mode,
                        Instant::now(),
                    );
                    let suppressed_bootstrap_promotions =
                        suppress_outbound_promotions_while_bootstrap_pending(
                            &registry,
                            &mut actions,
                        );

                    // Update Prometheus peer-selection counters after every tick.
                    if let Some(m) = metrics.as_ref() {
                        let c = PeerSelectionCounters::from_registry(
                            &registry,
                            Some(&governor_state),
                        );
                        m.set_peer_selection_counters(
                            config.targets.target_known as u64,
                            config.targets.target_established as u64,
                            config.targets.target_active as u64,
                            config.targets.target_known_big_ledger as u64,
                            config.targets.target_established_big_ledger as u64,
                            config.targets.target_active_big_ledger as u64,
                            c.known as u64,
                            c.established as u64,
                            c.active as u64,
                            c.known_big_ledger as u64,
                            c.established_big_ledger as u64,
                            c.active_big_ledger as u64,
                            c.known_local_root as u64,
                            c.established_local_root as u64,
                            c.active_local_root as u64,
                        );

                        // R223 — Phase D.2: aggregate lifetime peer
                        // stats across all known peers and publish
                        // the totals as Prometheus counters.  Updated
                        // on the same governor tick as the peer
                        // selection counters so dashboards see a
                        // consistent snapshot.
                        //
                        // R224 — refresh `bytes_in` per-peer from the
                        // BlockFetch pool's cumulative
                        // `bytes_delivered` counter before
                        // aggregating.  The pool's counter is
                        // already monotonic across reconnects (the
                        // pool registry survives session changes),
                        // so we use the cumulative-overwrite setter
                        // `set_lifetime_bytes_in` rather than the
                        // additive `record_lifetime_traffic` to
                        // avoid double-counting.
                        if let Some(pool) = config.block_fetch_pool.as_ref() {
                            if let Ok(p) = pool.lock() {
                                for (peer, state) in p.peers.iter() {
                                    governor_state.set_lifetime_bytes_in(
                                        *peer,
                                        state.bytes_delivered,
                                    );
                                }
                            }
                        }
                        // R237 — refresh `bytes_out` per-peer from the
                        // server-egress observations recorded by inbound
                        // mini-protocol responder tasks.  The source is
                        // cumulative per peer, so overwrite the lifetime
                        // total just like the bytes-in path above.
                        for (peer, bytes_out) in m.peer_lifetime_bytes_out_by_peer() {
                            governor_state.set_lifetime_bytes_out(peer, bytes_out);
                        }
                        let (
                            sessions_total,
                            failures_total,
                            bytes_in_total,
                            bytes_out_total,
                            handshakes_total,
                        ) = governor_state.lifetime_stats.values().fold(
                            (0u64, 0u64, 0u64, 0u64, 0u64),
                            |(sessions, failures, bytes_in, bytes_out, handshakes), s| {
                                    (
                                        sessions.saturating_add(s.sessions as u64),
                                        failures.saturating_add(s.failures_total as u64),
                                        bytes_in.saturating_add(s.bytes_in),
                                        bytes_out.saturating_add(s.bytes_out),
                                        handshakes
                                            .saturating_add(s.successful_handshakes as u64),
                                    )
                                },
                            );
                        m.set_peer_lifetime_sessions_total(sessions_total);
                        m.set_peer_lifetime_failures_total(failures_total);
                        m.set_peer_lifetime_bytes_in_total(bytes_in_total);
                        m.set_peer_lifetime_bytes_out_total(bytes_out_total);
                        // R226 — unique peer count + cumulative
                        // handshakes.  Cardinality of the lifetime
                        // map is a stable indicator of how many
                        // distinct addresses this process has ever
                        // observed (independent of how many times
                        // each reconnected).
                        m.set_peer_lifetime_unique_peers(
                            governor_state.lifetime_stats.len() as u64,
                        );
                        m.set_peer_lifetime_handshakes_total(handshakes_total);

                        // Update connection-manager Prometheus counters from actual CM state.
                        let cm_c = {
                            let cm = connection_manager.read().expect("cm lock poisoned");
                            cm.counters()
                        };
                        m.set_connection_manager_counters(
                            cm_c.full_duplex_conns as u64,
                            cm_c.duplex_conns as u64,
                            cm_c.unidirectional_conns as u64,
                            cm_c.inbound_conns as u64,
                            cm_c.outbound_conns as u64,
                        );
                    }

                    (actions, suppressed_bootstrap_promotions)
                };

                if suppressed_bootstrap_promotions > 0 {
                    tracer.trace_runtime(
                        "Net.Governor",
                        "Debug",
                        "suppressed outbound promotions while direct sync bootstrap is pending",
                        trace_fields([(
                            "suppressedPromotions",
                            json!(suppressed_bootstrap_promotions),
                        )]),
                    );
                }

                // Phase 6 — observe BlockFetch worker pool size each
                // tick.  Done OUTSIDE the registry-read scope so the
                // brief `tokio::sync::RwLock::read().await` doesn't
                // hold a `std::sync::RwLockReadGuard<PeerRegistry>`
                // across the await (which would break Send for the
                // governor task).
                if let Some(m) = metrics.as_ref() {
                    let workers_registered =
                        peer_manager.fetch_worker_pool.read().await.len() as u64;
                    m.set_blockfetch_workers_registered(workers_registered);
                    if let Some(cs_pool) = config.shared_chainsync_worker_pool.as_ref() {
                        let chainsync_registered = cs_pool.read().await.len() as u64;
                        m.set_chainsync_workers_registered(chainsync_registered);
                    }
                }

                if actions.is_empty() {
                    continue;
                }

                for action in actions {
                    let peer = governor_action_peer(&action);
                    let changed = if let Some(peer_state_action) =
                        governor_action_to_peer_state_action(&action)
                    {
                        match peer_state_action {
                            PeerStateAction::EstablishConnection(peer) => {
                                governor_state.mark_in_flight_warm(peer);
                                let (acquire_result, cm_actions) = {
                                    let mut cm = connection_manager
                                        .write()
                                        .expect("connection manager lock poisoned");
                                    match cm.acquire_outbound_connection(
                                        outbound_cm_local_addr(),
                                        peer,
                                    ) {
                                        Ok(result) => result,
                                        Err(err) => {
                                            tracer.trace_runtime(
                                                "Net.Governor",
                                                "Warning",
                                                "connection-manager acquire outbound failed",
                                                trace_fields([
                                                    ("peer", json!(peer.to_string())),
                                                    ("error", json!(err.to_string())),
                                                ]),
                                            );
                                            governor_state.clear_in_flight_warm(&peer);
                                            continue;
                                        }
                                    }
                                };

                                let mut changed = apply_cm_actions(
                                    &mut peer_manager,
                                    &peer_registry,
                                    &connection_manager,
                                    &mut governor_state,
                                    &node_config,
                                    cm_actions,
                                    &tracer,
                                    config.max_concurrent_block_fetch_peers,
                                metrics.as_ref(),
                                )
                                .await;

                                if matches!(acquire_result, AcquireOutboundResult::Reused(_)) {
                                    governor_state.record_success(peer);
                                    changed |= update_registry_status_from_cm(
                                        &connection_manager,
                                        &peer_registry,
                                        peer,
                                    );
                                }

                                governor_state.clear_in_flight_warm(&peer);
                                changed
                            }
                            PeerStateAction::ActivateConnection(peer) => {
                                governor_state.mark_in_flight_hot(peer);
                                if peer_manager.promote_to_hot(peer, &governor_state.hot_scheduling) {
                                    let mut registry = peer_registry
                                        .write()
                                        .expect("peer registry lock poisoned");
                                    let changed =
                                        registry.set_status(peer, PeerStatus::PeerHot);
                                    governor_state.clear_in_flight_hot(&peer);
                                    changed
                                } else {
                                    governor_state.clear_in_flight_hot(&peer);
                                    false
                                }
                            }
                            PeerStateAction::DeactivateConnection(peer) => {
                                governor_state.mark_in_flight_demote_hot(peer);
                                peer_manager.demote_to_warm(peer);
                                governor_state.clear_in_flight_demote_hot(&peer);
                                governor_state.clear_in_flight_hot(&peer);
                                let mut registry = peer_registry
                                    .write()
                                    .expect("peer registry lock poisoned");
                                registry.set_status(peer, PeerStatus::PeerWarm)
                            }
                            PeerStateAction::CloseConnection(peer) => {
                                governor_state.mark_in_flight_demote_warm(peer);

                                let (release_result, cm_actions) = {
                                    let mut cm = connection_manager
                                        .write()
                                        .expect("connection manager lock poisoned");
                                    cm.release_outbound_connection(peer)
                                };

                                let mut changed = apply_cm_actions(
                                    &mut peer_manager,
                                    &peer_registry,
                                    &connection_manager,
                                    &mut governor_state,
                                    &node_config,
                                    cm_actions,
                                    &tracer,
                                    config.max_concurrent_block_fetch_peers,
                                metrics.as_ref(),
                                )
                                .await;

                                match release_result {
                                    ReleaseOutboundResult::Error(err) => {
                                        tracer.trace_runtime(
                                            "Net.Governor",
                                            "Warning",
                                            "connection-manager release outbound failed",
                                            trace_fields([
                                                ("peer", json!(peer.to_string())),
                                                ("error", json!(err.to_string())),
                                            ]),
                                        );
                                    }
                                    ReleaseOutboundResult::DemotedToColdLocal(_)
                                    | ReleaseOutboundResult::Noop(_) => {
                                        changed |= update_registry_status_from_cm(
                                            &connection_manager,
                                            &peer_registry,
                                            peer,
                                        );
                                    }
                                }

                                governor_state.clear_in_flight_demote_warm(&peer);
                                governor_state.clear_in_flight_warm(&peer);
                                governor_state.clear_in_flight_hot(&peer);
                                changed
                            }
                        }
                    } else {
                        match action {
                            GovernorAction::ForgetPeer(peer) => {
                                governor_state.clear_in_flight_warm(&peer);
                                governor_state.clear_in_flight_hot(&peer);
                                let _ = peer_manager.demote_to_cold(peer).await;
                                {
                                    let mut cm = connection_manager
                                        .write()
                                        .expect("connection manager lock poisoned");
                                    let _ = cm.mark_terminating(
                                        peer,
                                        Some("forgotten by governor".to_owned()),
                                    );
                                    let _ = cm.time_wait_expired(peer);
                                    let _ = cm.remove_terminated(&peer);
                                }
                                let mut registry = peer_registry
                                    .write()
                                    .expect("peer registry lock poisoned");
                                registry.remove(&peer)
                            }
                            GovernorAction::ShareRequest(peer) => {
                                governor_state.mark_peer_share_sent();
                                let amount =
                                    peer_share_request_amount(&config.targets);
                                let result = if let Some(session) =
                                    peer_manager.warm_peers.get_mut(&peer)
                                {
                                    session.share_peers(amount).await
                                } else {
                                    Ok(None)
                                };
                                let changed = match result {
                                    Ok(Some(shared_peers)) => {
                                        governor_state.record_success(peer);
                                        let changed = {
                                            let mut registry = peer_registry
                                                .write()
                                                .expect("peer registry lock poisoned");
                                            registry.sync_peer_share_peers(
                                                shared_peers,
                                            )
                                        };
                                        if changed {
                                            tracer.trace_runtime(
                                                "Net.PeerSelection",
                                                "Info",
                                                "peer sharing response received",
                                                trace_fields([(
                                                    "peer",
                                                    json!(peer.to_string()),
                                                )]),
                                            );
                                        }
                                        changed
                                    }
                                    Ok(None) => false,
                                    Err(err) => {
                                        governor_state.record_failure(peer);
                                        tracer.trace_runtime(
                                            "Net.PeerSelection",
                                            "Warning",
                                            "peer sharing request failed",
                                            trace_fields([
                                                (
                                                    "peer",
                                                    json!(peer.to_string()),
                                                ),
                                                ("error", json!(err)),
                                            ]),
                                        );
                                        false
                                    }
                                };
                                governor_state.clear_peer_share_completed(1);
                                changed
                            }
                            GovernorAction::RequestPublicRoots => {
                                governor_state.mark_public_root_request_started();
                                let refresh_now = Instant::now();
                                let changed = {
                                    let mut registry = peer_registry
                                        .write()
                                        .expect("peer registry lock poisoned");
                                    let mut changed = root_sources.refresh(&mut registry, &tracer);
                                    changed |= reserve_bootstrap_sync_peers(
                                        &mut registry,
                                        std::iter::once(node_config.peer_addr)
                                            .chain(root_sources.bootstrap_peer_addrs()),
                                    );
                                    changed
                                };
                                governor_state.complete_public_root_request(
                                    refresh_now,
                                    changed,
                                    Duration::from_secs(60),
                                );
                                changed
                            }
                            GovernorAction::RequestBigLedgerPeers => {
                                governor_state.mark_big_ledger_request_started();
                                let refresh_now = Instant::now();
                                let observation = {
                                    let mut registry = peer_registry
                                        .write()
                                        .expect("peer registry lock poisoned");
                                    refresh_ledger_peer_sources_from_chain_db(
                                        &mut registry,
                                        &chain_db,
                                        &base_ledger_state,
                                        &topology,
                                        &tracer,
                                        config.ledger_judgement_settings,
                                        config.epoch_schedule,
                                    )
                                };
                                let changed = observation.update.changed;
                                governor_state.complete_big_ledger_request(
                                    refresh_now,
                                    changed,
                                    Duration::from_secs(60),
                                );
                                changed
                            }
                            GovernorAction::AdoptInboundPeer(peer) => {
                                governor_state.mark_inbound_peer_pick(Instant::now());
                                let mut registry = peer_registry
                                    .write()
                                    .expect("peer registry lock poisoned");
                                registry.insert_source(
                                    peer,
                                    PeerSource::PeerSourcePeerShare,
                                )
                            }
                            GovernorAction::PromoteToWarm(_)
                            | GovernorAction::PromoteToHot(_)
                            | GovernorAction::DemoteToWarm(_)
                            | GovernorAction::DemoteToCold(_) => false,
                        }
                    };
                    tracer.trace_runtime(
                        "Net.Governor",
                        if changed { "Info" } else { "Debug" },
                        "peer governor action applied",
                        trace_fields([
                            ("action", json!(governor_action_name(&action))),
                            (
                                "peer",
                                json!(peer.map(|p| p.to_string()).unwrap_or_else(|| "n/a".to_string())),
                            ),
                            (
                                "metricScore",
                                json!(peer
                                    .map(|p| governor_state.metrics.combined_score(&p))
                                    .unwrap_or(0)),
                            ),
                            ("changed", json!(changed)),
                        ]),
                    );
                }
            }
        }
    }
}
