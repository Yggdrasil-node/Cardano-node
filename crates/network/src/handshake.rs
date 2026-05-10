//! Handshake mini-protocol (umbrella module).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil parent shell aggregating
//! `handshake/{type,version,codec}.rs` (which mirror upstream
//! `Ouroboros.Network.Protocol.Handshake.{Type,Version,Codec}.hs`).
//! Upstream's `Ouroboros.Network.Protocol.Handshake` umbrella
//! module additionally carries the `runHandshakeClient` /
//! `runHandshakeServer` runtime drivers + `HandshakeArguments` /
//! `Versions` / `HandshakeException` types; in Yggdrasil that
//! runtime surface is folded into `peer.rs` (the per-peer
//! connection bring-up function), so this parent file is a pure
//! re-export aggregator without runtime API.

pub mod codec;
pub mod r#type;
pub mod version;
pub mod wire;

// Re-exports preserve the existing flat `crate::handshake::Foo` API for
// callers in the workspace; the actual definitions live in the per-concern
// leaves above. The `encode_version_data` / `decode_version_data` helpers
// remain `pub(super)` to `codec` — they are internal codec plumbing, not
// part of the public handshake API.
pub use r#type::{
    HandshakeMessage, HandshakeRequest, HandshakeState, HandshakeTransitionError, RefuseReason,
};
pub use version::{HandshakeVersion, NodeToNodeVersionData};
pub use wire::HandshakeWireCodec;
