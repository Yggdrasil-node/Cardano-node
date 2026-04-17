//! Connection manager decision engine.
//!
//! This module implements the upstream
//! `Ouroboros.Network.ConnectionManager.Core` as a pure decision engine.
//! Each operation (`acquire_outbound`, `include_inbound`,
//! `release_outbound`, `release_inbound`, `promoted_to_warm_remote`,
//! `demoted_to_cold_remote`) inspects the current per-peer
//! [`ConnectionState`] map and returns a new state together with side-effect
//! descriptors that the runtime (in `node/`) must execute.
//!
//! Design mirrors the inbound governor (`inbound_governor.rs`) and outbound
//! governor (`governor.rs`): all decisions are pure and testable; effectful
//! connection management stays in `node/`.
//!
//! Reference: `ouroboros-network-framework/src/Ouroboros/Network/ConnectionManager/Core.hs`

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Instant;

use crate::connection::{
    AbstractState, AcceptedConnectionsLimit, ConnStateId, ConnectionId, ConnectionManagerError,
    ConnectionState, DataFlow, DemotedToColdRemoteTr, OperationResult, Provenance, TimeoutExpired,
    Transition,
    timeouts::{PROTOCOL_IDLE_TIMEOUT, TIME_WAIT_TIMEOUT},
};

// ---------------------------------------------------------------------------
// Connection entry
// ---------------------------------------------------------------------------

/// A single entry in the connection manager state map.
///
/// Upstream: `MutableConnState` (minus the STM variable — our states
/// are directly stored).
#[derive(Clone, Debug)]
pub struct ConnectionEntry {
    /// Unique entry identifier.
    pub conn_state_id: ConnStateId,
    /// Current connection state.
    pub state: ConnectionState,
    /// Deadline for the responder timeout while in `OutboundDupState(Ticking)`.
    pub responder_timeout_deadline: Option<Instant>,
    /// Deadline for transitioning `TerminatingState` -> `TerminatedState`.
    pub time_wait_deadline: Option<Instant>,
}

// ---------------------------------------------------------------------------
// Side-effect actions
// ---------------------------------------------------------------------------

/// An action the runtime must execute after a CM decision.
///
/// These are the effectful consequences of pure state-machine transitions.
/// The runtime turns them into actual TCP operations, handshakes, and
/// protocol starts/stops.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CmAction {
    /// Start an outbound TCP connect + handshake for the given peer.
    StartConnect(SocketAddr),
    /// Terminate a connection (close the socket, stop the mux).
    TerminateConnection(ConnectionId),
    /// Start the remote responder timeout for an outbound duplex connection.
    ///
    /// After `PROTOCOL_IDLE_TIMEOUT`, the timeout transitions from
    /// `Ticking` to `Expired`.
    StartResponderTimeout(ConnectionId),
    /// Prune connections to stay within the accepted-connections limit.
    ///
    /// The `Vec` contains the remote addresses of connections the CM
    /// has selected for pruning. The runtime must terminate them.
    PruneConnections(Vec<SocketAddr>),
}

// ---------------------------------------------------------------------------
// Outbound acquire result
// ---------------------------------------------------------------------------

/// Outcome of `acquire_outbound_connection`.
///
/// Upstream: `Connected peerAddr handle handleError` — we don't carry the
/// handle in the pure layer; the runtime wires handles after executing
/// `CmAction::StartConnect`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcquireOutboundResult {
    /// A fresh outbound connection has been reserved. The runtime must
    /// execute the connect and feed the handshake result back via
    /// `outbound_handshake_done`.
    Fresh,
    /// The outbound request reused an existing inbound duplex connection
    /// and transitioned it to the duplex or outbound-dup state.
    Reused(AbstractState),
    /// The peer is in a terminating/terminated state.
    Disconnected(AbstractState),
}

/// Outcome of `release_outbound_connection`.
///
/// Upstream: the `unregisterOutboundConnection` results from
/// `ConnectionManager.Core`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReleaseOutboundResult {
    /// The connection was demoted to idle/cold.
    DemotedToColdLocal(AbstractState),
    /// The release was a no-op (already idle, unknown, or terminating).
    Noop(AbstractState),
    /// The operation is not valid in the current state.
    Error(ConnectionManagerError),
}

// ---------------------------------------------------------------------------
// Connection manager state
// ---------------------------------------------------------------------------

/// Mutable state of the connection manager decision engine.
///
/// Upstream: the `ConnectionManagerState` (ConnMap + id counter).
/// One entry per peer (remote address).
#[derive(Clone, Debug)]
pub struct ConnectionManagerState {
    /// Per-peer connection state. Keyed by remote `SocketAddr`.
    pub connections: BTreeMap<SocketAddr, ConnectionEntry>,
    /// Monotonic counter for unique entry IDs.
    next_id: u64,
    /// Accepted-connection limits.
    pub limits: AcceptedConnectionsLimit,
}

impl ConnectionManagerState {
    /// Create a new empty connection manager state with default limits.
    pub fn new() -> Self {
        Self {
            connections: BTreeMap::new(),
            next_id: 0,
            limits: AcceptedConnectionsLimit::default(),
        }
    }

    /// Create a new state with custom connection limits.
    pub fn with_limits(limits: AcceptedConnectionsLimit) -> Self {
        Self {
            connections: BTreeMap::new(),
            next_id: 0,
            limits,
        }
    }

    fn next_conn_state_id(&mut self) -> ConnStateId {
        let id = ConnStateId(self.next_id);
        self.next_id += 1;
        id
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Number of tracked connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Count of connections that are considered inbound.
    ///
    /// Upstream: `countIncomingConnections` counts states where
    /// `isInboundConn` returns `True`.
    pub fn inbound_connection_count(&self) -> u32 {
        self.connections
            .values()
            .filter(|e| e.state.abstract_state().is_inbound_conn())
            .count() as u32
    }

    /// Get the abstract state for a peer, or `UnknownConnectionSt` if not
    /// tracked.
    pub fn abstract_state_of(&self, peer: &SocketAddr) -> AbstractState {
        self.connections
            .get(peer)
            .map(|e| e.state.abstract_state())
            .unwrap_or(AbstractState::UnknownConnectionSt)
    }

    /// Build a snapshot of all abstract states.
    pub fn abstract_state_map(&self) -> BTreeMap<SocketAddr, AbstractState> {
        self.connections
            .iter()
            .map(|(addr, entry)| (*addr, entry.state.abstract_state()))
            .collect()
    }

    /// Compute accurate connection-manager counters by folding
    /// `connection_state_to_counters` over all tracked connections.
    ///
    /// Upstream: `connectionManagerStateToCounters` from
    /// `Ouroboros.Network.ConnectionManager.Core`.
    pub fn counters(&self) -> crate::governor::ConnectionManagerCounters {
        use crate::connection::connection_state_to_counters;
        let mut acc = crate::governor::ConnectionManagerCounters::default();
        for entry in self.connections.values() {
            let c = connection_state_to_counters(&entry.state);
            acc = acc + c;
        }
        acc
    }

    // -----------------------------------------------------------------------
    // 1. Acquire outbound connection
    // -----------------------------------------------------------------------

    /// Reserve an outbound connection to `peer`.
    ///
    /// Upstream: `requestOutboundConnection` / `acquireOutboundConnection`.
    ///
    /// Returns the result and a list of side-effect actions for the runtime.
    ///
    /// On `Fresh`, the runtime must:
    /// 1. Execute `CmAction::StartConnect`.
    /// 2. On handshake completion, call `outbound_handshake_done`.
    pub fn acquire_outbound_connection(
        &mut self,
        local_addr: SocketAddr,
        peer: SocketAddr,
    ) -> Result<(AcquireOutboundResult, Vec<CmAction>), ConnectionManagerError> {
        let conn_id = ConnectionId {
            local: local_addr,
            remote: peer,
        };

        match self.connections.get(&peer).map(|e| &e.state) {
            // -- Unknown: fresh connection --
            None => {
                let id = self.next_conn_state_id();
                self.connections.insert(
                    peer,
                    ConnectionEntry {
                        conn_state_id: id,
                        state: ConnectionState::ReservedOutboundState,
                        responder_timeout_deadline: None,
                        time_wait_deadline: None,
                    },
                );
                Ok((
                    AcquireOutboundResult::Fresh,
                    vec![CmAction::StartConnect(peer)],
                ))
            }

            // -- Already reserved/connecting outbound --
            Some(ConnectionState::ReservedOutboundState) => {
                Err(ConnectionManagerError::ConnectionExists {
                    provenance: Provenance::Outbound,
                    peer,
                })
            }
            Some(ConnectionState::UnnegotiatedState {
                provenance: Provenance::Outbound,
                ..
            }) => Err(ConnectionManagerError::ConnectionExists {
                provenance: Provenance::Outbound,
                peer,
            }),

            // -- Outbound already active --
            Some(ConnectionState::OutboundUniState { .. })
            | Some(ConnectionState::OutboundDupState { .. }) => {
                Err(ConnectionManagerError::ConnectionExists {
                    provenance: Provenance::Outbound,
                    peer,
                })
            }

            // -- OutboundIdle: should use promotedToWarmRemote instead --
            Some(ConnectionState::OutboundIdleState { .. }) => {
                Err(ConnectionManagerError::ForbiddenOperation {
                    peer,
                    state: self.abstract_state_of(&peer),
                })
            }

            // -- Reuse idle inbound duplex --
            Some(ConnectionState::InboundIdleState {
                data_flow: DataFlow::Duplex,
                ..
            }) => {
                let new_state = ConnectionState::OutboundDupState {
                    conn_id,
                    timeout_expired: TimeoutExpired::Ticking,
                };
                let from = self.abstract_state_of(&peer);
                if let Some(entry) = self.connections.get_mut(&peer) {
                    entry.state = new_state;
                    entry.responder_timeout_deadline = Some(Instant::now() + PROTOCOL_IDLE_TIMEOUT);
                    entry.time_wait_deadline = None;
                }
                let to = self.abstract_state_of(&peer);
                let _ = Transition {
                    from_state: from,
                    to_state: to,
                };
                Ok((
                    AcquireOutboundResult::Reused(to),
                    vec![CmAction::StartResponderTimeout(conn_id)],
                ))
            }

            // -- Cannot reuse unidirectional inbound --
            Some(ConnectionState::InboundIdleState {
                data_flow: DataFlow::Unidirectional,
                ..
            }) => Err(ConnectionManagerError::ForbiddenConnection(conn_id)),

            // -- Promote active inbound duplex to full duplex --
            Some(ConnectionState::InboundState {
                data_flow: DataFlow::Duplex,
                ..
            }) => {
                let new_state = ConnectionState::DuplexState { conn_id };
                let from = self.abstract_state_of(&peer);
                if let Some(entry) = self.connections.get_mut(&peer) {
                    entry.state = new_state;
                    entry.responder_timeout_deadline = None;
                    entry.time_wait_deadline = None;
                }
                let to = self.abstract_state_of(&peer);
                let _ = Transition {
                    from_state: from,
                    to_state: to,
                };
                // May need pruning if inbound count exceeds limit.
                let prune_actions = self.maybe_prune();
                Ok((AcquireOutboundResult::Reused(to), prune_actions))
            }

            // -- Cannot reuse unidirectional inbound --
            Some(ConnectionState::InboundState {
                data_flow: DataFlow::Unidirectional,
                ..
            }) => Err(ConnectionManagerError::ForbiddenConnection(conn_id)),

            // -- Already duplex --
            Some(ConnectionState::DuplexState { .. }) => {
                Err(ConnectionManagerError::ConnectionExists {
                    provenance: Provenance::Outbound,
                    peer,
                })
            }

            // -- Unnegotiated inbound: can reuse by overwriting --
            Some(ConnectionState::UnnegotiatedState {
                provenance: Provenance::Inbound,
                ..
            }) => {
                // Self-connect/reuse race: outbound overwrites inbound
                // UnnegotiatedState.
                let new_state = ConnectionState::UnnegotiatedState {
                    provenance: Provenance::Outbound,
                    conn_id,
                };
                if let Some(entry) = self.connections.get_mut(&peer) {
                    entry.state = new_state;
                    entry.responder_timeout_deadline = None;
                    entry.time_wait_deadline = None;
                }
                Ok((
                    AcquireOutboundResult::Fresh,
                    vec![CmAction::StartConnect(peer)],
                ))
            }

            // -- Terminating --
            Some(ConnectionState::TerminatingState { .. }) => Ok((
                AcquireOutboundResult::Disconnected(AbstractState::TerminatingSt),
                Vec::new(),
            )),

            // -- Terminated --
            Some(ConnectionState::TerminatedState { .. }) => Ok((
                AcquireOutboundResult::Disconnected(AbstractState::TerminatedSt),
                Vec::new(),
            )),
        }
    }

    /// Feed the result of a completed outbound handshake.
    ///
    /// Called by the runtime after `CmAction::StartConnect` completes
    /// successfully. Transitions `ReservedOutboundState` →
    /// `UnnegotiatedState Outbound` → `OutboundUniState` or
    /// `OutboundDupState Ticking`.
    ///
    /// In practice the runtime may call this in two phases:
    /// 1. After TCP connect: `Reserved → Unnegotiated Outbound`.
    /// 2. After handshake: `Unnegotiated Outbound → OutboundUni/Dup`.
    ///
    /// For simplicity we provide an all-in-one that goes straight from
    /// `Reserved` to the negotiated state.
    pub fn outbound_handshake_done(
        &mut self,
        local_addr: SocketAddr,
        peer: SocketAddr,
        data_flow: DataFlow,
    ) -> Result<AbstractState, ConnectionManagerError> {
        let conn_id = ConnectionId {
            local: local_addr,
            remote: peer,
        };

        let entry = self
            .connections
            .get_mut(&peer)
            .ok_or(ConnectionManagerError::UnknownPeer(peer))?;

        match &entry.state {
            ConnectionState::ReservedOutboundState
            | ConnectionState::UnnegotiatedState {
                provenance: Provenance::Outbound,
                ..
            } => {
                let new_state = match data_flow {
                    DataFlow::Unidirectional => ConnectionState::OutboundUniState { conn_id },
                    DataFlow::Duplex => ConnectionState::OutboundDupState {
                        conn_id,
                        timeout_expired: TimeoutExpired::Ticking,
                    },
                };
                entry.state = new_state;
                entry.responder_timeout_deadline = if matches!(data_flow, DataFlow::Duplex) {
                    Some(Instant::now() + PROTOCOL_IDLE_TIMEOUT)
                } else {
                    None
                };
                entry.time_wait_deadline = None;
                Ok(entry.state.abstract_state())
            }
            other => Err(ConnectionManagerError::ForbiddenOperation {
                peer,
                state: other.abstract_state(),
            }),
        }
    }

    /// Handle a failed outbound connect attempt.
    ///
    /// If the peer is in `ReservedOutboundState` or
    /// `UnnegotiatedState Outbound`, transitions to `TerminatedState`.
    pub fn outbound_connect_failed(
        &mut self,
        peer: SocketAddr,
    ) -> Result<(), ConnectionManagerError> {
        let entry = self
            .connections
            .get_mut(&peer)
            .ok_or(ConnectionManagerError::UnknownPeer(peer))?;

        match &entry.state {
            ConnectionState::ReservedOutboundState
            | ConnectionState::UnnegotiatedState {
                provenance: Provenance::Outbound,
                ..
            } => {
                entry.state = ConnectionState::TerminatedState { error: None };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                Ok(())
            }
            other => Err(ConnectionManagerError::ForbiddenOperation {
                peer,
                state: other.abstract_state(),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // 2. Release outbound connection
    // -----------------------------------------------------------------------

    /// Release (demote) an outbound connection to cold.
    ///
    /// Upstream: `unregisterOutboundConnection`.
    pub fn release_outbound_connection(
        &mut self,
        peer: SocketAddr,
    ) -> (ReleaseOutboundResult, Vec<CmAction>) {
        let entry = match self.connections.get_mut(&peer) {
            None => {
                return (
                    ReleaseOutboundResult::Noop(AbstractState::UnknownConnectionSt),
                    Vec::new(),
                );
            }
            Some(e) => e,
        };

        let from = entry.state.abstract_state();
        match &entry.state {
            // Cannot demote reserved or unnegotiated.
            ConnectionState::ReservedOutboundState | ConnectionState::UnnegotiatedState { .. } => (
                ReleaseOutboundResult::Error(ConnectionManagerError::ForbiddenOperation {
                    peer,
                    state: from,
                }),
                Vec::new(),
            ),

            // OutboundUni → OutboundIdle(Uni) → Terminate
            ConnectionState::OutboundUniState { conn_id } => {
                let cid = *conn_id;
                entry.state = ConnectionState::OutboundIdleState {
                    conn_id: cid,
                    data_flow: DataFlow::Unidirectional,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                let to = entry.state.abstract_state();
                (
                    ReleaseOutboundResult::DemotedToColdLocal(to),
                    vec![CmAction::TerminateConnection(cid)],
                )
            }

            // OutboundDup(Expired) → OutboundIdle(Duplex) → Terminate
            ConnectionState::OutboundDupState {
                conn_id,
                timeout_expired: TimeoutExpired::Expired,
            } => {
                let cid = *conn_id;
                entry.state = ConnectionState::OutboundIdleState {
                    conn_id: cid,
                    data_flow: DataFlow::Duplex,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                let to = entry.state.abstract_state();
                (
                    ReleaseOutboundResult::DemotedToColdLocal(to),
                    vec![CmAction::TerminateConnection(cid)],
                )
            }

            // OutboundDup(Ticking) → InboundIdle(Duplex)
            // Remote responder timeout still running; demote to inbound
            // since the remote side may still be using it.
            ConnectionState::OutboundDupState {
                conn_id,
                timeout_expired: TimeoutExpired::Ticking,
            } => {
                let cid = *conn_id;
                entry.state = ConnectionState::InboundIdleState {
                    conn_id: cid,
                    data_flow: DataFlow::Duplex,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                // May need pruning due to new inbound connection.
                let prune_actions = self.maybe_prune();
                (
                    ReleaseOutboundResult::Noop(AbstractState::InboundIdleSt(DataFlow::Duplex)),
                    prune_actions,
                )
            }

            // Already idle: no-op.
            ConnectionState::OutboundIdleState { .. } => {
                (ReleaseOutboundResult::Noop(from), Vec::new())
            }

            // InboundIdle(Duplex): outbound already released, no-op.
            ConnectionState::InboundIdleState {
                data_flow: DataFlow::Duplex,
                ..
            } => (ReleaseOutboundResult::Noop(from), Vec::new()),

            // InboundIdle(Uni) or InboundState(Uni): shouldn't happen for outbound release.
            ConnectionState::InboundIdleState {
                data_flow: DataFlow::Unidirectional,
                ..
            }
            | ConnectionState::InboundState {
                data_flow: DataFlow::Unidirectional,
                ..
            } => (
                ReleaseOutboundResult::Error(ConnectionManagerError::ForbiddenOperation {
                    peer,
                    state: from,
                }),
                Vec::new(),
            ),

            // InboundState(Duplex): outbound already demoted, no-op.
            ConnectionState::InboundState {
                data_flow: DataFlow::Duplex,
                ..
            } => (ReleaseOutboundResult::Noop(from), Vec::new()),

            // Duplex → InboundState(Duplex): demote the local (outbound) side.
            ConnectionState::DuplexState { conn_id } => {
                let cid = *conn_id;
                entry.state = ConnectionState::InboundState {
                    conn_id: cid,
                    data_flow: DataFlow::Duplex,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                let to = entry.state.abstract_state();
                (ReleaseOutboundResult::Noop(to), Vec::new())
            }

            // Terminating/Terminated: no-op.
            ConnectionState::TerminatingState { .. } | ConnectionState::TerminatedState { .. } => {
                (ReleaseOutboundResult::Noop(from), Vec::new())
            }
        }
    }

    // -----------------------------------------------------------------------
    // 3. Include inbound connection (accept phase)
    // -----------------------------------------------------------------------

    /// Accept a new inbound connection.
    ///
    /// Upstream: `includeInboundConnectionImpl` (accept phase).
    ///
    /// Returns `Ok(ConnStateId)` on success. The runtime must then run the
    /// handshake and call `inbound_handshake_done`.
    ///
    /// When the inbound count is at or above `hard_limit`, the method
    /// first attempts to prune idle inbound connections via
    /// [`prune_for_inbound`](Self::prune_for_inbound).  If pruning
    /// frees enough capacity the accept proceeds; otherwise the call
    /// returns an error.
    pub fn include_inbound_connection(
        &mut self,
        conn_id: ConnectionId,
    ) -> Result<(ConnStateId, Vec<CmAction>), ConnectionManagerError> {
        let peer = conn_id.remote;

        // Pre-prune: attempt to make room if at capacity.
        let mut prune_actions = Vec::new();
        if self.inbound_connection_count() >= self.limits.hard_limit {
            prune_actions = self.prune_for_inbound(1);
            // After pruning, re-check.
            if self.inbound_connection_count() >= self.limits.hard_limit {
                return Err(ConnectionManagerError::ForbiddenOperation {
                    peer,
                    state: AbstractState::UnknownConnectionSt,
                });
            }
        }

        let id = self.next_conn_state_id();
        let new_entry = ConnectionEntry {
            conn_state_id: id,
            state: ConnectionState::UnnegotiatedState {
                provenance: Provenance::Inbound,
                conn_id,
            },
            responder_timeout_deadline: None,
            time_wait_deadline: None,
        };

        match self.connections.get(&peer).map(|e| &e.state) {
            // Unknown: fresh accept.
            None => {
                self.connections.insert(peer, new_entry);
                Ok((id, prune_actions))
            }

            // ReservedOutbound: overwrite (inbound wins race).
            Some(ConnectionState::ReservedOutboundState) => {
                self.connections.insert(peer, new_entry);
                Ok((id, prune_actions))
            }

            // Unnegotiated: self-connect scenario.
            Some(ConnectionState::UnnegotiatedState { .. }) => {
                self.connections.insert(peer, new_entry);
                Ok((id, prune_actions))
            }

            // Terminating: reuse during TIME_WAIT.
            Some(ConnectionState::TerminatingState { .. }) => {
                self.connections.insert(peer, new_entry);
                Ok((id, prune_actions))
            }

            // Terminated: reuse slot.
            Some(ConnectionState::TerminatedState { .. }) => {
                self.connections.insert(peer, new_entry);
                Ok((id, prune_actions))
            }

            // Any other active state: connection already exists.
            Some(_other) => Err(ConnectionManagerError::ConnectionExists {
                provenance: Provenance::Inbound,
                peer,
            }),
        }
    }

    /// Feed the result of a completed inbound handshake.
    ///
    /// Transitions `UnnegotiatedState Inbound` → `InboundIdleState(data_flow)`.
    pub fn inbound_handshake_done(
        &mut self,
        peer: SocketAddr,
        data_flow: DataFlow,
    ) -> Result<AbstractState, ConnectionManagerError> {
        let entry = self
            .connections
            .get_mut(&peer)
            .ok_or(ConnectionManagerError::UnknownPeer(peer))?;

        match &entry.state {
            ConnectionState::UnnegotiatedState {
                provenance: Provenance::Inbound,
                conn_id,
            } => {
                let cid = *conn_id;
                entry.state = ConnectionState::InboundIdleState {
                    conn_id: cid,
                    data_flow,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                Ok(entry.state.abstract_state())
            }
            // Outbound won the race for this slot (self-connect): no-op.
            ConnectionState::OutboundUniState { .. }
            | ConnectionState::OutboundDupState { .. }
            | ConnectionState::OutboundIdleState { .. } => Ok(entry.state.abstract_state()),
            other => Err(ConnectionManagerError::ForbiddenOperation {
                peer,
                state: other.abstract_state(),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // 4. Release inbound connection
    // -----------------------------------------------------------------------

    /// Release an inbound connection.
    ///
    /// Upstream: `unregisterInboundConnection` /
    /// `releaseInboundConnection`.
    ///
    /// Called by the IG when the idle timeout fires on a connection in
    /// `RemoteIdleSt`.
    pub fn release_inbound_connection(
        &mut self,
        peer: SocketAddr,
    ) -> (OperationResult<DemotedToColdRemoteTr>, Vec<CmAction>) {
        let entry = match self.connections.get_mut(&peer) {
            None => {
                return (
                    OperationResult::OperationSuccess(DemotedToColdRemoteTr::CommitTr),
                    Vec::new(),
                );
            }
            Some(e) => e,
        };

        match &entry.state {
            // InboundIdle → Terminating (Commit)
            ConnectionState::InboundIdleState { conn_id, .. } => {
                let cid = *conn_id;
                entry.state = ConnectionState::TerminatingState {
                    conn_id: cid,
                    error: None,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = Some(Instant::now() + TIME_WAIT_TIMEOUT);
                (
                    OperationResult::OperationSuccess(DemotedToColdRemoteTr::CommitTr),
                    vec![CmAction::TerminateConnection(cid)],
                )
            }

            // OutboundDup(Ticking) → OutboundDup(Expired) (KeepTr)
            ConnectionState::OutboundDupState {
                conn_id,
                timeout_expired: TimeoutExpired::Ticking,
            } => {
                let cid = *conn_id;
                entry.state = ConnectionState::OutboundDupState {
                    conn_id: cid,
                    timeout_expired: TimeoutExpired::Expired,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                (
                    OperationResult::OperationSuccess(DemotedToColdRemoteTr::KeepTr),
                    Vec::new(),
                )
            }

            // OutboundDup(Expired): already expired, KeepTr.
            ConnectionState::OutboundDupState {
                timeout_expired: TimeoutExpired::Expired,
                ..
            } => (
                OperationResult::OperationSuccess(DemotedToColdRemoteTr::KeepTr),
                Vec::new(),
            ),

            // Duplex → OutboundDup(Ticking): remote side released,
            // outbound still active. KeepTr.
            ConnectionState::DuplexState { conn_id } => {
                let cid = *conn_id;
                entry.state = ConnectionState::OutboundDupState {
                    conn_id: cid,
                    timeout_expired: TimeoutExpired::Ticking,
                };
                entry.responder_timeout_deadline = Some(Instant::now() + PROTOCOL_IDLE_TIMEOUT);
                entry.time_wait_deadline = None;
                (
                    OperationResult::OperationSuccess(DemotedToColdRemoteTr::KeepTr),
                    Vec::new(),
                )
            }

            // OutboundIdle: assertion warning, CommitTr.
            ConnectionState::OutboundIdleState { conn_id, .. } => {
                let cid = *conn_id;
                entry.state = ConnectionState::TerminatingState {
                    conn_id: cid,
                    error: None,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = Some(Instant::now() + TIME_WAIT_TIMEOUT);
                (
                    OperationResult::OperationSuccess(DemotedToColdRemoteTr::CommitTr),
                    vec![CmAction::TerminateConnection(cid)],
                )
            }

            // Terminating: already going down, CommitTr.
            ConnectionState::TerminatingState { .. } => (
                OperationResult::OperationSuccess(DemotedToColdRemoteTr::CommitTr),
                Vec::new(),
            ),

            // Terminated.
            ConnectionState::TerminatedState { .. } => (
                OperationResult::UnsupportedState(AbstractState::TerminatedSt),
                Vec::new(),
            ),

            // InboundState: IG should have called demoted_to_cold_remote first.
            ConnectionState::InboundState { .. } => (
                OperationResult::UnsupportedState(entry.state.abstract_state()),
                Vec::new(),
            ),

            // All other states are unsupported for this operation.
            other => (
                OperationResult::UnsupportedState(other.abstract_state()),
                Vec::new(),
            ),
        }
    }

    // -----------------------------------------------------------------------
    // 5. Promoted to warm remote
    // -----------------------------------------------------------------------

    /// Remote side responders have started (promoted from idle to warm/hot).
    ///
    /// Upstream: `promotedToWarmRemote`.
    ///
    /// Called by the IG when it observes `AwakeRemote` or
    /// `PromotedToWarm^{Duplex}_{Remote}`.
    pub fn promoted_to_warm_remote(
        &mut self,
        peer: SocketAddr,
    ) -> (OperationResult<AbstractState>, Vec<CmAction>) {
        let entry = match self.connections.get_mut(&peer) {
            None => {
                return (
                    OperationResult::UnsupportedState(AbstractState::UnknownConnectionSt),
                    Vec::new(),
                );
            }
            Some(e) => e,
        };

        match &entry.state {
            // InboundIdle(Uni) → InboundState(Uni): Awake^Uni_Remote
            ConnectionState::InboundIdleState {
                conn_id,
                data_flow: DataFlow::Unidirectional,
            } => {
                let cid = *conn_id;
                entry.state = ConnectionState::InboundState {
                    conn_id: cid,
                    data_flow: DataFlow::Unidirectional,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                let to = entry.state.abstract_state();
                (OperationResult::OperationSuccess(to), Vec::new())
            }

            // InboundIdle(Duplex) → InboundState(Duplex): Awake^Duplex_Remote
            ConnectionState::InboundIdleState {
                conn_id,
                data_flow: DataFlow::Duplex,
            } => {
                let cid = *conn_id;
                entry.state = ConnectionState::InboundState {
                    conn_id: cid,
                    data_flow: DataFlow::Duplex,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                let to = entry.state.abstract_state();
                let prune = self.maybe_prune();
                (OperationResult::OperationSuccess(to), prune)
            }

            // OutboundDup → DuplexState: PromotedToWarm^Duplex_Remote
            ConnectionState::OutboundDupState { conn_id, .. } => {
                let cid = *conn_id;
                entry.state = ConnectionState::DuplexState { conn_id: cid };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                let to = entry.state.abstract_state();
                let prune = self.maybe_prune();
                (OperationResult::OperationSuccess(to), prune)
            }

            // OutboundIdle(Duplex) → InboundState(Duplex): Awake^Duplex_Remote
            ConnectionState::OutboundIdleState {
                conn_id,
                data_flow: DataFlow::Duplex,
            } => {
                let cid = *conn_id;
                entry.state = ConnectionState::InboundState {
                    conn_id: cid,
                    data_flow: DataFlow::Duplex,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                let to = entry.state.abstract_state();
                let prune = self.maybe_prune();
                (OperationResult::OperationSuccess(to), prune)
            }

            // InboundState: identity transition (already active).
            ConnectionState::InboundState { .. } => (
                OperationResult::OperationSuccess(entry.state.abstract_state()),
                Vec::new(),
            ),

            // Terminating/Terminated: connection gone.
            ConnectionState::TerminatingState { .. } | ConnectionState::TerminatedState { .. } => (
                OperationResult::TerminatedConnection(entry.state.abstract_state()),
                Vec::new(),
            ),

            // Everything else: unsupported.
            other => (
                OperationResult::UnsupportedState(other.abstract_state()),
                Vec::new(),
            ),
        }
    }

    // -----------------------------------------------------------------------
    // 6. Demoted to cold remote
    // -----------------------------------------------------------------------

    /// Remote side responders have all stopped (demoted to cold/idle).
    ///
    /// Upstream: `demotedToColdRemote`. **Idempotent.**
    ///
    /// Called by the IG when it observes `WaitIdleRemote` /
    /// `DemotedToCold^{*}_{Remote}`.
    pub fn demoted_to_cold_remote(
        &mut self,
        peer: SocketAddr,
    ) -> (OperationResult<AbstractState>, Vec<CmAction>) {
        let entry = match self.connections.get_mut(&peer) {
            None => {
                return (
                    OperationResult::UnsupportedState(AbstractState::UnknownConnectionSt),
                    Vec::new(),
                );
            }
            Some(e) => e,
        };

        match &entry.state {
            // InboundState(df) → InboundIdle(df): DemotedToCold^df_Remote
            ConnectionState::InboundState { conn_id, data_flow } => {
                let cid = *conn_id;
                let df = *data_flow;
                entry.state = ConnectionState::InboundIdleState {
                    conn_id: cid,
                    data_flow: df,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                let to = entry.state.abstract_state();
                (OperationResult::OperationSuccess(to), Vec::new())
            }

            // Duplex → OutboundDup(Ticking): DemotedToCold^Duplex_Remote
            ConnectionState::DuplexState { conn_id } => {
                let cid = *conn_id;
                entry.state = ConnectionState::OutboundDupState {
                    conn_id: cid,
                    timeout_expired: TimeoutExpired::Ticking,
                };
                entry.responder_timeout_deadline = Some(Instant::now() + PROTOCOL_IDLE_TIMEOUT);
                entry.time_wait_deadline = None;
                let to = entry.state.abstract_state();
                (OperationResult::OperationSuccess(to), Vec::new())
            }

            // Idempotent: already idle or outbound variants are no-ops.
            ConnectionState::OutboundUniState { .. }
            | ConnectionState::OutboundDupState { .. }
            | ConnectionState::OutboundIdleState { .. }
            | ConnectionState::InboundIdleState { .. } => (
                OperationResult::OperationSuccess(entry.state.abstract_state()),
                Vec::new(),
            ),

            // Terminating/Terminated: connection gone.
            ConnectionState::TerminatingState { .. } | ConnectionState::TerminatedState { .. } => (
                OperationResult::TerminatedConnection(entry.state.abstract_state()),
                Vec::new(),
            ),

            // Everything else: unsupported.
            other => (
                OperationResult::UnsupportedState(other.abstract_state()),
                Vec::new(),
            ),
        }
    }

    // -----------------------------------------------------------------------
    // Timeout transitions
    // -----------------------------------------------------------------------

    /// Notify that the responder timeout for an outbound duplex connection
    /// has expired.
    ///
    /// Transitions: `OutboundDupState(Ticking)` → `OutboundDupState(Expired)`.
    pub fn responder_timeout_expired(
        &mut self,
        peer: SocketAddr,
    ) -> Result<AbstractState, ConnectionManagerError> {
        let entry = self
            .connections
            .get_mut(&peer)
            .ok_or(ConnectionManagerError::UnknownPeer(peer))?;

        match &entry.state {
            ConnectionState::OutboundDupState {
                conn_id,
                timeout_expired: TimeoutExpired::Ticking,
            } => {
                let cid = *conn_id;
                entry.state = ConnectionState::OutboundDupState {
                    conn_id: cid,
                    timeout_expired: TimeoutExpired::Expired,
                };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                Ok(entry.state.abstract_state())
            }
            other => Err(ConnectionManagerError::ForbiddenOperation {
                peer,
                state: other.abstract_state(),
            }),
        }
    }

    /// Transition from `TerminatingState` to `TerminatedState` after the
    /// TIME_WAIT timeout.
    pub fn time_wait_expired(&mut self, peer: SocketAddr) -> Result<(), ConnectionManagerError> {
        let entry = self
            .connections
            .get_mut(&peer)
            .ok_or(ConnectionManagerError::UnknownPeer(peer))?;

        match &entry.state {
            ConnectionState::TerminatingState { error, .. } => {
                let err = error.clone();
                entry.state = ConnectionState::TerminatedState { error: err };
                entry.responder_timeout_deadline = None;
                entry.time_wait_deadline = None;
                Ok(())
            }
            other => Err(ConnectionManagerError::ForbiddenOperation {
                peer,
                state: other.abstract_state(),
            }),
        }
    }

    /// Remove a terminated connection from the map.
    ///
    /// Upstream: entries in `TerminatedSt` are eventually garbage-collected.
    pub fn remove_terminated(&mut self, peer: &SocketAddr) -> bool {
        if let Some(entry) = self.connections.get(peer) {
            if matches!(entry.state, ConnectionState::TerminatedState { .. }) {
                self.connections.remove(peer);
                return true;
            }
        }
        false
    }

    /// Mark a connection as terminating (e.g. on error, timeout, or
    /// explicit close by the runtime).
    pub fn mark_terminating(
        &mut self,
        peer: SocketAddr,
        reason: Option<String>,
    ) -> Option<AbstractState> {
        let entry = self.connections.get_mut(&peer)?;
        match &entry.state {
            // Don't transition if already terminating/terminated.
            ConnectionState::TerminatingState { .. } | ConnectionState::TerminatedState { .. } => {
                Some(entry.state.abstract_state())
            }
            // Any other state → TerminatingState.
            _ => {
                let conn_id = entry.state.conn_id();
                if let Some(cid) = conn_id {
                    entry.state = ConnectionState::TerminatingState {
                        conn_id: cid,
                        error: reason,
                    };
                    entry.responder_timeout_deadline = None;
                    entry.time_wait_deadline = Some(Instant::now() + TIME_WAIT_TIMEOUT);
                } else {
                    entry.state = ConnectionState::TerminatedState { error: reason };
                    entry.responder_timeout_deadline = None;
                    entry.time_wait_deadline = None;
                }
                Some(entry.state.abstract_state())
            }
        }
    }

    /// Advance timeout-driven transitions and return any resulting actions.
    ///
    /// This mirrors the upstream CM timeout maintenance where expiry events
    /// are fed back into the state machine over time.
    pub fn timeout_tick(&mut self, now: Instant) -> Vec<CmAction> {
        let mut actions = Vec::new();

        let peers: Vec<SocketAddr> = self.connections.keys().copied().collect();
        for peer in peers {
            let entry_snapshot = self.connections.get(&peer).cloned();
            let Some(entry) = entry_snapshot else {
                continue;
            };

            if matches!(
                entry.state,
                ConnectionState::OutboundDupState {
                    timeout_expired: TimeoutExpired::Ticking,
                    ..
                }
            ) && entry
                .responder_timeout_deadline
                .is_some_and(|deadline| now >= deadline)
            {
                let _ = self.responder_timeout_expired(peer);
            }

            if matches!(entry.state, ConnectionState::TerminatingState { .. })
                && entry
                    .time_wait_deadline
                    .is_some_and(|deadline| now >= deadline)
            {
                let _ = self.time_wait_expired(peer);
                let _removed = self.remove_terminated(&peer);
            }
        }

        // Opportunistically collect any stale terminated entries that may
        // remain from non-timeout CM transitions.
        let terminated: Vec<SocketAddr> = self
            .connections
            .iter()
            .filter_map(|(peer, entry)| {
                matches!(entry.state, ConnectionState::TerminatedState { .. }).then_some(*peer)
            })
            .collect();
        for peer in terminated {
            self.connections.remove(&peer);
        }

        actions.append(&mut self.maybe_prune());
        actions
    }

    // -----------------------------------------------------------------------
    // Pruning
    // -----------------------------------------------------------------------

    /// Proactively prune up to `needed` idle inbound connections to make
    /// room for a new inbound accept.
    ///
    /// Upstream: `includeInboundConnectionImpl` calls the prune policy
    /// before the hard-limit check so that an incoming connection can
    /// succeed even when the inbound count is at the limit, as long as
    /// idle connections are available for eviction.
    ///
    /// Selection priority (matching upstream `simplePrunePolicy`):
    /// 1. `InboundIdleState` — idle inbound connections.
    /// 2. `TerminatedState` — already-terminated entries (free slots).
    ///
    /// Within each group, the most recently added entry (highest
    /// `ConnStateId`) is pruned first.
    ///
    /// Selected entries are moved to `TerminatingState` (or removed if
    /// already terminated) and the corresponding `CmAction::PruneConnections`
    /// is emitted for the runtime to close the sockets.
    pub fn prune_for_inbound(&mut self, needed: usize) -> Vec<CmAction> {
        if needed == 0 {
            return Vec::new();
        }

        // Collect prunable connections: prefer idle, then terminated.
        let mut prunable: Vec<(SocketAddr, ConnStateId, bool)> = self
            .connections
            .iter()
            .filter_map(|(addr, e)| match &e.state {
                ConnectionState::InboundIdleState { .. } => Some((*addr, e.conn_state_id, false)),
                ConnectionState::TerminatedState { .. } => Some((*addr, e.conn_state_id, true)),
                _ => None,
            })
            .collect();

        // Sort: terminated first (free slots), then idle by descending
        // ConnStateId (most recently added first).
        prunable.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.1.0.cmp(&a.1.0)));

        let mut to_prune_addrs = Vec::new();
        for (addr, _id, is_terminated) in prunable.into_iter().take(needed) {
            if is_terminated {
                // Remove terminated entries outright — they're already done.
                self.connections.remove(&addr);
            } else {
                // Transition InboundIdle → Terminating.
                if let Some(entry) = self.connections.get_mut(&addr) {
                    if let Some(cid) = entry.state.conn_id() {
                        entry.state = ConnectionState::TerminatingState {
                            conn_id: cid,
                            error: Some("pruned for inbound capacity".to_owned()),
                        };
                        entry.responder_timeout_deadline = None;
                        entry.time_wait_deadline = Some(
                            std::time::Instant::now()
                                + crate::connection::timeouts::TIME_WAIT_TIMEOUT,
                        );
                    }
                }
                to_prune_addrs.push(addr);
            }
        }

        if to_prune_addrs.is_empty() {
            Vec::new()
        } else {
            vec![CmAction::PruneConnections(to_prune_addrs)]
        }
    }

    /// Check if the inbound connection count exceeds the hard limit and
    /// select connections for pruning if so.
    ///
    /// Upstream: `mkPruneAction` selects connections to prune using the
    /// `PrunePolicy`. Our default policy selects the most recently added
    /// inbound connections (highest ConnStateId) that are in idle states.
    fn maybe_prune(&self) -> Vec<CmAction> {
        let inbound = self.inbound_connection_count();
        if inbound <= self.limits.hard_limit {
            return Vec::new();
        }

        let excess = (inbound - self.limits.hard_limit) as usize;

        // Collect prunable connections: inbound idle only.
        //
        // Terminated entries are cleaned up by `timeout_tick` directly and
        // should never consume prune budget, since they do not contribute to
        // `inbound_connection_count`.
        let mut prunable: Vec<(SocketAddr, ConnStateId)> = self
            .connections
            .iter()
            .filter(|(_, e)| matches!(e.state, ConnectionState::InboundIdleState { .. }))
            .map(|(addr, e)| (*addr, e.conn_state_id))
            .collect();

        // Sort by ConnStateId descending (prune most recently added first,
        // matching upstream default PrunePolicy).
        prunable.sort_by(|a, b| b.1.0.cmp(&a.1.0));

        let to_prune: Vec<SocketAddr> = prunable.into_iter().take(excess).map(|(a, _)| a).collect();

        if to_prune.is_empty() {
            Vec::new()
        } else {
            vec![CmAction::PruneConnections(to_prune)]
        }
    }
}

impl Default for ConnectionManagerState {
    fn default() -> Self {
        Self::new()
    }
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

    fn local() -> SocketAddr {
        addr(1000)
    }

    fn peer(port: u16) -> SocketAddr {
        addr(port)
    }

    // -- Constructor --

    #[test]
    fn new_state_is_empty() {
        let cm = ConnectionManagerState::new();
        assert_eq!(cm.connection_count(), 0);
        assert_eq!(cm.inbound_connection_count(), 0);
    }

    #[test]
    fn default_limits() {
        let cm = ConnectionManagerState::new();
        assert_eq!(cm.limits.hard_limit, 512);
        assert_eq!(cm.limits.soft_limit, 384);
    }

    #[test]
    fn custom_limits() {
        let cm = ConnectionManagerState::with_limits(AcceptedConnectionsLimit {
            hard_limit: 10,
            soft_limit: 8,
            delay: std::time::Duration::from_secs(1),
        });
        assert_eq!(cm.limits.hard_limit, 10);
    }

    #[test]
    fn timeout_tick_expires_outbound_dup_responder_timeout() {
        let mut cm = ConnectionManagerState::new();
        let p = peer(2001);

        let _ = cm
            .acquire_outbound_connection(local(), p)
            .expect("acquire outbound");
        cm.outbound_handshake_done(local(), p, DataFlow::Duplex)
            .expect("handshake done");

        let deadline = cm
            .connections
            .get(&p)
            .and_then(|entry| entry.responder_timeout_deadline)
            .expect("responder deadline set");
        let _ = cm.timeout_tick(deadline + std::time::Duration::from_secs(1));

        assert_eq!(
            cm.abstract_state_of(&p),
            AbstractState::OutboundDupSt(TimeoutExpired::Expired)
        );
    }

    #[test]
    fn timeout_tick_removes_terminated_after_time_wait() {
        let mut cm = ConnectionManagerState::new();
        let p = peer(2002);

        let _ = cm
            .acquire_outbound_connection(local(), p)
            .expect("acquire outbound");
        cm.outbound_handshake_done(local(), p, DataFlow::Unidirectional)
            .expect("handshake done");
        let _ = cm.mark_terminating(p, Some("test".to_owned()));

        let deadline = cm
            .connections
            .get(&p)
            .and_then(|entry| entry.time_wait_deadline)
            .expect("time wait deadline set");
        let _ = cm.timeout_tick(deadline + std::time::Duration::from_secs(1));

        assert_eq!(cm.abstract_state_of(&p), AbstractState::UnknownConnectionSt);
    }

    #[test]
    fn timeout_tick_pruning_excludes_terminated_entries() {
        let mut cm = ConnectionManagerState::with_limits(AcceptedConnectionsLimit {
            hard_limit: 10,
            soft_limit: 1,
            delay: std::time::Duration::from_secs(1),
        });

        let p1 = peer(2101);
        let p2 = peer(2102);
        let p3 = peer(2103);

        for p in [p1, p2, p3] {
            let cid = ConnectionId {
                local: local(),
                remote: p,
            };
            cm.include_inbound_connection(cid).expect("include inbound");
            cm.inbound_handshake_done(p, DataFlow::Duplex)
                .expect("inbound handshake");
        }

        let _ = cm.mark_terminating(p2, Some("test terminated cleanup".to_owned()));
        cm.time_wait_expired(p2).expect("time wait expiry");
        assert_eq!(cm.abstract_state_of(&p2), AbstractState::TerminatedSt);

        cm.limits.hard_limit = 1;

        let actions = cm.timeout_tick(Instant::now());

        assert_eq!(cm.abstract_state_of(&p2), AbstractState::UnknownConnectionSt);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            CmAction::PruneConnections(addrs) => {
                assert_eq!(addrs.len(), 1);
                assert!(addrs[0] == p1 || addrs[0] == p3);
                assert_ne!(addrs[0], p2);
            }
            other => panic!("expected PruneConnections, got {:?}", other),
        }
    }

    // -- Acquire outbound --

    #[test]
    fn acquire_outbound_fresh() {
        let mut cm = ConnectionManagerState::new();
        let (result, actions) = cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        assert_eq!(result, AcquireOutboundResult::Fresh);
        assert_eq!(actions, vec![CmAction::StartConnect(peer(2000))]);
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::ReservedOutboundSt
        );
    }

    #[test]
    fn acquire_outbound_duplicate_reserved_errors() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        let err = cm
            .acquire_outbound_connection(local(), peer(2000))
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectionManagerError::ConnectionExists { .. }
        ));
    }

    #[test]
    fn acquire_outbound_reuses_idle_inbound_duplex() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();

        let (result, actions) = cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        match result {
            AcquireOutboundResult::Reused(st) => {
                assert_eq!(st, AbstractState::OutboundDupSt(TimeoutExpired::Ticking));
            }
            other => panic!("expected Reused, got {:?}", other),
        }
        assert_eq!(actions, vec![CmAction::StartResponderTimeout(cid)]);
    }

    #[test]
    fn acquire_outbound_rejects_idle_inbound_uni() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Unidirectional)
            .unwrap();

        let err = cm
            .acquire_outbound_connection(local(), peer(2000))
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectionManagerError::ForbiddenConnection(_)
        ));
    }

    #[test]
    fn acquire_outbound_promotes_active_inbound_duplex_to_duplex() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2000));

        let (result, _) = cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        match result {
            AcquireOutboundResult::Reused(st) => {
                assert_eq!(st, AbstractState::DuplexSt);
            }
            other => panic!("expected Reused(DuplexSt), got {:?}", other),
        }
    }

    #[test]
    fn acquire_outbound_disconnected_when_terminating() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        // Put a connection into terminating.
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.mark_terminating(peer(2000), None);

        let (result, _) = cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        assert_eq!(
            result,
            AcquireOutboundResult::Disconnected(AbstractState::TerminatingSt)
        );
    }

    // -- Outbound handshake --

    #[test]
    fn outbound_handshake_uni() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        let st = cm
            .outbound_handshake_done(local(), peer(2000), DataFlow::Unidirectional)
            .unwrap();
        assert_eq!(st, AbstractState::OutboundUniSt);
    }

    #[test]
    fn outbound_handshake_duplex() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        let st = cm
            .outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();
        assert_eq!(st, AbstractState::OutboundDupSt(TimeoutExpired::Ticking));
    }

    #[test]
    fn outbound_connect_failed_transitions_to_terminated() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_connect_failed(peer(2000)).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::TerminatedSt
        );
    }

    // -- Release outbound --

    #[test]
    fn release_outbound_uni_terminates() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Unidirectional)
            .unwrap();

        let (result, actions) = cm.release_outbound_connection(peer(2000));
        match result {
            ReleaseOutboundResult::DemotedToColdLocal(st) => {
                assert_eq!(st, AbstractState::OutboundIdleSt(DataFlow::Unidirectional));
            }
            other => panic!("expected DemotedToColdLocal, got {:?}", other),
        }
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], CmAction::TerminateConnection(_)));
    }

    #[test]
    fn release_outbound_dup_expired_terminates() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.responder_timeout_expired(peer(2000)).unwrap();

        let (result, actions) = cm.release_outbound_connection(peer(2000));
        match result {
            ReleaseOutboundResult::DemotedToColdLocal(st) => {
                assert_eq!(st, AbstractState::OutboundIdleSt(DataFlow::Duplex));
            }
            other => panic!("expected DemotedToColdLocal, got {:?}", other),
        }
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn release_outbound_dup_ticking_demotes_to_inbound_idle() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();

        let (result, _) = cm.release_outbound_connection(peer(2000));
        match result {
            ReleaseOutboundResult::Noop(st) => {
                assert_eq!(st, AbstractState::InboundIdleSt(DataFlow::Duplex));
            }
            other => panic!("expected Noop(InboundIdleSt), got {:?}", other),
        }
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::InboundIdleSt(DataFlow::Duplex)
        );
    }

    #[test]
    fn release_outbound_duplex_state_demotes_to_inbound_active() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2000)); // → DuplexSt

        let (result, _) = cm.release_outbound_connection(peer(2000));
        match result {
            ReleaseOutboundResult::Noop(st) => {
                assert_eq!(st, AbstractState::InboundSt(DataFlow::Duplex));
            }
            other => panic!("expected Noop(InboundSt), got {:?}", other),
        }
    }

    #[test]
    fn release_outbound_unknown_noop() {
        let mut cm = ConnectionManagerState::new();
        let (result, actions) = cm.release_outbound_connection(peer(2000));
        match result {
            ReleaseOutboundResult::Noop(st) => {
                assert_eq!(st, AbstractState::UnknownConnectionSt);
            }
            other => panic!("expected Noop(Unknown), got {:?}", other),
        }
        assert!(actions.is_empty());
    }

    // -- Include inbound --

    #[test]
    fn include_inbound_fresh() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        let (id, actions) = cm.include_inbound_connection(cid).unwrap();
        assert_eq!(id, ConnStateId(0));
        assert!(actions.is_empty());
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::UnnegotiatedSt(Provenance::Inbound)
        );
    }

    #[test]
    fn include_inbound_overwrites_reserved_outbound() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::ReservedOutboundSt
        );

        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::UnnegotiatedSt(Provenance::Inbound)
        );
    }

    #[test]
    fn include_inbound_overwrites_terminated() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.mark_terminating(peer(2000), None);
        cm.time_wait_expired(peer(2000)).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::TerminatedSt
        );

        // Accept new inbound into terminated slot.
        cm.include_inbound_connection(cid).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::UnnegotiatedSt(Provenance::Inbound)
        );
    }

    #[test]
    fn include_inbound_hard_limit() {
        let mut cm = ConnectionManagerState::with_limits(AcceptedConnectionsLimit {
            hard_limit: 2,
            soft_limit: 1,
            delay: std::time::Duration::from_secs(1),
        });

        // Fill up to hard limit with *active* (non-idle) inbound connections
        // so prune_for_inbound cannot evict them.
        for i in 0..2 {
            let p = peer(2000 + i);
            let cid = ConnectionId {
                local: local(),
                remote: p,
            };
            cm.include_inbound_connection(cid).unwrap();
            cm.inbound_handshake_done(p, DataFlow::Duplex).unwrap();
            cm.promoted_to_warm_remote(p); // InboundState (active)
        }

        // Third should fail — no idle connections to prune.
        let cid = ConnectionId {
            local: local(),
            remote: peer(3000),
        };
        let err = cm.include_inbound_connection(cid).unwrap_err();
        assert!(matches!(
            err,
            ConnectionManagerError::ForbiddenOperation { .. }
        ));
    }

    #[test]
    fn include_inbound_rejects_active_connection() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2000)); // InboundState

        // Try to include another inbound from same peer.
        let err = cm.include_inbound_connection(cid).unwrap_err();
        assert!(matches!(
            err,
            ConnectionManagerError::ConnectionExists { .. }
        ));
    }

    // -- Inbound handshake --

    #[test]
    fn inbound_handshake_duplex() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        let st = cm
            .inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        assert_eq!(st, AbstractState::InboundIdleSt(DataFlow::Duplex));
    }

    #[test]
    fn inbound_handshake_uni() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        let st = cm
            .inbound_handshake_done(peer(2000), DataFlow::Unidirectional)
            .unwrap();
        assert_eq!(st, AbstractState::InboundIdleSt(DataFlow::Unidirectional));
    }

    // -- Release inbound --

    #[test]
    fn release_inbound_idle_commits() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();

        let (result, actions) = cm.release_inbound_connection(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(DemotedToColdRemoteTr::CommitTr)
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], CmAction::TerminateConnection(_)));
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::TerminatingSt
        );
    }

    #[test]
    fn release_inbound_outbound_dup_ticking_keeps() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();
        // OutboundDup(Ticking)

        let (result, actions) = cm.release_inbound_connection(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(DemotedToColdRemoteTr::KeepTr)
        );
        assert!(actions.is_empty());
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::OutboundDupSt(TimeoutExpired::Expired)
        );
    }

    #[test]
    fn release_inbound_duplex_keeps_outbound() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2000)); // → DuplexSt

        let (result, actions) = cm.release_inbound_connection(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(DemotedToColdRemoteTr::KeepTr)
        );
        assert!(actions.is_empty());
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::OutboundDupSt(TimeoutExpired::Ticking)
        );
    }

    #[test]
    fn release_inbound_unknown_commits() {
        let mut cm = ConnectionManagerState::new();
        let (result, _) = cm.release_inbound_connection(peer(9999));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(DemotedToColdRemoteTr::CommitTr)
        );
    }

    // -- Promoted to warm remote --

    #[test]
    fn promote_warm_remote_inbound_idle_uni() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Unidirectional)
            .unwrap();

        let (result, _) = cm.promoted_to_warm_remote(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(AbstractState::InboundSt(DataFlow::Unidirectional))
        );
    }

    #[test]
    fn promote_warm_remote_inbound_idle_duplex() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();

        let (result, _) = cm.promoted_to_warm_remote(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(AbstractState::InboundSt(DataFlow::Duplex))
        );
    }

    #[test]
    fn promote_warm_remote_outbound_dup_to_duplex() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();

        let (result, _) = cm.promoted_to_warm_remote(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(AbstractState::DuplexSt)
        );
    }

    #[test]
    fn promote_warm_remote_outbound_idle_duplex() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.responder_timeout_expired(peer(2000)).unwrap();
        cm.release_outbound_connection(peer(2000)); // → OutboundIdle(Duplex)

        let (result, _) = cm.promoted_to_warm_remote(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(AbstractState::InboundSt(DataFlow::Duplex))
        );
    }

    #[test]
    fn promote_warm_remote_identity_for_active_inbound() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2000));

        // Second promote is identity.
        let (result, _) = cm.promoted_to_warm_remote(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(AbstractState::InboundSt(DataFlow::Duplex))
        );
    }

    #[test]
    fn promote_warm_remote_unsupported_for_outbound_uni() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Unidirectional)
            .unwrap();

        let (result, _) = cm.promoted_to_warm_remote(peer(2000));
        assert!(matches!(result, OperationResult::UnsupportedState(_)));
    }

    #[test]
    fn promote_warm_remote_terminated_returns_terminated() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.mark_terminating(peer(2000), None);

        let (result, _) = cm.promoted_to_warm_remote(peer(2000));
        assert!(matches!(result, OperationResult::TerminatedConnection(_)));
    }

    // -- Demoted to cold remote --

    #[test]
    fn demote_cold_remote_inbound_active_to_idle() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2000));
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::InboundSt(DataFlow::Duplex)
        );

        let (result, _) = cm.demoted_to_cold_remote(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(AbstractState::InboundIdleSt(DataFlow::Duplex))
        );
    }

    #[test]
    fn demote_cold_remote_duplex_to_outbound_dup() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2000)); // → DuplexSt

        let (result, _) = cm.demoted_to_cold_remote(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(AbstractState::OutboundDupSt(
                TimeoutExpired::Ticking
            ))
        );
    }

    #[test]
    fn demote_cold_remote_idempotent_for_idle() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();

        // Already idle.
        let (result, _) = cm.demoted_to_cold_remote(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(AbstractState::InboundIdleSt(DataFlow::Duplex))
        );
    }

    #[test]
    fn demote_cold_remote_terminated_returns_terminated() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.mark_terminating(peer(2000), None);

        let (result, _) = cm.demoted_to_cold_remote(peer(2000));
        assert!(matches!(result, OperationResult::TerminatedConnection(_)));
    }

    // -- Timeout transitions --

    #[test]
    fn responder_timeout_expired_transitions() {
        let mut cm = ConnectionManagerState::new();
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();

        let st = cm.responder_timeout_expired(peer(2000)).unwrap();
        assert_eq!(st, AbstractState::OutboundDupSt(TimeoutExpired::Expired));
    }

    #[test]
    fn time_wait_expired_transitions() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.mark_terminating(peer(2000), None);

        cm.time_wait_expired(peer(2000)).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::TerminatedSt
        );
    }

    // -- Cleanup --

    #[test]
    fn remove_terminated() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.mark_terminating(peer(2000), None);
        cm.time_wait_expired(peer(2000)).unwrap();

        assert!(cm.remove_terminated(&peer(2000)));
        assert_eq!(cm.connection_count(), 0);
    }

    #[test]
    fn remove_terminated_rejects_active() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();

        assert!(!cm.remove_terminated(&peer(2000)));
        assert_eq!(cm.connection_count(), 1);
    }

    // -- Full lifecycle --

    #[test]
    fn full_outbound_lifecycle() {
        let mut cm = ConnectionManagerState::new();

        // 1. Acquire → Reserved
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::ReservedOutboundSt
        );

        // 2. Handshake → OutboundDup(Ticking)
        cm.outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::OutboundDupSt(TimeoutExpired::Ticking)
        );

        // 3. Remote starts → DuplexSt
        cm.promoted_to_warm_remote(peer(2000));
        assert_eq!(cm.abstract_state_of(&peer(2000)), AbstractState::DuplexSt);

        // 4. Remote stops → OutboundDup(Ticking) (demoted remote)
        cm.demoted_to_cold_remote(peer(2000));
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::OutboundDupSt(TimeoutExpired::Ticking)
        );

        // 5. Timeout → OutboundDup(Expired)
        cm.responder_timeout_expired(peer(2000)).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::OutboundDupSt(TimeoutExpired::Expired)
        );

        // 6. Release outbound → OutboundIdle → Terminate.
        let (result, actions) = cm.release_outbound_connection(peer(2000));
        assert!(matches!(
            result,
            ReleaseOutboundResult::DemotedToColdLocal(_)
        ));
        assert_eq!(actions.len(), 1);

        // 7. Mark terminating.
        cm.mark_terminating(peer(2000), None);
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::TerminatingSt
        );

        // 8. TIME_WAIT → Terminated.
        cm.time_wait_expired(peer(2000)).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::TerminatedSt
        );

        // 9. Cleanup.
        assert!(cm.remove_terminated(&peer(2000)));
        assert_eq!(cm.connection_count(), 0);
    }

    #[test]
    fn full_inbound_lifecycle() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };

        // 1. Accept → Unnegotiated(Inbound)
        cm.include_inbound_connection(cid).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::UnnegotiatedSt(Provenance::Inbound)
        );
        assert_eq!(cm.inbound_connection_count(), 0); // Unnegotiated Inbound not counted by upstream is_inbound_conn

        // 2. Handshake → InboundIdle(Duplex)
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::InboundIdleSt(DataFlow::Duplex)
        );
        assert_eq!(cm.inbound_connection_count(), 1);

        // 3. Remote wakes → InboundState(Duplex)
        cm.promoted_to_warm_remote(peer(2000));
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::InboundSt(DataFlow::Duplex)
        );

        // 4. Remote goes idle → InboundIdle(Duplex)
        cm.demoted_to_cold_remote(peer(2000));
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::InboundIdleSt(DataFlow::Duplex)
        );

        // 5. Release inbound → Terminating (CommitTr)
        let (result, actions) = cm.release_inbound_connection(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(DemotedToColdRemoteTr::CommitTr)
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::TerminatingSt
        );

        // 6. TIME_WAIT → Terminated.
        cm.time_wait_expired(peer(2000)).unwrap();
        assert!(cm.remove_terminated(&peer(2000)));
        assert_eq!(cm.connection_count(), 0);
    }

    #[test]
    fn duplex_reuse_lifecycle() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };

        // Inbound arrives and goes active.
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2000));
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::InboundSt(DataFlow::Duplex)
        );

        // Outbound governor wants to reuse → DuplexSt.
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        assert_eq!(cm.abstract_state_of(&peer(2000)), AbstractState::DuplexSt);

        // Governor releases outbound → InboundState(Duplex).
        cm.release_outbound_connection(peer(2000));
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::InboundSt(DataFlow::Duplex)
        );

        // IG demotes remote → InboundIdle.
        cm.demoted_to_cold_remote(peer(2000));
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::InboundIdleSt(DataFlow::Duplex)
        );

        // Release inbound → Commit.
        let (result, _) = cm.release_inbound_connection(peer(2000));
        assert_eq!(
            result,
            OperationResult::OperationSuccess(DemotedToColdRemoteTr::CommitTr)
        );
    }

    // -- Counter accuracy --

    #[test]
    fn counter_accuracy_multi_peer() {
        let mut cm = ConnectionManagerState::new();

        // Peer 1: outbound duplex active
        cm.acquire_outbound_connection(local(), peer(2001)).unwrap();
        cm.outbound_handshake_done(local(), peer(2001), DataFlow::Duplex)
            .unwrap();

        // Peer 2: inbound duplex active
        let cid2 = ConnectionId {
            local: local(),
            remote: peer(2002),
        };
        cm.include_inbound_connection(cid2).unwrap();
        cm.inbound_handshake_done(peer(2002), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2002));

        // Peer 3: full duplex
        cm.acquire_outbound_connection(local(), peer(2003)).unwrap();
        cm.outbound_handshake_done(local(), peer(2003), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2003));

        assert_eq!(cm.connection_count(), 3);
        // InboundSt(Duplex) + DuplexSt = 2 inbound
        assert_eq!(cm.inbound_connection_count(), 2);
    }

    #[test]
    fn abstract_state_map_snapshot() {
        let mut cm = ConnectionManagerState::new();

        cm.acquire_outbound_connection(local(), peer(2001)).unwrap();
        cm.outbound_handshake_done(local(), peer(2001), DataFlow::Duplex)
            .unwrap();

        let cid = ConnectionId {
            local: local(),
            remote: peer(2002),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2002), DataFlow::Unidirectional)
            .unwrap();

        let map = cm.abstract_state_map();
        assert_eq!(map.len(), 2);
        assert_eq!(
            map[&peer(2001)],
            AbstractState::OutboundDupSt(TimeoutExpired::Ticking)
        );
        assert_eq!(
            map[&peer(2002)],
            AbstractState::InboundIdleSt(DataFlow::Unidirectional)
        );
    }

    #[test]
    fn mark_terminating_from_various_states() {
        // Inbound active → terminating.
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(2000), DataFlow::Duplex)
            .unwrap();
        cm.promoted_to_warm_remote(peer(2000));

        let st = cm.mark_terminating(peer(2000), Some("test".into()));
        assert_eq!(st, Some(AbstractState::TerminatingSt));

        // Already terminating → returns TerminatingSt (no-op).
        let st = cm.mark_terminating(peer(2000), None);
        assert_eq!(st, Some(AbstractState::TerminatingSt));
    }

    #[test]
    fn overwrite_race_outbound_reserved_by_inbound() {
        let mut cm = ConnectionManagerState::new();

        // Outbound reserves.
        cm.acquire_outbound_connection(local(), peer(2000)).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::ReservedOutboundSt
        );

        // Inbound arrives and overwrites.
        let cid = ConnectionId {
            local: local(),
            remote: peer(2000),
        };
        cm.include_inbound_connection(cid).unwrap();
        assert_eq!(
            cm.abstract_state_of(&peer(2000)),
            AbstractState::UnnegotiatedSt(Provenance::Inbound)
        );

        // Original outbound connect will fail. Outbound handshake on the
        // inbound entry fails because it's Inbound provenance.
        let err = cm
            .outbound_handshake_done(local(), peer(2000), DataFlow::Duplex)
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectionManagerError::ForbiddenOperation { .. }
        ));
    }

    // -- Prune for inbound --

    #[test]
    fn prune_for_inbound_empty_cm_returns_empty() {
        let mut cm = ConnectionManagerState::new();
        let actions = cm.prune_for_inbound(1);
        assert!(actions.is_empty());
    }

    #[test]
    fn prune_for_inbound_evicts_idle_inbound() {
        let mut cm = ConnectionManagerState::with_limits(AcceptedConnectionsLimit {
            hard_limit: 2,
            soft_limit: 1,
            delay: std::time::Duration::from_secs(1),
        });

        // Insert two idle inbound connections.
        for i in 0..2 {
            let p = peer(3000 + i);
            let cid = ConnectionId {
                local: local(),
                remote: p,
            };
            cm.include_inbound_connection(cid).unwrap();
            cm.inbound_handshake_done(p, DataFlow::Duplex).unwrap();
        }

        let actions = cm.prune_for_inbound(1);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            CmAction::PruneConnections(addrs) => {
                assert_eq!(addrs.len(), 1);
                // The most recently added (highest ConnStateId) should be
                // selected for pruning.
            }
            other => panic!("expected PruneConnections, got {:?}", other),
        }
    }

    #[test]
    fn prune_for_inbound_removes_terminated_first() {
        let mut cm = ConnectionManagerState::with_limits(AcceptedConnectionsLimit {
            hard_limit: 2,
            soft_limit: 1,
            delay: std::time::Duration::from_secs(1),
        });

        // Insert one idle and one terminated.
        let p1 = peer(3010);
        let cid1 = ConnectionId {
            local: local(),
            remote: p1,
        };
        cm.include_inbound_connection(cid1).unwrap();
        cm.inbound_handshake_done(p1, DataFlow::Duplex).unwrap();

        let p2 = peer(3011);
        let cid2 = ConnectionId {
            local: local(),
            remote: p2,
        };
        cm.include_inbound_connection(cid2).unwrap();
        cm.inbound_handshake_done(p2, DataFlow::Duplex).unwrap();
        cm.mark_terminating(p2, None);
        cm.time_wait_expired(p2).unwrap();
        assert_eq!(cm.abstract_state_of(&p2), AbstractState::TerminatedSt);

        // Prune 1: should pick terminated first (free slot, no close needed).
        let actions = cm.prune_for_inbound(1);
        // Terminated entry is removed outright — no PruneConnections needed.
        assert!(
            actions.is_empty(),
            "terminated removal needs no runtime action"
        );
        // p2 should be gone.
        assert_eq!(
            cm.abstract_state_of(&p2),
            AbstractState::UnknownConnectionSt
        );
    }

    #[test]
    fn prune_for_inbound_does_not_evict_active_connections() {
        let mut cm = ConnectionManagerState::with_limits(AcceptedConnectionsLimit {
            hard_limit: 2,
            soft_limit: 1,
            delay: std::time::Duration::from_secs(1),
        });

        // Insert two *active* inbound connections (InboundState, not idle).
        for i in 0..2 {
            let p = peer(3020 + i);
            let cid = ConnectionId {
                local: local(),
                remote: p,
            };
            cm.include_inbound_connection(cid).unwrap();
            cm.inbound_handshake_done(p, DataFlow::Duplex).unwrap();
            cm.promoted_to_warm_remote(p); // → InboundState (active)
        }

        // No idle connections to prune.
        let actions = cm.prune_for_inbound(1);
        assert!(actions.is_empty());
    }

    #[test]
    fn include_inbound_prunes_to_make_room() {
        let mut cm = ConnectionManagerState::with_limits(AcceptedConnectionsLimit {
            hard_limit: 2,
            soft_limit: 1,
            delay: std::time::Duration::from_secs(1),
        });

        // Fill to hard limit with idle inbound.
        for i in 0..2 {
            let p = peer(3030 + i);
            let cid = ConnectionId {
                local: local(),
                remote: p,
            };
            cm.include_inbound_connection(cid).unwrap();
            cm.inbound_handshake_done(p, DataFlow::Duplex).unwrap();
        }
        assert_eq!(cm.inbound_connection_count(), 2);

        // New inbound should succeed by pruning one idle connection.
        let new_peer = peer(3040);
        let new_cid = ConnectionId {
            local: local(),
            remote: new_peer,
        };
        let (id, actions) = cm.include_inbound_connection(new_cid).unwrap();
        assert!(id.0 > 0);
        // Should have emitted PruneConnections for the evicted idle peer.
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], CmAction::PruneConnections(_)));
    }

    #[test]
    fn include_inbound_fails_when_all_active() {
        let mut cm = ConnectionManagerState::with_limits(AcceptedConnectionsLimit {
            hard_limit: 2,
            soft_limit: 1,
            delay: std::time::Duration::from_secs(1),
        });

        // Fill to hard limit with active (non-idle) inbound.
        for i in 0..2 {
            let p = peer(3050 + i);
            let cid = ConnectionId {
                local: local(),
                remote: p,
            };
            cm.include_inbound_connection(cid).unwrap();
            cm.inbound_handshake_done(p, DataFlow::Duplex).unwrap();
            cm.promoted_to_warm_remote(p); // active
        }

        // New inbound should fail — nothing to prune.
        let new_cid = ConnectionId {
            local: local(),
            remote: peer(3060),
        };
        let err = cm.include_inbound_connection(new_cid).unwrap_err();
        assert!(matches!(
            err,
            ConnectionManagerError::ForbiddenOperation { .. }
        ));
    }

    // -- counters() --

    #[test]
    fn counters_empty_state() {
        let cm = ConnectionManagerState::new();
        let c = cm.counters();
        assert_eq!(c.full_duplex_conns, 0);
        assert_eq!(c.duplex_conns, 0);
        assert_eq!(c.unidirectional_conns, 0);
        assert_eq!(c.inbound_conns, 0);
        assert_eq!(c.outbound_conns, 0);
    }

    #[test]
    fn counters_outbound_unidirectional() {
        let mut cm = ConnectionManagerState::new();
        let _ = cm
            .acquire_outbound_connection(local(), peer(4001))
            .expect("acquire");
        cm.outbound_handshake_done(local(), peer(4001), DataFlow::Unidirectional)
            .expect("handshake");
        let c = cm.counters();
        assert_eq!(c.outbound_conns, 1);
        assert_eq!(c.unidirectional_conns, 1);
        assert_eq!(c.duplex_conns, 0);
        assert_eq!(c.inbound_conns, 0);
    }

    #[test]
    fn counters_outbound_duplex() {
        let mut cm = ConnectionManagerState::new();
        let _ = cm
            .acquire_outbound_connection(local(), peer(4002))
            .expect("acquire");
        cm.outbound_handshake_done(local(), peer(4002), DataFlow::Duplex)
            .expect("handshake");
        let c = cm.counters();
        assert_eq!(c.outbound_conns, 1);
        assert_eq!(c.duplex_conns, 1);
    }

    #[test]
    fn counters_inbound_duplex() {
        let mut cm = ConnectionManagerState::new();
        let cid = ConnectionId {
            local: local(),
            remote: peer(4003),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(4003), DataFlow::Duplex)
            .unwrap();
        let c = cm.counters();
        assert_eq!(c.inbound_conns, 1);
        assert_eq!(c.duplex_conns, 1);
    }

    #[test]
    fn counters_mixed_connections() {
        let mut cm = ConnectionManagerState::new();
        // Outbound unidirectional
        let _ = cm
            .acquire_outbound_connection(local(), peer(4010))
            .expect("acquire");
        cm.outbound_handshake_done(local(), peer(4010), DataFlow::Unidirectional)
            .expect("hs");
        // Outbound duplex
        let _ = cm
            .acquire_outbound_connection(local(), peer(4011))
            .expect("acquire");
        cm.outbound_handshake_done(local(), peer(4011), DataFlow::Duplex)
            .expect("hs");
        // Inbound duplex
        let cid = ConnectionId {
            local: local(),
            remote: peer(4012),
        };
        cm.include_inbound_connection(cid).unwrap();
        cm.inbound_handshake_done(peer(4012), DataFlow::Duplex)
            .unwrap();

        let c = cm.counters();
        assert_eq!(c.outbound_conns, 2);
        assert_eq!(c.inbound_conns, 1);
        assert_eq!(c.unidirectional_conns, 1);
        assert_eq!(c.duplex_conns, 2);
    }
}
