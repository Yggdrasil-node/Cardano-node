//! Inbound governor state types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** Ouroboros/Network/InboundGovernor/State.hs.
//! Filename flattens the upstream directory; the file carries the
//! state-record definitions (`InboundConnectionEntry`,
//! `InboundGovernorState`) plus their constructors and pure
//! accessors. The runtime step-function and event-handlers live
//! in the sibling `inbound_governor.rs` (mirroring upstream's
//! `InboundGovernor.hs`).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use crate::connection::{
    ConnectionId, DataFlow, InboundGovernorCounters, RemoteSt, ResponderCounters,
};

// ---------------------------------------------------------------------------
// Per-connection state
// ---------------------------------------------------------------------------

/// State tracked by the inbound governor for a single inbound connection.
///
/// Upstream: `ConnectionState` in `InboundGovernor/State.hs` (the IG-level
/// state, not the CM-level `ConnectionState` from `ConnectionManager.Types`).
#[derive(Clone, Debug)]
pub struct InboundConnectionEntry {
    /// The full connection identifier (local + remote addresses).
    pub conn_id: ConnectionId,
    /// Remote peer state as seen by the inbound governor.
    pub remote_st: RemoteSt,
    /// Negotiated data-flow mode for this connection.
    pub data_flow: DataFlow,
    /// Mini-protocol responder counters for this connection.
    pub responder_counters: ResponderCounters,
    /// Monotonic timestamp (milliseconds since reference) when the connection
    /// entered the `RemoteIdleSt` state. `None` when not idle.
    pub idle_since_ms: Option<u64>,
    /// Monotonic timestamp (milliseconds since reference) when the connection
    /// was first established. Used for duplex peer maturation.
    pub connected_at_ms: u64,
}

// ---------------------------------------------------------------------------
// Inbound governor state
// ---------------------------------------------------------------------------

/// Mutable state of the inbound governor.
///
/// Upstream: `InboundGovernor.State.State` — tracks all inbound connections,
/// duplex peer maturation queues, and aggregate counters.
#[derive(Clone, Debug)]
pub struct InboundGovernorState {
    /// Per-peer inbound connection state, keyed by remote address.
    ///
    /// Upstream: `connections :: Map (ConnectionId peerAddr) ConnectionState`.
    pub connections: BTreeMap<SocketAddr, InboundConnectionEntry>,

    /// Duplex peers that have been connected for ≥ `MATURE_PEER_DELAY`
    /// (15 min). These are exposed for peer sharing.
    ///
    /// Upstream: `matureDuplexPeers :: Map peerAddr versionData`.
    pub mature_duplex_peers: BTreeMap<SocketAddr, u64>,

    /// Duplex peers not yet mature, keyed by remote address with their
    /// connection timestamp.
    ///
    /// Upstream: `freshDuplexPeers :: OrdPSQ peerAddr Time versionData`.
    pub fresh_duplex_peers: BTreeMap<SocketAddr, u64>,

    /// Cached aggregate counters, recomputed after each step.
    pub counters: InboundGovernorCounters,

    /// Protocol idle timeout — how long a remote peer can remain idle
    /// before `CommitRemote` fires.
    ///
    /// Upstream: `PROTOCOL_IDLE_TIMEOUT` (5 s by default in N2N).
    pub protocol_idle_timeout_ms: u64,
}

impl InboundGovernorState {
    /// Create a new, empty inbound governor state with the default
    /// protocol idle timeout.
    pub fn new() -> Self {
        Self {
            connections: BTreeMap::new(),
            mature_duplex_peers: BTreeMap::new(),
            fresh_duplex_peers: BTreeMap::new(),
            counters: InboundGovernorCounters::default(),
            protocol_idle_timeout_ms: 5_000,
        }
    }

    /// Create a new state with a custom protocol idle timeout.
    pub fn with_idle_timeout(idle_timeout: Duration) -> Self {
        Self {
            protocol_idle_timeout_ms: idle_timeout.as_millis() as u64,
            ..Self::new()
        }
    }

    // -----------------------------------------------------------------------
    // Pure accessors (no side effects, no state mutation)
    // -----------------------------------------------------------------------

    /// Number of tracked inbound connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Look up the remote state of a specific peer.
    pub fn remote_state(&self, peer: &SocketAddr) -> Option<RemoteSt> {
        self.connections.get(peer).map(|e| e.remote_st)
    }

    /// Set of mature duplex peer addresses, suitable for peer sharing.
    pub fn mature_duplex_peer_set(&self) -> &BTreeMap<SocketAddr, u64> {
        &self.mature_duplex_peers
    }
}

impl Default for InboundGovernorState {
    fn default() -> Self {
        Self::new()
    }
}
