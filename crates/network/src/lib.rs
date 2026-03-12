//! Networking-facing types for handshake, multiplexing, and mini-protocol state.

/// Handshake version and request types.
pub mod handshake;
/// Multiplexer channel helpers.
pub mod multiplexer;
/// Mini-protocol state modules.
pub mod protocols;

/// Handshake request and negotiated version wrappers.
pub use handshake::{HandshakeRequest, HandshakeVersion};
/// Multiplexer channel identifier.
pub use multiplexer::MuxChannel;
/// Exported ChainSync and BlockFetch protocol states.
pub use protocols::{BlockFetchState, ChainSyncState};
