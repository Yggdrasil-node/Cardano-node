//! DMQ `LocalMsgNotification` mini-protocol — local DMQ-signature
//! notification (node-to-client).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Collapses the upstream
//! `DMQ/Protocol/LocalMsgNotification/{Type,Codec,Client,Server}.hs`
//! files into one Rust file, mirroring the
//! `crates/network/src/protocols/` one-file-per-mini-protocol
//! pattern. Unlike `SigSubmission` / `LocalMsgSubmission`,
//! `LocalMsgNotification` is DMQ's *own* node-to-client protocol —
//! the server pushes newly-diffused signatures to a local client. It
//! is parameterized over the message type upstream; for DMQ that is
//! [`Sig`].

use crate::protocol::sig_submission::Sig;

/// Whether the server has more messages it can provide.
///
/// Upstream `data HasMore = HasMore | DoesNotHaveMore`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HasMore {
    /// The server has further messages available.
    HasMore,
    /// The server has no further messages.
    DoesNotHaveMore,
}

/// States of the `LocalMsgNotification` mini-protocol.
///
/// Mirror of upstream `type data LocalMsgNotification msg where StIdle
/// / StBusy StBlockingStyle / StDone`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMsgNotificationState {
    /// Client agency — must send `MsgRequest` or `MsgClientDone`.
    StIdle,
    /// Server agency — must reply with `MsgReply`.
    StBusy {
        /// Whether this is a blocking request.
        blocking: bool,
    },
    /// Terminal state — no further messages.
    StDone,
}

/// Messages of the `LocalMsgNotification` mini-protocol.
///
/// Mirror of upstream `Message (LocalMsgNotification msg)`:
/// `MsgRequest` (with a blocking style), `MsgReply` (a list of
/// messages plus a [`HasMore`] flag), `MsgClientDone`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalMsgNotificationMessage {
    /// Request messages from the server. `StIdle → StBusy(blocking)`.
    ///
    /// The blocking request is used when the server has announced it
    /// has no further messages; otherwise the non-blocking request
    /// must be used.
    MsgRequest {
        /// `true` blocking, `false` non-blocking.
        blocking: bool,
    },
    /// Reply with a list of messages and a has-more flag.
    /// `StBusy → StIdle`.
    MsgReply {
        /// The notified signatures (non-empty for a blocking reply).
        messages: Vec<Sig>,
        /// Whether the server has further messages.
        has_more: HasMore,
    },
    /// Client terminates the exchange. `StIdle → StDone`.
    MsgClientDone,
}

impl LocalMsgNotificationMessage {
    /// Human-readable tag name, used in transition-error messages.
    pub fn tag_name(&self) -> &'static str {
        match self {
            LocalMsgNotificationMessage::MsgRequest { .. } => "MsgRequest",
            LocalMsgNotificationMessage::MsgReply { .. } => "MsgReply",
            LocalMsgNotificationMessage::MsgClientDone => "MsgClientDone",
        }
    }
}

/// An illegal `LocalMsgNotification` state transition.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("illegal LocalMsgNotification transition: {message} not allowed in {state:?}")]
pub struct LocalMsgNotificationTransitionError {
    /// The state the message arrived in.
    pub state: LocalMsgNotificationState,
    /// The offending message's tag name.
    pub message: &'static str,
}

impl LocalMsgNotificationState {
    /// The next state after an incoming message, or an error if the
    /// transition is illegal.
    ///
    /// Mirror of upstream's `LocalMsgNotification` `StateAgency` /
    /// `Message` transitions: `StIdle`+`MsgRequest`→`StBusy`,
    /// `StBusy`+`MsgReply`→`StIdle`, `StIdle`+`MsgClientDone`→`StDone`.
    pub fn transition(
        self,
        msg: &LocalMsgNotificationMessage,
    ) -> Result<LocalMsgNotificationState, LocalMsgNotificationTransitionError> {
        match (self, msg) {
            (
                LocalMsgNotificationState::StIdle,
                LocalMsgNotificationMessage::MsgRequest { blocking },
            ) => Ok(LocalMsgNotificationState::StBusy {
                blocking: *blocking,
            }),
            (
                LocalMsgNotificationState::StBusy { .. },
                LocalMsgNotificationMessage::MsgReply { .. },
            ) => Ok(LocalMsgNotificationState::StIdle),
            (LocalMsgNotificationState::StIdle, LocalMsgNotificationMessage::MsgClientDone) => {
                Ok(LocalMsgNotificationState::StDone)
            }
            (state, msg) => Err(LocalMsgNotificationTransitionError {
                state,
                message: msg.tag_name(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_follows_the_protocol() {
        let busy = LocalMsgNotificationState::StIdle
            .transition(&LocalMsgNotificationMessage::MsgRequest { blocking: true })
            .expect("request");
        assert_eq!(busy, LocalMsgNotificationState::StBusy { blocking: true });
        assert_eq!(
            busy.transition(&LocalMsgNotificationMessage::MsgReply {
                messages: vec![],
                has_more: HasMore::DoesNotHaveMore,
            })
            .expect("reply"),
            LocalMsgNotificationState::StIdle
        );
        assert_eq!(
            LocalMsgNotificationState::StIdle
                .transition(&LocalMsgNotificationMessage::MsgClientDone)
                .expect("done"),
            LocalMsgNotificationState::StDone
        );
    }

    #[test]
    fn transition_rejects_illegal_messages() {
        // MsgReply is illegal in StIdle.
        let err = LocalMsgNotificationState::StIdle
            .transition(&LocalMsgNotificationMessage::MsgReply {
                messages: vec![],
                has_more: HasMore::HasMore,
            })
            .expect_err("rejects");
        assert_eq!(err.message, "MsgReply");
        assert_eq!(err.state, LocalMsgNotificationState::StIdle);
        // MsgClientDone is illegal in StBusy.
        assert!(
            LocalMsgNotificationState::StBusy { blocking: false }
                .transition(&LocalMsgNotificationMessage::MsgClientDone)
                .is_err()
        );
    }

    #[test]
    fn has_more_round_trips() {
        assert_ne!(HasMore::HasMore, HasMore::DoesNotHaveMore);
    }
}
