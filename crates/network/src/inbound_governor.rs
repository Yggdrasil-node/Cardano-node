//! Inbound governor decision engine.
//!
//! This module implements the upstream `Ouroboros.Network.InboundGovernor`
//! state machine as a pure step function. Each call to
//! [`InboundGovernorState::step`] processes a single
//! [`InboundGovernorEvent`] and returns a list of
//! [`InboundGovernorAction`] values representing calls to the connection
//! manager.
//!
//! The design mirrors our outbound governor (`governor.rs`): all decisions
//! are pure and testable; effectful connection management stays in `node/`.
//!
//! Reference: `ouroboros-network-framework/src/Ouroboros/Network/InboundGovernor.hs`
//! and `InboundGovernor/State.hs`.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use crate::connection::{
    ConnectionId, DataFlow, DemotedToColdRemoteTr, InboundGovernorCounters, InboundGovernorEvent,
    RemoteSt, ResponderCounters, inbound_constants,
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
// Actions produced by the IG step function
// ---------------------------------------------------------------------------

/// An action that the inbound governor step function asks the connection
/// manager to perform.
///
/// These mirror the four CM method calls that the upstream IG makes:
/// `promotedToWarmRemote`, `demotedToColdRemote`, `releaseInboundConnection`,
/// and connection unregistration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InboundGovernorAction {
    /// Ask the CM to promote the remote side of a connection from idle/cold
    /// to warm.
    ///
    /// Upstream: `promotedToWarmRemote connectionManager connId`.
    PromotedToWarmRemote(ConnectionId),

    /// Ask the CM to demote the remote side of a connection from
    /// established/warm to idle.
    ///
    /// Upstream: `demotedToColdRemote connectionManager connId`.
    DemotedToColdRemote(ConnectionId),

    /// Ask the CM to release an inbound connection (commit or keep).
    ///
    /// Upstream: `releaseInboundConnection connectionManager connId`.
    /// The CM will return either `CommitTr` (connection closed) or
    /// `KeepTr` (outbound side still using it).
    ReleaseInboundConnection(ConnectionId),

    /// Unregister the connection from the IG's tracking map.
    /// This is an IG-internal operation triggered by `MuxFinished`.
    UnregisterConnection(ConnectionId),
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
    // Accessors
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

    // -----------------------------------------------------------------------
    // Counter recomputation
    // -----------------------------------------------------------------------

    /// Recompute aggregate counters from tracked connections.
    fn recompute_counters(&mut self) {
        let mut c = InboundGovernorCounters::default();
        for entry in self.connections.values() {
            c.count_state(entry.remote_st);
        }
        self.counters = c;
    }

    // -----------------------------------------------------------------------
    // Duplex peer maturation
    // -----------------------------------------------------------------------

    /// Mature any fresh duplex peers whose connection age exceeds
    /// `MATURE_PEER_DELAY` (15 min).
    ///
    /// Returns the number of peers newly matured.
    ///
    /// Upstream: `maturedPeers` STM action in `InboundGovernor.hs` uses
    /// `OrdPSQ.atMostView (addTime (-inboundMaturePeerDelay) now) freshDuplexPeers`.
    pub fn mature_peers(&mut self, now_ms: u64) -> usize {
        let threshold_ms = inbound_constants::MATURE_PEER_DELAY.as_millis() as u64;
        let cutoff = now_ms.saturating_sub(threshold_ms);

        let mut newly_matured = Vec::new();
        for (addr, &connected_at) in &self.fresh_duplex_peers {
            if connected_at <= cutoff {
                newly_matured.push(*addr);
            }
        }

        for addr in &newly_matured {
            if let Some(connected_at) = self.fresh_duplex_peers.remove(addr) {
                self.mature_duplex_peers.insert(*addr, connected_at);
            }
        }

        newly_matured.len()
    }

    // -----------------------------------------------------------------------
    // Core step function
    // -----------------------------------------------------------------------

    /// Process a single inbound governor event and return the resulting
    /// actions for the connection manager.
    ///
    /// This is the pure decision core of the inbound governor. The caller
    /// is responsible for feeding events and executing the returned actions
    /// against the actual connection manager.
    ///
    /// `now_ms` is the current monotonic time in milliseconds.
    ///
    /// Upstream: `inboundGovernorStep` processes a batch of events via
    /// `foldM` — here we process one at a time for simplicity.
    pub fn step(&mut self, event: InboundGovernorEvent, now_ms: u64) -> Vec<InboundGovernorAction> {
        let actions = match event {
            InboundGovernorEvent::NewConnection(conn_id) => {
                self.handle_new_connection(conn_id, now_ms)
            }
            InboundGovernorEvent::MuxFinished(conn_id) => self.handle_mux_finished(conn_id),
            InboundGovernorEvent::MiniProtocolTerminated(conn_id) => {
                self.handle_mini_protocol_terminated(conn_id)
            }
            InboundGovernorEvent::WaitIdleRemote(conn_id) => self.handle_wait_idle_remote(conn_id),
            InboundGovernorEvent::AwakeRemote(conn_id) => self.handle_awake_remote(conn_id),
            InboundGovernorEvent::RemotePromotedToHot(conn_id) => {
                self.handle_remote_promoted_to_hot(conn_id)
            }
            InboundGovernorEvent::RemoteDemotedToWarm(conn_id) => {
                self.handle_remote_demoted_to_warm(conn_id)
            }
            InboundGovernorEvent::CommitRemote(conn_id) => self.handle_commit_remote(conn_id),
            InboundGovernorEvent::MaturedDuplexPeers => self.handle_matured_duplex_peers(now_ms),
            InboundGovernorEvent::InactivityTimeout => self.handle_inactivity_timeout(now_ms),
        };

        self.recompute_counters();
        actions
    }

    // -----------------------------------------------------------------------
    // Event handlers
    // -----------------------------------------------------------------------

    /// Handle a new inbound connection.
    ///
    /// Register the connection in `RemoteIdleSt` state. If the connection
    /// is duplex, insert it into the fresh-duplex-peers queue for maturation
    /// tracking.
    ///
    /// If a connection from this peer already exists, preserve the existing
    /// state (upstream behavior for connection reuse).
    fn handle_new_connection(
        &mut self,
        conn_id: ConnectionId,
        now_ms: u64,
    ) -> Vec<InboundGovernorAction> {
        let peer = conn_id.remote;

        // If already tracked, preserve existing state (upstream preserves).
        if self.connections.contains_key(&peer) {
            return Vec::new();
        }

        let entry = InboundConnectionEntry {
            conn_id,
            remote_st: RemoteSt::RemoteIdleSt,
            data_flow: DataFlow::Duplex, // default to duplex; real data flow comes from negotiation
            responder_counters: ResponderCounters::default(),
            idle_since_ms: Some(now_ms),
            connected_at_ms: now_ms,
        };

        self.connections.insert(peer, entry);

        // Track duplex peers for maturation.
        self.fresh_duplex_peers.insert(peer, now_ms);

        Vec::new()
    }

    /// Handle a new inbound connection with explicit data flow.
    ///
    /// Same as the private `handle_new_connection` but takes the negotiated
    /// `DataFlow` explicitly. Only duplex connections are tracked for
    /// maturation.
    pub fn new_connection_with_data_flow(
        &mut self,
        conn_id: ConnectionId,
        data_flow: DataFlow,
        now_ms: u64,
    ) -> Vec<InboundGovernorAction> {
        let peer = conn_id.remote;

        if self.connections.contains_key(&peer) {
            return Vec::new();
        }

        let entry = InboundConnectionEntry {
            conn_id,
            remote_st: RemoteSt::RemoteIdleSt,
            data_flow,
            responder_counters: ResponderCounters::default(),
            idle_since_ms: Some(now_ms),
            connected_at_ms: now_ms,
        };

        self.connections.insert(peer, entry);

        if data_flow == DataFlow::Duplex {
            self.fresh_duplex_peers.insert(peer, now_ms);
        }

        self.recompute_counters();
        Vec::new()
    }

    /// Handle mux finished — unregister the connection.
    ///
    /// Upstream: `unregisterConnection` removes from `connections`,
    /// `matureDuplexPeers`, and `freshDuplexPeers`.
    fn handle_mux_finished(&mut self, conn_id: ConnectionId) -> Vec<InboundGovernorAction> {
        let peer = conn_id.remote;
        self.connections.remove(&peer);
        self.fresh_duplex_peers.remove(&peer);
        self.mature_duplex_peers.remove(&peer);
        vec![InboundGovernorAction::UnregisterConnection(conn_id)]
    }

    /// Handle a mini-protocol instance terminating on a connection.
    ///
    /// Upstream restarts the responder if it terminated cleanly,
    /// or stops the mux on error. In our pure model we signal that
    /// the caller should manage this. No actions emitted here; the
    /// runtime decides whether to restart or shut down.
    fn handle_mini_protocol_terminated(
        &mut self,
        _conn_id: ConnectionId,
    ) -> Vec<InboundGovernorAction> {
        // No state change or actions from the IG itself.
        // The runtime (mux layer) handles responder restart/teardown.
        Vec::new()
    }

    /// Handle all responders going idle (remote side quiescent).
    ///
    /// Transition: `RemoteWarmSt` | `RemoteHotSt` → `RemoteIdleSt`.
    /// Emit `DemotedToColdRemote` to notify the CM.
    /// Start the idle timeout.
    fn handle_wait_idle_remote(&mut self, conn_id: ConnectionId) -> Vec<InboundGovernorAction> {
        let peer = conn_id.remote;
        if let Some(entry) = self.connections.get_mut(&peer) {
            // Only transition from established (warm or hot) to idle.
            if entry.remote_st.is_established() {
                entry.remote_st = RemoteSt::RemoteIdleSt;
                entry.idle_since_ms = Some(0); // caller should set real time
                entry.responder_counters = ResponderCounters::default();
                return vec![InboundGovernorAction::DemotedToColdRemote(conn_id)];
            }
        }
        Vec::new()
    }

    /// Handle remote side becoming active again.
    ///
    /// Transition: `RemoteIdleSt` | `RemoteColdSt` → `RemoteWarmSt`.
    /// Emit `PromotedToWarmRemote` to notify the CM.
    fn handle_awake_remote(&mut self, conn_id: ConnectionId) -> Vec<InboundGovernorAction> {
        let peer = conn_id.remote;
        if let Some(entry) = self.connections.get_mut(&peer) {
            match entry.remote_st {
                RemoteSt::RemoteIdleSt | RemoteSt::RemoteColdSt => {
                    entry.remote_st = RemoteSt::RemoteWarmSt;
                    entry.idle_since_ms = None;
                    return vec![InboundGovernorAction::PromotedToWarmRemote(conn_id)];
                }
                _ => {}
            }
        }
        Vec::new()
    }

    /// Handle remote side promoted from warm to hot.
    ///
    /// Transition: `RemoteWarmSt` → `RemoteHotSt`.
    /// This is an IG-internal transition, no CM action needed.
    fn handle_remote_promoted_to_hot(
        &mut self,
        conn_id: ConnectionId,
    ) -> Vec<InboundGovernorAction> {
        let peer = conn_id.remote;
        if let Some(entry) = self.connections.get_mut(&peer) {
            if entry.remote_st == RemoteSt::RemoteWarmSt {
                entry.remote_st = RemoteSt::RemoteHotSt;
            }
        }
        Vec::new()
    }

    /// Handle remote side demoted from hot to warm.
    ///
    /// Transition: `RemoteHotSt` → `RemoteWarmSt`.
    /// This is an IG-internal transition, no CM action needed.
    fn handle_remote_demoted_to_warm(
        &mut self,
        conn_id: ConnectionId,
    ) -> Vec<InboundGovernorAction> {
        let peer = conn_id.remote;
        if let Some(entry) = self.connections.get_mut(&peer) {
            if entry.remote_st == RemoteSt::RemoteHotSt {
                entry.remote_st = RemoteSt::RemoteWarmSt;
            }
        }
        Vec::new()
    }

    /// Handle commit-remote: the idle timeout for a connection has expired.
    ///
    /// Ask the CM to release the inbound connection. The CM will return
    /// either `CommitTr` (connection fully closed) or `KeepTr` (outbound
    /// side still active).
    ///
    /// This only fires for connections in `RemoteIdleSt`.
    fn handle_commit_remote(&mut self, conn_id: ConnectionId) -> Vec<InboundGovernorAction> {
        let peer = conn_id.remote;
        if let Some(entry) = self.connections.get(&peer) {
            if entry.remote_st == RemoteSt::RemoteIdleSt {
                return vec![InboundGovernorAction::ReleaseInboundConnection(conn_id)];
            }
        }
        Vec::new()
    }

    /// Apply the result of a `ReleaseInboundConnection` action.
    ///
    /// The caller executes the CM call and feeds the result back here.
    /// - `CommitTr`: unregister the connection.
    /// - `KeepTr`: transition to `RemoteColdSt` (outbound still using it).
    pub fn apply_commit_result(
        &mut self,
        conn_id: ConnectionId,
        result: DemotedToColdRemoteTr,
    ) -> Vec<InboundGovernorAction> {
        let peer = conn_id.remote;
        match result {
            DemotedToColdRemoteTr::CommitTr => {
                self.connections.remove(&peer);
                self.fresh_duplex_peers.remove(&peer);
                self.mature_duplex_peers.remove(&peer);
                self.recompute_counters();
                vec![InboundGovernorAction::UnregisterConnection(conn_id)]
            }
            DemotedToColdRemoteTr::KeepTr => {
                if let Some(entry) = self.connections.get_mut(&peer) {
                    entry.remote_st = RemoteSt::RemoteColdSt;
                    entry.idle_since_ms = None;
                }
                self.recompute_counters();
                Vec::new()
            }
        }
    }

    /// Handle the matured-duplex-peers event.
    ///
    /// Matures any fresh duplex peers that have exceeded the threshold.
    fn handle_matured_duplex_peers(&mut self, now_ms: u64) -> Vec<InboundGovernorAction> {
        self.mature_peers(now_ms);
        Vec::new()
    }

    /// Handle the periodic inactivity timeout.
    ///
    /// This wakes the governor loop to check for matured peers and
    /// any idle connections whose timeout has expired.
    ///
    /// Returns `CommitRemote`-equivalent actions for connections that
    /// have been idle longer than `protocol_idle_timeout_ms`.
    fn handle_inactivity_timeout(&mut self, now_ms: u64) -> Vec<InboundGovernorAction> {
        // Mature any fresh duplex peers.
        self.mature_peers(now_ms);

        // Check for idle connections whose timeout has expired.
        let idle_timeout = self.protocol_idle_timeout_ms;
        let mut commits = Vec::new();

        for entry in self.connections.values() {
            if entry.remote_st == RemoteSt::RemoteIdleSt {
                if let Some(idle_since) = entry.idle_since_ms {
                    if now_ms.saturating_sub(idle_since) >= idle_timeout {
                        commits.push(InboundGovernorAction::ReleaseInboundConnection(
                            entry.conn_id,
                        ));
                    }
                }
            }
        }

        commits
    }

    // -----------------------------------------------------------------------
    // Idle timeout scan (for external use)
    // -----------------------------------------------------------------------

    /// Scan for connections whose idle timeout has expired and return
    /// `CommitRemote` events for them.
    ///
    /// The caller should feed these events back through [`Self::step`] or
    /// process them directly. This is used when the runtime wants to
    /// poll for expired idle timeouts without waiting for the full
    /// inactivity timeout cycle.
    pub fn expired_idle_connections(&self, now_ms: u64) -> Vec<InboundGovernorEvent> {
        let idle_timeout = self.protocol_idle_timeout_ms;
        let mut events = Vec::new();

        for entry in self.connections.values() {
            if entry.remote_st == RemoteSt::RemoteIdleSt {
                if let Some(idle_since) = entry.idle_since_ms {
                    if now_ms.saturating_sub(idle_since) >= idle_timeout {
                        events.push(InboundGovernorEvent::CommitRemote(entry.conn_id));
                    }
                }
            }
        }

        events
    }

    // -----------------------------------------------------------------------
    // Responder counter tracking
    // -----------------------------------------------------------------------

    /// Update responder counters for a connection and derive the
    /// appropriate IG events.
    ///
    /// This models the upstream `inboundGovernorMuxTracer` logic: when
    /// counters change, appropriate `AwakeRemote`, `RemotePromotedToHot`,
    /// `RemoteDemotedToWarm`, and `WaitIdleRemote` events are generated.
    ///
    /// Returns events to be fed into [`Self::step`].
    pub fn update_responder_counters(
        &self,
        peer: &SocketAddr,
        new_counters: ResponderCounters,
    ) -> Vec<InboundGovernorEvent> {
        let Some(entry) = self.connections.get(peer) else {
            return Vec::new();
        };

        let old = &entry.responder_counters;
        let mut events = Vec::new();

        let old_total = old.hot_responders + old.non_hot_responders;
        let new_total = new_counters.hot_responders + new_counters.non_hot_responders;

        // First activity (both counters were 0) → AwakeRemote
        if old_total == 0 && new_total > 0 {
            events.push(InboundGovernorEvent::AwakeRemote(entry.conn_id));
        }

        // First hot responder → RemotePromotedToHot
        if old.hot_responders == 0 && new_counters.hot_responders > 0 && old_total > 0 {
            // Only when already awake (old_total > 0 means we didn't just fire AwakeRemote)
            events.push(InboundGovernorEvent::RemotePromotedToHot(entry.conn_id));
        } else if old_total == 0 && new_counters.hot_responders > 0 {
            // AwakeRemote already fired; if first activity is a hot responder,
            // also fire promoted-to-hot after the awake.
            events.push(InboundGovernorEvent::RemotePromotedToHot(entry.conn_id));
        }

        // Last hot responder leaves → RemoteDemotedToWarm (if still active)
        if old.hot_responders > 0 && new_counters.hot_responders == 0 && new_total > 0 {
            events.push(InboundGovernorEvent::RemoteDemotedToWarm(entry.conn_id));
        }

        // All responders gone → WaitIdleRemote
        if old_total > 0 && new_total == 0 {
            events.push(InboundGovernorEvent::WaitIdleRemote(entry.conn_id));
        }

        events
    }

    /// Actually apply new responder counters to the stored entry.
    ///
    /// Call this after processing the events from [`Self::update_responder_counters`].
    pub fn set_responder_counters(&mut self, peer: &SocketAddr, new_counters: ResponderCounters) {
        if let Some(entry) = self.connections.get_mut(peer) {
            entry.responder_counters = new_counters;
        }
    }
}

impl Default for InboundGovernorState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Verified remote state transitions
// ---------------------------------------------------------------------------

/// Validate that a remote state transition is allowed by the IG state
/// machine.
///
/// Upstream transition table from test utils:
/// ```text
/// None         → RemoteIdleSt   (NewConnection)
/// RemoteIdleSt → RemoteWarmSt   (AwakeRemote)
/// RemoteColdSt → RemoteWarmSt   (AwakeRemote)
/// RemoteWarmSt → RemoteHotSt    (RemotePromotedToHot)
/// RemoteHotSt  → RemoteWarmSt   (RemoteDemotedToWarm)
/// RemoteWarmSt → RemoteIdleSt   (WaitIdleRemote)
/// RemoteHotSt  → RemoteIdleSt   (WaitIdleRemote)
/// RemoteIdleSt → RemoteColdSt   (CommitRemote + KeepTr)
/// RemoteIdleSt → None           (CommitRemote + CommitTr)
/// RemoteColdSt → None           (MuxFinished)
/// RemoteIdleSt → None           (MuxFinished)
/// RemoteWarmSt → None           (MuxFinished/error)
/// RemoteHotSt  → None           (MuxFinished/error)
/// ```
pub fn verify_remote_transition(from: Option<RemoteSt>, to: Option<RemoteSt>) -> bool {
    use RemoteSt::*;
    matches!(
        (from, to),
        // NewConnection
        (None, Some(RemoteIdleSt))
        // AwakeRemote
        | (Some(RemoteIdleSt), Some(RemoteWarmSt))
        | (Some(RemoteColdSt), Some(RemoteWarmSt))
        // RemotePromotedToHot
        | (Some(RemoteWarmSt), Some(RemoteHotSt))
        // RemoteDemotedToWarm
        | (Some(RemoteHotSt), Some(RemoteWarmSt))
        // WaitIdleRemote (from established)
        | (Some(RemoteWarmSt), Some(RemoteIdleSt))
        | (Some(RemoteHotSt), Some(RemoteIdleSt))
        // CommitRemote + KeepTr
        | (Some(RemoteIdleSt), Some(RemoteColdSt))
        // CommitRemote + CommitTr / MuxFinished from any state
        | (Some(RemoteIdleSt), None)
        | (Some(RemoteColdSt), None)
        | (Some(RemoteWarmSt), None)
        | (Some(RemoteHotSt), None)
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn conn_id(local_port: u16, remote_port: u16) -> ConnectionId {
        ConnectionId {
            local: addr(local_port),
            remote: addr(remote_port),
        }
    }

    // -- Constructor --

    #[test]
    fn new_state_is_empty() {
        let st = InboundGovernorState::new();
        assert_eq!(st.connection_count(), 0);
        assert_eq!(st.counters, InboundGovernorCounters::default());
        assert!(st.mature_duplex_peers.is_empty());
        assert!(st.fresh_duplex_peers.is_empty());
    }

    #[test]
    fn default_idle_timeout() {
        let st = InboundGovernorState::new();
        assert_eq!(st.protocol_idle_timeout_ms, 5_000);
    }

    #[test]
    fn custom_idle_timeout() {
        let st = InboundGovernorState::with_idle_timeout(Duration::from_secs(10));
        assert_eq!(st.protocol_idle_timeout_ms, 10_000);
    }

    // -- NewConnection --

    #[test]
    fn new_connection_registers_peer() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        let actions = st.step(InboundGovernorEvent::NewConnection(cid), 100);
        assert!(actions.is_empty());
        assert_eq!(st.connection_count(), 1);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteIdleSt));
        assert_eq!(st.counters.idle_peers_remote, 1);
    }

    #[test]
    fn new_connection_duplex_added_to_fresh_peers() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 500);
        assert!(st.fresh_duplex_peers.contains_key(&cid.remote));
        assert_eq!(*st.fresh_duplex_peers.get(&cid.remote).unwrap(), 500);
    }

    #[test]
    fn new_connection_preserves_existing() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        // Awake the remote.
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));

        // Duplicate NewConnection should preserve warm state.
        st.step(InboundGovernorEvent::NewConnection(cid), 300);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));
        assert_eq!(st.connection_count(), 1);
    }

    #[test]
    fn new_connection_with_explicit_unidirectional() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.new_connection_with_data_flow(cid, DataFlow::Unidirectional, 100);
        assert_eq!(st.connection_count(), 1);
        // Unidirectional should not go into fresh duplex peers.
        assert!(!st.fresh_duplex_peers.contains_key(&cid.remote));
    }

    // -- AwakeRemote --

    #[test]
    fn awake_remote_from_idle() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        let actions = st.step(InboundGovernorEvent::AwakeRemote(cid), 200);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], InboundGovernorAction::PromotedToWarmRemote(cid));
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));
        assert_eq!(st.counters.warm_peers_remote, 1);
        assert_eq!(st.counters.idle_peers_remote, 0);
    }

    #[test]
    fn awake_remote_from_cold() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        // Set up → idle → awake → idle → commit (KeepTr) → cold.
        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        st.step(InboundGovernorEvent::WaitIdleRemote(cid), 300);
        st.step(InboundGovernorEvent::CommitRemote(cid), 400);
        st.apply_commit_result(cid, DemotedToColdRemoteTr::KeepTr);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteColdSt));

        // Now awake from cold.
        let actions = st.step(InboundGovernorEvent::AwakeRemote(cid), 500);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], InboundGovernorAction::PromotedToWarmRemote(cid));
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));
    }

    #[test]
    fn awake_remote_noop_when_already_warm() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);

        // Second awake is a no-op.
        let actions = st.step(InboundGovernorEvent::AwakeRemote(cid), 300);
        assert!(actions.is_empty());
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));
    }

    // -- RemotePromotedToHot / RemoteDemotedToWarm --

    #[test]
    fn promote_warm_to_hot() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);

        let actions = st.step(InboundGovernorEvent::RemotePromotedToHot(cid), 300);
        assert!(actions.is_empty()); // IG-internal, no CM action
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteHotSt));
        assert_eq!(st.counters.hot_peers_remote, 1);
    }

    #[test]
    fn demote_hot_to_warm() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        st.step(InboundGovernorEvent::RemotePromotedToHot(cid), 300);

        let actions = st.step(InboundGovernorEvent::RemoteDemotedToWarm(cid), 400);
        assert!(actions.is_empty()); // IG-internal
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));
    }

    #[test]
    fn promote_noop_when_not_warm() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        // Idle → try promote to hot (should be no-op)
        let actions = st.step(InboundGovernorEvent::RemotePromotedToHot(cid), 200);
        assert!(actions.is_empty());
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteIdleSt));
    }

    // -- WaitIdleRemote --

    #[test]
    fn wait_idle_from_warm() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);

        let actions = st.step(InboundGovernorEvent::WaitIdleRemote(cid), 300);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], InboundGovernorAction::DemotedToColdRemote(cid));
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteIdleSt));
    }

    #[test]
    fn wait_idle_from_hot() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        st.step(InboundGovernorEvent::RemotePromotedToHot(cid), 300);

        let actions = st.step(InboundGovernorEvent::WaitIdleRemote(cid), 400);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], InboundGovernorAction::DemotedToColdRemote(cid));
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteIdleSt));
    }

    #[test]
    fn wait_idle_noop_when_already_idle() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        // Already idle from NewConnection.
        let actions = st.step(InboundGovernorEvent::WaitIdleRemote(cid), 200);
        assert!(actions.is_empty());
    }

    // -- CommitRemote --

    #[test]
    fn commit_remote_from_idle() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);

        let actions = st.step(InboundGovernorEvent::CommitRemote(cid), 200);
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            InboundGovernorAction::ReleaseInboundConnection(cid)
        );
    }

    #[test]
    fn commit_remote_noop_from_warm() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);

        // CommitRemote should be ignored when peer is warm.
        let actions = st.step(InboundGovernorEvent::CommitRemote(cid), 300);
        assert!(actions.is_empty());
    }

    // -- apply_commit_result --

    #[test]
    fn commit_result_commit_tr_unregisters() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::CommitRemote(cid), 200);

        let actions = st.apply_commit_result(cid, DemotedToColdRemoteTr::CommitTr);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], InboundGovernorAction::UnregisterConnection(cid));
        assert_eq!(st.connection_count(), 0);
        assert!(!st.fresh_duplex_peers.contains_key(&cid.remote));
    }

    #[test]
    fn commit_result_keep_tr_moves_to_cold() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::CommitRemote(cid), 200);

        let actions = st.apply_commit_result(cid, DemotedToColdRemoteTr::KeepTr);
        assert!(actions.is_empty());
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteColdSt));
        assert_eq!(st.counters.cold_peers_remote, 1);
    }

    // -- MuxFinished --

    #[test]
    fn mux_finished_unregisters() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);

        let actions = st.step(InboundGovernorEvent::MuxFinished(cid), 300);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], InboundGovernorAction::UnregisterConnection(cid));
        assert_eq!(st.connection_count(), 0);
        assert!(!st.fresh_duplex_peers.contains_key(&cid.remote));
    }

    // -- Duplex peer maturation --

    #[test]
    fn fresh_duplex_peers_not_mature_before_threshold() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 1000);
        // 14 minutes = 840_000 ms — not yet mature.
        let matured = st.mature_peers(841_000);
        assert_eq!(matured, 0);
        assert!(st.fresh_duplex_peers.contains_key(&cid.remote));
        assert!(st.mature_duplex_peers.is_empty());
    }

    #[test]
    fn duplex_peer_matures_after_threshold() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 1000);
        // 15 minutes = 900_000 ms — exactly at threshold.
        let matured = st.mature_peers(901_000);
        assert_eq!(matured, 1);
        assert!(!st.fresh_duplex_peers.contains_key(&cid.remote));
        assert!(st.mature_duplex_peers.contains_key(&cid.remote));
    }

    #[test]
    fn multiple_peers_mature_at_different_times() {
        let mut st = InboundGovernorState::new();

        let cid1 = conn_id(1000, 2001);
        let cid2 = conn_id(1000, 2002);
        let cid3 = conn_id(1000, 2003);

        st.step(InboundGovernorEvent::NewConnection(cid1), 1000);
        st.step(InboundGovernorEvent::NewConnection(cid2), 2000);
        st.step(InboundGovernorEvent::NewConnection(cid3), 3000);

        // After 901_000 ms, only cid1 should be mature.
        let matured = st.mature_peers(901_000);
        assert_eq!(matured, 1);
        assert!(st.mature_duplex_peers.contains_key(&cid1.remote));
        assert!(!st.mature_duplex_peers.contains_key(&cid2.remote));

        // After 902_000 ms, cid2 should also mature.
        let matured = st.mature_peers(902_000);
        assert_eq!(matured, 1);
        assert!(st.mature_duplex_peers.contains_key(&cid2.remote));

        // After 903_000 ms, cid3 should mature.
        let matured = st.mature_peers(903_000);
        assert_eq!(matured, 1);
        assert!(st.mature_duplex_peers.contains_key(&cid3.remote));
        assert!(st.fresh_duplex_peers.is_empty());
    }

    #[test]
    fn mux_finished_removes_from_mature_peers() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 1000);
        st.mature_peers(901_000);
        assert!(st.mature_duplex_peers.contains_key(&cid.remote));

        st.step(InboundGovernorEvent::MuxFinished(cid), 902_000);
        assert!(!st.mature_duplex_peers.contains_key(&cid.remote));
    }

    // -- MaturedDuplexPeers event --

    #[test]
    fn matured_duplex_peers_event_triggers_maturation() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 1000);

        let actions = st.step(InboundGovernorEvent::MaturedDuplexPeers, 901_000);
        assert!(actions.is_empty());
        assert!(st.mature_duplex_peers.contains_key(&cid.remote));
    }

    // -- InactivityTimeout --

    #[test]
    fn inactivity_timeout_matures_peers() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 1000);
        st.step(InboundGovernorEvent::InactivityTimeout, 901_000);
        assert!(st.mature_duplex_peers.contains_key(&cid.remote));
    }

    #[test]
    fn inactivity_timeout_detects_expired_idle() {
        let mut st = InboundGovernorState::with_idle_timeout(Duration::from_secs(5));
        let cid = conn_id(1000, 2000);

        // Connection enters idle at t=100.
        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        // Idle since t=100. At t=5200 (>5000ms later), timeout fires.
        let actions = st.step(InboundGovernorEvent::InactivityTimeout, 5200);

        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            InboundGovernorAction::ReleaseInboundConnection(cid)
        );
    }

    #[test]
    fn inactivity_timeout_does_not_fire_for_warm_peers() {
        let mut st = InboundGovernorState::with_idle_timeout(Duration::from_secs(5));
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);

        // Warm peer should not trigger idle timeout.
        let actions = st.step(InboundGovernorEvent::InactivityTimeout, 999_000);
        assert!(actions.is_empty());
    }

    // -- expired_idle_connections --

    #[test]
    fn expired_idle_connections_scan() {
        let mut st = InboundGovernorState::with_idle_timeout(Duration::from_secs(5));

        let cid1 = conn_id(1000, 2001);
        let cid2 = conn_id(1000, 2002);

        st.step(InboundGovernorEvent::NewConnection(cid1), 100);
        st.step(InboundGovernorEvent::NewConnection(cid2), 3000);

        // At t=5200, only cid1's idle has expired (100+5000=5100 < 5200).
        let events = st.expired_idle_connections(5200);
        assert_eq!(events.len(), 1);
        let InboundGovernorEvent::CommitRemote(c) = &events[0] else {
            panic!("expected CommitRemote");
        };
        assert_eq!(c.remote, cid1.remote);
    }

    // -- Responder counter events --

    #[test]
    fn responder_counter_first_activity_awakes() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);
        st.step(InboundGovernorEvent::NewConnection(cid), 100);

        let events = st.update_responder_counters(
            &cid.remote,
            ResponderCounters {
                hot_responders: 0,
                non_hot_responders: 1,
            },
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0], InboundGovernorEvent::AwakeRemote(cid));
    }

    #[test]
    fn responder_counter_hot_start_promotes() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);
        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        st.set_responder_counters(
            &cid.remote,
            ResponderCounters {
                hot_responders: 0,
                non_hot_responders: 1,
            },
        );

        // A hot responder starts while already warm.
        let events = st.update_responder_counters(
            &cid.remote,
            ResponderCounters {
                hot_responders: 1,
                non_hot_responders: 1,
            },
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0], InboundGovernorEvent::RemotePromotedToHot(cid));
    }

    #[test]
    fn responder_counter_last_hot_demotes() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);
        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        st.set_responder_counters(
            &cid.remote,
            ResponderCounters {
                hot_responders: 1,
                non_hot_responders: 1,
            },
        );

        let events = st.update_responder_counters(
            &cid.remote,
            ResponderCounters {
                hot_responders: 0,
                non_hot_responders: 1,
            },
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0], InboundGovernorEvent::RemoteDemotedToWarm(cid));
    }

    #[test]
    fn responder_counter_all_stop_waits_idle() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);
        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        st.set_responder_counters(
            &cid.remote,
            ResponderCounters {
                hot_responders: 0,
                non_hot_responders: 1,
            },
        );

        let events = st.update_responder_counters(
            &cid.remote,
            ResponderCounters {
                hot_responders: 0,
                non_hot_responders: 0,
            },
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0], InboundGovernorEvent::WaitIdleRemote(cid));
    }

    // -- verify_remote_transition --

    #[test]
    fn valid_remote_transitions() {
        use RemoteSt::*;
        assert!(verify_remote_transition(None, Some(RemoteIdleSt)));
        assert!(verify_remote_transition(
            Some(RemoteIdleSt),
            Some(RemoteWarmSt)
        ));
        assert!(verify_remote_transition(
            Some(RemoteColdSt),
            Some(RemoteWarmSt)
        ));
        assert!(verify_remote_transition(
            Some(RemoteWarmSt),
            Some(RemoteHotSt)
        ));
        assert!(verify_remote_transition(
            Some(RemoteHotSt),
            Some(RemoteWarmSt)
        ));
        assert!(verify_remote_transition(
            Some(RemoteWarmSt),
            Some(RemoteIdleSt)
        ));
        assert!(verify_remote_transition(
            Some(RemoteHotSt),
            Some(RemoteIdleSt)
        ));
        assert!(verify_remote_transition(
            Some(RemoteIdleSt),
            Some(RemoteColdSt)
        ));
        assert!(verify_remote_transition(Some(RemoteIdleSt), None));
        assert!(verify_remote_transition(Some(RemoteColdSt), None));
        assert!(verify_remote_transition(Some(RemoteWarmSt), None));
        assert!(verify_remote_transition(Some(RemoteHotSt), None));
    }

    #[test]
    fn invalid_remote_transitions() {
        use RemoteSt::*;
        // Can't go idle → hot directly (must go through warm).
        assert!(!verify_remote_transition(
            Some(RemoteIdleSt),
            Some(RemoteHotSt)
        ));
        // Can't go cold → hot directly.
        assert!(!verify_remote_transition(
            Some(RemoteColdSt),
            Some(RemoteHotSt)
        ));
        // Can't go cold → idle (must awake to warm first).
        assert!(!verify_remote_transition(
            Some(RemoteColdSt),
            Some(RemoteIdleSt)
        ));
        // Can't go warm → cold directly.
        assert!(!verify_remote_transition(
            Some(RemoteWarmSt),
            Some(RemoteColdSt)
        ));
        // Can't go hot → cold directly.
        assert!(!verify_remote_transition(
            Some(RemoteHotSt),
            Some(RemoteColdSt)
        ));
        // Can't re-enter from none to anything except idle.
        assert!(!verify_remote_transition(None, Some(RemoteWarmSt)));
        assert!(!verify_remote_transition(None, Some(RemoteHotSt)));
        assert!(!verify_remote_transition(None, Some(RemoteColdSt)));
        assert!(!verify_remote_transition(None, None));
    }

    // -- Full lifecycle --

    #[test]
    fn full_connection_lifecycle_commit_tr() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        // 1. NewConnection → RemoteIdleSt
        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteIdleSt));

        // 2. AwakeRemote → RemoteWarmSt (CM: promotedToWarmRemote)
        let a = st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        assert_eq!(a, vec![InboundGovernorAction::PromotedToWarmRemote(cid)]);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));

        // 3. RemotePromotedToHot → RemoteHotSt
        st.step(InboundGovernorEvent::RemotePromotedToHot(cid), 300);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteHotSt));

        // 4. RemoteDemotedToWarm → RemoteWarmSt
        st.step(InboundGovernorEvent::RemoteDemotedToWarm(cid), 400);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));

        // 5. WaitIdleRemote → RemoteIdleSt (CM: demotedToColdRemote)
        let a = st.step(InboundGovernorEvent::WaitIdleRemote(cid), 500);
        assert_eq!(a, vec![InboundGovernorAction::DemotedToColdRemote(cid)]);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteIdleSt));

        // 6. CommitRemote (CM: releaseInboundConnection)
        let a = st.step(InboundGovernorEvent::CommitRemote(cid), 600);
        assert_eq!(
            a,
            vec![InboundGovernorAction::ReleaseInboundConnection(cid)]
        );

        // 7. CM returns CommitTr → unregister
        let a = st.apply_commit_result(cid, DemotedToColdRemoteTr::CommitTr);
        assert_eq!(a, vec![InboundGovernorAction::UnregisterConnection(cid)]);
        assert_eq!(st.connection_count(), 0);
    }

    #[test]
    fn full_lifecycle_keep_tr_then_reawake() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        st.step(InboundGovernorEvent::WaitIdleRemote(cid), 300);
        st.step(InboundGovernorEvent::CommitRemote(cid), 400);

        // CM says KeepTr (outbound side still using it).
        let a = st.apply_commit_result(cid, DemotedToColdRemoteTr::KeepTr);
        assert!(a.is_empty());
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteColdSt));

        // Peer wakes up again from cold.
        let a = st.step(InboundGovernorEvent::AwakeRemote(cid), 500);
        assert_eq!(a, vec![InboundGovernorAction::PromotedToWarmRemote(cid)]);
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));
    }

    #[test]
    fn mux_finished_during_active_use() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);
        st.step(InboundGovernorEvent::RemotePromotedToHot(cid), 300);

        // Mux crashes while peer is hot.
        let a = st.step(InboundGovernorEvent::MuxFinished(cid), 400);
        assert_eq!(a, vec![InboundGovernorAction::UnregisterConnection(cid)]);
        assert_eq!(st.connection_count(), 0);
    }

    // -- Multi-peer scenarios --

    #[test]
    fn multiple_peers_tracked_independently() {
        let mut st = InboundGovernorState::new();

        let cid1 = conn_id(1000, 2001);
        let cid2 = conn_id(1000, 2002);
        let cid3 = conn_id(1000, 2003);

        st.step(InboundGovernorEvent::NewConnection(cid1), 100);
        st.step(InboundGovernorEvent::NewConnection(cid2), 200);
        st.step(InboundGovernorEvent::NewConnection(cid3), 300);
        assert_eq!(st.connection_count(), 3);
        assert_eq!(st.counters.idle_peers_remote, 3);

        // Wake up only cid1 and cid2.
        st.step(InboundGovernorEvent::AwakeRemote(cid1), 400);
        st.step(InboundGovernorEvent::AwakeRemote(cid2), 500);
        assert_eq!(st.counters.warm_peers_remote, 2);
        assert_eq!(st.counters.idle_peers_remote, 1);

        // Promote only cid1 to hot.
        st.step(InboundGovernorEvent::RemotePromotedToHot(cid1), 600);
        assert_eq!(st.counters.hot_peers_remote, 1);
        assert_eq!(st.counters.warm_peers_remote, 1);

        // MuxFinished on cid3 (idle).
        st.step(InboundGovernorEvent::MuxFinished(cid3), 700);
        assert_eq!(st.connection_count(), 2);
        assert_eq!(st.counters.idle_peers_remote, 0);

        // Counters should reflect current state.
        assert_eq!(st.counters.hot_peers_remote, 1);
        assert_eq!(st.counters.warm_peers_remote, 1);
    }

    #[test]
    fn unknown_peer_events_are_no_ops() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 9999); // never registered

        assert!(
            st.step(InboundGovernorEvent::AwakeRemote(cid), 100)
                .is_empty()
        );
        assert!(
            st.step(InboundGovernorEvent::WaitIdleRemote(cid), 200)
                .is_empty()
        );
        assert!(
            st.step(InboundGovernorEvent::RemotePromotedToHot(cid), 300)
                .is_empty()
        );
        assert!(
            st.step(InboundGovernorEvent::CommitRemote(cid), 400)
                .is_empty()
        );
    }

    #[test]
    fn mini_protocol_terminated_is_transparent() {
        let mut st = InboundGovernorState::new();
        let cid = conn_id(1000, 2000);

        st.step(InboundGovernorEvent::NewConnection(cid), 100);
        st.step(InboundGovernorEvent::AwakeRemote(cid), 200);

        let actions = st.step(InboundGovernorEvent::MiniProtocolTerminated(cid), 300);
        assert!(actions.is_empty());
        // State unchanged.
        assert_eq!(st.remote_state(&cid.remote), Some(RemoteSt::RemoteWarmSt));
    }

    #[test]
    fn counters_accurate_after_complex_sequence() {
        let mut st = InboundGovernorState::new();

        let peers: Vec<_> = (2001..=2005).map(|p| conn_id(1000, p)).collect();

        // Register 5 peers.
        for (i, &cid) in peers.iter().enumerate() {
            st.step(InboundGovernorEvent::NewConnection(cid), (i as u64) * 100);
        }
        assert_eq!(st.counters.idle_peers_remote, 5);

        // Wake 3, promote 1 to hot.
        st.step(InboundGovernorEvent::AwakeRemote(peers[0]), 600);
        st.step(InboundGovernorEvent::AwakeRemote(peers[1]), 700);
        st.step(InboundGovernorEvent::AwakeRemote(peers[2]), 800);
        st.step(InboundGovernorEvent::RemotePromotedToHot(peers[0]), 900);

        assert_eq!(st.counters.hot_peers_remote, 1);
        assert_eq!(st.counters.warm_peers_remote, 2);
        assert_eq!(st.counters.idle_peers_remote, 2);
        assert_eq!(st.counters.cold_peers_remote, 0);
        assert_eq!(st.counters.total(), 5);

        // Mux finish on hot peer.
        st.step(InboundGovernorEvent::MuxFinished(peers[0]), 1000);
        assert_eq!(st.counters.hot_peers_remote, 0);
        assert_eq!(st.counters.warm_peers_remote, 2);
        assert_eq!(st.counters.idle_peers_remote, 2);
        assert_eq!(st.counters.total(), 4);
    }
}
