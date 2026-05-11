//! Trace-forwarder DataPoint acceptor runtime aggregator.
//!
//! Wires the trace-forwarder DataPoint configuration + codec +
//! acceptor driver + external `DataPointRequestor` shared state into
//! a single async function spawnable by the trace-forwarder
//! mini-protocol layer. Implements the upstream `acceptorActions`
//! recursive loop (initial empty request → wait-for-ask → request →
//! deliver reply → check brake → repeat) plus a graceful-shutdown
//! `MsgDone` exchange bounded by [`SHUTDOWN_TIMEOUT`].
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Run/DataPoint/Acceptor.hs.
//!
//! Filename flattens the upstream directory; mirror of upstream's
//! `Trace.Forward.Run.DataPoint.Acceptor` module which exposes
//! `acceptDataPointsInit` (initiator-mode) +
//! `acceptDataPointsResp` (responder-mode) entry points and the
//! supporting `runPeerWithRequestor` / `acceptorActions` helpers.
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `acceptDataPointsResp config mkDPRequestor peerErrorHandler` | [`accept_data_points_resp`]            |
//! | `acceptDataPointsInit config mkDPRequestor peerErrorHandler` | [`accept_data_points_init`]            |
//! | `runPeerWithRequestor config mkDPRequestor peerErrorHandler` | (collapsed — both `*_init` / `*_resp` call into [`run_acceptor_loop`] directly) |
//! | `acceptorActions config dpRequestor [DataPointName]` (recursive) | [`run_acceptor_loop`] (loop body)   |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`InitiatorProtocolOnly` / `ResponderProtocolOnly`
//!   `RunMiniProtocol` shapes**: upstream's `RunMiniProtocol` GADT
//!   distinguishes the two roles at the type level. Yggdrasil's mux
//!   layer doesn't carry that role distinction in the function
//!   signature — both `*_init` and `*_resp` take a `ProtocolHandle`
//!   and run the same loop. Same precedent as R421's
//!   [`crate::trace_object_run_acceptor`].
//! - **`Network.Mux.MiniProtocolCb`**: upstream wraps the loop in a
//!   callback object that the mux layer invokes on accept. Yggdrasil
//!   exposes the loop as a plain `async fn`; callers spawn it via
//!   `tokio::task::spawn` after acquiring the protocol handle from
//!   the mux.
//! - **`Ouroboros.Network.Driver.Simple.runPeer` typed-protocol
//!   driver loop**: collapses since R454's [`DataPointAcceptor`]
//!   already exposes the per-state driver methods directly.

use std::time::Duration;

use crate::data_point_acceptor::{DataPointAcceptor, DataPointAcceptorError};
use crate::mux::ProtocolHandle;
use crate::protocols::{DataPointAcceptorConfiguration, DataPointRequestor};

/// Total budget for the graceful-shutdown handshake once the brake
/// flag is raised. Matches the corresponding R421 constant in
/// [`crate::trace_object_run_acceptor`]; the two sister-protocol
/// aggregators share the same operator-visible 15-second budget.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(15_000);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the trace-forwarder DataPoint acceptor runtime.
#[derive(Debug, thiserror::Error)]
pub enum AcceptDataPointsError {
    /// Request/reply round-trip failed.
    #[error("acceptor protocol error: {0}")]
    Acceptor(#[from] DataPointAcceptorError),

    /// Graceful shutdown didn't complete within [`SHUTDOWN_TIMEOUT`].
    #[error("graceful shutdown timed out after {timeout:?}")]
    Timeout {
        /// The configured shutdown budget.
        timeout: Duration,
    },
}

// ---------------------------------------------------------------------------
// Runtime entry points
// ---------------------------------------------------------------------------

/// Run the acceptor side (responder mode) of the trace-forwarder
/// DataPoint mini-protocol over the supplied protocol handle, using
/// the requestor returned by `mk_dp_requestor` for external-context
/// coordination.
///
/// Mirror of upstream's `acceptDataPointsResp config mkDPRequestor
/// peerErrorHandler` entry point. Returns `Ok(())` when the brake
/// flag is raised and the graceful `MsgDone` exchange completes
/// within [`SHUTDOWN_TIMEOUT`]; returns [`AcceptDataPointsError::Timeout`]
/// if the shutdown handshake runs over budget.
///
/// The `mk_dp_requestor` closure runs once at the start of the
/// protocol — mirror of upstream's `dpRequestor <- mkDPRequestor ctx`.
/// The closure is given the chance to register the requestor handle
/// with whatever external context will be using it (e.g. a
/// node-info dispatcher); the caller commonly hands back a clone of
/// a long-lived shared requestor.
///
/// The `peer_error_handler` closure is invoked exactly once on any
/// transport-level error — `Mux` / `ConnectionClosed` / decode /
/// state-violation. Mirror of upstream's `peerErrorHandler ctx`
/// finalizer wrapped via `finally`.
pub async fn accept_data_points_resp<MkReq, PeerErrorHandler>(
    config: DataPointAcceptorConfiguration,
    handle: ProtocolHandle,
    mk_dp_requestor: MkReq,
    peer_error_handler: PeerErrorHandler,
) -> Result<(), AcceptDataPointsError>
where
    MkReq: FnOnce() -> DataPointRequestor + Send,
    PeerErrorHandler: FnOnce(&DataPointAcceptorError) + Send,
{
    run_acceptor_loop(config, handle, mk_dp_requestor, peer_error_handler).await
}

/// Run the acceptor side (initiator mode) of the trace-forwarder
/// DataPoint mini-protocol. Mirror of upstream's
/// `acceptDataPointsInit`.
///
/// Operationally identical to [`accept_data_points_resp`] —
/// upstream distinguishes the two via the
/// `InitiatorProtocolOnly` / `ResponderProtocolOnly` `RunMiniProtocol`
/// GADT branches, but Yggdrasil's mux layer doesn't carry that
/// role distinction in the function signature. Provided for API
/// parity with upstream's two-entry-point shape; both paths route
/// through [`run_acceptor_loop`].
pub async fn accept_data_points_init<MkReq, PeerErrorHandler>(
    config: DataPointAcceptorConfiguration,
    handle: ProtocolHandle,
    mk_dp_requestor: MkReq,
    peer_error_handler: PeerErrorHandler,
) -> Result<(), AcceptDataPointsError>
where
    MkReq: FnOnce() -> DataPointRequestor + Send,
    PeerErrorHandler: FnOnce(&DataPointAcceptorError) + Send,
{
    run_acceptor_loop(config, handle, mk_dp_requestor, peer_error_handler).await
}

// ---------------------------------------------------------------------------
// Internal driver loop
// ---------------------------------------------------------------------------

/// Run the recursive acceptor-actions loop until the brake flag is
/// raised + a graceful `MsgDone` exchange completes (or the
/// shutdown budget expires).
///
/// Mirror of upstream's `acceptorActions config dpRequestor []`
/// recursive function. The initial empty-name request matches
/// upstream's `acceptorActions config dpRequestor []` call site
/// in `runPeerWithRequestor` — establishes the channel before any
/// external context has asked for data-points.
async fn run_acceptor_loop<MkReq, PeerErrorHandler>(
    config: DataPointAcceptorConfiguration,
    handle: ProtocolHandle,
    mk_dp_requestor: MkReq,
    peer_error_handler: PeerErrorHandler,
) -> Result<(), AcceptDataPointsError>
where
    MkReq: FnOnce() -> DataPointRequestor + Send,
    PeerErrorHandler: FnOnce(&DataPointAcceptorError) + Send,
{
    let mut acceptor = DataPointAcceptor::new(handle);
    let dp_requestor = mk_dp_requestor();

    let result = run_until_stopped(&config, &mut acceptor, &dp_requestor).await;

    match result {
        Ok(()) => {
            // Brake raised; perform graceful MsgDone within the
            // SHUTDOWN_TIMEOUT budget.
            tokio::time::timeout(SHUTDOWN_TIMEOUT, acceptor.done())
                .await
                .map_err(|_| AcceptDataPointsError::Timeout {
                    timeout: SHUTDOWN_TIMEOUT,
                })?
                .map_err(|e| {
                    peer_error_handler(&e);
                    AcceptDataPointsError::Acceptor(e)
                })?;
            Ok(())
        }
        Err(e) => {
            peer_error_handler(&e);
            Err(AcceptDataPointsError::Acceptor(e))
        }
    }
}

/// Inner loop body. Mirror of upstream's recursive
/// `acceptorActions`:
///   request → deliver reply (skip if empty) → check brake →
///   wait-for-ask → repeat.
///
/// Returns `Ok(())` when the brake fires; returns `Err(...)` on a
/// transport-level failure mid-request.
///
/// Synthesis carve-out: upstream's `acceptorActions` blocks on
/// `readTVar askDataPoints >>= check` between rounds. Brake-tripping
/// during that wait would hang the loop in upstream too — the
/// upstream design relies on external context setting both
/// `shouldWeStop` AND raising `askDataPoints` (or on the mux
/// shutting the channel) to wake the loop. Yggdrasil makes this
/// more robust by racing the wait-for-ask against a brake poll via
/// `tokio::select!`; the brake-poll cadence (50ms) matches R421's
/// [`crate::trace_object_run_acceptor::SHUTDOWN_TIMEOUT`] precedent.
async fn run_until_stopped(
    config: &DataPointAcceptorConfiguration,
    acceptor: &mut DataPointAcceptor,
    dp_requestor: &DataPointRequestor,
) -> Result<(), DataPointAcceptorError> {
    // Initial dp_names = []. Mirror of upstream's
    // `acceptorActions config dpRequestor []` call site.
    let mut dp_names = Vec::new();

    loop {
        // 1. Send request with current dp_names + await reply.
        let reply = acceptor.request(dp_names).await?;

        // 2. Deliver reply to external context. `put_reply`
        //    internally skips empty lists, matching upstream's
        //    `unless (null replyWithDataPoints)` guard.
        dp_requestor.put_reply(reply).await;

        // 3. Check brake. Mirror of `ifM (readTVarIO shouldWeStop) ...`.
        if config.is_stopped().await {
            return Ok(());
        }

        // 4. Race wait_for_ask against the brake. If external
        //    context asks, take the names + loop. If the brake is
        //    tripped while we wait, exit cleanly.
        //
        //    Upstream mirror: `atomically $ readTVar askDataPoints
        //    >>= check; dpNames' <- readTVarIO dataPointsNames`
        //    with the addition of brake-aware wake-up.
        tokio::select! {
            new_names = dp_requestor.wait_for_ask() => {
                dp_names = new_names;
            }
            () = wait_for_stop(&config.should_we_stop) => {
                return Ok(());
            }
        }
    }
}

/// Polls the brake flag every 50ms until it becomes `true`. Mirror
/// of R421's `wait_for_stop` (in [`crate::trace_object_run_acceptor`])
/// — the 50ms granularity is operationally fast enough for graceful
/// shutdown; tighter polling buys nothing.
async fn wait_for_stop(stop_flag: &std::sync::Arc<tokio::sync::RwLock<bool>>) {
    loop {
        if *stop_flag.read().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::mux::{MessageChannel, MiniProtocolDir, MiniProtocolNum, MuxHandle, start_unix};
    use crate::protocols::{
        DataPointForwardMessage, DataPointForwardState, DataPointName, DataPointValue,
    };
    use std::sync::Arc;
    use tokio::net::UnixStream;

    /// Trace-forwarder uses its own sub-protocol number-space. Per
    /// upstream's `Cardano.Tracer.Acceptors.Server`, the DataPoints
    /// sub-protocol gets number 3.
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

    #[test]
    fn shutdown_timeout_matches_15_seconds() {
        // Lock down the operator-visible shutdown grace.
        assert_eq!(SHUTDOWN_TIMEOUT, Duration::from_millis(15_000));
    }

    #[tokio::test]
    async fn initial_empty_request_then_brake_terminates_cleanly() {
        // The simplest acceptor lifecycle: initial dummy
        // MsgDataPointsRequest([]) → empty reply → brake →
        // MsgDone. No external ask happens.
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let config = DataPointAcceptorConfiguration::new();
        let stop_flag_clone = config.should_we_stop.clone();
        let requestor = DataPointRequestor::new();
        let requestor_clone = requestor.clone();

        let forwarder_task = tokio::spawn(async move {
            let mut forwarder = MessageChannel::new(f_handle);
            // First message: initial empty request.
            let raw1 = forwarder.recv().await.expect("recv 1");
            let msg1 =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw1)
                    .expect("decode 1");
            assert_eq!(msg1, DataPointForwardMessage::MsgDataPointsRequest(vec![]));
            // Reply with empty values.
            let reply = DataPointForwardMessage::MsgDataPointsReply(vec![]);
            forwarder
                .send(reply.to_cbor())
                .await
                .expect("send empty reply");

            // Second message: MsgDone after brake.
            let raw2 = forwarder.recv().await.expect("recv 2");
            let msg2 =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw2)
                    .expect("decode 2");
            assert_eq!(msg2, DataPointForwardMessage::MsgDone);
        });

        // Engage the brake on a delayed task so the acceptor sees
        // the initial reply first, then trips the brake.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            *stop_flag_clone.write().await = true;
        });

        let mut peer_err_count = 0u32;
        let peer_error_handler = |_e: &DataPointAcceptorError| {
            peer_err_count += 1;
        };

        let result = accept_data_points_resp(
            config,
            a_handle,
            move || requestor_clone,
            peer_error_handler,
        )
        .await;

        forwarder_task.await.expect("forwarder task");
        assert!(result.is_ok(), "expected clean exit, got {result:?}");
        // Sanity: the requestor wasn't used externally; ask flag
        // should still be unset.
        assert!(!requestor.debug_ask_flag().await);
    }

    #[tokio::test]
    async fn full_request_reply_round_trip_via_requestor() {
        // External context asks for data-points; the acceptor loop
        // picks it up, sends MsgDataPointsRequest, receives reply,
        // delivers it to the requestor (waking external). Then
        // brake.
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let config = DataPointAcceptorConfiguration::new();
        let stop_flag_clone = config.should_we_stop.clone();
        let requestor = DataPointRequestor::new();
        let requestor_for_acceptor = requestor.clone();
        let requestor_for_external = requestor.clone();

        let forwarder_task = tokio::spawn(async move {
            let mut forwarder = MessageChannel::new(f_handle);
            // 1. Initial empty request.
            let raw1 = forwarder.recv().await.expect("recv 1");
            let msg1 =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw1)
                    .expect("decode 1");
            assert_eq!(msg1, DataPointForwardMessage::MsgDataPointsRequest(vec![]));
            forwarder
                .send(DataPointForwardMessage::MsgDataPointsReply(vec![]).to_cbor())
                .await
                .expect("send empty reply");

            // 2. Real request triggered by external ask.
            let raw2 = forwarder.recv().await.expect("recv 2");
            let msg2 =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw2)
                    .expect("decode 2");
            assert_eq!(
                msg2,
                DataPointForwardMessage::MsgDataPointsRequest(vec![DataPointName::new(
                    "node-info"
                )])
            );
            forwarder
                .send(
                    DataPointForwardMessage::MsgDataPointsReply(vec![(
                        DataPointName::new("node-info"),
                        Some(DataPointValue::new(b"{}".to_vec())),
                    )])
                    .to_cbor(),
                )
                .await
                .expect("send real reply");

            // 3. MsgDone after brake.
            let raw3 = forwarder.recv().await.expect("recv 3");
            let msg3 =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw3)
                    .expect("decode 3");
            assert_eq!(msg3, DataPointForwardMessage::MsgDone);
        });

        // Acceptor loop runs in a task.
        let acceptor_task = tokio::spawn(async move {
            accept_data_points_resp(
                config,
                a_handle,
                move || requestor_for_acceptor,
                |_e: &DataPointAcceptorError| {},
            )
            .await
        });

        // Drive the external side: wait a tick (for initial empty
        // round-trip), then ask, then trip the brake. The loop's
        // brake-aware wait_for_ask wakes on the brake without
        // needing a dummy ask.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let reply = requestor_for_external
            .ask_for_data_points(vec![DataPointName::new("node-info")])
            .await;
        assert_eq!(reply.len(), 1);
        assert_eq!(reply[0].0, DataPointName::new("node-info"));
        assert_eq!(
            reply[0].1.as_ref().expect("Just"),
            &DataPointValue::new(b"{}".to_vec())
        );
        // Trip the brake — the brake-aware wait_for_ask in the loop
        // wakes within ~50ms and proceeds to MsgDone.
        *stop_flag_clone.write().await = true;

        let acceptor_result = tokio::time::timeout(Duration::from_secs(5), acceptor_task)
            .await
            .expect("acceptor timed out")
            .expect("acceptor panicked");
        forwarder_task.await.expect("forwarder task");

        match acceptor_result {
            Ok(()) => {}
            other => panic!("expected clean exit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn peer_error_handler_invoked_on_disconnect() {
        // If the forwarder drops the connection mid-protocol, the
        // peer error handler must be called.
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let config = DataPointAcceptorConfiguration::new();
        let requestor = DataPointRequestor::new();
        let requestor_for_acceptor = requestor.clone();
        let peer_err_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let peer_err_count_clone = Arc::clone(&peer_err_count);

        // Drop the forwarder side immediately.
        drop(f_handle);

        let result = accept_data_points_resp(
            config,
            a_handle,
            move || requestor_for_acceptor,
            move |_e: &DataPointAcceptorError| {
                peer_err_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
        )
        .await;

        assert!(matches!(result, Err(AcceptDataPointsError::Acceptor(_))));
        assert_eq!(peer_err_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn init_entry_point_routes_to_same_loop_as_resp() {
        // Operationally identical — verify the *_init variant runs
        // the same loop body as the *_resp variant.
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let config = DataPointAcceptorConfiguration::new();
        let stop_flag_clone = config.should_we_stop.clone();
        let requestor = DataPointRequestor::new();
        let requestor_for_acceptor = requestor.clone();

        let forwarder_task = tokio::spawn(async move {
            let mut forwarder = MessageChannel::new(f_handle);
            let raw1 = forwarder.recv().await.expect("recv 1");
            let _msg1 =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw1)
                    .expect("decode 1");
            forwarder
                .send(DataPointForwardMessage::MsgDataPointsReply(vec![]).to_cbor())
                .await
                .expect("send reply");
            let raw2 = forwarder.recv().await.expect("recv 2");
            let msg2 =
                DataPointForwardMessage::from_cbor_in_state(DataPointForwardState::StIdle, &raw2)
                    .expect("decode 2");
            assert_eq!(msg2, DataPointForwardMessage::MsgDone);
        });

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            *stop_flag_clone.write().await = true;
        });

        let result = accept_data_points_init(
            config,
            a_handle,
            move || requestor_for_acceptor,
            |_e: &DataPointAcceptorError| {},
        )
        .await;

        forwarder_task.await.expect("forwarder task");
        assert!(result.is_ok(), "expected clean exit, got {result:?}");
    }
}
