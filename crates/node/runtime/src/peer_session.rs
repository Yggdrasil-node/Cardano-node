//! NodeConfig + PeerSession + reconnecting verified-sync request types.
//!
//! Mirrors upstream:
//! - `Ouroboros.Network.NodeToNode` connection bootstrap config + the
//!   per-peer mini-protocol client bundle that the runtime holds while
//!   driving a sync session
//! - `Ouroboros.Consensus.Node.Run` reconnect/resume request shape used
//!   when the verified-sync service must rebind to a fresh peer after
//!   ChainSync rollback or peer disconnect
//!
//! Six top-level types:
//! - `NodeConfig` — peer address, network magic, handshake versions,
//!   topology subset for bootstrap.
//! - `PeerSession` — owned bundle of the 5 mini-protocol clients
//!   (ChainSync, BlockFetch, KeepAlive, TxSubmission, PeerSharing) +
//!   the connection-manager state + the abstract-state sender.
//! - `ReconnectingSyncServiceOutcome` / `ResumedSyncServiceOutcome` —
//!   service-exit summaries.
//! - `ReconnectingVerifiedSyncRequest<'a>` /
//!   `ResumeReconnectingVerifiedSyncRequest<'a>` — builder structs for
//!   the verified-sync entry points; both use the `with_*` builder
//!   pattern for the long list of optional cross-task shared handles.
//!
//! Extracted from `runtime.rs` in R271f.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side per-peer session bundle
//! (NodeConfig + PeerSession + verified-sync reconnect-request
//! shapes). Upstream `Ouroboros.Network.NodeToNode` and
//! `Ouroboros.Consensus.Node.Run` carry this state across multiple
//! structs/threads; Yggdrasil unifies them in one async-runtime
//! session bundle.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use yggdrasil_consensus::mempool::{SharedMempool, SharedTxState};
use yggdrasil_consensus::{ChainState, NonceEvolutionState, TentativeState};
use yggdrasil_ledger::{LedgerState, Point};
use yggdrasil_network::{
    BlockFetchClient, ChainSyncClient, HandshakeVersion, KeepAliveClient, MiniProtocolNum,
    NodeToNodeVersionData, PeerRegistry, PeerSharingClient, TxSubmissionClient, UseLedgerPeers,
};
use yggdrasil_storage::LedgerRecoveryOutcome;

use yggdrasil_node_sync::VerifiedSyncServiceConfig;
use yggdrasil_node_tracer::NodeMetrics;

use super::ChainTipNotify;
use super::block_producer_config::SharedBlockProducerState;

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
    ///
    /// `Some` while the session retains direct ownership of the
    /// BlockFetch wire handle (legacy single-peer path).  Becomes
    /// `None` after [`PeerSession::take_block_fetch`] migrates the
    /// handle into a per-peer
    /// [`yggdrasil_node_sync::blockfetch_worker::FetchWorkerHandle`] for the
    /// multi-peer dispatch path.  Once migrated, the worker owns
    /// the handle until disconnect; the sync loop reaches the
    /// peer through the
    /// [`yggdrasil_node_sync::blockfetch_worker::FetchWorkerPool`] instead of
    /// touching this field.
    pub block_fetch: Option<BlockFetchClient>,
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

impl PeerSession {
    /// Returns a mutable reference to the BlockFetch client, panicking
    /// with a descriptive message if the handle has already been
    /// migrated into a [`yggdrasil_node_sync::blockfetch_worker::FetchWorkerHandle`].
    /// Callers operating on the legacy single-peer path use this
    /// helper rather than `block_fetch.as_mut().unwrap()` so the
    /// failure message points at the design contract instead of an
    /// opaque `unwrap` panic.
    pub fn block_fetch_mut(&mut self) -> &mut BlockFetchClient {
        self.block_fetch.as_mut().expect(
            "PeerSession.block_fetch was migrated to a FetchWorkerHandle; \
             callers on the legacy direct-fetch path must check `has_block_fetch()` first",
        )
    }

    /// Returns `true` if the BlockFetch client is still owned directly
    /// by the session (legacy single-peer path).  Returns `false`
    /// after `take_block_fetch` has migrated the handle into a
    /// per-peer worker.
    pub fn has_block_fetch(&self) -> bool {
        self.block_fetch.is_some()
    }

    /// Take ownership of the BlockFetch client from the session.
    ///
    /// The caller is expected to spawn a per-peer worker around the
    /// returned handle via
    /// [`yggdrasil_node_sync::blockfetch_worker::FetchWorkerHandle::spawn_with_block_fetch_client`]
    /// and register the worker in a
    /// [`yggdrasil_node_sync::blockfetch_worker::FetchWorkerPool`].  Subsequent
    /// fetches for this peer must go through the pool — the
    /// `block_fetch` field is left as `None` and any direct
    /// access via `block_fetch_mut` panics with a descriptive
    /// message.
    ///
    /// Returns `None` if the handle has already been migrated.
    /// Mirrors upstream `bracketSyncWithFetchClient` lifecycle from
    /// `Ouroboros.Network.BlockFetch.ClientRegistry`: the per-peer
    /// fetch state is owned by the fetch task for the connection's
    /// lifetime.
    pub fn take_block_fetch(&mut self) -> Option<BlockFetchClient> {
        self.block_fetch.take()
    }
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
    /// Optional storage directory under which slot-indexed ChainDepState
    /// sidecars are persisted whenever a ledger checkpoint is written. When
    /// `Some(path)`, the runtime restores nonce/OpCert state from the exact
    /// recovered point on restart and requires exact history for persistent
    /// non-origin rollback.
    pub chain_dep_persist_dir: Option<PathBuf>,
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
            chain_dep_persist_dir: None,
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

    /// Enable atomic persistence of slot-indexed ChainDepState sidecars under
    /// `dir` whenever a ledger checkpoint is written. Pass `None` (the
    /// default) for non-persistent test-style runs.
    pub fn with_chain_dep_persist_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.chain_dep_persist_dir = dir;
        self
    }

    /// Attach a shared TxSubmission inbound dedup state so the eviction
    /// pipeline can call [`SharedTxState::mark_confirmed`] for every
    /// confirmed roll-forward batch.  Mirrors upstream
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.State` `bufferedTxs`
    /// population on confirmation.
    pub fn with_inbound_tx_state(mut self, inbound_tx_state: Option<SharedTxState>) -> Self {
        self.inbound_tx_state = inbound_tx_state;
        self
    }
}
