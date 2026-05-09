//! Outbound peer management — runtime-side structures and helpers.
//!
//! Mirrors the runtime-side glue around upstream
//! `Ouroboros.Network.PeerSelection.PeerStateActions` (per-peer warm/hot
//! state machine), `Ouroboros.Network.RootPeers` (root peer source
//! providers), and `Ouroboros.Network.BlockFetch.ClientRegistry`
//! (per-peer fetch-worker pool).
//!
//! Three structs anchor this module:
//!
//! - `ManagedWarmPeer` — runtime-side per-peer warm state (mux session,
//!   keepalive cookie counter, hot/warm flag, last-known tip,
//!   per-protocol temperature control bundle).
//! - `OutboundPeerManager` — the governor-side cluster of warm peers
//!   plus the shared per-peer BlockFetch worker pool.
//! - `RuntimeRootPeerSources` — the runtime-side bundle of DNS root
//!   peer providers (local roots, bootstrap peers, public-config peers,
//!   ledger-peer source) plus the shared `RootPeerProviderState` that
//!   merges them.
//!
//! Plus ~25 helper fns covering mux temperature control bundles,
//! per-protocol weight application, peer-share request sizing,
//! reconnect storage tip selection, preferred-hot-peer handoff,
//! reconnect attempt-state ordering, and ledger-peer snapshot
//! derivation.
//!
//! Extracted from `runtime.rs` in R271n (Phase γ §R271 fourteenth slice).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side runtime structures for
//! outbound peer management. Mirrors glue around upstream
//! `Ouroboros.Network.PeerSelection.PeerStateActions` (per-peer
//! warm/hot state), `Ouroboros.Network.RootPeers` (root peer
//! providers), and `Ouroboros.Network.BlockFetch.ClientRegistry`
//! (per-peer fetch-worker pool). Upstream wires these inline in
//! the diffusion config; Yggdrasil isolates the runtime-side
//! structures here for testability.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde_json::json;

use yggdrasil_ledger::{LedgerState, Point, PoolRelayAccessPoint};
use yggdrasil_network::{
    BlockFetchClient, ControlMessage, DnsRefreshPolicy, DnsRootPeerProvider, GovernorState,
    GovernorTargets, LedgerPeerSnapshot, LocalRootConfig, LocalRootTargets, MiniProtocolNum,
    PeerAccessPoint, PeerAttemptState, PeerRegistry, PeerSource, PeerStatus, RootPeerProviderState,
    TemperatureBundle, TopologyConfig, peer_attempt_state, refresh_root_peer_state_and_registry,
    resolve_peer_access_points,
};

use crate::sync::SyncError;
use crate::tracer::{NodeTracer, trace_fields};

use super::bootstrap::bootstrap;
use super::peer_session::{NodeConfig, PeerSession};

pub(super) struct ManagedWarmPeer {
    pub(super) session: PeerSession,
    pub(super) last_keepalive_at: Instant,
    pub(super) next_cookie: u16,
    /// When `true` the peer is considered hot (active data exchange candidate).
    pub(super) is_hot: bool,
    /// Most recently observed chain tip from this peer, used for chain
    /// selection among hot peers.
    pub(super) last_known_tip: Option<Point>,
    /// Runtime-side temperature control state for this peer's mini-protocols.
    pub(super) control: TemperatureBundle<ControlMessage>,
}

fn control_bundle_cold_to_warm() -> TemperatureBundle<ControlMessage> {
    TemperatureBundle {
        hot: ControlMessage::Quiesce,
        warm: ControlMessage::Continue,
        established: ControlMessage::Continue,
    }
}

fn apply_control_activate(bundle: &mut TemperatureBundle<ControlMessage>) {
    bundle.warm = ControlMessage::Quiesce;
    bundle.hot = ControlMessage::Continue;
}

fn apply_control_deactivate(bundle: &mut TemperatureBundle<ControlMessage>) {
    bundle.hot = ControlMessage::Quiesce;
    bundle.warm = ControlMessage::Continue;
}

pub(super) fn apply_control_close(bundle: &mut TemperatureBundle<ControlMessage>) {
    bundle.hot = ControlMessage::Terminate;
    bundle.warm = ControlMessage::Terminate;
    bundle.established = ControlMessage::Terminate;
}

/// Hot-tier egress weights: ChainSync 3, BlockFetch 2, others 1.
///
/// Upstream: `hotProtocol` scheduling allocates proportionally more egress
/// bandwidth to data-intensive mini-protocols when a peer is hot, reducing
/// fetch latency and improving chain sync throughput.
///
/// Slice D-Scheduler — these weights are now sourced from the governor's
/// [`HotPeerScheduling`](yggdrasil_network::HotPeerScheduling) table
/// rather than hardcoded here, so operators can tune the per-protocol
/// share via `set_hot_protocol_weight` without touching this file. The
/// upstream-default share comes from `defaultMiniProtocolParameters`
/// (BlockFetch=10, ChainSync=3, TxSubmission=2, KeepAlive=1, PeerSharing=1).
fn apply_hot_weights(
    weights: &[(MiniProtocolNum, yggdrasil_network::WeightHandle)],
    scheduling: &yggdrasil_network::HotPeerScheduling,
) {
    for (proto, handle) in weights {
        // `WeightHandle::set` floor-clamps to 1, but make the intent
        // explicit here: a `0` weight in the scheduling table means
        // "disable from scheduler share" and we don't want a zero
        // round count to starve a protocol entirely.
        let w = scheduling.hot_protocol_weight(*proto).max(1);
        handle.set(w);
    }
}

fn apply_warm_weights(weights: &[(MiniProtocolNum, yggdrasil_network::WeightHandle)]) {
    for (_proto, handle) in weights {
        handle.set(yggdrasil_network::DEFAULT_PROTOCOL_WEIGHT);
    }
}

impl ManagedWarmPeer {
    pub(super) fn new(session: PeerSession, now: Instant) -> Self {
        Self {
            session,
            last_keepalive_at: now,
            next_cookie: 1,
            is_hot: false,
            last_known_tip: None,
            control: control_bundle_cold_to_warm(),
        }
    }

    pub(super) async fn maybe_send_keepalive(
        &mut self,
        interval: Duration,
        now: Instant,
    ) -> Result<bool, SyncError> {
        if now.duration_since(self.last_keepalive_at) < interval {
            return Ok(false);
        }

        self.session.keep_alive.keep_alive(self.next_cookie).await?;
        self.next_cookie = self.next_cookie.wrapping_add(1);
        self.last_keepalive_at = now;
        Ok(true)
    }

    pub(super) fn abort(self) {
        self.session.mux.abort();
    }

    pub(super) async fn share_peers(
        &mut self,
        amount: u16,
    ) -> Result<Option<Vec<SocketAddr>>, String> {
        let Some(peer_sharing) = self.session.peer_sharing.as_mut() else {
            return Ok(None);
        };

        peer_sharing
            .share_request(amount)
            .await
            .map(|peers| Some(peers.into_iter().map(|peer| peer.addr).collect()))
            .map_err(|err| err.to_string())
    }
}

/// Shared per-peer BlockFetch worker pool reachable from both the
/// governor task (writer: register on promote, unregister on
/// demote) and the sync-loop task (reader: dispatch fetch plans).
///
/// Wraps a [`crate::blockfetch_worker::FetchWorkerPool`] in
/// `Arc<tokio::sync::RwLock<>>` so multiple readers can dispatch
/// concurrently while writes (register / unregister) take a brief
/// exclusive lock.  Mirrors upstream
/// `Ouroboros.Network.BlockFetch.ClientRegistry` shared across the
/// fetch-decision policy thread and the per-peer fetch threads via
/// STM.
pub type SharedFetchWorkerPool = std::sync::Arc<
    tokio::sync::RwLock<crate::blockfetch_worker::FetchWorkerPool<crate::sync::MultiEraBlock>>,
>;

/// Construct a fresh shared fetch-worker pool.  Cloning the returned
/// `Arc` is cheap; both governor and sync-loop configs hold their
/// own clones.
pub fn new_shared_fetch_worker_pool() -> SharedFetchWorkerPool {
    std::sync::Arc::new(tokio::sync::RwLock::new(
        crate::blockfetch_worker::FetchWorkerPool::new(),
    ))
}

pub(super) struct OutboundPeerManager {
    pub(super) warm_peers: BTreeMap<SocketAddr, ManagedWarmPeer>,
    /// Per-peer BlockFetch workers populated when the operator opts
    /// into multi-peer dispatch via
    /// `max_concurrent_block_fetch_peers > 1`.  Empty by default —
    /// the legacy single-peer path uses `session.block_fetch`
    /// directly.  When non-empty, the sync loop's multi-peer branch
    /// dispatches through this pool.
    ///
    /// The pool is shared (Arc<tokio::sync::RwLock<>>) so the sync
    /// loop in a separate task can read it for dispatch while the
    /// governor task writes to it on promote/demote.  Constructed
    /// at runtime startup via
    /// [`new_shared_fetch_worker_pool`] and cloned into both the
    /// governor's [`OutboundPeerManager`] and the sync service's
    /// [`crate::sync::VerifiedSyncServiceConfig::shared_fetch_worker_pool`].
    ///
    /// Populated by [`OutboundPeerManager::migrate_session_to_worker`]
    /// at promote time; entries are removed by
    /// [`OutboundPeerManager::unregister_worker`] on disconnect.
    ///
    /// Mirrors upstream
    /// `Ouroboros.Network.BlockFetch.ClientRegistry` per-peer
    /// `FetchClientStateVars` map shared across threads via STM.
    pub(super) fetch_worker_pool: SharedFetchWorkerPool,
}

pub(super) struct RuntimeRootPeerSources {
    state: RootPeerProviderState,
    local_roots: Option<DnsRootPeerProvider>,
    bootstrap_peers: Option<DnsRootPeerProvider>,
    public_config_peers: Option<DnsRootPeerProvider>,
}

impl OutboundPeerManager {
    pub(super) fn new() -> Self {
        Self::with_fetch_worker_pool(new_shared_fetch_worker_pool())
    }

    /// Construct an `OutboundPeerManager` that shares its fetch
    /// worker pool with another runtime task (typically the sync
    /// loop).  Use [`new_shared_fetch_worker_pool`] to construct
    /// the shared pool at runtime startup, then clone the `Arc`
    /// into the sync service config.
    pub(super) fn with_fetch_worker_pool(pool: SharedFetchWorkerPool) -> Self {
        Self {
            warm_peers: BTreeMap::new(),
            fetch_worker_pool: pool,
        }
    }

    /// Migrate the session's BlockFetch handle into a per-peer fetch
    /// worker registered in the shared [`Self::fetch_worker_pool`].
    /// Returns `true` on first migration, `false` if the peer is
    /// unknown or the handle has already been migrated.
    ///
    /// The runtime calls this after promote-to-warm when operating
    /// in multi-peer dispatch mode (knob > 1).  Once migrated, the
    /// session's `block_fetch` field is `None` and the sync loop
    /// reaches the peer through `fetch_worker_pool.dispatch_plan(...)`.
    ///
    /// Mirrors upstream `bracketSyncWithFetchClient` lifecycle: the
    /// per-peer fetch state is owned by a dedicated thread/task for
    /// the connection's lifetime.
    #[allow(dead_code)] // Phase 6 scaffolding — runtime branch caller pending.
    pub(super) async fn migrate_session_to_worker(&mut self, peer: SocketAddr) -> bool {
        let Some(managed) = self.warm_peers.get_mut(&peer) else {
            return false;
        };
        let Some(block_fetch) = managed.session.take_block_fetch() else {
            return false;
        };
        let handle = crate::blockfetch_worker::FetchWorkerHandle::spawn_with_block_fetch_client(
            peer,
            block_fetch,
        );
        // Replace any stale handle (e.g. from a prior session that was
        // not cleanly unregistered).  The previous handle's drop
        // closes its channel and exits its task.  Brief write-lock
        // hold; readers (sync-loop dispatchers) may transiently wait.
        let _previous = self.fetch_worker_pool.write().await.register(handle);
        true
    }

    /// Remove and shut down the worker for `peer` (graceful exit).
    /// Returns `true` if a worker was registered.  Used when a peer
    /// disconnects: the runtime calls this before dropping the
    /// session so the worker task exits cleanly without affecting
    /// siblings.
    #[allow(dead_code)] // Phase 6 scaffolding — runtime branch caller pending.
    pub(super) async fn unregister_worker(&mut self, peer: &SocketAddr) -> bool {
        match self.fetch_worker_pool.write().await.unregister(peer) {
            Some(handle) => {
                drop(handle);
                true
            }
            None => false,
        }
    }

    /// Clone of the shared fetch-worker pool handle.  Runtime
    /// startup calls this once and threads the clone into
    /// [`crate::sync::VerifiedSyncServiceConfig::shared_fetch_worker_pool`]
    /// so the sync loop can dispatch through the same pool the
    /// governor populates.
    #[allow(dead_code)] // Phase 6 scaffolding — runtime startup wiring pending.
    pub(super) fn shared_fetch_worker_pool(&self) -> SharedFetchWorkerPool {
        self.fetch_worker_pool.clone()
    }

    pub(super) async fn promote_to_warm(
        &mut self,
        node_config: &NodeConfig,
        peer: SocketAddr,
        governor_state: &mut GovernorState,
        tracer: &NodeTracer,
    ) -> bool {
        if self.warm_peers.contains_key(&peer) {
            return false;
        }

        let peer_config = NodeConfig {
            peer_addr: peer,
            network_magic: node_config.network_magic,
            protocol_versions: node_config.protocol_versions.clone(),
            peer_sharing: node_config.peer_sharing,
        };

        match bootstrap(&peer_config).await {
            Ok(session) => {
                let connected_peer_addr = session.connected_peer_addr;
                self.warm_peers
                    .insert(peer, ManagedWarmPeer::new(session, Instant::now()));
                governor_state.record_success(peer);
                // R223 — Phase D.2: bump the lifetime-stats session
                // counters when a peer successfully transitions to
                // warm.  Distinct from `record_success` above which
                // resets the session-keyed `failures` map; this
                // accumulates monotonically across reconnects so
                // dashboards can distinguish "first contact" from
                // "5th reconnect this hour" (peer churn).
                governor_state.record_lifetime_session_started(peer);
                tracer.trace_runtime(
                    "Net.Governor",
                    "Info",
                    "warm peer connection established",
                    trace_fields([
                        ("peer", json!(peer.to_string())),
                        ("connectedPeer", json!(connected_peer_addr.to_string())),
                    ]),
                );
                true
            }
            Err(err) => {
                governor_state.record_failure(peer);
                // R223 — Phase D.2: bump the lifetime-stats failure
                // counter alongside the session-keyed one.
                governor_state.record_lifetime_session_failure(peer);
                tracer.trace_runtime(
                    "Net.Governor",
                    "Warning",
                    "warm peer connection failed",
                    trace_fields([
                        ("peer", json!(peer.to_string())),
                        ("error", json!(err.to_string())),
                    ]),
                );
                false
            }
        }
    }

    pub(super) async fn demote_to_cold(&mut self, peer: SocketAddr) -> bool {
        match self.warm_peers.remove(&peer) {
            Some(mut session) => {
                apply_control_close(&mut session.control);
                session.abort();
                // Unregister any per-peer fetch worker so the per-peer
                // task exits cleanly without affecting siblings.  No-op
                // when no worker was migrated for this peer.  Mirrors
                // upstream `bracketSyncWithFetchClient` exit path.
                let _ = self.fetch_worker_pool.write().await.unregister(&peer);
                true
            }
            None => false,
        }
    }

    /// Mark a warm peer as hot (active data exchange candidate).
    ///
    /// Returns `true` when the peer was found and its status changed.
    /// The underlying session remains alive so the peer continues to
    /// receive KeepAlive heartbeats while hot.
    /// Borrow the per-peer `BlockFetchClient` for every currently-hot
    /// peer, sliced as `&mut [(SocketAddr, &mut BlockFetchClient)]`.
    ///
    /// This is the runtime seam (Phase 6 of `docs/ARCHITECTURE.md`)
    /// that exposes hot peers' BlockFetch handles to the sync loop's
    /// multi-peer dispatcher (`crate::sync::dispatch_range_with_tentative`).
    /// The closure-style API keeps borrow checking in the manager —
    /// no `Arc<Mutex<BlockFetchClient>>` wrapper is required because
    /// the closure runs synchronously while the mutable borrow is
    /// held.
    ///
    /// Returns the closure's output.  When no peers are currently hot,
    /// the closure receives an empty slice — callers should treat
    /// that as "fall back to single-peer dispatch via the leader
    /// session".
    ///
    /// Reference: upstream `Ouroboros.Network.BlockFetch.ClientRegistry`
    /// holds long-lived per-peer `FetchClientStateVars` shared with the
    /// fetch-decision policy via `TVar`; this accessor is the Rust-side
    /// borrow-checked equivalent for the synchronous schedule step.
    #[allow(dead_code)] // Phase 6 scaffolding — sync-loop consumer pending.
    pub(super) fn with_hot_block_fetch_clients<R>(
        &mut self,
        f: impl FnOnce(&mut [(SocketAddr, &mut BlockFetchClient)]) -> R,
    ) -> R {
        // Collect (addr, &mut BlockFetchClient) pairs for every hot
        // peer.  Iteration order follows `BTreeMap`'s sort by
        // `SocketAddr`, so the resulting slice is deterministic across
        // ticks — matches the upstream invariant that the scheduler
        // sees peers in a stable order.
        let mut handles: Vec<(SocketAddr, &mut BlockFetchClient)> = self
            .warm_peers
            .iter_mut()
            .filter(|(_, m)| m.is_hot)
            .map(|(addr, m)| {
                (
                    *addr,
                    m.session
                        .block_fetch
                        .as_mut()
                        .expect("block_fetch migrated"),
                )
            })
            .collect();
        f(&mut handles)
    }

    /// Return the list of currently-hot peer addresses.  Cheap snapshot
    /// used by the sync loop to size the dispatcher's effective
    /// concurrency without holding a `&mut` borrow on the manager.
    #[allow(dead_code)] // Phase 6 scaffolding — sync-loop consumer pending.
    pub(super) fn hot_peer_addrs(&self) -> Vec<SocketAddr> {
        self.warm_peers
            .iter()
            .filter(|(_, m)| m.is_hot)
            .map(|(addr, _)| *addr)
            .collect()
    }

    pub(super) fn promote_to_hot(
        &mut self,
        peer: SocketAddr,
        scheduling: &yggdrasil_network::HotPeerScheduling,
    ) -> bool {
        match self.warm_peers.get_mut(&peer) {
            Some(managed) if !managed.is_hot => {
                managed.is_hot = true;
                apply_control_activate(&mut managed.control);
                // Boost hot-tier protocol weights from the governor's
                // `HotPeerScheduling` table so per-protocol egress share
                // matches upstream `defaultMiniProtocolParameters`
                // (BlockFetch=10, ChainSync=3, TxSubmission=2, etc.).
                apply_hot_weights(&managed.session.protocol_weights, scheduling);
                true
            }
            _ => false,
        }
    }

    /// Demote a hot peer back to warm.
    ///
    /// Returns `true` when the peer was found and its `is_hot` flag cleared.
    pub(super) fn demote_to_warm(&mut self, peer: SocketAddr) -> bool {
        match self.warm_peers.get_mut(&peer) {
            Some(managed) if managed.is_hot => {
                managed.is_hot = false;
                apply_control_deactivate(&mut managed.control);
                // Reset all protocol weights to uniform when demoted.
                apply_warm_weights(&managed.session.protocol_weights);
                true
            }
            _ => false,
        }
    }

    /// Query the chain tip of each hot peer via ChainSync `find_intersect`
    /// and update the cached `last_known_tip`.
    ///
    /// Uses Origin as the sole candidate point so the server always returns
    /// its current tip without advancing the cursor.  Peers that fail the
    /// query are left with their previous tip value and tracked as failures.
    ///
    /// Reference: upstream `peerSelectionGovernor` refreshes candidate tips
    /// periodically.
    pub(super) async fn refresh_hot_peer_tips(
        &mut self,
        peer_registry: &Arc<RwLock<PeerRegistry>>,
        governor_state: &mut GovernorState,
        tracer: &NodeTracer,
    ) -> Vec<SocketAddr> {
        let hot_peers: Vec<SocketAddr> = self
            .warm_peers
            .iter()
            .filter(|(_, m)| m.is_hot)
            .map(|(addr, _)| *addr)
            .collect();
        let mut failed_peers = Vec::new();

        for peer in hot_peers {
            let Some(managed) = self.warm_peers.get_mut(&peer) else {
                continue;
            };
            match managed
                .session
                .chain_sync
                .find_intersect_points(vec![Point::Origin])
                .await
            {
                Ok(resp) => {
                    let tip = match &resp {
                        yggdrasil_network::TypedIntersectResponse::Found { tip, .. } => *tip,
                        yggdrasil_network::TypedIntersectResponse::NotFound { tip } => *tip,
                    };
                    if let Some(slot) = tip.slot() {
                        governor_state.metrics.record_upstreamyness(peer, slot.0);
                    }
                    if let Ok(mut registry) = peer_registry.write() {
                        let _ = registry.set_hot_tip_slot(peer, tip.slot().map(|slot| slot.0));
                    }
                    tracer.trace_runtime(
                        "Net.Governor",
                        "Debug",
                        "hot peer tip refreshed",
                        trace_fields([
                            ("peer", json!(peer.to_string())),
                            ("tip", json!(format!("{:?}", tip))),
                        ]),
                    );
                    managed.last_known_tip = Some(tip);
                }
                Err(err) => {
                    governor_state.record_failure(peer);
                    failed_peers.push(peer);
                    if let Ok(mut registry) = peer_registry.write() {
                        let _ = registry.set_hot_tip_slot(peer, None);
                    }
                    tracer.trace_runtime(
                        "Net.Governor",
                        "Warning",
                        "hot peer tip query failed",
                        trace_fields([
                            ("peer", json!(peer.to_string())),
                            ("error", json!(err.to_string())),
                        ]),
                    );
                }
            }
        }

        failed_peers
    }

    /// Select the best hot peer to sync from based on its last known tip.
    ///
    /// Returns the address of the hot peer with the highest block number
    /// at its reported tip (most advanced chain), or `None` if no hot
    /// peers have a known tip.
    ///
    /// Reference: upstream chain selection picks the peer whose candidate
    /// chain header is best according to `selectView`.
    pub(super) fn best_hot_peer(&self) -> Option<SocketAddr> {
        self.warm_peers
            .iter()
            .filter(|(_, m)| m.is_hot && m.last_known_tip.is_some())
            .max_by_key(|(_, m)| m.last_known_tip.as_ref().and_then(|tip| tip.slot()))
            .map(|(addr, _)| *addr)
    }

    pub(super) async fn drive_keepalives(
        &mut self,
        keepalive_interval: Option<Duration>,
        governor_state: &mut GovernorState,
        tracer: &NodeTracer,
    ) -> Vec<SocketAddr> {
        let Some(interval) = keepalive_interval else {
            return Vec::new();
        };

        let now = Instant::now();
        let peers = self.warm_peers.keys().copied().collect::<Vec<_>>();
        let mut failed = Vec::new();

        for peer in peers {
            let Some(session) = self.warm_peers.get_mut(&peer) else {
                continue;
            };

            match session.maybe_send_keepalive(interval, now).await {
                Ok(sent) => {
                    if sent {
                        governor_state.record_success(peer);
                    }
                }
                Err(err) => {
                    governor_state.record_failure(peer);
                    failed.push((peer, err));
                }
            }
        }

        let mut failed_peers = Vec::new();
        for (peer, err) in failed {
            let _ = self.demote_to_cold(peer).await;
            failed_peers.push(peer);
            tracer.trace_runtime(
                "Net.Governor",
                "Warning",
                "warm peer keepalive failed",
                trace_fields([
                    ("peer", json!(peer.to_string())),
                    ("error", json!(err.to_string())),
                ]),
            );
        }

        failed_peers
    }
}

pub(super) fn peer_share_request_amount(targets: &GovernorTargets) -> u16 {
    targets.target_known.clamp(1, u16::MAX as usize) as u16
}

impl RuntimeRootPeerSources {
    pub(super) fn new(topology: &TopologyConfig) -> Self {
        let policy = DnsRefreshPolicy::default();
        let local_roots = (!topology.local_roots.is_empty()).then(|| {
            DnsRootPeerProvider::local_roots(topology.local_roots.clone())
                .with_policy(policy.clone())
        });
        let bootstrap_peers =
            (!topology.bootstrap_peers.configured_peers().is_empty()).then(|| {
                DnsRootPeerProvider::bootstrap_peers(
                    topology.bootstrap_peers.configured_peers().to_vec(),
                )
                .with_policy(policy.clone())
            });
        let public_config_peers = (!topology.public_roots.is_empty()).then(|| {
            DnsRootPeerProvider::public_config_peers(topology.public_roots.clone())
                .with_policy(policy)
        });

        Self {
            state: RootPeerProviderState::from_topology(topology),
            local_roots,
            bootstrap_peers,
            public_config_peers,
        }
    }

    pub(super) fn sync_registry(&self, registry: &mut PeerRegistry) -> bool {
        registry.sync_root_peers(self.state.providers())
    }

    pub(super) fn bootstrap_peer_addrs(&self) -> Vec<SocketAddr> {
        self.state.providers().public_roots.bootstrap_peers.clone()
    }

    pub(super) fn local_root_targets(&self) -> Vec<LocalRootTargets> {
        local_root_targets_from_resolved_groups(&self.state.providers().local_roots)
    }

    pub(super) fn refresh(&mut self, registry: &mut PeerRegistry, tracer: &NodeTracer) -> bool {
        let mut changed = false;

        if let Some(provider) = &mut self.local_roots {
            match refresh_root_peer_state_and_registry(&mut self.state, registry, provider) {
                Ok(provider_changed) => changed |= provider_changed,
                Err(err) => trace_root_refresh_error(tracer, "LocalRoots", err.to_string()),
            }
        }

        if let Some(provider) = &mut self.bootstrap_peers {
            match refresh_root_peer_state_and_registry(&mut self.state, registry, provider) {
                Ok(provider_changed) => changed |= provider_changed,
                Err(err) => trace_root_refresh_error(tracer, "BootstrapPeers", err.to_string()),
            }
        }

        if let Some(provider) = &mut self.public_config_peers {
            match refresh_root_peer_state_and_registry(&mut self.state, registry, provider) {
                Ok(provider_changed) => changed |= provider_changed,
                Err(err) => trace_root_refresh_error(tracer, "PublicConfigPeers", err.to_string()),
            }
        }

        changed
    }
}

fn trace_root_refresh_error(tracer: &NodeTracer, source: &str, error: String) {
    tracer.trace_runtime(
        "Net.PeerSelection",
        "Warning",
        "root peer refresh failed",
        trace_fields([("source", json!(source)), ("error", json!(error))]),
    );
}

/// Seed a peer registry from the primary peer and current topology-owned root sources.
pub fn seed_peer_registry(primary_peer: SocketAddr, topology: &TopologyConfig) -> PeerRegistry {
    let mut registry = PeerRegistry::default();
    let root_providers = topology.resolved_root_providers();
    registry.sync_root_peers(&root_providers);
    // Insert the primary peer after syncing root peers so that sync_root_peers
    // (which clears all Bootstrap/LocalRoot/PublicRoot sources first) does not
    // remove the primary peer's Bootstrap source when the primary is not listed
    // in the topology bootstrap set.
    registry.insert_source(primary_peer, PeerSource::PeerSourceBootstrap);
    reserve_bootstrap_sync_peers(
        &mut registry,
        std::iter::once(primary_peer).chain(root_providers.public_roots.bootstrap_peers),
    );
    registry
}

pub(super) fn reserve_bootstrap_sync_peers(
    registry: &mut PeerRegistry,
    peers: impl IntoIterator<Item = SocketAddr>,
) -> bool {
    let mut changed = false;

    for peer in peers {
        changed |= registry.insert_source(peer, PeerSource::PeerSourceBootstrap);
        if registry
            .get(&peer)
            .is_some_and(|entry| entry.status == PeerStatus::PeerCold)
        {
            changed |= registry.set_status(peer, PeerStatus::PeerCooling);
        }
    }

    changed
}

pub(super) fn registry_reserve_bootstrap_attempt_peers(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    peers: impl IntoIterator<Item = SocketAddr>,
) {
    if let Some(reg) = peer_registry {
        if let Ok(mut guard) = reg.write() {
            let _ = reserve_bootstrap_sync_peers(&mut guard, peers);
        }
    }
}

pub(super) fn reconnect_storage_tip(volatile_tip: Point, best_tip: Point) -> Point {
    if volatile_tip == Point::Origin && best_tip != Point::Origin {
        best_tip
    } else {
        volatile_tip
    }
}

/// Derive local-root governor targets from resolved topology groups.
pub fn local_root_targets_from_config(local_roots: &[LocalRootConfig]) -> Vec<LocalRootTargets> {
    local_roots
        .iter()
        .filter_map(|group| {
            let peers = group
                .access_points
                .iter()
                .flat_map(resolve_peer_access_points)
                .collect::<Vec<_>>();
            if peers.is_empty() {
                None
            } else {
                Some(LocalRootTargets::from_config(group, peers))
            }
        })
        .collect()
}

fn local_root_targets_from_resolved_groups(
    local_roots: &[yggdrasil_network::ResolvedLocalRootGroup],
) -> Vec<LocalRootTargets> {
    local_roots
        .iter()
        .map(|group| LocalRootTargets {
            peers: group.peers.clone(),
            hot_valency: group.hot_valency,
            warm_valency: group.warm_valency,
            trustable: group.trustable,
        })
        .collect()
}

pub(super) fn point_slot(point: &Point) -> Option<u64> {
    match point {
        Point::Origin => None,
        Point::BlockPoint(slot, _) => Some(slot.0),
    }
}

pub(super) fn preferred_hot_peer_from_registry(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
) -> Option<SocketAddr> {
    let registry_lock = peer_registry?;
    let registry = registry_lock.read().ok()?;
    registry.preferred_hot_peer()
}

pub(super) fn preferred_hot_peer_handoff_target(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    current_peer: SocketAddr,
) -> Option<SocketAddr> {
    let registry_lock = peer_registry?;
    let registry = registry_lock.read().ok()?;
    let preferred = registry.preferred_hot_peer()?;
    if preferred == current_peer {
        return None;
    }

    let preferred_tip = registry.hot_tip_slot(preferred);
    let current_tip = registry.hot_tip_slot(current_peer);
    match (preferred_tip, current_tip) {
        (Some(preferred_slot), Some(current_slot)) if preferred_slot > current_slot => {
            Some(preferred)
        }
        (Some(_), None) => Some(preferred),
        _ => None,
    }
}

pub(super) fn reconnect_preferred_peer_with_source(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    previous_preferred_peer: Option<SocketAddr>,
) -> Option<(SocketAddr, &'static str)> {
    preferred_hot_peer_from_registry(peer_registry)
        .map(|peer| (peer, "hot"))
        .or(previous_preferred_peer.map(|peer| (peer, "previous")))
}

pub(super) fn ordered_reconnect_fallback_peers(
    primary_peer: SocketAddr,
    refreshed_fallback_peers: &[SocketAddr],
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
) -> Vec<SocketAddr> {
    let mut ordered = Vec::new();

    if let Some(registry_lock) = peer_registry {
        if let Ok(registry) = registry_lock.read() {
            for peer in registry.hot_peers_by_reconnect_priority() {
                if peer != primary_peer
                    && refreshed_fallback_peers.contains(&peer)
                    && !ordered.contains(&peer)
                {
                    ordered.push(peer);
                }
            }
        }
    }

    for peer in refreshed_fallback_peers {
        if *peer != primary_peer && !ordered.contains(peer) {
            ordered.push(*peer);
        }
    }

    ordered
}

pub(super) fn prepare_reconnect_attempt_state(
    primary_peer: SocketAddr,
    refreshed_fallback_peers: &[SocketAddr],
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    previous_preferred_peer: Option<SocketAddr>,
) -> (PeerAttemptState, Option<(SocketAddr, &'static str)>) {
    let reconnect_preference =
        reconnect_preferred_peer_with_source(peer_registry, previous_preferred_peer);
    let ordered_fallback_peers =
        ordered_reconnect_fallback_peers(primary_peer, refreshed_fallback_peers, peer_registry);
    let mut attempt_state = peer_attempt_state(primary_peer, &ordered_fallback_peers);
    if let Some((peer_addr, _)) = reconnect_preference {
        attempt_state.record_success(peer_addr);
    }

    (attempt_state, reconnect_preference)
}

#[cfg(test)]
pub(super) fn reconnect_preferred_peer(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    previous_preferred_peer: Option<SocketAddr>,
) -> Option<SocketAddr> {
    reconnect_preferred_peer_with_source(peer_registry, previous_preferred_peer)
        .map(|(peer, _)| peer)
}

fn extend_unique_peers(target: &mut Vec<SocketAddr>, peers: impl IntoIterator<Item = SocketAddr>) {
    for peer in peers {
        if !target.contains(&peer) {
            target.push(peer);
        }
    }
}

fn extend_unique_ledger_peers(
    target: &mut Vec<SocketAddr>,
    access_points: impl IntoIterator<Item = PoolRelayAccessPoint>,
) {
    for access_point in access_points {
        let peer_access_point = PeerAccessPoint {
            address: access_point.address,
            port: access_point.port,
        };
        extend_unique_peers(target, resolve_peer_access_points(&peer_access_point));
    }
}

pub(super) fn ledger_peer_snapshot_from_ledger_state(
    ledger_state: &LedgerState,
) -> LedgerPeerSnapshot {
    let mut ledger_peers = Vec::new();
    extend_unique_ledger_peers(
        &mut ledger_peers,
        ledger_state.pool_state().relay_access_points(),
    );
    LedgerPeerSnapshot::new(ledger_peers, Vec::new())
}
