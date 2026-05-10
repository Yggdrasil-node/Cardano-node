//! Generic wire helpers for the Ouroboros handshake mini-protocol —
//! shared across NodeToNode (NtN) and trace-forwarder variants.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis. Mirror of
//! upstream's `Ouroboros.Network.Protocol.Handshake.Codec.codecHandshake`
//! genericity over a `CodecCBORTerm` for the version-tag type.
//!
//! Upstream's `codecHandshake forwardingVersionCodec` (Server.hs:132)
//! and the equivalent `codecHandshake nodeToNodeVersionCodec` are
//! the same function specialized over different per-version
//! codecs. Yggdrasil collapses that genericity into a
//! [`HandshakeWireCodec`] trait that abstracts the per-entry
//! version + version-data encoding/decoding; the version-table
//! and refuse-reason structural layout is shared verbatim across
//! all callers.
//!
//! Used by:
//! - `crates/network/src/handshake/codec.rs` (NtN handshake;
//!   [`NtNHandshakeCodec`] impl).
//! - `crates/network/src/protocols/trace_object_forward_handshake.rs`
//!   (trace-forwarder handshake; `TraceForwardHandshakeCodec` impl
//!   in that file).

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder, vec_with_strict_capacity};

/// Trait abstracting the per-entry (version, version-data) wire
/// encoding for a handshake variant. Implementors plug in the
/// CBOR-shape-specific encoder/decoder pair; the structural
/// version-table + refuse-reason helpers in this module dispatch
/// through the trait.
pub trait HandshakeWireCodec {
    /// The version-tag type (e.g. `HandshakeVersion(u16)` for NtN,
    /// `ForwardingVersion` for trace-forwarder).
    type Version;
    /// The version-data payload type carried alongside the version
    /// tag (e.g. `NodeToNodeVersionData` for NtN,
    /// `ForwardingVersionData` for trace-forwarder).
    type VersionData;

    /// Encode the version tag to CBOR. Mirror of upstream's
    /// `encodeTerm` for the per-variant `forwardingVersionCodec` /
    /// `nodeToNodeVersionCodec`.
    fn encode_version(enc: &mut Encoder, version: &Self::Version);

    /// Decode the version tag from CBOR. Mirror of upstream's
    /// `decodeTerm`.
    fn decode_version(dec: &mut Decoder<'_>) -> Result<Self::Version, LedgerError>;

    /// Encode the version-data payload to CBOR. Mirror of
    /// upstream's `cborTermVersionDataCodec`'s `encodeTerm`.
    fn encode_version_data(enc: &mut Encoder, data: &Self::VersionData);

    /// Decode the version-data payload from CBOR. Mirror of
    /// upstream's `cborTermVersionDataCodec`'s `decodeTerm`.
    fn decode_version_data(dec: &mut Decoder<'_>) -> Result<Self::VersionData, LedgerError>;
}

// ---------------------------------------------------------------------------
// Version table — shared structural layout across all variants
// ---------------------------------------------------------------------------

/// Encode a version table as a CBOR map: `{version: versionData, ...}`.
/// Mirror of upstream's version-table encoding inside
/// `codecHandshake`'s `MsgProposeVersions` / `MsgQueryReply` arms.
pub fn encode_version_table<C: HandshakeWireCodec>(
    enc: &mut Encoder,
    versions: &[(C::Version, C::VersionData)],
) {
    enc.map(versions.len() as u64);
    for (ver, data) in versions {
        C::encode_version(enc, ver);
        C::encode_version_data(enc, data);
    }
}

/// Result type for [`decode_version_table`] — alias to dodge
/// clippy's `type_complexity` lint on the underlying generic
/// `Result<Vec<(...)>, _>` shape.
pub type DecodeVersionTableResult<C> = Result<
    Vec<(
        <C as HandshakeWireCodec>::Version,
        <C as HandshakeWireCodec>::VersionData,
    )>,
    LedgerError,
>;

/// Decode a version table from a CBOR map. Bounded by `max` to
/// guard against a malicious peer shipping an over-large table.
pub fn decode_version_table<C: HandshakeWireCodec>(
    dec: &mut Decoder<'_>,
    max: usize,
) -> DecodeVersionTableResult<C> {
    let count = dec.map()?;
    let mut versions = vec_with_strict_capacity(count, max)?;
    for _ in 0..count {
        let ver = C::decode_version(dec)?;
        let data = C::decode_version_data(dec)?;
        versions.push((ver, data));
    }
    Ok(versions)
}
