//! Networking types for Ouroboros handshake, multiplexing, and mini-protocol
//! state machines.
//!
//! This crate models the node-to-node protocol surface defined by the
//! [Ouroboros network specifications](https://github.com/IntersectMBO/ouroboros-network/).

/// Async multiplexer bearer — transport abstraction for SDU-framed I/O.
pub mod bearer;
/// Handshake negotiation types and state machine.
pub mod handshake;
/// Multiplexer / demultiplexer — SDU routing between bearer and protocol channels.
pub mod mux;
/// Multiplexer framing, SDU header, and protocol numbering.
pub mod multiplexer;
/// Mini-protocol state machine modules.
pub mod protocols;

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
pub use mux::{MuxError, MuxHandle, ProtocolHandle, start as start_mux};

// -- Protocol re-exports ------------------------------------------------------
pub use protocols::{
    BlockFetchMessage, BlockFetchState, BlockFetchTransitionError, ChainRange,
    ChainSyncMessage, ChainSyncState, ChainSyncTransitionError,
    KeepAliveMessage, KeepAliveState, KeepAliveTransitionError,
    TxIdAndSize, TxSubmissionMessage, TxSubmissionState, TxSubmissionTransitionError,
};
