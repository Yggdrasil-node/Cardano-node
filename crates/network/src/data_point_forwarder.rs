//! DataPointForward mini-protocol forwarder driver.
//!
//! Wraps a [`crate::mux::ProtocolHandle`] with typed receive/send
//! methods that maintain the DataPointForward state machine
//! invariants. The forwarder (cardano-node side) waits for the
//! acceptor's `MsgDataPointsRequest`, looks up the requested names
//! in its data-point store, and replies with `MsgDataPointsReply`.
//!
//! Per upstream's protocol convention, the **acceptor** is the
//! protocol's *client* (it issues requests) and the **forwarder**
//! is the protocol's *server* (it answers requests).
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Protocol/DataPoint/Forwarder.hs.
//!
//! Filename flattens the upstream directory; mirror of upstream's
//! `Trace.Forward.Protocol.DataPoint.Forwarder` callback-record-
//! style `DataPointForwarder m a` type + the
//! `dataPointForwarderPeer` interpretation function. Yggdrasil
//! collapses the typed-protocol server-peer machinery into direct
//! method calls on a [`DataPointForwarder`] driver struct, matching
//! the precedent set by [`crate::data_point_acceptor`] (R454).
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                              |
//! |---------------------------------------------------------|----------------------------------------|
//! | `data DataPointForwarder m a = DataPointForwarder { ... }` | [`DataPointForwarder`]              |
//! | `recvMsgDataPointsRequest :: [DataPointName] -> m DataPointValues` | [`DataPointForwarder::wait_for_request`] (return names; caller computes values + calls `send_reply`) |
//! | `recvMsgDone :: m a`                                    | [`DataPointForwarder::wait_for_done`] (returns `()`)  |
//! | `dataPointForwarderPeer :: ... -> Server ... 'StIdle`   | (collapsed — driver methods drive the typed-protocol loop directly) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **Callback-record API**: upstream's `recvMsgDataPointsRequest
//!   :: [DataPointName] -> m DataPointValues` continuation pattern
//!   inverts control — the caller passes a function that computes
//!   the reply. Yggdrasil's port returns the names from
//!   `wait_for_request` and has the caller send the reply via
//!   `send_reply` — direct async-method calls rather than
//!   callback registration. Same pattern as R454's
//!   [`DataPointAcceptor`].
//! - **`Network.TypedProtocol.Peer.Server` machinery**: upstream's
//!   `Await`/`Yield`/`Effect`/`Done` peer-construction primitives
//!   collapse into direct mux send/recv calls.
//! - **Final `a` return value on `recvMsgDone`**: upstream allows
//!   the forwarder to compute an arbitrary final value. Yggdrasil
//!   returns `Result<(), _>` from `wait_for_done`; any final
//!   computation is done by the caller after the function returns.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    DataPointForwardMessage, DataPointForwardState, DataPointForwardTransitionError, DataPointName,
    DataPointValues,
};

// ---------------------------------------------------------------------------
// Forwarder error
// ---------------------------------------------------------------------------

/// Errors from the DataPointForward forwarder driver.
#[derive(Debug, thiserror::Error)]
pub enum DataPointForwarderError {
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

    /// Unexpected message from the acceptor (got a non-request
    /// message in `StIdle`, etc.).
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),

    /// Caller invoked a method outside the legal protocol state
    /// (e.g. `send_reply` before `wait_for_request`).
    #[error("invalid forwarder state: {actual:?}; required {required:?}")]
    InvalidState {
        /// The current forwarder state.
        actual: DataPointForwardState,
        /// The state required by the called method.
        required: DataPointForwardState,
    },
}

/// One transition outcome of [`DataPointForwarder::wait_for_request`].
///
/// The acceptor may either request data-points (advancing the
/// state machine to `StBusy`) or terminate the protocol with
/// `MsgDone`. The driver returns this discriminator so the caller
/// can dispatch on it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataPointForwarderEvent {
    /// The acceptor sent `MsgDataPointsRequest(names)`; the
    /// forwarder is now in `StBusy` and must reply via
    /// [`DataPointForwarder::send_reply`].
    Request(Vec<DataPointName>),
    /// The acceptor sent `MsgDone`; the protocol terminated
    /// cleanly.
    Done,
}

// ---------------------------------------------------------------------------
// DataPointForwarder
// ---------------------------------------------------------------------------

/// A DataPointForward forwarder driver maintaining the protocol
/// state machine.
///
/// Usage:
/// 1. Call [`Self::wait_for_request`] to await the acceptor's next
///    message. Returns `Request(names)` or `Done`.
/// 2. On `Request(names)`: look up the names in your data-point
///    store, then call [`Self::send_reply`] with the
///    `(name, maybe-bytes)` pairs. The protocol returns to
///    `StIdle`.
/// 3. On `Done`: the protocol has terminated cleanly; drop the
///    driver.
pub struct DataPointForwarder {
    channel: MessageChannel,
    state: DataPointForwardState,
}

impl DataPointForwarder {
    /// Create a new forwarder driver from a DataPointForward
    /// `ProtocolHandle`. The protocol starts in `StIdle` —
    /// forwarder (server) is awaiting the acceptor's first request.
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

    /// Await the acceptor's next message and dispatch on its type.
    /// Returns [`DataPointForwarderEvent::Request`] when the
    /// acceptor sends `MsgDataPointsRequest(names)` (the protocol
    /// transitions to `StBusy` — the caller MUST follow up with
    /// [`Self::send_reply`] to return to `StIdle`), or
    /// [`DataPointForwarderEvent::Done`] when the acceptor sends
    /// `MsgDone` (the protocol transitions to `StDone` — the
    /// caller should drop the driver).
    ///
    /// Mirror of upstream's `Await \case MsgDataPointsRequest ... |
    /// MsgDone ...` server-peer dispatch.
    ///
    /// Must be called when the forwarder is in `StIdle` (acceptor
    /// agency — we're waiting for them to send us a message).
    pub async fn wait_for_request(
        &mut self,
    ) -> Result<DataPointForwarderEvent, DataPointForwarderError> {
        if self.state != DataPointForwardState::StIdle {
            return Err(DataPointForwarderError::InvalidState {
                actual: self.state,
                required: DataPointForwardState::StIdle,
            });
        }
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(DataPointForwarderError::ConnectionClosed)?;
        let msg = DataPointForwardMessage::from_cbor_in_state(self.state, &raw)
            .map_err(|e| DataPointForwarderError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        match msg {
            DataPointForwardMessage::MsgDataPointsRequest(names) => {
                Ok(DataPointForwarderEvent::Request(names))
            }
            DataPointForwardMessage::MsgDone => Ok(DataPointForwarderEvent::Done),
            other => Err(DataPointForwarderError::UnexpectedMessage(format!(
                "{} in state {:?}",
                other.tag(),
                self.state
            ))),
        }
    }

    /// Reply to a pending `MsgDataPointsRequest` with the supplied
    /// `(name, maybe-bytes)` pairs. Transitions the protocol back
    /// to `StIdle`.
    ///
    /// Mirror of upstream's
    /// `Yield (MsgDataPointsReply reply) go` server-peer
    /// continuation.
    ///
    /// Must be called when the forwarder is in `StBusy` (i.e.
    /// immediately after [`Self::wait_for_request`] returns
    /// `Request(_)`).
    pub async fn send_reply(
        &mut self,
        values: DataPointValues,
    ) -> Result<(), DataPointForwarderError> {
        if self.state != DataPointForwardState::StBusy {
            return Err(DataPointForwarderError::InvalidState {
                actual: self.state,
                required: DataPointForwardState::StBusy,
            });
        }
        let msg = DataPointForwardMessage::MsgDataPointsReply(values);
        self.state = self.state.transition(&msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(DataPointForwarderError::Mux)?;
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

    const DATA_POINTS_NUM: MiniProtocolNum = MiniProtocolNum(3);

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
    async fn forwarder_starts_in_stidle() {
        let (_a, f, _a_mux, _f_mux) = protocol_handle_pair();
        let forwarder = DataPointForwarder::new(f);
        assert_eq!(forwarder.state(), DataPointForwardState::StIdle);
    }

    #[tokio::test]
    async fn forwarder_request_reply_round_trip() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut forwarder = DataPointForwarder::new(f_handle);
        let mut acceptor = MessageChannel::new(a_handle);

        let acceptor_task = tokio::spawn(async move {
            // Acceptor sends MsgDataPointsRequest(["node-info", "tip"]).
            let req = DataPointForwardMessage::MsgDataPointsRequest(vec![
                DataPointName::new("node-info"),
                DataPointName::new("tip"),
            ]);
            acceptor.send(req.to_cbor()).await.expect("acceptor send");

            // Receive forwarder's reply.
            let raw = acceptor.recv().await.expect("acceptor recv");
            let reply =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StBusy, &raw)
                    .expect("decode reply");
            match reply {
                DataPointForwardMessage::MsgDataPointsReply(values) => values,
                other => panic!("expected reply, got {other:?}"),
            }
        });

        // Forwarder side: wait for request, send reply with canned
        // values.
        let event = forwarder
            .wait_for_request()
            .await
            .expect("wait_for_request");
        let names = match event {
            DataPointForwarderEvent::Request(names) => names,
            other => panic!("expected Request, got {other:?}"),
        };
        assert_eq!(
            names,
            vec![DataPointName::new("node-info"), DataPointName::new("tip")]
        );
        assert_eq!(forwarder.state(), DataPointForwardState::StBusy);

        let reply_values: DataPointValues = vec![
            (
                DataPointName::new("node-info"),
                Some(DataPointValue::new(b"{\"version\":\"11.0.1\"}".to_vec())),
            ),
            (DataPointName::new("tip"), None),
        ];
        forwarder
            .send_reply(reply_values.clone())
            .await
            .expect("send_reply");
        assert_eq!(forwarder.state(), DataPointForwardState::StIdle);

        // Acceptor receives + decodes the canned reply.
        let received = acceptor_task.await.expect("acceptor task");
        assert_eq!(received, reply_values);
    }

    #[tokio::test]
    async fn forwarder_done_terminates_cleanly() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut forwarder = DataPointForwarder::new(f_handle);
        let acceptor = MessageChannel::new(a_handle);

        let acceptor_task = tokio::spawn(async move {
            let done = DataPointForwardMessage::MsgDone;
            acceptor.send(done.to_cbor()).await.expect("send done");
        });

        let event = forwarder
            .wait_for_request()
            .await
            .expect("wait_for_request");
        assert_eq!(event, DataPointForwarderEvent::Done);
        assert_eq!(forwarder.state(), DataPointForwardState::StDone);
        acceptor_task.await.expect("acceptor task");
    }

    #[tokio::test]
    async fn forwarder_multiple_round_trips_sequential() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut forwarder = DataPointForwarder::new(f_handle);
        let mut acceptor = MessageChannel::new(a_handle);

        let acceptor_task = tokio::spawn(async move {
            for round in 0..3u32 {
                let req =
                    DataPointForwardMessage::MsgDataPointsRequest(vec![DataPointName::new("seq")]);
                acceptor.send(req.to_cbor()).await.expect("send req");
                let raw = acceptor.recv().await.expect("recv reply");
                let reply = DataPointForwardMessage::from_cbor_in_state(
                    DataPointForwardState::StBusy,
                    &raw,
                )
                .expect("decode");
                match reply {
                    DataPointForwardMessage::MsgDataPointsReply(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(
                            values[0].1.as_ref().expect("Just").as_slice(),
                            &[round as u8]
                        );
                    }
                    other => panic!("expected reply, got {other:?}"),
                }
            }
        });

        for round in 0..3u32 {
            let event = forwarder
                .wait_for_request()
                .await
                .expect("wait_for_request");
            assert!(matches!(event, DataPointForwarderEvent::Request(_)));
            forwarder
                .send_reply(vec![(
                    DataPointName::new("seq"),
                    Some(DataPointValue::new(vec![round as u8])),
                )])
                .await
                .expect("send_reply");
            assert_eq!(forwarder.state(), DataPointForwardState::StIdle);
        }
        acceptor_task.await.expect("acceptor task");
    }

    #[tokio::test]
    async fn forwarder_send_reply_in_idle_state_errors() {
        let (_a, f, _a_mux, _f_mux) = protocol_handle_pair();
        let mut forwarder = DataPointForwarder::new(f);
        // Calling send_reply before wait_for_request should error
        // (we're in StIdle, not StBusy).
        let result = forwarder.send_reply(vec![]).await;
        assert!(matches!(
            result,
            Err(DataPointForwarderError::InvalidState { .. })
        ));
    }

    #[tokio::test]
    async fn forwarder_wait_for_request_in_done_state_errors() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let mut forwarder = DataPointForwarder::new(f_handle);
        let acceptor = MessageChannel::new(a_handle);

        // First: terminate the protocol via MsgDone.
        let _acceptor_task = tokio::spawn(async move {
            acceptor
                .send(DataPointForwardMessage::MsgDone.to_cbor())
                .await
                .expect("send done");
        });
        let _ = forwarder.wait_for_request().await;
        assert_eq!(forwarder.state(), DataPointForwardState::StDone);

        // Now wait_for_request again — should error.
        let result = forwarder.wait_for_request().await;
        assert!(matches!(
            result,
            Err(DataPointForwarderError::InvalidState { .. })
        ));
    }
}
