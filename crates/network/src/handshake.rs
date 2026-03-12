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
