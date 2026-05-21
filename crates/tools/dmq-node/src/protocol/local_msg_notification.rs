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

use crate::protocol::sig_submission::{Sig, decode_sig, encode_sig};
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};
use yggdrasil_network::{MessageChannel, MuxError, ProtocolHandle};

/// Anti-DoS cap on the number of messages decoded from a `MsgReply`
/// indefinite-length array.
const LOCAL_MSG_NOTIFICATION_LIST_MAX: usize = 4_096;

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

/// The reply payload of a `MsgReply` — a blocking-style-tagged list
/// of messages.
///
/// Mirror of upstream `data BlockingReplyList (blocking ::
/// StBlockingStyle) a` (re-exported by `LocalMsgNotification.Type`
/// from `TxSubmission2`): a blocking reply carries a non-empty list, a
/// non-blocking reply a possibly-empty list. yggdrasil flattens the
/// GADT's `blocking` type parameter into this 2-variant enum. The
/// blocking style also drives the wire encoding — a non-blocking
/// reply carries a `HasMore` flag, a blocking reply does not (the
/// upstream `Codec.hs` "Issue #15").
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlockingReplyList {
    /// `BlockingReply` — a non-empty reply (a blocking request must
    /// not return an empty list).
    Blocking(Vec<Sig>),
    /// `NonBlockingReply` — a possibly-empty reply.
    NonBlocking(Vec<Sig>),
}

impl BlockingReplyList {
    /// The reply's messages, regardless of blocking style.
    pub fn messages(&self) -> &[Sig] {
        match self {
            BlockingReplyList::Blocking(messages) | BlockingReplyList::NonBlocking(messages) => {
                messages
            }
        }
    }
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
    /// Reply with a (blocking-style-tagged) list of messages and a
    /// has-more flag. `StBusy → StIdle`.
    MsgReply {
        /// The notified signatures, tagged by blocking style.
        reply: BlockingReplyList,
        /// Whether the server has further messages. Carried for both
        /// styles, but a blocking reply does not encode it on the
        /// wire (upstream `Codec.hs` "Issue #15").
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

    /// Encode this message to CBOR.
    ///
    /// Wire format — mirror of upstream
    /// `LocalMsgNotification/Codec.hs`:
    /// - `MsgRequest` is `[0, blocking]`
    /// - `MsgReply` non-blocking is `[1, <indef [msg]>, hasMore]`
    /// - `MsgReply` blocking is `[2, <indef [msg]>]` (no `hasMore` —
    ///   the upstream "Issue #15")
    /// - `MsgClientDone` is `[3]`
    ///
    /// The message list is a CBOR *indefinite*-length array; each
    /// message is `encode_sig` (a CBOR byte string).
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        match self {
            LocalMsgNotificationMessage::MsgRequest { blocking } => {
                enc.array(2).unsigned(0).bool(*blocking);
            }
            LocalMsgNotificationMessage::MsgReply {
                reply: BlockingReplyList::NonBlocking(messages),
                has_more,
            } => {
                enc.array(3).unsigned(1);
                enc.array_indef();
                for sig in messages {
                    encode_sig(sig, &mut enc);
                }
                enc.break_stop();
                enc.bool(*has_more == HasMore::HasMore);
            }
            LocalMsgNotificationMessage::MsgReply {
                reply: BlockingReplyList::Blocking(messages),
                has_more: _,
            } => {
                enc.array(2).unsigned(2);
                enc.array_indef();
                for sig in messages {
                    encode_sig(sig, &mut enc);
                }
                enc.break_stop();
            }
            LocalMsgNotificationMessage::MsgClientDone => {
                enc.array(1).unsigned(3);
            }
        }
        enc.into_bytes()
    }

    /// Decode a message from CBOR bytes.
    ///
    /// Inverse of [`Self::to_cbor`]. A blocking `MsgReply` (tag `2`)
    /// has no encoded `hasMore`, so it decodes with
    /// `HasMore::DoesNotHaveMore` (the upstream "Issue #15").
    pub fn from_cbor(data: &[u8]) -> Result<LocalMsgNotificationMessage, LedgerError> {
        let mut dec = Decoder::new(data);
        let arr = dec.array()?;
        let tag = dec.unsigned()?;
        let msg = match (tag, arr) {
            (0, 2) => LocalMsgNotificationMessage::MsgRequest {
                blocking: dec.bool()?,
            },
            (1, 3) => {
                let messages = decode_indef_sigs(&mut dec)?;
                let has_more = if dec.bool()? {
                    HasMore::HasMore
                } else {
                    HasMore::DoesNotHaveMore
                };
                LocalMsgNotificationMessage::MsgReply {
                    reply: BlockingReplyList::NonBlocking(messages),
                    has_more,
                }
            }
            (2, 2) => {
                let messages = decode_indef_sigs(&mut dec)?;
                LocalMsgNotificationMessage::MsgReply {
                    reply: BlockingReplyList::Blocking(messages),
                    has_more: HasMore::DoesNotHaveMore,
                }
            }
            (3, 1) => LocalMsgNotificationMessage::MsgClientDone,
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

/// Decode the `MsgReply` indefinite-length array of `encode_sig`-encoded
/// signatures.
fn decode_indef_sigs(dec: &mut Decoder) -> Result<Vec<Sig>, LedgerError> {
    if dec.array_begin()?.is_some() {
        return Err(LedgerError::CborDecodeError(
            "LocalMsgNotification.MsgReply: expected an indefinite-length array".to_string(),
        ));
    }
    let mut sigs = Vec::new();
    while !dec.is_break() {
        if sigs.len() >= LOCAL_MSG_NOTIFICATION_LIST_MAX {
            return Err(LedgerError::DecodedCountTooLarge {
                count: sigs.len() as u64,
                max: LOCAL_MSG_NOTIFICATION_LIST_MAX,
            });
        }
        let raw = dec.bytes_owned()?;
        sigs.push(decode_sig(&raw)?);
    }
    dec.consume_break()?;
    Ok(sigs)
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

// ---------------------------------------------------------------------------
// Client driver (upstream `Protocol/LocalMsgNotification/Client.hs`)
// ---------------------------------------------------------------------------

/// Errors from the [`LocalMsgNotificationClient`] driver.
///
/// Mirror of the per-driver error enums in `crates/network` (e.g.
/// `KeepAliveClientError`).
#[derive(Debug, thiserror::Error)]
pub enum LocalMsgNotificationClientError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),
    /// The connection was closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,
    /// An illegal protocol-state transition.
    #[error("protocol error: {0}")]
    Protocol(#[from] LocalMsgNotificationTransitionError),
    /// A CBOR decode failure on an inbound message.
    #[error("CBOR decode error: {0}")]
    Decode(String),
    /// An unexpected message from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

/// A `LocalMsgNotification` client driver maintaining the protocol
/// state machine.
///
/// Mirror of upstream `Protocol/LocalMsgNotification/Client.hs`
/// (`localMsgNotificationClientPeer`), following the
/// `crates/network` mini-protocol-driver pattern (`keepalive_client.rs`)
/// — a struct wrapping a [`MessageChannel`] plus typed protocol
/// methods. The client drives the exchange: it requests notification
/// batches and terminates the protocol.
pub struct LocalMsgNotificationClient {
    channel: MessageChannel,
    state: LocalMsgNotificationState,
}

impl LocalMsgNotificationClient {
    /// Create a client driver from a `LocalMsgNotification`
    /// `ProtocolHandle`. The protocol starts in `StIdle` — client
    /// agency.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: LocalMsgNotificationState::StIdle,
        }
    }

    /// The current protocol state.
    pub fn state(&self) -> LocalMsgNotificationState {
        self.state
    }

    async fn send_msg(
        &mut self,
        msg: &LocalMsgNotificationMessage,
    ) -> Result<(), LocalMsgNotificationClientError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(LocalMsgNotificationClientError::Mux)
    }

    async fn recv_msg(
        &mut self,
    ) -> Result<LocalMsgNotificationMessage, LocalMsgNotificationClientError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(LocalMsgNotificationClientError::ConnectionClosed)?;
        let msg = LocalMsgNotificationMessage::from_cbor(&raw)
            .map_err(|err| LocalMsgNotificationClientError::Decode(err.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    /// Request a batch of notified signatures — send `MsgRequest` and
    /// await the server's `MsgReply`.
    ///
    /// `blocking` selects the blocking style: a blocking request is
    /// used once the server has announced it has no further messages.
    pub async fn request(
        &mut self,
        blocking: bool,
    ) -> Result<(BlockingReplyList, HasMore), LocalMsgNotificationClientError> {
        self.send_msg(&LocalMsgNotificationMessage::MsgRequest { blocking })
            .await?;
        match self.recv_msg().await? {
            LocalMsgNotificationMessage::MsgReply { reply, has_more } => Ok((reply, has_more)),
            other => Err(LocalMsgNotificationClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Terminate the protocol cleanly with `MsgClientDone`.
    pub async fn done(mut self) -> Result<(), LocalMsgNotificationClientError> {
        self.send_msg(&LocalMsgNotificationMessage::MsgClientDone)
            .await
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
                reply: BlockingReplyList::NonBlocking(vec![]),
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
                reply: BlockingReplyList::NonBlocking(vec![]),
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

    #[test]
    fn client_error_connection_closed_displays() {
        let s = format!("{}", LocalMsgNotificationClientError::ConnectionClosed);
        assert!(s.to_lowercase().contains("connection closed"), "got: {s}");
    }

    #[test]
    fn client_error_decode_propagates_inner_reason() {
        let err = LocalMsgNotificationClientError::Decode("malformed MsgReply".into());
        let s = format!("{err}");
        assert!(s.contains("CBOR decode"), "got: {s}");
        assert!(s.contains("malformed MsgReply"), "got: {s}");
    }

    #[test]
    fn client_error_unexpected_message_propagates_inner() {
        let err =
            LocalMsgNotificationClientError::UnexpectedMessage("MsgClientDone in StBusy".into());
        let s = format!("{err}");
        assert!(s.contains("unexpected message"), "got: {s}");
        assert!(s.contains("MsgClientDone in StBusy"), "got: {s}");
    }

    #[test]
    fn codec_round_trips_every_message() {
        let messages = vec![
            LocalMsgNotificationMessage::MsgRequest { blocking: true },
            LocalMsgNotificationMessage::MsgRequest { blocking: false },
            LocalMsgNotificationMessage::MsgReply {
                reply: BlockingReplyList::NonBlocking(vec![]),
                has_more: HasMore::HasMore,
            },
            LocalMsgNotificationMessage::MsgClientDone,
        ];
        for msg in messages {
            let encoded = msg.to_cbor();
            let decoded = LocalMsgNotificationMessage::from_cbor(&encoded).expect("decodes");
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn blocking_reply_decodes_without_has_more_flag() {
        // A blocking MsgReply omits hasMore on the wire (upstream
        // "Issue #15") — it always decodes as DoesNotHaveMore, even if
        // the encoded value carried HasMore.
        let msg = LocalMsgNotificationMessage::MsgReply {
            reply: BlockingReplyList::Blocking(vec![]),
            has_more: HasMore::HasMore,
        };
        let decoded = LocalMsgNotificationMessage::from_cbor(&msg.to_cbor()).expect("decodes");
        assert_eq!(
            decoded,
            LocalMsgNotificationMessage::MsgReply {
                reply: BlockingReplyList::Blocking(vec![]),
                has_more: HasMore::DoesNotHaveMore,
            }
        );
    }

    #[test]
    fn blocking_reply_list_exposes_messages_for_both_styles() {
        assert!(BlockingReplyList::NonBlocking(vec![]).messages().is_empty());
        assert!(BlockingReplyList::Blocking(vec![]).messages().is_empty());
        assert_ne!(
            BlockingReplyList::Blocking(vec![]),
            BlockingReplyList::NonBlocking(vec![]),
        );
    }
}
