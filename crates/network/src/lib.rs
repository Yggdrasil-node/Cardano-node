#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Networking types for Ouroboros handshake, multiplexing, and mini-protocol
//! state machines.
//!
//! This crate models the node-to-node protocol surface defined by the
//! [Ouroboros network specifications](https://github.com/IntersectMBO/ouroboros-network/).

/// Async multiplexer bearer — transport abstraction for SDU-framed I/O.
pub mod bearer;
/// BlockFetch client driver — typed, state-machine-correct protocol loop.
pub mod blockfetch_client;
/// Multi-peer concurrent BlockFetch foundation — pool, scheduler, reorder buffer.
pub mod blockfetch_pool;

/// Shared, runtime-instrumentable handle to a [`crate::blockfetch_pool::BlockFetchPool`].
///
/// Mutex is brief and never held across `.await`; the runtime calls
/// `note_dispatch` / `note_success` / `note_failure` synchronously around
/// each BlockFetch round-trip.
pub type BlockFetchInstrumentation =
    std::sync::Arc<std::sync::Mutex<crate::blockfetch_pool::BlockFetchPool>>;
/// ChainSync client driver — typed, state-machine-correct protocol loop.
pub mod chainsync_client;
/// Connection manager types and state machine.
pub mod connection;
/// DataPointForward mini-protocol acceptor driver
/// (cardano-tracer side of the data-point sub-protocol — sister to
/// the TraceObjectForward acceptor).
pub mod data_point_acceptor;
/// DataPointForward mini-protocol forwarder driver
/// (cardano-node side — replies to acceptor requests with
/// `(name, maybe-bytes)` pairs from a per-node DataPointStore).
pub mod data_point_forwarder;
/// DataPointForward mini-protocol acceptor runtime aggregator
/// (cardano-tracer's `acceptDataPoints{Init,Resp}` analog — sister
/// to the TraceObjectForward run-acceptor).
pub mod data_point_run_acceptor;
/// DataPointForward mini-protocol forwarder-side runtime aggregator
/// (cardano-node's `forwardDataPoints{Init,Resp}` analog — pairs
/// the R471 forwarder driver with the R472 DataPointStore).
pub mod data_point_run_forwarder;
/// Handshake negotiation types and state machine.
pub mod handshake;
/// KeepAlive client driver — typed, state-machine-correct protocol loop.
pub mod keepalive_client;
/// Refresh-oriented provider interfaces for time-varying ledger peers.
pub mod ledger_peers_provider;
/// TCP listener for inbound peer connections.
pub mod listener;
/// Unix-pipe listener for inbound trace-forwarder connections.
#[cfg(unix)]
pub mod local_listener;
/// Multiplexer framing, SDU header, and protocol numbering.
/// Multiplexer / demultiplexer — SDU routing between bearer and protocol channels.
pub mod mux;
/// Peer connection lifecycle — handshake negotiation and data-protocol setup.
pub mod peer;
/// Peer registry state and source/status tracking.
pub mod peer_registry;
/// Peer candidate resolution and ordering helpers for runtime bootstrap.
pub mod peer_selection;
/// Per-protocol per-state time limits matching upstream `ProtocolTimeLimits`.
pub mod protocol_limits;
/// Per-protocol upper bounds on peer-supplied count fields, used to gate
/// CBOR decoder pre-allocations against attacker-controlled values.
pub mod protocol_size_limits;
/// Mini-protocol state machine modules.
pub mod protocols;
/// Topology root-peer domain types and resolved provider snapshots.
pub mod root_peers;
/// Refresh-oriented provider interfaces for time-varying root peers.
pub mod root_peers_provider;
/// TraceObjectForward mini-protocol acceptor driver
/// (cardano-tracer side of the trace-forwarder pipe).
pub mod trace_object_acceptor;
/// Trace-forwarder handshake state-machine driver — runs the
/// ProposeVersions / AcceptVersion / Refuse exchange on a mux'd
/// HANDSHAKE channel for both responder and initiator roles.
pub mod trace_object_forward_handshake_driver;
/// TraceObjectForward mini-protocol acceptor runtime aggregator
/// (cardano-tracer's `acceptTraceObjects{Init,Resp}` analog).
pub mod trace_object_run_acceptor;
/// TxSubmission2 client driver — typed, state-machine-correct protocol loop.
pub mod txsubmission_client;

// -- Server-side protocol drivers ---------------------------------------------

/// BlockFetch server driver — typed, state-machine-correct responder loop.
pub mod blockfetch_server;
/// ChainSync server driver — typed, state-machine-correct responder loop.
pub mod chainsync_server;
/// KeepAlive server driver — typed, state-machine-correct responder loop.
pub mod keepalive_server;
/// TxSubmission2 server driver — typed, state-machine-correct responder loop.
pub mod txsubmission_server;

// -- Peer governor ------------------------------------------------------------

/// Peer governor — promotion, demotion, and valency enforcement decisions.
pub mod governor;

/// Inbound governor — pure step-function decision engine for inbound
/// connection lifecycle, remote peer state tracking, and duplex peer
/// maturation.
pub mod inbound_governor;

/// Connection manager decision engine — pure state machine for outbound
/// acquire/release, inbound include/release, and remote promote/demote.
pub mod connection_manager;

/// PeerSharing client driver — typed, state-machine-correct protocol loop.
pub mod peersharing_client;

/// PeerSharing server driver — typed, state-machine-correct responder loop.
pub mod peersharing_server;

/// Diffusion-layer types — temperature bundles, mini-protocol descriptors,
/// control messages, rate limiting, error policy, and peer connection handles.
pub mod diffusion;

/// Governor-to-runtime peer state action bridge helpers.
pub mod peer_state_actions;

// -- Node-to-Client (NtC) server drivers ----------------------------------

/// LocalTxSubmission server driver — NtC transaction intake from local clients.
pub mod local_tx_submission_server;

/// LocalTxMonitor server driver — NtC mempool monitoring for local clients.
pub mod local_tx_monitor_server;

/// LocalStateQuery server driver — NtC ledger state query responder.
pub mod local_state_query_server;

// -- Node-to-Client (NtC) client drivers ----------------------------------

/// LocalStateQuery client driver — NtC ledger state query initiator.
pub mod local_state_query_client;

/// LocalTxSubmission client driver — NtC transaction submission initiator.
pub mod local_tx_submission_client;

/// LocalTxMonitor client driver — NtC mempool monitor initiator.
pub mod local_tx_monitor_client;

/// Node-to-client (NtC) connection lifecycle and handshake.
#[cfg(unix)]
pub mod ntc_peer;

// -- Bearer re-exports --------------------------------------------------------
pub use bearer::{Bearer, BearerError, MAX_SDU_PAYLOAD, Sdu, TcpBearer};

// -- Handshake re-exports -----------------------------------------------------
pub use handshake::{
    HandshakeMessage, HandshakeRequest, HandshakeState, HandshakeTransitionError, HandshakeVersion,
    NodeToNodeVersionData, RefuseReason,
};

// -- Mux re-exports (R317 merged the previously-separate `multiplexer` module
// into `mux`, matching upstream `Ouroboros.Network.Mux` single-file layout) --
#[cfg(unix)]
pub use mux::start_unix as start_mux_unix;
#[cfg(unix)]
pub use mux::start_unix_configured as start_mux_unix_configured;
pub use mux::{
    DEFAULT_INGRESS_LIMIT, DEFAULT_PROTOCOL_WEIGHT, EGRESS_SOFT_LIMIT, MAX_SEGMENT_SIZE,
    MessageChannel, MiniProtocolDir, MiniProtocolNum, MuxChannel, MuxError, MuxHandle,
    ProtocolConfig, ProtocolHandle, SDU_HEADER_SIZE, SduDecodeError, SduHeader, WeightHandle,
    start as start_mux, start_configured as start_mux_configured,
};

// -- Peer re-exports ----------------------------------------------------------
pub use data_point_acceptor::{DataPointAcceptor, DataPointAcceptorError};
pub use data_point_forwarder::{
    DataPointForwarder, DataPointForwarderError, DataPointForwarderEvent,
};
pub use data_point_run_acceptor::{
    AcceptDataPointsError, SHUTDOWN_TIMEOUT as DATA_POINTS_SHUTDOWN_TIMEOUT,
    accept_data_points_init, accept_data_points_resp,
};
pub use data_point_run_forwarder::{
    ForwardDataPointsError, forward_data_points_init, forward_data_points_resp,
};
pub use ledger_peers_provider::{
    ConsensusLedgerPeerInputs, ConsensusLedgerPeerSource, LedgerPeerProvider,
    LedgerPeerProviderError, LedgerPeerProviderKind, LedgerPeerProviderRefresh,
    LedgerPeerRegistryUpdate, LedgerPeerSnapshot, LedgerPeerUseDecision, LedgerStateAgeInputs,
    LedgerStateJudgement, LiveLedgerPeerRefreshObservation, PeerSnapshotFileObservation,
    PeerSnapshotFileSource, PeerSnapshotFreshness, ScriptedLedgerPeerProvider,
    always_eligible_snapshot_peers, apply_ledger_peer_refresh, derive_peer_snapshot_freshness,
    eligible_ledger_peer_candidates, judge_ledger_peer_usage, judge_ledger_state_age,
    live_refresh_ledger_peer_registry, live_refresh_ledger_peer_registry_observed,
    merge_ledger_peer_snapshots, reconcile_ledger_peer_registry_with_policy,
    refresh_ledger_peer_registry,
};
pub use listener::{PeerListener, PeerListenerError};
#[cfg(unix)]
pub use local_listener::{LocalPeerListener, LocalPeerListenerError};
#[cfg(unix)]
pub use ntc_peer::{
    NodeToClientVersionData, NtcPeerConnection, NtcPeerError, ntc_accept, ntc_connect,
};
pub use peer::{PeerConnection, PeerError, accept as peer_accept, connect as peer_connect};
pub use peer_registry::{
    PeerRegistry, PeerRegistryEntry, PeerRegistryStatusCounts, PeerSource, PeerStatus,
};
pub use peer_selection::{
    LocalRootConfig, PeerAccessPoint, PeerAttemptState, PeerBootstrapTargets, PeerDiffusionMode,
    PublicRootConfig, bootstrap_targets, ordered_fallback_peers as ordered_peer_fallbacks,
    ordered_peer_candidates, peer_attempt_state, resolve_peer_access_point,
    resolve_peer_access_points,
};
pub use root_peers::{
    AfterSlot, ResolvedLocalRootGroup, RootPeerProviderState, RootPeerProviders, TopologyConfig,
    UseBootstrapPeers, UseLedgerPeers, reconcile_root_peer_providers, resolve_root_peer_providers,
};
pub use root_peers_provider::{
    DnsRefreshPolicy, DnsRootPeerProvider, DnsRootPeerProviderConfig, RootPeerProvider,
    RootPeerProviderError, RootPeerProviderKind, RootPeerProviderRefresh, ScriptedRootPeerProvider,
    refresh_root_peer_state, refresh_root_peer_state_and_registry,
};
pub use trace_object_acceptor::{TraceObjectAcceptor, TraceObjectAcceptorError};
pub use trace_object_forward_handshake_driver::{
    HANDSHAKE_DEADLINE, HandshakeError as TraceForwardHandshakeError, HandshakeOutcome,
    run_handshake_initiator, run_handshake_responder,
};
pub use trace_object_run_acceptor::{
    AcceptTimeout, AcceptTraceObjectsError, SHUTDOWN_TIMEOUT, accept_trace_objects_init,
    accept_trace_objects_resp, timeout_when_stopped,
};

// -- Protocol re-exports ------------------------------------------------------
pub use protocols::{
    AcquireFailure, AcquireTarget, BlockFetchMessage, BlockFetchState, BlockFetchTransitionError,
    ChainRange, ChainSyncMessage, ChainSyncState, ChainSyncTransitionError, KeepAliveMessage,
    KeepAliveState, KeepAliveTransitionError, LocalStateQueryMessage, LocalStateQueryState,
    LocalStateQueryTransitionError, LocalTxMonitorMessage, LocalTxMonitorState,
    LocalTxMonitorTransitionError, LocalTxSubmissionMessage, LocalTxSubmissionState,
    LocalTxSubmissionTransitionError, PeerSharingMessage, PeerSharingState,
    PeerSharingTransitionError, SharedPeerAddress, TxIdAndSize, TxSubmissionMessage,
    TxSubmissionState, TxSubmissionTransitionError,
};

// -- NtC server driver re-exports ---------------------------------------------
pub use local_state_query_server::{
    LocalStateQueryAcquiredRequest, LocalStateQueryIdleRequest, LocalStateQueryServer,
    LocalStateQueryServerError,
};
pub use local_tx_monitor_server::{
    LocalTxMonitorAcquiredRequest, LocalTxMonitorIdleRequest, LocalTxMonitorServer,
    LocalTxMonitorServerError,
};
pub use local_tx_submission_server::{
    LocalTxRequest, LocalTxSubmissionServer, LocalTxSubmissionServerError,
};

// -- ChainSync client re-exports ----------------------------------------------
pub use chainsync_client::{
    ChainSyncClient, ChainSyncClientError, DecodedHeaderNextResponse, IntersectResponse,
    NextResponse, TypedIntersectResponse, TypedNextResponse,
};

// -- BlockFetch client re-exports ---------------------------------------------
pub use blockfetch_client::{BatchResponse, BlockFetchClient, BlockFetchClientError};

// -- KeepAlive client re-exports ----------------------------------------------
pub use keepalive_client::{KeepAliveClient, KeepAliveClientError};

// -- TxSubmission client re-exports -------------------------------------------
pub use txsubmission_client::{TxServerRequest, TxSubmissionClient, TxSubmissionClientError};

// -- BlockFetch server re-exports ---------------------------------------------
pub use blockfetch_server::{BlockFetchServer, BlockFetchServerError, BlockFetchServerRequest};

// -- ChainSync server re-exports ----------------------------------------------
pub use chainsync_server::{ChainSyncServer, ChainSyncServerError, ChainSyncServerRequest};

// -- KeepAlive server re-exports ----------------------------------------------
pub use keepalive_server::{KeepAliveServer, KeepAliveServerError};

// -- TxSubmission server re-exports -------------------------------------------
pub use txsubmission_server::{TxIdsReply, TxSubmissionServer, TxSubmissionServerError};

// -- Governor re-exports ------------------------------------------------------
pub use governor::{
    AssociationMode, ChurnConfig, ChurnMode, ChurnPhase, ChurnRegime, ConnectionManagerCounters,
    ConsensusMode, FetchMode, GovernorAction, GovernorState, GovernorTargets, HotPeerScheduling,
    LocalRootTargets, NodePeerSharing, OutboundConnectionsState, PeerFailureRecord, PeerMetrics,
    PeerSelectionCounters, PeerSelectionMode, PeerSelectionTimeouts, churn_decrease,
    churn_decrease_active, churn_decrease_established, churn_mode_from_fetch_mode,
    compute_association_mode, compute_outbound_connections_state, enforce_local_root_valency,
    evaluate_cold_to_warm_big_ledger_promotions, evaluate_cold_to_warm_promotions,
    evaluate_forget_cold_peers, evaluate_forget_failed_peers, evaluate_hot_promotions,
    evaluate_hot_to_warm_big_ledger_demotions, evaluate_hot_to_warm_demotions,
    evaluate_peer_share_requests, evaluate_sensitive_hot_demotions,
    evaluate_sensitive_warm_demotions, evaluate_warm_to_cold_big_ledger_demotions,
    evaluate_warm_to_cold_demotions, evaluate_warm_to_hot_big_ledger_promotions,
    evaluate_warm_to_hot_promotions, fetch_mode_from_judgement, filter_sensitive_promotions,
    governor_tick, has_only_trustable_established_peers, hot_peers_remote,
    is_node_able_to_make_progress, peer_selection_mode, pick_churn_regime,
    requires_bootstrap_peers,
};

// -- PeerSharing client re-exports --------------------------------------------
pub use peersharing_client::{PeerSharingClient, PeerSharingClientError};

// -- PeerSharing server re-exports --------------------------------------------
pub use peersharing_server::{PeerSharingServer, PeerSharingServerError, PeerSharingServerRequest};

// -- Connection manager re-exports --------------------------------------------
pub use connection::{
    AbstractState, AcceptedConnectionsLimit, ConnStateId, ConnectionId, ConnectionManagerError,
    ConnectionState, ConnectionType, DataFlow, DemotedToColdRemoteTr, InboundGovernorCounters,
    InboundGovernorEvent, MaybeUnknown, OperationResult, Provenance, RemoteSt, ResponderCounters,
    TimeoutExpired, Transition, connection_state_to_counters, verify_abstract_transition,
};

// -- Inbound governor re-exports ----------------------------------------------
pub use inbound_governor::{
    InboundConnectionEntry, InboundGovernorAction, InboundGovernorState, verify_remote_transition,
};

// -- Connection manager decision engine re-exports ----------------------------
pub use connection_manager::{
    AcquireOutboundResult, CmAction, ConnectionEntry, ConnectionManagerState, ReleaseOutboundResult,
};

// -- Diffusion layer re-exports -----------------------------------------------
pub use diffusion::{
    ControlMessage, ErrorCommand, ErrorPolicyResult, MiniProtocolDescriptor, MiniProtocolLimits,
    MiniProtocolStart, MuxMode, OuroborosBundle, PeerConnectionHandle, PeerStateAction,
    ProtocolTemperature, RateLimitDecision, RepromoteDelay, RethrowPolicy, TemperatureBundle,
    ntc_ouroboros_bundle, ntn_ouroboros_bundle, rate_limit_decision,
};

// -- Peer state actions bridge re-exports ------------------------------------
pub use peer_state_actions::{
    PeerStateActions, governor_action_to_peer_state_action, governor_actions_to_peer_state_actions,
};

// -- NtC client driver re-exports ---------------------------------------------
pub use local_state_query_client::{LocalStateQueryClient, LocalStateQueryClientError};
pub use local_tx_monitor_client::{LocalTxMonitorClient, LocalTxMonitorClientError};
pub use local_tx_submission_client::{LocalTxSubmissionClient, LocalTxSubmissionClientError};
