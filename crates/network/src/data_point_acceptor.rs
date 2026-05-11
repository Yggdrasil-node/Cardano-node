//! DataPointForward mini-protocol acceptor driver.
//!
//! Wraps a [`crate::mux::ProtocolHandle`] with typed send/receive
//! methods that maintain the DataPointForward state machine
//! invariants. The acceptor (cardano-tracer side) periodically
//! requests data-points by name from the forwarder (cardano-node
//! side) and consumes the replies.
//!
//! Per upstream's protocol convention, the **acceptor** is the
//! protocol's *client* (it issues requests + drives the
//! conversation) and the **forwarder** is the protocol's *server*
//! (it answers requests with `(name, maybe-bytes)` pairs).
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Protocol/DataPoint/Acceptor.hs.
//!
//! Filename flattens the upstream directory; mirror of upstream's
//! `Trace.Forward.Protocol.DataPoint.Acceptor` continuation-style
//! `data DataPointAcceptor m a where ...` plus the
//! `dataPointAcceptorPeer` interpretation function. Yggdrasil
//! collapses the continuation-passing data type into direct
//! method calls on a [`DataPointAcceptor`] driver struct, matching
//! the precedent set by [`crate::trace_object_acceptor`] (R419).
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                              |
//! |---------------------------------------------------------|----------------------------------------|
//! | `data DataPointAcceptor m a where ...`                  | [`DataPointAcceptor`]                  |
//! | `SendMsgDataPointsRequest [DataPointName] cont`         | [`DataPointAcceptor::request`]         |
//! | `SendMsgDone (m a)`                                     | [`DataPointAcceptor::done`]            |
//! | `dataPointAcceptorPeer :: ... -> Client ... 'StIdle`    | (collapsed — driver methods drive the typed-protocol loop directly) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **Continuation-passing-style API**: upstream's
//!   `(DataPointValues -> m (DataPointAcceptor m a))` continuation
//!   parameter encodes a "next acceptor program" as an
//!   inversion-of-control callback. Rust's `async fn` makes this
//!   inversion unnecessary — callers just `.await` `request` and
//!   inspect the returned reply directly.
//! - **`Network.TypedProtocol.Peer.Client` machinery**: upstream's
//!   `Yield`/`Await`/`Effect`/`Done` peer-construction primitives
//!   collapse into direct mux send/recv calls.
//! - **`getResult :: m a` final-result parameter on
//!   `SendMsgDone`**: upstream threads a final-result continuation
//!   through `Done`. Yggdrasil returns `Result<(), _>` from
//!   [`DataPointAcceptor::done`]; any final result is computed by
//!   the caller after `done()` returns.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    DataPointForwardMessage, DataPointForwardState, DataPointForwardTransitionError, DataPointName,
    DataPointValues,
};

// ---------------------------------------------------------------------------
// Acceptor error
// ---------------------------------------------------------------------------

/// Errors from the DataPointForward acceptor driver.
#[derive(Debug, thiserror::Error)]
pub enum DataPointAcceptorError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] DataPointForwardTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the forwarder (got a non-reply
    /// message in `StBusy`, or a non-request message in `StIdle`,
    /// etc.).
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),

    /// Caller invoked a method outside the legal protocol state
    /// (e.g. `request` after `done`).
    #[error("invalid acceptor state: {actual:?}; required {required:?}")]
    InvalidState {
        /// The current acceptor state.
        actual: DataPointForwardState,
        /// The state required by the called method.
        required: DataPointForwardState,
    },
}

// ---------------------------------------------------------------------------
// DataPointAcceptor
// ---------------------------------------------------------------------------

/// A DataPointForward acceptor driver maintaining the protocol
/// state machine.
///
/// Usage:
/// 1. Call [`Self::request`] with a list of data-point names — the
///    driver sends `MsgDataPointsRequest` and awaits
///    `MsgDataPointsReply`, returning the `(name, maybe-bytes)`
///    payload.
/// 2. Repeat step 1 as many times as needed (each call is one
///    request/reply round-trip; the driver re-enters `StIdle` on
///    completion).
/// 3. Call [`Self::done`] to terminate the protocol cleanly.
pub struct DataPointAcceptor {
    channel: MessageChannel,
    state: DataPointForwardState,
}

impl DataPointAcceptor {
    /// Create a new acceptor driver from a DataPointForward
    /// `ProtocolHandle`. The protocol starts in `StIdle` — acceptor
    /// (client) agency, ready to send the first request.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: DataPointForwardState::StIdle,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> DataPointForwardState {
        self.state
    }

    /// Send a `MsgDataPointsRequest` with the supplied data-point
    /// names + await + decode the matching `MsgDataPointsReply`.
    /// Re-enters `StIdle` on completion.
    ///
    /// Mirror of upstream's `SendMsgDataPointsRequest [DataPointName]
    /// cont` data constructor + the corresponding `Yield`/`Await`
    /// peer interpretation in `dataPointAcceptorPeer`.
    ///
    /// Must be called when the acceptor is in `StIdle` (acceptor
    /// agency).
    pub async fn request(
        &mut self,
        names: Vec<DataPointName>,
    ) -> Result<DataPointValues, DataPointAcceptorError> {
        if self.state != DataPointForwardState::StIdle {
            return Err(DataPointAcceptorError::InvalidState {
                actual: self.state,
                required: DataPointForwardState::StIdle,
            });
        }

        // Send MsgDataPointsRequest.
        let request = DataPointForwardMessage::MsgDataPointsRequest(names);
        self.state = self.state.transition(&request)?;
        self.channel
            .send(request.to_cbor())
            .await
            .map_err(DataPointAcceptorError::Mux)?;

        // Receive MsgDataPointsReply.
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(DataPointAcceptorError::ConnectionClosed)?;
        let reply_msg = DataPointForwardMessage::from_cbor_in_state(self.state, &raw)
            .map_err(|e| DataPointAcceptorError::Decode(e.to_string()))?;

        match reply_msg {
            DataPointForwardMessage::MsgDataPointsReply(values) => {
                // `from_cbor_in_state` validated state-tag agreement;
                // move back to StIdle directly.
                self.state = DataPointForwardState::StIdle;
                Ok(values)
            }
            other => Err(DataPointAcceptorError::UnexpectedMessage(format!(
                "{} in state {:?}",
                other.tag(),
                self.state
            ))),
        }
    }

    /// Terminate the protocol by sending `MsgDone`. Consumes the
    /// driver. Mirror of upstream's `SendMsgDone` data constructor +
    /// the corresponding `Effect (Yield MsgDone . Done)` peer
    /// interpretation.
    ///
    /// Must be called when the acceptor is in `StIdle`.
    pub async fn done(mut self) -> Result<(), DataPointAcceptorError> {
        if self.state != DataPointForwardState::StIdle {
            return Err(DataPointAcceptorError::InvalidState {
                actual: self.state,
                required: DataPointForwardState::StIdle,
            });
        }
        let msg = DataPointForwardMessage::MsgDone;
        self.state = self.state.transition(&msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(DataPointAcceptorError::Mux)?;
        Ok(())
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::mux::{MiniProtocolDir, MiniProtocolNum, MuxHandle, start_unix};
    use crate::protocols::DataPointValue;
    use tokio::net::UnixStream;

    /// Trace-forwarder uses its own sub-protocol number-space
    /// (independent of NtN/NtC). Per upstream's
    /// `Cardano.Tracer.Acceptors.Server`, the DataPoints sub-
    /// protocol gets number 3.
    const DATA_POINTS_NUM: MiniProtocolNum = MiniProtocolNum(3);

    /// Spin up a connected mux pair over a Unix-stream pair and
    /// return the two protocol handles + the two mux handles.
    /// The mux handles MUST be kept alive for the duration of the
    /// test — dropping aborts the mux tasks mid-test and breaks the
    /// send/recv plumbing.
    fn protocol_handle_pair() -> (ProtocolHandle, ProtocolHandle, MuxHandle, MuxHandle) {
        let (a_stream, f_stream) = UnixStream::pair().expect("unix stream pair");
        let (mut a_handles, a_mux) =
            start_unix(a_stream, MiniProtocolDir::Initiator, &[DATA_POINTS_NUM], 1);
        let (mut f_handles, f_mux) =
            start_unix(f_stream, MiniProtocolDir::Responder, &[DATA_POINTS_NUM], 1);
        let a = a_handles.remove(&DATA_POINTS_NUM).expect("acceptor handle");
        let f = f_handles
            .remove(&DATA_POINTS_NUM)
            .expect("forwarder handle");
        (a, f, a_mux, f_mux)
    }

    #[tokio::test]
    async fn acceptor_starts_in_stidle() {
        let (a, _f, _a_mux, _f_mux) = protocol_handle_pair();
        let acceptor = DataPointAcceptor::new(a);
        assert_eq!(acceptor.state(), DataPointForwardState::StIdle);
    }

    #[tokio::test]
    async fn acceptor_request_round_trip() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut acceptor = DataPointAcceptor::new(a_handle);
        let mut forwarder = MessageChannel::new(f_handle);

        let forwarder_task = tokio::spawn(async move {
            let raw = forwarder.recv().await.expect("forwarder recv");
            let req =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw)
                    .expect("decode request");
            assert_eq!(
                req,
                DataPointForwardMessage::MsgDataPointsRequest(vec![
                    DataPointName::new("node-info"),
                    DataPointName::new("tip"),
                ])
            );
            let reply = DataPointForwardMessage::MsgDataPointsReply(vec![
                (
                    DataPointName::new("node-info"),
                    Some(DataPointValue::new(b"{\"version\":\"11.0.1\"}".to_vec())),
                ),
                (DataPointName::new("tip"), None),
            ]);
            forwarder
                .send(reply.to_cbor())
                .await
                .expect("forwarder send");
        });

        let payloads = acceptor
            .request(vec![
                DataPointName::new("node-info"),
                DataPointName::new("tip"),
            ])
            .await
            .expect("acceptor request");
        assert_eq!(payloads.len(), 2);
        assert_eq!(payloads[0].0, DataPointName::new("node-info"));
        assert_eq!(
            payloads[0].1.as_ref().expect("Just"),
            &DataPointValue::new(b"{\"version\":\"11.0.1\"}".to_vec())
        );
        assert_eq!(payloads[1].0, DataPointName::new("tip"));
        assert!(payloads[1].1.is_none());
        assert_eq!(acceptor.state(), DataPointForwardState::StIdle);
        forwarder_task.await.expect("forwarder task");
    }

    #[tokio::test]
    async fn acceptor_request_empty_names_legal() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut acceptor = DataPointAcceptor::new(a_handle);
        let mut forwarder = MessageChannel::new(f_handle);

        let forwarder_task = tokio::spawn(async move {
            let raw = forwarder.recv().await.expect("forwarder recv");
            let req =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw)
                    .expect("decode");
            assert_eq!(req, DataPointForwardMessage::MsgDataPointsRequest(vec![]));
            let reply = DataPointForwardMessage::MsgDataPointsReply(vec![]);
            forwarder
                .send(reply.to_cbor())
                .await
                .expect("forwarder send");
        });

        let payloads = acceptor
            .request(vec![])
            .await
            .expect("acceptor empty request");
        assert!(payloads.is_empty());
        forwarder_task.await.expect("forwarder task");
    }

    #[tokio::test]
    async fn acceptor_multiple_requests_sequential() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut acceptor = DataPointAcceptor::new(a_handle);
        let mut forwarder = MessageChannel::new(f_handle);

        let forwarder_task = tokio::spawn(async move {
            for round in 0..3u32 {
                let raw = forwarder.recv().await.expect("forwarder recv");
                let _req = DataPointForwardMessage::from_cbor_in_state(
                    DataPointForwardState::StIdle,
                    &raw,
                )
                .expect("decode");
                let reply = DataPointForwardMessage::MsgDataPointsReply(vec![(
                    DataPointName::new("round"),
                    Some(DataPointValue::new(vec![round as u8])),
                )]);
                forwarder
                    .send(reply.to_cbor())
                    .await
                    .expect("forwarder send");
            }
        });

        for expected_round in 0..3u32 {
            let payloads = acceptor
                .request(vec![DataPointName::new("round")])
                .await
                .expect("acceptor request");
            assert_eq!(payloads.len(), 1);
            assert_eq!(
                payloads[0].1.as_ref().expect("Just").as_slice(),
                &[expected_round as u8]
            );
            assert_eq!(acceptor.state(), DataPointForwardState::StIdle);
        }
        forwarder_task.await.expect("forwarder task");
    }

    #[tokio::test]
    async fn acceptor_done_terminates_cleanly() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let acceptor = DataPointAcceptor::new(a_handle);
        let mut forwarder = MessageChannel::new(f_handle);

        let forwarder_task = tokio::spawn(async move {
            let raw = forwarder.recv().await.expect("forwarder recv");
            let msg =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw)
                    .expect("decode");
            assert_eq!(msg, DataPointForwardMessage::MsgDone);
        });

        acceptor.done().await.expect("acceptor done");
        forwarder_task.await.expect("forwarder task");
    }

    #[tokio::test]
    async fn acceptor_done_after_request_then_reply_succeeds() {
        // Exercise the full canonical flow: request → reply →
        // (back to StIdle) → done.
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut acceptor = DataPointAcceptor::new(a_handle);
        let mut forwarder = MessageChannel::new(f_handle);

        let forwarder_task = tokio::spawn(async move {
            // First exchange — request + reply.
            let raw = forwarder.recv().await.expect("forwarder recv 1");
            let _req =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw)
                    .expect("decode 1");
            let reply =
                DataPointForwardMessage::MsgDataPointsReply(vec![(DataPointName::new("x"), None)]);
            forwarder
                .send(reply.to_cbor())
                .await
                .expect("forwarder send 1");

            // Second exchange — done.
            let raw2 = forwarder.recv().await.expect("forwarder recv 2");
            let msg =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw2)
                    .expect("decode 2");
            assert_eq!(msg, DataPointForwardMessage::MsgDone);
        });

        acceptor
            .request(vec![DataPointName::new("x")])
            .await
            .expect("request");
        acceptor.done().await.expect("done");
        forwarder_task.await.expect("forwarder task");
    }
}
