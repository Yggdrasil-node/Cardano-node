/// States of the KeepAlive mini-protocol state machine.
///
/// The KeepAlive protocol lets a client periodically check that the
/// connection to a server is still live by sending a cookie value that
/// the server echoes back.
///
/// ```text
///  MsgKeepAlive        MsgKeepAliveResponse
///  StClient ──────────► StServer ──────────► StClient
///    │
///    │ MsgDone
///    ▼
///  StDone
/// ```
///
/// Reference: `Ouroboros.Network.Protocol.KeepAlive.Type`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeepAliveState {
    /// Client agency — may send `MsgKeepAlive` or `MsgDone`.
    StClient,
    /// Server agency — must reply with `MsgKeepAliveResponse`.
    StServer,
    /// Terminal state — no further messages.
    StDone,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the KeepAlive mini-protocol.
///
/// CDDL wire tags (from `keep-alive.cddl`):
///
/// | Tag | Message                |
/// |-----|------------------------|
/// |  0  | `MsgKeepAlive`         |
/// |  1  | `MsgDone`              |
/// |  2  | `MsgKeepAliveResponse` |
///
/// Reference: `Ouroboros.Network.Protocol.KeepAlive.Type` — `Message`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeepAliveMessage {
    /// `[0, cookie]` — client sends a keep-alive ping with a cookie.
    ///
    /// Transition: `StClient → StServer`.
    MsgKeepAlive {
        /// An opaque 16-bit cookie echoed back by the server.
        cookie: u16,
    },

    /// `[1]` — client terminates the protocol.
    ///
    /// Transition: `StClient → StDone`.
    MsgDone,

    /// `[2, cookie]` — server echoes the cookie back.
    ///
    /// Transition: `StServer → StClient`.
    MsgKeepAliveResponse {
        /// The cookie from the corresponding `MsgKeepAlive`.
        cookie: u16,
    },
}

// ---------------------------------------------------------------------------
// Transition validation
// ---------------------------------------------------------------------------

/// Errors arising from illegal KeepAlive state transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum KeepAliveTransitionError {
    /// A message was received that is not legal in the current state.
    #[error("illegal keep-alive transition from {from:?} via {msg_tag}")]
    IllegalTransition {
        /// State the machine was in.
        from: KeepAliveState,
        /// Human-readable tag of the offending message.
        msg_tag: &'static str,
    },
}

impl KeepAliveState {
    /// Computes the next state given an incoming message, or returns
    /// an error if the transition is illegal.
    pub fn transition(self, msg: &KeepAliveMessage) -> Result<Self, KeepAliveTransitionError> {
        match (self, msg) {
            (Self::StClient, KeepAliveMessage::MsgKeepAlive { .. }) => Ok(Self::StServer),
            (Self::StClient, KeepAliveMessage::MsgDone) => Ok(Self::StDone),
            (Self::StServer, KeepAliveMessage::MsgKeepAliveResponse { .. }) => Ok(Self::StClient),
            (from, msg) => Err(KeepAliveTransitionError::IllegalTransition {
                from,
                msg_tag: match msg {
                    KeepAliveMessage::MsgKeepAlive { .. } => "MsgKeepAlive",
                    KeepAliveMessage::MsgDone => "MsgDone",
                    KeepAliveMessage::MsgKeepAliveResponse { .. } => "MsgKeepAliveResponse",
                },
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// CBOR wire codec
// ---------------------------------------------------------------------------

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

impl KeepAliveMessage {
    /// Encode this message to CBOR bytes.
    ///
    /// Wire format (matching upstream CDDL):
    /// - `MsgKeepAlive`         → `[0, cookie]`
    /// - `MsgDone`              → `[1]`
    /// - `MsgKeepAliveResponse` → `[2, cookie]`
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            Self::MsgKeepAlive { cookie } => {
                enc.array(2).unsigned(0).unsigned(u64::from(*cookie));
            }
            Self::MsgDone => {
                enc.array(1).unsigned(1);
            }
            Self::MsgKeepAliveResponse { cookie } => {
                enc.array(2).unsigned(2).unsigned(u64::from(*cookie));
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    pub fn from_cbor(data: &[u8]) -> Result<Self, LedgerError> {
        let mut dec = Decoder::new(data);
        let len = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, len) {
            (0, 2) => {
                let cookie = dec.unsigned()? as u16;
                Self::MsgKeepAlive { cookie }
            }
            (1, 1) => Self::MsgDone,
            (2, 2) => {
                let cookie = dec.unsigned()? as u16;
                Self::MsgKeepAliveResponse { cookie }
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
