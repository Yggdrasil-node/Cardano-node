//! Networking types for Ouroboros handshake, multiplexing, and mini-protocol
//! state machines.
//!
//! This crate models the node-to-node protocol surface defined by the
//! [Ouroboros network specifications](https://github.com/IntersectMBO/ouroboros-network/).

/// Handshake negotiation types and state machine.
pub mod handshake;
/// Multiplexer framing, SDU header, and protocol numbering.
pub mod multiplexer;
/// Mini-protocol state machine modules.
pub mod protocols;

// -- Handshake re-exports -----------------------------------------------------
pub use handshake::{
    HandshakeMessage, HandshakeRequest, HandshakeState, HandshakeTransitionError,
    HandshakeVersion, NodeToNodeVersionData, RefuseReason,
};

// -- Multiplexer re-exports ---------------------------------------------------
pub use multiplexer::{
    MiniProtocolDir, MiniProtocolNum, MuxChannel, SduDecodeError, SduHeader, SDU_HEADER_SIZE,
};

// -- Protocol re-exports ------------------------------------------------------
pub use protocols::{
    BlockFetchMessage, BlockFetchState, BlockFetchTransitionError, ChainRange,
    ChainSyncMessage, ChainSyncState, ChainSyncTransitionError,
    KeepAliveMessage, KeepAliveState, KeepAliveTransitionError,
};
