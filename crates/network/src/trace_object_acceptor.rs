//! TraceObjectForward mini-protocol acceptor driver.
//!
//! Wraps a [`crate::mux::ProtocolHandle`] with typed send/receive
//! methods that maintain the TraceObjectForward state machine
//! invariants. The acceptor (cardano-tracer side) periodically
//! requests batches of `TraceObject`s from the forwarder
//! (cardano-node side) and consumes the replies.
//!
//! Per upstream's protocol convention, the **acceptor** is the
//! protocol's *client* (it issues requests + drives the
//! conversation) and the **forwarder** is the protocol's *server*
//! (it answers requests with batches of trace objects). This
//! driver provides the acceptor side.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Protocol/TraceObject/Acceptor.hs.
//!
//! Filename flattens the upstream directory; mirror of upstream's
//! `Trace.Forward.Protocol.TraceObject.Acceptor` continuation-style
//! `data TraceObjectAcceptor lo m a where ...` plus the
//! `traceObjectAcceptorPeer` interpretation function. Yggdrasil
//! collapses the continuation-passing data type into direct
//! method calls on a [`TraceObjectAcceptor`] driver struct,
//! matching the precedent set by `keepalive_client.rs` (the
//! KeepAlive analog).
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                              |
//! |---------------------------------------------------------|----------------------------------------|
//! | `data TraceObjectAcceptor lo m a where ...`             | [`TraceObjectAcceptor`]                |
//! | `SendMsgTraceObjectsRequest TokBlocking n cont`         | [`TraceObjectAcceptor::request_blocking`]    |
//! | `SendMsgTraceObjectsRequest TokNonBlocking n cont`      | [`TraceObjectAcceptor::request_non_blocking`] |
//! | `SendMsgDone (m a)`                                     | [`TraceObjectAcceptor::done`]          |
//! | `traceObjectAcceptorPeer :: ... -> Client ... 'StIdle`  | (collapsed — driver methods drive the typed-protocol loop directly) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **Continuation-passing-style API**: upstream's
//!   `(BlockingReplyList blocking lo -> m (TraceObjectAcceptor lo m a))`
//!   continuation parameter encodes a "next acceptor program" as an
//!   inversion-of-control callback. Rust's `async fn` makes this
//!   inversion unnecessary — callers just `.await` `request_blocking`
//!   and inspect the returned reply directly.
//! - **`Network.TypedProtocol.Peer.Client` machinery**: upstream's
//!   `Yield`/`Await`/`Effect`/`Done` peer-construction primitives
//!   collapse into direct mux send/recv calls.

use std::marker::PhantomData;

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::Decoder;

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    NumberOfTraceObjects, StBlockingStyle, TraceObjectForwardMessage, TraceObjectForwardState,
    TraceObjectForwardTransitionError,
};

// ---------------------------------------------------------------------------
// Acceptor error
// ---------------------------------------------------------------------------

/// Errors from the TraceObjectForward acceptor driver.
#[derive(Debug, thiserror::Error)]
pub enum TraceObjectAcceptorError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] TraceObjectForwardTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the forwarder (got a non-reply
    /// message in `StBusy`, or a non-request message in `StIdle`,
    /// etc.).
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),

    /// Caller invoked a method outside the legal protocol state
    /// (e.g. `request_blocking` after `done`).
    #[error("invalid acceptor state: {actual:?}; required {required:?}")]
    InvalidState {
        /// The current acceptor state.
        actual: TraceObjectForwardState,
        /// The state required by the called method.
        required: TraceObjectForwardState,
    },
}

// ---------------------------------------------------------------------------
// TraceObjectAcceptor
// ---------------------------------------------------------------------------

/// A TraceObjectForward acceptor driver maintaining the protocol
/// state machine.
///
/// Usage:
/// 1. Call [`Self::request_blocking`] or
///    [`Self::request_non_blocking`] with a batch size — the driver
///    sends `MsgTraceObjectsRequest` and awaits
///    `MsgTraceObjectsReply`, returning the trace-object payload.
/// 2. Repeat step 1 as many times as needed (each call is one
///    request/reply round-trip; the driver re-enters `StIdle` on
///    completion).
/// 3. Call [`Self::done`] to terminate the protocol cleanly.
pub struct TraceObjectAcceptor<TraceObj> {
    channel: MessageChannel,
    state: TraceObjectForwardState,
    _phantom: PhantomData<TraceObj>,
}

impl<TraceObj> TraceObjectAcceptor<TraceObj> {
    /// Create a new acceptor driver from a TraceObjectForward
    /// `ProtocolHandle`. The protocol starts in `StIdle` — acceptor
    /// (client) agency, ready to send the first request.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: TraceObjectForwardState::StIdle,
            _phantom: PhantomData,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> TraceObjectForwardState {
        self.state
    }

    // ---- helpers ----------------------------------------------------------

    /// Send a `MsgTraceObjectsRequest` with the supplied blocking
    /// style and batch size, then await + decode the matching
    /// `MsgTraceObjectsReply`. Re-enters `StIdle` on completion.
    async fn send_request_recv_reply<F>(
        &mut self,
        blocking: StBlockingStyle,
        n_trace_objects: NumberOfTraceObjects,
        decode_reply_list: F,
    ) -> Result<Vec<TraceObj>, TraceObjectAcceptorError>
    where
        F: FnMut(&mut Decoder<'_>) -> Result<Vec<TraceObj>, LedgerError>,
    {
        if self.state != TraceObjectForwardState::StIdle {
            return Err(TraceObjectAcceptorError::InvalidState {
                actual: self.state,
                required: TraceObjectForwardState::StIdle,
            });
        }

        // Send MsgTraceObjectsRequest. Encode-side closure is
        // unused for request-only messages; the codec ignores it.
        let request: TraceObjectForwardMessage<TraceObj> =
            TraceObjectForwardMessage::MsgTraceObjectsRequest {
                blocking,
                n_trace_objects,
            };
        self.state = self.state.transition(&request)?;
        self.channel
            .send(request.to_cbor(|_, _| ()))
            .await
            .map_err(TraceObjectAcceptorError::Mux)?;

        // Receive MsgTraceObjectsReply. Decode-side closure is
        // user-supplied via `decode_reply_list`.
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(TraceObjectAcceptorError::ConnectionClosed)?;
        let reply_msg = TraceObjectForwardMessage::<TraceObj>::from_cbor_in_state(
            self.state,
            &raw,
            decode_reply_list,
        )
        .map_err(|e| TraceObjectAcceptorError::Decode(e.to_string()))?;

        match reply_msg {
            TraceObjectForwardMessage::MsgTraceObjectsReply { reply } => {
                // `from_cbor_in_state` already validated the blocking-
                // style match against `self.state.StBusy(blocking)` —
                // re-running `transition()` with a synthesized dummy
                // reply would just re-check the same invariant. Move
                // back to `StIdle` directly.
                self.state = TraceObjectForwardState::StIdle;
                Ok(reply.into_items())
            }
            other => Err(TraceObjectAcceptorError::UnexpectedMessage(format!(
                "{} in state {:?}",
                other.tag(),
                self.state
            ))),
        }
    }

    // ---- public API -------------------------------------------------------

    /// Send a blocking `MsgTraceObjectsRequest` with the supplied
    /// batch size + await the reply. The forwarder may take its time
    /// replying; the reply will always carry at least one trace
    /// object (upstream's `NonEmpty lo` invariant).
    ///
    /// Mirror of upstream's
    /// `SendMsgTraceObjectsRequest TokBlocking n cont` data
    /// constructor + the corresponding `Yield`/`Await` peer
    /// interpretation in `traceObjectAcceptorPeer`.
    ///
    /// Must be called when the acceptor is in `StIdle` (acceptor
    /// agency).
    pub async fn request_blocking<F>(
        &mut self,
        n_trace_objects: NumberOfTraceObjects,
        decode_reply_list: F,
    ) -> Result<Vec<TraceObj>, TraceObjectAcceptorError>
    where
        F: FnMut(&mut Decoder<'_>) -> Result<Vec<TraceObj>, LedgerError>,
    {
        self.send_request_recv_reply(
            StBlockingStyle::StBlocking,
            n_trace_objects,
            decode_reply_list,
        )
        .await
    }

    /// Send a non-blocking `MsgTraceObjectsRequest` with the
    /// supplied batch size + await the reply. The forwarder is
    /// expected to reply promptly; the reply may be empty.
    ///
    /// Mirror of upstream's
    /// `SendMsgTraceObjectsRequest TokNonBlocking n cont`.
    ///
    /// Must be called when the acceptor is in `StIdle`.
    pub async fn request_non_blocking<F>(
        &mut self,
        n_trace_objects: NumberOfTraceObjects,
        decode_reply_list: F,
    ) -> Result<Vec<TraceObj>, TraceObjectAcceptorError>
    where
        F: FnMut(&mut Decoder<'_>) -> Result<Vec<TraceObj>, LedgerError>,
    {
        self.send_request_recv_reply(
            StBlockingStyle::StNonBlocking,
            n_trace_objects,
            decode_reply_list,
        )
        .await
    }

    /// Terminate the protocol by sending `MsgDone`. Consumes the
    /// driver. Mirror of upstream's `SendMsgDone` data constructor +
    /// the corresponding `Effect (Yield MsgDone . Done)` peer
    /// interpretation.
    ///
    /// Must be called when the acceptor is in `StIdle`.
    pub async fn done(mut self) -> Result<(), TraceObjectAcceptorError> {
        if self.state != TraceObjectForwardState::StIdle {
            return Err(TraceObjectAcceptorError::InvalidState {
                actual: self.state,
                required: TraceObjectForwardState::StIdle,
            });
        }
        let msg: TraceObjectForwardMessage<TraceObj> = TraceObjectForwardMessage::MsgDone;
        self.state = self.state.transition(&msg)?;
        self.channel
            .send(msg.to_cbor(|_, _| ()))
            .await
            .map_err(TraceObjectAcceptorError::Mux)?;
        Ok(())
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::mux::{MiniProtocolDir, MiniProtocolNum, MuxHandle, start_unix};
    use crate::protocols::BlockingReplyList;
    use tokio::net::UnixStream;
    use yggdrasil_ledger::cbor::Encoder;

    /// Tiny stand-in payload for protocol-level tests.
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TestPayload(u32);

    /// Trace-forwarder uses its own sub-protocol number-space
    /// (independent of NtN/NtC). Per upstream's
    /// `Cardano.Tracer.Acceptors.Server`, the TraceObjects sub-
    /// protocol gets number 2.
    const TRACE_OBJECTS_NUM: MiniProtocolNum = MiniProtocolNum(2);

    fn encode_test_payloads(enc: &mut Encoder, list: &[TestPayload]) {
        enc.array(list.len() as u64);
        for p in list {
            enc.unsigned(u64::from(p.0));
        }
    }

    fn decode_test_payloads(dec: &mut Decoder<'_>) -> Result<Vec<TestPayload>, LedgerError> {
        let len = dec.array()?;
        let mut out = Vec::with_capacity(len as usize);
        for _ in 0..len {
            out.push(TestPayload(dec.unsigned()? as u32));
        }
        Ok(out)
    }

    /// Spin up a connected mux pair over a Unix-stream pair and
    /// return the two protocol handles + the two mux handles.
    /// The mux handles MUST be kept alive (e.g. via `let _a_mux`)
    /// for the duration of the test — dropping aborts the mux
    /// tasks mid-test and breaks the send/recv plumbing.
    /// Matches the precedent in `chainsync_client.rs`.
    fn protocol_handle_pair() -> (ProtocolHandle, ProtocolHandle, MuxHandle, MuxHandle) {
        let (a_stream, f_stream) = UnixStream::pair().expect("unix stream pair");
        let (mut a_handles, a_mux) = start_unix(
            a_stream,
            MiniProtocolDir::Initiator,
            &[TRACE_OBJECTS_NUM],
            1,
        );
        let (mut f_handles, f_mux) = start_unix(
            f_stream,
            MiniProtocolDir::Responder,
            &[TRACE_OBJECTS_NUM],
            1,
        );
        let a = a_handles
            .remove(&TRACE_OBJECTS_NUM)
            .expect("acceptor handle");
        let f = f_handles
            .remove(&TRACE_OBJECTS_NUM)
            .expect("forwarder handle");
        (a, f, a_mux, f_mux)
    }

    #[tokio::test]
    async fn acceptor_starts_in_stidle() {
        let (a, _f, _a_mux, _f_mux) = protocol_handle_pair();
        let acceptor: TraceObjectAcceptor<TestPayload> = TraceObjectAcceptor::new(a);
        assert_eq!(acceptor.state(), TraceObjectForwardState::StIdle);
    }

    #[tokio::test]
    async fn acceptor_request_blocking_round_trip() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut acceptor: TraceObjectAcceptor<TestPayload> = TraceObjectAcceptor::new(a_handle);
        let mut forwarder = MessageChannel::new(f_handle);

        let forwarder_task = tokio::spawn(async move {
            let raw = forwarder.recv().await.expect("forwarder recv");
            let req = TraceObjectForwardMessage::<TestPayload>::from_cbor_in_state(
                TraceObjectForwardState::StIdle,
                &raw,
                decode_test_payloads,
            )
            .expect("decode request");
            assert!(matches!(
                req,
                TraceObjectForwardMessage::MsgTraceObjectsRequest {
                    blocking: StBlockingStyle::StBlocking,
                    n_trace_objects: NumberOfTraceObjects(5),
                }
            ));
            let reply: TraceObjectForwardMessage<TestPayload> =
                TraceObjectForwardMessage::MsgTraceObjectsReply {
                    reply: BlockingReplyList::blocking(vec![
                        TestPayload(1),
                        TestPayload(2),
                        TestPayload(3),
                    ])
                    .expect("seed"),
                };
            forwarder
                .send(reply.to_cbor(encode_test_payloads))
                .await
                .expect("forwarder send");
        });

        let payloads = acceptor
            .request_blocking(NumberOfTraceObjects(5), decode_test_payloads)
            .await
            .expect("acceptor blocking request");
        assert_eq!(
            payloads,
            vec![TestPayload(1), TestPayload(2), TestPayload(3)]
        );
        assert_eq!(acceptor.state(), TraceObjectForwardState::StIdle);
        forwarder_task.await.expect("forwarder task");
    }

    #[tokio::test]
    async fn acceptor_request_non_blocking_empty_reply() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut acceptor: TraceObjectAcceptor<TestPayload> = TraceObjectAcceptor::new(a_handle);
        let mut forwarder = MessageChannel::new(f_handle);

        let forwarder_task = tokio::spawn(async move {
            let raw = forwarder.recv().await.expect("forwarder recv");
            let req = TraceObjectForwardMessage::<TestPayload>::from_cbor_in_state(
                TraceObjectForwardState::StIdle,
                &raw,
                decode_test_payloads,
            )
            .expect("decode");
            assert!(matches!(
                req,
                TraceObjectForwardMessage::MsgTraceObjectsRequest {
                    blocking: StBlockingStyle::StNonBlocking,
                    ..
                }
            ));
            let reply: TraceObjectForwardMessage<TestPayload> =
                TraceObjectForwardMessage::MsgTraceObjectsReply {
                    reply: BlockingReplyList::non_blocking(vec![]),
                };
            forwarder
                .send(reply.to_cbor(encode_test_payloads))
                .await
                .expect("forwarder send");
        });

        let payloads = acceptor
            .request_non_blocking(NumberOfTraceObjects(0), decode_test_payloads)
            .await
            .expect("acceptor non-blocking request");
        assert!(payloads.is_empty());
        forwarder_task.await.expect("forwarder task");
    }

    #[tokio::test]
    async fn acceptor_done_terminates_cleanly() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let acceptor: TraceObjectAcceptor<TestPayload> = TraceObjectAcceptor::new(a_handle);
        let mut forwarder = MessageChannel::new(f_handle);

        let forwarder_task = tokio::spawn(async move {
            let raw = forwarder.recv().await.expect("forwarder recv");
            let msg = TraceObjectForwardMessage::<TestPayload>::from_cbor_in_state(
                TraceObjectForwardState::StIdle,
                &raw,
                decode_test_payloads,
            )
            .expect("decode");
            assert!(matches!(msg, TraceObjectForwardMessage::MsgDone));
        });

        acceptor.done().await.expect("acceptor done");
        forwarder_task.await.expect("forwarder task");
    }
}
