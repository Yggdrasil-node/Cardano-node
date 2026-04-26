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

use crate::block_producer::{
    BlockProducerCredentials, ForgedBlock, ShouldForge, SlotClock, assemble_block_body,
    check_should_forge, forge_block, forged_block_to_storage_block, make_block_context,
    serialize_forged_block_cbor,
};
use crate::config::load_peer_snapshot_file;
use crate::sync::{
    LedgerCheckpointTracking, LedgerCheckpointUpdateOutcome, LedgerRecoveryOutcome,
    MultiEraSyncProgress, MultiEraSyncStep, SyncError, TypedIntersectResult,
    VerifiedSyncServiceConfig, VrfVerificationContext, apply_nonce_evolution_to_progress,
    apply_verified_progress_to_chaindb, decode_multi_era_block, extract_consumed_inputs,
    extract_tx_ids, multi_era_block_to_block, recover_ledger_state_chaindb,
    sync_batch_apply_verified, sync_batch_verified_with_tentative, track_chain_state,
    typed_find_intersect, validate_block_body_size, validate_block_protocol_version,
    verify_block_body_hash,
};
use crate::tracer::{NodeMetrics, NodeTracer, trace_fields};
use serde_json::Value;
use serde_json::json;
use yggdrasil_consensus::praos::ActiveSlotCoeff;
use yggdrasil_consensus::{
    ChainState, NonceEvolutionConfig, NonceEvolutionState, TentativeState, kes_period_of_slot,
};
use yggdrasil_ledger::{
    BlockNo, Decoder, EpochBoundaryEvent, HeaderHash, LedgerError, LedgerState,
    MultiEraSubmittedTx, Nonce, Point, PoolRelayAccessPoint, ShelleyTxIn, SlotNo, TxId,
    plutus_validation::PlutusEvaluator,
};
use yggdrasil_mempool::{
    MEMPOOL_ZERO_IDX, Mempool, MempoolEntry, MempoolError, MempoolIdx, MempoolSnapshot,
    SharedMempool, SharedTxState, SharedTxSubmissionMempoolReader, TxSubmissionMempoolReader,
};
use yggdrasil_network::{
    AbstractState, AcquireOutboundResult, AfterSlot, BlockFetchClient, ChainSyncClient, CmAction,
    ConnectionManagerState, ConsensusLedgerPeerInputs, ConsensusLedgerPeerSource, ConsensusMode,
    ControlMessage, DataFlow, DnsRefreshPolicy, DnsRootPeerProvider, GovernorAction,
    GovernorState, GovernorTargets, HandshakeVersion, KeepAliveClient, KeepAliveClientError,
    LedgerPeerSnapshot,
    LedgerPeerUseDecision, LedgerStateJudgement, LiveLedgerPeerRefreshObservation,
    LocalRootConfig, LocalRootTargets,
    MiniProtocolNum, NodePeerSharing, NodeToNodeVersionData, PeerAccessPoint, PeerAttemptState,
    PeerConnection, PeerError, PeerRegistry, PeerSelectionCounters, PeerSelectionTimeouts,
    PeerSharingClient, PeerSnapshotFileObservation, PeerSnapshotFileSource, PeerSnapshotFreshness,
    PeerSource, PeerStateAction, PeerStatus, ReleaseOutboundResult, RootPeerProviderState,
    TemperatureBundle, TopologyConfig, TxIdAndSize, TxServerRequest, TxSubmissionClient,
    TxSubmissionClientError, UseLedgerPeers, churn_mode_from_fetch_mode, compute_association_mode,
    derive_peer_snapshot_freshness, eligible_ledger_peer_candidates, fetch_mode_from_judgement,
    governor_action_to_peer_state_action, live_refresh_ledger_peer_registry_observed,
    merge_ledger_peer_snapshots, peer_attempt_state, peer_selection_mode, pick_churn_regime,
    refresh_root_peer_state_and_registry, resolve_peer_access_points,
};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

/// Notification used to wake ChainSync servers when the chain tip advances.
/// This is the Rust equivalent of the upstream ChainDB follower notification
/// mechanism, allowing servers to block efficiently instead of busy-polling.
pub type ChainTipNotify = Arc<tokio::sync::Notify>;

/// Shared block-producer state updated by the sync pipeline so the producer
/// loop reads live epoch nonce and stake sigma values across block forging.
///
/// Reference: upstream `forkBlockForging` in `NodeKernel.hs` re-reads the
/// ledger view's epoch nonce and per-pool relative stake each slot.
#[derive(Clone, Debug, Default)]
pub struct SharedBlockProducerState {
    /// Current epoch nonce available to the block producer.
    pub epoch_nonce: Option<Nonce>,
    /// Current delegated stake sigma (numerator / denominator) available to the block producer.
    pub sigma: Option<(u64, u64)>,
}

/// Update the shared block-producer state with the latest epoch nonce from
/// the nonce evolution state machine.
///
/// Called after each sync batch applies nonce evolution, so the concurrent
/// block producer loop observes the live nonce without polling the sync
/// pipeline.
///
/// Reference: upstream `forkBlockForging` reads `currentSlot`'s ledger view
/// epoch nonce on every slot tick.
fn update_bp_state_nonce(
    bp_state: &Option<Arc<RwLock<SharedBlockProducerState>>>,
    nonce_state: Option<&NonceEvolutionState>,
) {
    if let (Some(bp), Some(ns)) = (bp_state.as_ref(), nonce_state) {
        if let Ok(mut st) = bp.write() {
            st.epoch_nonce = Some(ns.epoch_nonce);
        }
    }
}

/// Update the shared block-producer state with the pool's relative stake
/// from the active (set) stake snapshot.
///
/// `pool_key_hash` is the Blake2b-224 hash of the block producer's cold
/// verification key (`issuer_vkey`).
///
/// The `set` snapshot is the one active for leader election in the current
/// epoch (upstream: `esNesPd . nesEs`).
///
/// Reference: upstream `forkBlockForging` reads `IndividualPoolStake` from
/// the epoch's stake distribution on every slot tick.
fn update_bp_state_sigma(
    bp_state: &Option<Arc<RwLock<SharedBlockProducerState>>>,
    stake_snapshots: Option<&yggdrasil_ledger::StakeSnapshots>,
    pool_key_hash: &[u8; 28],
) {
    if let (Some(bp), Some(snapshots)) = (bp_state.as_ref(), stake_snapshots) {
        let dist = snapshots.set.pool_stake_distribution();
        let sigma = dist.relative_stake(pool_key_hash);
        if let Ok(mut st) = bp.write() {
            st.sigma = Some(sigma);
        }
    }
}

/// Runtime governor configuration derived from node configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeGovernorConfig {
    /// Period between governor evaluation ticks.
    pub tick_interval: Duration,
    /// KeepAlive cadence for established warm peers.
    pub keepalive_interval: Option<Duration>,
    /// Node-level peer-sharing willingness for governor association mode.
    pub peer_sharing: NodePeerSharing,
    /// Consensus mode used to derive governor churn regime.
    pub consensus_mode: ConsensusMode,
    /// Target peer counts maintained by the governor.
    pub targets: GovernorTargets,
}

impl RuntimeGovernorConfig {
    /// Construct a runtime governor config from the explicit interval and targets.
    pub fn new(
        tick_interval: Duration,
        keepalive_interval: Option<Duration>,
        peer_sharing: NodePeerSharing,
        consensus_mode: ConsensusMode,
        targets: GovernorTargets,
    ) -> Self {
        Self {
            tick_interval,
            keepalive_interval,
            peer_sharing,
            consensus_mode,
            targets,
        }
    }
}

/// Runtime block-producer configuration derived from node configuration.
#[derive(Clone, Debug)]
pub struct RuntimeBlockProducerConfig {
    /// Slot duration used by the local slot clock.
    pub slot_length: Duration,
    /// Active slot coefficient `f` used for Praos leader checks.
    pub active_slot_coeff: ActiveSlotCoeff,
    /// Relative stake numerator for the forging key (sigma numerator).
    pub sigma_num: u64,
    /// Relative stake denominator for the forging key (sigma denominator).
    pub sigma_den: u64,
    /// Epoch nonce used for leader checks.
    pub epoch_nonce: Nonce,
    /// Maximum aggregate block-body size in bytes.
    pub max_block_body_size: u32,
    /// Protocol version inserted into forged headers.
    pub protocol_version: (u64, u64),
}

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
    // Keep forge-body assembly deterministic and fee-ordered.
    entries.sort_by(|left, right| right.fee.cmp(&left.fee));
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

    let decoded_block = multi_era_block_to_block(&decoded);
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
const HOT_WEIGHT_CHAIN_SYNC: u8 = 3;
const HOT_WEIGHT_BLOCK_FETCH: u8 = 2;

fn apply_hot_weights(weights: &[(MiniProtocolNum, yggdrasil_network::WeightHandle)]) {
    for (proto, handle) in weights {
        let w = match *proto {
            MiniProtocolNum::CHAIN_SYNC => HOT_WEIGHT_CHAIN_SYNC,
            MiniProtocolNum::BLOCK_FETCH => HOT_WEIGHT_BLOCK_FETCH,
            _ => yggdrasil_network::DEFAULT_PROTOCOL_WEIGHT,
        };
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
            Some(mut session) => {
                apply_control_close(&mut session.control);
                session.abort();
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
    fn promote_to_hot(&mut self, peer: SocketAddr) -> bool {
        match self.warm_peers.get_mut(&peer) {
            Some(managed) if !managed.is_hot => {
                managed.is_hot = true;
                apply_control_activate(&mut managed.control);
                // Boost hot-tier protocol weights so ChainSync and BlockFetch
                // get proportionally more egress bandwidth.
                apply_hot_weights(&managed.session.protocol_weights);
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
    ) {
        let hot_peers: Vec<SocketAddr> = self
            .warm_peers
            .iter()
            .filter(|(_, m)| m.is_hot)
            .map(|(addr, _)| *addr)
            .collect();

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
    registry.sync_root_peers(&topology.resolved_root_providers());
    // Insert the primary peer after syncing root peers so that sync_root_peers
    // (which clears all Bootstrap/LocalRoot/PublicRoot sources first) does not
    // remove the primary peer's Bootstrap source when the primary is not listed
    // in the topology bootstrap set.
    registry.insert_source(primary_peer, PeerSource::PeerSourceBootstrap);
    registry
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
struct ChainDbConsensusLedgerSource<'a, I, V, L>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    chain_db: &'a Arc<RwLock<ChainDb<I, V, L>>>,
    base_ledger_state: &'a LedgerState,
    tracer: &'a NodeTracer,
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
        match recover_ledger_state_chaindb(&chain_db, self.base_ledger_state.clone()) {
            Ok(recovery) => ConsensusLedgerPeerInputs {
                latest_slot: point_slot(&recovery.point).or_else(|| point_slot(&tip)),
                judgement: LedgerStateJudgement::YoungEnough,
                ledger_snapshot: ledger_peer_snapshot_from_ledger_state(&recovery.ledger_state),
            },
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

fn refresh_ledger_peer_sources_from_chain_db<I, V, L>(
    registry: &mut PeerRegistry,
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    base_ledger_state: &LedgerState,
    topology: &TopologyConfig,
    tracer: &NodeTracer,
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
            trace_fields([("decision", json!(format!("{:?}", observation.update.decision)))]),
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

async fn apply_cm_actions(
    peer_manager: &mut OutboundPeerManager,
    peer_registry: &Arc<RwLock<PeerRegistry>>,
    connection_manager: &Arc<RwLock<ConnectionManagerState>>,
    governor_state: &mut GovernorState,
    node_config: &NodeConfig,
    actions: Vec<CmAction>,
    tracer: &NodeTracer,
) -> bool {
    let mut changed = false;
    for cm_action in actions {
        match cm_action {
            CmAction::StartConnect(peer) => {
                if peer_manager
                    .promote_to_warm(node_config, peer, governor_state, tracer)
                    .await
                {
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
                            let _ = peer_manager.demote_to_cold(peer);
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
                let connection_changed = peer_manager.demote_to_cold(peer);
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
                    let connection_changed = peer_manager.demote_to_cold(peer);
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

/// Run the local block-producer loop until shutdown.
///
/// The loop advances a relative slot clock, evaluates Praos leadership using
/// loaded block-producer credentials, assembles a block body from the current
/// fee-ordered mempool snapshot, forges/signs a header, and inserts the new
/// block into volatile ChainDb storage.
#[allow(clippy::too_many_arguments)]
pub async fn run_block_producer_loop<I, V, L, F>(
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    mut credentials: BlockProducerCredentials,
    config: RuntimeBlockProducerConfig,
    _tip_notify: Option<ChainTipNotify>,
    bp_state: Option<Arc<RwLock<SharedBlockProducerState>>>,
    tracer: NodeTracer,
    metrics: Option<Arc<NodeMetrics>>,
    shutdown: F,
) where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let (tip_slot, _, _) = {
        let db = chain_db.read().expect("chain db lock poisoned");
        tip_context_from_chain_db(&db)
    };
    let anchor_slot = tip_slot
        .map(|slot| SlotNo(slot.0.saturating_add(1)))
        .unwrap_or(SlotNo(0));
    let slot_clock = SlotClock::new(anchor_slot, config.slot_length);

    let mut interval = tokio::time::interval(config.slot_length);
    let mut last_checked_slot: Option<SlotNo> = None;
    let mut last_kes_warning_period: Option<u64> = None;
    tokio::pin!(shutdown);

    tracer.trace_runtime(
        "Node.BlockProduction",
        "Notice",
        "block producer loop started",
        trace_fields([
            ("anchorSlot", json!(anchor_slot.0)),
            ("slotLengthSecs", json!(config.slot_length.as_secs())),
        ]),
    );

    loop {
        tokio::select! {
            biased;

            () = &mut shutdown => {
                tracer.trace_runtime(
                    "Node.BlockProduction",
                    "Notice",
                    "block producer loop stopped",
                    std::collections::BTreeMap::new(),
                );
                return;
            }

            _ = interval.tick() => {
                let current_slot = slot_clock.current_slot();
                if last_checked_slot
                    .map(|last| current_slot <= last)
                    .unwrap_or(false)
                {
                    continue;
                }
                last_checked_slot = Some(current_slot);

                if let Some(kes) = kes_expiry_warning(&credentials, current_slot) {
                    if last_kes_warning_period != Some(kes.current_period) {
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Warning",
                            "operational certificate nearing KES expiry",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("currentKesPeriod", json!(kes.current_period)),
                                ("certStartKesPeriod", json!(kes.cert_start_period)),
                                ("certEndKesPeriod", json!(kes.cert_end_period)),
                                ("remainingKesPeriods", json!(kes.remaining_periods)),
                                ("remainingKesSlots", json!(kes.remaining_slots)),
                            ]),
                        );
                        last_kes_warning_period = Some(kes.current_period);
                    }
                }

                let (tip_slot, tip_block_no, tip_hash) = {
                    let db = chain_db.read().expect("chain db lock poisoned");
                    tip_context_from_chain_db(&db)
                };

                let Some(context) = make_block_context(
                    current_slot,
                    tip_slot,
                    tip_block_no,
                    tip_hash,
                ) else {
                    // Upstream: TraceSlotIsImmutable — emitted when the
                    // current slot is not strictly ahead of the chain tip
                    // slot, meaning forging would target an immutable or
                    // already-occupied slot. The forge loop must skip this
                    // slot rather than silently dropping it from the trace
                    // record.
                    //
                    // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                    // `TraceSlotIsImmutable` and `NodeKernel.forkBlockForging`
                    // (`mkCurrentBlockContext` returning `Left ImmutableSlot`).
                    tracer.trace_runtime(
                        "Node.BlockProduction",
                        "Warning",
                        "slot is immutable",
                        trace_fields([
                            ("slot", json!(current_slot.0)),
                            ("tipSlot", json!(tip_slot.map(|s| s.0))),
                        ]),
                    );
                    continue;
                };

                // Upstream: TraceStartLeadershipCheck — emitted at the start
                // of every slot's leadership check, before the VRF/KES
                // evaluation. Operators rely on this event for per-slot
                // forge-loop liveness monitoring.
                //
                // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                // `TraceStartLeadershipCheck` and `NodeKernel.forkBlockForging`
                // (`traceWith tracer (TraceStartLeadershipCheck currentSlot)`).
                tracer.trace_runtime(
                    "Node.BlockProduction",
                    "Debug",
                    "starting leadership check",
                    trace_fields([
                        ("slot", json!(current_slot.0)),
                        ("blockNo", json!(context.block_number.0)),
                    ]),
                );

                // Read live epoch nonce and sigma from the shared state
                // updated by the sync pipeline, falling back to the static
                // startup values in config when unavailable.
                //
                // Reference: upstream `forkBlockForging` re-reads the ledger
                // view's epoch nonce and per-pool relative stake each slot.
                let (live_nonce, live_sigma_num, live_sigma_den) = {
                    let bp_snapshot = bp_state
                        .as_ref()
                        .and_then(|bp| bp.read().ok().map(|st| st.clone()));
                    let nonce = bp_snapshot
                        .as_ref()
                        .and_then(|s| s.epoch_nonce)
                        .unwrap_or(config.epoch_nonce);
                    let (sn, sd) = bp_snapshot
                        .as_ref()
                        .and_then(|s| s.sigma)
                        .unwrap_or((config.sigma_num, config.sigma_den));
                    (nonce, sn, sd)
                };

                let should_forge = check_should_forge(
                    &mut credentials,
                    current_slot,
                    live_nonce,
                    live_sigma_num,
                    live_sigma_den,
                    &config.active_slot_coeff,
                );

                let election = match should_forge {
                    ShouldForge::NotLeader => {
                        // Upstream: TraceNodeNotLeader — emitted whenever
                        // the slot leadership check determined the node is
                        // not the elected leader for this slot. Kept at
                        // Debug severity to match upstream's high-frequency
                        // per-slot tracing.
                        //
                        // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                        // `TraceNodeNotLeader` and `NodeKernel.forkBlockForging`.
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Debug",
                            "not slot leader",
                            trace_fields([("slot", json!(current_slot.0))]),
                        );
                        continue;
                    }
                    ShouldForge::ForgeStateUpdateError(err) => {
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Warning",
                            "forge-state update failed",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("error", json!(err)),
                            ]),
                        );
                        continue;
                    }
                    ShouldForge::CannotForge(err) => {
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Warning",
                            "cannot forge in elected slot",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("error", json!(err)),
                            ]),
                        );
                        continue;
                    }
                    ShouldForge::ShouldForge(election) => election,
                };

                // Upstream: TraceNodeIsLeader — emitted once leader election
                // has succeeded for this slot and before block construction
                // begins. Operators rely on this event to count elected
                // slots and reconcile against `TraceForgedBlock` /
                // `TraceAdoptedBlock`.
                //
                // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                // `TraceNodeIsLeader` and `NodeKernel.forkBlockForging`.
                tracer.trace_runtime(
                    "Node.BlockProduction",
                    "Notice",
                    "elected as slot leader",
                    trace_fields([
                        ("slot", json!(current_slot.0)),
                        ("blockNo", json!(context.block_number.0)),
                    ]),
                );

                let entries = mempool_entries_for_forging(&mempool);
                let (selected_preview, selected_size) =
                    assemble_block_body(entries.iter(), config.max_block_body_size);
                let selected_count = selected_preview.len();

                let issuer_vkey = credentials.issuer_vkey.clone();

                let forged = match forge_block(
                    &credentials,
                    &election,
                    &context,
                    current_slot,
                    &entries,
                    config.max_block_body_size,
                    issuer_vkey,
                    config.protocol_version,
                ) {
                    Ok(forged) => forged,
                    Err(err) => {
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Error",
                            "failed to forge block",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("blockNo", json!(context.block_number.0)),
                                ("error", json!(err.to_string())),
                            ]),
                        );
                        continue;
                    }
                };

                if let Err(err) = self_validate_forged_block(&forged) {
                    // Upstream: TraceForgedInvalidBlock — emitted at
                    // Critical severity when a locally forged block fails
                    // self-validation (protocol-version, body-hash,
                    // body-size, or header-identity check). This is more
                    // serious than a peer's invalid block: it indicates a
                    // local mempool/validation inconsistency that produced
                    // a malformed block, and operators must investigate.
                    //
                    // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                    // `TraceForgedInvalidBlock` and `NodeKernel.forkBlockForging`
                    // (post-forge `getIsInvalidBlock` check).
                    tracer.trace_runtime(
                        "Node.BlockProduction",
                        "Critical",
                        "forged invalid block (self-validation failed)",
                        trace_fields([
                            ("slot", json!(forged.slot.0)),
                            ("blockNo", json!(forged.block_number.0)),
                            ("headerHash", json!(hex::encode(forged.header_hash.0))),
                            ("error", json!(err.to_string())),
                        ]),
                    );
                    continue;
                }

                let storage_block = forged_block_to_storage_block(&forged);
                let add_result = {
                    let mut db = chain_db.write().expect("chain db lock poisoned");
                    db.add_volatile_block(storage_block)
                };

                match add_result {
                    Ok(()) => {
                        // Upstream: TraceForgedBlock — always emitted after
                        // successful Block.forgeBlock.
                        // Reference: NodeKernel.hs forkBlockForging ~line 735
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Notice",
                            "forged local block",
                            trace_fields([
                                ("slot", json!(forged.slot.0)),
                                ("blockNo", json!(forged.block_number.0)),
                                ("txCount", json!(selected_count)),
                                ("bodySize", json!(selected_size)),
                                ("headerHash", json!(hex::encode(forged.header_hash.0))),
                            ]),
                        );

                        // -- Post-forge adoption check --
                        // Upstream: NodeKernel.hs ~lines 746-785
                        // After adding the block, check whether our block
                        // became the new tip of the chain.
                        //
                        // In upstream Haskell this uses addBlockAsync +
                        // blockProcessed (STM wait) + getIsInvalidBlock.
                        // Our storage is synchronous so we can check
                        // immediately after add_volatile_block().
                        let adopted = {
                            let db = chain_db.read().expect("chain db lock poisoned");
                            match db.tip() {
                                Point::BlockPoint(tip_s, tip_h) => {
                                    tip_s == forged.slot && tip_h == forged.header_hash
                                }
                                Point::Origin => false,
                            }
                        };

                        if adopted {
                            // Upstream: TraceAdoptedBlock — block adopted
                            // successfully, normal path.
                            let confirmed_ids = forged
                                .transactions
                                .iter()
                                .map(|tx| tx.tx_id)
                                .collect::<Vec<_>>();
                            let removed = if confirmed_ids.is_empty() {
                                0
                            } else {
                                mempool.remove_confirmed(&confirmed_ids)
                            };

                            if let Some(ref m) = metrics {
                                m.add_blocks_synced(1);
                                m.set_current_slot(forged.slot.0);
                                m.set_current_block_number(forged.block_number.0);
                                m.set_mempool_gauges(mempool.len() as u64, mempool.size_bytes() as u64);
                            }

                            tracer.trace_runtime(
                                "Node.BlockProduction",
                                "Notice",
                                "adopted forged block",
                                trace_fields([
                                    ("slot", json!(forged.slot.0)),
                                    ("blockNo", json!(forged.block_number.0)),
                                    ("txCount", json!(selected_count)),
                                    ("mempoolEvicted", json!(removed)),
                                    ("headerHash", json!(hex::encode(forged.header_hash.0))),
                                ]),
                            );

                            // Wake ChainSync servers so they can push the
                            // new header to connected peers immediately
                            // without busy-polling.
                            if let Some(ref notify) = _tip_notify {
                                notify.notify_waiters();
                            }
                        } else {
                            // Upstream: TraceDidntAdoptBlock — block was
                            // valid but not adopted (another leader's block
                            // was preferred by chain selection).
                            //
                            // This is a warning-level event: it means a
                            // competing slot leader's block was adopted
                            // instead.  If our storage had an invalid-block
                            // set we would also check getIsInvalidBlock and
                            // emit TraceForgedInvalidBlock (critical) for
                            // mempool/validation inconsistencies.
                            tracer.trace_runtime(
                                "Node.BlockProduction",
                                "Warning",
                                "did not adopt forged block",
                                trace_fields([
                                    ("slot", json!(forged.slot.0)),
                                    ("blockNo", json!(forged.block_number.0)),
                                    ("headerHash", json!(hex::encode(forged.header_hash.0))),
                                ]),
                            );
                        }
                    }
                    Err(err) => {
                        // Upstream: FailedToAddBlock — the block could not
                        // be added to ChainDB at all.
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Warning",
                            "failed to persist forged block",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("blockNo", json!(context.block_number.0)),
                                ("error", json!(err.to_string())),
                            ]),
                        );
                    }
                }
            }
        }
    }
}

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
    let mut peer_manager = OutboundPeerManager::new();
    let mut root_sources = RuntimeRootPeerSources::new(&topology);
    let timeouts = PeerSelectionTimeouts::default();
    governor_state.enable_root_big_ledger_requests = true;
    governor_state.inbound_peers_retry_delay = timeouts.inbound_peers_retry_delay;
    governor_state.max_inbound_peers = timeouts.max_inbound_peers;
    tokio::pin!(shutdown);

    {
        let mut registry = peer_registry.write().expect("peer registry lock poisoned");
        root_sources.sync_registry(&mut registry);
        let _ = refresh_ledger_peer_sources_from_chain_db(
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

                peer_manager
                    .drive_keepalives(config.keepalive_interval, &mut governor_state, &tracer)
                    .await;

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
                    )
                };

                peer_manager
                    .refresh_hot_peer_tips(&peer_registry, &mut governor_state, &tracer)
                    .await;

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
                let actions = {
                    let registry = peer_registry.read().expect("peer registry lock poisoned");
                    let actions = governor_state.tick(
                        &registry,
                        &config.targets,
                        &local_root_groups,
                        selection_mode,
                        association_mode,
                        Instant::now(),
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

                    actions
                };

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
                                if peer_manager.promote_to_hot(peer) {
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
                                let _ = peer_manager.demote_to_cold(peer);
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
                                    root_sources.refresh(&mut registry, &tracer)
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
    evaluator: Option<&dyn PlutusEvaluator>,
    mut insert_entry: F,
) -> Result<MempoolAddTxResult, MempoolAddTxError>
where
    F: FnMut(
        MempoolEntry,
        Option<&yggdrasil_ledger::ProtocolParameters>,
    ) -> Result<(), MempoolError>,
{
    let tx_id = tx.tx_id();
    let mut staged_ledger = ledger.clone();
    match staged_ledger.apply_submitted_tx(&tx, current_slot, evaluator) {
        Ok(()) => {
            insert_entry(admitted_entry(tx), staged_ledger.protocol_params())?;
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
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<MempoolAddTxResult, MempoolAddTxError> {
    add_tx_with(
        ledger,
        tx,
        current_slot,
        evaluator,
        |entry, protocol_params| mempool.insert_checked(entry, current_slot, protocol_params),
    )
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
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<MempoolAddTxResult, MempoolAddTxError> {
    add_tx_with(
        ledger,
        tx,
        current_slot,
        evaluator,
        |entry, protocol_params| mempool.insert_checked(entry, current_slot, protocol_params),
    )
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
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<Vec<MempoolAddTxResult>, MempoolAddTxError>
where
    I: IntoIterator<Item = MultiEraSubmittedTx>,
{
    txs.into_iter()
        .map(|tx| add_tx_to_mempool(ledger, mempool, tx, current_slot, evaluator))
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
    evaluator: Option<&dyn PlutusEvaluator>,
) -> Result<Vec<MempoolAddTxResult>, MempoolAddTxError>
where
    I: IntoIterator<Item = MultiEraSubmittedTx>,
{
    txs.into_iter()
        .map(|tx| add_tx_to_shared_mempool(ledger, mempool, tx, current_slot, evaluator))
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
            // Build an index of the requested ids in one pass over the
            // mempool (O(n)) instead of doing a linear scan per requested
            // id (O(n*m)). The reply preserves the requested order; missing
            // ids are silently skipped, matching upstream
            // `Ouroboros.Network.TxSubmission.Outbound.txSubmissionOutbound`.
            use std::collections::{HashMap, HashSet};
            let requested: HashSet<TxId> = txids.iter().copied().collect();
            let by_id: HashMap<TxId, &Vec<u8>> = mempool
                .iter()
                .filter(|entry| requested.contains(&entry.tx_id))
                .map(|entry| (entry.tx_id, &entry.raw_tx))
                .collect();
            let txs: Vec<Vec<u8>> = txids
                .into_iter()
                .filter_map(|txid| by_id.get(&txid).map(|raw| (*raw).clone()))
                .collect();
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
    /// Peer-sharing wire value for handshake proposals (0 = disabled, >=1 = enabled).
    pub peer_sharing: u8,
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
    /// Optional PeerSharing client driver when negotiated with the peer.
    pub peer_sharing: Option<PeerSharingClient>,
    /// Mux handle — abort to tear down the connection.
    pub mux: yggdrasil_network::MuxHandle,
    /// Negotiated protocol version.
    pub version: HandshakeVersion,
    /// Agreed-upon version data.
    pub version_data: NodeToNodeVersionData,
    /// Per-protocol egress weight handles for dynamic scheduling adjustment.
    /// Stored as `(MiniProtocolNum, WeightHandle)` tuples.
    pub protocol_weights: Vec<(MiniProtocolNum, yggdrasil_network::WeightHandle)>,
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
    /// Optional shared tentative-header state used for diffusion pipelining.
    pub tentative_state: Option<Arc<RwLock<TentativeState>>>,
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
            tentative_state: None,
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

    /// Provide optional shared tentative-header state for diffusion pipelining.
    pub fn with_tentative_state(
        mut self,
        tentative_state: Option<Arc<RwLock<TentativeState>>>,
    ) -> Self {
        self.tentative_state = tentative_state;
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
    /// Optional shared peer registry for reading governor-managed hot peers
    /// at reconnect time. When present the reconnect loop prefers hot peers
    /// as sync candidates.
    pub peer_registry: Option<Arc<RwLock<PeerRegistry>>>,
    /// Optional shared mempool for evicting confirmed transactions during
    /// sync roll-forward and re-admitting rolled-back transactions.
    pub mempool: Option<SharedMempool>,
    /// Optional shared tentative-header state used for diffusion pipelining.
    pub tentative_state: Option<Arc<RwLock<TentativeState>>>,
    /// Optional chain-tip notification channel.  When present, the sync
    /// pipeline fires `notify_waiters()` after each successful batch
    /// application so inbound ChainSync servers can push updates without
    /// busy-looping.
    pub tip_notify: Option<ChainTipNotify>,
    /// Optional shared block-producer state for live epoch nonce and sigma
    /// updates.  When present, the sync pipeline pushes updated nonce after
    /// each batch and updated sigma after epoch boundary events.
    ///
    /// Reference: upstream `forkBlockForging` reads the ledger view's
    /// epoch nonce and per-pool relative stake on every slot.
    pub bp_state: Option<Arc<RwLock<SharedBlockProducerState>>>,
    /// Blake2b-224 hash of the block producer's cold verification key,
    /// used to look up the pool's relative stake in the stake distribution.
    pub bp_pool_key_hash: Option<[u8; 28]>,
    /// Optional shared TxSubmission inbound dedup state.  When present,
    /// confirmed TxIds from each roll-forward batch are recorded via
    /// [`SharedTxState::mark_confirmed`] so inbound peers stop re-fetching
    /// transactions that are already on-chain.  Mirrors upstream
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.State` `bufferedTxs`
    /// population on confirmation.
    pub inbound_tx_state: Option<SharedTxState>,
    /// Optional storage directory under which the OpCert counter sidecar
    /// (`ocert_counters.cbor`) is persisted whenever a ledger checkpoint
    /// is written. When `None`, the counters are process-local — same
    /// behavior as before this slice. When `Some(path)`, the runtime
    /// writes the encoded counter map atomically alongside each
    /// checkpoint persistence event so a restarted node retains its
    /// per-pool monotonicity high-water marks. Reference:
    /// `PraosState.csCounters` in `Ouroboros.Consensus.Protocol.Praos`.
    pub ocert_persist_dir: Option<PathBuf>,
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
            peer_registry: None,
            mempool: None,
            tentative_state: None,
            tip_notify: None,
            bp_state: None,
            bp_pool_key_hash: None,
            inbound_tx_state: None,
            ocert_persist_dir: None,
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

    /// Attach a shared peer registry so the reconnect loop can prefer
    /// governor-managed hot peers.
    pub fn with_peer_registry(mut self, peer_registry: Option<Arc<RwLock<PeerRegistry>>>) -> Self {
        self.peer_registry = peer_registry;
        self
    }

    /// Attach a shared mempool for sync-driven eviction and re-admission.
    pub fn with_mempool(mut self, mempool: Option<SharedMempool>) -> Self {
        self.mempool = mempool;
        self
    }

    /// Provide optional shared tentative-header state for diffusion pipelining.
    pub fn with_tentative_state(
        mut self,
        tentative_state: Option<Arc<RwLock<TentativeState>>>,
    ) -> Self {
        self.tentative_state = tentative_state;
        self
    }

    /// Attach a chain-tip notification channel so inbound ChainSync servers
    /// are woken after each successful sync batch application.
    pub fn with_tip_notify(mut self, tip_notify: Option<ChainTipNotify>) -> Self {
        self.tip_notify = tip_notify;
        self
    }

    /// Attach shared block-producer state for live nonce/sigma updates.
    pub fn with_bp_state(
        mut self,
        bp_state: Option<Arc<RwLock<SharedBlockProducerState>>>,
        bp_pool_key_hash: Option<[u8; 28]>,
    ) -> Self {
        self.bp_state = bp_state;
        self.bp_pool_key_hash = bp_pool_key_hash;
        self
    }

    /// Enable atomic persistence of the OpCert counter sidecar
    /// (`ocert_counters.cbor`) under `dir` whenever a ledger checkpoint is
    /// written. Pass `None` (the default) to keep counters process-local.
    /// Reference: `PraosState.csCounters` in
    /// `Ouroboros.Consensus.Protocol.Praos`.
    pub fn with_ocert_persist_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.ocert_persist_dir = dir;
        self
    }

    /// Attach a shared TxSubmission inbound dedup state so the eviction
    /// pipeline can call [`SharedTxState::mark_confirmed`] for every
    /// confirmed roll-forward batch.  Mirrors upstream
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.State` `bufferedTxs`
    /// population on confirmation.
    pub fn with_inbound_tx_state(
        mut self,
        inbound_tx_state: Option<SharedTxState>,
    ) -> Self {
        self.inbound_tx_state = inbound_tx_state;
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
    peer_registry: Option<Arc<RwLock<PeerRegistry>>>,
    mempool: Option<SharedMempool>,
    tentative_state: Option<Arc<RwLock<TentativeState>>>,
    tip_notify: Option<ChainTipNotify>,
    bp_state: Option<Arc<RwLock<SharedBlockProducerState>>>,
    bp_pool_key_hash: Option<[u8; 28]>,
    /// Optional shared TxSubmission inbound dedup state.  When present,
    /// the eviction pipeline notifies it of confirmed TxIds so peers that
    /// re-advertise on-chain transactions are immediately acked.
    inbound_tx_state: Option<SharedTxState>,
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
    /// Consecutive failures without making progress (for exponential backoff).
    /// Reset to 0 whenever a batch completes successfully.
    consecutive_failures: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RollbackReAdmissionStats {
    re_admitted: usize,
    duplicate: usize,
    expired: usize,
    conflicting: usize,
    capacity_exceeded: usize,
    protocol_rejected: usize,
    missing_cache_entry: usize,
}

fn cache_confirmed_entries(
    mempool: &SharedMempool,
    confirmed_ids: &[TxId],
    recently_confirmed: &mut BTreeMap<TxId, MempoolEntry>,
) -> usize {
    if confirmed_ids.is_empty() {
        return 0;
    }

    let snapshot = mempool.snapshot();
    let mut cached = 0usize;
    for tx_id in confirmed_ids {
        if recently_confirmed.contains_key(tx_id) {
            continue;
        }
        if let Some(entry) = snapshot.mempool_lookup_tx_by_id(tx_id) {
            recently_confirmed.insert(*tx_id, entry.clone());
            cached += 1;
        }
    }
    cached
}

fn re_admit_rolled_back_tx_ids(
    mempool: &SharedMempool,
    rolled_back_tx_ids: &[TxId],
    current_slot: SlotNo,
    recently_confirmed: &mut BTreeMap<TxId, MempoolEntry>,
) -> RollbackReAdmissionStats {
    let mut stats = RollbackReAdmissionStats::default();
    for tx_id in rolled_back_tx_ids {
        let Some(entry) = recently_confirmed.remove(tx_id) else {
            stats.missing_cache_entry += 1;
            continue;
        };

        match mempool.insert_checked(entry, current_slot, None) {
            Ok(()) => stats.re_admitted += 1,
            Err(MempoolError::Duplicate(_)) => stats.duplicate += 1,
            Err(MempoolError::TtlExpired { .. }) => stats.expired += 1,
            Err(MempoolError::ConflictingInputs(_)) => stats.conflicting += 1,
            Err(MempoolError::CapacityExceeded { .. }) => stats.capacity_exceeded += 1,
            Err(MempoolError::FeeTooSmall { .. })
            | Err(MempoolError::TxTooLarge { .. })
            | Err(MempoolError::ExUnitsExceedTxLimit { .. })
            | Err(MempoolError::ProtocolParamValidation(_)) => stats.protocol_rejected += 1,
        }
    }
    stats
}

/// Evict confirmed, conflicting, expired, and ledger-invalid mempool
/// entries after a roll-forward batch.
///
/// This implements the upstream `syncWithLedger` / `revalidateTxsFor` flow:
/// after structural eviction (confirmed, double-spend, TTL), remaining
/// entries are fully re-applied against a scratch copy of the post-block
/// ledger state.  Entries that fail re-application are evicted.
///
/// When `inbound_tx_state` is provided, the confirmed TxIds are also
/// recorded in the cross-peer TxSubmission dedup state via
/// [`SharedTxState::mark_confirmed`] so inbound peers stop re-advertising
/// transactions that have just been included on-chain.  Mirrors upstream
/// `Ouroboros.Network.TxSubmission.Inbound.V2.State` `bufferedTxs`
/// population on block confirmation.
///
/// Returns a tuple of `(cached, confirmed, conflicting, expired, revalidated)`.
fn evict_mempool_after_roll_forward(
    mempool: &SharedMempool,
    blocks: &[crate::sync::MultiEraBlock],
    tip: &Point,
    recently_confirmed: &mut BTreeMap<TxId, MempoolEntry>,
    checkpoint_tracking: Option<&LedgerCheckpointTracking>,
    inbound_tx_state: Option<&SharedTxState>,
) -> (usize, usize, usize, usize, usize) {
    let confirmed_ids: Vec<TxId> = blocks.iter().flat_map(extract_tx_ids).collect();
    if confirmed_ids.is_empty() {
        return (0, 0, 0, 0, 0);
    }
    let cached = cache_confirmed_entries(mempool, &confirmed_ids, recently_confirmed);
    let removed = mempool.remove_confirmed(&confirmed_ids);
    // Notify the cross-peer TxSubmission dedup state that these TxIds are
    // now on-chain so peers that re-advertise them are immediately acked
    // without re-fetching the bodies (upstream `bufferedTxs` semantics).
    if let Some(tx_state) = inbound_tx_state {
        tx_state.mark_confirmed(&confirmed_ids);
    }
    // Evict mempool txs whose inputs were consumed by
    // a *different* on-chain tx (double-spend conflict).
    // Reference: syncWithLedger / revalidateTxsFor.
    let consumed: Vec<ShelleyTxIn> = blocks.iter().flat_map(extract_consumed_inputs).collect();
    let conflicting = mempool.remove_conflicting_inputs(&consumed);
    let tip_slot = tip.slot().unwrap_or(SlotNo(0));
    let purged = mempool.purge_expired(tip_slot);
    // Full ledger re-validation: upstream `syncWithLedger` /
    // `revalidateTxsFor` re-applies every remaining tx
    // against the post-block ledger state.
    let revalidated = if let Some(tracking) = checkpoint_tracking {
        let mut scratch = tracking.ledger_state.clone();
        let eval = tracking.plutus_evaluator.clone();
        mempool.revalidate_with_ledger(|entry| match entry.to_multi_era_submitted_tx() {
            Ok(tx) => scratch
                .apply_submitted_tx(&tx, tip_slot, Some(&eval))
                .is_ok(),
            Err(_) => false,
        })
    } else {
        0
    };
    (cached, removed, conflicting, purged, revalidated)
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
            consecutive_failures: 0,
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
}

/// Register a freshly-bootstrapped peer in the shared `BlockFetchPool` so the
/// pool tracks per-peer state across reconnects.  Mirrors upstream
/// `addNewFetchClient` / `bracketFetchClient` in
/// `Ouroboros.Network.BlockFetch.ClientRegistry`: every active fetch client
/// must be registered with the registry while the session is live.
fn pool_register_peer(
    pool: Option<&yggdrasil_network::BlockFetchInstrumentation>,
    peer_addr: SocketAddr,
) {
    if let Some(p) = pool {
        if let Ok(mut guard) = p.lock() {
            guard.register_peer(peer_addr);
        }
    }
}

/// Update this peer's known fragment head in the shared pool after a
/// successful sync batch advances `current_point`.  The pool's scheduling
/// policy uses this to gate range assignments — a peer can only receive a
/// range whose `upper` is at or behind its known fragment head.  Mirrors
/// upstream `setFetchClientFragment` in
/// `Ouroboros.Network.BlockFetch.ClientState`.
fn pool_update_fragment_head(
    pool: Option<&yggdrasil_network::BlockFetchInstrumentation>,
    peer_addr: SocketAddr,
    head: Point,
) {
    if let Some(p) = pool {
        if let Ok(mut guard) = p.lock() {
            guard.set_peer_fragment_head(peer_addr, head);
        }
    }
}

/// Returns `true` when the pool has recorded enough consecutive failures
/// from `peer_addr` to warrant proactive demotion + mux teardown.  Mirrors
/// upstream `maxFetchClientFailures` policy in
/// `Ouroboros.Network.BlockFetch.ClientState`.
fn pool_should_demote_peer(
    pool: Option<&yggdrasil_network::BlockFetchInstrumentation>,
    peer_addr: SocketAddr,
) -> bool {
    if let Some(p) = pool {
        if let Ok(guard) = p.lock() {
            if let Some(state) = guard.peer_state(peer_addr) {
                return state.consecutive_failures
                    >= yggdrasil_network::blockfetch_pool::DEFAULT_FAILURE_DEMOTION_THRESHOLD;
            }
        }
    }
    false
}

/// Remove `peer_addr` from the pool when its session ends.  Preserves
/// historical counters for inspection but frees the per-peer slot so the
/// next connection re-registers cleanly.  Mirrors upstream
/// `removeFetchClient` in `Ouroboros.Network.BlockFetch.ClientRegistry`.
fn pool_unregister_peer(
    pool: Option<&yggdrasil_network::BlockFetchInstrumentation>,
    peer_addr: SocketAddr,
) {
    if let Some(p) = pool {
        if let Ok(mut guard) = p.lock() {
            let _ = guard.remove_peer(peer_addr);
        }
    }
}

#[allow(dead_code)]
mod _runstate_impl_marker {
    // Marker module — keeps the split impl-block boundary visible and
    // prevents accidental insertion of unrelated items between the two
    // halves of `impl ReconnectingRunState`.
}

impl ReconnectingRunState {

    fn record_progress(&mut self, progress: &MultiEraSyncProgress) {
        self.total_blocks += progress.fetched_blocks;
        self.total_rollbacks += progress.rollback_count;
        self.batches_completed += 1;
        // A successful batch resets the failure counter.
        self.consecutive_failures = 0;
    }

    /// Called when the inner loop breaks due to an error (reconnect).
    fn record_reconnect_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
    }

    /// Exponential backoff delay for reconnection attempts.
    ///
    /// The first reconnection attempt (consecutive_failures == 1) proceeds
    /// immediately so that peer rotation is not penalised.  From the second
    /// consecutive failure onward the delay doubles starting at 1 s, capped
    /// at 60 s.  Upstream reference: `peerBackerOff` exponential backoff in
    /// `Ouroboros.Network.PeerSelection.Governor`.
    fn reconnect_backoff(&self) -> std::time::Duration {
        if self.consecutive_failures <= 1 {
            return std::time::Duration::ZERO;
        }
        let exp = (self.consecutive_failures - 1).min(6); // cap at 2^6 = 64s
        let secs = 1u64
            .checked_shl(exp.saturating_sub(1))
            .unwrap_or(64)
            .min(60);
        std::time::Duration::from_secs(secs)
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

#[derive(Debug)]
enum BatchErrorDisposition {
    /// Reconnect to a different peer and retry.
    Reconnect,
    /// Reconnect and additionally record that the peer sent us invalid
    /// data.  Upstream this would trigger `InvalidBlockPunishment` /
    /// `PeerSentAnInvalidBlockException` and the governor would demote
    /// the peer.
    ///
    /// Reference: `Ouroboros.Consensus.Storage.ChainDB.API.Types.InvalidBlockPunishment`
    ReconnectAndPunish,
    /// Fatal local error — stop the sync service.
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
        if let Some(block_no) = progress.latest_block_number() {
            m.set_current_block_number(block_no);
        }
    }
}

fn peer_point_trace_fields(peer_addr: SocketAddr, current_point: Point) -> BTreeMap<String, Value> {
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
    fields.insert(
        "batchFetchedBlocks".to_owned(),
        json!(progress.fetched_blocks),
    );
    fields.insert("batchRollbacks".to_owned(), json!(progress.rollback_count));
    fields.insert("totalBlocks".to_owned(), json!(run_state.total_blocks));
    fields.insert(
        "batchesCompleted".to_owned(),
        json!(run_state.batches_completed),
    );
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

/// Wall-clock cadence at which the verified-sync reconnect loops emit
/// `MsgKeepAlive` heartbeats to peers.
///
/// Upstream `keepAliveTimeout` defaults to ~97 s; we send well below that
/// to keep the connection live without saturating the channel.  Reference:
/// `Ouroboros.Network.Protocol.KeepAlive.Codec`.
const KEEPALIVE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);

/// Heartbeat scheduler driving `MsgKeepAlive` traffic alongside the
/// verified-sync request/reply loop.
///
/// Each reconnecting verified-sync inner loop owns one of these so the
/// shared `session.keep_alive` driver receives a periodic ping that
/// matches upstream's `keepAliveClient` cadence.  Cookies are
/// monotonically wrapping `u16` values.
struct KeepAliveScheduler {
    last_sent_at: Instant,
    next_cookie: u16,
}

impl KeepAliveScheduler {
    /// Create a fresh scheduler that fires its first heartbeat one
    /// `KEEPALIVE_HEARTBEAT_INTERVAL` from now.
    fn new(now: Instant) -> Self {
        Self {
            last_sent_at: now,
            next_cookie: 1,
        }
    }

    /// Send a `MsgKeepAlive` if the heartbeat interval has elapsed.
    ///
    /// Returns `Ok(true)` when a heartbeat was sent and acknowledged,
    /// `Ok(false)` when no heartbeat was due, and propagates the
    /// underlying [`KeepAliveClient`] error otherwise so the caller can
    /// abort the mux and record a reconnect.
    async fn tick(
        &mut self,
        client: &mut KeepAliveClient,
    ) -> Result<bool, KeepAliveClientError> {
        if self.last_sent_at.elapsed() < KEEPALIVE_HEARTBEAT_INTERVAL {
            return Ok(false);
        }
        client.keep_alive(self.next_cookie).await?;
        self.next_cookie = self.next_cookie.wrapping_add(1);
        self.last_sent_at = Instant::now();
        Ok(true)
    }
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
        verified_sync_batch_trace_fields(peer_addr, current_point, progress, run_state, extras),
    );
}

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

    let snapshot = merge_ledger_peer_snapshots(
        &LedgerPeerSnapshot::new(ledger_peers, Vec::new()),
        snapshot_overlay,
    );
    let freshness: PeerSnapshotFreshness = derive_peer_snapshot_freshness(
        use_ledger_peers,
        peer_snapshot_path.is_some(),
        snapshot_slot,
        latest_slot,
        snapshot_available,
    );
    let mut blocked_peers = refreshed.clone();
    blocked_peers.push(primary_peer);
    let (decision, eligible_peers) = eligible_ledger_peer_candidates(
        &snapshot,
        &blocked_peers,
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

    extend_unique_socket_addrs(&mut refreshed, eligible_peers);
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
        peer_registry,
        mempool,
        tentative_state,
        tip_notify,
        bp_state,
        bp_pool_key_hash,
        inbound_tx_state,
    } = context;
    let ReconnectingVerifiedSyncState {
        mut from_point,
        mut nonce_state,
        mut checkpoint_tracking,
    } = state;

    tokio::pin!(shutdown);

    let mut run_state = ReconnectingRunState::new();
    let mut chain_state = config.security_param.map(ChainState::new);
    let mut ocert_counters = config.verification.ocert_counters.clone();
    let mut had_session = false;
    let mut preferred_peer = None;
    let mut recently_confirmed = BTreeMap::<TxId, MempoolEntry>::new();

    loop {
        // Exponential backoff before reattempting after consecutive failures.
        let backoff = run_state.reconnect_backoff();
        if !backoff.is_zero() {
            tracer.trace_runtime(
                "Net.PeerSelection",
                "Info",
                "delaying reconnect attempt",
                trace_fields([("backoffMs", json!(backoff.as_millis()))]),
            );
            tokio::select! {
                biased;
                () = &mut shutdown => {
                    trace_shutdown_before_bootstrap(tracer);
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }
                () = tokio::time::sleep(backoff) => {}
            }
        }

        let refreshed_fallback_peers = refresh_chain_db_reconnect_fallback_peers(
            node_config.peer_addr,
            fallback_peer_addrs,
            checkpoint_tracking.as_ref(),
            use_ledger_peers,
            peer_snapshot_path,
            tracer,
        );
        let (mut attempt_state, reconnect_preference) = prepare_reconnect_attempt_state(
            node_config.peer_addr,
            &refreshed_fallback_peers,
            peer_registry.as_ref(),
            preferred_peer,
        );

        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "refreshed reconnect peer candidates",
            trace_fields([
                ("fallbackPeerCount", json!(refreshed_fallback_peers.len())),
                (
                    "latestSlot",
                    json!(
                        checkpoint_tracking.as_ref().and_then(|tracking| tracking
                            .ledger_state
                            .tip
                            .slot()
                            .map(|slot| slot.0))
                    ),
                ),
                (
                    "useLedgerPeers",
                    json!(use_ledger_peers.map(|policy| format!("{policy:?}"))),
                ),
                (
                    "preferredPeer",
                    json!(reconnect_preference.map(|(peer, _)| peer.to_string())),
                ),
                (
                    "preferredPeerSource",
                    json!(reconnect_preference.map(|(_, source)| source)),
                ),
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
        pool_register_peer(
            config.block_fetch_pool.as_ref(),
            session.connected_peer_addr,
        );
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

        if let Err(err) = synchronize_chain_sync_to_point(
            &mut session.chain_sync,
            &mut from_point,
            tracer,
            session.connected_peer_addr,
        )
        .await
        {
            trace_reconnectable_sync_error(
                tracer,
                "ChainSync.Client.FindIntersect",
                "intersection request failed; retrying after reconnect",
                session.connected_peer_addr,
                &err,
                from_point,
            );
            session.mux.abort();
            run_state.record_reconnect_failure();
            continue;
        }

        let mut keepalive = KeepAliveScheduler::new(Instant::now());
        loop {
            // Drive the KeepAlive heartbeat alongside ChainSync/BlockFetch so
            // upstream peers do not tear down the connection at
            // `keepAliveTimeout` (~97 s default).
            if let Err(err) = keepalive.tick(&mut session.keep_alive).await {
                trace_reconnectable_sync_error(
                    tracer,
                    "KeepAlive.Client",
                    "keepalive failed; reconnecting",
                    session.connected_peer_addr,
                    &err,
                    from_point,
                );
                session.mux.abort();
                run_state.record_reconnect_failure();
                break;
            }

            let batch_fut = sync_batch_verified_with_tentative(
                &mut session.chain_sync,
                &mut session.block_fetch,
                from_point,
                config.batch_size,
                Some(&config.verification),
                tentative_state.as_ref(),
                &mut ocert_counters,
                config
                    .block_fetch_pool
                    .as_ref()
                    .map(|p| (p, session.connected_peer_addr)),
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
                            let vrf_ctx = if config.verify_vrf {
                                nonce_state.as_ref().zip(config.active_slot_coeff.as_ref()).map(
                                    |(ns, asc)| VrfVerificationContext {
                                        nonce_state: ns,
                                        active_slot_coeff: asc,
                                    },
                                )
                            } else {
                                None
                            };
                            let applied = apply_verified_progress_to_chaindb(
                                chain_db,
                                &progress,
                                chain_state.as_mut(),
                                checkpoint_tracking.as_mut(),
                                &config.checkpoint_policy,
                                vrf_ctx.as_ref(),
                                ocert_counters.as_ref(),
                            )?;

                            trace_epoch_boundary_events(tracer, &applied.epoch_boundary_events);

                            // Update shared block-producer state with live sigma after
                            // epoch boundary events (stake snapshot rotation).
                            if !applied.epoch_boundary_events.is_empty() {
                                if let Some(ref pkh) = bp_pool_key_hash {
                                    let snapshots = checkpoint_tracking.as_ref()
                                        .and_then(|ct| ct.stake_snapshots.as_ref());
                                    update_bp_state_sigma(&bp_state, snapshots, pkh);
                                }
                            }

                            // Epoch revalidation: when a new epoch begins, protocol parameters
                            // may have changed.  Re-validate all mempool entries and evict any
                            // that no longer satisfy the new fee / size / ExUnits constraints.
                            // Reference: Ouroboros.Consensus.Mempool.Impl.Update — syncWithLedger.
                            if !applied.epoch_boundary_events.is_empty() {
                                if let Some(ref mempool) = mempool {
                                    if let Some(ref tracking) = checkpoint_tracking {
                                        if let Some(params) = tracking.ledger_state.protocol_params() {
                                            let tip_slot = progress.current_point.slot().unwrap_or(SlotNo(0));
                                            let evicted = mempool.purge_invalid_for_params(tip_slot, params);
                                            if evicted > 0 {
                                                tracer.trace_runtime(
                                                    "Mempool.EpochRevalidation",
                                                    "Info",
                                                    "purged mempool entries invalid under new epoch params",
                                                    trace_fields([("evicted", json!(evicted))]),
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            if !applied.rolled_back_tx_ids.is_empty() {
                                tracer.trace_runtime(
                                    "ChainDB.Rollback",
                                    "Info",
                                    "collected rolled-back transaction ids",
                                    trace_fields([
                                        ("txCount", json!(applied.rolled_back_tx_ids.len())),
                                    ]),
                                );

                                if let Some(ref mempool) = mempool {
                                    let stats = re_admit_rolled_back_tx_ids(
                                        mempool,
                                        &applied.rolled_back_tx_ids,
                                        progress.current_point.slot().unwrap_or(SlotNo(0)),
                                        &mut recently_confirmed,
                                    );
                                    tracer.trace_runtime(
                                        "Mempool.RollbackReadmission",
                                        "Info",
                                        "processed rolled-back transaction re-admission",
                                        trace_fields([
                                            ("rolledBackTxCount", json!(applied.rolled_back_tx_ids.len())),
                                            ("reAdmitted", json!(stats.re_admitted)),
                                            ("duplicate", json!(stats.duplicate)),
                                            ("expired", json!(stats.expired)),
                                            ("conflicting", json!(stats.conflicting)),
                                            ("capacityExceeded", json!(stats.capacity_exceeded)),
                                            ("protocolRejected", json!(stats.protocol_rejected)),
                                            ("missingCacheEntry", json!(stats.missing_cache_entry)),
                                        ]),
                                    );
                                }
                            }

                            if let Some(ref mempool) = mempool {
                                for step in &progress.steps {
                                    if let MultiEraSyncStep::RollForward { blocks, tip, .. } = step {
                                        let (cached, removed, conflicting, purged, revalidated) =
                                            evict_mempool_after_roll_forward(
                                                mempool, blocks, tip,
                                                &mut recently_confirmed,
                                                checkpoint_tracking.as_ref(),
                                                inbound_tx_state.as_ref(),
                                            );
                                        if cached + removed + conflicting + purged + revalidated > 0 {
                                            tracer.trace_runtime(
                                                "Mempool.Eviction",
                                                "Info",
                                                "evicted confirmed/expired/conflicting txs from mempool",
                                                trace_fields([
                                                    ("cachedForRollback", json!(cached)),
                                                    ("confirmed", json!(removed)),
                                                    ("conflicting", json!(conflicting)),
                                                    ("expired", json!(purged)),
                                                    ("ledgerRevalidated", json!(revalidated)),
                                                ]),
                                            );
                                        }
                                    }
                                }
                            }

                            record_verified_batch_progress(
                                &mut from_point,
                                &mut run_state,
                                &progress,
                                nonce_state.as_mut(),
                                config.nonce_config.as_ref(),
                                metrics,
                            );

                            // Update pool fragment-head tracking with the
                            // live current_point so the multi-peer scheduler
                            // knows this peer can serve up through this slot.
                            pool_update_fragment_head(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                                from_point,
                            );

                            // Push live epoch nonce to the concurrent block producer.
                            update_bp_state_nonce(&bp_state, nonce_state.as_ref());

                            if let Some(ref notify) = tip_notify {
                                notify.notify_waiters();
                            }

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

                            if let Some(next_hot_peer) = preferred_hot_peer_handoff_target(
                                peer_registry.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.PeerSelection",
                                    "Info",
                                    "switching sync session to higher-tip hot peer",
                                    trace_fields([
                                        ("fromPeer", json!(session.connected_peer_addr.to_string())),
                                        ("toPeer", json!(next_hot_peer.to_string())),
                                    ]),
                                );
                                preferred_peer = Some(next_hot_peer);
                                session.mux.abort();
                                break;
                            }
                        }
                        Err(err) => {
                            let disposition = handle_reconnect_batch_error(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &err,
                            );
                            if pool_should_demote_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.BlockFetch.PoolDemote",
                                    "Warning",
                                    "fetch-client failure threshold exceeded for peer",
                                    trace_fields([(
                                        "peer",
                                        json!(session.connected_peer_addr.to_string()),
                                    )]),
                                );
                            }
                            pool_unregister_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            );
                            session.mux.abort();
                            match disposition {
                                BatchErrorDisposition::ReconnectAndPunish => {
                                    // Demote offending peer to Cold so the governor's
                                    // backoff/forget logic penalizes it (upstream
                                    // InvalidBlockPunishment closes the connection).
                                    if let Some(ref registry) = peer_registry {
                                        if let Ok(mut reg) = registry.write() {
                                            reg.set_status(session.connected_peer_addr, PeerStatus::PeerCold);
                                        }
                                    }
                                    run_state.record_reconnect_failure();
                                    break;
                                }
                                BatchErrorDisposition::Reconnect => {
                                    run_state.record_reconnect_failure();
                                    break;
                                }
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
        peer_registry,
        mempool,
        tentative_state,
        tip_notify,
        bp_state,
        bp_pool_key_hash,
        inbound_tx_state,
    } = context;
    let ReconnectingVerifiedSyncState {
        mut from_point,
        mut nonce_state,
        mut checkpoint_tracking,
    } = state;

    tokio::pin!(shutdown);

    let mut run_state = ReconnectingRunState::new();
    let mut chain_state = config.security_param.map(ChainState::new);
    let mut ocert_counters = config.verification.ocert_counters.clone();
    let mut had_session = false;
    let mut preferred_peer = None;
    let mut recently_confirmed = BTreeMap::<TxId, MempoolEntry>::new();

    loop {
        // Exponential backoff before reattempting after consecutive failures.
        let backoff = run_state.reconnect_backoff();
        if !backoff.is_zero() {
            tracer.trace_runtime(
                "Net.PeerSelection",
                "Info",
                "delaying reconnect attempt",
                trace_fields([("backoffMs", json!(backoff.as_millis()))]),
            );
            tokio::select! {
                biased;
                () = &mut shutdown => {
                    trace_shutdown_before_bootstrap(tracer);
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }
                () = tokio::time::sleep(backoff) => {}
            }
        }

        let refreshed_fallback_peers = refresh_chain_db_reconnect_fallback_peers(
            node_config.peer_addr,
            fallback_peer_addrs,
            checkpoint_tracking.as_ref(),
            use_ledger_peers,
            peer_snapshot_path,
            tracer,
        );
        let (mut attempt_state, reconnect_preference) = prepare_reconnect_attempt_state(
            node_config.peer_addr,
            &refreshed_fallback_peers,
            peer_registry.as_ref(),
            preferred_peer,
        );

        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "refreshed reconnect peer candidates",
            trace_fields([
                ("fallbackPeerCount", json!(refreshed_fallback_peers.len())),
                (
                    "latestSlot",
                    json!(
                        checkpoint_tracking.as_ref().and_then(|tracking| tracking
                            .ledger_state
                            .tip
                            .slot()
                            .map(|slot| slot.0))
                    ),
                ),
                (
                    "useLedgerPeers",
                    json!(use_ledger_peers.map(|policy| format!("{policy:?}"))),
                ),
                (
                    "preferredPeer",
                    json!(reconnect_preference.map(|(peer, _)| peer.to_string())),
                ),
                (
                    "preferredPeerSource",
                    json!(reconnect_preference.map(|(_, source)| source)),
                ),
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
        pool_register_peer(
            config.block_fetch_pool.as_ref(),
            session.connected_peer_addr,
        );
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

        if let Err(err) = synchronize_chain_sync_to_point(
            &mut session.chain_sync,
            &mut from_point,
            tracer,
            session.connected_peer_addr,
        )
        .await
        {
            trace_reconnectable_sync_error(
                tracer,
                "ChainSync.Client.FindIntersect",
                "intersection request failed; retrying after reconnect",
                session.connected_peer_addr,
                &err,
                from_point,
            );
            session.mux.abort();
            run_state.record_reconnect_failure();
            continue;
        }

        let mut keepalive = KeepAliveScheduler::new(Instant::now());
        loop {
            // Drive the KeepAlive heartbeat alongside ChainSync/BlockFetch so
            // upstream peers do not tear down the connection at
            // `keepAliveTimeout` (~97 s default).
            if let Err(err) = keepalive.tick(&mut session.keep_alive).await {
                trace_reconnectable_sync_error(
                    tracer,
                    "KeepAlive.Client",
                    "keepalive failed; reconnecting",
                    session.connected_peer_addr,
                    &err,
                    from_point,
                );
                session.mux.abort();
                run_state.record_reconnect_failure();
                break;
            }

            let batch_fut = sync_batch_verified_with_tentative(
                &mut session.chain_sync,
                &mut session.block_fetch,
                from_point,
                config.batch_size,
                Some(&config.verification),
                tentative_state.as_ref(),
                &mut ocert_counters,
                config
                    .block_fetch_pool
                    .as_ref()
                    .map(|p| (p, session.connected_peer_addr)),
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
                            let vrf_ctx = if config.verify_vrf {
                                nonce_state.as_ref().zip(config.active_slot_coeff.as_ref()).map(
                                    |(ns, asc)| VrfVerificationContext {
                                        nonce_state: ns,
                                        active_slot_coeff: asc,
                                    },
                                )
                            } else {
                                None
                            };
                            let applied = {
                                let mut chain_db = chain_db.write().map_err(|_| shared_chaindb_lock_error())?;
                                apply_verified_progress_to_chaindb(
                                    &mut *chain_db,
                                    &progress,
                                    chain_state.as_mut(),
                                    checkpoint_tracking.as_mut(),
                                    &config.checkpoint_policy,
                                    vrf_ctx.as_ref(),
                                    ocert_counters.as_ref(),
                                )?
                            };

                            trace_epoch_boundary_events(tracer, &applied.epoch_boundary_events);

                            // Push updated pool sigma to block producer on epoch boundary.
                            if !applied.epoch_boundary_events.is_empty() {
                                if let Some(ref pkh) = bp_pool_key_hash {
                                    let snapshots = checkpoint_tracking.as_ref()
                                        .and_then(|ct| ct.stake_snapshots.as_ref());
                                    update_bp_state_sigma(&bp_state, snapshots, pkh);
                                }
                            }

                            // Epoch revalidation: when a new epoch begins, protocol parameters
                            // may have changed.  Re-validate all mempool entries and evict any
                            // that no longer satisfy the new fee / size / ExUnits constraints.
                            // Reference: Ouroboros.Consensus.Mempool.Impl.Update — syncWithLedger.
                            if !applied.epoch_boundary_events.is_empty() {
                                if let Some(ref mempool) = mempool {
                                    if let Some(ref tracking) = checkpoint_tracking {
                                        if let Some(params) = tracking.ledger_state.protocol_params() {
                                            let tip_slot = progress.current_point.slot().unwrap_or(SlotNo(0));
                                            let evicted = mempool.purge_invalid_for_params(tip_slot, params);
                                            if evicted > 0 {
                                                tracer.trace_runtime(
                                                    "Mempool.EpochRevalidation",
                                                    "Info",
                                                    "purged mempool entries invalid under new epoch params",
                                                    trace_fields([("evicted", json!(evicted))]),
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            if !applied.rolled_back_tx_ids.is_empty() {
                                tracer.trace_runtime(
                                    "ChainDB.Rollback",
                                    "Info",
                                    "collected rolled-back transaction ids",
                                    trace_fields([
                                        ("txCount", json!(applied.rolled_back_tx_ids.len())),
                                    ]),
                                );

                                if let Some(ref mempool) = mempool {
                                    let stats = re_admit_rolled_back_tx_ids(
                                        mempool,
                                        &applied.rolled_back_tx_ids,
                                        progress.current_point.slot().unwrap_or(SlotNo(0)),
                                        &mut recently_confirmed,
                                    );
                                    tracer.trace_runtime(
                                        "Mempool.RollbackReadmission",
                                        "Info",
                                        "processed rolled-back transaction re-admission",
                                        trace_fields([
                                            ("rolledBackTxCount", json!(applied.rolled_back_tx_ids.len())),
                                            ("reAdmitted", json!(stats.re_admitted)),
                                            ("duplicate", json!(stats.duplicate)),
                                            ("expired", json!(stats.expired)),
                                            ("conflicting", json!(stats.conflicting)),
                                            ("capacityExceeded", json!(stats.capacity_exceeded)),
                                            ("protocolRejected", json!(stats.protocol_rejected)),
                                            ("missingCacheEntry", json!(stats.missing_cache_entry)),
                                        ]),
                                    );
                                }
                            }

                            if let Some(ref mempool) = mempool {
                                for step in &progress.steps {
                                    if let MultiEraSyncStep::RollForward { blocks, tip, .. } = step {
                                        let (cached, removed, conflicting, purged, revalidated) =
                                            evict_mempool_after_roll_forward(
                                                mempool, blocks, tip,
                                                &mut recently_confirmed,
                                                checkpoint_tracking.as_ref(),
                                                inbound_tx_state.as_ref(),
                                            );
                                        if cached + removed + conflicting + purged + revalidated > 0 {
                                            tracer.trace_runtime(
                                                "Mempool.Eviction",
                                                "Info",
                                                "evicted confirmed/expired/conflicting txs from mempool",
                                                trace_fields([
                                                    ("cachedForRollback", json!(cached)),
                                                    ("confirmed", json!(removed)),
                                                    ("conflicting", json!(conflicting)),
                                                    ("expired", json!(purged)),
                                                    ("ledgerRevalidated", json!(revalidated)),
                                                ]),
                                            );
                                        }
                                    }
                                }
                            }

                            record_verified_batch_progress(
                                &mut from_point,
                                &mut run_state,
                                &progress,
                                nonce_state.as_mut(),
                                config.nonce_config.as_ref(),
                                metrics,
                            );

                            // Update pool fragment-head tracking with the
                            // live current_point so the multi-peer scheduler
                            // knows this peer can serve up through this slot.
                            pool_update_fragment_head(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                                from_point,
                            );

                            // Push live epoch nonce to the concurrent block producer.
                            update_bp_state_nonce(&bp_state, nonce_state.as_ref());

                            if let Some(ref notify) = tip_notify {
                                notify.notify_waiters();
                            }

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

                            if let Some(next_hot_peer) = preferred_hot_peer_handoff_target(
                                peer_registry.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.PeerSelection",
                                    "Info",
                                    "switching sync session to higher-tip hot peer",
                                    trace_fields([
                                        ("fromPeer", json!(session.connected_peer_addr.to_string())),
                                        ("toPeer", json!(next_hot_peer.to_string())),
                                    ]),
                                );
                                preferred_peer = Some(next_hot_peer);
                                session.mux.abort();
                                break;
                            }
                        }
                        Err(err) => {
                            let disposition = handle_reconnect_batch_error(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &err,
                            );
                            if pool_should_demote_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.BlockFetch.PoolDemote",
                                    "Warning",
                                    "fetch-client failure threshold exceeded for peer",
                                    trace_fields([(
                                        "peer",
                                        json!(session.connected_peer_addr.to_string()),
                                    )]),
                                );
                            }
                            pool_unregister_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            );
                            session.mux.abort();
                            match disposition {
                                BatchErrorDisposition::ReconnectAndPunish => {
                                    if let Some(ref registry) = peer_registry {
                                        if let Ok(mut reg) = registry.write() {
                                            reg.set_status(session.connected_peer_addr, PeerStatus::PeerCold);
                                        }
                                    }
                                    run_state.record_reconnect_failure();
                                    break;
                                }
                                BatchErrorDisposition::Reconnect => {
                                    run_state.record_reconnect_failure();
                                    break;
                                }
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
                    peer_sharing: config.peer_sharing,
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

    // Extract weight handles before consuming the ProtocolHandles.
    let mut protocol_weights = vec![
        (MiniProtocolNum::CHAIN_SYNC, cs.weight_handle()),
        (MiniProtocolNum::BLOCK_FETCH, bf.weight_handle()),
        (MiniProtocolNum::KEEP_ALIVE, ka.weight_handle()),
        (MiniProtocolNum::TX_SUBMISSION, tx.weight_handle()),
    ];

    let peer_sharing = conn.protocols.remove(&MiniProtocolNum::PEER_SHARING);
    let peer_sharing = if conn.version_data.peer_sharing > 0 {
        if let Some(ref ps) = peer_sharing {
            protocol_weights.push((MiniProtocolNum::PEER_SHARING, ps.weight_handle()));
        }
        peer_sharing.map(PeerSharingClient::new)
    } else {
        None
    };

    Ok(PeerSession {
        connected_peer_addr,
        chain_sync: ChainSyncClient::new(cs),
        block_fetch: BlockFetchClient::new(bf),
        keep_alive: KeepAliveClient::new(ka),
        tx_submission: TxSubmissionClient::new(tx),
        peer_sharing,
        mux: conn.mux,
        version: conn.version,
        version_data: conn.version_data,
        protocol_weights,
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
    resume_reconnecting_verified_sync_service_chaindb_with_tracer(
        chain_db, request, &tracer, shutdown,
    )
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
        tentative_state: _,
    } = request;

    tokio::pin!(shutdown);

    let mut run_state = ReconnectingRunState::new();
    let mut chain_state = config.security_param.map(ChainState::new);
    let mut ocert_counters = config.verification.ocert_counters.clone();
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
        pool_register_peer(
            config.block_fetch_pool.as_ref(),
            session.connected_peer_addr,
        );

        trace_session_established(
            tracer,
            session.connected_peer_addr,
            run_state.reconnect_count,
            from_point,
        );

        if let Err(err) = synchronize_chain_sync_to_point(
            &mut session.chain_sync,
            &mut from_point,
            tracer,
            session.connected_peer_addr,
        )
        .await
        {
            trace_reconnectable_sync_error(
                tracer,
                "ChainSync.Client.FindIntersect",
                "intersection request failed; retrying after reconnect",
                session.connected_peer_addr,
                &err,
                from_point,
            );
            session.mux.abort();
            run_state.record_reconnect_failure();
            continue;
        }

        let mut keepalive = KeepAliveScheduler::new(Instant::now());
        loop {
            // Drive the KeepAlive heartbeat alongside ChainSync/BlockFetch so
            // upstream peers do not tear down the connection at
            // `keepAliveTimeout` (~97 s default).
            if let Err(err) = keepalive.tick(&mut session.keep_alive).await {
                trace_reconnectable_sync_error(
                    tracer,
                    "KeepAlive.Client",
                    "keepalive failed; reconnecting",
                    session.connected_peer_addr,
                    &err,
                    from_point,
                );
                session.mux.abort();
                run_state.record_reconnect_failure();
                break;
            }

            let batch_fut = sync_batch_apply_verified(
                &mut session.chain_sync,
                &mut session.block_fetch,
                store,
                from_point,
                config.batch_size,
                Some(&config.verification),
                &mut ocert_counters,
                config
                    .block_fetch_pool
                    .as_ref()
                    .map(|p| (p, session.connected_peer_addr)),
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

                            // Update pool fragment-head tracking with the
                            // live current_point so the multi-peer scheduler
                            // knows this peer can serve up through this slot.
                            pool_update_fragment_head(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                                from_point,
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
                            if pool_should_demote_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.BlockFetch.PoolDemote",
                                    "Warning",
                                    "fetch-client failure threshold exceeded for peer",
                                    trace_fields([(
                                        "peer",
                                        json!(session.connected_peer_addr.to_string()),
                                    )]),
                                );
                            }
                            pool_unregister_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            );
                            session.mux.abort();
                            match disposition {
                                BatchErrorDisposition::ReconnectAndPunish
                                | BatchErrorDisposition::Reconnect => {
                                    run_state.record_reconnect_failure();
                                    break;
                                }
                                BatchErrorDisposition::Fail => return Err(err),
                            }
                        }
                    }
                }
            }
        }

        // Exponential backoff before next reconnection attempt (upstream
        // reconnect delay with exponential increase, capped at 60 s).
        let backoff = run_state.reconnect_backoff();
        tokio::select! {
            biased;
            () = &mut shutdown => {
                return Ok(run_state.finish(from_point, nonce_state, chain_state));
            }
            () = tokio::time::sleep(backoff) => {}
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
        peer_registry: _,
        mempool: _,
        tentative_state,
        tip_notify,
        bp_state: _,
        bp_pool_key_hash: _,
        inbound_tx_state: _,
        ocert_persist_dir,
    } = request;

    let recovery = recover_ledger_state_chaindb(chain_db, base_ledger_state)?;
    tracer.trace_runtime(
        "Node.Recovery",
        "Notice",
        "recovered ledger state from coordinated storage",
        trace_fields([
            ("point", json!(format!("{:?}", recovery.point))),
            (
                "checkpointSlot",
                json!(recovery.checkpoint_slot.map(|slot| slot.0)),
            ),
            (
                "replayedVolatileBlocks",
                json!(recovery.replayed_volatile_blocks),
            ),
        ]),
    );

    let checkpoint_tracking = LedgerCheckpointTracking {
        base_ledger_state: recovery.ledger_state.clone(),
        ledger_state: recovery.ledger_state.clone(),
        last_persisted_point: recovery.point,
        plutus_evaluator: config.build_plutus_evaluator(),
        stake_snapshots: config
            .nonce_config
            .as_ref()
            .map(|_| yggdrasil_ledger::StakeSnapshots::new()),
        epoch_size: config.nonce_config.as_ref().map(|nc| {
            config
                .epoch_schedule
                .unwrap_or_else(|| yggdrasil_consensus::EpochSchedule::fixed(nc.epoch_size))
        }),
        pool_block_counts: std::collections::BTreeMap::new(),
        ocert_persist_dir: ocert_persist_dir.clone(),
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
            peer_registry: None,
            mempool: None,
            tentative_state,
            tip_notify,
            bp_state: None,
            bp_pool_key_hash: None,
            inbound_tx_state: None,
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
        chain_db, request, &tracer, shutdown,
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
        peer_registry,
        mempool,
        tentative_state,
        tip_notify,
        bp_state,
        bp_pool_key_hash,
        inbound_tx_state,
        ocert_persist_dir,
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
            (
                "checkpointSlot",
                json!(recovery.checkpoint_slot.map(|slot| slot.0)),
            ),
            (
                "replayedVolatileBlocks",
                json!(recovery.replayed_volatile_blocks),
            ),
        ]),
    );

    let checkpoint_tracking = LedgerCheckpointTracking {
        base_ledger_state: recovery.ledger_state.clone(),
        ledger_state: recovery.ledger_state.clone(),
        last_persisted_point: recovery.point,
        plutus_evaluator: config.build_plutus_evaluator(),
        stake_snapshots: config
            .nonce_config
            .as_ref()
            .map(|_| yggdrasil_ledger::StakeSnapshots::new()),
        epoch_size: config.nonce_config.as_ref().map(|nc| {
            config
                .epoch_schedule
                .unwrap_or_else(|| yggdrasil_consensus::EpochSchedule::fixed(nc.epoch_size))
        }),
        pool_block_counts: std::collections::BTreeMap::new(),
        ocert_persist_dir: ocert_persist_dir.clone(),
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
            peer_registry,
            mempool,
            tentative_state,
            tip_notify,
            bp_state,
            bp_pool_key_hash,
            inbound_tx_state,
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
        tentative_state,
    } = request;
    let checkpoint_tracking = {
        let mut ct = crate::sync::default_checkpoint_tracking(chain_db, base_ledger_state, config)?;
        if let Some(ref nonce_cfg) = config.nonce_config {
            ct.stake_snapshots = Some(yggdrasil_ledger::StakeSnapshots::new());
            ct.epoch_size = Some(
                config
                    .epoch_schedule
                    .unwrap_or_else(|| yggdrasil_consensus::EpochSchedule::fixed(nonce_cfg.epoch_size)),
            );
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
            peer_registry: None,
            mempool: None,
            tentative_state,
            tip_notify: None,
            bp_state: None,
            bp_pool_key_hash: None,
            inbound_tx_state: None,
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
        BatchErrorDisposition, BatchTraceExtras, CheckpointPersistenceOutcome, NodeConfig,
        ReconnectingRunState, ReconnectingVerifiedSyncRequest,
        ResumeReconnectingVerifiedSyncRequest, VerifiedSyncServiceConfig, checkpoint_trace_fields,
        handle_reconnect_batch_error, kes_expiry_warning_from_periods,
        local_root_targets_from_config, mempool_entries_for_forging,
        ordered_reconnect_fallback_peers, peer_share_request_amount,
        preferred_hot_peer_from_registry, preferred_hot_peer_handoff_target,
        prepare_reconnect_attempt_state, reconnect_preferred_peer,
        reconnect_preferred_peer_with_source, record_verified_batch_progress,
        refresh_ledger_peer_sources_from_chain_db, seed_peer_registry, self_validate_forged_block,
        session_established_trace_fields, sync_error_trace_fields, tip_context_from_chain_db,
        verified_sync_batch_trace_fields,
    };
    use crate::sync::LedgerCheckpointPolicy;
    use crate::sync::{MultiEraSyncProgress, SyncError, VerificationConfig};
    use crate::tracer::NodeTracer;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::{Arc, RwLock};
    use yggdrasil_consensus::{EpochSize, NonceEvolutionConfig, NonceEvolutionState};
    use yggdrasil_consensus::{HeaderBody as ConsensusHeaderBody, OpCert};
    use yggdrasil_crypto::blake2b::hash_bytes_256;
    use yggdrasil_crypto::ed25519::{Signature, VerificationKey};
    use yggdrasil_crypto::sum_kes::{SumKesSignature, SumKesVerificationKey};
    use yggdrasil_crypto::vrf::VrfVerificationKey;
    use yggdrasil_ledger::{
        BlockNo, Encoder, Era, HeaderHash, LedgerState, Nonce, Point, PoolParams, PraosHeader,
        PraosHeaderBody, Relay, RewardAccount, ShelleyOpCert, ShelleyVrfCert, SlotNo,
        StakeCredential, UnitInterval,
    };
    use yggdrasil_mempool::SharedMempool;
    use yggdrasil_network::{
        AfterSlot, BlockFetchClientError, ChainSyncClientError, GovernorTargets, HandshakeVersion,
        LedgerStateJudgement, LocalRootConfig, PeerAccessPoint, PeerRegistry, PeerSource,
        PeerStatus, TopologyConfig, UseBootstrapPeers, UseLedgerPeers,
    };
    use yggdrasil_storage::{ChainDb, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile};

    fn local_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    fn sample_mempool_entry(seed: u8, fee: u64, ttl: u64) -> yggdrasil_mempool::MempoolEntry {
        yggdrasil_mempool::MempoolEntry {
            era: yggdrasil_ledger::Era::Shelley,
            tx_id: yggdrasil_ledger::TxId([seed; 32]),
            fee,
            body: vec![seed],
            raw_tx: vec![seed, seed.wrapping_add(1)],
            size_bytes: 2,
            ttl: SlotNo(ttl),
            inputs: vec![],
        }
    }

    fn sample_node_config() -> NodeConfig {
        NodeConfig {
            peer_addr: local_addr(3001),
            network_magic: 42,
            protocol_versions: vec![HandshakeVersion(15)],
            peer_sharing: 1,
        }
    }

    fn sample_sync_config() -> VerifiedSyncServiceConfig {
        VerifiedSyncServiceConfig {
            batch_size: 1,
            verification: VerificationConfig {
                slots_per_kes_period: 129_600,
                max_kes_evolutions: 62,
                verify_body_hash: true,
                max_major_protocol_version: Some(10),
                future_check: None,
                ocert_counters: None,
                pp_major_protocol_version: None,
            },
            nonce_config: None,
            security_param: None,
            checkpoint_policy: LedgerCheckpointPolicy::default(),
            plutus_cost_model: None,
            verify_vrf: false,
            active_slot_coeff: None,
            slot_length_secs: None,
            system_start_unix_secs: None,
        epoch_schedule: None,
        block_fetch_pool: None,
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

    fn sample_forged_block_for_self_validation() -> crate::block_producer::ForgedBlock {
        let mut body_enc = Encoder::new();
        body_enc.array(0);
        body_enc.array(0);
        body_enc.map(0);
        body_enc.array(0);
        let body_payload = body_enc.into_bytes();
        // Match upstream `bbHash` / `hashTxSeq`: H( H(seg_1) || ... || H(seg_n) )
        // over the four post-header CBOR segments emitted above.
        let body_hash = {
            use yggdrasil_ledger::cbor::Decoder;
            let mut dec = Decoder::new(&body_payload);
            let mut combined = Vec::with_capacity(32 * 4);
            for _ in 0..4 {
                let s = dec.position();
                dec.skip().expect("skip body segment");
                let e = dec.position();
                combined.extend_from_slice(&hash_bytes_256(&body_payload[s..e]).0);
            }
            hash_bytes_256(&combined).0
        };
        let body_size = u32::try_from(body_payload.len()).expect("body size must fit in u32");

        let header_body = ConsensusHeaderBody {
            block_number: BlockNo(1),
            slot: SlotNo(1),
            prev_hash: None,
            issuer_vkey: VerificationKey::from_bytes([0x11; 32]),
            vrf_vkey: VrfVerificationKey::from_bytes([0x22; 32]),
            leader_vrf_output: vec![0x33; 32],
            leader_vrf_proof: [0x44; 80],
            nonce_vrf_output: None,
            nonce_vrf_proof: None,
            block_body_size: body_size,
            block_body_hash: body_hash,
            operational_cert: OpCert {
                hot_vkey: SumKesVerificationKey::from_bytes([0x55; 32]),
                sequence_number: 0,
                kes_period: 0,
                sigma: Signature([0x66; 64]),
            },
            protocol_version: (9, 0),
        };

        let kes_signature =
            SumKesSignature::from_bytes(0, &[0u8; 64]).expect("construct sum-kes signature");
        let praos_header = PraosHeader {
            body: PraosHeaderBody {
                block_number: header_body.block_number.0,
                slot: header_body.slot.0,
                prev_hash: header_body.prev_hash.map(|h| h.0),
                issuer_vkey: header_body.issuer_vkey.to_bytes(),
                vrf_vkey: header_body.vrf_vkey.to_bytes(),
                vrf_result: ShelleyVrfCert {
                    output: header_body.leader_vrf_output.clone(),
                    proof: header_body.leader_vrf_proof,
                },
                block_body_size: header_body.block_body_size,
                block_body_hash: header_body.block_body_hash,
                operational_cert: ShelleyOpCert {
                    hot_vkey: header_body.operational_cert.hot_vkey.to_bytes(),
                    sequence_number: header_body.operational_cert.sequence_number,
                    kes_period: header_body.operational_cert.kes_period,
                    sigma: header_body.operational_cert.sigma.0,
                },
                protocol_version: header_body.protocol_version,
            },
            signature: kes_signature.to_bytes().to_vec(),
        };
        let header_hash = praos_header.header_hash();

        crate::block_producer::ForgedBlock {
            header: crate::block_producer::ForgedBlockHeader {
                header_body,
                kes_signature,
            },
            transactions: Vec::new(),
            header_hash,
            slot: SlotNo(1),
            block_number: BlockNo(1),
            body_size,
            total_fees: 0,
        }
    }

    #[test]
    fn mempool_entries_for_forging_is_fee_ordered() {
        let mempool = SharedMempool::with_capacity(1024);
        mempool
            .insert(sample_mempool_entry(1, 10, 1000))
            .expect("insert low-fee tx");
        mempool
            .insert(sample_mempool_entry(2, 50, 1000))
            .expect("insert mid-fee tx");
        mempool
            .insert(sample_mempool_entry(3, 100, 1000))
            .expect("insert high-fee tx");

        let entries = mempool_entries_for_forging(&mempool);
        let fees = entries.iter().map(|entry| entry.fee).collect::<Vec<_>>();
        assert_eq!(fees, vec![100, 50, 10]);
    }

    #[test]
    fn self_validate_forged_block_accepts_structurally_valid_block() {
        let forged = sample_forged_block_for_self_validation();
        self_validate_forged_block(&forged)
            .expect("structurally valid forged block should self-validate");
    }

    #[test]
    fn self_validate_forged_block_rejects_body_hash_mismatch() {
        let mut forged = sample_forged_block_for_self_validation();
        forged.header.header_body.block_body_hash = [0xAB; 32];

        let err = self_validate_forged_block(&forged)
            .expect_err("tampered body hash must fail self-validation");

        assert!(
            err.to_string().contains("block body hash mismatch"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn self_validate_forged_block_rejects_header_hash_mismatch() {
        let mut forged = sample_forged_block_for_self_validation();
        forged.header_hash = HeaderHash([0xCD; 32]);

        let err = self_validate_forged_block(&forged)
            .expect_err("tampered header hash must fail self-validation");

        assert!(
            err.to_string().contains("forged header hash mismatch"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn kes_expiry_warning_triggers_near_window_end() {
        // cert validity window: [100, 162)
        let warning = kes_expiry_warning_from_periods(158, 100, 62, 129_600)
            .expect("warning should be emitted near KES expiry");

        assert_eq!(warning.current_period, 158);
        assert_eq!(warning.cert_start_period, 100);
        assert_eq!(warning.cert_end_period, 162);
        assert_eq!(warning.remaining_periods, 4);
        assert_eq!(warning.remaining_slots, 4 * 129_600);
    }

    #[test]
    fn kes_expiry_warning_suppressed_when_far_from_expiry() {
        // cert validity window: [10, 72), current=40 => remaining=32 (> threshold)
        let warning = kes_expiry_warning_from_periods(40, 10, 62, 129_600);
        assert!(warning.is_none());
    }

    #[test]
    fn tip_context_from_chain_db_reads_tip_block_number() {
        let mut chain_db = ChainDb::new(
            InMemoryImmutable::default(),
            InMemoryVolatile::default(),
            InMemoryLedgerStore::default(),
        );
        let block = yggdrasil_ledger::Block {
            era: Era::Conway,
            header: yggdrasil_ledger::BlockHeader {
                hash: HeaderHash([9; 32]),
                prev_hash: HeaderHash([0; 32]),
                slot_no: SlotNo(42),
                block_no: yggdrasil_ledger::BlockNo(7),
                issuer_vkey: [1; 32],
            },
            transactions: Vec::new(),
            raw_cbor: None,
            header_cbor_size: None,
        };
        chain_db
            .add_volatile_block(block)
            .expect("insert volatile tip block");

        let (tip_slot, tip_block_no, tip_hash) = tip_context_from_chain_db(&chain_db);
        assert_eq!(tip_slot, Some(SlotNo(42)));
        assert_eq!(tip_block_no, Some(yggdrasil_ledger::BlockNo(7)));
        assert_eq!(tip_hash, Some(HeaderHash([9; 32])));
    }

    #[test]
    fn peer_share_request_amount_is_clamped_to_u16() {
        let targets = GovernorTargets {
            target_known: usize::MAX,
            target_established: 5,
            target_active: 2,
            ..Default::default()
        };

        assert_eq!(peer_share_request_amount(&targets), u16::MAX);

        let targets = GovernorTargets {
            target_known: 0,
            target_established: 0,
            target_active: 0,
            ..Default::default()
        };
        assert_eq!(peer_share_request_amount(&targets), 1);
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
            Some(UseLedgerPeers::UseLedgerPeers(
                yggdrasil_network::AfterSlot::Always
            ))
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
            Some(UseLedgerPeers::UseLedgerPeers(
                yggdrasil_network::AfterSlot::Always
            ))
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
    fn resume_request_builder_sets_mempool() {
        let node = sample_node_config();
        let cfg = sample_sync_config();
        let mempool = SharedMempool::default();

        let req = ResumeReconnectingVerifiedSyncRequest::new(
            &node,
            &[],
            LedgerState::new(Era::Byron),
            &cfg,
        )
        .with_mempool(Some(mempool.clone()));

        assert!(req.mempool.is_some());

        // Default constructor has none.
        let req2 = ResumeReconnectingVerifiedSyncRequest::new(
            &node,
            &[],
            LedgerState::new(Era::Byron),
            &cfg,
        );
        assert!(req2.mempool.is_none());
    }

    #[test]
    fn re_admit_rolled_back_tx_ids_reinserts_cached_entries() {
        let mempool = SharedMempool::with_capacity(1024);
        let entry = sample_mempool_entry(42, 100, 1000);
        let tx_id = entry.tx_id;
        mempool.insert(entry.clone()).expect("insert entry");

        let mut recently_confirmed = BTreeMap::new();
        let cached = super::cache_confirmed_entries(&mempool, &[tx_id], &mut recently_confirmed);
        assert_eq!(cached, 1);

        let removed = mempool.remove_confirmed(&[tx_id]);
        assert_eq!(removed, 1);
        assert!(!mempool.contains(&tx_id));

        let stats = super::re_admit_rolled_back_tx_ids(
            &mempool,
            &[tx_id],
            SlotNo(10),
            &mut recently_confirmed,
        );

        assert_eq!(stats.re_admitted, 1);
        assert_eq!(stats.missing_cache_entry, 0);
        assert!(mempool.contains(&tx_id));
        assert!(!recently_confirmed.contains_key(&tx_id));
    }

    #[test]
    fn re_admit_rolled_back_tx_ids_counts_missing_cache_entries() {
        let mempool = SharedMempool::with_capacity(1024);
        let tx_id = yggdrasil_ledger::TxId([7; 32]);
        let mut recently_confirmed = BTreeMap::new();

        let stats = super::re_admit_rolled_back_tx_ids(
            &mempool,
            &[tx_id],
            SlotNo(10),
            &mut recently_confirmed,
        );

        assert_eq!(stats.re_admitted, 0);
        assert_eq!(stats.missing_cache_entry, 1);
        assert!(!mempool.contains(&tx_id));
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
        assert_eq!(
            fields.get("error"),
            Some(&json!("recovery error: checkpoint gap"))
        );
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
    fn handle_reconnect_batch_error_punishes_for_peer_attributable_errors() {
        let tracer = NodeTracer::disabled();

        // Exhaustive — every variant that `SyncError::is_peer_attributable`
        // returns `true` for MUST route to `ReconnectAndPunish`. Keeping
        // this list in lockstep with the `matches!` arms in
        // `is_peer_attributable` (+ the slice-52 exhaustiveness test)
        // gives two independent sources of truth: the classification
        // function AND the downstream disposition.
        let variants: Vec<SyncError> = vec![
            SyncError::BlockBodyHashMismatch,
            SyncError::Consensus(yggdrasil_consensus::ConsensusError::InvalidKesSignature),
            SyncError::LedgerDecode(yggdrasil_ledger::LedgerError::CborTrailingBytes(1)),
            SyncError::BlockFromFuture {
                slot: 999,
                excess_slots: 100,
            },
            SyncError::WrongBlockBodySize {
                declared: 1,
                actual: 2,
            },
            SyncError::ProtocolVersionMismatch {
                era: yggdrasil_ledger::Era::Conway,
                major: 1,
                minor: 0,
                expected_range: "9+".to_owned(),
            },
            SyncError::ProtocolVersionTooHigh { major: 99, max: 10 },
            SyncError::HeaderProtVerTooHigh {
                header_major: 15,
                pp_major: 10,
            },
        ];

        for err in &variants {
            assert!(
                err.is_peer_attributable(),
                "test precondition: {err:?} must be peer-attributable",
            );
            let disposition =
                handle_reconnect_batch_error(&tracer, local_addr(3006), Point::Origin, err);
            assert!(
                matches!(disposition, BatchErrorDisposition::ReconnectAndPunish),
                "expected ReconnectAndPunish for peer-attributable {err:?}, \
                 got {disposition:?}",
            );
        }
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

        let observation = refresh_ledger_peer_sources_from_chain_db(
            &mut registry,
            &chain_db,
            &base_ledger_state,
            &topology,
            &tracer,
        );

        assert!(observation.update.changed);
        assert_eq!(observation.judgement, LedgerStateJudgement::YoungEnough);
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

    #[test]
    fn promote_to_hot_marks_warm_peer() {
        use super::OutboundPeerManager;
        use yggdrasil_network::ControlMessage;

        let addr: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
        let mut mgr = OutboundPeerManager::new();

        // Cannot promote unknown peer.
        assert!(!mgr.promote_to_hot(addr));

        // Simulate adding a warm peer directly.
        let session = fake_peer_session(addr);
        mgr.warm_peers.insert(
            addr,
            super::ManagedWarmPeer::new(session, std::time::Instant::now()),
        );

        // First promotion succeeds.
        assert!(mgr.promote_to_hot(addr));
        assert!(mgr.warm_peers[&addr].is_hot);
        assert_eq!(mgr.warm_peers[&addr].control.hot, ControlMessage::Continue);
        assert_eq!(mgr.warm_peers[&addr].control.warm, ControlMessage::Quiesce);

        // Second promotion is idempotent.
        assert!(!mgr.promote_to_hot(addr));
    }

    #[test]
    fn demote_to_warm_clears_hot_flag() {
        use super::OutboundPeerManager;
        use yggdrasil_network::ControlMessage;

        let addr: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
        let mut mgr = OutboundPeerManager::new();
        let session = fake_peer_session(addr);
        mgr.warm_peers.insert(
            addr,
            super::ManagedWarmPeer::new(session, std::time::Instant::now()),
        );

        mgr.promote_to_hot(addr);
        assert!(mgr.warm_peers[&addr].is_hot);

        assert!(mgr.demote_to_warm(addr));
        assert!(!mgr.warm_peers[&addr].is_hot);
        assert_eq!(mgr.warm_peers[&addr].control.hot, ControlMessage::Quiesce);
        assert_eq!(mgr.warm_peers[&addr].control.warm, ControlMessage::Continue);

        // Demoting an already-warm peer is no-op.
        assert!(!mgr.demote_to_warm(addr));
    }

    #[test]
    fn demote_to_cold_terminates_temperature_bundle() {
        use super::OutboundPeerManager;
        use yggdrasil_network::ControlMessage;

        let addr: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
        let mut mgr = OutboundPeerManager::new();
        let session = fake_peer_session(addr);
        mgr.warm_peers.insert(
            addr,
            super::ManagedWarmPeer::new(session, std::time::Instant::now()),
        );

        assert!(mgr.demote_to_cold(addr));

        // Internal peer entry is removed after close. This verifies the
        // close path is reachable and does not panic while applying
        // terminate controls before aborting the mux.
        assert!(!mgr.warm_peers.contains_key(&addr));

        // Regression guard for expected control constants used by close.
        let mut bundle = yggdrasil_network::TemperatureBundle {
            hot: ControlMessage::Continue,
            warm: ControlMessage::Continue,
            established: ControlMessage::Continue,
        };
        super::apply_control_close(&mut bundle);
        assert_eq!(bundle.hot, ControlMessage::Terminate);
        assert_eq!(bundle.warm, ControlMessage::Terminate);
        assert_eq!(bundle.established, ControlMessage::Terminate);
    }

    #[test]
    fn split_timeout_actions_defers_inbound_scoped_actions() {
        let warm_peer: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
        let inbound_peer: std::net::SocketAddr = "5.6.7.8:3001".parse().unwrap();

        let mut mgr = super::OutboundPeerManager::new();
        mgr.warm_peers.insert(
            warm_peer,
            super::ManagedWarmPeer::new(fake_peer_session(warm_peer), std::time::Instant::now()),
        );

        let warm_conn_id = yggdrasil_network::ConnectionId {
            local: super::outbound_cm_local_addr(),
            remote: warm_peer,
        };
        let inbound_conn_id = yggdrasil_network::ConnectionId {
            local: super::outbound_cm_local_addr(),
            remote: inbound_peer,
        };

        let actions = vec![
            yggdrasil_network::CmAction::PruneConnections(vec![inbound_peer]),
            yggdrasil_network::CmAction::StartResponderTimeout(inbound_conn_id),
            yggdrasil_network::CmAction::TerminateConnection(inbound_conn_id),
            yggdrasil_network::CmAction::TerminateConnection(warm_conn_id),
        ];

        let (applicable, deferred) =
            super::split_timeout_cm_actions_for_governor(&mgr, actions);

        assert_eq!(deferred, 3);
        assert_eq!(applicable.len(), 1);
        assert!(matches!(
            applicable[0],
            yggdrasil_network::CmAction::TerminateConnection(conn_id) if conn_id.remote == warm_peer
        ));
    }

    #[test]
    fn best_hot_peer_selects_highest_slot() {
        use super::OutboundPeerManager;

        let addr_a: std::net::SocketAddr = "1.2.3.4:3001".parse().unwrap();
        let addr_b: std::net::SocketAddr = "5.6.7.8:3001".parse().unwrap();

        let mut mgr = OutboundPeerManager::new();

        // Insert two warm peers.
        let sess_a = fake_peer_session(addr_a);
        mgr.warm_peers.insert(
            addr_a,
            super::ManagedWarmPeer::new(sess_a, std::time::Instant::now()),
        );
        let sess_b = fake_peer_session(addr_b);
        mgr.warm_peers.insert(
            addr_b,
            super::ManagedWarmPeer::new(sess_b, std::time::Instant::now()),
        );

        // No hot peers → no best peer.
        assert!(mgr.best_hot_peer().is_none());

        // Promote both to hot.
        mgr.promote_to_hot(addr_a);
        mgr.promote_to_hot(addr_b);

        // Still none — no tips cached yet.
        assert!(mgr.best_hot_peer().is_none());

        // Give peer A a higher slot tip.
        mgr.warm_peers.get_mut(&addr_a).unwrap().last_known_tip =
            Some(Point::BlockPoint(SlotNo(200), HeaderHash([0xAA; 32])));
        mgr.warm_peers.get_mut(&addr_b).unwrap().last_known_tip =
            Some(Point::BlockPoint(SlotNo(100), HeaderHash([0xBB; 32])));

        assert_eq!(mgr.best_hot_peer(), Some(addr_a));

        // Switch — peer B gets a higher slot.
        mgr.warm_peers.get_mut(&addr_b).unwrap().last_known_tip =
            Some(Point::BlockPoint(SlotNo(300), HeaderHash([0xCC; 32])));

        assert_eq!(mgr.best_hot_peer(), Some(addr_b));
    }

    #[test]
    fn preferred_hot_peer_from_registry_prefers_highest_tip_slot() {
        let hot_a = local_addr(3101);
        let hot_b = local_addr(3102);
        let mut registry = PeerRegistry::default();

        registry.insert_source(hot_a, PeerSource::PeerSourceBootstrap);
        registry.insert_source(hot_b, PeerSource::PeerSourceBootstrap);
        registry.set_status(hot_a, PeerStatus::PeerHot);
        registry.set_status(hot_b, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(hot_a, Some(100));
        registry.set_hot_tip_slot(hot_b, Some(200));

        let shared = Arc::new(RwLock::new(registry));
        assert_eq!(preferred_hot_peer_from_registry(Some(&shared)), Some(hot_b));
    }

    #[test]
    fn preferred_hot_peer_from_registry_returns_none_without_registry() {
        assert_eq!(preferred_hot_peer_from_registry(None), None);
    }

    #[test]
    fn preferred_hot_peer_handoff_target_prefers_higher_tip_hot_peer() {
        let current = local_addr(3210);
        let better = local_addr(3211);
        let mut registry = PeerRegistry::default();

        registry.insert_source(current, PeerSource::PeerSourceBootstrap);
        registry.insert_source(better, PeerSource::PeerSourceBootstrap);
        registry.set_status(current, PeerStatus::PeerHot);
        registry.set_status(better, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(current, Some(100));
        registry.set_hot_tip_slot(better, Some(200));

        let shared = Arc::new(RwLock::new(registry));
        assert_eq!(
            preferred_hot_peer_handoff_target(Some(&shared), current),
            Some(better)
        );
    }

    #[test]
    fn preferred_hot_peer_handoff_target_ignores_non_improving_peer() {
        let current = local_addr(3212);
        let other = local_addr(3213);
        let mut registry = PeerRegistry::default();

        registry.insert_source(current, PeerSource::PeerSourceBootstrap);
        registry.insert_source(other, PeerSource::PeerSourceBootstrap);
        registry.set_status(current, PeerStatus::PeerHot);
        registry.set_status(other, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(current, Some(300));
        registry.set_hot_tip_slot(other, Some(200));

        let shared = Arc::new(RwLock::new(registry));
        assert_eq!(
            preferred_hot_peer_handoff_target(Some(&shared), current),
            None
        );
    }

    #[test]
    fn reconnect_preferred_peer_prefers_hot_registry_peer_over_previous() {
        let previous = local_addr(3201);
        let hot_peer = local_addr(3202);
        let mut registry = PeerRegistry::default();

        registry.insert_source(hot_peer, PeerSource::PeerSourceBootstrap);
        registry.set_status(hot_peer, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(hot_peer, Some(42));

        let shared = Arc::new(RwLock::new(registry));
        assert_eq!(
            reconnect_preferred_peer(Some(&shared), Some(previous)),
            Some(hot_peer)
        );
    }

    #[test]
    fn reconnect_preferred_peer_falls_back_to_previous_peer() {
        let previous = local_addr(3203);
        assert_eq!(
            reconnect_preferred_peer(None, Some(previous)),
            Some(previous)
        );
    }

    #[test]
    fn reconnect_preferred_peer_returns_none_without_candidates() {
        assert_eq!(reconnect_preferred_peer(None, None), None);
    }

    #[test]
    fn reconnect_preferred_peer_with_source_marks_hot_source() {
        let hot_peer = local_addr(3204);
        let mut registry = PeerRegistry::default();

        registry.insert_source(hot_peer, PeerSource::PeerSourceBootstrap);
        registry.set_status(hot_peer, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(hot_peer, Some(55));

        let shared = Arc::new(RwLock::new(registry));
        assert_eq!(
            reconnect_preferred_peer_with_source(Some(&shared), None),
            Some((hot_peer, "hot"))
        );
    }

    #[test]
    fn reconnect_preferred_peer_with_source_marks_previous_source() {
        let previous = local_addr(3205);
        assert_eq!(
            reconnect_preferred_peer_with_source(None, Some(previous)),
            Some((previous, "previous"))
        );
    }

    #[test]
    fn prepare_reconnect_attempt_state_prefers_hot_peer_over_previous() {
        let primary = local_addr(3301);
        let fallback = local_addr(3302);
        let previous = local_addr(3303);
        let hot = local_addr(3304);

        let mut registry = PeerRegistry::default();
        registry.insert_source(hot, PeerSource::PeerSourceBootstrap);
        registry.insert_source(fallback, PeerSource::PeerSourceBootstrap);
        registry.set_status(hot, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(hot, Some(500));
        let shared = Arc::new(RwLock::new(registry));

        let (attempt_state, preference) = prepare_reconnect_attempt_state(
            primary,
            &[fallback, hot],
            Some(&shared),
            Some(previous),
        );

        assert_eq!(preference, Some((hot, "hot")));
        assert_eq!(attempt_state.preferred_peer(), Some(hot));
    }

    #[test]
    fn prepare_reconnect_attempt_state_uses_previous_without_hot_peer() {
        let primary = local_addr(3305);
        let fallback = local_addr(3306);
        let previous = fallback;

        let (attempt_state, preference) =
            prepare_reconnect_attempt_state(primary, &[fallback], None, Some(previous));

        assert_eq!(preference, Some((previous, "previous")));
        assert_eq!(attempt_state.preferred_peer(), Some(previous));
    }

    #[test]
    fn ordered_reconnect_fallback_peers_prioritizes_ranked_hot_peers() {
        let primary = local_addr(3310);
        let hot_low = local_addr(3311);
        let hot_high = local_addr(3312);
        let cold = local_addr(3313);

        let mut registry = PeerRegistry::default();
        for peer in [hot_low, hot_high, cold] {
            registry.insert_source(peer, PeerSource::PeerSourceBootstrap);
        }
        registry.set_status(hot_low, PeerStatus::PeerHot);
        registry.set_status(hot_high, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(hot_low, Some(100));
        registry.set_hot_tip_slot(hot_high, Some(200));

        let shared = Arc::new(RwLock::new(registry));
        let ordered =
            ordered_reconnect_fallback_peers(primary, &[cold, hot_low, hot_high], Some(&shared));

        assert_eq!(ordered, vec![hot_high, hot_low, cold]);
    }

    /// Build a minimal `PeerSession` for unit tests that don't drive protocols.
    fn fake_peer_session(addr: std::net::SocketAddr) -> super::PeerSession {
        use yggdrasil_network::multiplexer::MiniProtocolNum;
        use yggdrasil_network::{HandshakeVersion, NodeToNodeVersionData};

        // We need real protocol handles. Create a TCP loopback pair and mux it.
        // However, that requires async. For pure unit tests we can use an
        // abortable sentinel that panics if any protocol method is called.
        //
        // The simplest approach: create protocol handles from a mux that will
        // never be driven (tests only inspect .is_hot / .promote_to_hot).
        // We build a TcpStream pair synchronously via std and wrap in tokio.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let listen_addr = listener.local_addr().unwrap();
            let client_stream = tokio::net::TcpStream::connect(listen_addr).await.unwrap();
            let (server_stream, _) = listener.accept().await.unwrap();

            let protocols = [
                MiniProtocolNum::CHAIN_SYNC,
                MiniProtocolNum::BLOCK_FETCH,
                MiniProtocolNum::KEEP_ALIVE,
                MiniProtocolNum::TX_SUBMISSION,
            ];

            let (mut handles, mux) = yggdrasil_network::mux::start(
                client_stream,
                yggdrasil_network::multiplexer::MiniProtocolDir::Initiator,
                &protocols,
                4096,
            );
            // Also start the server side so the mux doesn't immediately fail.
            let (_server_handles, server_mux) = yggdrasil_network::mux::start(
                server_stream,
                yggdrasil_network::multiplexer::MiniProtocolDir::Responder,
                &protocols,
                4096,
            );

            // Stash server mux so it outlives the construction; it will be
            // cleaned up when tests drop the manager.
            std::mem::forget(server_mux);

            // Extract weight handles before consuming protocol handles.
            let protocol_weights: Vec<(MiniProtocolNum, yggdrasil_network::WeightHandle)> =
                protocols
                    .iter()
                    .map(|p| (*p, handles.get(p).unwrap().weight_handle()))
                    .collect();

            super::PeerSession {
                connected_peer_addr: addr,
                chain_sync: yggdrasil_network::ChainSyncClient::new(
                    handles.remove(&MiniProtocolNum::CHAIN_SYNC).unwrap(),
                ),
                block_fetch: yggdrasil_network::BlockFetchClient::new(
                    handles.remove(&MiniProtocolNum::BLOCK_FETCH).unwrap(),
                ),
                keep_alive: yggdrasil_network::KeepAliveClient::new(
                    handles.remove(&MiniProtocolNum::KEEP_ALIVE).unwrap(),
                ),
                tx_submission: yggdrasil_network::TxSubmissionClient::new(
                    handles.remove(&MiniProtocolNum::TX_SUBMISSION).unwrap(),
                ),
                peer_sharing: None,
                mux,
                version: HandshakeVersion(15),
                version_data: NodeToNodeVersionData {
                    network_magic: 764824073,
                    initiator_only_diffusion_mode: false,
                    peer_sharing: 0,
                    query: false,
                },
                protocol_weights,
            }
        })
    }
}
