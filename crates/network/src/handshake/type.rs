//! Handshake mini-protocol type-level definitions.
//!
//! ## Naming parity
//!
//! **Strict mirror:** Ouroboros/Network/Protocol/Handshake/Type.hs.
//! Filename flattens the upstream directory; the file carries the
//! protocol's state-machine state enum, the `HandshakeMessage`
//! envelope, the `RefuseReason` enum, the inherent transition
//! method, and the per-message `tag_name` / `wire_tag` helpers
//! that upstream's `Type.hs` expresses through GHC's type-level
//! GADT machinery.

use super::version::{HandshakeVersion, NodeToNodeVersionData};

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
}
