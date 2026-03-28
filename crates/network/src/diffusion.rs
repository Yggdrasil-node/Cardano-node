//! Diffusion-layer types for server-side protocol multiplexing,
//! per-connection lifecycle management, and accept-loop rate limiting.
//!
//! This module provides the pure types that the runtime (`node/`) uses
//! to compose the outbound governor, inbound governor, connection
//! manager, and server into a cohesive diffusion component.
//!
//! All types are data-only; effectful orchestration (tokio tasks, TCP
//! listeners, mux spawning) belongs in `node/`.
//!
//! Reference:
//! - `ouroboros-network-framework/src/Ouroboros/Network/Mux.hs`
//!   (`TemperatureBundle`, `OuroborosBundle`, `ControlMessage`)
//! - `ouroboros-network-framework/src/Ouroboros/Network/ConnectionHandler.hs`
//!   (`Handle`, `MkMuxConnectionHandler`)
//! - `ouroboros-network-framework/src/Ouroboros/Network/Server2.hs`
//!   (accept-loop rate limiting)
//! - `ouroboros-network/src/Ouroboros/Network/PeerSelection/PeerStateActions.hs`
//!   (`PeerConnectionHandle`, `PeerStateActions`)
//! - `ouroboros-network-framework/src/Ouroboros/Network/InboundGovernor.hs`
//!   (`RethrowPolicy`, `ErrorPolicy`)

use std::net::SocketAddr;
use std::time::Duration;

use crate::connection::{AcceptedConnectionsLimit, ConnectionId, DataFlow};
use crate::multiplexer::MiniProtocolNum;

// ---------------------------------------------------------------------------
// Temperature tiers
// ---------------------------------------------------------------------------

/// Groups values by the three connection-temperature tiers used by
/// the peer governor.
///
/// Upstream: `TemperatureBundle` from `Ouroboros.Network.Mux`.
///
/// The three tiers control which mini-protocol instances are active at
/// each point in the peer lifecycle:
///
/// * **Hot** — started when a peer is promoted to active (ChainSync,
///   BlockFetch in NtN).
/// * **Warm** — started on established connection (KeepAlive,
///   TxSubmission in NtN).
/// * **Established** — started immediately after handshake (PeerSharing
///   in NtN).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemperatureBundle<T> {
    /// Mini-protocols active only when the peer is hot (active).
    pub hot: T,
    /// Mini-protocols active when the peer is warm (established).
    pub warm: T,
    /// Mini-protocols started immediately after handshake, regardless
    /// of warm/hot status.
    pub established: T,
}

impl<T> TemperatureBundle<T> {
    /// Apply a function to each tier, producing a new bundle.
    pub fn map<U, F: Fn(T) -> U>(self, f: F) -> TemperatureBundle<U> {
        TemperatureBundle {
            hot: f(self.hot),
            warm: f(self.warm),
            established: f(self.established),
        }
    }

    /// Apply a function to each tier together with its `ProtocolTemperature`.
    pub fn map_with_temp<U, F: Fn(ProtocolTemperature, T) -> U>(
        self,
        f: F,
    ) -> TemperatureBundle<U> {
        TemperatureBundle {
            hot: f(ProtocolTemperature::Hot, self.hot),
            warm: f(ProtocolTemperature::Warm, self.warm),
            established: f(ProtocolTemperature::Established, self.established),
        }
    }

    /// Iterate over all three tiers as `(temperature, &T)`.
    pub fn iter(&self) -> impl Iterator<Item = (ProtocolTemperature, &T)> {
        [
            (ProtocolTemperature::Hot, &self.hot),
            (ProtocolTemperature::Warm, &self.warm),
            (ProtocolTemperature::Established, &self.established),
        ]
        .into_iter()
    }
}

impl<T: Default> Default for TemperatureBundle<T> {
    fn default() -> Self {
        Self {
            hot: T::default(),
            warm: T::default(),
            established: T::default(),
        }
    }
}

/// Labels for the three mini-protocol temperature tiers.
///
/// Upstream: `ProtocolTemperature` from `Ouroboros.Network.Context`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProtocolTemperature {
    /// Active-tier protocols (e.g. ChainSync, BlockFetch).
    Hot,
    /// Warm-tier protocols (e.g. KeepAlive, TxSubmission).
    Warm,
    /// Established-tier protocols (e.g. PeerSharing).
    Established,
}

impl std::fmt::Display for ProtocolTemperature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hot => write!(f, "Hot"),
            Self::Warm => write!(f, "Warm"),
            Self::Established => write!(f, "Established"),
        }
    }
}

// ---------------------------------------------------------------------------
// Mini-protocol descriptors
// ---------------------------------------------------------------------------

/// When to start a mini-protocol instance on a connection.
///
/// Upstream: `StartOnDemandOrEagerly` from `Network.Mux.Types`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MiniProtocolStart {
    /// Start the mini-protocol immediately when the temperature tier
    /// becomes active.
    StartEagerly,
    /// Start only when the first SDU arrives for this mini-protocol.
    StartOnDemand,
    /// Like `StartOnDemand`, but also triggered if *any*
    /// `StartOnDemand` protocol in the same tier starts.
    StartOnDemandAny,
}

/// Ingress-queue limits for a single mini-protocol.
///
/// Upstream: `MiniProtocolLimits` from `Network.Mux.Types`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiniProtocolLimits {
    /// Maximum bytes that can be queued in the ingress direction.
    ///
    /// Upstream: `maximumIngressQueue`.
    pub maximum_ingress_queue: usize,
}

impl Default for MiniProtocolLimits {
    fn default() -> Self {
        Self {
            maximum_ingress_queue: 2_000_000,
        }
    }
}

/// Static descriptor for one mini-protocol within a connection.
///
/// Upstream: `MiniProtocolInfo` from `Network.Mux.Types`.
///
/// The runtime converts these descriptors into actual protocol tasks
/// when establishing a connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MiniProtocolDescriptor {
    /// Unique protocol number on the wire.
    pub num: MiniProtocolNum,
    /// Temperature tier this protocol belongs to.
    pub temperature: ProtocolTemperature,
    /// Whether to start eagerly or on demand.
    pub start_mode: MiniProtocolStart,
    /// Ingress queue limits.
    pub limits: MiniProtocolLimits,
}

/// The full set of mini-protocol descriptors for a connection, grouped
/// by temperature tier.
///
/// Upstream: `OuroborosBundle` from `Ouroboros.Network.Mux` —
/// `TemperatureBundle [MiniProtocol …]`.
///
/// The runtime builds an `OuroborosBundle` at startup for NtN and NtC
/// modes, then applies it to each new connection.
pub type OuroborosBundle = TemperatureBundle<Vec<MiniProtocolDescriptor>>;

/// Construct the standard Node-to-Node `OuroborosBundle`.
///
/// Protocol assignment matches upstream `nodeToNodeProtocols`:
/// - **Hot**: ChainSync (2), BlockFetch (3)
/// - **Warm**: TxSubmission (4), KeepAlive (8)
/// - **Established**: PeerSharing (10)
pub fn ntn_ouroboros_bundle() -> OuroborosBundle {
    OuroborosBundle {
        hot: vec![
            MiniProtocolDescriptor {
                num: MiniProtocolNum::CHAIN_SYNC,
                temperature: ProtocolTemperature::Hot,
                start_mode: MiniProtocolStart::StartEagerly,
                limits: MiniProtocolLimits {
                    maximum_ingress_queue: 2_000_000,
                },
            },
            MiniProtocolDescriptor {
                num: MiniProtocolNum::BLOCK_FETCH,
                temperature: ProtocolTemperature::Hot,
                start_mode: MiniProtocolStart::StartEagerly,
                limits: MiniProtocolLimits {
                    maximum_ingress_queue: 2_000_000,
                },
            },
        ],
        warm: vec![
            MiniProtocolDescriptor {
                num: MiniProtocolNum::TX_SUBMISSION,
                temperature: ProtocolTemperature::Warm,
                start_mode: MiniProtocolStart::StartEagerly,
                limits: MiniProtocolLimits {
                    maximum_ingress_queue: 2_000_000,
                },
            },
            MiniProtocolDescriptor {
                num: MiniProtocolNum::KEEP_ALIVE,
                temperature: ProtocolTemperature::Warm,
                start_mode: MiniProtocolStart::StartEagerly,
                limits: MiniProtocolLimits {
                    maximum_ingress_queue: 2_000_000,
                },
            },
        ],
        established: vec![MiniProtocolDescriptor {
            num: MiniProtocolNum::PEER_SHARING,
            temperature: ProtocolTemperature::Established,
            start_mode: MiniProtocolStart::StartEagerly,
            limits: MiniProtocolLimits {
                maximum_ingress_queue: 2_000_000,
            },
        }],
    }
}

/// Construct the standard Node-to-Client `OuroborosBundle`.
///
/// Protocol assignment matches upstream `nodeToClientProtocols`:
/// - **Established**: LocalTxSubmission (5), LocalStateQuery (7),
///   LocalTxMonitor (9)
///
/// NtC connections are responder-only; there are no hot/warm tiers.
pub fn ntc_ouroboros_bundle() -> OuroborosBundle {
    OuroborosBundle {
        hot: Vec::new(),
        warm: Vec::new(),
        established: vec![
            MiniProtocolDescriptor {
                num: MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION,
                temperature: ProtocolTemperature::Established,
                start_mode: MiniProtocolStart::StartEagerly,
                limits: MiniProtocolLimits {
                    maximum_ingress_queue: 2_000_000,
                },
            },
            MiniProtocolDescriptor {
                num: MiniProtocolNum::NTC_LOCAL_STATE_QUERY,
                temperature: ProtocolTemperature::Established,
                start_mode: MiniProtocolStart::StartEagerly,
                limits: MiniProtocolLimits {
                    maximum_ingress_queue: 2_000_000,
                },
            },
            MiniProtocolDescriptor {
                num: MiniProtocolNum::NTC_LOCAL_TX_MONITOR,
                temperature: ProtocolTemperature::Established,
                start_mode: MiniProtocolStart::StartEagerly,
                limits: MiniProtocolLimits {
                    maximum_ingress_queue: 2_000_000,
                },
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Control messages
// ---------------------------------------------------------------------------

/// Mux-level control message that the governor sends to per-temperature
/// protocol instances.
///
/// Upstream: `ControlMessage` from `Ouroboros.Network.Mux`.
///
/// Each connection carries a `TemperatureBundle<ControlMessage>`. When
/// the governor changes a peer's status, it updates the relevant tier:
///
/// * Promote cold→warm: set warm & established to `Continue`.
/// * Promote warm→hot: set hot to `Continue`.
/// * Demote hot→warm: set hot to `Terminate`.
/// * Demote warm→cold: set warm to `Terminate`, established to `Terminate`.
///
/// The `Quiesce` variant is used during churn: it tells in-progress
/// protocols to finish their current exchange and then stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlMessage {
    /// Protocols should continue running normally.
    Continue,
    /// Protocols should finish their current exchange gracefully and
    /// not start another.
    ///
    /// Used during churn: the protocol completes any in-flight
    /// request/response but does not issue new work.
    Quiesce,
    /// Protocols should stop as soon as possible.
    Terminate,
}

impl Default for ControlMessage {
    fn default() -> Self {
        Self::Continue
    }
}

// ---------------------------------------------------------------------------
// Mux mode
// ---------------------------------------------------------------------------

/// Multiplexer mode — selects which protocol directions are active.
///
/// Upstream: `Mode` from `Network.Mux.Types`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MuxMode {
    /// Only initiator-side protocols (outbound connections).
    InitiatorMode,
    /// Only responder-side protocols (inbound connections, NtC).
    ResponderMode,
    /// Both initiator and responder (typical NtN bidirectional).
    InitiatorResponderMode,
}

impl MuxMode {
    /// Whether the initiator side is active.
    pub fn has_initiator(self) -> bool {
        matches!(self, Self::InitiatorMode | Self::InitiatorResponderMode)
    }

    /// Whether the responder side is active.
    pub fn has_responder(self) -> bool {
        matches!(self, Self::ResponderMode | Self::InitiatorResponderMode)
    }
}

// ---------------------------------------------------------------------------
// Accept-loop rate limiting
// ---------------------------------------------------------------------------

/// Result of evaluating accept-loop rate limits.
///
/// Upstream: the accept loop in `Ouroboros.Network.Server2` applies
/// rate limiting before each `accept()` call based on the current
/// inbound connection count relative to `AcceptedConnectionsLimit`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RateLimitDecision {
    /// Under soft limit — accept immediately.
    NoDelay,
    /// Between soft and hard limit — delay by the configured amount
    /// before accepting the next connection.
    SoftDelay(Duration),
    /// At or above hard limit — refuse new connections.
    HardLimit,
}

/// Compute the accept-loop rate-limit decision.
///
/// Upstream: `runConnectionRateLimits` in `Server2.hs`.
///
/// * count < soft_limit → `NoDelay`
/// * soft_limit ≤ count < hard_limit → `SoftDelay(delay)`
/// * count ≥ hard_limit → `HardLimit`
pub fn rate_limit_decision(
    inbound_count: u32,
    limits: &AcceptedConnectionsLimit,
) -> RateLimitDecision {
    if inbound_count >= limits.hard_limit {
        RateLimitDecision::HardLimit
    } else if inbound_count >= limits.soft_limit {
        RateLimitDecision::SoftDelay(limits.delay)
    } else {
        RateLimitDecision::NoDelay
    }
}

// ---------------------------------------------------------------------------
// Error policy
// ---------------------------------------------------------------------------

/// How the runtime should respond to a mini-protocol error on a
/// connection.
///
/// Upstream: `ErrorPolicy` from
/// `Ouroboros.Network.ErrorPolicy` — we simplify to the three actions
/// the framework actually takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCommand {
    /// Shut the connection down immediately.
    ShutdownConnection,
    /// Shut the connection down and suspend the peer for the given
    /// duration before allowing reconnection.
    ShutdownAndSuspend(Duration),
    /// Ignore the error and continue.
    Ignore,
}

/// How to translate a mini-protocol exception into a connection-level
/// decision.
///
/// Upstream: `RethrowPolicy` from
/// `Ouroboros.Network.InboundGovernor` controls whether IG re-throws
/// an exception (kill con) or absorbs it (restart responder).
///
/// In our model, the runtime matches each mini-protocol error against
/// one of these policies to decide whether to release the connection,
/// log the error, or suspend the peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RethrowPolicy {
    /// Re-throw the exception, which will tear down the mux and
    /// connection.
    ///
    /// Upstream: `ShutdownPeer`.
    RethrowException,
    /// Absorb the exception; the responder will be restarted by the
    /// inbound governor.
    ///
    /// Upstream: `RestartResponder`.
    AbsorbException,
}

/// Result of classifying a mini-protocol error through the error
/// policy and rethrow policy.
///
/// The runtime uses this to decide what `InboundGovernorEvent` to
/// produce and whether to release the connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorPolicyResult {
    /// Whether to keep or shut down the connection.
    pub command: ErrorCommand,
    /// Whether the IG should rethrow (tear down) or absorb (restart).
    pub rethrow: RethrowPolicy,
    /// Optional peer suspension duration for the governor's failure
    /// tracking (`GovernorState.record_failure`).
    pub suspend_duration: Option<Duration>,
}

// ---------------------------------------------------------------------------
// Peer connection handle
// ---------------------------------------------------------------------------

/// Represents a single connection to a peer as seen by the governor
/// and `PeerStateActions`.
///
/// Upstream: `PeerConnectionHandle` from
/// `Ouroboros.Network.PeerSelection.PeerStateActions`.
///
/// The runtime creates one after a successful handshake and stores it
/// in the governor's peer state. The handle carries enough metadata
/// for `PeerStateActions` to perform CM operations and per-temperature
/// protocol start/stop without re-discovering connection details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerConnectionHandle {
    /// Unique connection identifier (local + remote addresses).
    pub conn_id: ConnectionId,
    /// Negotiated data flow (unidirectional or duplex).
    pub data_flow: DataFlow,
    /// Per-temperature control messages. The runtime updates these to
    /// start/stop protocol tiers.
    pub control: TemperatureBundle<ControlMessage>,
    /// Whether the NtN handshake negotiated peer sharing.
    pub peer_sharing_enabled: bool,
}

impl PeerConnectionHandle {
    /// Create a handle with all temperatures set to `Continue`.
    pub fn new(conn_id: ConnectionId, data_flow: DataFlow, peer_sharing: bool) -> Self {
        Self {
            conn_id,
            data_flow,
            control: TemperatureBundle {
                hot: ControlMessage::Continue,
                warm: ControlMessage::Continue,
                established: ControlMessage::Continue,
            },
            peer_sharing_enabled: peer_sharing,
        }
    }

    /// Returns `true` if the connection supports duplex data flow.
    pub fn is_duplex(&self) -> bool {
        self.data_flow == DataFlow::Duplex
    }

    /// Set the hot-tier control message (e.g. `Continue` on
    /// warm→hot promotion, `Terminate` on hot→warm demotion).
    pub fn set_hot_control(&mut self, msg: ControlMessage) {
        self.control.hot = msg;
    }

    /// Set the warm-tier control message (e.g. `Terminate` on
    /// warm→cold demotion).
    pub fn set_warm_control(&mut self, msg: ControlMessage) {
        self.control.warm = msg;
    }

    /// Set the established-tier control message.
    pub fn set_established_control(&mut self, msg: ControlMessage) {
        self.control.established = msg;
    }

    /// Terminate all protocol tiers.
    pub fn terminate_all(&mut self) {
        self.control.hot = ControlMessage::Terminate;
        self.control.warm = ControlMessage::Terminate;
        self.control.established = ControlMessage::Terminate;
    }

    /// Quiesce the hot tier (for churn-driven demotion).
    pub fn quiesce_hot(&mut self) {
        self.control.hot = ControlMessage::Quiesce;
    }
}

// ---------------------------------------------------------------------------
// Peer state actions (pure decision descriptors)
// ---------------------------------------------------------------------------

/// A governor-to-runtime action that bridges peer governor decisions
/// to CM operations and protocol starts/stops.
///
/// Upstream: `PeerStateActions` from
/// `Ouroboros.Network.PeerSelection.PeerStateActions` is a record of
/// IO actions. We model the same interface as an enum of pure action
/// descriptors that the runtime dispatches.
///
/// The runtime loop:
/// 1. Runs `governor_tick` → `Vec<GovernorAction>`.
/// 2. Translates each `GovernorAction` into a `PeerStateAction`.
/// 3. Dispatches each `PeerStateAction` against the CM + mux.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeerStateAction {
    /// Establish a new outbound connection to a peer.
    ///
    /// Runtime: CM `acquire_outbound_connection` → TCP connect →
    /// handshake → start warm+established mini-protocols →
    /// store `PeerConnectionHandle`.
    EstablishConnection(SocketAddr),

    /// Activate (promote to hot) an existing warm connection.
    ///
    /// Runtime: set hot-tier `ControlMessage` to `Continue` →
    /// start hot mini-protocols on the mux.
    ActivateConnection(SocketAddr),

    /// Deactivate (demote to warm) a hot connection.
    ///
    /// Runtime: set hot-tier `ControlMessage` to `Terminate` →
    /// await hot protocol completion.
    DeactivateConnection(SocketAddr),

    /// Close an established connection (demote to cold).
    ///
    /// Runtime: CM `release_outbound_connection` →
    /// terminate warm+established protocols → close socket.
    CloseConnection(SocketAddr),
}

// ---------------------------------------------------------------------------
// Repromote delay
// ---------------------------------------------------------------------------

/// How long to wait before attempting to repromote a peer after an
/// error-driven demotion.
///
/// Upstream: `RepromoteDelay` from `PeerStateActions`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepromoteDelay {
    /// Repromote after a short delay (default: 10 s).
    ShortDelay,
    /// Repromote after a long delay (default: 200 s).
    LongDelay,
}

impl RepromoteDelay {
    /// Convert to a `Duration`.
    pub fn as_duration(self) -> Duration {
        match self {
            Self::ShortDelay => Duration::from_secs(10),
            Self::LongDelay => Duration::from_secs(200),
        }
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

    // -- TemperatureBundle --

    #[test]
    fn temperature_bundle_default() {
        let bundle: TemperatureBundle<Vec<u8>> = TemperatureBundle::default();
        assert!(bundle.hot.is_empty());
        assert!(bundle.warm.is_empty());
        assert!(bundle.established.is_empty());
    }

    #[test]
    fn temperature_bundle_map() {
        let bundle = TemperatureBundle {
            hot: 1u32,
            warm: 2,
            established: 3,
        };
        let doubled = bundle.map(|x| x * 2);
        assert_eq!(doubled.hot, 2);
        assert_eq!(doubled.warm, 4);
        assert_eq!(doubled.established, 6);
    }

    #[test]
    fn temperature_bundle_map_with_temp() {
        let bundle = TemperatureBundle {
            hot: 10,
            warm: 20,
            established: 30,
        };
        let labeled = bundle.map_with_temp(|temp, val| format!("{}:{}", temp, val));
        assert_eq!(labeled.hot, "Hot:10");
        assert_eq!(labeled.warm, "Warm:20");
        assert_eq!(labeled.established, "Established:30");
    }

    #[test]
    fn temperature_bundle_iter() {
        let bundle = TemperatureBundle {
            hot: "h",
            warm: "w",
            established: "e",
        };
        let items: Vec<_> = bundle.iter().collect();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], (ProtocolTemperature::Hot, &"h"));
        assert_eq!(items[1], (ProtocolTemperature::Warm, &"w"));
        assert_eq!(items[2], (ProtocolTemperature::Established, &"e"));
    }

    // -- ProtocolTemperature --

    #[test]
    fn protocol_temperature_display() {
        assert_eq!(ProtocolTemperature::Hot.to_string(), "Hot");
        assert_eq!(ProtocolTemperature::Warm.to_string(), "Warm");
        assert_eq!(ProtocolTemperature::Established.to_string(), "Established");
    }

    #[test]
    fn protocol_temperature_ordering() {
        assert!(ProtocolTemperature::Hot < ProtocolTemperature::Warm);
        assert!(ProtocolTemperature::Warm < ProtocolTemperature::Established);
    }

    // -- MiniProtocolStart --

    #[test]
    fn mini_protocol_start_variants() {
        let _eager = MiniProtocolStart::StartEagerly;
        let _demand = MiniProtocolStart::StartOnDemand;
        let _any = MiniProtocolStart::StartOnDemandAny;
    }

    // -- MiniProtocolLimits --

    #[test]
    fn mini_protocol_limits_default() {
        let limits = MiniProtocolLimits::default();
        assert_eq!(limits.maximum_ingress_queue, 2_000_000);
    }

    // -- OuroborosBundle constructors --

    #[test]
    fn ntn_bundle_structure() {
        let bundle = ntn_ouroboros_bundle();
        assert_eq!(bundle.hot.len(), 2);
        assert_eq!(bundle.hot[0].num, MiniProtocolNum::CHAIN_SYNC);
        assert_eq!(bundle.hot[1].num, MiniProtocolNum::BLOCK_FETCH);
        assert_eq!(bundle.warm.len(), 2);
        assert_eq!(bundle.warm[0].num, MiniProtocolNum::TX_SUBMISSION);
        assert_eq!(bundle.warm[1].num, MiniProtocolNum::KEEP_ALIVE);
        assert_eq!(bundle.established.len(), 1);
        assert_eq!(bundle.established[0].num, MiniProtocolNum::PEER_SHARING);
    }

    #[test]
    fn ntc_bundle_structure() {
        let bundle = ntc_ouroboros_bundle();
        assert!(bundle.hot.is_empty());
        assert!(bundle.warm.is_empty());
        assert_eq!(bundle.established.len(), 3);
        assert_eq!(
            bundle.established[0].num,
            MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION
        );
        assert_eq!(
            bundle.established[1].num,
            MiniProtocolNum::NTC_LOCAL_STATE_QUERY
        );
        assert_eq!(
            bundle.established[2].num,
            MiniProtocolNum::NTC_LOCAL_TX_MONITOR
        );
    }

    #[test]
    fn all_ntn_protocols_eagerly_started() {
        let bundle = ntn_ouroboros_bundle();
        for (_, protos) in bundle.iter() {
            for p in protos {
                assert_eq!(p.start_mode, MiniProtocolStart::StartEagerly);
            }
        }
    }

    // -- ControlMessage --

    #[test]
    fn control_message_default_is_continue() {
        assert_eq!(ControlMessage::default(), ControlMessage::Continue);
    }

    // -- MuxMode --

    #[test]
    fn mux_mode_initiator_only() {
        let mode = MuxMode::InitiatorMode;
        assert!(mode.has_initiator());
        assert!(!mode.has_responder());
    }

    #[test]
    fn mux_mode_responder_only() {
        let mode = MuxMode::ResponderMode;
        assert!(!mode.has_initiator());
        assert!(mode.has_responder());
    }

    #[test]
    fn mux_mode_both() {
        let mode = MuxMode::InitiatorResponderMode;
        assert!(mode.has_initiator());
        assert!(mode.has_responder());
    }

    // -- Rate limiting --

    #[test]
    fn rate_limit_no_delay() {
        let limits = AcceptedConnectionsLimit::default(); // 512 hard, 384 soft
        assert_eq!(rate_limit_decision(0, &limits), RateLimitDecision::NoDelay);
        assert_eq!(
            rate_limit_decision(383, &limits),
            RateLimitDecision::NoDelay
        );
    }

    #[test]
    fn rate_limit_soft_delay() {
        let limits = AcceptedConnectionsLimit::default();
        let result = rate_limit_decision(384, &limits);
        assert_eq!(
            result,
            RateLimitDecision::SoftDelay(Duration::from_secs(5))
        );

        let result = rate_limit_decision(511, &limits);
        assert_eq!(
            result,
            RateLimitDecision::SoftDelay(Duration::from_secs(5))
        );
    }

    #[test]
    fn rate_limit_hard_limit() {
        let limits = AcceptedConnectionsLimit::default();
        assert_eq!(
            rate_limit_decision(512, &limits),
            RateLimitDecision::HardLimit
        );
        assert_eq!(
            rate_limit_decision(1000, &limits),
            RateLimitDecision::HardLimit
        );
    }

    #[test]
    fn rate_limit_custom_limits() {
        let limits = AcceptedConnectionsLimit {
            hard_limit: 10,
            soft_limit: 5,
            delay: Duration::from_millis(100),
        };
        assert_eq!(rate_limit_decision(4, &limits), RateLimitDecision::NoDelay);
        assert_eq!(
            rate_limit_decision(5, &limits),
            RateLimitDecision::SoftDelay(Duration::from_millis(100))
        );
        assert_eq!(
            rate_limit_decision(10, &limits),
            RateLimitDecision::HardLimit
        );
    }

    // -- ErrorCommand --

    #[test]
    fn error_command_suspend_duration() {
        let cmd = ErrorCommand::ShutdownAndSuspend(Duration::from_secs(60));
        match cmd {
            ErrorCommand::ShutdownAndSuspend(d) => {
                assert_eq!(d, Duration::from_secs(60));
            }
            _ => panic!("expected ShutdownAndSuspend"),
        }
    }

    // -- RethrowPolicy --

    #[test]
    fn rethrow_policy_variants() {
        assert_ne!(RethrowPolicy::RethrowException, RethrowPolicy::AbsorbException);
    }

    // -- PeerConnectionHandle --

    #[test]
    fn peer_connection_handle_new() {
        let conn_id = ConnectionId {
            local: addr(1000),
            remote: addr(2000),
        };
        let handle = PeerConnectionHandle::new(conn_id, DataFlow::Duplex, true);
        assert!(handle.is_duplex());
        assert!(handle.peer_sharing_enabled);
        assert_eq!(handle.control.hot, ControlMessage::Continue);
        assert_eq!(handle.control.warm, ControlMessage::Continue);
        assert_eq!(handle.control.established, ControlMessage::Continue);
    }

    #[test]
    fn peer_connection_handle_set_controls() {
        let conn_id = ConnectionId {
            local: addr(1000),
            remote: addr(2000),
        };
        let mut handle = PeerConnectionHandle::new(conn_id, DataFlow::Duplex, false);

        handle.set_hot_control(ControlMessage::Terminate);
        assert_eq!(handle.control.hot, ControlMessage::Terminate);
        assert_eq!(handle.control.warm, ControlMessage::Continue); // Unchanged.

        handle.set_warm_control(ControlMessage::Quiesce);
        assert_eq!(handle.control.warm, ControlMessage::Quiesce);
    }

    #[test]
    fn peer_connection_handle_terminate_all() {
        let conn_id = ConnectionId {
            local: addr(1000),
            remote: addr(2000),
        };
        let mut handle = PeerConnectionHandle::new(conn_id, DataFlow::Unidirectional, false);
        assert!(!handle.is_duplex());

        handle.terminate_all();
        assert_eq!(handle.control.hot, ControlMessage::Terminate);
        assert_eq!(handle.control.warm, ControlMessage::Terminate);
        assert_eq!(handle.control.established, ControlMessage::Terminate);
    }

    #[test]
    fn peer_connection_handle_quiesce_hot() {
        let conn_id = ConnectionId {
            local: addr(1000),
            remote: addr(2000),
        };
        let mut handle = PeerConnectionHandle::new(conn_id, DataFlow::Duplex, true);

        handle.quiesce_hot();
        assert_eq!(handle.control.hot, ControlMessage::Quiesce);
        assert_eq!(handle.control.warm, ControlMessage::Continue);
        assert_eq!(handle.control.established, ControlMessage::Continue);
    }

    // -- PeerStateAction --

    #[test]
    fn peer_state_action_variants() {
        let a = PeerStateAction::EstablishConnection(addr(2000));
        let b = PeerStateAction::ActivateConnection(addr(2000));
        let c = PeerStateAction::DeactivateConnection(addr(2000));
        let d = PeerStateAction::CloseConnection(addr(2000));

        assert_ne!(a, b);
        assert_ne!(c, d);
    }

    // -- RepromoteDelay --

    #[test]
    fn repromote_delay_durations() {
        assert_eq!(RepromoteDelay::ShortDelay.as_duration(), Duration::from_secs(10));
        assert_eq!(RepromoteDelay::LongDelay.as_duration(), Duration::from_secs(200));
    }

    // -- Integration: governor action → peer state action mapping --

    #[test]
    fn governor_to_peer_state_action_mapping() {
        // Demonstrate how GovernorAction maps to PeerStateAction.
        // PromoteToWarm → EstablishConnection (cold→warm requires TCP connect)
        // PromoteToHot  → ActivateConnection (warm→hot starts hot protocols)
        // DemoteToWarm  → DeactivateConnection (hot→warm stops hot protocols)
        // DemoteToCold  → CloseConnection (warm→cold tears down connection)
        let cold_peer = addr(2000);
        let warm_peer = addr(2001);
        let hot_peer = addr(2002);

        let actions = [
            PeerStateAction::EstablishConnection(cold_peer),
            PeerStateAction::ActivateConnection(warm_peer),
            PeerStateAction::DeactivateConnection(hot_peer),
            PeerStateAction::CloseConnection(warm_peer),
        ];

        assert_eq!(actions.len(), 4);
    }
}
