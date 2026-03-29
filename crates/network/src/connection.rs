//! Connection manager types and state machine.
//!
//! This module models the upstream `Ouroboros.Network.ConnectionManager` state
//! machine that tracks per-connection lifecycle from reservation through
//! negotiation, active data transfer, and termination.
//!
//! Reference: `ouroboros-network-framework/src/Ouroboros/Network/ConnectionManager/Types.hs`
//! and `State.hs`.

use std::net::SocketAddr;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Core enums
// ---------------------------------------------------------------------------

/// Whether a connection was initiated locally (outbound) or remotely (inbound).
///
/// Upstream: `Provenance` from `ConnectionManager.Types`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Provenance {
    /// Connection was accepted from a remote peer.
    Inbound,
    /// Connection was initiated by us.
    Outbound,
}

/// Data flow negotiation result for a connection.
///
/// Upstream: `DataFlow` from `ConnectionManager.Types`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DataFlow {
    /// Only one direction of protocol flow (initiator or responder, not both).
    Unidirectional,
    /// Both initiator and responder protocol instances may run simultaneously.
    Duplex,
}

/// Whether a timeout associated with a connection state has expired.
///
/// Upstream: `TimeoutExpired` from `ConnectionManager.Types`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TimeoutExpired {
    /// The timeout has fired.
    Expired,
    /// The timeout is still pending.
    Ticking,
}

// ---------------------------------------------------------------------------
// Connection type (derived)
// ---------------------------------------------------------------------------

/// Derived connection type for a given connection state.
///
/// This collapses the full [`AbstractState`] into a coarser classification
/// useful for counting and policy decisions.
///
/// Upstream: `ConnectionType` from `ConnectionManager.Types`, derived via
/// `connectionType`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ConnectionType {
    /// Connection not yet negotiated. Carries the initiator provenance.
    UnnegotiatedConn(Provenance),
    /// Inbound idle: negotiation done, responder protocols paused. Carries
    /// the negotiated data-flow capability.
    InboundIdleConn(DataFlow),
    /// Outbound idle: local protocols paused. Carries data-flow capability.
    OutboundIdleConn(DataFlow),
    /// Negotiated connection actively running protocols. Carries provenance
    /// and data-flow capability.
    NegotiatedConn(Provenance, DataFlow),
    /// Full-duplex: both local and remote sides running protocols simultaneously.
    DuplexConn,
}

// ---------------------------------------------------------------------------
// Abstract state (tracing-level)
// ---------------------------------------------------------------------------

/// Simplified connection state used for tracing and monitoring.
///
/// This is a projection of the full [`ConnectionState`] that drops runtime
/// handles, leaving only the state-machine label.
///
/// Upstream: `AbstractState` from `ConnectionManager.Types`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AbstractState {
    /// The connection is not tracked by the connection manager.
    UnknownConnectionSt,
    /// An outbound slot has been reserved but no TCP connection yet.
    ReservedOutboundSt,
    /// TCP connected but handshake not yet complete.
    UnnegotiatedSt(Provenance),
    /// Inbound connection idle (responder not running). Carries negotiated
    /// data-flow.
    InboundIdleSt(DataFlow),
    /// Inbound connection with responder actively running.
    InboundSt(DataFlow),
    /// Outbound unidirectional connection with active protocol.
    OutboundUniSt,
    /// Outbound duplex-capable with active protocol. The timeout tracks how
    /// long the remote side has to start its responder before we fall back.
    OutboundDupSt(TimeoutExpired),
    /// Outbound connection idle (local protocols paused).
    OutboundIdleSt(DataFlow),
    /// Full-duplex: both local and remote protocols running.
    DuplexSt,
    /// Waiting for the remote side to go idle before committing.
    WaitRemoteIdleSt,
    /// Connection is being torn down.
    TerminatingSt,
    /// Connection has been fully closed.
    TerminatedSt,
}

impl AbstractState {
    /// Whether this state corresponds to an inbound connection.
    ///
    /// Upstream: `isInboundConn` from `ConnectionManager.State`.
    pub fn is_inbound_conn(&self) -> bool {
        matches!(
            self,
            Self::InboundIdleSt(_) | Self::InboundSt(_) | Self::DuplexSt
        )
    }

    /// Derive the [`ConnectionType`] for this abstract state.
    ///
    /// Returns `None` for states that don't represent a classified connection
    /// type (Unknown, Reserved, WaitRemoteIdle, Terminating, Terminated).
    pub fn connection_type(&self) -> Option<ConnectionType> {
        match *self {
            Self::UnnegotiatedSt(p) => Some(ConnectionType::UnnegotiatedConn(p)),
            Self::InboundIdleSt(df) => Some(ConnectionType::InboundIdleConn(df)),
            Self::OutboundIdleSt(df) => Some(ConnectionType::OutboundIdleConn(df)),
            Self::InboundSt(df) => Some(ConnectionType::NegotiatedConn(Provenance::Inbound, df)),
            Self::OutboundUniSt => Some(ConnectionType::NegotiatedConn(
                Provenance::Outbound,
                DataFlow::Unidirectional,
            )),
            Self::OutboundDupSt(_) => Some(ConnectionType::NegotiatedConn(
                Provenance::Outbound,
                DataFlow::Duplex,
            )),
            Self::DuplexSt => Some(ConnectionType::DuplexConn),
            _ => None,
        }
    }
}

impl std::fmt::Display for AbstractState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownConnectionSt => write!(f, "UnknownConnectionSt"),
            Self::ReservedOutboundSt => write!(f, "ReservedOutboundSt"),
            Self::UnnegotiatedSt(p) => write!(f, "UnnegotiatedSt({p:?})"),
            Self::InboundIdleSt(df) => write!(f, "InboundIdleSt({df:?})"),
            Self::InboundSt(df) => write!(f, "InboundSt({df:?})"),
            Self::OutboundUniSt => write!(f, "OutboundUniSt"),
            Self::OutboundDupSt(te) => write!(f, "OutboundDupSt({te:?})"),
            Self::OutboundIdleSt(df) => write!(f, "OutboundIdleSt({df:?})"),
            Self::DuplexSt => write!(f, "DuplexSt"),
            Self::WaitRemoteIdleSt => write!(f, "WaitRemoteIdleSt"),
            Self::TerminatingSt => write!(f, "TerminatingSt"),
            Self::TerminatedSt => write!(f, "TerminatedSt"),
        }
    }
}

// ---------------------------------------------------------------------------
// Connection state (full runtime state machine)
// ---------------------------------------------------------------------------

/// A unique identifier for a connection-manager entry.
///
/// Upstream: `ConnStateId` from `ConnectionManager.State`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConnStateId(pub u64);

/// Identifies a specific connection by local + remote address pair.
///
/// Upstream: `ConnectionId` from `ConnectionManager.Types`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConnectionId {
    pub local: SocketAddr,
    pub remote: SocketAddr,
}

/// Full connection state tracked by the connection manager.
///
/// Unlike the upstream Haskell, which carries async thread handles and STM
/// variables, the Rust version tracks just the state-machine position and
/// metadata needed for transition validation and counter derivation.
///
/// Upstream: `ConnectionState` from `ConnectionManager.State`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    /// An outbound connection slot has been reserved but not yet connected.
    ReservedOutboundState,
    /// TCP connection established, handshake in progress.
    UnnegotiatedState {
        provenance: Provenance,
        conn_id: ConnectionId,
    },
    /// Outbound unidirectional: handshake completed, running initiator protocols.
    OutboundUniState {
        conn_id: ConnectionId,
    },
    /// Outbound duplex-capable: running initiator protocols, waiting for
    /// remote responder. Timeout tracks whether the remote peer has started
    /// within the allowed window.
    OutboundDupState {
        conn_id: ConnectionId,
        timeout_expired: TimeoutExpired,
    },
    /// Outbound idle: local protocols finished, connection kept alive.
    OutboundIdleState {
        conn_id: ConnectionId,
        data_flow: DataFlow,
    },
    /// Inbound idle: responder not currently running.
    InboundIdleState {
        conn_id: ConnectionId,
        data_flow: DataFlow,
    },
    /// Inbound active: responder running.
    InboundState {
        conn_id: ConnectionId,
        data_flow: DataFlow,
    },
    /// Full-duplex: both local and remote protocols running.
    DuplexState {
        conn_id: ConnectionId,
    },
    /// Connection is being torn down. Optional error records the reason.
    TerminatingState {
        conn_id: ConnectionId,
        error: Option<String>,
    },
    /// Connection fully closed. Optional error records the reason.
    TerminatedState {
        error: Option<String>,
    },
}

impl ConnectionState {
    /// Project this state to the simplified [`AbstractState`].
    ///
    /// Upstream: `abstractState` from `ConnectionManager.State`.
    pub fn abstract_state(&self) -> AbstractState {
        match self {
            Self::ReservedOutboundState => AbstractState::ReservedOutboundSt,
            Self::UnnegotiatedState { provenance, .. } => {
                AbstractState::UnnegotiatedSt(*provenance)
            }
            Self::OutboundUniState { .. } => AbstractState::OutboundUniSt,
            Self::OutboundDupState {
                timeout_expired, ..
            } => AbstractState::OutboundDupSt(*timeout_expired),
            Self::OutboundIdleState { data_flow, .. } => {
                AbstractState::OutboundIdleSt(*data_flow)
            }
            Self::InboundIdleState { data_flow, .. } => {
                AbstractState::InboundIdleSt(*data_flow)
            }
            Self::InboundState { data_flow, .. } => AbstractState::InboundSt(*data_flow),
            Self::DuplexState { .. } => AbstractState::DuplexSt,
            Self::TerminatingState { .. } => AbstractState::TerminatingSt,
            Self::TerminatedState { .. } => AbstractState::TerminatedSt,
        }
    }

    /// Extract the [`ConnectionId`] if the state carries one.
    pub fn conn_id(&self) -> Option<ConnectionId> {
        match self {
            Self::ReservedOutboundState | Self::TerminatedState { .. } => None,
            Self::UnnegotiatedState { conn_id, .. }
            | Self::OutboundUniState { conn_id }
            | Self::OutboundDupState { conn_id, .. }
            | Self::OutboundIdleState { conn_id, .. }
            | Self::InboundIdleState { conn_id, .. }
            | Self::InboundState { conn_id, .. }
            | Self::DuplexState { conn_id }
            | Self::TerminatingState { conn_id, .. } => Some(*conn_id),
        }
    }
}

// ---------------------------------------------------------------------------
// State → counter mapping
// ---------------------------------------------------------------------------

/// Derive [`super::governor::ConnectionManagerCounters`] for a single
/// connection state.
///
/// This is the Rust equivalent of the upstream `connectionStateToCounters`
/// function in `ConnectionManager.Core`, which maps each state to a
/// unit-counter struct that is then summed across all connections.
///
/// Upstream: `connectionStateToCounters` from `ConnectionManager.Core`.
pub fn connection_state_to_counters(state: &ConnectionState) -> super::governor::ConnectionManagerCounters {
    use super::governor::ConnectionManagerCounters;

    match state {
        ConnectionState::ReservedOutboundState => ConnectionManagerCounters::default(),
        ConnectionState::UnnegotiatedState {
            provenance: Provenance::Inbound,
            ..
        } => ConnectionManagerCounters {
            inbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::UnnegotiatedState {
            provenance: Provenance::Outbound,
            ..
        } => ConnectionManagerCounters {
            outbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::OutboundUniState { .. } => ConnectionManagerCounters {
            unidirectional_conns: 1,
            outbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::OutboundDupState { .. } => ConnectionManagerCounters {
            duplex_conns: 1,
            outbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::OutboundIdleState {
            data_flow: DataFlow::Unidirectional,
            ..
        } => ConnectionManagerCounters {
            unidirectional_conns: 1,
            outbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::OutboundIdleState {
            data_flow: DataFlow::Duplex,
            ..
        } => ConnectionManagerCounters {
            duplex_conns: 1,
            outbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::InboundIdleState {
            data_flow: DataFlow::Unidirectional,
            ..
        } => ConnectionManagerCounters {
            unidirectional_conns: 1,
            inbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::InboundIdleState {
            data_flow: DataFlow::Duplex,
            ..
        } => ConnectionManagerCounters {
            duplex_conns: 1,
            inbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::InboundState {
            data_flow: DataFlow::Unidirectional,
            ..
        } => ConnectionManagerCounters {
            unidirectional_conns: 1,
            inbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::InboundState {
            data_flow: DataFlow::Duplex,
            ..
        } => ConnectionManagerCounters {
            duplex_conns: 1,
            inbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::DuplexState { .. } => ConnectionManagerCounters {
            full_duplex_conns: 1,
            duplex_conns: 1,
            inbound_conns: 1,
            outbound_conns: 1,
            ..Default::default()
        },
        ConnectionState::TerminatingState { .. } => ConnectionManagerCounters {
            terminating_conns: 1,
            ..Default::default()
        },
        ConnectionState::TerminatedState { .. } => ConnectionManagerCounters::default(),
    }
}

// ---------------------------------------------------------------------------
// Transition types
// ---------------------------------------------------------------------------

/// A state transition record from one [`AbstractState`] to another.
///
/// Upstream: `Transition` from `ConnectionManager.Types`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Transition {
    pub from_state: AbstractState,
    pub to_state: AbstractState,
}

/// Wraps a state that may or may not be known (e.g. due to a race condition).
///
/// Upstream: `MaybeUnknown` from `ConnectionManager.Types`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaybeUnknown<S> {
    /// State is known.
    Known(S),
    /// State is known but obtained during a race (may be stale).
    Race(S),
    /// State is not known (peer not in connection map).
    Unknown,
}

impl<S> MaybeUnknown<S> {
    /// Extract the inner state regardless of whether it was obtained
    /// cleanly or during a race.
    pub fn state(&self) -> Option<&S> {
        match self {
            Self::Known(s) | Self::Race(s) => Some(s),
            Self::Unknown => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Operation result
// ---------------------------------------------------------------------------

/// Outcome of a connection manager operation (e.g. promote, demote).
///
/// Upstream: `OperationResult` from `ConnectionManager.Types`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationResult<A> {
    /// The connection was in a state that does not support the requested
    /// operation.
    UnsupportedState(AbstractState),
    /// The operation completed successfully with the given value.
    OperationSuccess(A),
    /// The connection has terminated before the operation could complete.
    TerminatedConnection(AbstractState),
}

/// Outcome of a `DemotedToCold^{Remote}` transition.
///
/// Upstream: `DemotedToColdRemoteTr` from `ConnectionManager.Types`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemotedToColdRemoteTr {
    /// The connection should be committed (terminated) — `Commit^{dataFlow}`
    /// from `InboundIdleState`.
    CommitTr,
    /// The connection should be kept alive — the outbound side still uses it,
    /// or a level-triggered `Awake^{Duplex}_{Local}`.
    KeepTr,
}

// ---------------------------------------------------------------------------
// Connection limits
// ---------------------------------------------------------------------------

/// Accepted-connection rate-limiting parameters.
///
/// * Below `soft_limit`: accept immediately.
/// * Between `soft_limit` and `hard_limit`: linearly increasing delay up to
///   `delay`.
/// * At or above `hard_limit`: block until the count drops, then wait `delay`.
///
/// Upstream: `AcceptedConnectionsLimit` from `Network.Server.RateLimiting`.
/// Default values from upstream `Configuration.hs`:
/// `{ hard_limit: 512, soft_limit: 384, delay: 5s }`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcceptedConnectionsLimit {
    /// Maximum number of concurrent accepted connections.
    pub hard_limit: u32,
    /// Threshold above which rate-limiting delay starts.
    pub soft_limit: u32,
    /// Maximum delay applied when at the hard limit.
    pub delay: Duration,
}

impl Default for AcceptedConnectionsLimit {
    fn default() -> Self {
        Self {
            hard_limit: 512,
            soft_limit: 384,
            delay: Duration::from_secs(5),
        }
    }
}

impl AcceptedConnectionsLimit {
    /// Compute the accept delay for a given number of current connections.
    ///
    /// Returns `None` if the hard limit is reached (caller must wait for a
    /// connection to close). Returns `Some(Duration)` for the rate-limiting
    /// delay (zero below soft limit).
    pub fn accept_delay(&self, current: u32) -> Option<Duration> {
        if current >= self.hard_limit {
            return None; // hard-limited — must wait
        }
        if current <= self.soft_limit || self.hard_limit == self.soft_limit {
            return Some(Duration::ZERO);
        }
        // Linear interpolation between soft_limit and hard_limit.
        let range = self.hard_limit - self.soft_limit;
        let over = current - self.soft_limit;
        let fraction = over as f64 / range as f64;
        let delay_ms = (self.delay.as_millis() as f64 * fraction) as u64;
        Some(Duration::from_millis(delay_ms))
    }
}

// ---------------------------------------------------------------------------
// Connection manager errors
// ---------------------------------------------------------------------------

/// Errors that can occur during connection manager operations.
///
/// Upstream: `ConnectionManagerError` from `ConnectionManager.Types`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionManagerError {
    /// Attempted to create a connection that already exists.
    ConnectionExists {
        provenance: Provenance,
        peer: SocketAddr,
    },
    /// The requested connection is forbidden (e.g. policy violation).
    ForbiddenConnection(ConnectionId),
    /// Inbound connection referenced but not found.
    InboundConnectionNotFound(SocketAddr),
    /// State machine reached an impossible state.
    ImpossibleConnection(ConnectionId),
    /// Connection is in the process of terminating.
    ConnectionTerminating(ConnectionId),
    /// Connection has already terminated.
    ConnectionTerminated(ConnectionId),
    /// Internal state machine invariant violation.
    ImpossibleState(SocketAddr),
    /// The requested operation is not valid in the current state.
    ForbiddenOperation {
        peer: SocketAddr,
        state: AbstractState,
    },
    /// Peer address is not known to the connection manager.
    UnknownPeer(SocketAddr),
}

impl std::fmt::Display for ConnectionManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionExists { provenance, peer } => {
                write!(f, "connection already exists ({provenance:?}) for {peer}")
            }
            Self::ForbiddenConnection(cid) => {
                write!(f, "forbidden connection {}→{}", cid.local, cid.remote)
            }
            Self::InboundConnectionNotFound(addr) => {
                write!(f, "inbound connection not found for {addr}")
            }
            Self::ImpossibleConnection(cid) => {
                write!(f, "impossible connection state {}→{}", cid.local, cid.remote)
            }
            Self::ConnectionTerminating(cid) => {
                write!(f, "connection terminating {}→{}", cid.local, cid.remote)
            }
            Self::ConnectionTerminated(cid) => {
                write!(f, "connection terminated {}→{}", cid.local, cid.remote)
            }
            Self::ImpossibleState(addr) => {
                write!(f, "impossible state for {addr}")
            }
            Self::ForbiddenOperation { peer, state } => {
                write!(f, "forbidden operation for {peer} in state {state}")
            }
            Self::UnknownPeer(addr) => {
                write!(f, "unknown peer {addr}")
            }
        }
    }
}

impl std::error::Error for ConnectionManagerError {}

// ---------------------------------------------------------------------------
// Default timeouts
// ---------------------------------------------------------------------------

/// Default connection manager timeout constants.
///
/// Upstream: `ConnectionManager.Core` and `Diffusion.Configuration`.
pub mod timeouts {
    use std::time::Duration;

    /// TCP TIME_WAIT-equivalent: time in `TerminatingSt` before moving
    /// to `TerminatedSt`.
    ///
    /// Upstream: `defaultTimeWaitTimeout = 60s`.
    pub const TIME_WAIT_TIMEOUT: Duration = Duration::from_secs(60);

    /// Idle timeout for inbound connections before reset.
    ///
    /// Upstream: `defaultProtocolIdleTimeout = 5s`.
    pub const PROTOCOL_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

    /// Timeout for an outbound-idle connection before close.
    ///
    /// Upstream: `defaultResetTimeout = 5s`.
    pub const RESET_TIMEOUT: Duration = Duration::from_secs(5);

    /// Maximum time to receive a single SDU (guards against very slow peers;
    /// ~17 kbps minimum throughput).
    ///
    /// Upstream: `sduTimeout = 30s`.
    pub const SDU_TIMEOUT: Duration = Duration::from_secs(30);

    /// SDU timeout during the handshake phase (tighter than normal).
    ///
    /// Upstream: `sduHandshakeTimeout = 10s`.
    pub const SDU_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

    /// Node-to-client local connection idle timeout.
    ///
    /// Upstream: `local_PROTOCOL_IDLE_TIMEOUT = 2s`.
    pub const LOCAL_PROTOCOL_IDLE_TIMEOUT: Duration = Duration::from_secs(2);

    /// Node-to-client TIME_WAIT timeout (immediate for local connections).
    ///
    /// Upstream: `local_TIME_WAIT_TIMEOUT = 0s`.
    pub const LOCAL_TIME_WAIT_TIMEOUT: Duration = Duration::ZERO;

    /// Per-protocol message receive deadline for N2N server-side drivers.
    ///
    /// Applied when a server driver is waiting for the next client message
    /// (client has protocol agency).  Corresponds to upstream `shortWait`
    /// in `ouroboros-network-protocols` (60 s).  The TxSubmission blocking
    /// request is the only exception and has no deadline (`waitForever`).
    pub const PROTOCOL_RECV_TIMEOUT: Duration = Duration::from_secs(60);
}

// ---------------------------------------------------------------------------
// Abstract transition validation
// ---------------------------------------------------------------------------

/// Checks whether a transition between two [`AbstractState`]s is valid
/// according to the upstream connection manager state machine rules.
///
/// Returns `true` if the `from → to` transition is an allowed edge in the
/// state diagram, `false` otherwise.
///
/// Upstream: derived from `verifyAbstractTransition` in
/// `Test.Ouroboros.Network.ConnectionManager.Utils`.
pub fn verify_abstract_transition(from: AbstractState, to: AbstractState) -> bool {
    use AbstractState::*;
    use DataFlow::*;
    use Provenance::*;
    use TimeoutExpired::*;

    matches!(
        (from, to),
        // -- Outbound path --
        (TerminatedSt | UnknownConnectionSt, ReservedOutboundSt)
            | (ReservedOutboundSt, UnnegotiatedSt(Outbound))
            | (UnnegotiatedSt(Outbound), OutboundUniSt)
            | (UnnegotiatedSt(Outbound), OutboundDupSt(Ticking))
            | (OutboundUniSt, OutboundIdleSt(Unidirectional))
            | (OutboundDupSt(Ticking), OutboundDupSt(Expired))
            | (OutboundDupSt(Expired), OutboundIdleSt(Duplex))
            | (OutboundDupSt(Ticking), DuplexSt)
            | (OutboundDupSt(Expired), DuplexSt)
            | (OutboundIdleSt(_), TerminatingSt)
            // -- Inbound path --
            | (TerminatedSt | UnknownConnectionSt, UnnegotiatedSt(Inbound))
            | (ReservedOutboundSt, UnnegotiatedSt(Inbound)) // overwritten
            | (UnnegotiatedSt(Inbound), InboundIdleSt(Duplex))
            | (UnnegotiatedSt(Inbound), InboundIdleSt(Unidirectional))
            | (InboundIdleSt(Duplex), InboundSt(Duplex))
            | (InboundIdleSt(Unidirectional), InboundSt(Unidirectional))
            | (InboundSt(Duplex), InboundIdleSt(Duplex))
            | (InboundSt(Unidirectional), InboundIdleSt(Unidirectional))
            | (InboundIdleSt(_), TerminatingSt) // Commit
            | (InboundSt(Duplex), DuplexSt) // PromotedToWarm^{Duplex}_{Local}
            // -- Duplex transitions --
            | (DuplexSt, OutboundDupSt(Ticking)) // DemotedToCold^{Duplex}_{Remote}
            | (DuplexSt, InboundSt(Duplex)) // DemotedToCold^{Duplex}_{Local}
            // -- Self-connect / overwrite races --
            | (InboundIdleSt(Duplex), OutboundDupSt(Ticking))
            | (InboundIdleSt(Unidirectional), OutboundUniSt)
            | (UnnegotiatedSt(Outbound), UnnegotiatedSt(Inbound))
            | (UnnegotiatedSt(Inbound), UnnegotiatedSt(Outbound))
            // -- Terminal transitions --
            | (TerminatingSt, TerminatedSt)
            | (TerminatingSt, UnnegotiatedSt(Inbound)) // reuse during TIME_WAIT
    )
    // Any state can transition to TerminatedSt or UnknownConnectionSt on
    // error/cleanup, but those are not validated here — they are handled
    // as exceptional paths.
}

// ---------------------------------------------------------------------------
// Inbound governor types
// ---------------------------------------------------------------------------

/// Remote peer state as tracked by the inbound governor.
///
/// Upstream: `RemoteSt` from `InboundGovernor.State` (tracing-level).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RemoteSt {
    /// Remote peer's responder is warm (established but not actively sending).
    RemoteWarmSt,
    /// Remote peer's responder is hot (actively transferring data).
    RemoteHotSt,
    /// Remote peer's responder is idle (no mini-protocols running).
    RemoteIdleSt,
    /// Remote peer has been committed to cold.
    RemoteColdSt,
}

impl RemoteSt {
    /// Whether this state is "established" (Warm or Hot).
    ///
    /// Upstream: pattern synonym `RemoteEstablished` matches `RemoteWarm | RemoteHot`.
    pub fn is_established(&self) -> bool {
        matches!(self, Self::RemoteWarmSt | Self::RemoteHotSt)
    }
}

/// Counters for inbound governor state tracking.
///
/// Upstream: `Counters` from `InboundGovernor.State`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InboundGovernorCounters {
    /// Number of inbound peers in cold/committed state.
    pub cold_peers_remote: usize,
    /// Number of inbound peers in idle state.
    pub idle_peers_remote: usize,
    /// Number of inbound peers in warm state.
    pub warm_peers_remote: usize,
    /// Number of inbound peers in hot state.
    pub hot_peers_remote: usize,
}

impl InboundGovernorCounters {
    /// Increment the counter for a single remote peer state.
    pub fn count_state(&mut self, state: RemoteSt) {
        match state {
            RemoteSt::RemoteColdSt => self.cold_peers_remote += 1,
            RemoteSt::RemoteIdleSt => self.idle_peers_remote += 1,
            RemoteSt::RemoteWarmSt => self.warm_peers_remote += 1,
            RemoteSt::RemoteHotSt => self.hot_peers_remote += 1,
        }
    }

    /// Total number of inbound peers tracked.
    pub fn total(&self) -> usize {
        self.cold_peers_remote
            + self.idle_peers_remote
            + self.warm_peers_remote
            + self.hot_peers_remote
    }
}

impl std::ops::Add for InboundGovernorCounters {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            cold_peers_remote: self.cold_peers_remote + rhs.cold_peers_remote,
            idle_peers_remote: self.idle_peers_remote + rhs.idle_peers_remote,
            warm_peers_remote: self.warm_peers_remote + rhs.warm_peers_remote,
            hot_peers_remote: self.hot_peers_remote + rhs.hot_peers_remote,
        }
    }
}

/// Counters for mini-protocol responder instances on a single connection.
///
/// Upstream: `ResponderCounters` from `InboundGovernor.State`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResponderCounters {
    /// Number of responders in hot state.
    pub hot_responders: usize,
    /// Number of responders in non-hot state (warm, idle, etc.).
    pub non_hot_responders: usize,
}

/// Inbound governor event representing an observable state change.
///
/// Upstream: `Event` in `InboundGovernor.hs`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InboundGovernorEvent {
    /// A new inbound connection has been accepted and negotiated.
    NewConnection(ConnectionId),
    /// The mux for a connection has finished.
    MuxFinished(ConnectionId),
    /// A mini-protocol instance on a connection has terminated.
    MiniProtocolTerminated(ConnectionId),
    /// Remote side has gone idle (all responders stopped).
    WaitIdleRemote(ConnectionId),
    /// Remote side has become active again.
    AwakeRemote(ConnectionId),
    /// Remote side promoted to hot.
    RemotePromotedToHot(ConnectionId),
    /// Remote side demoted to warm.
    RemoteDemotedToWarm(ConnectionId),
    /// Remote side committed (connection to be closed or kept).
    CommitRemote(ConnectionId),
    /// One or more duplex peers have matured (been connected for ≥15 min).
    MaturedDuplexPeers,
    /// Periodic inactivity timeout fired (~31s).
    InactivityTimeout,
}

/// Inbound governor constants.
///
/// Upstream: `inboundMaturePeerDelay` and `inactionTimeout` from
/// `InboundGovernor.hs`.
pub mod inbound_constants {
    use std::time::Duration;

    /// Time before a duplex inbound peer is considered "mature".
    ///
    /// Upstream: `inboundMaturePeerDelay = 900s` (15 minutes).
    pub const MATURE_PEER_DELAY: Duration = Duration::from_secs(900);

    /// Periodic wakeup interval for the inbound governor event loop.
    ///
    /// Upstream: `inactionTimeout ≈ 31.4s` (π * 10).
    pub const INACTION_TIMEOUT: Duration = Duration::from_millis(31_416);
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

    // -- Provenance / DataFlow / TimeoutExpired --

    #[test]
    fn provenance_equality() {
        assert_eq!(Provenance::Inbound, Provenance::Inbound);
        assert_ne!(Provenance::Inbound, Provenance::Outbound);
    }

    #[test]
    fn data_flow_equality() {
        assert_eq!(DataFlow::Duplex, DataFlow::Duplex);
        assert_ne!(DataFlow::Duplex, DataFlow::Unidirectional);
    }

    #[test]
    fn timeout_expired_equality() {
        assert_eq!(TimeoutExpired::Ticking, TimeoutExpired::Ticking);
        assert_ne!(TimeoutExpired::Ticking, TimeoutExpired::Expired);
    }

    // -- AbstractState --

    #[test]
    fn abstract_state_is_inbound() {
        assert!(AbstractState::InboundIdleSt(DataFlow::Duplex).is_inbound_conn());
        assert!(AbstractState::InboundSt(DataFlow::Unidirectional).is_inbound_conn());
        assert!(AbstractState::DuplexSt.is_inbound_conn());
        assert!(!AbstractState::OutboundUniSt.is_inbound_conn());
        assert!(!AbstractState::ReservedOutboundSt.is_inbound_conn());
        assert!(!AbstractState::TerminatedSt.is_inbound_conn());
    }

    #[test]
    fn abstract_state_connection_type() {
        assert_eq!(
            AbstractState::UnnegotiatedSt(Provenance::Outbound).connection_type(),
            Some(ConnectionType::UnnegotiatedConn(Provenance::Outbound))
        );
        assert_eq!(
            AbstractState::DuplexSt.connection_type(),
            Some(ConnectionType::DuplexConn)
        );
        assert_eq!(
            AbstractState::OutboundDupSt(TimeoutExpired::Ticking).connection_type(),
            Some(ConnectionType::NegotiatedConn(Provenance::Outbound, DataFlow::Duplex))
        );
        assert_eq!(AbstractState::ReservedOutboundSt.connection_type(), None);
        assert_eq!(AbstractState::TerminatedSt.connection_type(), None);
    }

    #[test]
    fn abstract_state_display() {
        assert_eq!(format!("{}", AbstractState::DuplexSt), "DuplexSt");
        assert_eq!(
            format!("{}", AbstractState::UnnegotiatedSt(Provenance::Inbound)),
            "UnnegotiatedSt(Inbound)"
        );
    }

    // -- ConnectionState → AbstractState --

    #[test]
    fn connection_state_abstract_reserved() {
        let s = ConnectionState::ReservedOutboundState;
        assert_eq!(s.abstract_state(), AbstractState::ReservedOutboundSt);
    }

    #[test]
    fn connection_state_abstract_unnegotiated() {
        let s = ConnectionState::UnnegotiatedState {
            provenance: Provenance::Inbound,
            conn_id: conn_id(1000, 2000),
        };
        assert_eq!(
            s.abstract_state(),
            AbstractState::UnnegotiatedSt(Provenance::Inbound)
        );
    }

    #[test]
    fn connection_state_abstract_duplex() {
        let s = ConnectionState::DuplexState {
            conn_id: conn_id(1000, 2000),
        };
        assert_eq!(s.abstract_state(), AbstractState::DuplexSt);
    }

    #[test]
    fn connection_state_abstract_outbound_dup() {
        let s = ConnectionState::OutboundDupState {
            conn_id: conn_id(1000, 2000),
            timeout_expired: TimeoutExpired::Expired,
        };
        assert_eq!(
            s.abstract_state(),
            AbstractState::OutboundDupSt(TimeoutExpired::Expired)
        );
    }

    #[test]
    fn connection_state_abstract_terminating() {
        let s = ConnectionState::TerminatingState {
            conn_id: conn_id(1000, 2000),
            error: Some("test".into()),
        };
        assert_eq!(s.abstract_state(), AbstractState::TerminatingSt);
    }

    #[test]
    fn connection_state_abstract_terminated() {
        let s = ConnectionState::TerminatedState { error: None };
        assert_eq!(s.abstract_state(), AbstractState::TerminatedSt);
    }

    #[test]
    fn connection_state_conn_id() {
        let cid = conn_id(1000, 2000);
        assert_eq!(ConnectionState::ReservedOutboundState.conn_id(), None);
        assert_eq!(
            ConnectionState::TerminatedState { error: None }.conn_id(),
            None
        );
        assert_eq!(
            ConnectionState::DuplexState { conn_id: cid }.conn_id(),
            Some(cid)
        );
        assert_eq!(
            ConnectionState::InboundState {
                conn_id: cid,
                data_flow: DataFlow::Duplex
            }
            .conn_id(),
            Some(cid)
        );
    }

    // -- connection_state_to_counters --

    #[test]
    fn counters_reserved_is_zero() {
        let c = connection_state_to_counters(&ConnectionState::ReservedOutboundState);
        assert_eq!(c, super::super::governor::ConnectionManagerCounters::default());
    }

    #[test]
    fn counters_inbound_unnegotiated() {
        let c = connection_state_to_counters(&ConnectionState::UnnegotiatedState {
            provenance: Provenance::Inbound,
            conn_id: conn_id(1000, 2000),
        });
        assert_eq!(c.inbound_conns, 1);
        assert_eq!(c.outbound_conns, 0);
        assert_eq!(c.duplex_conns, 0);
    }

    #[test]
    fn counters_outbound_uni() {
        let c = connection_state_to_counters(&ConnectionState::OutboundUniState {
            conn_id: conn_id(1000, 2000),
        });
        assert_eq!(c.outbound_conns, 1);
        assert_eq!(c.unidirectional_conns, 1);
        assert_eq!(c.duplex_conns, 0);
    }

    #[test]
    fn counters_outbound_dup() {
        let c = connection_state_to_counters(&ConnectionState::OutboundDupState {
            conn_id: conn_id(1000, 2000),
            timeout_expired: TimeoutExpired::Ticking,
        });
        assert_eq!(c.outbound_conns, 1);
        assert_eq!(c.duplex_conns, 1);
        assert_eq!(c.full_duplex_conns, 0);
    }

    #[test]
    fn counters_full_duplex() {
        let c = connection_state_to_counters(&ConnectionState::DuplexState {
            conn_id: conn_id(1000, 2000),
        });
        assert_eq!(c.full_duplex_conns, 1);
        assert_eq!(c.duplex_conns, 1);
        assert_eq!(c.inbound_conns, 1);
        assert_eq!(c.outbound_conns, 1);
        assert_eq!(c.unidirectional_conns, 0);
        assert_eq!(c.terminating_conns, 0);
    }

    #[test]
    fn counters_terminated_is_zero() {
        let c = connection_state_to_counters(&ConnectionState::TerminatedState { error: None });
        assert_eq!(c, super::super::governor::ConnectionManagerCounters::default());
    }

    #[test]
    fn counters_terminating() {
        let c = connection_state_to_counters(&ConnectionState::TerminatingState {
            conn_id: conn_id(1000, 2000),
            error: None,
        });
        assert_eq!(c.terminating_conns, 1);
        assert_eq!(c.outbound_conns, 0);
    }

    #[test]
    fn counters_inbound_idle_duplex() {
        let c = connection_state_to_counters(&ConnectionState::InboundIdleState {
            conn_id: conn_id(1000, 2000),
            data_flow: DataFlow::Duplex,
        });
        assert_eq!(c.inbound_conns, 1);
        assert_eq!(c.duplex_conns, 1);
        assert_eq!(c.unidirectional_conns, 0);
    }

    #[test]
    fn counters_inbound_state_uni() {
        let c = connection_state_to_counters(&ConnectionState::InboundState {
            conn_id: conn_id(1000, 2000),
            data_flow: DataFlow::Unidirectional,
        });
        assert_eq!(c.inbound_conns, 1);
        assert_eq!(c.unidirectional_conns, 1);
        assert_eq!(c.duplex_conns, 0);
    }

    #[test]
    fn counters_outbound_idle_uni() {
        let c = connection_state_to_counters(&ConnectionState::OutboundIdleState {
            conn_id: conn_id(1000, 2000),
            data_flow: DataFlow::Unidirectional,
        });
        assert_eq!(c.outbound_conns, 1);
        assert_eq!(c.unidirectional_conns, 1);
        assert_eq!(c.duplex_conns, 0);
    }

    #[test]
    fn counters_outbound_idle_dup() {
        let c = connection_state_to_counters(&ConnectionState::OutboundIdleState {
            conn_id: conn_id(1000, 2000),
            data_flow: DataFlow::Duplex,
        });
        assert_eq!(c.outbound_conns, 1);
        assert_eq!(c.duplex_conns, 1);
        assert_eq!(c.unidirectional_conns, 0);
    }

    // -- AcceptedConnectionsLimit --

    #[test]
    fn accepted_connections_limit_defaults() {
        let l = AcceptedConnectionsLimit::default();
        assert_eq!(l.hard_limit, 512);
        assert_eq!(l.soft_limit, 384);
        assert_eq!(l.delay, Duration::from_secs(5));
    }

    #[test]
    fn accept_delay_below_soft() {
        let l = AcceptedConnectionsLimit::default();
        assert_eq!(l.accept_delay(100), Some(Duration::ZERO));
        assert_eq!(l.accept_delay(384), Some(Duration::ZERO));
    }

    #[test]
    fn accept_delay_between_soft_and_hard() {
        let l = AcceptedConnectionsLimit::default();
        // 448 = midpoint between 384 and 512
        let delay = l.accept_delay(448).unwrap();
        assert!(delay > Duration::ZERO);
        assert!(delay < l.delay);
    }

    #[test]
    fn accept_delay_at_hard_limit() {
        let l = AcceptedConnectionsLimit::default();
        assert_eq!(l.accept_delay(512), None);
        assert_eq!(l.accept_delay(600), None);
    }

    // -- MaybeUnknown --

    #[test]
    fn maybe_unknown_state() {
        let known = MaybeUnknown::Known(42);
        let race = MaybeUnknown::Race(42);
        let unknown: MaybeUnknown<i32> = MaybeUnknown::Unknown;

        assert_eq!(known.state(), Some(&42));
        assert_eq!(race.state(), Some(&42));
        assert_eq!(unknown.state(), None);
    }

    // -- OperationResult --

    #[test]
    fn operation_result_variants() {
        let success: OperationResult<u32> = OperationResult::OperationSuccess(42);
        assert_eq!(success, OperationResult::OperationSuccess(42));

        let unsupported: OperationResult<u32> =
            OperationResult::UnsupportedState(AbstractState::TerminatedSt);
        assert_eq!(
            unsupported,
            OperationResult::UnsupportedState(AbstractState::TerminatedSt)
        );
    }

    // -- DemotedToColdRemoteTr --

    #[test]
    fn demoted_to_cold_remote_tr() {
        assert_ne!(DemotedToColdRemoteTr::CommitTr, DemotedToColdRemoteTr::KeepTr);
    }

    // -- Transition --

    #[test]
    fn transition_from_to() {
        let t = Transition {
            from_state: AbstractState::ReservedOutboundSt,
            to_state: AbstractState::UnnegotiatedSt(Provenance::Outbound),
        };
        assert_eq!(t.from_state, AbstractState::ReservedOutboundSt);
        assert_eq!(
            t.to_state,
            AbstractState::UnnegotiatedSt(Provenance::Outbound)
        );
    }

    // -- verify_abstract_transition --

    #[test]
    fn valid_outbound_path() {
        use AbstractState::*;
        use DataFlow::*;
        use Provenance::*;
        use TimeoutExpired::*;

        // Unknown → Reserved
        assert!(verify_abstract_transition(UnknownConnectionSt, ReservedOutboundSt));
        // Reserved → Unnegotiated(Outbound)
        assert!(verify_abstract_transition(ReservedOutboundSt, UnnegotiatedSt(Outbound)));
        // Unnegotiated(Outbound) → OutboundUni
        assert!(verify_abstract_transition(UnnegotiatedSt(Outbound), OutboundUniSt));
        // Unnegotiated(Outbound) → OutboundDup(Ticking)
        assert!(verify_abstract_transition(
            UnnegotiatedSt(Outbound),
            OutboundDupSt(Ticking)
        ));
        // OutboundDup(Ticking) → OutboundDup(Expired)
        assert!(verify_abstract_transition(
            OutboundDupSt(Ticking),
            OutboundDupSt(Expired)
        ));
        // OutboundDup(Expired) → OutboundIdle(Duplex)
        assert!(verify_abstract_transition(OutboundDupSt(Expired), OutboundIdleSt(Duplex)));
        // OutboundUni → OutboundIdle(Uni)
        assert!(verify_abstract_transition(OutboundUniSt, OutboundIdleSt(Unidirectional)));
        // OutboundIdle → Terminating
        assert!(verify_abstract_transition(OutboundIdleSt(Unidirectional), TerminatingSt));
        assert!(verify_abstract_transition(OutboundIdleSt(Duplex), TerminatingSt));
        // Terminating → Terminated
        assert!(verify_abstract_transition(TerminatingSt, TerminatedSt));
    }

    #[test]
    fn valid_inbound_path() {
        use AbstractState::*;
        use DataFlow::*;
        use Provenance::*;

        // Unknown → Unnegotiated(Inbound)
        assert!(verify_abstract_transition(
            UnknownConnectionSt,
            UnnegotiatedSt(Inbound)
        ));
        // Unnegotiated(Inbound) → InboundIdle(Duplex)
        assert!(verify_abstract_transition(
            UnnegotiatedSt(Inbound),
            InboundIdleSt(Duplex)
        ));
        // InboundIdle(Duplex) → InboundSt(Duplex)
        assert!(verify_abstract_transition(
            InboundIdleSt(Duplex),
            InboundSt(Duplex)
        ));
        // InboundSt(Duplex) → InboundIdle(Duplex)
        assert!(verify_abstract_transition(
            InboundSt(Duplex),
            InboundIdleSt(Duplex)
        ));
        // InboundIdle → Terminating (Commit)
        assert!(verify_abstract_transition(InboundIdleSt(Unidirectional), TerminatingSt));
    }

    #[test]
    fn valid_duplex_transitions() {
        use AbstractState::*;
        use DataFlow::*;
        use TimeoutExpired::*;

        // OutboundDup → Duplex (remote promoted to warm)
        assert!(verify_abstract_transition(OutboundDupSt(Ticking), DuplexSt));
        assert!(verify_abstract_transition(OutboundDupSt(Expired), DuplexSt));
        // InboundSt(Duplex) → Duplex (local promoted to warm)
        assert!(verify_abstract_transition(InboundSt(Duplex), DuplexSt));
        // Duplex → OutboundDup(Ticking) (remote demoted to cold)
        assert!(verify_abstract_transition(DuplexSt, OutboundDupSt(Ticking)));
        // Duplex → InboundSt(Duplex) (local demoted to cold)
        assert!(verify_abstract_transition(DuplexSt, InboundSt(Duplex)));
    }

    #[test]
    fn invalid_transitions() {
        use AbstractState::*;
        use DataFlow::*;
        use TimeoutExpired::*;

        // Cannot go from Reserved directly to OutboundUni (must negotiate first)
        assert!(!verify_abstract_transition(ReservedOutboundSt, OutboundUniSt));
        // Cannot go from OutboundUni to Duplex directly
        assert!(!verify_abstract_transition(OutboundUniSt, DuplexSt));
        // Cannot go from Terminated back to Duplex
        assert!(!verify_abstract_transition(TerminatedSt, DuplexSt));
        // Cannot go from InboundIdle(Uni) to InboundSt(Duplex)
        assert!(!verify_abstract_transition(
            InboundIdleSt(Unidirectional),
            InboundSt(Duplex)
        ));
        // Cannot go from OutboundDup(Ticking) → OutboundIdle (must expire first)
        assert!(!verify_abstract_transition(
            OutboundDupSt(Ticking),
            OutboundIdleSt(Duplex)
        ));
    }

    #[test]
    fn self_connect_overwrite_transitions() {
        use AbstractState::*;
        use DataFlow::*;
        use Provenance::*;
        use TimeoutExpired::*;

        // InboundIdle(Duplex) → OutboundDup(Ticking) — self-connect
        assert!(verify_abstract_transition(
            InboundIdleSt(Duplex),
            OutboundDupSt(Ticking)
        ));
        // InboundIdle(Uni) → OutboundUni — self-connect
        assert!(verify_abstract_transition(
            InboundIdleSt(Unidirectional),
            OutboundUniSt
        ));
        // Unnegotiated races
        assert!(verify_abstract_transition(
            UnnegotiatedSt(Outbound),
            UnnegotiatedSt(Inbound)
        ));
        assert!(verify_abstract_transition(
            UnnegotiatedSt(Inbound),
            UnnegotiatedSt(Outbound)
        ));
    }

    #[test]
    fn terminating_to_inbound_reuse() {
        // Reuse during TIME_WAIT
        assert!(verify_abstract_transition(
            AbstractState::TerminatingSt,
            AbstractState::UnnegotiatedSt(Provenance::Inbound)
        ));
    }

    // -- ConnectionManagerError Display --

    #[test]
    fn connection_manager_error_display() {
        let e = ConnectionManagerError::ConnectionExists {
            provenance: Provenance::Outbound,
            peer: addr(3000),
        };
        let s = format!("{e}");
        assert!(s.contains("already exists"));
        assert!(s.contains("Outbound"));
    }

    // -- RemoteSt --

    #[test]
    fn remote_st_is_established() {
        assert!(RemoteSt::RemoteWarmSt.is_established());
        assert!(RemoteSt::RemoteHotSt.is_established());
        assert!(!RemoteSt::RemoteIdleSt.is_established());
        assert!(!RemoteSt::RemoteColdSt.is_established());
    }

    // -- InboundGovernorCounters --

    #[test]
    fn inbound_governor_counters_empty() {
        let c = InboundGovernorCounters::default();
        assert_eq!(c.total(), 0);
    }

    #[test]
    fn inbound_governor_counters_count_states() {
        let mut c = InboundGovernorCounters::default();
        c.count_state(RemoteSt::RemoteWarmSt);
        c.count_state(RemoteSt::RemoteHotSt);
        c.count_state(RemoteSt::RemoteColdSt);
        c.count_state(RemoteSt::RemoteIdleSt);
        c.count_state(RemoteSt::RemoteWarmSt);

        assert_eq!(c.warm_peers_remote, 2);
        assert_eq!(c.hot_peers_remote, 1);
        assert_eq!(c.cold_peers_remote, 1);
        assert_eq!(c.idle_peers_remote, 1);
        assert_eq!(c.total(), 5);
    }

    #[test]
    fn inbound_governor_counters_add() {
        let a = InboundGovernorCounters {
            cold_peers_remote: 1,
            idle_peers_remote: 2,
            warm_peers_remote: 3,
            hot_peers_remote: 4,
        };
        let b = InboundGovernorCounters {
            cold_peers_remote: 10,
            idle_peers_remote: 20,
            warm_peers_remote: 30,
            hot_peers_remote: 40,
        };
        let c = a + b;
        assert_eq!(c.cold_peers_remote, 11);
        assert_eq!(c.idle_peers_remote, 22);
        assert_eq!(c.warm_peers_remote, 33);
        assert_eq!(c.hot_peers_remote, 44);
    }

    // -- ResponderCounters --

    #[test]
    fn responder_counters_default() {
        let c = ResponderCounters::default();
        assert_eq!(c.hot_responders, 0);
        assert_eq!(c.non_hot_responders, 0);
    }

    // -- InboundGovernorEvent --

    #[test]
    fn inbound_governor_event_new_connection() {
        let cid = conn_id(1000, 2000);
        let e = InboundGovernorEvent::NewConnection(cid);
        assert_eq!(e, InboundGovernorEvent::NewConnection(cid));
    }

    // -- inbound constants --

    #[test]
    fn inbound_constants_values() {
        assert_eq!(inbound_constants::MATURE_PEER_DELAY, Duration::from_secs(900));
        assert_eq!(
            inbound_constants::INACTION_TIMEOUT,
            Duration::from_millis(31_416)
        );
    }

    // -- timeout constants --

    #[test]
    fn timeout_constants() {
        assert_eq!(timeouts::TIME_WAIT_TIMEOUT, Duration::from_secs(60));
        assert_eq!(timeouts::PROTOCOL_IDLE_TIMEOUT, Duration::from_secs(5));
        assert_eq!(timeouts::RESET_TIMEOUT, Duration::from_secs(5));
        assert_eq!(timeouts::SDU_TIMEOUT, Duration::from_secs(30));
        assert_eq!(timeouts::SDU_HANDSHAKE_TIMEOUT, Duration::from_secs(10));
        assert_eq!(timeouts::LOCAL_PROTOCOL_IDLE_TIMEOUT, Duration::from_secs(2));
        assert_eq!(timeouts::LOCAL_TIME_WAIT_TIMEOUT, Duration::ZERO);
    }

    // -- ConnStateId --

    #[test]
    fn conn_state_id_ordering() {
        let a = ConnStateId(1);
        let b = ConnStateId(2);
        assert!(a < b);
        assert_eq!(ConnStateId(5), ConnStateId(5));
    }

    // -- Sum counters across a collection of states --

    #[test]
    fn sum_counters_across_multiple_states() {
        use super::super::governor::ConnectionManagerCounters;

        let states = vec![
            ConnectionState::OutboundUniState {
                conn_id: conn_id(1000, 2000),
            },
            ConnectionState::DuplexState {
                conn_id: conn_id(1001, 2001),
            },
            ConnectionState::InboundIdleState {
                conn_id: conn_id(1002, 2002),
                data_flow: DataFlow::Duplex,
            },
            ConnectionState::TerminatingState {
                conn_id: conn_id(1003, 2003),
                error: None,
            },
            ConnectionState::ReservedOutboundState,
        ];

        let total: ConnectionManagerCounters = states
            .iter()
            .map(connection_state_to_counters)
            .fold(ConnectionManagerCounters::default(), |a, b| a + b);

        assert_eq!(total.outbound_conns, 2); // OutboundUni + Duplex
        assert_eq!(total.inbound_conns, 2); // Duplex + InboundIdle
        assert_eq!(total.duplex_conns, 2); // Duplex + InboundIdle(Dup)
        assert_eq!(total.full_duplex_conns, 1); // Duplex
        assert_eq!(total.unidirectional_conns, 1); // OutboundUni
        assert_eq!(total.terminating_conns, 1); // Terminating
    }
}
