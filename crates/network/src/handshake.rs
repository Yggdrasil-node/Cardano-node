/// A network protocol version number used during handshake negotiation.
///
/// Node-to-node versions 14 and 15 are currently defined.
///
/// Reference: `handshake-node-to-node-v14.cddl` — `versionNumber_v14 = 14 / 15`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HandshakeVersion(pub u16);

impl HandshakeVersion {
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

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

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
    let mut versions = Vec::with_capacity(count as usize);
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
                        let mut vs = Vec::with_capacity(count as usize);
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
