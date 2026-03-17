//! Node runtime — wires networking, storage, and protocol client drivers
//! into a cohesive sync lifecycle.
//!
//! Reference: `cardano-node/src/Cardano/Node/Run.hs`.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::config::{derive_peer_snapshot_freshness, load_peer_snapshot_file};
use crate::sync::{
    LedgerCheckpointTracking, LedgerCheckpointUpdateOutcome, LedgerRecoveryOutcome,
    MultiEraSyncProgress, SyncError, VerifiedSyncServiceConfig,
    apply_verified_progress_to_chaindb,
    apply_nonce_evolution_to_progress, recover_ledger_state_chaindb,
    sync_batch_apply_verified, sync_batch_verified, track_chain_state,
};
use crate::tracer::{NodeMetrics, NodeTracer, trace_fields};
use serde_json::json;
use serde_json::Value;
use yggdrasil_consensus::{ChainState, NonceEvolutionConfig, NonceEvolutionState};
use yggdrasil_network::{
    AfterSlot, BlockFetchClient, ChainSyncClient, HandshakeVersion, KeepAliveClient,
    DnsRefreshPolicy, DnsRootPeerProvider,
    GovernorAction, GovernorState, GovernorTargets, LedgerPeerSnapshot,
    LedgerPeerUseDecision, LedgerStateJudgement, LocalRootConfig,
    LocalRootTargets, MiniProtocolNum, NodeToNodeVersionData, PeerAccessPoint,
    PeerConnection, PeerError, PeerRegistry, PeerSource, PeerStatus,
    PeerSnapshotFreshness, PeerAttemptState, TxIdAndSize, TxServerRequest,
    RootPeerProviderState, TopologyConfig, TxSubmissionClient,
    TxSubmissionClientError, UseLedgerPeers, judge_ledger_peer_usage,
    peer_attempt_state, reconcile_ledger_peer_registry_with_policy,
    refresh_root_peer_state_and_registry, resolve_peer_access_points,
};
use yggdrasil_ledger::{
    LedgerError, LedgerState, MultiEraSubmittedTx, Point, PoolRelayAccessPoint,
    SlotNo, TxId,
};
use yggdrasil_mempool::{
    Mempool, MempoolEntry, MempoolError, MempoolIdx, MempoolSnapshot,
    SharedMempool, MEMPOOL_ZERO_IDX, SharedTxSubmissionMempoolReader,
    TxSubmissionMempoolReader,
};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

/// Runtime governor configuration derived from node configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeGovernorConfig {
    /// Period between governor evaluation ticks.
    pub tick_interval: Duration,
    /// KeepAlive cadence for established warm peers.
    pub keepalive_interval: Option<Duration>,
    /// Target peer counts maintained by the governor.
    pub targets: GovernorTargets,
}

impl RuntimeGovernorConfig {
    /// Construct a runtime governor config from the explicit interval and targets.
    pub fn new(
        tick_interval: Duration,
        keepalive_interval: Option<Duration>,
        targets: GovernorTargets,
    ) -> Self {
        Self {
            tick_interval,
            keepalive_interval,
            targets,
        }
    }
}

struct ManagedWarmPeer {
    session: PeerSession,
    last_keepalive_at: Instant,
    next_cookie: u16,
}

impl ManagedWarmPeer {
    fn new(session: PeerSession, now: Instant) -> Self {
        Self {
            session,
            last_keepalive_at: now,
            next_cookie: 1,
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
}

struct OutboundPeerManager {
    warm_peers: BTreeMap<SocketAddr, ManagedWarmPeer>,
}

struct RuntimeRootPeerSources {
    state: RootPeerProviderState,
    local_roots: Option<DnsRootPeerProvider>,
    bootstrap_peers: Option<DnsRootPeerProvider>,
    public_config_peers: Option<DnsRootPeerProvider>,
}

impl OutboundPeerManager {
    fn new() -> Self {
        Self {
            warm_peers: BTreeMap::new(),
        }
    }

    fn contains(&self, peer: &SocketAddr) -> bool {
        self.warm_peers.contains_key(peer)
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
        };

        match bootstrap(&peer_config).await {
            Ok(session) => {
                let connected_peer_addr = session.connected_peer_addr;
                self.warm_peers.insert(
                    connected_peer_addr,
                    ManagedWarmPeer::new(session, Instant::now()),
                );
                governor_state.record_success(peer);
                tracer.trace_runtime(
                    "Net.Governor",
                    "Info",
                    "warm peer connection established",
                    trace_fields([("peer", json!(connected_peer_addr.to_string()))]),
                );
                true
            }
            Err(err) => {
                governor_state.record_failure(peer);
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

    fn demote_to_cold(&mut self, peer: SocketAddr) -> bool {
        match self.warm_peers.remove(&peer) {
            Some(session) => {
                session.abort();
                true
            }
            None => false,
        }
    }

    async fn drive_keepalives(
        &mut self,
        keepalive_interval: Option<Duration>,
        governor_state: &mut GovernorState,
        tracer: &NodeTracer,
    ) {
        let Some(interval) = keepalive_interval else {
            return;
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

        for (peer, err) in failed {
            if let Some(session) = self.warm_peers.remove(&peer) {
                session.abort();
            }
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
    }
}

impl RuntimeRootPeerSources {
    fn new(topology: &TopologyConfig) -> Self {
        let policy = DnsRefreshPolicy::default();
        let local_roots = (!topology.local_roots.is_empty()).then(|| {
            DnsRootPeerProvider::local_roots(topology.local_roots.clone())
                .with_policy(policy.clone())
        });
        let bootstrap_peers = (!topology.bootstrap_peers.configured_peers().is_empty()).then(|| {
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
                Err(err) => {
                    trace_root_refresh_error(tracer, "PublicConfigPeers", err.to_string())
                }
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
pub fn seed_peer_registry(
    primary_peer: SocketAddr,
    topology: &TopologyConfig,
) -> PeerRegistry {
    let mut registry = PeerRegistry::default();
    registry.sync_root_peers(&topology.resolved_root_providers());
    // Insert the primary peer after syncing root peers so that sync_root_peers
    // (which clears all Bootstrap/LocalRoot/PublicRoot sources first) does not
    // remove the primary peer's Bootstrap source when the primary is not listed
    // in the topology bootstrap set.
    registry.insert_source(primary_peer, PeerSource::PeerSourceBootstrap);
    registry
}

/// Derive local-root governor targets from resolved topology groups.
pub fn local_root_targets_from_config(
    local_roots: &[LocalRootConfig],
) -> Vec<LocalRootTargets> {
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
        })
        .collect()
}

fn point_slot(point: &Point) -> Option<u64> {
    match point {
        Point::Origin => None,
        Point::BlockPoint(slot, _) => Some(slot.0),
    }
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

fn merge_ledger_peer_snapshots(
    ledger_snapshot: &LedgerPeerSnapshot,
    snapshot_file: Option<LedgerPeerSnapshot>,
) -> LedgerPeerSnapshot {
    let mut merged_ledger_peers = ledger_snapshot.ledger_peers.clone();
    let mut merged_big_ledger_peers = ledger_snapshot.big_ledger_peers.clone();

    if let Some(snapshot_file) = snapshot_file {
        extend_unique_peers(&mut merged_ledger_peers, snapshot_file.ledger_peers);
        extend_unique_peers(&mut merged_big_ledger_peers, snapshot_file.big_ledger_peers);
    }

    LedgerPeerSnapshot::new(merged_ledger_peers, merged_big_ledger_peers)
}

fn ledger_peer_snapshot_from_ledger_state(ledger_state: &LedgerState) -> LedgerPeerSnapshot {
    let mut ledger_peers = Vec::new();
    extend_unique_ledger_peers(&mut ledger_peers, ledger_state.pool_state().relay_access_points());
    LedgerPeerSnapshot::new(ledger_peers, Vec::new())
}

fn refresh_ledger_peer_sources_from_chain_db<I, V, L>(
    registry: &mut PeerRegistry,
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    base_ledger_state: &LedgerState,
    topology: &TopologyConfig,
    tracer: &NodeTracer,
) -> bool
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    if !topology.use_ledger_peers.enabled() {
        return false;
    }

    let (latest_slot, ledger_state_judgement, ledger_snapshot) = {
        let chain_db = chain_db.read().expect("chain db lock poisoned");
        let tip = chain_db.recovery().tip;
        match recover_ledger_state_chaindb(&chain_db, base_ledger_state.clone()) {
            Ok(recovery) => (
                point_slot(&recovery.point).or_else(|| point_slot(&tip)),
                LedgerStateJudgement::YoungEnough,
                ledger_peer_snapshot_from_ledger_state(&recovery.ledger_state),
            ),
            Err(err) => {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to recover ledger peers from chain db",
                    trace_fields([("error", json!(err.to_string()))]),
                );
                (
                    point_slot(&tip),
                    LedgerStateJudgement::Unavailable,
                    LedgerPeerSnapshot::default(),
                )
            }
        }
    };

    let mut snapshot_slot = None;
    let mut snapshot_available = topology.peer_snapshot_file.is_none();
    let mut snapshot_file = None;

    if let Some(peer_snapshot_file) = topology.peer_snapshot_file.as_deref() {
        match load_peer_snapshot_file(Path::new(peer_snapshot_file)) {
            Ok(loaded_snapshot) => {
                snapshot_slot = loaded_snapshot.slot;
                snapshot_available = true;
                snapshot_file = Some(loaded_snapshot.snapshot);
            }
            Err(err) => {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to refresh configured peer snapshot",
                    trace_fields([
                        ("snapshotPath", json!(peer_snapshot_file)),
                        ("error", json!(err.to_string())),
                    ]),
                );
            }
        }
    }

    let peer_snapshot_freshness = derive_peer_snapshot_freshness(
        topology.use_ledger_peers,
        topology.peer_snapshot_file.is_some(),
        snapshot_slot,
        latest_slot,
        snapshot_available,
    );

    let update = reconcile_ledger_peer_registry_with_policy(
        registry,
        merge_ledger_peer_snapshots(&ledger_snapshot, snapshot_file),
        topology.use_ledger_peers,
        latest_slot,
        ledger_state_judgement,
        peer_snapshot_freshness,
    );

    if update.changed {
        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "ledger peer registry refreshed",
            trace_fields([
                ("decision", json!(format!("{:?}", update.decision))),
                ("latestSlot", json!(latest_slot)),
                (
                    "peerSnapshotFreshness",
                    json!(format!("{:?}", peer_snapshot_freshness)),
                ),
            ]),
        );
    }

    update.changed
}

fn governor_action_name(action: &GovernorAction) -> &'static str {
    match action {
        GovernorAction::PromoteToWarm(_) => "PromoteToWarm",
        GovernorAction::PromoteToHot(_) => "PromoteToHot",
        GovernorAction::DemoteToWarm(_) => "DemoteToWarm",
        GovernorAction::DemoteToCold(_) => "DemoteToCold",
    }
}

fn governor_action_peer(action: &GovernorAction) -> SocketAddr {
    match action {
        GovernorAction::PromoteToWarm(peer)
        | GovernorAction::PromoteToHot(peer)
        | GovernorAction::DemoteToWarm(peer)
        | GovernorAction::DemoteToCold(peer) => *peer,
    }
}

/// Run the peer governor loop until shutdown.
///
/// The loop periodically refreshes root peers from DNS-backed providers,
/// refreshes ledger peers from the current ChainDb recovery view plus optional
/// peer snapshot file, drives warm-peer KeepAlive traffic, and then executes
/// governor actions against the shared peer registry and outbound warm sessions.
pub async fn run_governor_loop<I, V, L, F>(
    node_config: NodeConfig,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    peer_registry: Arc<RwLock<PeerRegistry>>,
    mut governor_state: GovernorState,
    config: RuntimeGovernorConfig,
    topology: TopologyConfig,
    base_ledger_state: LedgerState,
    tracer: NodeTracer,
    shutdown: F,
) where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let mut interval = tokio::time::interval(config.tick_interval);
    let mut peer_manager = OutboundPeerManager::new();
    let mut root_sources = RuntimeRootPeerSources::new(&topology);
    tokio::pin!(shutdown);

    {
        let mut registry = peer_registry.write().expect("peer registry lock poisoned");
        root_sources.sync_registry(&mut registry);
        refresh_ledger_peer_sources_from_chain_db(
            &mut registry,
            &chain_db,
            &base_ledger_state,
            &topology,
            &tracer,
        );
    }

    tracer.trace_runtime(
        "Net.Governor",
        "Notice",
        "peer governor started",
        trace_fields([
            ("tickIntervalSecs", json!(config.tick_interval.as_secs())),
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
                tracer.trace_runtime(
                    "Net.Governor",
                    "Notice",
                    "peer governor stopped",
                    BTreeMap::new(),
                );
                return;
            }

            _ = interval.tick() => {
                {
                    let mut registry = peer_registry.write().expect("peer registry lock poisoned");
                    root_sources.refresh(&mut registry, &tracer);
                    refresh_ledger_peer_sources_from_chain_db(
                        &mut registry,
                        &chain_db,
                        &base_ledger_state,
                        &topology,
                        &tracer,
                    );
                }

                peer_manager
                    .drive_keepalives(config.keepalive_interval, &mut governor_state, &tracer)
                    .await;

                let local_root_groups = root_sources.local_root_targets();
                let actions = {
                    let registry = peer_registry.read().expect("peer registry lock poisoned");
                    governor_state.tick(&registry, &config.targets, &local_root_groups, Instant::now())
                };

                if actions.is_empty() {
                    continue;
                }

                for action in actions {
                    let peer = governor_action_peer(&action);
                    let changed = match action {
                        GovernorAction::PromoteToWarm(peer) => {
                            if peer_manager
                                .promote_to_warm(&node_config, peer, &mut governor_state, &tracer)
                                .await
                            {
                                let mut registry = peer_registry
                                    .write()
                                    .expect("peer registry lock poisoned");
                                registry.set_status(peer, PeerStatus::PeerWarm)
                            } else {
                                false
                            }
                        }
                        GovernorAction::PromoteToHot(peer) => {
                            if peer_manager.contains(&peer) {
                                let mut registry = peer_registry
                                    .write()
                                    .expect("peer registry lock poisoned");
                                registry.set_status(peer, PeerStatus::PeerHot)
                            } else {
                                false
                            }
                        }
                        GovernorAction::DemoteToWarm(peer) => {
                            let mut registry = peer_registry
                                .write()
                                .expect("peer registry lock poisoned");
                            registry.set_status(peer, PeerStatus::PeerWarm)
                        }
                        GovernorAction::DemoteToCold(peer) => {
                            let connection_changed = peer_manager.demote_to_cold(peer);
                            let mut registry = peer_registry
                                .write()
                                .expect("peer registry lock poisoned");
                            registry.set_status(peer, PeerStatus::PeerCold) || connection_changed
                        }
                    };
                    tracer.trace_runtime(
                        "Net.Governor",
                        if changed { "Info" } else { "Debug" },
                        "peer governor action applied",
                        trace_fields([
                            ("action", json!(governor_action_name(&action))),
                            ("peer", json!(peer.to_string())),
                            ("changed", json!(changed)),
                        ]),
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TxSubmission mempool integration
// ---------------------------------------------------------------------------

/// Result of attempting to add a single transaction to the mempool.
///
/// This mirrors the upstream `MempoolAddTxResult` split between accepted and
/// rejected transactions while keeping queue-level failures separate.
#[derive(Debug, Eq, PartialEq)]
pub enum MempoolAddTxResult {
    /// The transaction was validated and added to the mempool.
    MempoolTxAdded(TxId),
    /// The transaction was rejected by ledger validation and not added.
    MempoolTxRejected(TxId, LedgerError),
}

/// Queue-level failures encountered while adding a transaction to the mempool.
#[derive(Debug, thiserror::Error)]
pub enum MempoolAddTxError {
    /// Underlying mempool capacity, duplicate, or TTL error.
    #[error("mempool admission error: {0}")]
    Mempool(#[from] MempoolError),
}

fn admitted_entry(tx: MultiEraSubmittedTx) -> MempoolEntry {
    let fee = tx.fee();
    let ttl = tx.expires_at().unwrap_or(SlotNo(u64::MAX));
    MempoolEntry::from_multi_era_submitted_tx(tx, fee, ttl)
}

fn add_tx_with<F>(
    ledger: &mut LedgerState,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
    mut insert_entry: F,
) -> Result<MempoolAddTxResult, MempoolAddTxError>
where
    F: FnMut(MempoolEntry) -> Result<(), MempoolError>,
{
    let tx_id = tx.tx_id();
    let mut staged_ledger = ledger.clone();
    match staged_ledger.apply_submitted_tx(&tx, current_slot) {
        Ok(()) => {
            insert_entry(admitted_entry(tx))?;
            *ledger = staged_ledger;
            Ok(MempoolAddTxResult::MempoolTxAdded(tx_id))
        }
        Err(err) => Ok(MempoolAddTxResult::MempoolTxRejected(tx_id, err)),
    }
}

/// Validate and add a single transaction to the mempool.
///
/// The transaction is first applied to a staged clone of the caller-provided
/// ledger state. If ledger validation fails, the ledger and mempool remain
/// unchanged and the result is `MempoolTxRejected`. If validation succeeds, the
/// transaction is inserted into the mempool and the staged ledger state is
/// committed.
pub fn add_tx_to_mempool(
    ledger: &mut LedgerState,
    mempool: &mut Mempool,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
) -> Result<MempoolAddTxResult, MempoolAddTxError> {
    add_tx_with(ledger, tx, current_slot, |entry| {
        mempool.insert_checked(entry, current_slot)
    })
}

/// Validate and add a single transaction to a shared mempool.
///
/// This is the shared-handle variant of [`add_tx_to_mempool`]. Accepted
/// transactions update the caller's ledger state only after the shared mempool
/// insert succeeds.
pub fn add_tx_to_shared_mempool(
    ledger: &mut LedgerState,
    mempool: &SharedMempool,
    tx: MultiEraSubmittedTx,
    current_slot: SlotNo,
) -> Result<MempoolAddTxResult, MempoolAddTxError> {
    add_tx_with(ledger, tx, current_slot, |entry| {
        mempool.insert_checked(entry, current_slot)
    })
}

/// Validate and add a sequence of transactions to the mempool in order.
///
/// This mirrors the upstream `addTxs` semantics: each transaction is checked
/// against the ledger state produced by all previously accepted transactions in
/// the same batch. Rejected transactions do not advance the staged ledger
/// state. Queue-level failures stop the batch and return an error.
pub fn add_txs_to_mempool<I>(
    ledger: &mut LedgerState,
    mempool: &mut Mempool,
    txs: I,
    current_slot: SlotNo,
) -> Result<Vec<MempoolAddTxResult>, MempoolAddTxError>
where
    I: IntoIterator<Item = MultiEraSubmittedTx>,
{
    txs.into_iter()
        .map(|tx| add_tx_to_mempool(ledger, mempool, tx, current_slot))
        .collect()
}

/// Validate and add a sequence of transactions to a shared mempool in order.
///
/// Accepted transactions update the caller's ledger state one by one so later
/// transactions in the batch can depend on earlier accepted outputs.
pub fn add_txs_to_shared_mempool<I>(
    ledger: &mut LedgerState,
    mempool: &SharedMempool,
    txs: I,
    current_slot: SlotNo,
) -> Result<Vec<MempoolAddTxResult>, MempoolAddTxError>
where
    I: IntoIterator<Item = MultiEraSubmittedTx>,
{
    txs.into_iter()
        .map(|tx| add_tx_to_shared_mempool(ledger, mempool, tx, current_slot))
        .collect()
}

/// Errors from serving TxSubmission requests out of a mempool snapshot.
#[derive(Debug, thiserror::Error)]
pub enum TxSubmissionServiceError {
    /// Underlying TxSubmission protocol client error.
    #[error("tx-submission client error: {0}")]
    Client(#[from] TxSubmissionClientError),
}

/// Outcome returned when the managed TxSubmission service finishes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TxSubmissionServiceOutcome {
    /// Number of TxSubmission requests handled by the service.
    pub handled_requests: usize,
    /// `true` when the protocol terminated normally via `MsgDone`, `false`
    /// when the service stopped due to shutdown.
    pub terminated_by_protocol: bool,
}

trait TxSubmissionSnapshotReader {
    fn mempool_get_snapshot(&self) -> MempoolSnapshot;
}

impl TxSubmissionSnapshotReader for TxSubmissionMempoolReader<'_> {
    fn mempool_get_snapshot(&self) -> MempoolSnapshot {
        self.mempool_get_snapshot()
    }
}

impl TxSubmissionSnapshotReader for SharedTxSubmissionMempoolReader {
    fn mempool_get_snapshot(&self) -> MempoolSnapshot {
        self.mempool_get_snapshot()
    }
}

/// Serve a single TxSubmission request using the current mempool contents.
///
/// Tx ids are advertised from a TxSubmission mempool snapshot using the
/// monotonic `last_idx` cursor expected by the outbound side. For blocking
/// requests with no available transactions after `last_idx`, the helper
/// terminates the mini-protocol with `MsgDone` and returns `Ok(false)`.
async fn serve_txsubmission_request_from_snapshot_reader<R>(
    client: &mut TxSubmissionClient,
    reader: &R,
    last_idx: &mut MempoolIdx,
) -> Result<bool, TxSubmissionServiceError>
where
    R: TxSubmissionSnapshotReader,
{
    match client.recv_request().await? {
        TxServerRequest::RequestTxIds { blocking, req, .. } => {
            let snapshot = reader.mempool_get_snapshot();
            let txids = snapshot
                .mempool_txids_after(*last_idx)
                .into_iter()
                .take(req as usize)
                .map(|(txid, idx, size_bytes)| {
                    *last_idx = idx;
                    TxIdAndSize {
                        txid,
                        size: size_bytes.min(u32::MAX as usize) as u32,
                    }
                })
                .collect::<Vec<_>>();

            if txids.is_empty() && blocking {
                client.send_done().await?;
                Ok(false)
            } else {
                client.reply_tx_ids(txids).await?;
                Ok(true)
            }
        }
        TxServerRequest::RequestTxs { txids } => {
            let snapshot = reader.mempool_get_snapshot();
            let txs = txids
                .into_iter()
                .filter_map(|txid| snapshot.mempool_lookup_tx_by_id(&txid))
                .map(|entry| entry.raw_tx.clone())
                .collect::<Vec<_>>();
            client.reply_txs(txs).await?;
            Ok(true)
        }
    }
}

pub async fn serve_txsubmission_request_from_reader(
    client: &mut TxSubmissionClient,
    reader: &TxSubmissionMempoolReader<'_>,
    last_idx: &mut MempoolIdx,
) -> Result<bool, TxSubmissionServiceError> {
    serve_txsubmission_request_from_snapshot_reader(client, reader, last_idx).await
}

/// Run a managed TxSubmission loop backed by a shared mempool snapshot source
/// until shutdown or protocol termination.
///
/// This variant allows concurrent mempool updates while the service is
/// running. Each request takes a fresh snapshot from the shared handle and
/// continues advertising from the previously served `last_idx` position.
pub async fn run_txsubmission_service_shared<F>(
    client: &mut TxSubmissionClient,
    mempool: &SharedMempool,
    shutdown: F,
) -> Result<TxSubmissionServiceOutcome, TxSubmissionServiceError>
where
    F: Future<Output = ()>,
{
    client.init().await?;
    tokio::pin!(shutdown);

    let mut handled_requests = 0usize;
    let reader = mempool.txsubmission_mempool_reader();
    let mut last_idx = MEMPOOL_ZERO_IDX;

    loop {
        let serve_fut =
            serve_txsubmission_request_from_snapshot_reader(client, &reader, &mut last_idx);

        tokio::select! {
            biased;

            () = &mut shutdown => {
                return Ok(TxSubmissionServiceOutcome {
                    handled_requests,
                    terminated_by_protocol: false,
                });
            }

            result = serve_fut => {
                handled_requests += 1;
                let should_continue = result?;
                if !should_continue {
                    return Ok(TxSubmissionServiceOutcome {
                        handled_requests,
                        terminated_by_protocol: true,
                    });
                }
            }
        }
    }
}

/// Serve a single TxSubmission request using the current mempool contents.
///
/// Tx ids are advertised in the mempool's existing fee-descending order. For
/// blocking requests with no available transactions, the helper terminates the
/// mini-protocol with `MsgDone` and returns `Ok(false)`.
pub async fn serve_txsubmission_request_from_mempool(
    client: &mut TxSubmissionClient,
    mempool: &Mempool,
) -> Result<bool, TxSubmissionServiceError> {
    match client.recv_request().await? {
        TxServerRequest::RequestTxIds { blocking, req, .. } => {
            let txids = mempool
                .iter()
                .take(req as usize)
                .map(|entry| TxIdAndSize {
                    txid: entry.tx_id,
                    size: entry.size_bytes.min(u32::MAX as usize) as u32,
                })
                .collect::<Vec<_>>();

            if txids.is_empty() && blocking {
                client.send_done().await?;
                Ok(false)
            } else {
                client.reply_tx_ids(txids).await?;
                Ok(true)
            }
        }
        TxServerRequest::RequestTxs { txids } => {
            let txs = txids
                .into_iter()
                .filter_map(|txid| mempool.iter().find(|entry| entry.tx_id == txid))
                .map(|entry| entry.raw_tx.clone())
                .collect::<Vec<_>>();
            client.reply_txs(txs).await?;
            Ok(true)
        }
    }
}

/// Run a managed TxSubmission loop backed by the current mempool snapshot
/// until shutdown or protocol termination.
///
/// The service sends `MsgInit` once, then repeatedly serves incoming
/// TxSubmission requests from the provided mempool. If a blocking request
/// arrives while the mempool is empty, the helper terminates the protocol with
/// `MsgDone` and returns an outcome marked as protocol-terminated.
pub async fn run_txsubmission_service<F>(
    client: &mut TxSubmissionClient,
    mempool: &Mempool,
    shutdown: F,
) -> Result<TxSubmissionServiceOutcome, TxSubmissionServiceError>
where
    F: Future<Output = ()>,
{
    client.init().await?;
    tokio::pin!(shutdown);

    let mut handled_requests = 0usize;
    let reader = mempool.txsubmission_mempool_reader();
    let mut last_idx = MEMPOOL_ZERO_IDX;

    loop {
        let serve_fut =
            serve_txsubmission_request_from_snapshot_reader(client, &reader, &mut last_idx);

        tokio::select! {
            biased;

            () = &mut shutdown => {
                return Ok(TxSubmissionServiceOutcome {
                    handled_requests,
                    terminated_by_protocol: false,
                });
            }

            result = serve_fut => {
                handled_requests += 1;
                let should_continue = result?;
                if !should_continue {
                    return Ok(TxSubmissionServiceOutcome {
                        handled_requests,
                        terminated_by_protocol: true,
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// NodeConfig
// ---------------------------------------------------------------------------

/// Minimal configuration for establishing a node-to-node connection.
///
/// This covers the subset needed for initial sync bootstrapping.
#[derive(Clone, Debug)]
pub struct NodeConfig {
    /// Address of the upstream peer to connect to.
    pub peer_addr: SocketAddr,
    /// The network magic for the target network (e.g. mainnet = 764824073).
    pub network_magic: u32,
    /// Protocol versions to propose during handshake, ordered by preference.
    pub protocol_versions: Vec<HandshakeVersion>,
}

// ---------------------------------------------------------------------------
// PeerSession — result of bootstrapping a connection
// ---------------------------------------------------------------------------

/// A fully-negotiated peer session with typed protocol drivers ready for use.
///
/// Owns the [`PeerConnection`]'s mux handle and exposes each data-protocol
/// client as a named field.
pub struct PeerSession {
    /// Upstream peer address that completed the handshake.
    pub connected_peer_addr: SocketAddr,
    /// ChainSync client driver.
    pub chain_sync: ChainSyncClient,
    /// BlockFetch client driver.
    pub block_fetch: BlockFetchClient,
    /// KeepAlive client driver.
    pub keep_alive: KeepAliveClient,
    /// TxSubmission client driver.
    pub tx_submission: TxSubmissionClient,
    /// Mux handle — abort to tear down the connection.
    pub mux: yggdrasil_network::MuxHandle,
    /// Negotiated protocol version.
    pub version: HandshakeVersion,
    /// Agreed-upon version data.
    pub version_data: NodeToNodeVersionData,
}

/// Outcome returned when the reconnecting verified sync runner stops.
#[derive(Clone, Debug)]
pub struct ReconnectingSyncServiceOutcome {
    /// Final chain point when the service stopped.
    pub final_point: Point,
    /// Total blocks fetched across all batches.
    pub total_blocks: usize,
    /// Total rollback events across all batches.
    pub total_rollbacks: usize,
    /// Number of batch iterations completed.
    pub batches_completed: usize,
    /// Final nonce evolution state (present when nonce tracking was enabled).
    pub nonce_state: Option<NonceEvolutionState>,
    /// Final chain state (present when chain tracking was enabled).
    pub chain_state: Option<ChainState>,
    /// Total number of blocks that crossed the stability window during the run.
    pub stable_block_count: usize,
    /// Number of reconnects performed after the initial successful session.
    pub reconnect_count: usize,
    /// The most recent peer that successfully completed bootstrap.
    pub last_connected_peer_addr: Option<SocketAddr>,
}

/// Outcome returned when a coordinated-storage sync run first restores ledger
/// state from `ChainDb` recovery data and then starts reconnecting sync.
#[derive(Clone, Debug)]
pub struct ResumedSyncServiceOutcome {
    /// Ledger recovery state rebuilt before live syncing begins.
    pub recovery: LedgerRecoveryOutcome,
    /// Outcome from the reconnecting live sync loop started at the recovered point.
    pub sync: ReconnectingSyncServiceOutcome,
}

/// Request parameters for reconnecting verified sync runners.
pub struct ReconnectingVerifiedSyncRequest<'a> {
    /// Node-to-node bootstrap configuration.
    pub node_config: &'a NodeConfig,
    /// Ordered fallback peers tried after the primary peer.
    pub fallback_peer_addrs: &'a [SocketAddr],
    /// Chain point from which live sync should begin.
    pub from_point: Point,
    /// Base ledger state used for coordinated-storage replay paths.
    pub base_ledger_state: LedgerState,
    /// Verified sync policy and batch configuration.
    pub config: &'a VerifiedSyncServiceConfig,
    /// Optional nonce-evolution state to carry through the run.
    pub nonce_state: Option<NonceEvolutionState>,
    /// Optional ledger-peer policy for refreshing ChainDb reconnect targets.
    pub use_ledger_peers: Option<UseLedgerPeers>,
    /// Optional resolved peer snapshot file path for reconnect-time refresh.
    pub peer_snapshot_path: Option<PathBuf>,
}

impl<'a> ReconnectingVerifiedSyncRequest<'a> {
    /// Construct a reconnecting verified-sync request with optional fields
    /// initialized to their disabled defaults.
    pub fn new(
        node_config: &'a NodeConfig,
        fallback_peer_addrs: &'a [SocketAddr],
        from_point: Point,
        base_ledger_state: LedgerState,
        config: &'a VerifiedSyncServiceConfig,
    ) -> Self {
        Self {
            node_config,
            fallback_peer_addrs,
            from_point,
            base_ledger_state,
            config,
            nonce_state: None,
            use_ledger_peers: None,
            peer_snapshot_path: None,
        }
    }

    /// Attach a nonce-evolution state to carry through the reconnecting run.
    pub fn with_nonce_state(mut self, nonce_state: Option<NonceEvolutionState>) -> Self {
        self.nonce_state = nonce_state;
        self
    }

    /// Enable reconnect-time ledger-peer policy refresh.
    pub fn with_use_ledger_peers(mut self, use_ledger_peers: Option<UseLedgerPeers>) -> Self {
        self.use_ledger_peers = use_ledger_peers;
        self
    }

    /// Provide an optional resolved peer snapshot file path for reconnect-time refresh.
    pub fn with_peer_snapshot_path(mut self, peer_snapshot_path: Option<PathBuf>) -> Self {
        self.peer_snapshot_path = peer_snapshot_path;
        self
    }
}

/// Request parameters for coordinated-storage reconnecting sync resumption.
pub struct ResumeReconnectingVerifiedSyncRequest<'a> {
    /// Node-to-node bootstrap configuration.
    pub node_config: &'a NodeConfig,
    /// Ordered fallback peers tried after the primary peer.
    pub fallback_peer_addrs: &'a [SocketAddr],
    /// Base ledger state used before replaying persisted recovery data.
    pub base_ledger_state: LedgerState,
    /// Verified sync policy and batch configuration.
    pub config: &'a VerifiedSyncServiceConfig,
    /// Optional nonce-evolution state to carry through the resumed run.
    pub nonce_state: Option<NonceEvolutionState>,
    /// Optional ledger-peer policy for refreshing ChainDb reconnect targets.
    pub use_ledger_peers: Option<UseLedgerPeers>,
    /// Optional resolved peer snapshot file path for reconnect-time refresh.
    pub peer_snapshot_path: Option<PathBuf>,
    /// Optional metrics tracker updated during sync.
    pub metrics: Option<&'a NodeMetrics>,
}

impl<'a> ResumeReconnectingVerifiedSyncRequest<'a> {
    /// Construct a coordinated-storage resume request with optional fields
    /// initialized to their disabled defaults.
    pub fn new(
        node_config: &'a NodeConfig,
        fallback_peer_addrs: &'a [SocketAddr],
        base_ledger_state: LedgerState,
        config: &'a VerifiedSyncServiceConfig,
    ) -> Self {
        Self {
            node_config,
            fallback_peer_addrs,
            base_ledger_state,
            config,
            nonce_state: None,
            use_ledger_peers: None,
            peer_snapshot_path: None,
            metrics: None,
        }
    }

    /// Attach a nonce-evolution state to carry through the resumed run.
    pub fn with_nonce_state(mut self, nonce_state: Option<NonceEvolutionState>) -> Self {
        self.nonce_state = nonce_state;
        self
    }

    /// Enable reconnect-time ledger-peer policy refresh.
    pub fn with_use_ledger_peers(mut self, use_ledger_peers: Option<UseLedgerPeers>) -> Self {
        self.use_ledger_peers = use_ledger_peers;
        self
    }

    /// Provide an optional resolved peer snapshot file path for reconnect-time refresh.
    pub fn with_peer_snapshot_path(mut self, peer_snapshot_path: Option<PathBuf>) -> Self {
        self.peer_snapshot_path = peer_snapshot_path;
        self
    }

    /// Attach an optional metrics sink for runtime progress reporting.
    pub fn with_metrics(mut self, metrics: Option<&'a NodeMetrics>) -> Self {
        self.metrics = metrics;
        self
    }
}

type CheckpointTracking = LedgerCheckpointTracking;

fn shared_chaindb_lock_error() -> SyncError {
    SyncError::Recovery("shared ChainDb lock poisoned".to_owned())
}

struct ReconnectingVerifiedSyncContext<'a> {
    node_config: &'a NodeConfig,
    fallback_peer_addrs: &'a [SocketAddr],
    use_ledger_peers: Option<UseLedgerPeers>,
    peer_snapshot_path: Option<&'a Path>,
    config: &'a VerifiedSyncServiceConfig,
    tracer: &'a NodeTracer,
    metrics: Option<&'a NodeMetrics>,
}

struct ReconnectingVerifiedSyncState {
    from_point: Point,
    nonce_state: Option<NonceEvolutionState>,
    checkpoint_tracking: Option<CheckpointTracking>,
}

struct ReconnectingRunState {
    total_blocks: usize,
    total_rollbacks: usize,
    batches_completed: usize,
    stable_block_count: usize,
    reconnect_count: usize,
    last_connected_peer_addr: Option<SocketAddr>,
}

impl ReconnectingRunState {
    fn new() -> Self {
        Self {
            total_blocks: 0,
            total_rollbacks: 0,
            batches_completed: 0,
            stable_block_count: 0,
            reconnect_count: 0,
            last_connected_peer_addr: None,
        }
    }

    fn record_session(&mut self, peer_addr: SocketAddr, had_session: &mut bool) {
        if *had_session {
            self.reconnect_count += 1;
        } else {
            *had_session = true;
        }
        self.last_connected_peer_addr = Some(peer_addr);
    }

    fn record_progress(&mut self, progress: &MultiEraSyncProgress) {
        self.total_blocks += progress.fetched_blocks;
        self.total_rollbacks += progress.rollback_count;
        self.batches_completed += 1;
    }

    fn finish(
        self,
        final_point: Point,
        nonce_state: Option<NonceEvolutionState>,
        chain_state: Option<ChainState>,
    ) -> ReconnectingSyncServiceOutcome {
        ReconnectingSyncServiceOutcome {
            final_point,
            total_blocks: self.total_blocks,
            total_rollbacks: self.total_rollbacks,
            batches_completed: self.batches_completed,
            nonce_state,
            chain_state,
            stable_block_count: self.stable_block_count,
            reconnect_count: self.reconnect_count,
            last_connected_peer_addr: self.last_connected_peer_addr,
        }
    }
}

struct BatchTraceExtras {
    stable_block_count: Option<usize>,
    checkpoint_tracked: Option<bool>,
}

enum BatchErrorDisposition {
    Reconnect,
    Fail,
}

fn record_verified_batch_progress(
    from_point: &mut Point,
    run_state: &mut ReconnectingRunState,
    progress: &MultiEraSyncProgress,
    nonce_state: Option<&mut NonceEvolutionState>,
    nonce_config: Option<&NonceEvolutionConfig>,
    metrics: Option<&NodeMetrics>,
) {
    *from_point = progress.current_point;
    run_state.record_progress(progress);

    if let Some((state, nonce_cfg)) = nonce_state.zip(nonce_config) {
        apply_nonce_evolution_to_progress(state, progress, nonce_cfg);
    }

    if let Some(m) = metrics {
        m.add_blocks_synced(progress.fetched_blocks as u64);
        m.add_rollbacks(progress.rollback_count as u64);
        m.inc_batches_completed();
        if let Point::BlockPoint(slot, _) = progress.current_point {
            m.set_current_slot(slot.0);
        }
    }
}

fn peer_point_trace_fields(
    peer_addr: SocketAddr,
    current_point: Point,
) -> BTreeMap<String, Value> {
    trace_fields([
        ("peer", json!(peer_addr.to_string())),
        ("currentPoint", json!(format!("{:?}", current_point))),
    ])
}

fn session_established_trace_fields(
    peer_addr: SocketAddr,
    reconnect_count: usize,
    from_point: Point,
) -> BTreeMap<String, Value> {
    trace_fields([
        ("peer", json!(peer_addr.to_string())),
        ("reconnectCount", json!(reconnect_count)),
        ("fromPoint", json!(format!("{:?}", from_point))),
    ])
}

fn sync_error_trace_fields(
    peer_addr: SocketAddr,
    error: &impl ToString,
    current_point: Point,
) -> BTreeMap<String, Value> {
    let mut fields = peer_point_trace_fields(peer_addr, current_point);
    fields.insert("error".to_owned(), json!(error.to_string()));
    fields
}

fn verified_sync_batch_trace_fields(
    peer_addr: SocketAddr,
    current_point: Point,
    progress: &MultiEraSyncProgress,
    run_state: &ReconnectingRunState,
    extras: BatchTraceExtras,
) -> BTreeMap<String, Value> {
    let mut fields = peer_point_trace_fields(peer_addr, current_point);
    fields.insert("batchFetchedBlocks".to_owned(), json!(progress.fetched_blocks));
    fields.insert("batchRollbacks".to_owned(), json!(progress.rollback_count));
    fields.insert("totalBlocks".to_owned(), json!(run_state.total_blocks));
    fields.insert("batchesCompleted".to_owned(), json!(run_state.batches_completed));
    if let Some(stable_block_count) = extras.stable_block_count {
        fields.insert("stableBlocks".to_owned(), json!(stable_block_count));
    }
    if let Some(checkpoint_tracked) = extras.checkpoint_tracked {
        fields.insert("checkpointTracked".to_owned(), json!(checkpoint_tracked));
    }

    fields
}

fn trace_shutdown_before_bootstrap(tracer: &NodeTracer) {
    tracer.trace_runtime(
        "Node.Shutdown",
        "Notice",
        "shutdown requested before bootstrap completed",
        BTreeMap::new(),
    );
}

fn trace_shutdown_during_session(
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

fn trace_sync_failure(
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
    error: &SyncError,
    current_point: Point,
) {
    tracer.trace_runtime(
        "Node.Sync",
        "Error",
        "verified sync service failed",
        sync_error_trace_fields(peer_addr, error, current_point),
    );
}

fn trace_verified_sync_batch_applied(
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
    current_point: Point,
    progress: &MultiEraSyncProgress,
    run_state: &ReconnectingRunState,
    extras: BatchTraceExtras,
) {
    tracer.trace_runtime(
        "ChainSync.Client",
        "Info",
        "verified sync batch applied",
        verified_sync_batch_trace_fields(
            peer_addr,
            current_point,
            progress,
            run_state,
            extras,
        ),
    );
}

fn handle_reconnect_batch_error(
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
    current_point: Point,
    error: &SyncError,
) -> BatchErrorDisposition {
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
    let latest_slot = checkpoint_tracking.ledger_state.tip.slot().map(|slot| slot.0);
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
        for access_point in checkpoint_tracking.ledger_state.pool_state().relay_access_points() {
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
    let mut snapshot = LedgerPeerSnapshot::new(ledger_peers, Vec::new());

    if let Some(peer_snapshot_path) = peer_snapshot_path {
        match load_peer_snapshot_file(peer_snapshot_path) {
            Ok(loaded_snapshot) => {
                snapshot_slot = loaded_snapshot.slot;
                snapshot_available = true;
                let mut merged_ledger_peers = snapshot.ledger_peers;
                extend_unique_socket_addrs(&mut merged_ledger_peers, loaded_snapshot.snapshot.ledger_peers);

                snapshot = LedgerPeerSnapshot::new(
                    merged_ledger_peers,
                    loaded_snapshot.snapshot.big_ledger_peers,
                );
            }
            Err(err) => {
                snapshot_available = false;
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to refresh reconnect peer snapshot",
                    trace_fields([
                        ("snapshotPath", json!(peer_snapshot_path.display().to_string())),
                        ("error", json!(err.to_string())),
                    ]),
                );
            }
        }
    }

    let freshness: PeerSnapshotFreshness = derive_peer_snapshot_freshness(
        use_ledger_peers,
        peer_snapshot_path.is_some(),
        snapshot_slot,
        latest_slot,
        snapshot_available,
    );
    let decision = judge_ledger_peer_usage(
        use_ledger_peers,
        latest_slot,
        LedgerStateJudgement::YoungEnough,
        freshness,
    );

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
        ]),
    );

    if decision != LedgerPeerUseDecision::Eligible {
        return refreshed;
    }

    extend_unique_socket_addrs(
        &mut refreshed,
        snapshot
            .ledger_peers
            .into_iter()
            .chain(snapshot.big_ledger_peers)
            .filter(|peer| *peer != primary_peer),
    );
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
        CheckpointPersistenceOutcome::ClearedDisabled => {
            ("Notice", "ledger checkpoints cleared because persistence is disabled")
        }
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

async fn run_reconnecting_verified_sync_service_chaindb_inner<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    context: ReconnectingVerifiedSyncContext<'_>,
    state: ReconnectingVerifiedSyncState,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncContext {
        node_config,
        fallback_peer_addrs,
        use_ledger_peers,
        peer_snapshot_path,
        config,
        tracer,
        metrics,
    } = context;
    let ReconnectingVerifiedSyncState {
        mut from_point,
        mut nonce_state,
        mut checkpoint_tracking,
    } = state;

    tokio::pin!(shutdown);

    let mut run_state = ReconnectingRunState::new();
    let mut chain_state = config.security_param.map(ChainState::new);
    let mut had_session = false;
    let mut preferred_peer = None;

    loop {
        let refreshed_fallback_peers = refresh_chain_db_reconnect_fallback_peers(
            node_config.peer_addr,
            fallback_peer_addrs,
            checkpoint_tracking.as_ref(),
            use_ledger_peers,
            peer_snapshot_path,
            tracer,
        );
        let mut attempt_state = peer_attempt_state(node_config.peer_addr, &refreshed_fallback_peers);
        if let Some(peer_addr) = preferred_peer {
            attempt_state.record_success(peer_addr);
        }

        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "refreshed reconnect peer candidates",
            trace_fields([
                ("fallbackPeerCount", json!(refreshed_fallback_peers.len())),
                (
                    "latestSlot",
                    json!(checkpoint_tracking
                        .as_ref()
                        .and_then(|tracking| tracking.ledger_state.tip.slot().map(|slot| slot.0))),
                ),
                ("useLedgerPeers", json!(use_ledger_peers.map(|policy| format!("{policy:?}")))),
            ]),
        );

        let mut session = tokio::select! {
            biased;

            () = &mut shutdown => {
                trace_shutdown_before_bootstrap(tracer);
                return Ok(run_state.finish(from_point, nonce_state, chain_state));
            }

            result = bootstrap_with_attempt_state(node_config, &mut attempt_state, tracer) => result?,
        };

        run_state.record_session(session.connected_peer_addr, &mut had_session);
        if had_session && run_state.reconnect_count > 0 {
            if let Some(m) = metrics {
                m.inc_reconnects();
            }
        }
        preferred_peer = Some(session.connected_peer_addr);

        trace_session_established(
            tracer,
            session.connected_peer_addr,
            run_state.reconnect_count,
            from_point,
        );

        loop {
            let batch_fut = sync_batch_verified(
                &mut session.chain_sync,
                &mut session.block_fetch,
                from_point,
                config.batch_size,
                Some(&config.verification),
            );

            tokio::select! {
                biased;

                () = &mut shutdown => {
                    trace_shutdown_during_session(
                        tracer,
                        session.connected_peer_addr,
                        from_point,
                    );
                    session.mux.abort();
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }

                result = batch_fut => {
                    match result {
                        Ok(progress) => {
                            let applied = apply_verified_progress_to_chaindb(
                                chain_db,
                                &progress,
                                chain_state.as_mut(),
                                checkpoint_tracking.as_mut(),
                                &config.checkpoint_policy,
                            )?;

                            record_verified_batch_progress(
                                &mut from_point,
                                &mut run_state,
                                &progress,
                                nonce_state.as_mut(),
                                config.nonce_config.as_ref(),
                                metrics,
                            );

                            run_state.stable_block_count += applied.stable_block_count;
                            if let Some(m) = metrics {
                                m.add_stable_blocks_promoted(applied.stable_block_count as u64);
                            }

                            if let Some(checkpoint_outcome) = applied.checkpoint_outcome.as_ref() {
                                if let CheckpointPersistenceOutcome::Persisted { slot, .. } = checkpoint_outcome {
                                    if let Some(m) = metrics {
                                        m.set_checkpoint_slot(slot.0);
                                    }
                                }
                                trace_checkpoint_outcome(
                                    tracer,
                                    checkpoint_outcome,
                                    &config.checkpoint_policy,
                                );
                            }

                            trace_verified_sync_batch_applied(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &progress,
                                &run_state,
                                BatchTraceExtras {
                                    stable_block_count: Some(run_state.stable_block_count),
                                    checkpoint_tracked: Some(checkpoint_tracking.is_some()),
                                },
                            );
                        }
                        Err(err) => {
                            let disposition = handle_reconnect_batch_error(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &err,
                            );
                            session.mux.abort();
                            match disposition {
                                BatchErrorDisposition::Reconnect => break,
                                BatchErrorDisposition::Fail => return Err(err),
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn run_reconnecting_verified_sync_service_shared_chaindb_inner<I, V, L, F>(
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    context: ReconnectingVerifiedSyncContext<'_>,
    state: ReconnectingVerifiedSyncState,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncContext {
        node_config,
        fallback_peer_addrs,
        use_ledger_peers,
        peer_snapshot_path,
        config,
        tracer,
        metrics,
    } = context;
    let ReconnectingVerifiedSyncState {
        mut from_point,
        mut nonce_state,
        mut checkpoint_tracking,
    } = state;

    tokio::pin!(shutdown);

    let mut run_state = ReconnectingRunState::new();
    let mut chain_state = config.security_param.map(ChainState::new);
    let mut had_session = false;
    let mut preferred_peer = None;

    loop {
        let refreshed_fallback_peers = refresh_chain_db_reconnect_fallback_peers(
            node_config.peer_addr,
            fallback_peer_addrs,
            checkpoint_tracking.as_ref(),
            use_ledger_peers,
            peer_snapshot_path,
            tracer,
        );
        let mut attempt_state = peer_attempt_state(node_config.peer_addr, &refreshed_fallback_peers);
        if let Some(peer_addr) = preferred_peer {
            attempt_state.record_success(peer_addr);
        }

        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "refreshed reconnect peer candidates",
            trace_fields([
                ("fallbackPeerCount", json!(refreshed_fallback_peers.len())),
                (
                    "latestSlot",
                    json!(checkpoint_tracking
                        .as_ref()
                        .and_then(|tracking| tracking.ledger_state.tip.slot().map(|slot| slot.0))),
                ),
                ("useLedgerPeers", json!(use_ledger_peers.map(|policy| format!("{policy:?}")))),
            ]),
        );

        let mut session = tokio::select! {
            biased;

            () = &mut shutdown => {
                trace_shutdown_before_bootstrap(tracer);
                return Ok(run_state.finish(from_point, nonce_state, chain_state));
            }

            result = bootstrap_with_attempt_state(node_config, &mut attempt_state, tracer) => result?,
        };

        run_state.record_session(session.connected_peer_addr, &mut had_session);
        if had_session && run_state.reconnect_count > 0 {
            if let Some(m) = metrics {
                m.inc_reconnects();
            }
        }
        preferred_peer = Some(session.connected_peer_addr);

        trace_session_established(
            tracer,
            session.connected_peer_addr,
            run_state.reconnect_count,
            from_point,
        );

        loop {
            let batch_fut = sync_batch_verified(
                &mut session.chain_sync,
                &mut session.block_fetch,
                from_point,
                config.batch_size,
                Some(&config.verification),
            );

            tokio::select! {
                biased;

                () = &mut shutdown => {
                    trace_shutdown_during_session(
                        tracer,
                        session.connected_peer_addr,
                        from_point,
                    );
                    session.mux.abort();
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }

                result = batch_fut => {
                    match result {
                        Ok(progress) => {
                            let applied = {
                                let mut chain_db = chain_db.write().map_err(|_| shared_chaindb_lock_error())?;
                                apply_verified_progress_to_chaindb(
                                    &mut *chain_db,
                                    &progress,
                                    chain_state.as_mut(),
                                    checkpoint_tracking.as_mut(),
                                    &config.checkpoint_policy,
                                )?
                            };

                            record_verified_batch_progress(
                                &mut from_point,
                                &mut run_state,
                                &progress,
                                nonce_state.as_mut(),
                                config.nonce_config.as_ref(),
                                metrics,
                            );

                            run_state.stable_block_count += applied.stable_block_count;
                            if let Some(m) = metrics {
                                m.add_stable_blocks_promoted(applied.stable_block_count as u64);
                            }

                            if let Some(checkpoint_outcome) = applied.checkpoint_outcome.as_ref() {
                                if let CheckpointPersistenceOutcome::Persisted { slot, .. } = checkpoint_outcome {
                                    if let Some(m) = metrics {
                                        m.set_checkpoint_slot(slot.0);
                                    }
                                }
                                trace_checkpoint_outcome(
                                    tracer,
                                    checkpoint_outcome,
                                    &config.checkpoint_policy,
                                );
                            }

                            trace_verified_sync_batch_applied(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &progress,
                                &run_state,
                                BatchTraceExtras {
                                    stable_block_count: Some(run_state.stable_block_count),
                                    checkpoint_tracked: Some(checkpoint_tracking.is_some()),
                                },
                            );
                        }
                        Err(err) => {
                            let disposition = handle_reconnect_batch_error(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &err,
                            );
                            session.mux.abort();
                            match disposition {
                                BatchErrorDisposition::Reconnect => break,
                                BatchErrorDisposition::Fail => return Err(err),
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// bootstrap
// ---------------------------------------------------------------------------

/// Connect to an upstream peer and set up all protocol client drivers.
///
/// This is the main runtime entry point for syncing from a remote node.
///
/// # Errors
///
/// Returns `PeerError` if the TCP connection or handshake fails.
pub async fn bootstrap(config: &NodeConfig) -> Result<PeerSession, PeerError> {
    bootstrap_with_fallbacks(config, &[]).await
}

/// Connect to the primary upstream peer, retrying ordered fallbacks on failure.
///
/// The primary address in [`NodeConfig`] is always attempted first. Fallback
/// peers are then tried in the provided order, skipping duplicates.
pub async fn bootstrap_with_fallbacks(
    config: &NodeConfig,
    fallback_peer_addrs: &[SocketAddr],
) -> Result<PeerSession, PeerError> {
    let tracer = NodeTracer::disabled();
    let mut attempt_state = peer_attempt_state(config.peer_addr, fallback_peer_addrs);
    bootstrap_with_attempt_state(config, &mut attempt_state, &tracer).await
}

async fn bootstrap_with_attempt_state(
    config: &NodeConfig,
    attempt_state: &mut PeerAttemptState,
    tracer: &NodeTracer,
) -> Result<PeerSession, PeerError> {
    let proposals: Vec<(HandshakeVersion, NodeToNodeVersionData)> = config
        .protocol_versions
        .iter()
        .map(|v| {
            (
                *v,
                NodeToNodeVersionData {
                    network_magic: config.network_magic,
                    initiator_only_diffusion_mode: false,
                    peer_sharing: 0,
                    query: false,
                },
            )
        })
        .collect();

    let candidate_peer_addrs = attempt_state.attempt_order();

    let mut last_error = None;
    let mut connected_peer_addr = config.peer_addr;
    let mut conn_opt = None;

    for (attempt_index, peer_addr) in candidate_peer_addrs.into_iter().enumerate() {
        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "attempting bootstrap peer",
            trace_fields([
                ("attempt", json!(attempt_index + 1)),
                ("peer", json!(peer_addr.to_string())),
                ("networkMagic", json!(config.network_magic)),
            ]),
        );

        match yggdrasil_network::peer_connect(peer_addr, proposals.clone()).await {
            Ok(conn) => {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Info",
                    "bootstrap peer connected",
                    trace_fields([
                        ("attempt", json!(attempt_index + 1)),
                        ("peer", json!(peer_addr.to_string())),
                    ]),
                );
                connected_peer_addr = peer_addr;
                attempt_state.record_success(peer_addr);
                conn_opt = Some(conn);
                break;
            }
            Err(err) => {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "bootstrap peer failed",
                    trace_fields([
                        ("attempt", json!(attempt_index + 1)),
                        ("peer", json!(peer_addr.to_string())),
                        ("error", json!(err.to_string())),
                    ]),
                );
                last_error = Some(err);
            }
        }
    }

    let mut conn: PeerConnection = match conn_opt {
        Some(conn) => conn,
        None => return Err(last_error.expect("at least one peer candidate")),
    };

    let cs = conn
        .protocols
        .remove(&MiniProtocolNum::CHAIN_SYNC)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing ChainSync protocol handle".into(),
        })?;
    let bf = conn
        .protocols
        .remove(&MiniProtocolNum::BLOCK_FETCH)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing BlockFetch protocol handle".into(),
        })?;
    let ka = conn
        .protocols
        .remove(&MiniProtocolNum::KEEP_ALIVE)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing KeepAlive protocol handle".into(),
        })?;
    let tx = conn
        .protocols
        .remove(&MiniProtocolNum::TX_SUBMISSION)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing TxSubmission protocol handle".into(),
        })?;

    Ok(PeerSession {
        connected_peer_addr,
        chain_sync: ChainSyncClient::new(cs),
        block_fetch: BlockFetchClient::new(bf),
        keep_alive: KeepAliveClient::new(ka),
        tx_submission: TxSubmissionClient::new(tx),
        mux: conn.mux,
        version: conn.version,
        version_data: conn.version_data,
    })
}

/// Run the verified sync loop, reconnecting through ordered bootstrap peers
/// when protocol connectivity is lost.
///
/// The runner preserves the current chain point, nonce evolution state, and
/// optional chain state across reconnects. Only bootstrap, ChainSync, and
/// BlockFetch failures trigger reconnection; decode, verification, and storage
/// failures still return immediately.
pub async fn run_reconnecting_verified_sync_service<S, F>(
    store: &mut S,
    request: ReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    S: VolatileStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    run_reconnecting_verified_sync_service_with_tracer(store, request, &tracer, shutdown).await
}

/// Run the verified sync loop, reconnecting through ordered bootstrap peers
/// while coordinating storage through [`ChainDb`].
pub async fn run_reconnecting_verified_sync_service_chaindb<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    run_reconnecting_verified_sync_service_chaindb_with_tracer(chain_db, request, &tracer, shutdown)
        .await
}

/// Recover ledger state from coordinated storage and then run reconnecting
/// verified sync from the recovered point.
pub async fn resume_reconnecting_verified_sync_service_chaindb<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    resume_reconnecting_verified_sync_service_chaindb_with_tracer(chain_db, request, &tracer, shutdown)
        .await
}

/// Run the reconnecting verified sync loop while emitting runtime trace events.
///
/// Trace emission is driven by the node config-derived [`NodeTracer`] and stays
/// within the node integration layer: bootstrap attempts, successful session
/// establishment, connectivity-triggered reconnects, batch completion, and
/// graceful shutdown are traced, while decode, verification, and storage
/// failures still return immediately.
pub async fn run_reconnecting_verified_sync_service_with_tracer<S, F>(
    store: &mut S,
    request: ReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    S: VolatileStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        mut from_point,
        base_ledger_state: _,
        config,
        mut nonce_state,
        use_ledger_peers: _,
        peer_snapshot_path: _,
    } = request;

    tokio::pin!(shutdown);

    let mut run_state = ReconnectingRunState::new();
    let mut chain_state = config.security_param.map(ChainState::new);
    let mut had_session = false;
    let mut attempt_state = peer_attempt_state(node_config.peer_addr, fallback_peer_addrs);

    loop {
        let mut session = tokio::select! {
            biased;

            () = &mut shutdown => {
                trace_shutdown_before_bootstrap(tracer);
                return Ok(run_state.finish(from_point, nonce_state, chain_state));
            }

            result = bootstrap_with_attempt_state(node_config, &mut attempt_state, tracer) => result?,
        };

        run_state.record_session(session.connected_peer_addr, &mut had_session);

        trace_session_established(
            tracer,
            session.connected_peer_addr,
            run_state.reconnect_count,
            from_point,
        );

        loop {
            let batch_fut = sync_batch_apply_verified(
                &mut session.chain_sync,
                &mut session.block_fetch,
                store,
                from_point,
                config.batch_size,
                Some(&config.verification),
            );

            tokio::select! {
                biased;

                () = &mut shutdown => {
                    trace_shutdown_during_session(
                        tracer,
                        session.connected_peer_addr,
                        from_point,
                    );
                    session.mux.abort();
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }

                result = batch_fut => {
                    match result {
                        Ok(progress) => {
                            record_verified_batch_progress(
                                &mut from_point,
                                &mut run_state,
                                &progress,
                                nonce_state.as_mut(),
                                config.nonce_config.as_ref(),
                                None,
                            );

                            if let Some(ref mut cs) = chain_state {
                                for step in &progress.steps {
                                    run_state.stable_block_count += track_chain_state(cs, step)?;
                                }
                            }

                            trace_verified_sync_batch_applied(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &progress,
                                &run_state,
                                BatchTraceExtras {
                                    stable_block_count: None,
                                    checkpoint_tracked: None,
                                },
                            );
                        }
                        Err(err) => {
                            let disposition = handle_reconnect_batch_error(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &err,
                            );
                            session.mux.abort();
                            match disposition {
                                BatchErrorDisposition::Reconnect => break,
                                BatchErrorDisposition::Fail => return Err(err),
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Recover ledger state from coordinated storage and then run reconnecting
/// verified sync while emitting runtime trace events.
pub async fn resume_reconnecting_verified_sync_service_chaindb_with_tracer<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ResumeReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        base_ledger_state,
        config,
        nonce_state,
        use_ledger_peers,
        peer_snapshot_path,
        metrics,
    } = request;

    let recovery = recover_ledger_state_chaindb(chain_db, base_ledger_state)?;
    tracer.trace_runtime(
        "Node.Recovery",
        "Notice",
        "recovered ledger state from coordinated storage",
        trace_fields([
            ("point", json!(format!("{:?}", recovery.point))),
            ("checkpointSlot", json!(recovery.checkpoint_slot.map(|slot| slot.0))),
            ("replayedVolatileBlocks", json!(recovery.replayed_volatile_blocks)),
        ]),
    );

    let checkpoint_tracking = LedgerCheckpointTracking {
        base_ledger_state: recovery.ledger_state.clone(),
        ledger_state: recovery.ledger_state.clone(),
        last_persisted_point: recovery.point,
        plutus_evaluator: config
            .plutus_cost_model
            .clone()
            .map(crate::plutus_eval::CekPlutusEvaluator::with_cost_model)
            .unwrap_or_default(),
        stake_snapshots: config.nonce_config.as_ref().map(|_| yggdrasil_ledger::StakeSnapshots::new()),
        epoch_size: config.nonce_config.as_ref().map(|nc| nc.epoch_size),
    };

    let sync = run_reconnecting_verified_sync_service_chaindb_inner(
        chain_db,
        ReconnectingVerifiedSyncContext {
            node_config,
            fallback_peer_addrs,
            use_ledger_peers,
            peer_snapshot_path: peer_snapshot_path.as_deref(),
            config,
            tracer,
            metrics,
        },
        ReconnectingVerifiedSyncState {
            from_point: recovery.point,
            nonce_state,
            checkpoint_tracking: Some(checkpoint_tracking),
        },
        shutdown,
    )
    .await?;

    Ok(ResumedSyncServiceOutcome { recovery, sync })
}

pub async fn resume_reconnecting_verified_sync_service_shared_chaindb<I, V, L, F>(
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer(
        chain_db,
        request,
        &tracer,
        shutdown,
    )
    .await
}

pub async fn resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer<I, V, L, F>(
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ResumeReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        base_ledger_state,
        config,
        nonce_state,
        use_ledger_peers,
        peer_snapshot_path,
        metrics,
    } = request;

    let recovery = {
        let chain_db = chain_db.read().map_err(|_| shared_chaindb_lock_error())?;
        recover_ledger_state_chaindb(&chain_db, base_ledger_state)?
    };
    tracer.trace_runtime(
        "Node.Recovery",
        "Notice",
        "recovered ledger state from coordinated storage",
        trace_fields([
            ("point", json!(format!("{:?}", recovery.point))),
            ("checkpointSlot", json!(recovery.checkpoint_slot.map(|slot| slot.0))),
            ("replayedVolatileBlocks", json!(recovery.replayed_volatile_blocks)),
        ]),
    );

    let checkpoint_tracking = LedgerCheckpointTracking {
        base_ledger_state: recovery.ledger_state.clone(),
        ledger_state: recovery.ledger_state.clone(),
        last_persisted_point: recovery.point,
        plutus_evaluator: config
            .plutus_cost_model
            .clone()
            .map(crate::plutus_eval::CekPlutusEvaluator::with_cost_model)
            .unwrap_or_default(),
        stake_snapshots: config.nonce_config.as_ref().map(|_| yggdrasil_ledger::StakeSnapshots::new()),
        epoch_size: config.nonce_config.as_ref().map(|nc| nc.epoch_size),
    };

    let sync = run_reconnecting_verified_sync_service_shared_chaindb_inner(
        chain_db,
        ReconnectingVerifiedSyncContext {
            node_config,
            fallback_peer_addrs,
            use_ledger_peers,
            peer_snapshot_path: peer_snapshot_path.as_deref(),
            config,
            tracer,
            metrics,
        },
        ReconnectingVerifiedSyncState {
            from_point: recovery.point,
            nonce_state,
            checkpoint_tracking: Some(checkpoint_tracking),
        },
        shutdown,
    )
    .await?;

    Ok(ResumedSyncServiceOutcome { recovery, sync })
}

/// Run the reconnecting verified sync loop over coordinated storage while
/// emitting runtime trace events.
pub async fn run_reconnecting_verified_sync_service_chaindb_with_tracer<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        from_point,
        base_ledger_state,
        config,
        nonce_state,
        use_ledger_peers,
        peer_snapshot_path,
    } = request;
    let checkpoint_tracking = {
        let mut ct = crate::sync::default_checkpoint_tracking(
            chain_db,
            base_ledger_state,
            config.plutus_cost_model.clone(),
        )?;
        if let Some(ref nonce_cfg) = config.nonce_config {
            ct.stake_snapshots = Some(yggdrasil_ledger::StakeSnapshots::new());
            ct.epoch_size = Some(nonce_cfg.epoch_size);
        }
        Some(ct)
    };

    run_reconnecting_verified_sync_service_chaindb_inner(
        chain_db,
        ReconnectingVerifiedSyncContext {
            node_config,
            fallback_peer_addrs,
            use_ledger_peers,
            peer_snapshot_path: peer_snapshot_path.as_deref(),
            config,
            tracer,
            metrics: None,
        },
        ReconnectingVerifiedSyncState {
            from_point,
            nonce_state,
            checkpoint_tracking,
        },
        shutdown,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        BatchErrorDisposition, BatchTraceExtras, CheckpointPersistenceOutcome,
        NodeConfig, ReconnectingVerifiedSyncRequest, ResumeReconnectingVerifiedSyncRequest,
        VerifiedSyncServiceConfig,
        ReconnectingRunState, checkpoint_trace_fields, handle_reconnect_batch_error,
        local_root_targets_from_config, record_verified_batch_progress,
        refresh_ledger_peer_sources_from_chain_db,
        seed_peer_registry, session_established_trace_fields, sync_error_trace_fields,
        verified_sync_batch_trace_fields,
    };
    use crate::sync::{MultiEraSyncProgress, SyncError, VerificationConfig};
    use crate::tracer::NodeTracer;
    use crate::sync::LedgerCheckpointPolicy;
    use serde_json::json;
    use std::sync::{Arc, RwLock};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use yggdrasil_consensus::{EpochSize, NonceEvolutionConfig, NonceEvolutionState};
    use yggdrasil_ledger::{
        Era, HeaderHash, LedgerState, Nonce, Point, PoolParams, Relay,
        RewardAccount, SlotNo, StakeCredential, UnitInterval,
    };
    use yggdrasil_network::{
        AfterSlot, BlockFetchClientError, ChainSyncClientError, LocalRootConfig,
        HandshakeVersion, PeerAccessPoint, PeerSource, TopologyConfig, UseBootstrapPeers,
        UseLedgerPeers,
    };
    use yggdrasil_storage::{ChainDb, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile};

    fn local_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    fn sample_node_config() -> NodeConfig {
        NodeConfig {
            peer_addr: local_addr(3001),
            network_magic: 42,
            protocol_versions: vec![HandshakeVersion(15)],
        }
    }

    fn sample_sync_config() -> VerifiedSyncServiceConfig {
        VerifiedSyncServiceConfig {
            batch_size: 1,
            verification: VerificationConfig {
                slots_per_kes_period: 129_600,
                max_kes_evolutions: 62,
                verify_body_hash: true,
            },
            nonce_config: None,
            security_param: None,
            checkpoint_policy: LedgerCheckpointPolicy::default(),
            plutus_cost_model: None,
        }
    }

    fn sample_pool_params(relay: Relay, operator: u8) -> PoolParams {
        PoolParams {
            operator: [operator; 28],
            vrf_keyhash: [operator; 32],
            pledge: 1,
            cost: 1,
            margin: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([operator; 28]),
            },
            pool_owners: vec![[operator; 28]],
            relays: vec![relay],
            pool_metadata: None,
        }
    }

    fn ledger_state_with_pool_relay(peer: SocketAddr) -> LedgerState {
        let mut state = LedgerState::new(Era::Conway);
        state.pool_state_mut().register(sample_pool_params(
            Relay::SingleHostAddr(
                Some(peer.port()),
                Some(match peer.ip() {
                    IpAddr::V4(addr) => addr.octets(),
                    IpAddr::V6(_) => panic!("test peer should be IPv4"),
                }),
                None,
            ),
            7,
        ));
        state
    }

    #[test]
    fn reconnect_request_builder_sets_optional_fields() {
        let node = sample_node_config();
        let cfg = sample_sync_config();
        let path = std::path::PathBuf::from("snapshot.json");

        let req = ReconnectingVerifiedSyncRequest::new(
            &node,
            &[],
            Point::Origin,
            LedgerState::new(Era::Byron),
            &cfg,
        )
        .with_nonce_state(Some(NonceEvolutionState::new(Nonce::Neutral)))
        .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(
            yggdrasil_network::AfterSlot::Always,
        )))
        .with_peer_snapshot_path(Some(path.clone()));

        assert!(req.nonce_state.is_some());
        assert_eq!(
            req.use_ledger_peers,
            Some(UseLedgerPeers::UseLedgerPeers(yggdrasil_network::AfterSlot::Always))
        );
        assert_eq!(req.peer_snapshot_path, Some(path));
    }

    #[test]
    fn reconnect_request_builder_last_call_wins_for_overrides() {
        let node = sample_node_config();
        let cfg = sample_sync_config();
        let first = std::path::PathBuf::from("first.json");
        let second = std::path::PathBuf::from("second.json");

        let req = ReconnectingVerifiedSyncRequest::new(
            &node,
            &[],
            Point::Origin,
            LedgerState::new(Era::Byron),
            &cfg,
        )
        .with_peer_snapshot_path(Some(first))
        .with_peer_snapshot_path(Some(second.clone()))
        .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(
            yggdrasil_network::AfterSlot::Always,
        )))
        .with_use_ledger_peers(None)
        .with_nonce_state(Some(NonceEvolutionState::new(Nonce::Neutral)))
        .with_nonce_state(None);

        assert_eq!(req.peer_snapshot_path, Some(second));
        assert_eq!(req.use_ledger_peers, None);
        assert_eq!(req.nonce_state, None);
    }

    #[test]
    fn resume_request_builder_sets_optional_fields() {
        let node = sample_node_config();
        let cfg = sample_sync_config();
        let path = std::path::PathBuf::from("snapshot.json");
        let metrics = crate::tracer::NodeMetrics::new();

        let req = ResumeReconnectingVerifiedSyncRequest::new(
            &node,
            &[],
            LedgerState::new(Era::Byron),
            &cfg,
        )
        .with_nonce_state(Some(NonceEvolutionState::new(Nonce::Neutral)))
        .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(
            yggdrasil_network::AfterSlot::Always,
        )))
        .with_peer_snapshot_path(Some(path.clone()))
        .with_metrics(Some(&metrics));

        assert!(req.nonce_state.is_some());
        assert_eq!(
            req.use_ledger_peers,
            Some(UseLedgerPeers::UseLedgerPeers(yggdrasil_network::AfterSlot::Always))
        );
        assert_eq!(req.peer_snapshot_path, Some(path));
        assert!(req.metrics.is_some());
    }

    #[test]
    fn resume_request_builder_last_call_wins_for_overrides() {
        let node = sample_node_config();
        let cfg = sample_sync_config();
        let first = std::path::PathBuf::from("first.json");
        let second = std::path::PathBuf::from("second.json");

        let req = ResumeReconnectingVerifiedSyncRequest::new(
            &node,
            &[],
            LedgerState::new(Era::Byron),
            &cfg,
        )
        .with_peer_snapshot_path(Some(first))
        .with_peer_snapshot_path(Some(second.clone()))
        .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(
            yggdrasil_network::AfterSlot::Always,
        )))
        .with_use_ledger_peers(None)
        .with_nonce_state(Some(NonceEvolutionState::new(Nonce::Neutral)))
        .with_nonce_state(None);

        assert_eq!(req.peer_snapshot_path, Some(second));
        assert_eq!(req.use_ledger_peers, None);
        assert_eq!(req.nonce_state, None);
    }

    #[test]
    fn checkpoint_trace_fields_include_persisted_prune_counts() {
        let policy = LedgerCheckpointPolicy {
            min_slot_delta: 2160,
            max_snapshots: 8,
        };
        let fields = checkpoint_trace_fields(
            &CheckpointPersistenceOutcome::Persisted {
                slot: SlotNo(4320),
                retained_snapshots: 8,
                pruned_snapshots: 2,
                rollback_count: 1,
            },
            &policy,
        );

        assert_eq!(fields.get("action"), Some(&json!("persisted")));
        assert_eq!(fields.get("slot"), Some(&json!(4320)));
        assert_eq!(fields.get("retainedSnapshots"), Some(&json!(8)));
        assert_eq!(fields.get("prunedSnapshots"), Some(&json!(2)));
        assert_eq!(fields.get("rollbackCount"), Some(&json!(1)));
        assert_eq!(fields.get("checkpointIntervalSlots"), Some(&json!(2160)));
        assert_eq!(fields.get("maxLedgerSnapshots"), Some(&json!(8)));
    }

    #[test]
    fn checkpoint_trace_fields_include_skip_delta() {
        let policy = LedgerCheckpointPolicy {
            min_slot_delta: 2160,
            max_snapshots: 8,
        };
        let fields = checkpoint_trace_fields(
            &CheckpointPersistenceOutcome::Skipped {
                slot: SlotNo(1200),
                rollback_count: 0,
                since_last_slot_delta: 1200,
            },
            &policy,
        );

        assert_eq!(fields.get("action"), Some(&json!("skipped")));
        assert_eq!(fields.get("slot"), Some(&json!(1200)));
        assert_eq!(fields.get("sinceLastSlotDelta"), Some(&json!(1200)));
        assert_eq!(fields.get("rollbackCount"), Some(&json!(0)));
    }

    #[test]
    fn session_established_trace_fields_include_peer_reconnects_and_point() {
        let fields = session_established_trace_fields(
            local_addr(3001),
            2,
            Point::BlockPoint(SlotNo(42), HeaderHash([7; 32])),
        );

        assert_eq!(fields.get("peer"), Some(&json!("127.0.0.1:3001")));
        assert_eq!(fields.get("reconnectCount"), Some(&json!(2)));
        let from_point = fields
            .get("fromPoint")
            .and_then(|value| value.as_str())
            .expect("fromPoint should be a string");
        assert!(from_point.starts_with("BlockPoint(SlotNo(42), HeaderHash(0707070707070707"));
    }

    #[test]
    fn verified_sync_batch_trace_fields_include_optional_runtime_context() {
        let progress = MultiEraSyncProgress {
            current_point: Point::BlockPoint(SlotNo(21), HeaderHash([5; 32])),
            steps: vec![],
            fetched_blocks: 3,
            rollback_count: 1,
        };
        let mut run_state = ReconnectingRunState::new();
        run_state.record_progress(&progress);
        run_state.stable_block_count = 9;

        let fields = verified_sync_batch_trace_fields(
            local_addr(3002),
            progress.current_point,
            &progress,
            &run_state,
            BatchTraceExtras {
                stable_block_count: Some(run_state.stable_block_count),
                checkpoint_tracked: Some(true),
            },
        );

        assert_eq!(fields.get("peer"), Some(&json!("127.0.0.1:3002")));
        assert_eq!(fields.get("batchFetchedBlocks"), Some(&json!(3)));
        assert_eq!(fields.get("batchRollbacks"), Some(&json!(1)));
        assert_eq!(fields.get("totalBlocks"), Some(&json!(3)));
        assert_eq!(fields.get("batchesCompleted"), Some(&json!(1)));
        assert_eq!(fields.get("stableBlocks"), Some(&json!(9)));
        assert_eq!(fields.get("checkpointTracked"), Some(&json!(true)));
    }

    #[test]
    fn sync_error_trace_fields_include_error_and_point() {
        let fields = sync_error_trace_fields(
            local_addr(3003),
            &SyncError::Recovery("checkpoint gap".to_owned()),
            Point::Origin,
        );

        assert_eq!(fields.get("peer"), Some(&json!("127.0.0.1:3003")));
        assert_eq!(fields.get("currentPoint"), Some(&json!("Origin")));
        assert_eq!(fields.get("error"), Some(&json!("recovery error: checkpoint gap")));
    }

    #[test]
    fn handle_reconnect_batch_error_reconnects_for_connectivity_errors() {
        let tracer = NodeTracer::disabled();

        let chainsync = handle_reconnect_batch_error(
            &tracer,
            local_addr(3004),
            Point::Origin,
            &SyncError::ChainSync(ChainSyncClientError::ConnectionClosed),
        );
        let blockfetch = handle_reconnect_batch_error(
            &tracer,
            local_addr(3005),
            Point::Origin,
            &SyncError::BlockFetch(BlockFetchClientError::ConnectionClosed),
        );

        assert!(matches!(chainsync, BatchErrorDisposition::Reconnect));
        assert!(matches!(blockfetch, BatchErrorDisposition::Reconnect));
    }

    #[test]
    fn handle_reconnect_batch_error_fails_for_non_connectivity_errors() {
        let tracer = NodeTracer::disabled();
        let disposition = handle_reconnect_batch_error(
            &tracer,
            local_addr(3006),
            Point::Origin,
            &SyncError::Recovery("inconsistent checkpoint".to_owned()),
        );

        assert!(matches!(disposition, BatchErrorDisposition::Fail));
    }

    #[test]
    fn reconnecting_run_state_accumulates_progress_and_session_metadata() {
        let mut run_state = ReconnectingRunState::new();
        let mut had_session = false;
        let first_peer = local_addr(3007);
        let second_peer = local_addr(3008);

        run_state.record_session(first_peer, &mut had_session);
        run_state.record_session(second_peer, &mut had_session);
        run_state.record_progress(&MultiEraSyncProgress {
            current_point: Point::Origin,
            steps: vec![],
            fetched_blocks: 2,
            rollback_count: 1,
        });
        run_state.record_progress(&MultiEraSyncProgress {
            current_point: Point::Origin,
            steps: vec![],
            fetched_blocks: 4,
            rollback_count: 0,
        });
        run_state.stable_block_count = 5;

        let outcome = run_state.finish(Point::Origin, None, None);

        assert_eq!(outcome.total_blocks, 6);
        assert_eq!(outcome.total_rollbacks, 1);
        assert_eq!(outcome.batches_completed, 2);
        assert_eq!(outcome.stable_block_count, 5);
        assert_eq!(outcome.reconnect_count, 1);
        assert_eq!(outcome.last_connected_peer_addr, Some(second_peer));
    }

    #[test]
    fn record_verified_batch_progress_updates_point_totals_and_preserves_empty_nonce_state() {
        let progress = MultiEraSyncProgress {
            current_point: Point::BlockPoint(SlotNo(5), HeaderHash([9; 32])),
            steps: vec![],
            fetched_blocks: 4,
            rollback_count: 2,
        };
        let nonce_cfg = NonceEvolutionConfig {
            epoch_size: EpochSize(10),
            stability_window: 100,
            extra_entropy: Nonce::Neutral,
        };
        let mut from_point = Point::Origin;
        let mut run_state = ReconnectingRunState::new();
        let mut nonce_state = NonceEvolutionState::new(Nonce::Neutral);

        record_verified_batch_progress(
            &mut from_point,
            &mut run_state,
            &progress,
            Some(&mut nonce_state),
            Some(&nonce_cfg),
            None,
        );

        assert_eq!(from_point, progress.current_point);
        assert_eq!(run_state.total_blocks, 4);
        assert_eq!(run_state.total_rollbacks, 2);
        assert_eq!(run_state.batches_completed, 1);
        assert_eq!(nonce_state.evolving_nonce, Nonce::Neutral);
    }

    #[test]
    fn seed_peer_registry_preserves_bootstrap_and_local_root_sources() {
        let primary = local_addr(3001);
        let local_root = LocalRootConfig {
            access_points: vec![PeerAccessPoint {
                address: "127.0.0.1".to_owned(),
                port: 3002,
            }],
            advertise: false,
            trustable: true,
            hot_valency: 1,
            warm_valency: Some(1),
            diffusion_mode: Default::default(),
        };
        let topology = TopologyConfig {
            bootstrap_peers: UseBootstrapPeers::UseBootstrapPeers(vec![PeerAccessPoint {
                address: "127.0.0.1".to_owned(),
                port: 3003,
            }]),
            local_roots: vec![local_root],
            public_roots: Vec::new(),
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        };

        let registry = seed_peer_registry(primary, &topology);

        let primary_entry = registry.get(&primary).expect("primary peer present");
        let local_root_entry = registry
            .get(&local_addr(3002))
            .expect("local root peer present");
        let bootstrap_entry = registry
            .get(&local_addr(3003))
            .expect("bootstrap peer present");

        assert!(
            primary_entry
                .sources
                .contains(&PeerSource::PeerSourceBootstrap)
        );
        assert!(
            local_root_entry
                .sources
                .contains(&PeerSource::PeerSourceLocalRoot)
        );
        assert!(
            bootstrap_entry
                .sources
                .contains(&PeerSource::PeerSourceBootstrap)
        );
    }

    #[test]
    fn refresh_ledger_peer_sources_uses_supplied_base_ledger_state() {
        let relay_peer = local_addr(3010);
        let base_ledger_state = ledger_state_with_pool_relay(relay_peer);
        let chain_db = Arc::new(RwLock::new(ChainDb::new(
            InMemoryImmutable::default(),
            InMemoryVolatile::default(),
            InMemoryLedgerStore::default(),
        )));
        let topology = TopologyConfig {
            use_ledger_peers: UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
            ..TopologyConfig::default()
        };
        let tracer = NodeTracer::disabled();
        let mut registry = yggdrasil_network::PeerRegistry::default();

        let changed = refresh_ledger_peer_sources_from_chain_db(
            &mut registry,
            &chain_db,
            &base_ledger_state,
            &topology,
            &tracer,
        );

        assert!(changed);
        let entry = registry
            .get(&relay_peer)
            .expect("ledger-derived relay peer should be present");
        assert!(entry.sources.contains(&PeerSource::PeerSourceLedger));
    }

    #[test]
    fn local_root_targets_use_effective_warm_valency() {
        let local_roots = vec![LocalRootConfig {
            access_points: vec![PeerAccessPoint {
                address: "127.0.0.1".to_owned(),
                port: 4001,
            }],
            advertise: false,
            trustable: false,
            hot_valency: 2,
            warm_valency: None,
            diffusion_mode: Default::default(),
        }];

        let targets = local_root_targets_from_config(&local_roots);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].hot_valency, 2);
        assert_eq!(targets[0].warm_valency, 2);
        assert_eq!(targets[0].peers, vec![local_addr(4001)]);
    }
}
