//! Trace-forwarder handshake message codec — wraps the generic
//! Ouroboros handshake message envelope around the trace-forwarder-
//! specific [`ForwardingVersion`] + [`ForwardingVersionData`] types
//! from R432.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side specialization. Mirror
//! of upstream's `Handshake.codecHandshake forwardingVersionCodec`
//! + `Handshake.cborTermVersionDataCodec forwardingCodecCBORTerm`
//! pair (called from `Cardano.Tracer.Acceptors.Server` line 132-133).
//! Upstream's `codecHandshake` is parameterized over a
//! `CodecCBORTerm` for the version-tag type; Yggdrasil's port
//! avoids the typeclass machinery by inlining the trace-forwarder-
//! specific wire encoding here.
//!
//! Wire format mirrors upstream's `handshake-node-to-node-v14.cddl`
//! exactly (the same envelope is reused for trace-forwarder):
//!
//! | Tag | Wire shape                                     | Message                  |
//! |-----|------------------------------------------------|--------------------------|
//! |  0  | `[0, {versionTag → versionData, ...}]`        | ProposeVersions          |
//! |  1  | `[1, versionTag, versionData]`                 | AcceptVersion            |
//! |  2  | `[2, refuseReason]`                            | Refuse                   |
//! |  3  | `[3, {versionTag → versionData, ...}]`        | QueryReply               |
//!
//! Refuse-reason wire format (same as upstream NtN handshake):
//!
//! | Tag | Wire shape                                  | Reason variant         |
//! |-----|---------------------------------------------|------------------------|
//! |  0  | `[0, [versionTag, ...]]`                    | VersionMismatch        |
//! |  1  | `[1, versionTag, message]`                  | HandshakeDecodeError   |
//! |  2  | `[2, versionTag, message]`                  | Refused                |
//!
//! Where `versionTag` is the CBOR-encoded
//! [`ForwardingVersion::tag`] (1 or 2) and `versionData` is the
//! CBOR-encoded [`ForwardingVersionData::network_magic`] as a
//! single unsigned integer (per R432's
//! [`encode_forwarding_version_data`]).

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

use super::trace_object_forward_version::{ForwardingVersion, ForwardingVersionData};
use crate::handshake::wire::{self, HandshakeWireCodec};

/// Maximum number of (version, version-data) entries in a
/// trace-forwarder version table. The trace-forwarder only has 2
/// versions in practice; the bound is set generously to absorb
/// future expansion while still fending off a malicious peer
/// shipping a 2^32-entry table.
const TRACE_FORWARD_VERSION_TABLE_MAX: usize = 64;

/// Trace-forwarder handshake-codec impl of
/// [`HandshakeWireCodec`]. Plugs the
/// [`encode_forwarding_version`]-style per-tag (CBOR unsigned 1-2) +
/// per-version-data (CBOR unsigned u32 network-magic) wire encodings
/// into the generic version-table helpers from
/// [`crate::handshake::wire`].
pub struct TraceForwardHandshakeCodec;

impl HandshakeWireCodec for TraceForwardHandshakeCodec {
    type Version = ForwardingVersion;
    type VersionData = ForwardingVersionData;

    fn encode_version(enc: &mut Encoder, version: &Self::Version) {
        enc.unsigned(u64::from(version.tag()));
    }

    fn decode_version(dec: &mut Decoder<'_>) -> Result<Self::Version, LedgerError> {
        let tag = dec.unsigned()?;
        decode_version_tag(tag)
    }

    fn encode_version_data(enc: &mut Encoder, data: &Self::VersionData) {
        enc.unsigned(u64::from(data.network_magic));
    }

    fn decode_version_data(dec: &mut Decoder<'_>) -> Result<Self::VersionData, LedgerError> {
        let magic = dec.unsigned()?;
        decode_version_data(magic)
    }
}

// ---------------------------------------------------------------------------
// Message envelope
// ---------------------------------------------------------------------------

/// Trace-forwarder handshake messages. Mirror of upstream's
/// `Handshake (Forwarding lo) Term` message protocol with the
/// version-data Term slot specialized to
/// [`ForwardingVersionData`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceForwardHandshakeMessage {
    /// `[0, {ver → data, ...}]` — proposed version table.
    ProposeVersions(Vec<(ForwardingVersion, ForwardingVersionData)>),
    /// `[1, ver, data]` — accepted version + data.
    AcceptVersion(ForwardingVersion, ForwardingVersionData),
    /// `[2, refuseReason]` — refuse with reason.
    Refuse(TraceForwardRefuseReason),
    /// `[3, {ver → data, ...}]` — query-mode reply table.
    QueryReply(Vec<(ForwardingVersion, ForwardingVersionData)>),
}

/// Reasons a trace-forwarder handshake might be refused. Mirror of
/// upstream's `RefuseReason` ADT with [`ForwardingVersion`]
/// substituted for `vNumber`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceForwardRefuseReason {
    /// `[0, [ver, ...]]` — no overlap between local and remote
    /// version tables.
    VersionMismatch(Vec<ForwardingVersion>),
    /// `[1, ver, msg]` — the version-data CBOR for the agreed
    /// version failed to decode.
    HandshakeDecodeError(ForwardingVersion, String),
    /// `[2, ver, msg]` — the version was acceptable but the
    /// data was rejected (e.g. mismatched network-magic).
    Refused(ForwardingVersion, String),
}

// ---------------------------------------------------------------------------
// Wire codec
// ---------------------------------------------------------------------------

impl TraceForwardHandshakeMessage {
    /// Encode this handshake message to CBOR bytes.
    ///
    /// Wire format (matching upstream `codecHandshake
    /// forwardingVersionCodec`):
    /// - `ProposeVersions` → `[0, {ver → data}]`
    /// - `AcceptVersion`   → `[1, ver, data]`
    /// - `Refuse`          → `[2, refuseReason]`
    /// - `QueryReply`      → `[3, {ver → data}]`
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::ProposeVersions(versions) => {
                enc.array(2).unsigned(0);
                encode_version_table(&mut enc, versions);
            }
            Self::AcceptVersion(ver, data) => {
                enc.array(3).unsigned(1).unsigned(u64::from(ver.tag()));
                enc.unsigned(u64::from(data.network_magic));
            }
            Self::Refuse(reason) => {
                enc.array(2).unsigned(2);
                encode_refuse_reason(&mut enc, reason);
            }
            Self::QueryReply(versions) => {
                enc.array(2).unsigned(3);
                encode_version_table(&mut enc, versions);
            }
        }
        enc.into_bytes()
    }

    /// Decode a handshake message from CBOR bytes.
    ///
    /// Returns [`LedgerError::CborTypeMismatch`] for an unknown
    /// outer tag, [`LedgerError::CborTrailingBytes`] if the input
    /// has extra bytes after the message body.
    pub fn from_cbor(data: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(data);
        let arr_len = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, arr_len) {
            (0, 2) => {
                let versions = decode_version_table(&mut dec)?;
                Self::ProposeVersions(versions)
            }
            (1, 3) => {
                let ver_tag = dec.unsigned()?;
                let version = decode_version_tag(ver_tag)?;
                let magic = dec.unsigned()?;
                let data = decode_version_data(magic)?;
                Self::AcceptVersion(version, data)
            }
            (2, 2) => {
                let reason = decode_refuse_reason(&mut dec)?;
                Self::Refuse(reason)
            }
            (3, 2) => {
                let versions = decode_version_table(&mut dec)?;
                Self::QueryReply(versions)
            }
            _ => {
                return Err(LedgerError::CborTypeMismatch {
                    expected: 0,
                    actual: tag as u8,
                });
            }
        };
        if !dec.is_empty() {
            return Err(LedgerError::CborTrailingBytes(dec.remaining()));
        }
        Ok(msg)
    }
}

// ---------------------------------------------------------------------------
// Version-table helpers — R434: route through generic helpers
// ---------------------------------------------------------------------------

fn encode_version_table(
    enc: &mut Encoder,
    versions: &[(ForwardingVersion, ForwardingVersionData)],
) {
    wire::encode_version_table::<TraceForwardHandshakeCodec>(enc, versions);
}

fn decode_version_table(
    dec: &mut Decoder<'_>,
) -> Result<Vec<(ForwardingVersion, ForwardingVersionData)>, LedgerError> {
    wire::decode_version_table::<TraceForwardHandshakeCodec>(dec, TRACE_FORWARD_VERSION_TABLE_MAX)
}

fn decode_version_tag(tag: u64) -> Result<ForwardingVersion, LedgerError> {
    match tag {
        1 => Ok(ForwardingVersion::V1),
        2 => Ok(ForwardingVersion::V2),
        other => Err(LedgerError::CborDecodeError(format!(
            "decode ForwardingVersion: unknown tag: {other}"
        ))),
    }
}

fn decode_version_data(magic: u64) -> Result<ForwardingVersionData, LedgerError> {
    if magic > 0xffff_ffff {
        return Err(LedgerError::CborDecodeError(format!(
            "networkMagic out of bound: {magic}"
        )));
    }
    Ok(ForwardingVersionData {
        network_magic: magic as u32,
    })
}

// ---------------------------------------------------------------------------
// Refuse-reason helpers
// ---------------------------------------------------------------------------

fn encode_refuse_reason(enc: &mut Encoder, reason: &TraceForwardRefuseReason) {
    match reason {
        TraceForwardRefuseReason::VersionMismatch(vs) => {
            enc.array(2).unsigned(0);
            enc.array(vs.len() as u64);
            for v in vs {
                enc.unsigned(u64::from(v.tag()));
            }
        }
        TraceForwardRefuseReason::HandshakeDecodeError(ver, msg) => {
            enc.array(3)
                .unsigned(1)
                .unsigned(u64::from(ver.tag()))
                .text(msg);
        }
        TraceForwardRefuseReason::Refused(ver, msg) => {
            enc.array(3)
                .unsigned(2)
                .unsigned(u64::from(ver.tag()))
                .text(msg);
        }
    }
}

fn decode_refuse_reason(dec: &mut Decoder<'_>) -> Result<TraceForwardRefuseReason, LedgerError> {
    let inner_len = dec.array()?;
    let inner_tag = dec.unsigned()?;
    match (inner_tag, inner_len) {
        (0, 2) => {
            let count = dec.array()?;
            let cap = count.min(64) as usize;
            let mut vs = Vec::with_capacity(cap);
            for _ in 0..count {
                let ver_tag = dec.unsigned()?;
                vs.push(decode_version_tag(ver_tag)?);
            }
            Ok(TraceForwardRefuseReason::VersionMismatch(vs))
        }
        (1, 3) => {
            let ver_tag = dec.unsigned()?;
            let version = decode_version_tag(ver_tag)?;
            let msg = dec.text()?.to_owned();
            Ok(TraceForwardRefuseReason::HandshakeDecodeError(version, msg))
        }
        (2, 3) => {
            let ver_tag = dec.unsigned()?;
            let version = decode_version_tag(ver_tag)?;
            let msg = dec.text()?.to_owned();
            Ok(TraceForwardRefuseReason::Refused(version, msg))
        }
        _ => Err(LedgerError::CborTypeMismatch {
            expected: 0,
            actual: inner_tag as u8,
        }),
    }
}

// ---------------------------------------------------------------------------
// Operator helper — simple-singleton-versions
// ---------------------------------------------------------------------------

/// Build a [`TraceForwardHandshakeMessage::ProposeVersions`]
/// carrying a single (version, data) pair. Mirror of upstream's
/// `Handshake.simpleSingletonVersions ForwardingV_1
/// (ForwardingVersionData $ NetworkMagic netMagic) (\_ -> ...)`.
///
/// The lambda continuation upstream passes is the responder
/// application; Yggdrasil's port doesn't carry that here since
/// the handshake codec is decoupled from the application.
pub fn simple_singleton_versions(
    version: ForwardingVersion,
    data: ForwardingVersionData,
) -> TraceForwardHandshakeMessage {
    TraceForwardHandshakeMessage::ProposeVersions(vec![(version, data)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_for(magic: u32) -> ForwardingVersionData {
        ForwardingVersionData {
            network_magic: magic,
        }
    }

    #[test]
    fn propose_versions_single_v1_round_trips() {
        let msg = TraceForwardHandshakeMessage::ProposeVersions(vec![(
            ForwardingVersion::V1,
            data_for(764824073),
        )]);
        let bytes = msg.to_cbor();
        let decoded = TraceForwardHandshakeMessage::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn propose_versions_full_table_round_trips() {
        let msg = TraceForwardHandshakeMessage::ProposeVersions(vec![
            (ForwardingVersion::V1, data_for(1)),
            (ForwardingVersion::V2, data_for(2)),
        ]);
        let bytes = msg.to_cbor();
        let decoded = TraceForwardHandshakeMessage::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn accept_version_round_trips() {
        let msg =
            TraceForwardHandshakeMessage::AcceptVersion(ForwardingVersion::V1, data_for(764824073));
        let bytes = msg.to_cbor();
        let decoded = TraceForwardHandshakeMessage::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn refuse_version_mismatch_round_trips() {
        let msg =
            TraceForwardHandshakeMessage::Refuse(TraceForwardRefuseReason::VersionMismatch(vec![
                ForwardingVersion::V1,
                ForwardingVersion::V2,
            ]));
        let bytes = msg.to_cbor();
        let decoded = TraceForwardHandshakeMessage::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn refuse_handshake_decode_error_round_trips() {
        let msg =
            TraceForwardHandshakeMessage::Refuse(TraceForwardRefuseReason::HandshakeDecodeError(
                ForwardingVersion::V1,
                "bad data".to_string(),
            ));
        let bytes = msg.to_cbor();
        let decoded = TraceForwardHandshakeMessage::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn refuse_refused_round_trips() {
        let msg = TraceForwardHandshakeMessage::Refuse(TraceForwardRefuseReason::Refused(
            ForwardingVersion::V2,
            "magic mismatch".to_string(),
        ));
        let bytes = msg.to_cbor();
        let decoded = TraceForwardHandshakeMessage::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn query_reply_round_trips() {
        let msg = TraceForwardHandshakeMessage::QueryReply(vec![
            (ForwardingVersion::V1, data_for(764824073)),
            (ForwardingVersion::V2, data_for(764824073)),
        ]);
        let bytes = msg.to_cbor();
        let decoded = TraceForwardHandshakeMessage::from_cbor(&bytes).expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn simple_singleton_versions_builds_propose_with_one_entry() {
        let msg = simple_singleton_versions(ForwardingVersion::V1, data_for(42));
        match msg {
            TraceForwardHandshakeMessage::ProposeVersions(entries) => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].0, ForwardingVersion::V1);
                assert_eq!(entries[0].1.network_magic, 42);
            }
            other => panic!("expected ProposeVersions, got {other:?}"),
        }
    }

    #[test]
    fn from_cbor_unknown_outer_tag_errors() {
        // [9, x] — unknown outer tag 9.
        let bytes = vec![0x82, 0x09, 0x00];
        let result = TraceForwardHandshakeMessage::from_cbor(&bytes);
        assert!(matches!(result, Err(LedgerError::CborTypeMismatch { .. })));
    }

    #[test]
    fn from_cbor_unknown_version_tag_in_propose_errors() {
        // ProposeVersions with version tag 99 (unknown).
        // Wire bytes: [0, {99: <magic>}]
        let mut enc = Encoder::new();
        enc.array(2).unsigned(0);
        enc.map(1).unsigned(99).unsigned(1);
        let bytes = enc.into_bytes();
        let result = TraceForwardHandshakeMessage::from_cbor(&bytes);
        match result {
            Err(LedgerError::CborDecodeError(msg)) => {
                assert!(msg.contains("unknown tag: 99"));
            }
            other => panic!("expected CborDecodeError, got {other:?}"),
        }
    }

    #[test]
    fn from_cbor_out_of_bound_magic_errors() {
        // ProposeVersions with version 1, magic 2^32 (out of u32 bound).
        let mut enc = Encoder::new();
        enc.array(2).unsigned(0);
        enc.map(1).unsigned(1).unsigned(0x1_0000_0000);
        let bytes = enc.into_bytes();
        let result = TraceForwardHandshakeMessage::from_cbor(&bytes);
        match result {
            Err(LedgerError::CborDecodeError(msg)) => {
                assert!(msg.contains("networkMagic out of bound"));
            }
            other => panic!("expected CborDecodeError, got {other:?}"),
        }
    }

    #[test]
    fn from_cbor_trailing_bytes_errors() {
        // A valid message with extra trailing 0x00 byte.
        let valid = TraceForwardHandshakeMessage::AcceptVersion(ForwardingVersion::V1, data_for(1));
        let mut bytes = valid.to_cbor();
        bytes.push(0x00);
        let result = TraceForwardHandshakeMessage::from_cbor(&bytes);
        assert!(matches!(result, Err(LedgerError::CborTrailingBytes(_))));
    }

    #[test]
    fn propose_versions_wire_format_byte_stable() {
        // ProposeVersions singleton with V1 + magic 1.
        // Wire bytes:
        //   0x82           array(2)
        //   0x00           unsigned 0       (outer tag)
        //   0xA1           map(1)           (version table)
        //   0x01           unsigned 1       (version tag)
        //   0x01           unsigned 1       (network magic)
        let msg = simple_singleton_versions(ForwardingVersion::V1, data_for(1));
        assert_eq!(msg.to_cbor(), vec![0x82, 0x00, 0xA1, 0x01, 0x01]);
    }

    #[test]
    fn accept_version_wire_format_byte_stable() {
        // AcceptVersion V1, magic 1:
        //   0x83 0x01 0x01 0x01 — array(3), unsigned(1), unsigned(1), unsigned(1)
        let msg = TraceForwardHandshakeMessage::AcceptVersion(ForwardingVersion::V1, data_for(1));
        assert_eq!(msg.to_cbor(), vec![0x83, 0x01, 0x01, 0x01]);
    }
}
