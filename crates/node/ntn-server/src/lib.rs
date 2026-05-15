#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Inbound peer session handling — server-side protocol orchestration.
//!
//! When the node accepts an inbound connection via [`PeerListener`], the
//! resulting [`PeerConnection`] contains protocol handles for the four data
//! mini-protocols.  This module provides helpers to wrap those handles in
//! server drivers and run them concurrently for a single inbound peer.
//!
//! The session runs until the remote peer disconnects or the node shuts
//! down.  Each protocol runs as an independent tokio task so a slow
//! BlockFetch batch does not stall KeepAlive responses.
//!
//! Reference: `ouroboros-network-framework`'s inbound-governor session
//! lifecycle.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side inbound server. Accepts NtN connections, runs the per-peer protocol bundle (handshake → mux → ChainSync server + BlockFetch server + KeepAlive server + TxSubmission server + PeerSharing server). Upstream's `Cardano.Diffusion.NodeToNode.runDiffusionM` carries this concern; Yggdrasil's `server.rs` is the binary-side equivalent.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use yggdrasil_consensus::TentativeState;
use yggdrasil_consensus::mempool::{SharedMempool, SharedTxState};
use yggdrasil_ledger::{
    ByronBlock, CborDecode, CborEncode, Decoder, Encoder, MultiEraSubmittedTx, Point, SlotNo, Tip,
    TxId,
};
use yggdrasil_network::mux::MiniProtocolNum;
use yggdrasil_network::{
    AcceptedConnectionsLimit, BlockFetchServer, BlockFetchServerError, BlockFetchServerRequest,
    ChainSyncServer, ChainSyncServerError, ChainSyncServerRequest, CmAction, ConnectionId,
    ConnectionManagerState, DataFlow, InboundGovernorAction, InboundGovernorEvent,
    InboundGovernorState, KeepAliveMessage, KeepAliveServer, KeepAliveServerError, MuxHandle,
    NodePeerSharing, OperationResult, PeerConnection, PeerListener, PeerListenerError,
    PeerRegistry, PeerSharingMessage, PeerSharingServer, PeerSharingServerError,
    PeerSharingServerRequest, PeerStatus, RateLimitDecision, ResponderCounters, SharedPeerAddress,
    TxIdsReply, TxSubmissionMessage, TxSubmissionServer, TxSubmissionServerError,
    rate_limit_decision,
};
use yggdrasil_node_runtime::{MempoolAddTxResult, add_txs_to_shared_mempool_with_eviction};
use yggdrasil_node_sync::recover_ledger_state_chaindb;
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

// ---------------------------------------------------------------------------
// InboundPeerSession
// ---------------------------------------------------------------------------

/// Server-side protocol drivers for a single accepted inbound peer.
///
/// Constructed from a [`PeerConnection`] returned by [`PeerListener::accept_peer`].
pub struct InboundPeerSession {
    /// ChainSync server driver.
    pub chain_sync: ChainSyncServer,
    /// BlockFetch server driver.
    pub block_fetch: BlockFetchServer,
    /// KeepAlive server driver.
    pub keep_alive: KeepAliveServer,
    /// TxSubmission server driver (server-driven request flow).
    pub tx_submission: TxSubmissionServer,
    /// Optional PeerSharing server driver.
    pub peer_sharing: Option<PeerSharingServer>,
    /// Mux handle for aborting all background tasks on shutdown.
    pub mux: MuxHandle,
    /// Remote peer address.
    pub remote_addr: SocketAddr,
}

/// A provider of peer addresses for the PeerSharing responder.
///
/// Implementations return a list of known shareable peer addresses when a
/// remote node requests peers over mini-protocol 10.
pub trait PeerSharingProvider: Send + Sync {
    /// Return up to `amount` peer addresses to share with the requester.
    fn shareable_peers(&self, amount: u16) -> Vec<SharedPeerAddress>;
}

/// A sink for transactions pulled from an inbound TxSubmission client.
pub trait TxSubmissionConsumer: Send + Sync {
    /// Consume submitted transaction bytes and return the number accepted.
    fn consume_txs(&self, txs: Vec<Vec<u8>>) -> usize;
}

/// Shared `ChainDb` + shared mempool backed TxSubmission consumer.
///
/// This implementation recovers the current ledger state from coordinated
/// storage, decodes submitted transactions using the current ledger era, and
/// then admits them into the shared mempool using the existing runtime helper.
#[derive(Clone)]
pub struct SharedTxSubmissionConsumer<I, V, L> {
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
    /// Optional operator metrics — when set, every successful admission
    /// increments `mempool_tx_added` and every rejection increments
    /// `mempool_tx_rejected`, backing the Prometheus exports of the same
    /// names.
    metrics: Option<Arc<yggdrasil_node_tracer::NodeMetrics>>,
}

impl<I: std::fmt::Debug, V: std::fmt::Debug, L: std::fmt::Debug> std::fmt::Debug
    for SharedTxSubmissionConsumer<I, V, L>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedTxSubmissionConsumer")
            .field("chain_db", &self.chain_db)
            .field("mempool", &self.mempool)
            .field(
                "evaluator",
                &self.evaluator.as_ref().map(|_| "<PlutusEvaluator>"),
            )
            .field("metrics", &self.metrics.as_ref().map(|_| "<NodeMetrics>"))
            .finish()
    }
}

impl<I, V, L> SharedTxSubmissionConsumer<I, V, L> {
    /// Create a new shared TxSubmission consumer from coordinated storage and a mempool.
    pub fn new(chain_db: Arc<RwLock<ChainDb<I, V, L>>>, mempool: SharedMempool) -> Self {
        Self {
            chain_db,
            mempool,
            evaluator: None,
            metrics: None,
        }
    }

    /// Create a new shared TxSubmission consumer with a Plutus evaluator.
    pub fn with_evaluator(
        chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
        mempool: SharedMempool,
        evaluator: Option<
            Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>,
        >,
    ) -> Self {
        Self {
            chain_db,
            mempool,
            evaluator,
            metrics: None,
        }
    }

    /// Attach a [`yggdrasil_node_tracer::NodeMetrics`] handle so per-tx admission
    /// outcomes are mirrored into the `mempool_tx_added` /
    /// `mempool_tx_rejected` Prometheus counters.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<yggdrasil_node_tracer::NodeMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Shared mempool receiving admitted inbound transactions.
    pub fn mempool(&self) -> &SharedMempool {
        &self.mempool
    }
}

impl<I, V, L> TxSubmissionConsumer for SharedTxSubmissionConsumer<I, V, L>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    fn consume_txs(&self, txs: Vec<Vec<u8>>) -> usize {
        let mut ledger_state = {
            let chain_db = match self.chain_db.read() {
                Ok(guard) => guard,
                Err(_) => return 0,
            };
            match recover_ledger_state_chaindb(
                &chain_db,
                yggdrasil_ledger::LedgerState::new(yggdrasil_ledger::Era::Byron),
            ) {
                Ok(recovery) => recovery.ledger_state,
                Err(_) => return 0,
            }
        };

        let current_slot = match ledger_state.tip {
            Point::Origin => SlotNo(0),
            Point::BlockPoint(slot, _) => slot,
        };

        let decoded = txs
            .into_iter()
            .filter_map(|raw_tx| {
                MultiEraSubmittedTx::from_cbor_bytes_for_era(ledger_state.current_era, &raw_tx).ok()
            })
            .collect::<Vec<_>>();

        if decoded.is_empty() {
            return 0;
        }

        let eval_ref = self
            .evaluator
            .as_ref()
            .map(|e| e.as_ref() as &dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator);

        // Eviction-aware inbound admission: when the mempool is full,
        // displace the lowest-fee tail rather than rejecting the
        // incoming tx outright. Mirrors upstream
        // `Ouroboros.Consensus.Mempool.Impl.Update.makeRoomForTransaction`.
        match add_txs_to_shared_mempool_with_eviction(
            &mut ledger_state,
            &self.mempool,
            decoded,
            current_slot,
            eval_ref,
        ) {
            Ok(outcomes) => {
                let mut admitted = 0;
                for outcome in &outcomes {
                    match &outcome.result {
                        MempoolAddTxResult::MempoolTxAdded(_) => {
                            admitted += 1;
                            if let Some(m) = &self.metrics {
                                m.inc_mempool_tx_added();
                                // Each evicted tx is also tracked as a
                                // mempool reject so operator dashboards
                                // can see the displacement rate.
                                for _ in &outcome.evicted {
                                    m.inc_mempool_tx_rejected();
                                }
                            }
                        }
                        MempoolAddTxResult::MempoolTxRejected(_, _) => {
                            if let Some(m) = &self.metrics {
                                m.inc_mempool_tx_rejected();
                            }
                        }
                    }
                }
                admitted
            }
            Err(_) => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// PeerSharing provider backed by a shared PeerRegistry
// ---------------------------------------------------------------------------

/// Shared [`PeerRegistry`]-backed peer-sharing provider that serves warm and
/// hot peers to inbound requester nodes.
///
/// Only peers with status `PeerWarm` or `PeerHot` are returned, matching the
/// upstream policy of advertising established peers only.
#[derive(Clone, Debug)]
pub struct SharedPeerSharingProvider {
    peer_registry: Arc<RwLock<PeerRegistry>>,
    inbound_governor: Option<Arc<RwLock<InboundGovernorState>>>,
}

impl SharedPeerSharingProvider {
    /// Create a new provider from a shared peer registry.
    pub fn new(peer_registry: Arc<RwLock<PeerRegistry>>) -> Self {
        Self {
            peer_registry,
            inbound_governor: None,
        }
    }

    /// Create a new provider from a shared peer registry and optional
    /// inbound governor state so mature inbound peers can be shared.
    pub fn with_inbound_governor(
        peer_registry: Arc<RwLock<PeerRegistry>>,
        inbound_governor: Option<Arc<RwLock<InboundGovernorState>>>,
    ) -> Self {
        Self {
            peer_registry,
            inbound_governor,
        }
    }
}

/// Hard cap on the number of peers a single PeerSharing request may
/// return.  Mirrors upstream `Ouroboros.Network.PeerSelection.PeerSharing`
/// which transports the requested amount as `Word8` (max 255) — we accept
/// `u16` on the wire (`HandshakeVersion`-bound) but cap the work the
/// provider performs at the upstream maximum to keep a malicious peer
/// from forcing a full-registry walk per request.
pub const PEER_SHARING_MAX_AMOUNT: u16 = 255;

impl PeerSharingProvider for SharedPeerSharingProvider {
    fn shareable_peers(&self, amount: u16) -> Vec<SharedPeerAddress> {
        // Clamp to the upstream Word8 maximum; a peer requesting
        // u16::MAX must not get more work than the protocol intends.
        let amount = amount.min(PEER_SHARING_MAX_AMOUNT);
        let registry = match self.peer_registry.read() {
            Ok(guard) => guard,
            Err(_) => return Vec::new(),
        };

        let mut peers = registry
            .iter()
            .filter(|(_, entry)| matches!(entry.status, PeerStatus::PeerWarm | PeerStatus::PeerHot))
            .take(amount as usize)
            .map(|(addr, _)| SharedPeerAddress { addr: *addr })
            .collect::<Vec<_>>();

        if let Some(shared_ig) = self.inbound_governor.as_ref() {
            if let Ok(ig) = shared_ig.read() {
                for addr in ig.mature_duplex_peer_set().keys() {
                    if peers.len() >= amount as usize {
                        break;
                    }
                    if !peers.iter().any(|peer| peer.addr == *addr) {
                        peers.push(SharedPeerAddress { addr: *addr });
                    }
                }
            }
        }

        peers.truncate(amount as usize);
        peers
    }
}

fn now_ms(start: &Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

/// Registry of inbound session abort handles keyed by remote `SocketAddr`.
///
/// This is the bridge that lets connection-manager `TerminateConnection` and
/// `PruneConnections` actions actually abort the inbound mux/session tasks
/// that were spawned by `run_inbound_accept_loop`.
///
/// Reference: upstream `Ouroboros.Network.ConnectionManager.Core` `terminate`
/// invocation in the inbound responder server (`Ouroboros.Network.Server2`),
/// which closes the associated mux when the connection-manager state machine
/// transitions to `TerminatingState`.
#[derive(Clone, Default)]
pub struct InboundSessionAborts {
    inner: Arc<RwLock<BTreeMap<SocketAddr, (tokio::task::AbortHandle, tokio::task::AbortHandle)>>>,
}

impl InboundSessionAborts {
    /// Register a session's mux abort handles.
    pub fn insert(&self, peer: SocketAddr, mux: &MuxHandle) {
        let pair = (mux.reader.abort_handle(), mux.writer.abort_handle());
        if let Ok(mut map) = self.inner.write() {
            map.insert(peer, pair);
        }
    }

    /// Remove a session entry (called when the session task exits normally).
    pub fn remove(&self, peer: &SocketAddr) {
        if let Ok(mut map) = self.inner.write() {
            map.remove(peer);
        }
    }

    /// Abort the session for `peer` if still registered. Returns `true` when
    /// an entry was found and aborted.
    pub fn abort(&self, peer: &SocketAddr) -> bool {
        let pair_opt = self.inner.write().ok().and_then(|mut map| map.remove(peer));
        if let Some((reader, writer)) = pair_opt {
            reader.abort();
            writer.abort();
            true
        } else {
            false
        }
    }
}

impl std::fmt::Debug for InboundSessionAborts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.inner.read().map(|m| m.len()).unwrap_or(0);
        f.debug_struct("InboundSessionAborts")
            .field("len", &len)
            .finish()
    }
}

fn execute_cm_actions(cm_actions: Vec<CmAction>, aborts: Option<&InboundSessionAborts>) {
    for action in cm_actions {
        match action {
            CmAction::TerminateConnection(cid) => {
                // Abort the inbound mux for this peer if the session is still
                // alive; matches upstream `terminate` in `Ouroboros.Network.Server2`.
                if let Some(reg) = aborts {
                    let _ = reg.abort(&cid.remote);
                }
            }
            CmAction::PruneConnections(peers) => {
                // Pruning evicts idle/terminated inbound connections beyond
                // the hard limit; tear down each affected inbound session.
                if let Some(reg) = aborts {
                    for peer in peers {
                        let _ = reg.abort(&peer);
                    }
                }
            }
            CmAction::StartResponderTimeout(_) => {
                // The CM `timeout_tick` loop fires the responder-timeout
                // expiry directly from `responder_timeout_deadline`; this
                // action is informational only.
            }
            CmAction::StartConnect(_) => {
                // Outbound dialing is handled by the runtime governor bridge,
                // not the inbound accept loop. Inbound CM operations should
                // never produce this action; ignore defensively.
            }
        }
    }
}

fn process_connection_manager_timeouts(
    connection_manager: &Arc<RwLock<ConnectionManagerState>>,
    aborts: Option<&InboundSessionAborts>,
) {
    let cm_actions = {
        let mut cm = connection_manager
            .write()
            .expect("connection manager lock poisoned");
        cm.timeout_tick(Instant::now())
    };
    execute_cm_actions(cm_actions, aborts);
}

fn apply_inbound_governor_actions(
    inbound_governor: &Arc<RwLock<InboundGovernorState>>,
    connection_manager: &Arc<RwLock<ConnectionManagerState>>,
    aborts: Option<&InboundSessionAborts>,
    actions: Vec<InboundGovernorAction>,
) {
    let mut pending = actions;

    while let Some(action) = pending.pop() {
        match action {
            InboundGovernorAction::PromotedToWarmRemote(conn_id) => {
                let (_result, cm_actions) = {
                    let mut cm = connection_manager
                        .write()
                        .expect("connection manager lock poisoned");
                    cm.promoted_to_warm_remote(conn_id.remote)
                };
                execute_cm_actions(cm_actions, aborts);
            }
            InboundGovernorAction::DemotedToColdRemote(conn_id) => {
                let (_result, cm_actions) = {
                    let mut cm = connection_manager
                        .write()
                        .expect("connection manager lock poisoned");
                    cm.demoted_to_cold_remote(conn_id.remote)
                };
                execute_cm_actions(cm_actions, aborts);
            }
            InboundGovernorAction::ReleaseInboundConnection(conn_id) => {
                let (release_result, cm_actions) = {
                    let mut cm = connection_manager
                        .write()
                        .expect("connection manager lock poisoned");
                    cm.release_inbound_connection(conn_id.remote)
                };
                execute_cm_actions(cm_actions, aborts);

                if let OperationResult::OperationSuccess(commit_result) = release_result {
                    let follow_up = {
                        let mut ig = inbound_governor
                            .write()
                            .expect("inbound governor lock poisoned");
                        ig.apply_commit_result(conn_id, commit_result)
                    };
                    pending.extend(follow_up);
                }
            }
            InboundGovernorAction::UnregisterConnection(_conn_id) => {
                // The state update already happened in IG; no CM call needed.
            }
        }
    }
}

fn process_inbound_governor_events(
    inbound_governor: &Arc<RwLock<InboundGovernorState>>,
    connection_manager: &Arc<RwLock<ConnectionManagerState>>,
    aborts: Option<&InboundSessionAborts>,
    now_ms: u64,
    events: Vec<InboundGovernorEvent>,
) {
    for event in events {
        let actions = {
            let mut ig = inbound_governor
                .write()
                .expect("inbound governor lock poisoned");
            ig.step(event, now_ms)
        };
        apply_inbound_governor_actions(inbound_governor, connection_manager, aborts, actions);
    }
}

fn update_inbound_responder_counters(
    inbound_governor: &Arc<RwLock<InboundGovernorState>>,
    connection_manager: &Arc<RwLock<ConnectionManagerState>>,
    aborts: Option<&InboundSessionAborts>,
    peer: SocketAddr,
    counters: ResponderCounters,
    now_ms: u64,
) {
    let events = {
        let ig = inbound_governor
            .read()
            .expect("inbound governor lock poisoned");
        ig.update_responder_counters(&peer, counters)
    };

    {
        let mut ig = inbound_governor
            .write()
            .expect("inbound governor lock poisoned");
        ig.set_responder_counters(&peer, counters);
    }

    process_inbound_governor_events(inbound_governor, connection_manager, aborts, now_ms, events);
}

impl InboundPeerSession {
    /// Build an inbound session from an accepted [`PeerConnection`].
    ///
    /// Consumes the per-protocol handles from the connection and wraps
    /// them in server drivers.  Returns `None` if any required protocol
    /// handle is missing.
    pub fn from_connection(mut conn: PeerConnection, remote_addr: SocketAddr) -> Option<Self> {
        let cs = conn.protocols.remove(&MiniProtocolNum::CHAIN_SYNC)?;
        let bf = conn.protocols.remove(&MiniProtocolNum::BLOCK_FETCH)?;
        let ka = conn.protocols.remove(&MiniProtocolNum::KEEP_ALIVE)?;
        let ts = conn.protocols.remove(&MiniProtocolNum::TX_SUBMISSION)?;
        let ps = conn
            .protocols
            .remove(&MiniProtocolNum::PEER_SHARING)
            .map(PeerSharingServer::new);
        Some(Self {
            chain_sync: ChainSyncServer::new(cs),
            block_fetch: BlockFetchServer::new(bf),
            keep_alive: KeepAliveServer::new(ka),
            tx_submission: TxSubmissionServer::new(ts),
            peer_sharing: ps,
            mux: conn.mux,
            remote_addr,
        })
    }
}

// ---------------------------------------------------------------------------
// KeepAlive server task
// ---------------------------------------------------------------------------

/// Run the KeepAlive echo loop until the client sends `MsgDone` or the
/// Run the KeepAlive server loop, echoing cookies back until the client
/// sends `MsgDone` or the connection drops.
///
/// Enforces upstream `timeLimitsKeepAlive` — 60 s server-side timeout
/// (upstream `SingServer → Just 60`) per receive.
pub async fn run_keepalive_server(
    mut server: KeepAliveServer,
    metrics: Option<&yggdrasil_node_tracer::NodeMetrics>,
    peer: Option<SocketAddr>,
) -> Result<(), KeepAliveServerError> {
    loop {
        let result = tokio::time::timeout(
            yggdrasil_network::protocol_limits::keepalive::SERVER
                .expect("keepalive server timeout constant must be set"),
            server.recv_keep_alive(),
        )
        .await;
        match result {
            Ok(Ok(Some(cookie))) => {
                server.respond(cookie).await?;
                if let Some(m) = metrics {
                    let bytes = KeepAliveMessage::MsgKeepAliveResponse { cookie }
                        .to_cbor()
                        .len() as u64;
                    m.add_keepalive_server_bytes_served_for_peer(peer, bytes);
                }
            }
            Ok(Ok(None)) => return Ok(()), // client sent MsgDone
            Ok(Err(e)) => return Err(e),
            Err(_elapsed) => return Err(KeepAliveServerError::Timeout),
        }
    }
}

// ---------------------------------------------------------------------------
// BlockFetch server task (storage-backed)
// ---------------------------------------------------------------------------

/// A trait for looking up raw block bytes by hash range.
///
/// The node layer implements this over its storage backend (e.g. `ChainDb`).
pub trait BlockProvider: Send + Sync {
    /// Look up blocks in the given range `(from, to]`.
    ///
    /// The lower bound is exclusive and the upper bound is inclusive, which
    /// matches BlockFetch usage after ChainSync advances the current point.
    /// Returns the raw CBOR bytes for each block in chain order, or an
    /// empty vec if the range is unavailable.
    fn get_block_range(&self, from: &[u8], to: &[u8]) -> Vec<Vec<u8>>;
}

/// Run the BlockFetch server loop, serving blocks from a [`BlockProvider`].
pub async fn run_blockfetch_server(
    mut server: BlockFetchServer,
    provider: &dyn BlockProvider,
    metrics: Option<&yggdrasil_node_tracer::NodeMetrics>,
    peer: Option<SocketAddr>,
) -> Result<(), BlockFetchServerError> {
    loop {
        match server.recv_request().await? {
            BlockFetchServerRequest::RequestRange(range) => {
                let blocks = provider.get_block_range(&range.lower, &range.upper);
                // R234/R237 — Phase D.2 bytes-out: tally bytes served
                // as a peer.  The exported Prometheus counter stays
                // aggregate-only; the optional `peer` address feeds the
                // internal lifetime-stat fold without high-cardinality
                // labels.
                if let Some(m) = metrics {
                    let bytes_out: u64 = blocks.iter().map(|b| b.len() as u64).sum();
                    m.add_blockfetch_server_bytes_served_for_peer(peer, bytes_out);
                }
                server.serve_batch(blocks).await?;
            }
            BlockFetchServerRequest::ClientDone => return Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// ChainSync server task (storage-backed)
// ---------------------------------------------------------------------------

/// A trait for serving chain headers and finding intersections.
///
/// The node layer implements this over its storage + consensus state.
///
/// **Wire-shape contract** (R220): every `tip` `Vec<u8>` returned by
/// the methods below MUST encode the upstream `Tip` envelope, not a
/// bare `Point`.  `Tip` is `[]` (genesis) or `[point, blockNo]`
/// (specific tip).  See `chain_tip_envelope_cbor` in this file for
/// the canonical helper.
pub trait ChainProvider: Send + Sync {
    /// Return the current chain tip as CBOR-encoded `Tip` envelope
    /// (`[]` for genesis, `[point, blockNo]` for a specific tip).
    /// Used by `MsgIntersectNotFound` and as the `tip` slot of
    /// `MsgRollForward`/`MsgRollBackward`/`MsgIntersectFound`.
    fn chain_tip(&self) -> Vec<u8>;

    /// Return the current chain tip as CBOR-encoded bare `Point`
    /// (`[]` for genesis, `[slot, hash]` for a specific tip).  Used
    /// as the `point` slot of `MsgRollBackward` and to seed the
    /// chainsync cursor.  Distinct from [`Self::chain_tip`] which
    /// returns the upstream `Tip` envelope used at tip-slot positions.
    /// Default impl returns Origin's bytes for maximum backward
    /// compatibility — production providers must override.
    fn chain_tip_point(&self) -> Vec<u8> {
        Point::Origin.to_cbor_bytes()
    }

    /// Given the cursor (last sent point), return the next roll-forward
    /// `(point, header, tip)`, or `None` if at tip.  `point` is the
    /// roll-forward block's point (bare CBOR `Point`); `tip` is the
    /// `Tip` envelope per the trait-level contract.  Used by
    /// `MsgRollForward`.
    fn next_header(&self, cursor: &Option<Vec<u8>>) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)>;

    /// Find the best intersection from the client's candidate points.
    ///
    /// Returns `(found_point, tip)` or `None` if no intersection.
    /// `found_point` is bare CBOR `Point` (echoed back to the client);
    /// `tip` is the `Tip` envelope.  Used by `MsgIntersectFound`.
    fn find_intersect(&self, points: &[Vec<u8>]) -> Option<(Vec<u8>, Vec<u8>)>;

    /// Return the tentative tip header, if diffusion pipelining is active.
    ///
    /// When a tentative header is set (header validated, body not yet),
    /// this returns `(point, header_cbor, tip)` representing the tentative
    /// extension of the confirmed chain.  ChainSync servers use this to
    /// announce the header before body validation completes.
    ///
    /// Reference: `cdbTentativeHeader` in
    /// `Ouroboros.Consensus.Storage.ChainDB.Impl.Types`.
    fn tentative_tip(&self) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        None
    }
}

/// Run the ChainSync server loop, serving headers from a [`ChainProvider`].
///
/// When `tip_notify` is provided, the server awaits it instead of busy-loop
/// polling when the client is at the tip.  This is the Rust equivalent of
/// the upstream ChainDB follower notification mechanism.
pub async fn run_chainsync_server(
    mut server: ChainSyncServer,
    provider: &dyn ChainProvider,
    tip_notify: Option<yggdrasil_node_runtime::ChainTipNotify>,
    metrics: Option<&yggdrasil_node_tracer::NodeMetrics>,
    peer: Option<SocketAddr>,
) -> Result<(), ChainSyncServerError> {
    let mut cursor: Option<Vec<u8>> = None;
    // Track whether the last served header was a tentative (pipelined)
    // header.  If the tentative header is later trapped (body invalid),
    // we must roll-backward to the confirmed tip before serving new data.
    let mut served_tentative = false;

    // R235/R237 — Phase D.2 bytes-out (ChainSync slice): tally the
    // bytes emitted in `MsgRollForward { header, tip }` per call and
    // optionally attribute them to the remote peer for lifetime stats.
    // Counterpart to R234's BlockFetch server bytes-out.
    // ChainSync header bytes are smaller per message (~94 bytes
    // for Byron, ~150 bytes for Shelley) but fire on every
    // RollForward so the cumulative volume is comparable to
    // BlockFetch over a full sync.
    let record_emit =
        |header: &[u8], tip: &[u8], m: Option<&yggdrasil_node_tracer::NodeMetrics>| {
            if let Some(metrics) = m {
                let bytes_out = (header.len() as u64).saturating_add(tip.len() as u64);
                metrics.add_chainsync_server_bytes_served_for_peer(peer, bytes_out);
            }
        };

    loop {
        match server.recv_request().await? {
            ChainSyncServerRequest::RequestNext => {
                // If we previously served a tentative header and it has
                // since been cleared (either adopted into confirmed chain
                // or trapped), reconcile:
                // - If confirmed chain now includes the cursor → ok, keep going.
                // - Otherwise → tentative was trapped, roll backward.
                if served_tentative {
                    if provider.next_header(&cursor).is_some() || provider.tentative_tip().is_some()
                    {
                        // Tentative was adopted: confirmed chain advanced
                        // past the cursor, or tentative is still set.
                        // Fall through to normal processing.
                    } else {
                        // Tentative was trapped: roll backward to confirmed tip.
                        // R221 — `MsgRollBackward.point` is bare `Point`;
                        // `tip` is the upstream `Tip` envelope.  Get the
                        // two from distinct provider methods.
                        served_tentative = false;
                        let confirmed_tip_point = provider.chain_tip_point();
                        let confirmed_tip_envelope = provider.chain_tip();
                        // Reset cursor to the confirmed chain tip's bare Point.
                        cursor = Some(confirmed_tip_point.clone());
                        server
                            .roll_backward(confirmed_tip_point, confirmed_tip_envelope)
                            .await?;
                        continue;
                    }
                }

                match provider.next_header(&cursor) {
                    Some((point, header, tip)) => {
                        served_tentative = false;
                        cursor = Some(point);
                        record_emit(&header, &tip, metrics);
                        server.roll_forward(header, tip).await?;
                    }
                    None => {
                        // At confirmed tip — check for tentative header.
                        if let Some((point, header, tip)) = provider.tentative_tip() {
                            served_tentative = true;
                            cursor = Some(point);
                            record_emit(&header, &tip, metrics);
                            server.roll_forward(header, tip).await?;
                        } else {
                            // No tentative either — tell client to wait.
                            server.await_reply().await?;
                            loop {
                                if let Some(ref notify) = tip_notify {
                                    notify.notified().await;
                                } else {
                                    tokio::task::yield_now().await;
                                }
                                // Check confirmed chain first.
                                if let Some((point, header, tip)) = provider.next_header(&cursor) {
                                    served_tentative = false;
                                    cursor = Some(point);
                                    record_emit(&header, &tip, metrics);
                                    server.roll_forward(header, tip).await?;
                                    break;
                                }
                                // Check tentative header.
                                if let Some((point, header, tip)) = provider.tentative_tip() {
                                    served_tentative = true;
                                    cursor = Some(point);
                                    record_emit(&header, &tip, metrics);
                                    server.roll_forward(header, tip).await?;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            ChainSyncServerRequest::FindIntersect { points } => {
                served_tentative = false;
                match provider.find_intersect(&points) {
                    Some((point, tip)) => {
                        cursor = Some(point.clone());
                        if let Some(m) = metrics {
                            // R235 — also tally MsgIntersectFound
                            // (point + tip envelope bytes).
                            m.add_chainsync_server_bytes_served_for_peer(
                                peer,
                                (point.len() + tip.len()) as u64,
                            );
                        }
                        server.intersect_found(point, tip).await?;
                    }
                    None => {
                        let tip = provider.chain_tip();
                        if let Some(m) = metrics {
                            m.add_chainsync_server_bytes_served_for_peer(peer, tip.len() as u64);
                        }
                        server.intersect_not_found(tip).await?;
                    }
                }
            }
            ChainSyncServerRequest::Done => return Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// TxSubmission server task
// ---------------------------------------------------------------------------

/// Run the TxSubmission server loop, pulling transactions from the remote peer.
///
/// The server requests batches of transaction ids, requests the corresponding
/// bodies, hands those bodies to the provided consumer, then acknowledges the
/// advertised ids on the next request. The loop terminates cleanly when the
/// remote client responds with `MsgDone` to a blocking request.
///
/// When a [`SharedTxState`] and remote `SocketAddr` are provided, the server
/// performs cross-peer TxId deduplication: advertised TxIds that are already
/// known or being fetched from another peer are acknowledged without
/// downloading, preventing duplicate work across concurrent inbound sessions.
/// Greedily select a prefix of `candidates` that fits within
/// `budget_remaining` advertised bytes, looking each entry's size up in
/// `sizes` (defaulting to 0 for missing entries).  The first candidate is
/// always admitted to guarantee forward progress even when a single
/// transaction exceeds the cap, mirroring upstream `collectTxs` behaviour
/// from `Ouroboros.Network.TxSubmission.Inbound.V2`.
///
/// Returns `(admitted, deferred)` where `deferred = candidates.len() -
/// admitted.len()`.
pub fn select_within_byte_budget(
    candidates: &[TxId],
    sizes: &std::collections::HashMap<TxId, u32>,
    budget_remaining: u64,
) -> (Vec<TxId>, usize) {
    let mut admitted: Vec<TxId> = Vec::with_capacity(candidates.len());
    let mut remaining = budget_remaining;
    for t in candidates {
        let sz = sizes.get(t).copied().unwrap_or(0) as u64;
        if admitted.is_empty() || sz <= remaining {
            admitted.push(*t);
            remaining = remaining.saturating_sub(sz);
        } else {
            break;
        }
    }
    let deferred = candidates.len().saturating_sub(admitted.len());
    (admitted, deferred)
}

/// Truncate `candidates` to at most `budget_remaining` entries, mirroring
/// the per-peer in-flight TxIds count cap (upstream `requestedTxsInflight`
/// set size from `Ouroboros.Network.TxSubmission.Inbound.V2.State`).
/// At least one candidate is always admitted (when `candidates` is
/// non-empty) so the loop makes forward progress even when the peer is
/// at the count cap, mirroring the same first-admit guarantee used by
/// [`select_within_byte_budget`].
///
/// Returns `(admitted, deferred)` where `deferred = candidates.len() -
/// admitted.len()`.
pub fn clamp_to_count_budget(candidates: &[TxId], budget_remaining: usize) -> (Vec<TxId>, usize) {
    if candidates.is_empty() {
        return (Vec::new(), 0);
    }
    let take = budget_remaining.max(1).min(candidates.len());
    let admitted = candidates[..take].to_vec();
    let deferred = candidates.len() - take;
    (admitted, deferred)
}

/// Compute the number of TxIds to request from a peer on the next
/// `MsgRequestTxIds`, clamped against the per-peer outstanding-TxIds cap.
///
/// `peer_unacked_count` is the local view of the peer's `unacknowledged`
/// set length; `ack` is the wire-level acknowledgement we are about to
/// send (which will reduce the peer's view of unacked by `ack`); `batch`
/// is the desired (unclamped) batch size; `max_unacked` is the upstream
/// `maxUnacknowledgedTxIds` policy bound.
///
/// The returned value is at least `1` so the loop always makes forward
/// progress when the peer has any capacity, mirroring upstream
/// `Ouroboros.Network.TxSubmission.Inbound.V2.Decision.txDecision`.
pub fn clamp_request_count(
    peer_unacked_count: usize,
    ack: u16,
    batch: u16,
    max_unacked: u16,
) -> u16 {
    let outstanding = peer_unacked_count.saturating_sub(ack as usize);
    let headroom = (max_unacked as usize).saturating_sub(outstanding);
    (batch as usize).min(headroom).max(1) as u16
}

pub async fn run_txsubmission_server(
    mut server: TxSubmissionServer,
    consumer: &dyn TxSubmissionConsumer,
    dedup: Option<(&SharedTxState, SocketAddr)>,
    metrics: Option<&yggdrasil_node_tracer::NodeMetrics>,
    peer: Option<SocketAddr>,
) -> Result<(), TxSubmissionServerError> {
    const TXSUBMISSION_BATCH_SIZE: u16 = 16;
    /// Per-peer cap on advertised bytes in flight, mirroring upstream
    /// `txsSizeInflightPerPeer` from
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.Policy` (default ~64 KiB).
    /// When a peer is at or above this budget, the server defers issuing
    /// further `MsgRequestTxs` until prior fetches complete and decrement
    /// the per-peer byte count.
    const MAX_TXS_SIZE_INFLIGHT_PER_PEER: u64 = 64 * 1024;
    /// Global cap on advertised bytes in flight across all peers,
    /// mirroring upstream `maxTxsSizeInflight` from
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.Policy`.  Typically
    /// `txsSizeInflightPerPeer * numPeers`; we use a fixed multiple
    /// (64 KiB * 32 = 2 MiB) to bound aggregate memory consumption when
    /// many peers concurrently advertise large transaction backlogs.
    /// The effective per-iteration budget is the minimum of per-peer
    /// remaining and global remaining.
    const MAX_TXS_SIZE_INFLIGHT_TOTAL: u64 = 64 * 1024 * 32;
    /// Per-peer cap on outstanding (advertised-but-not-yet-finalized)
    /// TxIds, mirroring upstream `maxUnacknowledgedTxIds` from
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.Policy`.  Acts as a
    /// safety bound on the per-peer `unacknowledged` set so a peer
    /// cannot indefinitely starve the server's per-peer slot by
    /// repeatedly advertising deferred txids that never get fetched.
    const MAX_UNACKNOWLEDGED_TXIDS_PER_PEER: u16 = 64;
    /// Per-peer cap on the COUNT of TxIds currently being fetched
    /// (`MsgRequestTxs` issued, response not yet finalized) — sibling
    /// of the per-peer byte budget but expressed in TxIds rather than
    /// bytes.  Mirrors the upstream `requestedTxsInflight` set whose
    /// size is consulted by `Decision.txDecision` for fairness/limit
    /// checks in `Ouroboros.Network.TxSubmission.Inbound.V2.State`.
    /// Bounds how many concurrent TxId fetches any single peer can
    /// have outstanding regardless of advertised size, so a peer that
    /// only advertises tiny transactions still cannot monopolize the
    /// server's per-peer fetch slot.
    const MAX_TXS_REQUESTED_PER_PEER: usize = 32;

    server.recv_init().await?;
    let mut ack = 0u16;

    // Register this peer for dedup tracking at session start.
    if let Some((tx_state, peer_addr)) = &dedup {
        tx_state.register_peer(*peer_addr);
    }

    // Upstream `serverPeer` uses blocking MsgRequestTxIds in its main
    // collection loop: after processing each batch the server acks the
    // txids that were successfully downloaded and asks the client to
    // advertise more, blocking if the client has nothing queued yet.
    // Reference: `Ouroboros.Network.TxSubmission.Inbound.serverPeer`.

    loop {
        // Clamp the next batch size against the per-peer outstanding
        // cap (upstream `maxUnacknowledgedTxIds`).  The wire `ack` we are
        // about to send will reduce the peer's view of unacked by `ack`,
        // so the post-ack outstanding count is approximately
        // `peer_unacked_count.saturating_sub(ack as usize)`.  Always
        // request at least 1 to guarantee the loop makes forward
        // progress when the peer has capacity.
        let req = if let Some((tx_state, peer_addr)) = &dedup {
            clamp_request_count(
                tx_state.peer_unacked_count(peer_addr),
                ack,
                TXSUBMISSION_BATCH_SIZE,
                MAX_UNACKNOWLEDGED_TXIDS_PER_PEER,
            )
        } else {
            TXSUBMISSION_BATCH_SIZE
        };

        // Issue the blocking `MsgRequestTxIds`.  On transport / protocol
        // error we must unregister this peer from the shared dedup state
        // before propagating the error so the per-peer entry (and any
        // bytes / TxIds it had in flight) is released — without this
        // cleanup, repeated reconnects would leak `PeerTxState` entries
        // and inflate `inflight_bytes_total` / `peer_inflight_count`
        // indefinitely.  Mirrors the upstream `bracketTxSubmissionPeer`
        // cleanup in `Ouroboros.Network.TxSubmission.Inbound.V2.Server`.
        let reply = match server.request_tx_ids(true, ack, req).await {
            Ok(r) => {
                if let Some(m) = metrics {
                    let bytes = TxSubmissionMessage::MsgRequestTxIds {
                        blocking: true,
                        ack,
                        req,
                    }
                    .to_cbor()
                    .len() as u64;
                    m.add_txsubmission_server_bytes_served_for_peer(peer, bytes);
                }
                r
            }
            Err(e) => {
                if let Some((tx_state, peer_addr)) = &dedup {
                    tx_state.unregister_peer(peer_addr);
                }
                return Err(e);
            }
        };
        match reply {
            TxIdsReply::Done => {
                if let Some((tx_state, peer_addr)) = &dedup {
                    tx_state.unregister_peer(peer_addr);
                }
                return Ok(());
            }
            TxIdsReply::TxIds(txids) if txids.is_empty() => {
                // Empty reply on blocking request means peer had nothing;
                // continue the loop and try again.
                ack = 0;
                continue;
            }
            TxIdsReply::TxIds(txids) => {
                // Build lookup of advertised sizes for later verification.
                let advertised_sizes: std::collections::HashMap<TxId, u32> =
                    txids.iter().map(|item| (item.txid, item.size)).collect();

                let advertised_count = txids.len();
                let all_txids: Vec<_> = txids.into_iter().map(|item| item.txid).collect();

                // Filter through shared state to avoid re-fetching known txids.
                // Returns the admitted set actually requested plus the count
                // deferred due to the per-peer byte budget so that the
                // wire-level acknowledgement stays consistent with what we
                // are actually consuming from the peer's outbound queue.
                let (to_request, deferred) = if let Some((tx_state, peer_addr)) = &dedup {
                    let outcome = tx_state.filter_advertised(peer_addr, &all_txids);
                    if outcome.to_fetch.is_empty() {
                        // All txids already known — ack them all without
                        // requesting and continue.
                        ack = advertised_count.min(u16::MAX as usize) as u16;
                        continue;
                    }
                    // Apply the per-peer in-flight TxIds COUNT cap
                    // (upstream `requestedTxsInflight` set size limit)
                    // BEFORE the byte budget so that a peer advertising
                    // many small transactions is bounded by count even
                    // when bytes-in-flight remain low.  Then apply the
                    // per-peer byte budget (upstream
                    // `txsSizeInflightPerPeer`) AND the global aggregate
                    // byte budget (upstream `maxTxsSizeInflight`).  The
                    // effective remaining byte budget is the minimum of
                    // per-peer remaining and global remaining, so any
                    // peer is bounded both by its own quota and by the
                    // shared global quota.  Greedily include candidates
                    // in advertised order while the running total stays
                    // at or below the budget; always admit at least one
                    // so the server makes forward progress even when a
                    // single tx exceeds the cap.  Remaining unfetched
                    // candidates are counted as `deferred` and are NOT
                    // acknowledged on the wire so the peer keeps them
                    // queued for re-advertisement once prior fetches
                    // drain.
                    let count_current = tx_state.peer_inflight_count(peer_addr);
                    let count_remaining = MAX_TXS_REQUESTED_PER_PEER.saturating_sub(count_current);
                    let (count_admitted, count_deferred) =
                        clamp_to_count_budget(&outcome.to_fetch, count_remaining);
                    let per_peer_current = tx_state.peer_inflight_bytes(peer_addr);
                    let per_peer_remaining =
                        MAX_TXS_SIZE_INFLIGHT_PER_PEER.saturating_sub(per_peer_current);
                    let global_current = tx_state.inflight_bytes_total();
                    let global_remaining =
                        MAX_TXS_SIZE_INFLIGHT_TOTAL.saturating_sub(global_current);
                    let budget_remaining = per_peer_remaining.min(global_remaining);
                    let (admitted, byte_deferred) = select_within_byte_budget(
                        &count_admitted,
                        &advertised_sizes,
                        budget_remaining,
                    );
                    let deferred = count_deferred + byte_deferred;
                    // Record sizes for per-peer / global byte accounting
                    // (upstream `requestedTxsInflightSize` /
                    // `inflightTxsSize`).  Falls back to size 0 if the
                    // peer omitted the size for an advertised txid.
                    let sized: Vec<_> = admitted
                        .iter()
                        .map(|t| (*t, advertised_sizes.get(t).copied().unwrap_or(0)))
                        .collect();
                    tx_state.mark_in_flight_sized(peer_addr, &sized);
                    (admitted, deferred)
                } else {
                    (all_txids, 0)
                };

                // Ack what we are consuming from the peer's queue: all
                // advertised entries except those deferred for budget.
                ack = advertised_count
                    .saturating_sub(deferred)
                    .min(u16::MAX as usize) as u16;

                let txs = {
                    let timeout = yggdrasil_network::protocol_limits::txsubmission::ST_TXS
                        .expect("txsubmission ST_TXS timeout constant must be set");
                    match tokio::time::timeout(timeout, server.request_txs(to_request.clone()))
                        .await
                    {
                        Ok(Ok(txs)) => {
                            if let Some(m) = metrics {
                                let bytes = TxSubmissionMessage::MsgRequestTxs {
                                    txids: to_request.clone(),
                                }
                                .to_cbor()
                                .len() as u64;
                                m.add_txsubmission_server_bytes_served_for_peer(peer, bytes);
                            }
                            txs
                        }
                        Ok(Err(e)) => {
                            if let Some((tx_state, peer_addr)) = &dedup {
                                tx_state.unregister_peer(peer_addr);
                            }
                            return Err(e);
                        }
                        Err(_elapsed) => {
                            if let Some((tx_state, peer_addr)) = &dedup {
                                tx_state.unregister_peer(peer_addr);
                            }
                            return Err(TxSubmissionServerError::Timeout);
                        }
                    }
                };

                // Verify advertised body sizes match actual received sizes.
                // Upstream reference: `txSubmissionInbound` validates each
                // received tx body against its advertised `TxSizeInBytes`.
                if txs.len() == to_request.len() {
                    for (tx_bytes, txid) in txs.iter().zip(to_request.iter()) {
                        if let Some(&advertised) = advertised_sizes.get(txid) {
                            let actual = tx_bytes.len() as u32;
                            if actual != advertised {
                                if let Some((tx_state, peer_addr)) = &dedup {
                                    tx_state.unregister_peer(peer_addr);
                                }
                                return Err(TxSubmissionServerError::UnexpectedMessage(format!(
                                    "body size mismatch for tx {}: advertised {} vs actual {}",
                                    hex::encode(txid.0),
                                    advertised,
                                    actual,
                                )));
                            }
                        }
                    }
                }

                // Track delivery outcome in the shared dedup state.  When
                // the peer returned exactly the bodies requested, mark
                // every requested TxId as received (added to the `known`
                // ring so no peer re-fetches it).  When the reply was
                // short (peer dropped some bodies between
                // `MsgReplyTxIds` and `MsgReplyTxs`, e.g. mempool
                // expiry), conservatively mark the entire batch as
                // not-found for THIS peer: the per-peer in-flight
                // count/byte counters drain, the TxIds are removed from
                // `global_in_flight` so another peer may supply them,
                // and `known` is NOT poisoned with TxIds whose body
                // never arrived.  Mirrors upstream
                // `Ouroboros.Network.TxSubmission.Inbound.V2.Server`
                // handling of partial `MsgReplyTxs` replies, which
                // splits the requested set into delivered vs missing
                // and routes the missing items back through the
                // not-acknowledged pathway for re-fetch.
                if let Some((tx_state, peer_addr)) = &dedup {
                    if txs.len() == to_request.len() {
                        tx_state.mark_received(peer_addr, &to_request);
                    } else {
                        tx_state.mark_not_found(peer_addr, &to_request);
                    }
                }

                let _accepted = consumer.consume_txs(txs);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PeerSharing server task
// ---------------------------------------------------------------------------

/// Run the PeerSharing server loop, serving known peers from a
/// [`PeerSharingProvider`].
///
/// Terminates when the client sends `MsgDone` or the connection drops.
pub async fn run_peersharing_server(
    mut server: PeerSharingServer,
    provider: &dyn PeerSharingProvider,
    metrics: Option<&yggdrasil_node_tracer::NodeMetrics>,
    peer: Option<SocketAddr>,
) -> Result<(), PeerSharingServerError> {
    loop {
        match server.recv_request().await? {
            PeerSharingServerRequest::ShareRequest { amount } => {
                let peers = provider.shareable_peers(amount);
                let bytes = PeerSharingMessage::MsgSharePeers {
                    peers: peers.clone(),
                }
                .to_cbor()
                .len() as u64;
                server.share_peers(peers).await?;
                if let Some(m) = metrics {
                    m.add_peersharing_server_bytes_served_for_peer(peer, bytes);
                }
            }
            PeerSharingServerRequest::Done => return Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// ChainDb-backed providers
// ---------------------------------------------------------------------------

/// Shared read handle to a [`ChainDb`] for concurrent provider access.
///
/// Follows the same `Arc<RwLock<T>>` pattern used by [`SharedMempool`] in the
/// mempool crate.  The sync pipeline holds the write lock during block
/// application; inbound server tasks take short-lived read locks for
/// lookups.
///
/// [`SharedMempool`]: yggdrasil_consensus::mempool::SharedMempool
#[derive(Clone, Debug)]
pub struct SharedChainDb<I, V, L> {
    inner: Arc<RwLock<ChainDb<I, V, L>>>,
    tentative: Option<Arc<RwLock<TentativeState>>>,
}

impl<I, V, L> SharedChainDb<I, V, L> {
    /// Wrap an existing [`ChainDb`] in a new shared handle.
    pub fn new(chain_db: ChainDb<I, V, L>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(chain_db)),
            tentative: None,
        }
    }

    /// Create a shared handle from a pre-existing `Arc`.
    pub fn from_arc(arc: Arc<RwLock<ChainDb<I, V, L>>>) -> Self {
        Self {
            inner: arc,
            tentative: None,
        }
    }

    /// Create a shared handle from a pre-existing `Arc` with a shared
    /// `TentativeState` for diffusion pipelining.
    pub fn from_arc_with_tentative(
        arc: Arc<RwLock<ChainDb<I, V, L>>>,
        tentative: Arc<RwLock<TentativeState>>,
    ) -> Self {
        Self {
            inner: arc,
            tentative: Some(tentative),
        }
    }

    /// Obtain a read-only reference to the underlying `Arc<RwLock<_>>`.
    pub fn inner(&self) -> &Arc<RwLock<ChainDb<I, V, L>>> {
        &self.inner
    }
}

impl<I, V, L> BlockProvider for SharedChainDb<I, V, L>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    fn get_block_range(&self, from: &[u8], to: &[u8]) -> Vec<Vec<u8>> {
        let from_point = match Point::from_cbor_bytes(from) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        let to_point = match Point::from_cbor_bytes(to) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        let to_hash = match to_point.hash() {
            Some(h) => h,
            None => return Vec::new(),
        };

        let db = match self.inner.read() {
            Ok(guard) => guard,
            Err(_) => return Vec::new(),
        };

        // Collect blocks in (from, to] across both stores.
        let mut blocks = Vec::new();

        if let Ok(suffix) = db.immutable().suffix_after(&from_point) {
            blocks.extend(suffix);
        }

        if let Some(pos) = blocks.iter().position(|b| b.header.hash == to_hash) {
            blocks.truncate(pos + 1);
            return blocks
                .into_iter()
                .filter_map(|b| b.raw_cbor.map(|arc| arc.to_vec()))
                .collect();
        }

        if let Ok(vol_prefix) = db.volatile().prefix_up_to(&to_point) {
            let skip = match from_point.hash() {
                Some(from_hash) => vol_prefix
                    .iter()
                    .position(|b| b.header.hash == from_hash)
                    .map(|pos| pos + 1)
                    .or_else(|| {
                        blocks.last().and_then(|last_immutable| {
                            vol_prefix
                                .iter()
                                .position(|b| b.header.slot_no > last_immutable.header.slot_no)
                        })
                    })
                    .unwrap_or(0),
                None => blocks
                    .last()
                    .and_then(|last_immutable| {
                        vol_prefix
                            .iter()
                            .position(|b| b.header.slot_no > last_immutable.header.slot_no)
                    })
                    .unwrap_or(0),
            };

            blocks.extend(vol_prefix.into_iter().skip(skip));
        }

        if let Some(pos) = blocks.iter().position(|b| b.header.hash == to_hash) {
            blocks.truncate(pos + 1);
            blocks
                .into_iter()
                .filter_map(|b| b.raw_cbor.map(|arc| arc.to_vec()))
                .collect()
        } else {
            Vec::new()
        }
    }
}

fn block_point(block: &yggdrasil_ledger::Block) -> Point {
    Point::BlockPoint(block.header.slot_no, block.header.hash)
}

/// R220 — encode the chain tip as the canonical upstream `Tip` envelope:
/// `[]` for genesis or `[point, blockNo]` for a known tip.  This is the
/// shape ChainSync `MsgRollForward { header, tip }` and `MsgIntersectFound
/// { point, tip }` consume on the wire — pre-R220 the server emitted
/// `Point::to_cbor_bytes()` (`[]` or `[slot, hash]`) which broke any
/// upstream-conforming client (including yggdrasil acting as a client)
/// with `point decode error: CBOR: type mismatch (expected major 4, got
/// 0)` because the third element (blockNo, a uint major 0) was absent
/// where the client expected the wrapping list.  Reference:
/// `Cardano.Slotting.Block` — `Tip blk = TipGenesis | Tip Point BlockNo`.
fn chain_tip_envelope_cbor<I, V, L>(db: &yggdrasil_storage::ChainDb<I, V, L>) -> Vec<u8>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    let tip_point = db.tip();
    let tip = match tip_point {
        Point::Origin => Tip::TipGenesis,
        Point::BlockPoint(_, hash) => {
            let block_no = db
                .volatile()
                .get_block(&hash)
                .or_else(|| db.immutable().get_block(&hash))
                .map(|b| b.header.block_no);
            match block_no {
                Some(bn) => Tip::Tip(tip_point, bn),
                None => Tip::TipGenesis,
            }
        }
    };
    let mut enc = Encoder::new();
    tip.encode_cbor(&mut enc);
    enc.into_bytes()
}

fn extract_chainsync_header(raw_block: &[u8]) -> Option<Vec<u8>> {
    mod era_tag {
        pub const BYRON_EBB: u64 = 0;
        pub const BYRON_MAIN: u64 = 1;
        pub const SHELLEY: u64 = 2;
        pub const ALLEGRA: u64 = 3;
        pub const MARY: u64 = 4;
        pub const ALONZO: u64 = 5;
        pub const BABBAGE: u64 = 6;
        pub const CONWAY: u64 = 7;
    }

    let mut dec = Decoder::new(raw_block);
    if dec.array().ok()? != 2 {
        return None;
    }

    let tag = dec.unsigned().ok()?;
    let body_start = dec.position();
    dec.skip().ok()?;
    let body_bytes = dec.slice(body_start, dec.position()).ok()?;

    match tag {
        era_tag::BYRON_EBB => match ByronBlock::decode_ebb(body_bytes).ok()? {
            ByronBlock::EpochBoundary { raw_header, .. } => Some(raw_header),
            ByronBlock::MainBlock { .. } => None,
        },
        era_tag::BYRON_MAIN => match ByronBlock::decode_main(body_bytes).ok()? {
            ByronBlock::MainBlock { raw_header, .. } => Some(raw_header),
            ByronBlock::EpochBoundary { .. } => None,
        },
        era_tag::SHELLEY
        | era_tag::ALLEGRA
        | era_tag::MARY
        | era_tag::ALONZO
        | era_tag::BABBAGE
        | era_tag::CONWAY => {
            let mut body_dec = Decoder::new(body_bytes);
            if let Some(0) = body_dec.array_begin().ok()? {
                return None;
            }
            Some(body_dec.raw_value().ok()?.to_vec())
        }
        _ => None,
    }
}

impl<I, V, L> ChainProvider for SharedChainDb<I, V, L>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    fn chain_tip(&self) -> Vec<u8> {
        let db = match self.inner.read() {
            Ok(guard) => guard,
            Err(_) => {
                // Fallback when read-lock is poisoned: emit the
                // genesis-tip envelope (`[]`).  R220 — must be the
                // upstream `Tip` shape, not bare `Point`.
                let mut enc = Encoder::new();
                Tip::TipGenesis.encode_cbor(&mut enc);
                return enc.into_bytes();
            }
        };
        chain_tip_envelope_cbor(&*db)
    }

    fn chain_tip_point(&self) -> Vec<u8> {
        // R221 — bare `Point` for use as `MsgRollBackward.point` and
        // chainsync cursor.  Distinct from `chain_tip()` which emits
        // the `Tip` envelope used at tip-slot positions.
        match self.inner.read() {
            Ok(db) => db.tip().to_cbor_bytes(),
            Err(_) => Point::Origin.to_cbor_bytes(),
        }
    }

    fn next_header(&self, cursor: &Option<Vec<u8>>) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        let cursor_point = match cursor {
            Some(bytes) => Point::from_cbor_bytes(bytes).ok()?,
            None => Point::Origin,
        };

        let db = self.inner.read().ok()?;
        let tip = db.tip();

        // No data beyond the cursor.
        if cursor_point == tip {
            return None;
        }

        // Get the first block after the cursor.
        let next = find_next_block(db.immutable(), db.volatile(), &cursor_point)?;

        let next_point = block_point(&next).to_cbor_bytes();
        let header_cbor = extract_chainsync_header(next.raw_cbor.as_deref()?)?;
        // R220 — encode tip as upstream `Tip` envelope, not bare `Point`.
        let tip_cbor = chain_tip_envelope_cbor(&*db);
        // `tip` is referenced for the cursor==tip equality check above.
        let _ = tip;
        Some((next_point, header_cbor, tip_cbor))
    }

    fn find_intersect(&self, points: &[Vec<u8>]) -> Option<(Vec<u8>, Vec<u8>)> {
        let db = self.inner.read().ok()?;
        // R220 — encode tip as upstream `Tip` envelope (`[]` or
        // `[point, blockNo]`), not bare `Point`.  Pre-R220 the server
        // emitted the wrong shape and ChainSync clients failed with
        // `CBOR type mismatch (expected major 4, got 0)`.
        let tip_cbor = chain_tip_envelope_cbor(&*db);

        // Walk the candidate list front-to-back; the client sends points
        // from most-recent to oldest, so the first hit is the best.
        for raw_point in points {
            if let Ok(point) = Point::from_cbor_bytes(raw_point) {
                let found = match point.hash() {
                    Some(h) => {
                        db.immutable().get_block(&h).is_some()
                            || db.volatile().get_block(&h).is_some()
                    }
                    None => true, // Origin always intersects.
                };
                if found {
                    return Some((raw_point.clone(), tip_cbor));
                }
            }
        }

        None
    }

    fn tentative_tip(&self) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        let tentative = self.tentative.as_ref()?;
        let ts = tentative.read().ok()?;
        let th = ts.tentative()?;

        // The tentative tip point is the tentative header's block point.
        let tip_point = Point::BlockPoint(th.slot, th.header_hash);
        let point_cbor = tip_point.to_cbor_bytes();
        let header_cbor = th.raw_header.clone();
        // R220 — encode tip as upstream `Tip` envelope (`[point, blockNo]`),
        // not bare `Point`.  Tentative headers may not have a known
        // BlockNo yet (the tentative is announced ahead of full
        // verification); fall back to genesis-shape envelope rather
        // than emitting an invalid wire shape.  Tentative headers
        // carry `block_number` per `TentativeHeader`; use it when
        // present.
        let tip_cbor = {
            let mut enc = Encoder::new();
            Tip::Tip(tip_point, th.block_no).encode_cbor(&mut enc);
            enc.into_bytes()
        };
        Some((point_cbor, header_cbor, tip_cbor))
    }
}

/// Find the first block strictly after `cursor` in the combined chain.
fn find_next_block<I: ImmutableStore, V: VolatileStore>(
    immutable: &I,
    volatile: &V,
    cursor: &Point,
) -> Option<yggdrasil_ledger::Block> {
    // If cursor is Origin, return the very first block.
    if *cursor == Point::Origin {
        let imm_suffix = immutable.suffix_after(&Point::Origin).ok()?;
        if let Some(first) = imm_suffix.into_iter().next() {
            return Some(first);
        }
        // No immutable blocks — try volatile.
        let vol_tip = volatile.tip();
        if vol_tip == Point::Origin {
            return None;
        }
        let vol_blocks = volatile.prefix_up_to(&vol_tip).ok()?;
        return vol_blocks.into_iter().next();
    }

    // Try immutable suffix after cursor (first element is next).
    if let Ok(suffix) = immutable.suffix_after(cursor) {
        if let Some(next) = suffix.into_iter().next() {
            return Some(next);
        }
        // cursor is the immutable tip — next is the first volatile block.
        let vol_tip = volatile.tip();
        if vol_tip != Point::Origin {
            if let Ok(vol_blocks) = volatile.prefix_up_to(&vol_tip) {
                return vol_blocks.into_iter().next();
            }
        }
        return None;
    }

    // cursor might be in volatile — find it and return the next.
    let vol_tip = volatile.tip();
    if vol_tip == Point::Origin {
        return None;
    }
    let vol_blocks = volatile.prefix_up_to(&vol_tip).ok()?;
    let cursor_hash = cursor.hash()?;
    let pos = vol_blocks
        .iter()
        .position(|b| b.header.hash == cursor_hash)?;
    vol_blocks.into_iter().nth(pos + 1)
}

// ---------------------------------------------------------------------------
// Inbound listener loop
// ---------------------------------------------------------------------------

/// Errors from the inbound listener service.
#[derive(Debug, thiserror::Error)]
pub enum InboundServiceError {
    /// Listener setup failed.
    #[error("listener error: {0}")]
    Listener(#[from] PeerListenerError),

    /// A protocol handle was missing from the accepted connection.
    #[error("missing protocol handle for inbound peer {addr}")]
    MissingProtocol { addr: SocketAddr },
}

/// Run the inbound connection accept loop.
///
/// Accepts connections on the given [`PeerListener`], builds an
/// [`InboundPeerSession`] for each, and spawns protocol server tasks.
/// When `block_provider` and `chain_provider` are supplied, BlockFetch
/// and ChainSync server tasks are spawned alongside KeepAlive.  When a
/// `peer_sharing_provider` is supplied, PeerSharing server tasks are
/// spawned for connections that negotiated the protocol.
///
/// The loop runs until the `shutdown` future resolves or a fatal listener
/// error occurs.
#[allow(clippy::too_many_arguments)]
pub async fn run_inbound_accept_loop<F: std::future::Future<Output = ()>>(
    listener: &PeerListener,
    block_provider: Option<Arc<dyn BlockProvider>>,
    chain_provider: Option<Arc<dyn ChainProvider>>,
    tx_submission_consumer: Option<Arc<dyn TxSubmissionConsumer>>,
    peer_sharing_provider: Option<Arc<dyn PeerSharingProvider>>,
    inbound_peers: Option<Arc<RwLock<BTreeMap<SocketAddr, NodePeerSharing>>>>,
    connection_manager: Option<Arc<RwLock<ConnectionManagerState>>>,
    inbound_governor: Option<Arc<RwLock<InboundGovernorState>>>,
    accepted_connections_limit: Option<AcceptedConnectionsLimit>,
    shared_tx_state: Option<SharedTxState>,
    tip_notify: Option<yggdrasil_node_runtime::ChainTipNotify>,
    tracer: Option<&yggdrasil_node_tracer::NodeTracer>,
    metrics: Option<&Arc<yggdrasil_node_tracer::NodeMetrics>>,
    shutdown: F,
) -> Result<(), InboundServiceError> {
    let listener_local_addr = listener
        .local_addr()
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
    let start = Instant::now();
    let mut inactivity_tick = tokio::time::interval(Duration::from_millis(31_400));
    let mut cm_timeout_tick = tokio::time::interval(Duration::from_secs(1));
    let mut session_tasks: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();
    let session_aborts = InboundSessionAborts::default();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            // Reap completed session tasks to free memory.
            Some(_) = session_tasks.join_next(), if !session_tasks.is_empty() => {}
            _ = cm_timeout_tick.tick(), if connection_manager.is_some() => {
                if let Some(shared_cm) = connection_manager.as_ref() {
                    process_connection_manager_timeouts(shared_cm, Some(&session_aborts));
                }
            }
            _ = inactivity_tick.tick() => {
                if let (Some(shared_ig), Some(shared_cm)) =
                    (inbound_governor.as_ref(), connection_manager.as_ref())
                {
                    process_inbound_governor_events(
                        shared_ig,
                        shared_cm,
                        Some(&session_aborts),
                        now_ms(&start),
                        vec![InboundGovernorEvent::InactivityTimeout],
                    );
                }
            }
            result = listener.accept_tcp() => {
                let (stream, addr) = result?;

                // -- Rate-limit check BEFORE handshake (upstream
                // `runConnectionRateLimits`).  Performing this check on the
                // bare TCP stream — not a fully handshaked connection —
                // means a hard-limit rejection costs only a TCP accept,
                // never a CBOR decode.  Combined with the bounded handshake
                // deadline in `PeerListener::handshake_on`, this closes the
                // unauth-remote DoS path documented in audit finding H-2.
                if let (Some(shared_cm), Some(limits)) =
                    (connection_manager.as_ref(), accepted_connections_limit.as_ref())
                {
                    let inbound_count = {
                        let cm = shared_cm.read().expect("connection manager lock poisoned");
                        cm.inbound_connection_count()
                    };
                    match rate_limit_decision(inbound_count, limits) {
                        RateLimitDecision::NoDelay => {}
                        RateLimitDecision::SoftDelay(d) => {
                            if let Some(t) = tracer {
                                t.trace_runtime(
                                    "Net.Inbound",
                                    "Debug",
                                    "soft delay before accepting inbound connection",
                                    yggdrasil_node_tracer::trace_fields([
                                        ("peer", serde_json::json!(addr.to_string())),
                                        ("delayMs", serde_json::json!(d.as_millis())),
                                        ("inboundCount", serde_json::json!(inbound_count)),
                                    ]),
                                );
                            }
                            tokio::time::sleep(d).await;
                        }
                        RateLimitDecision::HardLimit => {
                            if let Some(t) = tracer {
                                t.trace_runtime(
                                    "Net.Inbound",
                                    "Warning",
                                    "inbound connection rejected at hard limit",
                                    yggdrasil_node_tracer::trace_fields([
                                        ("peer", serde_json::json!(addr.to_string())),
                                        ("inboundCount", serde_json::json!(inbound_count)),
                                    ]),
                                );
                            }
                            if let Some(m) = metrics {
                                m.inc_inbound_rejected();
                            }
                            // At hard limit — drop the TCP stream before
                            // any handshake bytes are exchanged.  No mux
                            // tasks are spawned; no allocation occurs.
                            drop(stream);
                            continue;
                        }
                    }
                }

                // Now run the handshake with a hard outer deadline so a
                // stalled or slowloris peer cannot occupy the accept loop
                // indefinitely (audit finding H-2).
                let conn = match listener.handshake_on(stream, addr).await {
                    Ok(conn) => conn,
                    Err(e) => {
                        if let Some(t) = tracer {
                            t.trace_runtime(
                                "Net.Inbound",
                                "Warning",
                                "inbound handshake failed",
                                yggdrasil_node_tracer::trace_fields([
                                    ("peer", serde_json::json!(addr.to_string())),
                                    ("error", serde_json::json!(e.to_string())),
                                ]),
                            );
                        }
                        if let Some(m) = metrics {
                            m.inc_inbound_rejected();
                        }
                        continue;
                    }
                };

                let data_flow = if conn.version_data.initiator_only_diffusion_mode {
                    DataFlow::Unidirectional
                } else {
                    DataFlow::Duplex
                };
                let conn_id = ConnectionId {
                    local: listener_local_addr,
                    remote: addr,
                };

                if let Some(shared_cm) = connection_manager.as_ref() {
                    let include_result = {
                        let mut cm = shared_cm
                            .write()
                            .expect("connection manager lock poisoned");
                        cm.include_inbound_connection(conn_id)
                    };

                    let (_, cm_actions) = match include_result {
                        Ok(result) => result,
                        Err(_) => {
                            conn.mux.abort();
                            continue;
                        }
                    };

                    let handshake_result = {
                        let mut cm = shared_cm
                            .write()
                            .expect("connection manager lock poisoned");
                        cm.inbound_handshake_done(addr, data_flow)
                    };
                    if handshake_result.is_err() {
                        conn.mux.abort();
                        continue;
                    }

                    let should_abort = cm_actions.into_iter().any(|action| {
                        matches!(
                            action,
                            CmAction::TerminateConnection(cid) if cid.remote == addr
                        )
                    });
                    if should_abort {
                        conn.mux.abort();
                        continue;
                    }
                }

                if let (Some(shared_ig), Some(shared_cm)) =
                    (inbound_governor.as_ref(), connection_manager.as_ref())
                {
                    let actions = {
                        let mut ig = shared_ig
                            .write()
                            .expect("inbound governor lock poisoned");
                        ig.new_connection_with_data_flow(
                            conn_id,
                            data_flow,
                            now_ms(&start),
                        )
                    };
                    apply_inbound_governor_actions(
                        shared_ig,
                        shared_cm,
                        Some(&session_aborts),
                        actions,
                    );
                }

                let session = InboundPeerSession::from_connection(conn, addr)
                    .ok_or(InboundServiceError::MissingProtocol { addr })?;
                session_aborts.insert(addr, &session.mux);

                if let Some(t) = tracer {
                    t.trace_runtime(
                        "Net.Inbound",
                        "Info",
                        "inbound peer session started",
                        yggdrasil_node_tracer::trace_fields([
                            ("peer", serde_json::json!(addr.to_string())),
                            ("dataFlow", serde_json::json!(format!("{:?}", data_flow))),
                            ("peerSharing", serde_json::json!(session.peer_sharing.is_some())),
                        ]),
                    );
                }
                if let Some(m) = metrics {
                    m.inc_inbound_accepted();
                }

                let peer_sharing_mode = if session.peer_sharing.is_some() {
                    NodePeerSharing::PeerSharingEnabled
                } else {
                    NodePeerSharing::PeerSharingDisabled
                };
                if let Some(shared_inbound_peers) = inbound_peers.as_ref() {
                    if let Ok(mut peers) = shared_inbound_peers.write() {
                        peers.insert(addr, peer_sharing_mode);
                    }
                }

                let bp = block_provider.clone();
                let cp = chain_provider.clone();
                let tx_consumer = tx_submission_consumer.clone();
                let ps_provider = peer_sharing_provider.clone();
                let shared_inbound_peers = inbound_peers.clone();
                let shared_cm = connection_manager.clone();
                let shared_ig = inbound_governor.clone();
                let session_aborts_clone = session_aborts.clone();
                let session_tx_state = shared_tx_state.clone();
                let session_tip_notify = tip_notify.clone();
                // R234 — Phase D.2 bytes-out: clone the metrics
                // handle for the BlockFetch responder spawn so it
                // can record server-side bytes-served.
                let bf_metrics: Option<Arc<yggdrasil_node_tracer::NodeMetrics>> =
                    metrics.cloned();
                let remote_addr = session.remote_addr;
                let connection_id = conn_id;
                let base = start;
                let responder_counters = Arc::new(tokio::sync::Mutex::new(
                    ResponderCounters::default(),
                ));

                session_tasks.spawn(async move {
                    let ka = {
                        let shared_ig = shared_ig.clone();
                        let shared_cm = shared_cm.clone();
                        let session_aborts = session_aborts_clone.clone();
                        let responder_counters = responder_counters.clone();
                        let ka_metrics = bf_metrics.clone();
                        tokio::spawn(async move {
                            if let (Some(ig), Some(cm)) =
                                (shared_ig.as_ref(), shared_cm.as_ref())
                            {
                                let counters = {
                                    let mut counters = responder_counters.lock().await;
                                    counters.non_hot_responders += 1;
                                    *counters
                                };
                                update_inbound_responder_counters(
                                    ig,
                                    cm,
                                    Some(&session_aborts),
                                    connection_id.remote,
                                    counters,
                                    now_ms(&base),
                                );
                            }

                            let _ = run_keepalive_server(
                                session.keep_alive,
                                ka_metrics.as_deref(),
                                Some(connection_id.remote),
                            )
                            .await;

                            if let (Some(ig), Some(cm)) =
                                (shared_ig.as_ref(), shared_cm.as_ref())
                            {
                                let counters = {
                                    let mut counters = responder_counters.lock().await;
                                    counters.non_hot_responders =
                                        counters.non_hot_responders.saturating_sub(1);
                                    *counters
                                };
                                update_inbound_responder_counters(
                                    ig,
                                    cm,
                                    Some(&session_aborts),
                                    connection_id.remote,
                                    counters,
                                    now_ms(&base),
                                );
                            }
                        })
                    };

                    let bf = bp.map(|provider| {
                        let shared_ig = shared_ig.clone();
                        let shared_cm = shared_cm.clone();
                        let session_aborts = session_aborts_clone.clone();
                        let responder_counters = responder_counters.clone();
                        // R234 — clone the metrics handle into the
                        // BlockFetch responder spawn so
                        // `run_blockfetch_server` can record
                        // server-side bytes-served (Phase D.2 bytes-out
                        // initial slice).
                        let bf_metrics = bf_metrics.clone();
                        tokio::spawn(async move {
                            if let (Some(ig), Some(cm)) =
                                (shared_ig.as_ref(), shared_cm.as_ref())
                            {
                                let counters = {
                                    let mut counters = responder_counters.lock().await;
                                    counters.hot_responders += 1;
                                    *counters
                                };
                                update_inbound_responder_counters(
                                    ig,
                                    cm,
                                    Some(&session_aborts),
                                    connection_id.remote,
                                    counters,
                                    now_ms(&base),
                                );
                            }

                            let _ = run_blockfetch_server(
                                session.block_fetch,
                                &*provider,
                                bf_metrics.as_deref(),
                                Some(connection_id.remote),
                            )
                            .await;

                            if let (Some(ig), Some(cm)) =
                                (shared_ig.as_ref(), shared_cm.as_ref())
                            {
                                let counters = {
                                    let mut counters = responder_counters.lock().await;
                                    counters.hot_responders =
                                        counters.hot_responders.saturating_sub(1);
                                    *counters
                                };
                                update_inbound_responder_counters(
                                    ig,
                                    cm,
                                    Some(&session_aborts),
                                    connection_id.remote,
                                    counters,
                                    now_ms(&base),
                                );
                            }
                        })
                    });

                    let cs = cp.map(|provider| {
                        let shared_ig = shared_ig.clone();
                        let shared_cm = shared_cm.clone();
                        let session_aborts = session_aborts_clone.clone();
                        let responder_counters = responder_counters.clone();
                        let notify = session_tip_notify.clone();
                        // R235 — clone metrics into ChainSync responder
                        // spawn for server-side bytes-out accounting.
                        let cs_metrics = bf_metrics.clone();
                        tokio::spawn(async move {
                            if let (Some(ig), Some(cm)) =
                                (shared_ig.as_ref(), shared_cm.as_ref())
                            {
                                let counters = {
                                    let mut counters = responder_counters.lock().await;
                                    counters.hot_responders += 1;
                                    *counters
                                };
                                update_inbound_responder_counters(
                                    ig,
                                    cm,
                                    Some(&session_aborts),
                                    connection_id.remote,
                                    counters,
                                    now_ms(&base),
                                );
                            }

                            let _ = run_chainsync_server(
                                session.chain_sync,
                                &*provider,
                                notify,
                                cs_metrics.as_deref(),
                                Some(connection_id.remote),
                            )
                            .await;

                            if let (Some(ig), Some(cm)) =
                                (shared_ig.as_ref(), shared_cm.as_ref())
                            {
                                let counters = {
                                    let mut counters = responder_counters.lock().await;
                                    counters.hot_responders =
                                        counters.hot_responders.saturating_sub(1);
                                    *counters
                                };
                                update_inbound_responder_counters(
                                    ig,
                                    cm,
                                    Some(&session_aborts),
                                    connection_id.remote,
                                    counters,
                                    now_ms(&base),
                                );
                            }
                        })
                    });

                    let tx = tx_consumer.map(|consumer| {
                        let shared_ig = shared_ig.clone();
                        let shared_cm = shared_cm.clone();
                        let session_aborts = session_aborts_clone.clone();
                        let responder_counters = responder_counters.clone();
                        let dedup = session_tx_state.as_ref().map(|ts| (ts.clone(), connection_id.remote));
                        let tx_metrics = bf_metrics.clone();
                        tokio::spawn(async move {
                            if let (Some(ig), Some(cm)) =
                                (shared_ig.as_ref(), shared_cm.as_ref())
                            {
                                let counters = {
                                    let mut counters = responder_counters.lock().await;
                                    counters.hot_responders += 1;
                                    *counters
                                };
                                update_inbound_responder_counters(
                                    ig,
                                    cm,
                                    Some(&session_aborts),
                                    connection_id.remote,
                                    counters,
                                    now_ms(&base),
                                );
                            }

                            let dedup_ref = dedup.as_ref().map(|(ts, addr)| (ts, *addr));
                            let _ = run_txsubmission_server(
                                session.tx_submission,
                                &*consumer,
                                dedup_ref,
                                tx_metrics.as_deref(),
                                Some(connection_id.remote),
                            )
                            .await;

                            if let (Some(ig), Some(cm)) =
                                (shared_ig.as_ref(), shared_cm.as_ref())
                            {
                                let counters = {
                                    let mut counters = responder_counters.lock().await;
                                    counters.hot_responders =
                                        counters.hot_responders.saturating_sub(1);
                                    *counters
                                };
                                update_inbound_responder_counters(
                                    ig,
                                    cm,
                                    Some(&session_aborts),
                                    connection_id.remote,
                                    counters,
                                    now_ms(&base),
                                );
                            }
                        })
                    });

                    let ps = session.peer_sharing.and_then(|server| {
                        ps_provider.map(|provider| {
                            let shared_ig = shared_ig.clone();
                            let shared_cm = shared_cm.clone();
                            let session_aborts = session_aborts_clone.clone();
                            let responder_counters = responder_counters.clone();
                            let ps_metrics = bf_metrics.clone();
                            tokio::spawn(async move {
                                if let (Some(ig), Some(cm)) =
                                    (shared_ig.as_ref(), shared_cm.as_ref())
                                {
                                    let counters = {
                                        let mut counters = responder_counters.lock().await;
                                        counters.non_hot_responders += 1;
                                        *counters
                                    };
                                    update_inbound_responder_counters(
                                        ig,
                                        cm,
                                        Some(&session_aborts),
                                        connection_id.remote,
                                        counters,
                                        now_ms(&base),
                                    );
                                }

                                let _ = run_peersharing_server(
                                    server,
                                    &*provider,
                                    ps_metrics.as_deref(),
                                    Some(connection_id.remote),
                                )
                                .await;

                                if let (Some(ig), Some(cm)) =
                                    (shared_ig.as_ref(), shared_cm.as_ref())
                                {
                                    let counters = {
                                        let mut counters = responder_counters.lock().await;
                                        counters.non_hot_responders =
                                            counters.non_hot_responders.saturating_sub(1);
                                        *counters
                                    };
                                    update_inbound_responder_counters(
                                        ig,
                                        cm,
                                        Some(&session_aborts),
                                        connection_id.remote,
                                        counters,
                                        now_ms(&base),
                                    );
                                }
                            })
                        })
                    });

                    // Wait for KeepAlive to finish (indicates peer disconnected
                    // or sent MsgDone). Then abort the remaining tasks.
                    let _ = ka.await;
                    if let Some(h) = bf { h.abort(); }
                    if let Some(h) = cs { h.abort(); }
                    if let Some(h) = tx { h.abort(); }
                    if let Some(h) = ps { h.abort(); }
                    session.mux.abort();

                    if let (Some(shared_ig), Some(cm_state)) =
                        (shared_ig.as_ref(), shared_cm.as_ref())
                    {
                        process_inbound_governor_events(
                            shared_ig,
                            cm_state,
                            Some(&session_aborts_clone),
                            now_ms(&base),
                            vec![InboundGovernorEvent::MuxFinished(connection_id)],
                        );
                    }

                    if let Some(cm_state) = shared_cm {
                        let mut cm = match cm_state.write() {
                            Ok(guard) => guard,
                            Err(_) => return,
                        };
                        let (_release_result, cm_actions) =
                            cm.release_inbound_connection(remote_addr);
                        execute_cm_actions(cm_actions, Some(&session_aborts_clone));
                        let _ = cm.mark_terminating(
                            remote_addr,
                            Some("inbound session ended".to_owned()),
                        );
                        let _ = cm.time_wait_expired(remote_addr);
                        let _ = cm.remove_terminated(&remote_addr);
                    }

                    if let Some(shared_peers) = shared_inbound_peers {
                        if let Ok(mut peers) = shared_peers.write() {
                            peers.remove(&remote_addr);
                        }
                    }
                    session_aborts_clone.remove(&remote_addr);
                });
            }
        }
    }

    // -- Graceful shutdown: drain active inbound sessions --
    // Upstream `Ouroboros.Network.Server2` shutdown sequence:
    // 1. Stop accepting new connections (loop already exited)
    // 2. Signal CommitRemote for all tracked inbound peers so IG/CM
    //    can begin their teardown transitions.
    // 3. Wait up to INBOUND_DRAIN_TIMEOUT for session tasks to finish.
    // 4. Abort any remaining tasks that haven't exited.
    const INBOUND_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

    if let (Some(shared_ig), Some(shared_cm)) =
        (inbound_governor.as_ref(), connection_manager.as_ref())
    {
        let tracked_peers: Vec<(SocketAddr, ConnectionId)> = {
            let ig = shared_ig.read().expect("inbound governor lock poisoned");
            ig.connections
                .iter()
                .map(|(&peer, entry)| (peer, entry.conn_id))
                .collect()
        };

        for (_peer, conn_id) in &tracked_peers {
            process_inbound_governor_events(
                shared_ig,
                shared_cm,
                Some(&session_aborts),
                now_ms(&start),
                vec![InboundGovernorEvent::CommitRemote(*conn_id)],
            );
        }
    }

    // Wait for active session tasks with a bounded timeout.
    let drain_deadline = tokio::time::Instant::now() + INBOUND_DRAIN_TIMEOUT;
    while !session_tasks.is_empty() {
        match tokio::time::timeout_at(drain_deadline, session_tasks.join_next()).await {
            Ok(Some(_)) => {}  // task completed
            Ok(None) => break, // JoinSet empty
            Err(_) => break,   // timeout expired
        }
    }

    // Force-abort anything still running.
    session_tasks.shutdown().await;

    Ok(())
}

#[cfg(test)]
mod tests;
