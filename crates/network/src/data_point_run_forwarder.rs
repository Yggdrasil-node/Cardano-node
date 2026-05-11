//! Trace-forwarder DataPoint forwarder-side runtime aggregator.
//!
//! Wires the trace-forwarder DataPoint forwarder configuration +
//! codec + forwarder driver + `DataPointStore` into a single async
//! function spawnable by the trace-forwarder mini-protocol layer.
//! Implements the upstream `dataPointForwarderPeer` loop (await
//! request → look up names in store → send reply → loop until
//! MsgDone) mux-wired.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Run/DataPoint/Forwarder.hs.
//!
//! Filename flattens the upstream directory; mirror of upstream's
//! `Trace.Forward.Run.DataPoint.Forwarder` module which exposes
//! `forwardDataPointsInit` (initiator-mode) +
//! `forwardDataPointsResp` (responder-mode) entry points + the
//! `runPeerWithDPStore` helper.
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `forwardDataPointsResp config dpStore`                        | [`forward_data_points_resp`]           |
//! | `forwardDataPointsInit config dpStore`                        | [`forward_data_points_init`]           |
//! | `runPeerWithDPStore config dpStore`                           | (collapsed — both `*_init` / `*_resp` call into [`run_forwarder_loop`] directly) |
//! | `dataPointForwarderPeer (readFromStore dpStore)` (server)     | [`run_forwarder_loop`] (loop body)     |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`InitiatorProtocolOnly` / `ResponderProtocolOnly`
//!   `RunMiniProtocol` shapes**: same as the acceptor-side R457
//!   precedent — Yggdrasil's mux doesn't carry the role
//!   distinction in the function signature.
//! - **`Network.Mux.MiniProtocolCb`**: same as R457 — Yggdrasil
//!   exposes the loop as a plain `async fn`; callers spawn it via
//!   `tokio::task::spawn` after acquiring the protocol handle.
//! - **`Ouroboros.Network.Driver.Simple.runPeer` typed-protocol
//!   driver loop**: collapses since R471's [`DataPointForwarder`]
//!   already exposes the per-state driver methods directly.

use crate::data_point_forwarder::{
    DataPointForwarder, DataPointForwarderError, DataPointForwarderEvent,
};
use crate::mux::ProtocolHandle;
use crate::protocols::{DataPointForwarderConfiguration, DataPointStore, read_from_store};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the trace-forwarder DataPoint forwarder runtime.
#[derive(Debug, thiserror::Error)]
pub enum ForwardDataPointsError {
    /// Forwarder protocol error.
    #[error("forwarder protocol error: {0}")]
    Forwarder(#[from] DataPointForwarderError),
}

// ---------------------------------------------------------------------------
// Runtime entry points
// ---------------------------------------------------------------------------

/// Run the forwarder side (responder mode) of the trace-forwarder
/// DataPoint mini-protocol over the supplied protocol handle, using
/// the supplied `DataPointStore` as the source of data-point
/// values.
///
/// Mirror of upstream's `forwardDataPointsResp config dpStore` entry
/// point. Returns `Ok(())` when the acceptor terminates the
/// protocol via `MsgDone`; returns [`ForwardDataPointsError::Forwarder`]
/// on a transport-level failure.
///
/// `config` carries the optional debug tracer + (in upstream, a
/// queueSize field that's absent here since DataPoint forwarder
/// has no internal queue — it serves values on demand from
/// `dpStore`).
pub async fn forward_data_points_resp(
    _config: DataPointForwarderConfiguration,
    handle: ProtocolHandle,
    dp_store: DataPointStore,
) -> Result<(), ForwardDataPointsError> {
    run_forwarder_loop(handle, dp_store).await
}

/// Run the forwarder side (initiator mode) of the trace-forwarder
/// DataPoint mini-protocol. Mirror of upstream's
/// `forwardDataPointsInit`.
///
/// Operationally identical to [`forward_data_points_resp`] —
/// upstream distinguishes the two via the `InitiatorProtocolOnly`
/// / `ResponderProtocolOnly` `RunMiniProtocol` GADT branches, but
/// Yggdrasil's mux layer doesn't carry that role distinction in
/// the function signature. Same precedent as R457's
/// `accept_data_points_{init,resp}` symmetry.
pub async fn forward_data_points_init(
    _config: DataPointForwarderConfiguration,
    handle: ProtocolHandle,
    dp_store: DataPointStore,
) -> Result<(), ForwardDataPointsError> {
    run_forwarder_loop(handle, dp_store).await
}

// ---------------------------------------------------------------------------
// Internal driver loop
// ---------------------------------------------------------------------------

/// Run the forwarder-side recursive loop until the acceptor sends
/// `MsgDone`. Mirror of upstream's `dataPointForwarderPeer (readFromStore
/// dpStore)` server-peer interpretation:
///
/// ```text
/// loop:
///   1. wait_for_request()              -- Await acceptor's next message.
///   2. match event:
///        Request(names) ->
///          values = read_from_store(store, &names);
///          send_reply(values);          -- back to StIdle, loop.
///        Done -> return Ok(())
/// ```
async fn run_forwarder_loop(
    handle: ProtocolHandle,
    dp_store: DataPointStore,
) -> Result<(), ForwardDataPointsError> {
    let mut forwarder = DataPointForwarder::new(handle);
    loop {
        match forwarder.wait_for_request().await? {
            DataPointForwarderEvent::Request(names) => {
                let values = read_from_store(&dp_store, &names).await;
                forwarder.send_reply(values).await?;
            }
            DataPointForwarderEvent::Done => {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::mux::{MessageChannel, MiniProtocolDir, MiniProtocolNum, MuxHandle, start_unix};
    use crate::protocols::{
        DataPointForwardMessage, DataPointForwardState, DataPointName, DataPointValue,
        init_data_point_store, write_to_store,
    };
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
    async fn forward_data_points_resp_handles_request_done_round_trip() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let store = init_data_point_store();
        // Populate the store with two data-points.
        write_to_store(
            &store,
            DataPointName::new("node-info"),
            DataPointValue::new(b"{\"version\":\"11.0.1\"}".to_vec()),
        )
        .await;
        write_to_store(
            &store,
            DataPointName::new("tip"),
            DataPointValue::new(b"42".to_vec()),
        )
        .await;
        let config = DataPointForwarderConfiguration::new();

        // Forwarder runs in a task; acceptor drives the protocol.
        let forwarder_task =
            tokio::spawn(async move { forward_data_points_resp(config, f_handle, store).await });

        let acceptor = MessageChannel::new(a_handle);

        // Request 1: ask for node-info + tip + unknown name.
        let req = DataPointForwardMessage::MsgDataPointsRequest(vec![
            DataPointName::new("node-info"),
            DataPointName::new("tip"),
            DataPointName::new("unknown"),
        ]);
        acceptor.send(req.to_cbor()).await.expect("acceptor send");

        let mut acceptor = acceptor;
        let raw = acceptor.recv().await.expect("acceptor recv");
        let reply =
            DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StBusy, &raw)
                .expect("decode reply");
        match reply {
            DataPointForwardMessage::MsgDataPointsReply(values) => {
                assert_eq!(values.len(), 3);
                // Three lookups in request-order.
                assert_eq!(values[0].0, DataPointName::new("node-info"));
                assert_eq!(
                    values[0].1.as_ref().expect("Just").as_slice(),
                    b"{\"version\":\"11.0.1\"}"
                );
                assert_eq!(values[1].0, DataPointName::new("tip"));
                assert_eq!(values[1].1.as_ref().expect("Just").as_slice(), b"42");
                assert_eq!(values[2].0, DataPointName::new("unknown"));
                assert!(values[2].1.is_none(), "unknown name → None");
            }
            other => panic!("expected reply, got {other:?}"),
        }

        // Terminate the protocol.
        acceptor
            .send(DataPointForwardMessage::MsgDone.to_cbor())
            .await
            .expect("acceptor done");

        forwarder_task
            .await
            .expect("forwarder task")
            .expect("forwarder result");
    }

    #[tokio::test]
    async fn forward_data_points_init_routes_to_same_loop_as_resp() {
        // Operationally identical — verify the *_init variant runs
        // the same loop body as the *_resp variant.
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let store = init_data_point_store();
        let config = DataPointForwarderConfiguration::new();
        let forwarder_task =
            tokio::spawn(async move { forward_data_points_init(config, f_handle, store).await });

        let acceptor = MessageChannel::new(a_handle);
        acceptor
            .send(DataPointForwardMessage::MsgDone.to_cbor())
            .await
            .expect("acceptor done");

        forwarder_task
            .await
            .expect("forwarder task")
            .expect("forwarder result");
    }

    #[tokio::test]
    async fn forward_data_points_resp_serves_multiple_request_rounds() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let store = init_data_point_store();
        write_to_store(
            &store,
            DataPointName::new("counter"),
            DataPointValue::new(b"0".to_vec()),
        )
        .await;
        let config = DataPointForwarderConfiguration::new();
        let forwarder_task =
            tokio::spawn(async move { forward_data_points_resp(config, f_handle, store).await });

        let acceptor = MessageChannel::new(a_handle);
        let mut acceptor = acceptor;
        for round in 0..3u32 {
            let req =
                DataPointForwardMessage::MsgDataPointsRequest(vec![DataPointName::new("counter")]);
            acceptor.send(req.to_cbor()).await.expect("acceptor send");
            let raw = acceptor.recv().await.expect("acceptor recv");
            let reply =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StBusy, &raw)
                    .expect("decode");
            match reply {
                DataPointForwardMessage::MsgDataPointsReply(values) => {
                    assert_eq!(values.len(), 1, "round {round}: 1 value expected");
                    assert_eq!(values[0].1.as_ref().expect("Just").as_slice(), b"0");
                }
                other => panic!("expected reply at round {round}, got {other:?}"),
            }
        }
        acceptor
            .send(DataPointForwardMessage::MsgDone.to_cbor())
            .await
            .expect("acceptor done");
        forwarder_task
            .await
            .expect("forwarder task")
            .expect("forwarder result");
    }

    #[tokio::test]
    async fn forward_data_points_resp_propagates_transport_error() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        // Drop the acceptor side immediately — the forwarder should
        // see ConnectionClosed when it tries wait_for_request.
        drop(a_handle);
        let store = init_data_point_store();
        let config = DataPointForwarderConfiguration::new();
        let result = forward_data_points_resp(config, f_handle, store).await;
        assert!(matches!(result, Err(ForwardDataPointsError::Forwarder(_))));
    }
}
