//! Networking types for Ouroboros handshake, multiplexing, and mini-protocol
//! state machines.
//!
//! This crate models the node-to-node protocol surface defined by the
//! [Ouroboros network specifications](https://github.com/IntersectMBO/ouroboros-network/).

/// Async multiplexer bearer — transport abstraction for SDU-framed I/O.
pub mod bearer;
/// BlockFetch client driver — typed, state-machine-correct protocol loop.
pub mod blockfetch_client;
/// ChainSync client driver — typed, state-machine-correct protocol loop.
pub mod chainsync_client;
/// Handshake negotiation types and state machine.
pub mod handshake;
/// KeepAlive client driver — typed, state-machine-correct protocol loop.
pub mod keepalive_client;
/// Refresh-oriented provider interfaces for time-varying ledger peers.
pub mod ledger_peers_provider;
/// TCP listener for inbound peer connections.
pub mod listener;
/// Multiplexer / demultiplexer — SDU routing between bearer and protocol channels.
pub mod mux;
/// Multiplexer framing, SDU header, and protocol numbering.
pub mod multiplexer;
/// Peer connection lifecycle — handshake negotiation and data-protocol setup.
pub mod peer;
/// Peer registry state and source/status tracking.
pub mod peer_registry;
/// Topology root-peer domain types and resolved provider snapshots.
pub mod root_peers;
/// Refresh-oriented provider interfaces for time-varying root peers.
pub mod root_peers_provider;
/// Peer candidate resolution and ordering helpers for runtime bootstrap.
pub mod peer_selection;
/// Mini-protocol state machine modules.
pub mod protocols;
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

/// PeerSharing client driver — typed, state-machine-correct protocol loop.
pub mod peersharing_client;

/// PeerSharing server driver — typed, state-machine-correct responder loop.
pub mod peersharing_server;

// -- Bearer re-exports --------------------------------------------------------
pub use bearer::{Bearer, BearerError, Sdu, TcpBearer, MAX_SDU_PAYLOAD};

// -- Handshake re-exports -----------------------------------------------------
pub use handshake::{
    HandshakeMessage, HandshakeRequest, HandshakeState, HandshakeTransitionError,
    HandshakeVersion, NodeToNodeVersionData, RefuseReason,
};

// -- Multiplexer re-exports ---------------------------------------------------
pub use multiplexer::{
    MiniProtocolDir, MiniProtocolNum, MuxChannel, SduDecodeError, SduHeader, SDU_HEADER_SIZE,
};

// -- Mux re-exports -----------------------------------------------------------
pub use mux::{MessageChannel, MuxError, MuxHandle, ProtocolHandle, start as start_mux, MAX_SEGMENT_SIZE};

// -- Peer re-exports ----------------------------------------------------------
pub use peer::{PeerConnection, PeerError, connect as peer_connect, accept as peer_accept};
pub use listener::{PeerListener, PeerListenerError};
pub use peer_registry::{
    PeerRegistry, PeerRegistryEntry, PeerRegistryStatusCounts, PeerSource,
    PeerStatus,
};
pub use root_peers::{
    AfterSlot, ResolvedLocalRootGroup, RootPeerProviderState, RootPeerProviders,
    TopologyConfig, UseBootstrapPeers, UseLedgerPeers,
    reconcile_root_peer_providers, resolve_root_peer_providers,
};
pub use root_peers_provider::{
    DnsRefreshPolicy, DnsRootPeerProvider, DnsRootPeerProviderConfig,
    RootPeerProvider, RootPeerProviderError, RootPeerProviderKind,
    RootPeerProviderRefresh, ScriptedRootPeerProvider,
    refresh_root_peer_state, refresh_root_peer_state_and_registry,
};
pub use ledger_peers_provider::{
    LedgerPeerRegistryUpdate, LedgerPeerUseDecision, LedgerStateJudgement,
    PeerSnapshotFreshness,
    LedgerPeerProvider, LedgerPeerProviderError, LedgerPeerProviderKind,
    LedgerPeerProviderRefresh, LedgerPeerSnapshot, ScriptedLedgerPeerProvider,
    apply_ledger_peer_refresh, judge_ledger_peer_usage,
    reconcile_ledger_peer_registry_with_policy, refresh_ledger_peer_registry,
};
pub use peer_selection::{
    LocalRootConfig, PeerAccessPoint, PeerAttemptState, PeerBootstrapTargets,
    PeerDiffusionMode, PublicRootConfig,
    bootstrap_targets, ordered_fallback_peers as ordered_peer_fallbacks,
    ordered_peer_candidates, peer_attempt_state, resolve_peer_access_point,
    resolve_peer_access_points,
};

// -- Protocol re-exports ------------------------------------------------------
pub use protocols::{
    BlockFetchMessage, BlockFetchState, BlockFetchTransitionError, ChainRange,
    ChainSyncMessage, ChainSyncState, ChainSyncTransitionError,
    KeepAliveMessage, KeepAliveState, KeepAliveTransitionError,
    PeerSharingMessage, PeerSharingState, PeerSharingTransitionError, SharedPeerAddress,
    TxIdAndSize, TxSubmissionMessage, TxSubmissionState, TxSubmissionTransitionError,
};

// -- ChainSync client re-exports ----------------------------------------------
pub use chainsync_client::{
    ChainSyncClient, ChainSyncClientError, IntersectResponse, NextResponse,
    DecodedHeaderNextResponse, TypedIntersectResponse, TypedNextResponse,
};

// -- BlockFetch client re-exports ---------------------------------------------
pub use blockfetch_client::{
    BatchResponse, BlockFetchClient, BlockFetchClientError,
};

// -- KeepAlive client re-exports ----------------------------------------------
pub use keepalive_client::{
    KeepAliveClient, KeepAliveClientError,
};

// -- TxSubmission client re-exports -------------------------------------------
pub use txsubmission_client::{
    TxServerRequest, TxSubmissionClient, TxSubmissionClientError,
};

// -- BlockFetch server re-exports ---------------------------------------------
pub use blockfetch_server::{
    BlockFetchServer, BlockFetchServerError, BlockFetchServerRequest,
};

// -- ChainSync server re-exports ----------------------------------------------
pub use chainsync_server::{
    ChainSyncServer, ChainSyncServerError, ChainSyncServerRequest,
};

// -- KeepAlive server re-exports ----------------------------------------------
pub use keepalive_server::{
    KeepAliveServer, KeepAliveServerError,
};

// -- TxSubmission server re-exports -------------------------------------------
pub use txsubmission_server::{
    TxIdsReply, TxSubmissionServer, TxSubmissionServerError,
};

// -- Governor re-exports ------------------------------------------------------
pub use governor::{
    ChurnConfig, GovernorAction, GovernorState, GovernorTargets, LocalRootTargets,
    enforce_local_root_valency, evaluate_cold_to_warm_promotions,
    evaluate_hot_to_warm_demotions, evaluate_warm_to_cold_demotions,
    evaluate_warm_to_hot_promotions, governor_tick,
};

// -- PeerSharing client re-exports --------------------------------------------
pub use peersharing_client::{PeerSharingClient, PeerSharingClientError};

// -- PeerSharing server re-exports --------------------------------------------
pub use peersharing_server::{
    PeerSharingServer, PeerSharingServerError, PeerSharingServerRequest,
};
