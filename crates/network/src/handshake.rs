/// A network protocol version number used during handshake negotiation.
///
/// Node-to-node versions 14 and 15 are currently defined.
///
/// Reference: `handshake-node-to-node-v14.cddl` — `versionNumber_v14 = 14 / 15`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HandshakeVersion(pub u16);

impl HandshakeVersion {
    /// Node-to-node protocol version 13 (Conway / PeerSharing).
    pub const V13: Self = Self(13);
    /// Node-to-node protocol version 14.
    pub const V14: Self = Self(14);
    /// Node-to-node protocol version 15.
    pub const V15: Self = Self(15);
}

// ---------------------------------------------------------------------------
// Version data negotiated alongside the version number
// ---------------------------------------------------------------------------

/// Per-version parameters exchanged during the node-to-node handshake.
///
/// Reference: `node-to-node-version-data-v14.cddl` —
/// `[networkMagic, initiatorOnlyDiffusionMode, peerSharing, query]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeToNodeVersionData {
    /// Network discriminator (e.g. `764824073` for mainnet).
    pub network_magic: u32,
    /// When `true` the initiator will not act as a responder on this
    /// connection.
    pub initiator_only_diffusion_mode: bool,
    /// Peer-sharing willingness indicator: `0` = disabled, `1` = enabled.
    pub peer_sharing: u8,
    /// When `true` the handshake is a version query only; the connection will
    /// be closed after the server replies.
    pub query: bool,
}

// ---------------------------------------------------------------------------
// Handshake message envelope
// ---------------------------------------------------------------------------

/// Messages of the Handshake mini-protocol.
///
/// Wire tags match the upstream CDDL:
/// - `0` → `ProposeVersions`
/// - `1` → `AcceptVersion`
/// - `2` → `Refuse`
/// - `3` → `QueryReply`
///
/// Reference: `handshake-node-to-node-v14.cddl`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandshakeMessage {
    /// `[0, versionTable]` — client proposes a set of acceptable versions.
    ProposeVersions(Vec<(HandshakeVersion, NodeToNodeVersionData)>),
    /// `[1, versionNumber, versionData]` — server accepts a version.
    AcceptVersion(HandshakeVersion, NodeToNodeVersionData),
    /// `[2, refuseReason]` — server refuses the handshake.
    Refuse(RefuseReason),
    /// `[3, versionTable]` — server replies to a query-only handshake.
    QueryReply(Vec<(HandshakeVersion, NodeToNodeVersionData)>),
}

/// Reason the server refused a handshake.
///
/// Reference: `handshake-node-to-node-v14.cddl` — `refuseReason`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefuseReason {
    /// `[0, [*versionNumber]]` — none of the proposed versions are acceptable.
    VersionMismatch(Vec<HandshakeVersion>),
    /// `[1, versionNumber, tstr]` — version data could not be decoded.
    HandshakeDecodeError(HandshakeVersion, String),
    /// `[2, versionNumber, tstr]` — server refuses the connection for the
    /// given version with a human-readable reason.
    Refused(HandshakeVersion, String),
}

impl std::fmt::Display for RefuseReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VersionMismatch(accepted) => {
                write!(f, "version mismatch — peer accepts [")?;
                for (i, v) in accepted.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v.0)?;
                }
                write!(f, "]")
            }
            Self::HandshakeDecodeError(version, reason) => {
                write!(
                    f,
                    "handshake version data for version {} failed to decode: {reason}",
                    version.0,
                )
            }
            Self::Refused(version, reason) => {
                write!(f, "refused version {}: {reason}", version.0)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Handshake state machine
// ---------------------------------------------------------------------------

/// States of the Handshake mini-protocol state machine.
///
/// ```text
///  ┌───────────┐  MsgProposeVersions ┌───────────┐
///  │ StPropose │ ──────────────────► │ StConfirm │
///  └───────────┘                     └───────────┘
///                                        │
///                    MsgAcceptVersion /  │
///                    MsgRefuse /         │
///                    MsgQueryReply       │
///                                        ▼
///                                   ┌─────────┐
///                                   │ StDone  │
///                                   └─────────┘
/// ```
///
/// - `StPropose` — client agency: must send `ProposeVersions`.
/// - `StConfirm` — server agency: must reply with `AcceptVersion`, `Refuse`,
///   or `QueryReply`.
/// - `StDone` — terminal, no further messages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakeState {
    /// Client must propose versions.
    StPropose,
    /// Server must respond.
    StConfirm,
    /// Terminal state.
    StDone,
}

// ---------------------------------------------------------------------------
// Legacy convenience wrapper (preserved from scaffold)
// ---------------------------------------------------------------------------

/// A minimal handshake request carrying network magic and version.
///
/// This is a simplified view used before the full handshake state machine is
/// exercised. Prefer [`HandshakeMessage::ProposeVersions`] for protocol work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandshakeRequest {
    pub network_magic: u32,
    pub version: HandshakeVersion,
}

// ---------------------------------------------------------------------------
// State transitions
// ---------------------------------------------------------------------------

/// Errors arising from illegal Handshake state transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HandshakeTransitionError {
    /// A message was received that is not legal in the current state.
    #[error("illegal handshake transition from {from:?} via {msg_tag}")]
    IllegalTransition {
        from: HandshakeState,
        msg_tag: &'static str,
    },
}

impl HandshakeState {
    /// Computes the next state given an incoming message.
    pub fn transition(self, msg: &HandshakeMessage) -> Result<Self, HandshakeTransitionError> {
        match (self, msg) {
            (Self::StPropose, HandshakeMessage::ProposeVersions(_)) => Ok(Self::StConfirm),
            (Self::StConfirm, HandshakeMessage::AcceptVersion(..)) => Ok(Self::StDone),
            (Self::StConfirm, HandshakeMessage::Refuse(_)) => Ok(Self::StDone),
            (Self::StConfirm, HandshakeMessage::QueryReply(_)) => Ok(Self::StDone),
            (from, msg) => Err(HandshakeTransitionError::IllegalTransition {
                from,
                msg_tag: msg.tag_name(),
            }),
        }
    }
}

impl HandshakeMessage {
    /// Human-readable tag name used in error messages.
    pub fn tag_name(&self) -> &'static str {
        match self {
            Self::ProposeVersions(_) => "ProposeVersions",
            Self::AcceptVersion(..) => "AcceptVersion",
            Self::Refuse(_) => "Refuse",
            Self::QueryReply(_) => "QueryReply",
        }
    }

    /// The CDDL wire tag for this message variant.
    pub fn wire_tag(&self) -> u8 {
        match self {
            Self::ProposeVersions(_) => 0,
            Self::AcceptVersion(..) => 1,
            Self::Refuse(_) => 2,
            Self::QueryReply(_) => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// CBOR wire codec
// ---------------------------------------------------------------------------

use crate::protocol_size_limits::handshake as handshake_limits;
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder, vec_with_strict_capacity};

/// Encode a version data value as a CBOR array:
/// `[networkMagic, initiatorOnlyDiffusionMode, peerSharing, query]`.
fn encode_version_data(enc: &mut Encoder, vd: &NodeToNodeVersionData) {
    enc.array(4)
        .unsigned(u64::from(vd.network_magic))
        .bool(vd.initiator_only_diffusion_mode)
        .unsigned(u64::from(vd.peer_sharing))
        .bool(vd.query);
}

/// Decode a version data value from a CBOR array.
///
/// The array length varies by protocol version:
/// - V7–V10: `[networkMagic, initiatorOnlyDiffusionMode]` (2 elements)
/// - V11–V12: `[networkMagic, initiatorOnlyDiffusionMode, peerSharing]` (3 elements)
/// - V13+:    `[networkMagic, initiatorOnlyDiffusionMode, peerSharing, query]` (4 elements)
///
/// Missing fields default to `peer_sharing = 0` (disabled) and `query = false`.
fn decode_version_data(dec: &mut Decoder<'_>) -> Result<NodeToNodeVersionData, LedgerError> {
    let len = dec.array()?;
    if !(2..=4).contains(&len) {
        return Err(LedgerError::CborInvalidLength {
            expected: 4,
            actual: len as usize,
        });
    }
    let network_magic = dec.unsigned()? as u32;
    let initiator_only_diffusion_mode = dec.bool()?;
    let peer_sharing = if len >= 3 { dec.unsigned()? as u8 } else { 0 };
    let query = if len >= 4 { dec.bool()? } else { false };
    Ok(NodeToNodeVersionData {
        network_magic,
        initiator_only_diffusion_mode,
        peer_sharing,
        query,
    })
}

/// Encode a version table as a CBOR map: `{version: versionData, ...}`.
fn encode_version_table(enc: &mut Encoder, versions: &[(HandshakeVersion, NodeToNodeVersionData)]) {
    enc.map(versions.len() as u64);
    for (ver, vd) in versions {
        enc.unsigned(u64::from(ver.0));
        encode_version_data(enc, vd);
    }
}

/// Decode a version table from a CBOR map.
fn decode_version_table(
    dec: &mut Decoder<'_>,
) -> Result<Vec<(HandshakeVersion, NodeToNodeVersionData)>, LedgerError> {
    let count = dec.map()?;
    let mut versions = vec_with_strict_capacity(count, handshake_limits::VERSION_TABLE_MAX)?;
    for _ in 0..count {
        let ver_num = dec.unsigned()? as u16;
        let vd = decode_version_data(dec)?;
        versions.push((HandshakeVersion(ver_num), vd));
    }
    Ok(versions)
}

impl HandshakeMessage {
    /// Encode this message to CBOR bytes.
    ///
    /// Wire format (matching upstream `handshake-node-to-node-v14.cddl`):
    /// - `[0, versionTable]` — ProposeVersions
    /// - `[1, versionNumber, versionData]` — AcceptVersion
    /// - `[2, refuseReason]` — Refuse
    /// - `[3, versionTable]` — QueryReply
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::ProposeVersions(versions) => {
                enc.array(2).unsigned(0);
                encode_version_table(&mut enc, versions);
            }
            Self::AcceptVersion(ver, vd) => {
                enc.array(3).unsigned(1).unsigned(u64::from(ver.0));
                encode_version_data(&mut enc, vd);
            }
            Self::Refuse(reason) => {
                enc.array(2).unsigned(2);
                match reason {
                    RefuseReason::VersionMismatch(vs) => {
                        enc.array(2).unsigned(0);
                        enc.array(vs.len() as u64);
                        for v in vs {
                            enc.unsigned(u64::from(v.0));
                        }
                    }
                    RefuseReason::HandshakeDecodeError(ver, msg) => {
                        enc.array(3)
                            .unsigned(1)
                            .unsigned(u64::from(ver.0))
                            .text(msg);
                    }
                    RefuseReason::Refused(ver, msg) => {
                        enc.array(3)
                            .unsigned(2)
                            .unsigned(u64::from(ver.0))
                            .text(msg);
                    }
                }
            }
            Self::QueryReply(versions) => {
                enc.array(2).unsigned(3);
                encode_version_table(&mut enc, versions);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
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
                let ver_num = dec.unsigned()? as u16;
                let vd = decode_version_data(&mut dec)?;
                Self::AcceptVersion(HandshakeVersion(ver_num), vd)
            }
            (2, 2) => {
                let reason_len = dec.array()?;
                let reason_tag = dec.unsigned()?;
                let reason = match (reason_tag, reason_len) {
                    (0, 2) => {
                        let count = dec.array()?;
                        let mut vs = vec_with_strict_capacity(
                            count,
                            handshake_limits::REFUSE_VERSION_LIST_MAX,
                        )?;
                        for _ in 0..count {
                            vs.push(HandshakeVersion(dec.unsigned()? as u16));
                        }
                        RefuseReason::VersionMismatch(vs)
                    }
                    (1, 3) => {
                        let ver_num = dec.unsigned()? as u16;
                        let msg = dec.text()?.to_owned();
                        RefuseReason::HandshakeDecodeError(HandshakeVersion(ver_num), msg)
                    }
                    (2, 3) => {
                        let ver_num = dec.unsigned()? as u16;
                        let msg = dec.text()?.to_owned();
                        RefuseReason::Refused(HandshakeVersion(ver_num), msg)
                    }
                    _ => {
                        return Err(LedgerError::CborTypeMismatch {
                            expected: 0,
                            actual: reason_tag as u8,
                        });
                    }
                };
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── RefuseReason Display tests ────────────────────────────────────
    //
    // Operator-facing error messages embed `RefuseReason` via
    // `PeerError::Refused { reason: {reason} }` (slice 66 switched from
    // Debug formatting to Display). These tests pin the human-readable
    // format so a future refactor reverting to Debug would surface the
    // variant names and fail CI.

    #[test]
    fn refuse_reason_display_version_mismatch() {
        let r = RefuseReason::VersionMismatch(vec![HandshakeVersion(13), HandshakeVersion(14)]);
        let s = format!("{r}");
        assert!(
            s.contains("version mismatch"),
            "must identify the rule: {s}",
        );
        assert!(s.contains("13"), "must list version 13: {s}");
        assert!(s.contains("14"), "must list version 14: {s}");
        // Not the Debug variant name.
        assert!(
            !s.contains("VersionMismatch"),
            "Display must not leak the Debug variant name: {s}",
        );
    }

    #[test]
    fn refuse_reason_display_handshake_decode_error() {
        let r = RefuseReason::HandshakeDecodeError(HandshakeVersion(14), "expected map".to_owned());
        let s = format!("{r}");
        assert!(s.contains("handshake version data"), "rule name: {s}");
        assert!(s.contains("14"), "must name the version: {s}");
        assert!(
            s.contains("expected map"),
            "must surface the inner reason: {s}"
        );
    }

    #[test]
    fn refuse_reason_display_refused() {
        let r = RefuseReason::Refused(HandshakeVersion(13), "wrong magic".to_owned());
        let s = format!("{r}");
        assert!(s.contains("refused version 13"), "rule + version: {s}");
        assert!(
            s.contains("wrong magic"),
            "must surface the inner reason: {s}"
        );
    }

    #[test]
    fn refuse_reason_display_empty_version_list_is_stable() {
        // Edge case: empty accepted-versions list must still render cleanly
        // rather than panicking or producing trailing punctuation.
        let r = RefuseReason::VersionMismatch(Vec::new());
        let s = format!("{r}");
        assert!(s.contains("version mismatch"));
        assert!(s.contains("[]"), "empty list rendered as `[]`: {s}");
    }

    #[test]
    fn version_data_codec_encodes_4_elements_decodes_2_to_4() {
        // Deliberate asymmetry: the encoder always writes v13+ form
        // (4 elements) because we only advertise v13+ in our supported
        // version lists. The decoder accepts v7-v10 (2 elements) and
        // v11-v12 (3 elements) for compatibility with older peers that
        // might happen to speak to us.
        //
        // This test pins both halves so a future refactor that changes
        // either direction silently breaks wire compatibility.
        use yggdrasil_ledger::{Decoder, Encoder};

        let vd = NodeToNodeVersionData {
            network_magic: 42,
            initiator_only_diffusion_mode: true,
            peer_sharing: 1,
            query: true,
        };

        // Encode always produces 4 elements.
        let mut enc = Encoder::new();
        encode_version_data(&mut enc, &vd);
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().expect("array header");
        assert_eq!(
            len, 4,
            "encode_version_data must always emit 4 elements (v13+ shape)",
        );

        // Round-trip for the 4-element form preserves all fields.
        let decoded = decode_version_data(&mut Decoder::new(&bytes)).expect("decode 4-elem");
        assert_eq!(decoded, vd);

        // Legacy 2-element form decodes with defaults for missing fields.
        let mut enc = Encoder::new();
        enc.array(2).unsigned(42).bool(true);
        let legacy2 = enc.into_bytes();
        let decoded = decode_version_data(&mut Decoder::new(&legacy2)).expect("decode 2-elem");
        assert_eq!(decoded.network_magic, 42);
        assert!(decoded.initiator_only_diffusion_mode);
        assert_eq!(
            decoded.peer_sharing, 0,
            "missing field defaults to disabled"
        );
        assert!(!decoded.query, "missing field defaults to non-query");

        // Legacy 3-element form decodes with default for `query` only.
        let mut enc = Encoder::new();
        enc.array(3).unsigned(42).bool(false).unsigned(1);
        let legacy3 = enc.into_bytes();
        let decoded = decode_version_data(&mut Decoder::new(&legacy3)).expect("decode 3-elem");
        assert_eq!(decoded.network_magic, 42);
        assert!(!decoded.initiator_only_diffusion_mode);
        assert_eq!(decoded.peer_sharing, 1);
        assert!(!decoded.query);

        // Lengths outside [2, 4] are rejected.
        let mut enc = Encoder::new();
        enc.array(5)
            .unsigned(1)
            .bool(false)
            .unsigned(0)
            .bool(false)
            .unsigned(99);
        assert!(
            decode_version_data(&mut Decoder::new(&enc.into_bytes())).is_err(),
            "5-element array must be rejected",
        );
        let mut enc = Encoder::new();
        enc.array(1).unsigned(1);
        assert!(
            decode_version_data(&mut Decoder::new(&enc.into_bytes())).is_err(),
            "1-element array must be rejected",
        );
    }

    /// Encoder-side drift guard for the `HandshakeMessage` wire-tag space.
    ///
    /// 4 message variants (tags 0..=3) per the
    /// `handshake-node-to-node-v14.cddl` outer envelope, with mixed
    /// array lengths (2/3/2/2). A coupled encoder/decoder typo would
    /// silently misinterpret every handshake — e.g. tag-1 `AcceptVersion`
    /// mistakenly decoded as tag-2 `Refuse` would close every connection
    /// that should have succeeded, indistinguishable from a real refusal.
    /// Pre-existing tests cover RefuseReason `Display`, version-table
    /// codec, and per-version constants — but no test pins the outer
    /// message tag/arity directly. This closes that gap.
    ///
    /// Reference: `handshake-node-to-node-v14.cddl`;
    /// `Ouroboros.Network.Protocol.Handshake.Codec`.
    #[test]
    fn handshake_message_encoder_tag_and_arity_match_canonical_cddl() {
        use yggdrasil_ledger::Decoder;

        let vd = NodeToNodeVersionData {
            network_magic: 42,
            initiator_only_diffusion_mode: false,
            peer_sharing: 1,
            query: false,
        };
        let cases: Vec<(u64, u64, HandshakeMessage)> = vec![
            (
                0,
                2,
                HandshakeMessage::ProposeVersions(vec![(HandshakeVersion(13), vd.clone())]),
            ),
            (
                1,
                3,
                HandshakeMessage::AcceptVersion(HandshakeVersion(14), vd.clone()),
            ),
            (
                2,
                2,
                HandshakeMessage::Refuse(RefuseReason::Refused(
                    HandshakeVersion(13),
                    "wrong magic".to_owned(),
                )),
            ),
            (
                3,
                2,
                HandshakeMessage::QueryReply(vec![(HandshakeVersion(15), vd)]),
            ),
        ];
        assert_eq!(cases.len(), 4, "HandshakeMessage tag space must be 0..=3");

        let mut seen: Vec<u64> = Vec::with_capacity(4);
        for (canonical_tag, canonical_len, msg) in cases {
            let bytes = msg.to_cbor();
            let mut dec = Decoder::new(&bytes);
            let len = dec
                .array()
                .expect("HandshakeMessage encodes as a CBOR array");
            assert_eq!(
                len, canonical_len,
                "HandshakeMessage::{msg:?} array length {len}, expected {canonical_len}",
            );
            let tag = dec.unsigned().expect("first array element is the tag");
            assert_eq!(tag, canonical_tag, "HandshakeMessage::{msg:?} tag drift");
            seen.push(tag);
        }
        seen.sort();
        assert_eq!(
            seen,
            vec![0, 1, 2, 3],
            "HandshakeMessage tag set must be exactly 0..=3",
        );
    }

    /// Encoder-side drift guard for the inner `RefuseReason` wire-tag
    /// space, embedded inside `HandshakeMessage::Refuse` (outer tag 2).
    ///
    /// 3 sub-tag variants (0..=2) with mixed array lengths (2/3/3):
    /// `VersionMismatch` (length 2 with version list), `HandshakeDecode
    /// Error` (length 3 with version + reason), `Refused` (length 3 with
    /// version + reason). A typo swapping inner tag-1 `HandshakeDecode
    /// Error` and tag-2 `Refused` would silently misclassify every
    /// connection failure — operator dashboards would attribute decode
    /// errors to deliberate refusals and vice versa.
    ///
    /// Reference: `handshake-node-to-node-v14.cddl` — `refuseReason`.
    #[test]
    fn refuse_reason_encoder_inner_tag_and_arity_match_canonical_cddl() {
        use yggdrasil_ledger::Decoder;

        let cases: Vec<(u64, u64, RefuseReason)> = vec![
            (
                0,
                2,
                RefuseReason::VersionMismatch(vec![HandshakeVersion(13), HandshakeVersion(14)]),
            ),
            (
                1,
                3,
                RefuseReason::HandshakeDecodeError(HandshakeVersion(14), "expected map".to_owned()),
            ),
            (
                2,
                3,
                RefuseReason::Refused(HandshakeVersion(13), "wrong magic".to_owned()),
            ),
        ];
        assert_eq!(cases.len(), 3, "RefuseReason inner tag space must be 0..=2");

        let mut seen: Vec<u64> = Vec::with_capacity(3);
        for (canonical_tag, canonical_len, reason) in cases {
            // Wrap in HandshakeMessage::Refuse to exercise the actual
            // encoder path (RefuseReason has no standalone encode method).
            let bytes = HandshakeMessage::Refuse(reason.clone()).to_cbor();
            let mut dec = Decoder::new(&bytes);
            let outer_len = dec.array().expect("outer Refuse array");
            assert_eq!(outer_len, 2, "outer Refuse must be array length 2");
            let outer_tag = dec.unsigned().expect("outer Refuse tag");
            assert_eq!(outer_tag, 2, "outer Refuse tag must be 2");
            let inner_len = dec.array().expect("inner RefuseReason array");
            assert_eq!(
                inner_len, canonical_len,
                "RefuseReason::{reason:?} inner array length {inner_len}, expected {canonical_len}",
            );
            let inner_tag = dec.unsigned().expect("inner RefuseReason tag");
            assert_eq!(
                inner_tag, canonical_tag,
                "RefuseReason::{reason:?} inner tag drift",
            );
            seen.push(inner_tag);
        }
        seen.sort();
        assert_eq!(
            seen,
            vec![0, 1, 2],
            "RefuseReason inner tag set must be exactly 0..=2",
        );
    }

    #[test]
    fn ntn_handshake_version_constants_are_sequential() {
        // Mirror of slice-87's NtC drift guard for the NtN side: pin
        // that `V13 / V14 / V15` map to literal u16 values `13 / 14 / 15`.
        // A copy-paste typo in ONE constant (e.g. `V14: Self(15)`) would
        // silently misnegotiate client connections onto the wrong NtN
        // protocol semantics — catastrophic for mux-mini-protocol
        // behaviour while the handshake itself succeeds.
        assert_eq!(HandshakeVersion::V13.0, 13);
        assert_eq!(HandshakeVersion::V14.0, 14);
        assert_eq!(HandshakeVersion::V15.0, 15);
    }
}
