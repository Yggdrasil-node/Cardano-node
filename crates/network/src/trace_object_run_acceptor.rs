//! Trace-forwarder TraceObject acceptor runtime aggregator.
//!
//! Wires the trace-forwarder configuration + codec + acceptor
//! driver + caller-supplied trace-object handler into a single
//! async function spawnable by the trace-forwarder mini-protocol
//! layer. Implements the upstream `acceptorActions` recursive loop
//! (request → handle → check-stop → repeat) plus the
//! `timeoutWhenStopped` graceful-shutdown semantics.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Run/TraceObject/Acceptor.hs.
//!
//! Filename flattens the upstream directory; mirror of upstream's
//! `Trace.Forward.Run.TraceObject.Acceptor` module which exposes
//! `acceptTraceObjectsInit` (initiator-mode) +
//! `acceptTraceObjectsResp` (responder-mode) entry points and the
//! supporting `runPeerWithHandler` / `acceptorActions` /
//! `timeoutWhenStopped` helpers.
//!
//! Mapping summary:
//!
//! | Upstream                                                  | Yggdrasil                                |
//! |-----------------------------------------------------------|------------------------------------------|
//! | `acceptTraceObjectsResp config loHandler peerErrorHandler`| [`accept_trace_objects_resp`]            |
//! | `acceptTraceObjectsInit config loHandler peerErrorHandler`| [`accept_trace_objects_init`]            |
//! | `runPeerWithHandler config loHandler peerErrorHandler`    | (collapsed — both `*_init` / `*_resp` call into the same internal driver loop directly) |
//! | `acceptorActions config loHandler` (recursive)            | [`run_acceptor_loop`] (synthesis-internal) |
//! | `timeoutWhenStopped stopVar 15_000 action`                | [`timeout_when_stopped`]                 |
//! | `data Timeout = Timeout` exception                        | [`AcceptTimeout`]                        |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`InitiatorProtocolOnly` / `ResponderProtocolOnly` `RunMiniProtocol` shapes**:
//!   upstream's `RunMiniProtocol` GADT distinguishes the two roles
//!   at the type level. Yggdrasil's mux layer doesn't carry that
//!   role distinction in the function signature — both `*_init`
//!   and `*_resp` take a `ProtocolHandle` and run the same loop
//!   (the difference is what the *caller* does: a forwarder thread
//!   spawns the initiator side, a tracer thread spawns the
//!   responder side).
//! - **`Network.Mux.MiniProtocolCb`**: upstream wraps the loop in a
//!   callback object that the mux layer invokes on accept. Yggdrasil
//!   exposes the loop as a plain `async fn`; callers spawn it via
//!   `tokio::task::spawn` after acquiring the protocol handle from
//!   the mux.
//! - **`Ouroboros.Network.Driver.Simple.runPeer` typed-protocol
//!   driver loop**: collapses since R419's `TraceObjectAcceptor`
//!   already exposes the per-state driver methods directly.

use std::time::Duration;

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::Decoder;

use crate::mux::ProtocolHandle;
use crate::protocols::AcceptorConfiguration;
use crate::trace_object_acceptor::{TraceObjectAcceptor, TraceObjectAcceptorError};

/// Total budget for the graceful-shutdown handshake once the brake
/// flag is raised. Matches upstream's hardcoded `15_000` ms in
/// `timeoutWhenStopped`.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(15_000);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the trace-forwarder TraceObject acceptor runtime.
#[derive(Debug, thiserror::Error)]
pub enum AcceptTraceObjectsError {
    /// Request/reply round-trip failed.
    #[error("acceptor protocol error: {0}")]
    Acceptor(#[from] TraceObjectAcceptorError),

    /// Graceful shutdown didn't complete within
    /// [`SHUTDOWN_TIMEOUT`]. Mirror of upstream's `Timeout`
    /// exception thrown by `timeoutWhenStopped`.
    #[error("graceful shutdown timed out after {timeout:?}")]
    Timeout {
        /// The configured shutdown budget.
        timeout: Duration,
    },
}

/// Synthesis-side type-level marker for shutdown-timeout errors.
/// Mirror of upstream's `data Timeout = Timeout deriving Show`
/// exception type. Yggdrasil surfaces this as a value via
/// [`AcceptTraceObjectsError::Timeout`] rather than a separate
/// exception type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcceptTimeout;

// ---------------------------------------------------------------------------
// Runtime entry points
// ---------------------------------------------------------------------------

/// Run the acceptor side (responder mode) of the trace-forwarder
/// TraceObject mini-protocol over the supplied protocol handle,
/// dispatching every received batch of trace objects to the
/// supplied `lo_handler` callback.
///
/// Mirror of upstream's `acceptTraceObjectsResp config loHandler
/// peerErrorHandler` entry point. Returns `Ok(())` when the brake
/// flag is raised and the graceful `MsgDone` exchange completes
/// within [`SHUTDOWN_TIMEOUT`]; returns
/// [`AcceptTraceObjectsError::Timeout`] if the shutdown handshake
/// runs over budget.
///
/// The `peer_error_handler` closure is invoked exactly once on any
/// transport-level error — `Mux` / `ConnectionClosed` / decode /
/// state-violation. Mirror of upstream's `peerErrorHandler ctx`
/// finalizer wrapped via `finally`.
pub async fn accept_trace_objects_resp<TraceObj, LoHandler, PeerErrorHandler, DecodeF>(
    config: AcceptorConfiguration,
    handle: ProtocolHandle,
    decode_reply_list: DecodeF,
    lo_handler: LoHandler,
    peer_error_handler: PeerErrorHandler,
) -> Result<(), AcceptTraceObjectsError>
where
    LoHandler: FnMut(Vec<TraceObj>) + Send,
    PeerErrorHandler: FnOnce(&TraceObjectAcceptorError) + Send,
    DecodeF: FnMut(&mut Decoder<'_>) -> Result<Vec<TraceObj>, LedgerError> + Clone + Send,
{
    run_acceptor_loop(
        config,
        handle,
        decode_reply_list,
        lo_handler,
        peer_error_handler,
    )
    .await
}

/// Run the acceptor side (initiator mode) of the trace-forwarder
/// TraceObject mini-protocol. Mirror of upstream's
/// `acceptTraceObjectsInit`.
///
/// Operationally identical to [`accept_trace_objects_resp`] —
/// upstream distinguishes the two via the
/// `InitiatorProtocolOnly` / `ResponderProtocolOnly` `RunMiniProtocol`
/// GADT branches, but Yggdrasil's mux layer doesn't carry that
/// role distinction in the function signature. Provided for API
/// parity with upstream's two-entry-point shape; both paths route
/// through [`run_acceptor_loop`].
pub async fn accept_trace_objects_init<TraceObj, LoHandler, PeerErrorHandler, DecodeF>(
    config: AcceptorConfiguration,
    handle: ProtocolHandle,
    decode_reply_list: DecodeF,
    lo_handler: LoHandler,
    peer_error_handler: PeerErrorHandler,
) -> Result<(), AcceptTraceObjectsError>
where
    LoHandler: FnMut(Vec<TraceObj>) + Send,
    PeerErrorHandler: FnOnce(&TraceObjectAcceptorError) + Send,
    DecodeF: FnMut(&mut Decoder<'_>) -> Result<Vec<TraceObj>, LedgerError> + Clone + Send,
{
    run_acceptor_loop(
        config,
        handle,
        decode_reply_list,
        lo_handler,
        peer_error_handler,
    )
    .await
}

// ---------------------------------------------------------------------------
// Internal driver loop
// ---------------------------------------------------------------------------

/// Run the recursive acceptor-actions loop until the brake flag is
/// raised + a graceful `MsgDone` exchange completes (or the
/// shutdown budget expires).
///
/// Mirror of upstream's `acceptorActions config loHandler` chained
/// via `runPeer` inside `timeoutWhenStopped`.
async fn run_acceptor_loop<TraceObj, LoHandler, PeerErrorHandler, DecodeF>(
    config: AcceptorConfiguration,
    handle: ProtocolHandle,
    mut decode_reply_list: DecodeF,
    mut lo_handler: LoHandler,
    peer_error_handler: PeerErrorHandler,
) -> Result<(), AcceptTraceObjectsError>
where
    LoHandler: FnMut(Vec<TraceObj>) + Send,
    PeerErrorHandler: FnOnce(&TraceObjectAcceptorError) + Send,
    DecodeF: FnMut(&mut Decoder<'_>) -> Result<Vec<TraceObj>, LedgerError> + Clone + Send,
{
    let mut acceptor: TraceObjectAcceptor<TraceObj> = TraceObjectAcceptor::new(handle);
    let what_to_request = config.what_to_request;

    let result = run_until_stopped(
        &config,
        &mut acceptor,
        &mut decode_reply_list,
        &mut lo_handler,
    )
    .await;

    match result {
        Ok(()) => {
            // Brake raised; perform graceful MsgDone within the
            // SHUTDOWN_TIMEOUT budget. Upstream's
            // `timeoutWhenStopped` runs the entire action under
            // the timeout; Yggdrasil applies it specifically to the
            // shutdown leg since the main loop already bails on
            // brake in run_until_stopped.
            let _ = what_to_request; // suppress unused-on-shutdown-only path
            tokio::time::timeout(SHUTDOWN_TIMEOUT, acceptor.done())
                .await
                .map_err(|_| AcceptTraceObjectsError::Timeout {
                    timeout: SHUTDOWN_TIMEOUT,
                })?
                .map_err(|e| {
                    peer_error_handler(&e);
                    AcceptTraceObjectsError::Acceptor(e)
                })?;
            Ok(())
        }
        Err(e) => {
            peer_error_handler(&e);
            Err(AcceptTraceObjectsError::Acceptor(e))
        }
    }
}

/// Inner loop body: request → handle → check brake → repeat. Returns
/// `Ok(())` when the brake fires; returns `Err(...)` on a transport-
/// level failure mid-request.
async fn run_until_stopped<TraceObj, LoHandler, DecodeF>(
    config: &AcceptorConfiguration,
    acceptor: &mut TraceObjectAcceptor<TraceObj>,
    decode_reply_list: &mut DecodeF,
    lo_handler: &mut LoHandler,
) -> Result<(), TraceObjectAcceptorError>
where
    LoHandler: FnMut(Vec<TraceObj>),
    DecodeF: FnMut(&mut Decoder<'_>) -> Result<Vec<TraceObj>, LedgerError> + Clone,
{
    let what_to_request = config.what_to_request;
    loop {
        // Upstream's `acceptorActions` checks `shouldWeStop` *after*
        // each round-trip (post-loHandler) — the first request is
        // unconditional, even if the brake is already engaged. We
        // match that ordering exactly: blocking-request first, then
        // dispatch, then check brake.
        let payloads = acceptor
            .request_blocking(what_to_request, decode_reply_list.clone())
            .await?;
        lo_handler(payloads);

        if config.is_stopped().await {
            return Ok(());
        }
    }
}

// ---------------------------------------------------------------------------
// Timeout-when-stopped helper
// ---------------------------------------------------------------------------

/// Race a long-running `action` against the brake flag + a fixed
/// timeout. If the brake is raised, wait up to `timeout`; if the
/// action hasn't completed by then, return [`AcceptTimeout`]. If
/// the brake is never raised, the action runs to completion.
///
/// Mirror of upstream's `timeoutWhenStopped stopVar delay action`.
///
/// This helper is exposed for callers that want to wrap their own
/// `acceptor`-style loops in the same shutdown semantics. The
/// built-in [`accept_trace_objects_resp`] / [`accept_trace_objects_init`]
/// already apply this internally; explicit use is optional.
pub async fn timeout_when_stopped<F, T>(
    stop_flag: &std::sync::Arc<tokio::sync::RwLock<bool>>,
    timeout: Duration,
    action: F,
) -> Result<T, AcceptTimeout>
where
    F: std::future::Future<Output = T>,
{
    tokio::pin!(action);

    // Race the action against the brake flag. If the action finishes
    // first, return its value. If the brake fires, fall through to
    // the timeout-bounded wait.
    tokio::select! {
        biased;
        v = &mut action => return Ok(v),
        _ = wait_for_stop(stop_flag) => {}
    }

    // Brake engaged; wait up to `timeout` for the action to finish.
    match tokio::time::timeout(timeout, action).await {
        Ok(v) => Ok(v),
        Err(_) => Err(AcceptTimeout),
    }
}

/// Polls the stop-flag every 50ms until it becomes `true`. Mirror
/// of upstream's `atomically (readTVar stopVar >>= check)` STM
/// retry-on-change pattern. The 50ms granularity is operationally
/// fast enough for graceful shutdown; tighter polling buys nothing.
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
    use crate::mux::{MiniProtocolDir, MiniProtocolNum, MuxHandle, start_unix};
    use crate::protocols::{
        AcceptorConfiguration, BlockingReplyList, NumberOfTraceObjects, StBlockingStyle,
        TraceObjectForwardMessage, TraceObjectForwardState,
    };
    use std::sync::Arc;
    use tokio::net::UnixStream;
    use yggdrasil_ledger::cbor::Encoder;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TestPayload(u32);

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
        let a = a_handles.remove(&TRACE_OBJECTS_NUM).expect("a");
        let f = f_handles.remove(&TRACE_OBJECTS_NUM).expect("f");
        (a, f, a_mux, f_mux)
    }

    #[tokio::test]
    async fn accept_trace_objects_resp_round_trips_batches_then_stops() {
        let (a_handle, f_handle, _a_mux, _f_mux) = protocol_handle_pair();
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(3));
        let stop_flag_clone = config.should_we_stop.clone();

        // Forwarder side: keep replying with the same canned batch
        // until it receives MsgDone. This way the test is robust to
        // tokio scheduling (the brake-set is async; the loop may
        // round-trip multiple batches before the brake takes effect).
        let forwarder_task = tokio::spawn(async move {
            use crate::mux::MessageChannel;
            let mut forwarder = MessageChannel::new(f_handle);
            let canned_payloads = vec![TestPayload(11), TestPayload(22), TestPayload(33)];
            let mut request_count = 0u32;
            loop {
                let raw = forwarder.recv().await.expect("recv");
                let msg = TraceObjectForwardMessage::<TestPayload>::from_cbor_in_state(
                    TraceObjectForwardState::StIdle,
                    &raw,
                    decode_test_payloads,
                )
                .expect("decode");
                match msg {
                    TraceObjectForwardMessage::MsgTraceObjectsRequest {
                        blocking: StBlockingStyle::StBlocking,
                        n_trace_objects: NumberOfTraceObjects(3),
                    } => {
                        request_count += 1;
                        let reply: TraceObjectForwardMessage<TestPayload> =
                            TraceObjectForwardMessage::MsgTraceObjectsReply {
                                reply: BlockingReplyList::blocking(canned_payloads.clone())
                                    .expect("seed"),
                            };
                        forwarder
                            .send(reply.to_cbor(encode_test_payloads))
                            .await
                            .expect("send reply");
                    }
                    TraceObjectForwardMessage::MsgDone => return request_count,
                    other => panic!("unexpected message: {other:?}"),
                }
            }
        });

        // The lo_handler synchronously raises the brake using
        // `try_write` — the brake's tokio RwLock is uncontended in
        // this test, so try_write succeeds immediately. This avoids
        // the need to spawn a task and depend on the scheduler
        // running it before the next loop iteration. Use a
        // std::sync::Mutex for the collected buffer (sync-safe in
        // an async context, unlike tokio's blocking_lock which
        // panics inside the runtime).
        let collected: Arc<std::sync::Mutex<Vec<TestPayload>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let collected_clone = Arc::clone(&collected);
        let lo_handler = move |payloads: Vec<TestPayload>| {
            // Synchronously record + raise the brake.
            collected_clone
                .lock()
                .expect("collected lock")
                .extend(payloads.iter().cloned());
            *stop_flag_clone
                .try_write()
                .expect("brake try_write should succeed in test") = true;
        };
        let mut peer_err_count = 0u32;
        let peer_error_handler = |_e: &TraceObjectAcceptorError| {
            peer_err_count += 1;
        };

        let result = accept_trace_objects_resp::<TestPayload, _, _, _>(
            config,
            a_handle,
            decode_test_payloads,
            lo_handler,
            peer_error_handler,
        )
        .await;

        let request_count = forwarder_task.await.expect("forwarder task");

        assert!(result.is_ok(), "loop should exit cleanly: {result:?}");
        assert_eq!(peer_err_count, 0, "no peer errors expected");
        assert_eq!(
            request_count, 1,
            "exactly one batch round-trip before brake takes effect"
        );
        let buf = collected.lock().expect("collected final lock");
        assert_eq!(
            *buf,
            vec![TestPayload(11), TestPayload(22), TestPayload(33)]
        );
    }

    #[tokio::test]
    async fn timeout_when_stopped_returns_action_value_when_brake_clear() {
        let stop_flag = Arc::new(tokio::sync::RwLock::new(false));
        let action = async { 42i32 };
        let v = timeout_when_stopped(&stop_flag, Duration::from_millis(100), action)
            .await
            .expect("ok");
        assert_eq!(v, 42);
    }

    #[tokio::test]
    async fn timeout_when_stopped_completes_action_after_brake_within_budget() {
        let stop_flag = Arc::new(tokio::sync::RwLock::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);

        // Engage the brake after 75ms; the action takes 150ms; the
        // shutdown budget is 200ms — so the action wins.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(75)).await;
            *stop_flag_clone.write().await = true;
        });

        let action = async {
            tokio::time::sleep(Duration::from_millis(150)).await;
            "completed"
        };
        let result = timeout_when_stopped(&stop_flag, Duration::from_millis(200), action).await;
        assert!(matches!(result, Ok("completed")));
    }

    #[tokio::test]
    async fn timeout_when_stopped_errors_when_action_overruns_budget() {
        let stop_flag = Arc::new(tokio::sync::RwLock::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);

        // Engage the brake immediately; action takes 500ms; budget 100ms.
        tokio::spawn(async move {
            *stop_flag_clone.write().await = true;
        });

        let action = async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            "should-not-complete"
        };
        let result = timeout_when_stopped(&stop_flag, Duration::from_millis(100), action).await;
        assert!(matches!(result, Err(AcceptTimeout)));
    }

    #[test]
    fn shutdown_timeout_matches_upstream_15_seconds() {
        // Lock down the upstream constant so a careless edit can't
        // silently shorten the operator-visible shutdown grace.
        assert_eq!(SHUTDOWN_TIMEOUT, Duration::from_millis(15_000));
    }

    #[test]
    fn accept_timeout_marker_round_trips() {
        let _t1 = AcceptTimeout;
        let _t2 = AcceptTimeout;
        assert_eq!(_t1, _t2);
        let s = format!("{:?}", AcceptTimeout);
        assert!(s.contains("AcceptTimeout"));
    }
}
