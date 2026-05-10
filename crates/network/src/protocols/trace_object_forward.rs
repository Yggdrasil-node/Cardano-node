//! TraceObjectForward mini-protocol type-level definitions
//! (state machine + message types).
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Protocol/TraceObject/Type.hs.
//!
//! Filename flattens the upstream directory; this file carries the
//! protocol's typed state machine + message envelope, mirroring
//! upstream's `Type.hs`. The CBOR codec lands in R418
//! (`Trace.Forward.Protocol.TraceObject.Codec` mirror), the
//! responder driver in R419 (`Trace.Forward.Protocol.TraceObject.Acceptor`
//! mirror), and the `RunMiniProtocol` aggregator in R420
//! (`Trace.Forward.Run.TraceObject.Acceptor` mirror).
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                              |
//! |---------------------------------------------------------|----------------------------------------|
//! | `data TraceObjectForward lo where StIdle / StBusy b / StDone` | [`TraceObjectForwardState`]      |
//! | `StBlockingStyle = StBlocking | StNonBlocking`          | [`StBlockingStyle`]                    |
//! | `TokBlockingStyle 'StBlocking | TokNonBlocking 'StNonBlocking` | (collapses — Rust's `StBlockingStyle` enum *is* the value-level token) |
//! | `newtype NumberOfTraceObjects { nTraceObjects :: Word16 }` | [`NumberOfTraceObjects`]            |
//! | `data BlockingReplyList blocking lo`                    | [`BlockingReplyList`]                  |
//! | `Message (TraceObjectForward lo) from to`               | [`TraceObjectForwardMessage`]          |
//! | `MsgTraceObjectsRequest`                                | [`TraceObjectForwardMessage::MsgTraceObjectsRequest`] |
//! | `MsgTraceObjectsReply`                                  | [`TraceObjectForwardMessage::MsgTraceObjectsReply`]   |
//! | `MsgDone`                                               | [`TraceObjectForwardMessage::MsgDone`] |
//! | `type StateAgency 'StIdle = 'ClientAgency`              | [`Agency::Acceptor`] (per [`TraceObjectForwardState::agency`]) |
//! | `type StateAgency ('StBusy _) = 'ServerAgency`          | [`Agency::Forwarder`]                  |
//! | `type StateAgency 'StDone = 'NobodyAgency`              | [`Agency::Nobody`]                     |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **GADT + DataKinds + Singletons type-level encoding**: upstream
//!   uses `data TraceObjectForward lo where StIdle :: ...` to lift
//!   states into the type level + `SingTraceObjectForward` singletons
//!   to scrutinize them at runtime. Rust enums collapse this — the
//!   value-level enum *is* the runtime representation; type-state
//!   safety is enforced via the [`TraceObjectForwardState::transition`]
//!   exhaustive-match validator (matching the precedent set by
//!   `keep_alive.rs`, `chain_sync.rs`, etc.).
//! - **`Protocol` typeclass + `StateAgency` type family**: upstream
//!   threads agency through the typed-protocol typeclass machinery
//!   for compile-time message-direction safety. Yggdrasil exposes
//!   the same agency information via the runtime
//!   [`TraceObjectForwardState::agency`] method (returning [`Agency`])
//!   — same information, runtime-checked rather than compile-time.
//! - **`ShowProxy` instances**: upstream's `Show`-only-via-proxy
//!   types collapse into Rust's standard `Debug` derivation.
//!
//! Reference: `Trace.Forward.Protocol.TraceObject.Type` from the
//! upstream `trace-forward` package (vendored at
//! `.reference-haskell-cardano-node/trace-forward/`).

// ---------------------------------------------------------------------------
// Auxiliary types
// ---------------------------------------------------------------------------

/// Number of trace objects requested by the acceptor in a
/// [`TraceObjectForwardMessage::MsgTraceObjectsRequest`]. Mirror of
/// upstream `newtype NumberOfTraceObjects { nTraceObjects :: Word16 }`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct NumberOfTraceObjects(pub u16);

impl NumberOfTraceObjects {
    /// Construct from a raw `u16`. Matches upstream's record-syntax
    /// constructor + accessor pair.
    pub const fn new(n: u16) -> Self {
        Self(n)
    }

    /// The raw 16-bit count. Mirror of upstream's
    /// `nTraceObjects :: NumberOfTraceObjects -> Word16`.
    pub const fn n_trace_objects(self) -> u16 {
        self.0
    }
}

/// Blocking style for [`TraceObjectForwardMessage::MsgTraceObjectsRequest`].
///
/// In upstream, this is encoded on the wire as a `Bool` — `True`
/// marks blocking and `False` marks non-blocking — see
/// `Trace.Forward.Protocol.TraceObject.Codec` for the wire shape.
///
/// Mirror of upstream `data StBlockingStyle = StBlocking | StNonBlocking`
/// AND its value-level token `data TokBlockingStyle (k :: StBlockingStyle)`
/// — the two collapse into a single Rust enum since Rust enums *are*
/// runtime tokens.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum StBlockingStyle {
    /// Blocking sub-state of `StBusy`. The reply must contain at
    /// least one trace object; there is no timeout.
    StBlocking,
    /// Non-blocking sub-state of `StBusy`. The reply may be empty;
    /// the forwarder is bound by a timeout.
    StNonBlocking,
}

/// List of trace objects in a reply, indexed by the
/// [`StBlockingStyle`] of the originating request.
///
/// Mirror of upstream `data BlockingReplyList blocking lo where
/// BlockingReply :: NonEmpty lo -> ... | NonBlockingReply :: [lo] -> ...`.
/// The blocking-variant `NonEmpty` invariant is enforced at
/// constructor time via [`BlockingReplyList::blocking`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlockingReplyList<TraceObj> {
    /// Reply to a `StBlocking` request. The list MUST be non-empty —
    /// the constructor [`Self::blocking`] enforces this invariant.
    Blocking(Vec<TraceObj>),

    /// Reply to a `StNonBlocking` request. The list may be empty.
    NonBlocking(Vec<TraceObj>),
}

impl<TraceObj> BlockingReplyList<TraceObj> {
    /// Construct a `Blocking` reply, validating the upstream
    /// `NonEmpty` invariant. Returns
    /// [`BlockingReplyListEmptyError`] if `items` is empty.
    pub fn blocking(items: Vec<TraceObj>) -> Result<Self, BlockingReplyListEmptyError> {
        if items.is_empty() {
            Err(BlockingReplyListEmptyError)
        } else {
            Ok(Self::Blocking(items))
        }
    }

    /// Construct a `NonBlocking` reply. Always succeeds; the empty
    /// list is allowed.
    pub fn non_blocking(items: Vec<TraceObj>) -> Self {
        Self::NonBlocking(items)
    }

    /// The blocking style of the originating request that this
    /// reply matches.
    pub fn style(&self) -> StBlockingStyle {
        match self {
            Self::Blocking(_) => StBlockingStyle::StBlocking,
            Self::NonBlocking(_) => StBlockingStyle::StNonBlocking,
        }
    }

    /// View the underlying trace objects regardless of variant.
    pub fn items(&self) -> &[TraceObj] {
        match self {
            Self::Blocking(v) | Self::NonBlocking(v) => v.as_slice(),
        }
    }

    /// Take the underlying trace objects regardless of variant.
    pub fn into_items(self) -> Vec<TraceObj> {
        match self {
            Self::Blocking(v) | Self::NonBlocking(v) => v,
        }
    }
}

/// Returned from [`BlockingReplyList::blocking`] when the caller
/// supplies an empty `Vec`. Mirrors the upstream type-level
/// `NonEmpty lo` constraint.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("BlockingReplyList::blocking requires a non-empty list of trace objects")]
pub struct BlockingReplyListEmptyError;

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// States of the TraceObjectForward mini-protocol state machine.
///
/// The protocol terminology matches upstream:
/// 1. The **forwarder** collects `TraceObject`s and sends them to
///    the **acceptor** by request.
/// 2. The **acceptor** receives `TraceObject`s from the forwarder.
/// 3. After the connection is established, the acceptor asks for
///    `TraceObject`s; the forwarder replies. So the acceptor plays
///    the *client* role and the forwarder plays the *server* role.
///
/// ```text
///                 MsgTraceObjectsRequest
///   StIdle ────────────────────────────► StBusy(blocking | non-blocking)
///     │                                       │
///     │ MsgDone                                │ MsgTraceObjectsReply
///     ▼                                       ▼
///   StDone                                  StIdle
/// ```
///
/// Reference: `Trace.Forward.Protocol.TraceObject.Type` —
/// `TraceObjectForward`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TraceObjectForwardState {
    /// Acceptor agency — may send `MsgTraceObjectsRequest` (with
    /// either blocking style) or `MsgDone`.
    StIdle,
    /// Forwarder agency — must reply with `MsgTraceObjectsReply`
    /// matching the originating blocking style.
    StBusy(StBlockingStyle),
    /// Terminal state — no further messages.
    StDone,
}

/// Which party currently has agency to send the next message in
/// the protocol. Mirror of upstream's `StateAgency` type family
/// over `TraceObjectForward`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Agency {
    /// The acceptor (client) sends next. Mirror of `'ClientAgency`.
    Acceptor,
    /// The forwarder (server) sends next. Mirror of `'ServerAgency`.
    Forwarder,
    /// Terminal — no party sends. Mirror of `'NobodyAgency`.
    Nobody,
}

impl TraceObjectForwardState {
    /// The party with agency in this state. Mirror of upstream's
    /// `StateAgency` type-family clauses.
    pub const fn agency(self) -> Agency {
        match self {
            Self::StIdle => Agency::Acceptor,
            Self::StBusy(_) => Agency::Forwarder,
            Self::StDone => Agency::Nobody,
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Messages of the TraceObjectForward mini-protocol.
///
/// CBOR wire shape (R418 codec port mirrors
/// `Trace.Forward.Protocol.TraceObject.Codec`):
///
/// | Tag | Message                  |
/// |-----|--------------------------|
/// |  0  | `MsgTraceObjectsRequest` |
/// |  1  | `MsgTraceObjectsReply`   |
/// |  2  | `MsgDone`                |
///
/// Reference: `Trace.Forward.Protocol.TraceObject.Type` — `Message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceObjectForwardMessage<TraceObj> {
    /// `[0, blocking, n_trace_objects]` — acceptor requests up to
    /// `n_trace_objects` trace objects from the forwarder, in the
    /// indicated blocking style.
    ///
    /// Transition: `StIdle → StBusy(blocking)`.
    MsgTraceObjectsRequest {
        /// Whether the forwarder may take its time replying
        /// (blocking) or must reply promptly (non-blocking).
        blocking: StBlockingStyle,
        /// Maximum number of trace objects to return.
        n_trace_objects: NumberOfTraceObjects,
    },

    /// `[1, [trace-object, …]]` — forwarder replies with the list
    /// of trace objects, matching the originating blocking style.
    ///
    /// Transition: `StBusy(blocking) → StIdle`.
    MsgTraceObjectsReply {
        /// The reply payload. Variant matches the originating
        /// request's blocking style.
        reply: BlockingReplyList<TraceObj>,
    },

    /// `[2]` — acceptor terminates the protocol.
    ///
    /// Transition: `StIdle → StDone`.
    MsgDone,
}

impl<TraceObj> TraceObjectForwardMessage<TraceObj> {
    /// Human-readable tag of the message variant. Used in
    /// [`TraceObjectForwardTransitionError`] reports and
    /// debug logging.
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::MsgTraceObjectsRequest { .. } => "MsgTraceObjectsRequest",
            Self::MsgTraceObjectsReply { .. } => "MsgTraceObjectsReply",
            Self::MsgDone => "MsgDone",
        }
    }
}

// ---------------------------------------------------------------------------
// Transition validation
// ---------------------------------------------------------------------------

/// Errors arising from illegal TraceObjectForward state transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TraceObjectForwardTransitionError {
    /// A message was received that is not legal in the current state.
    #[error("illegal trace-object-forward transition from {from:?} via {msg_tag}")]
    IllegalTransition {
        /// State the machine was in.
        from: TraceObjectForwardState,
        /// Human-readable tag of the offending message.
        msg_tag: &'static str,
    },
    /// `MsgTraceObjectsReply` arrived in `StBusy(b)` but its
    /// `BlockingReplyList` variant disagreed with the originating
    /// request's blocking style.
    #[error(
        "trace-object-forward reply blocking-style mismatch: \
         expected {expected:?}, got {actual:?}"
    )]
    BlockingStyleMismatch {
        /// The blocking style the forwarder was supposed to reply with
        /// (taken from the originating `MsgTraceObjectsRequest`).
        expected: StBlockingStyle,
        /// The blocking style actually carried by the
        /// `BlockingReplyList`.
        actual: StBlockingStyle,
    },
}

impl TraceObjectForwardState {
    /// Computes the next state given an incoming message, or returns
    /// an error if the transition is illegal.
    pub fn transition<TraceObj>(
        self,
        msg: &TraceObjectForwardMessage<TraceObj>,
    ) -> Result<Self, TraceObjectForwardTransitionError> {
        match (self, msg) {
            (Self::StIdle, TraceObjectForwardMessage::MsgTraceObjectsRequest { blocking, .. }) => {
                Ok(Self::StBusy(*blocking))
            }

            (Self::StIdle, TraceObjectForwardMessage::MsgDone) => Ok(Self::StDone),

            (Self::StBusy(expected), TraceObjectForwardMessage::MsgTraceObjectsReply { reply }) => {
                let actual = reply.style();
                if actual == expected {
                    Ok(Self::StIdle)
                } else {
                    Err(TraceObjectForwardTransitionError::BlockingStyleMismatch {
                        expected,
                        actual,
                    })
                }
            }

            (from, msg) => Err(TraceObjectForwardTransitionError::IllegalTransition {
                from,
                msg_tag: msg.tag(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny stand-in payload for protocol-level tests. Production
    /// uses [`yggdrasil_cardano_tracer::TraceObject`] (cardano-tracer
    /// is a downstream consumer; importing it here would create a
    /// circular dep).
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TestPayload(u32);

    #[test]
    fn number_of_trace_objects_round_trips() {
        let n = NumberOfTraceObjects::new(42);
        assert_eq!(n.n_trace_objects(), 42);
        assert_eq!(n.0, 42);
    }

    #[test]
    fn blocking_reply_list_blocking_rejects_empty() {
        let err = BlockingReplyList::<TestPayload>::blocking(vec![]).expect_err("empty");
        assert_eq!(err, BlockingReplyListEmptyError);
    }

    #[test]
    fn blocking_reply_list_blocking_accepts_one_or_more() {
        let one = BlockingReplyList::blocking(vec![TestPayload(1)]).expect("one");
        assert!(matches!(one, BlockingReplyList::Blocking(ref v) if v.len() == 1));
        let three =
            BlockingReplyList::blocking(vec![TestPayload(1), TestPayload(2), TestPayload(3)])
                .expect("three");
        assert_eq!(three.items().len(), 3);
    }

    #[test]
    fn blocking_reply_list_non_blocking_accepts_empty() {
        let empty = BlockingReplyList::<TestPayload>::non_blocking(vec![]);
        assert!(empty.items().is_empty());
        assert_eq!(empty.style(), StBlockingStyle::StNonBlocking);
    }

    #[test]
    fn blocking_reply_list_style_matches_variant() {
        let b = BlockingReplyList::blocking(vec![TestPayload(0)]).expect("b");
        let nb = BlockingReplyList::<TestPayload>::non_blocking(vec![]);
        assert_eq!(b.style(), StBlockingStyle::StBlocking);
        assert_eq!(nb.style(), StBlockingStyle::StNonBlocking);
    }

    #[test]
    fn blocking_reply_list_into_items_unifies_variants() {
        let b = BlockingReplyList::blocking(vec![TestPayload(7)]).expect("b");
        assert_eq!(b.into_items(), vec![TestPayload(7)]);
        let nb = BlockingReplyList::non_blocking(vec![TestPayload(8), TestPayload(9)]);
        assert_eq!(nb.into_items(), vec![TestPayload(8), TestPayload(9)]);
    }

    #[test]
    fn agency_matches_upstream_state_agency_clauses() {
        // 'StIdle = 'ClientAgency      → Acceptor
        assert_eq!(TraceObjectForwardState::StIdle.agency(), Agency::Acceptor);
        // 'StBusy _ = 'ServerAgency    → Forwarder
        assert_eq!(
            TraceObjectForwardState::StBusy(StBlockingStyle::StBlocking).agency(),
            Agency::Forwarder
        );
        assert_eq!(
            TraceObjectForwardState::StBusy(StBlockingStyle::StNonBlocking).agency(),
            Agency::Forwarder
        );
        // 'StDone = 'NobodyAgency      → Nobody
        assert_eq!(TraceObjectForwardState::StDone.agency(), Agency::Nobody);
    }

    #[test]
    fn message_tag_strings_match_upstream_constructor_names() {
        let req: TraceObjectForwardMessage<TestPayload> =
            TraceObjectForwardMessage::MsgTraceObjectsRequest {
                blocking: StBlockingStyle::StBlocking,
                n_trace_objects: NumberOfTraceObjects(1),
            };
        assert_eq!(req.tag(), "MsgTraceObjectsRequest");

        let rep: TraceObjectForwardMessage<TestPayload> =
            TraceObjectForwardMessage::MsgTraceObjectsReply {
                reply: BlockingReplyList::<TestPayload>::non_blocking(vec![]),
            };
        assert_eq!(rep.tag(), "MsgTraceObjectsReply");

        let done: TraceObjectForwardMessage<TestPayload> = TraceObjectForwardMessage::MsgDone;
        assert_eq!(done.tag(), "MsgDone");
    }

    #[test]
    fn idle_request_blocking_advances_to_busy_blocking() {
        let next = TraceObjectForwardState::StIdle
            .transition(
                &TraceObjectForwardMessage::<TestPayload>::MsgTraceObjectsRequest {
                    blocking: StBlockingStyle::StBlocking,
                    n_trace_objects: NumberOfTraceObjects(10),
                },
            )
            .expect("legal");
        assert_eq!(
            next,
            TraceObjectForwardState::StBusy(StBlockingStyle::StBlocking)
        );
    }

    #[test]
    fn idle_request_non_blocking_advances_to_busy_non_blocking() {
        let next = TraceObjectForwardState::StIdle
            .transition(
                &TraceObjectForwardMessage::<TestPayload>::MsgTraceObjectsRequest {
                    blocking: StBlockingStyle::StNonBlocking,
                    n_trace_objects: NumberOfTraceObjects(0),
                },
            )
            .expect("legal");
        assert_eq!(
            next,
            TraceObjectForwardState::StBusy(StBlockingStyle::StNonBlocking)
        );
    }

    #[test]
    fn idle_done_terminates() {
        let next = TraceObjectForwardState::StIdle
            .transition(&TraceObjectForwardMessage::<TestPayload>::MsgDone)
            .expect("legal");
        assert_eq!(next, TraceObjectForwardState::StDone);
    }

    #[test]
    fn busy_reply_matching_style_returns_to_idle() {
        let blocking_reply = BlockingReplyList::blocking(vec![TestPayload(1)]).expect("seed");
        let next = TraceObjectForwardState::StBusy(StBlockingStyle::StBlocking)
            .transition(&TraceObjectForwardMessage::MsgTraceObjectsReply {
                reply: blocking_reply,
            })
            .expect("legal");
        assert_eq!(next, TraceObjectForwardState::StIdle);

        let non_blocking_reply: BlockingReplyList<TestPayload> =
            BlockingReplyList::non_blocking(vec![]);
        let next2 = TraceObjectForwardState::StBusy(StBlockingStyle::StNonBlocking)
            .transition(&TraceObjectForwardMessage::MsgTraceObjectsReply {
                reply: non_blocking_reply,
            })
            .expect("legal");
        assert_eq!(next2, TraceObjectForwardState::StIdle);
    }

    #[test]
    fn busy_reply_mismatched_style_errors() {
        let blocking_reply = BlockingReplyList::blocking(vec![TestPayload(1)]).expect("seed");
        let err = TraceObjectForwardState::StBusy(StBlockingStyle::StNonBlocking)
            .transition(&TraceObjectForwardMessage::MsgTraceObjectsReply {
                reply: blocking_reply,
            })
            .expect_err("mismatch");
        assert_eq!(
            err,
            TraceObjectForwardTransitionError::BlockingStyleMismatch {
                expected: StBlockingStyle::StNonBlocking,
                actual: StBlockingStyle::StBlocking,
            }
        );
    }

    #[test]
    fn idle_reply_is_illegal() {
        let reply = BlockingReplyList::non_blocking(vec![]);
        let err = TraceObjectForwardState::StIdle
            .transition(&TraceObjectForwardMessage::<TestPayload>::MsgTraceObjectsReply { reply })
            .expect_err("illegal");
        assert_eq!(
            err,
            TraceObjectForwardTransitionError::IllegalTransition {
                from: TraceObjectForwardState::StIdle,
                msg_tag: "MsgTraceObjectsReply",
            }
        );
    }

    #[test]
    fn busy_request_is_illegal() {
        let err = TraceObjectForwardState::StBusy(StBlockingStyle::StBlocking)
            .transition(
                &TraceObjectForwardMessage::<TestPayload>::MsgTraceObjectsRequest {
                    blocking: StBlockingStyle::StBlocking,
                    n_trace_objects: NumberOfTraceObjects(1),
                },
            )
            .expect_err("illegal");
        assert_eq!(
            err,
            TraceObjectForwardTransitionError::IllegalTransition {
                from: TraceObjectForwardState::StBusy(StBlockingStyle::StBlocking),
                msg_tag: "MsgTraceObjectsRequest",
            }
        );
    }

    #[test]
    fn done_state_is_terminal_for_all_messages() {
        let done = TraceObjectForwardState::StDone;
        let req: TraceObjectForwardMessage<TestPayload> =
            TraceObjectForwardMessage::MsgTraceObjectsRequest {
                blocking: StBlockingStyle::StBlocking,
                n_trace_objects: NumberOfTraceObjects(1),
            };
        assert!(done.transition(&req).is_err());
        let rep: TraceObjectForwardMessage<TestPayload> =
            TraceObjectForwardMessage::MsgTraceObjectsReply {
                reply: BlockingReplyList::non_blocking(vec![]),
            };
        assert!(done.transition(&rep).is_err());
        let d: TraceObjectForwardMessage<TestPayload> = TraceObjectForwardMessage::MsgDone;
        assert!(done.transition(&d).is_err());
    }
}
