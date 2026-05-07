//! Node runtime — wires networking, storage, and protocol client drivers
//! into a cohesive sync lifecycle.
//!
//! Reference: `cardano-node/src/Cardano/Node/Run.hs`.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

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
use crate::tracer::{NodeMetrics, NodeTracer, trace_fields};
use serde_json::Value;
use serde_json::json;
use yggdrasil_consensus::mempool::{MEMPOOL_ZERO_IDX, MempoolEntry, SharedMempool};
use yggdrasil_consensus::{ChainState, EpochSchedule, SecurityParam, kes_period_of_slot};
use yggdrasil_ledger::{
    BlockNo, Decoder, EpochBoundaryEvent, HeaderHash, LedgerState, Point, PoolRelayAccessPoint,
    SlotNo,
};
use yggdrasil_network::{
    AbstractState, AfterSlot, BlockFetchClient, ChainSyncClient, CmAction, ConnectionManagerState,
    ConsensusLedgerPeerInputs, ConsensusLedgerPeerSource, ControlMessage, DataFlow,
    DnsRefreshPolicy, DnsRootPeerProvider, GovernorAction, GovernorState, GovernorTargets,
    LedgerPeerSnapshot, LedgerPeerUseDecision, LedgerStateJudgement,
    LiveLedgerPeerRefreshObservation, LocalRootConfig, LocalRootTargets, MiniProtocolNum,
    NodeToNodeVersionData, PeerAccessPoint, PeerAttemptState, PeerRegistry,
    PeerSnapshotFileObservation, PeerSnapshotFileSource, PeerSnapshotFreshness, PeerSource,
    PeerStatus, RootPeerProviderState, TemperatureBundle, TopologyConfig, UseLedgerPeers,
    always_eligible_snapshot_peers, derive_peer_snapshot_freshness,
    eligible_ledger_peer_candidates, live_refresh_ledger_peer_registry_observed,
    merge_ledger_peer_snapshots, peer_attempt_state, refresh_root_peer_state_and_registry,
    resolve_peer_access_points,
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

struct ManagedWarmPeer {
    session: PeerSession,
    last_keepalive_at: Instant,
    next_cookie: u16,
    /// When `true` the peer is considered hot (active data exchange candidate).
    is_hot: bool,
    /// Most recently observed chain tip from this peer, used for chain
    /// selection among hot peers.
    last_known_tip: Option<Point>,
    /// Runtime-side temperature control state for this peer's mini-protocols.
    control: TemperatureBundle<ControlMessage>,
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

fn apply_control_close(bundle: &mut TemperatureBundle<ControlMessage>) {
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
    fn new(session: PeerSession, now: Instant) -> Self {
        Self {
            session,
            last_keepalive_at: now,
            next_cookie: 1,
            is_hot: false,
            last_known_tip: None,
            control: control_bundle_cold_to_warm(),
        }
    }

    async fn maybe_send_keepalive(
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

    fn abort(self) {
        self.session.mux.abort();
    }

    async fn share_peers(&mut self, amount: u16) -> Result<Option<Vec<SocketAddr>>, String> {
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

struct OutboundPeerManager {
    warm_peers: BTreeMap<SocketAddr, ManagedWarmPeer>,
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
    fetch_worker_pool: SharedFetchWorkerPool,
}

struct RuntimeRootPeerSources {
    state: RootPeerProviderState,
    local_roots: Option<DnsRootPeerProvider>,
    bootstrap_peers: Option<DnsRootPeerProvider>,
    public_config_peers: Option<DnsRootPeerProvider>,
}

impl OutboundPeerManager {
    fn new() -> Self {
        Self::with_fetch_worker_pool(new_shared_fetch_worker_pool())
    }

    /// Construct an `OutboundPeerManager` that shares its fetch
    /// worker pool with another runtime task (typically the sync
    /// loop).  Use [`new_shared_fetch_worker_pool`] to construct
    /// the shared pool at runtime startup, then clone the `Arc`
    /// into the sync service config.
    fn with_fetch_worker_pool(pool: SharedFetchWorkerPool) -> Self {
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
    async fn migrate_session_to_worker(&mut self, peer: SocketAddr) -> bool {
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
    async fn unregister_worker(&mut self, peer: &SocketAddr) -> bool {
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
    fn shared_fetch_worker_pool(&self) -> SharedFetchWorkerPool {
        self.fetch_worker_pool.clone()
    }

    async fn promote_to_warm(
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

    async fn demote_to_cold(&mut self, peer: SocketAddr) -> bool {
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
    fn with_hot_block_fetch_clients<R>(
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
    fn hot_peer_addrs(&self) -> Vec<SocketAddr> {
        self.warm_peers
            .iter()
            .filter(|(_, m)| m.is_hot)
            .map(|(addr, _)| *addr)
            .collect()
    }

    fn promote_to_hot(
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
    fn demote_to_warm(&mut self, peer: SocketAddr) -> bool {
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
    async fn refresh_hot_peer_tips(
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
    fn best_hot_peer(&self) -> Option<SocketAddr> {
        self.warm_peers
            .iter()
            .filter(|(_, m)| m.is_hot && m.last_known_tip.is_some())
            .max_by_key(|(_, m)| m.last_known_tip.as_ref().and_then(|tip| tip.slot()))
            .map(|(addr, _)| *addr)
    }

    async fn drive_keepalives(
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

fn peer_share_request_amount(targets: &GovernorTargets) -> u16 {
    targets.target_known.clamp(1, u16::MAX as usize) as u16
}

impl RuntimeRootPeerSources {
    fn new(topology: &TopologyConfig) -> Self {
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

    fn sync_registry(&self, registry: &mut PeerRegistry) -> bool {
        registry.sync_root_peers(self.state.providers())
    }

    fn bootstrap_peer_addrs(&self) -> Vec<SocketAddr> {
        self.state.providers().public_roots.bootstrap_peers.clone()
    }

    fn local_root_targets(&self) -> Vec<LocalRootTargets> {
        local_root_targets_from_resolved_groups(&self.state.providers().local_roots)
    }

    fn refresh(&mut self, registry: &mut PeerRegistry, tracer: &NodeTracer) -> bool {
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

fn reserve_bootstrap_sync_peers(
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

fn registry_reserve_bootstrap_attempt_peers(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    peers: impl IntoIterator<Item = SocketAddr>,
) {
    if let Some(reg) = peer_registry {
        if let Ok(mut guard) = reg.write() {
            let _ = reserve_bootstrap_sync_peers(&mut guard, peers);
        }
    }
}

fn reconnect_storage_tip(volatile_tip: Point, best_tip: Point) -> Point {
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

fn point_slot(point: &Point) -> Option<u64> {
    match point {
        Point::Origin => None,
        Point::BlockPoint(slot, _) => Some(slot.0),
    }
}

fn preferred_hot_peer_from_registry(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
) -> Option<SocketAddr> {
    let registry_lock = peer_registry?;
    let registry = registry_lock.read().ok()?;
    registry.preferred_hot_peer()
}

fn preferred_hot_peer_handoff_target(
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

fn reconnect_preferred_peer_with_source(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    previous_preferred_peer: Option<SocketAddr>,
) -> Option<(SocketAddr, &'static str)> {
    preferred_hot_peer_from_registry(peer_registry)
        .map(|peer| (peer, "hot"))
        .or(previous_preferred_peer.map(|peer| (peer, "previous")))
}

fn ordered_reconnect_fallback_peers(
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

fn prepare_reconnect_attempt_state(
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
fn reconnect_preferred_peer(
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

fn ledger_peer_snapshot_from_ledger_state(ledger_state: &LedgerState) -> LedgerPeerSnapshot {
    let mut ledger_peers = Vec::new();
    extend_unique_ledger_peers(
        &mut ledger_peers,
        ledger_state.pool_state().relay_access_points(),
    );
    LedgerPeerSnapshot::new(ledger_peers, Vec::new())
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

fn governor_action_name(action: &GovernorAction) -> &'static str {
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

fn governor_action_peer(action: &GovernorAction) -> Option<SocketAddr> {
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

fn direct_sync_bootstrap_pending(registry: &PeerRegistry) -> bool {
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

fn suppress_outbound_promotions_while_bootstrap_pending(
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

fn outbound_cm_local_addr() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 0))
}

fn data_flow_from_version_data(version_data: &NodeToNodeVersionData) -> DataFlow {
    if version_data.initiator_only_diffusion_mode {
        DataFlow::Unidirectional
    } else {
        DataFlow::Duplex
    }
}

fn peer_status_from_cm_state(state: AbstractState) -> PeerStatus {
    match state {
        AbstractState::OutboundUniSt
        | AbstractState::OutboundDupSt(_)
        | AbstractState::InboundSt(_)
        | AbstractState::DuplexSt => PeerStatus::PeerWarm,
        _ => PeerStatus::PeerCold,
    }
}

fn update_registry_status_from_cm(
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

async fn retire_failed_outbound_peer(
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
async fn apply_cm_actions(
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
fn split_timeout_cm_actions_for_governor(
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
