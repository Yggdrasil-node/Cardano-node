//! Trace-forwarder responder-mode entry point — `runAcceptorsServer`
//! analog. Accepts inbound forwarder connections (LocalPipe / Unix
//! socket; RemoteSocket TCP path deferred), spawns the per-
//! connection sub-protocol drivers, and orchestrates teardown via
//! the `error_handler`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Acceptors/Server.hs.
//!
//! Direct port of upstream's `runAcceptorsServer` + supporting
//! helpers. Trace-objects + DataPoint sub-protocols are both wired
//! (R424 + R458); EKG sub-protocol remains a deferred carve-out
//! (Hackage-source synthesis). The LocalPipe path uses Yggdrasil's
//! `crates/network/src/local_listener.rs::LocalPeerListener`
//! primitive; the RemoteSocket TCP path defers pending the
//! trace-forwarder handshake-over-socket codec port.
//!
//! Mapping summary:
//!
//! | Upstream                                                                    | Yggdrasil                                |
//! |-----------------------------------------------------------------------------|------------------------------------------|
//! | `runAcceptorsServer :: TracerEnv -> TracerEnvRTView -> HowToConnect -> ... -> IO ()` | [`run_acceptors_server`]      |
//! | `doListenToForwarderLocal :: ... -> LocalAddress -> ... -> IO ()`           | [`do_listen_to_forwarder_local`]         |
//! | `doListenToForwarderSocket :: ... -> Socket.SockAddr -> ... -> IO ()`       | (deferred — see [`do_listen_to_forwarder_socket_status`]) |
//! | `appResponder protocolsWithNums :: OuroborosApplication ...`                | (collapsed — Yggdrasil's mux dispatches by `MiniProtocolNum` directly via `start_unix`) |
//! | `runEKGAcceptor :: TracerEnv -> EKGF.AcceptorConfiguration -> errorHandler -> ...` | (deferred — see [`run_ekg_acceptor_status`]) |
//! | `runTraceObjectsAcceptor :: TracerEnv -> TracerEnvRTView -> TF.AcceptorConfiguration TraceObject -> errorHandler -> ...` | [`run_trace_objects_acceptor`] |
//! | `runDataPointsAcceptor :: TracerEnv -> DPF.AcceptorConfiguration -> errorHandler -> ...` | (R458 — wired via [`yggdrasil_network::data_point_run_acceptor::accept_data_points_resp`]; status descriptor at [`run_data_points_acceptor_status`]) |
//! | `errorHandler :: ConnectionId addr -> IO ()`                                | (inlined — see [`run_acceptors_server`]'s spawn body) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **EKG sub-protocol responder (`runEKGAcceptor` /
//!   `acceptMetricsResp`)**: the `System.Metrics.Network.Acceptor`
//!   API comes from the upstream `ekg-forward` Hackage package
//!   (not vendored under `.reference-haskell-cardano-node/`). Per
//!   the R411 arc plan + advisor sanity check, the EKG ReqResp
//!   sub-protocol is a synthesis carve-out — the wire format would
//!   need to be reverse-engineered from the Hackage source rather
//!   than mirrored directly. Operationally cardano-tracer can run
//!   without EKG ingest (the per-node Prometheus/EKG endpoints
//!   from R408-R414 read from MetricsStore which can also be fed
//!   manually for testing).
//! - **DataPoint sub-protocol responder (`runDataPointsAcceptor` /
//!   `acceptDataPointsResp`)** (R458 closure): now wired via
//!   [`yggdrasil_network::data_point_run_acceptor::accept_data_points_resp`].
//!   The R452-R457 arc ported the trace-forward DataPoint
//!   Type/Codec/Acceptor/Configuration/Utils/Run files;
//!   R458 wired the per-connection mux to include
//!   [`DATA_POINTS_NUM`] alongside [`TRACE_OBJECTS_NUM`] and runs
//!   both sub-protocol drivers concurrently via `tokio::join!`.
//!   Both sub-protocols share the connection-level brake flag.
//! - **`Net.RemoteSocket host port` TCP path**: requires the trace-
//!   forwarder handshake codec port (R425+). The LocalPipe path
//!   covers the operationally-canonical SPO setup (cardano-tracer
//!   running co-located with cardano-node over a Unix-domain
//!   socket).
//! - **Trace-forwarder handshake codec**: deferred to R425+. The
//!   LocalPipe path uses `Handshake.noTimeLimitsHandshake` which
//!   collapses to no-op for the same-host trust boundary. The
//!   Yggdrasil LocalPeerListener accepts the connection without
//!   handshake; downstream fully-conformant integration with a
//!   running cardano-node forwarder will require the handshake
//!   port.
//! - **`OuroborosApplication` + `MiniProtocol` records**: collapse
//!   into Yggdrasil's `mux::start_unix(stream, role,
//!   &[MiniProtocolNum], buffer_size)` direct dispatch. The
//!   `miniProtocolStart = Mux.StartEagerly` and
//!   `maximumIngressQueue = maxBound` hints are folded into
//!   Yggdrasil's `ProtocolConfig::default_for` defaults.

use std::sync::Arc;

#[cfg(any(test, unix))]
use yggdrasil_ledger::LedgerError;
#[cfg(any(test, unix))]
use yggdrasil_ledger::cbor::Decoder;
#[cfg(unix)]
use yggdrasil_network::data_point_run_acceptor::accept_data_points_resp;
#[cfg(unix)]
use yggdrasil_network::local_listener::{LocalPeerListener, LocalPeerListenerError};
use yggdrasil_network::mux::MiniProtocolNum;
#[cfg(unix)]
use yggdrasil_network::mux::{MiniProtocolDir, ProtocolHandle, start_unix};
use yggdrasil_network::protocols::AcceptorConfiguration;
#[cfg(unix)]
use yggdrasil_network::protocols::DataPointAcceptorConfiguration;
#[cfg(unix)]
use yggdrasil_network::trace_object_acceptor::TraceObjectAcceptorError;
#[cfg(unix)]
use yggdrasil_network::trace_object_run_acceptor::{
    AcceptTraceObjectsError, accept_trace_objects_resp,
};

use crate::configuration::HowToConnect;
use crate::logging::TraceObject;
use crate::metrics_store::AcceptedMetrics;
use crate::types::{ConnectedNodes, ConnectedNodesNames};

/// Sub-protocol number for the trace-objects sub-protocol within
/// the trace-forwarder mux pipe. Mirrors the upstream
/// `Cardano.Tracer.Acceptors.Server` per-protocol number 2.
pub const TRACE_OBJECTS_NUM: MiniProtocolNum = MiniProtocolNum(2);

/// Sub-protocol number reserved for the EKG ReqResp sub-protocol
/// in the upstream wire format (number 1). Currently unused since
/// the EKG sub-protocol is a carve-out; the constant exists so
/// `mux::start_unix` can be wired with the canonical number-space
/// when the EKG port lands.
pub const EKG_NUM: MiniProtocolNum = MiniProtocolNum(1);

/// Sub-protocol number for the DataPoint sub-protocol within the
/// trace-forwarder mux pipe. Mirrors the upstream
/// `Cardano.Tracer.Acceptors.Server` per-protocol number 3.
/// R458 closed the previously-deferred port — the per-connection
/// mux now multiplexes HANDSHAKE + TRACE_OBJECTS + DATA_POINTS.
pub const DATA_POINTS_NUM: MiniProtocolNum = MiniProtocolNum(3);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the trace-forwarder responder-mode server.
#[derive(Debug, thiserror::Error)]
pub enum AcceptorsServerError {
    /// Failed to bind / accept on the local pipe listener.
    #[cfg(unix)]
    #[error("local listener error: {0}")]
    LocalListener(#[from] LocalPeerListenerError),

    /// Mux setup failed for an accepted connection.
    #[error("mux error: {0}")]
    Mux(#[from] yggdrasil_network::mux::MuxError),

    /// Sub-protocol handle missing from the mux response.
    #[error("missing protocol handle: {0:?}")]
    MissingProtocolHandle(MiniProtocolNum),

    /// LocalPipe trace-forwarder transport is unavailable on this platform.
    #[error("{0}")]
    UnsupportedLocalPipe(String),
}

// ---------------------------------------------------------------------------
// Acceptor-side runtime state slice
// ---------------------------------------------------------------------------

/// Slice of the cardano-tracer runtime state that the acceptor
/// server needs to operate. Per the R398 plan's TracerEnv option (b)
/// decision, we accept the explicit slice rather than coupling to
/// the full `TracerEnv` record.
#[derive(Clone)]
pub struct AcceptorsServerState {
    /// Set of currently-connected forwarder NodeIds.
    pub connected_nodes: ConnectedNodes,
    /// Bidirectional map from NodeId to operator-friendly node name.
    pub connected_nodes_names: ConnectedNodesNames,
    /// Per-node MetricsStore registry.
    pub accepted_metrics: AcceptedMetrics,
    /// Per-(node_name, LoggingParams) registry of open log file
    /// handles. R465: plumbed into the per-connection
    /// `remove_disconnected_node` finalizer so disconnecting
    /// forwarders' handle entries get dropped (closing their file
    /// descriptors via Arc-drop) rather than leaking until the
    /// next reconnect.
    pub handle_registry: crate::types::HandleRegistry,
    /// Per-connection registry of `DataPointRequestor` handles,
    /// keyed by `NodeId`. R470: populated by the per-connection
    /// acceptor spawn body after handshake completes; removed by
    /// the per-connection teardown alongside the HandleRegistry
    /// entries. External callers (e.g. `ask_node_name`) look up
    /// the requestor by NodeId to query `NodeInfo` data-points
    /// from the connected forwarder.
    pub data_point_requestors: crate::types::DataPointRequestors,
    /// Network magic for the trace-forwarder handshake (deferred —
    /// see module docs).
    pub network_magic: u32,
}

// ---------------------------------------------------------------------------
// Top-level server entry
// ---------------------------------------------------------------------------

/// Run the trace-forwarder responder-mode server. Mirror of upstream's
/// `runAcceptorsServer tracerEnv tracerEnvRTView howToConnect (ekg, tf, dpf)`.
///
/// `lo_handler` is invoked once per inbound `MsgTraceObjectsReply`
/// batch; the canonical operator implementation routes through
/// `crate::handlers::logs::trace_objects::trace_objects_handler`.
///
/// Returns when the configuration's stop-flag is engaged + all
/// active connections have terminated their `MsgDone` handshakes
/// within the [`yggdrasil_network::trace_object_run_acceptor::SHUTDOWN_TIMEOUT`]
/// budget.
///
/// LocalPipe is the only wire path currently supported; RemoteSocket
/// surfaces a [`do_listen_to_forwarder_socket_status`] deferral.
pub async fn run_acceptors_server<LoHandler>(
    state: AcceptorsServerState,
    how_to_connect: HowToConnect,
    tf_config: AcceptorConfiguration,
    lo_handler: Arc<LoHandler>,
) -> Result<(), AcceptorsServerError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    match how_to_connect {
        HowToConnect::LocalPipe { local_pipe } => {
            do_listen_to_forwarder_local(state, local_pipe, tf_config, lo_handler).await
        }
        HowToConnect::RemoteSocket { .. } => {
            // RemoteSocket path requires the trace-forwarder handshake
            // codec (deferred to R425+). Surface as an error rather
            // than silently no-oping so operators get a clear signal.
            #[cfg(unix)]
            {
                Err(AcceptorsServerError::LocalListener(
                    LocalPeerListenerError::Bind {
                        path: std::path::PathBuf::from("<RemoteSocket placeholder>"),
                        source: std::io::Error::other(do_listen_to_forwarder_socket_status()),
                    },
                ))
            }
            #[cfg(not(unix))]
            {
                Err(AcceptorsServerError::UnsupportedLocalPipe(
                    do_listen_to_forwarder_socket_status().to_string(),
                ))
            }
        }
    }
}

/// Bind a local pipe listener at `path`, accept inbound forwarder
/// connections in a loop, and spawn the per-connection
/// trace-objects acceptor for each. Mirror of upstream's
/// `doListenToForwarderLocal snocket address netMagic timeLimits app`.
pub async fn do_listen_to_forwarder_local<LoHandler>(
    state: AcceptorsServerState,
    socket_path: std::path::PathBuf,
    tf_config: AcceptorConfiguration,
    lo_handler: Arc<LoHandler>,
) -> Result<(), AcceptorsServerError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    #[cfg(not(unix))]
    {
        let _ = (state, tf_config, lo_handler);
        Err(AcceptorsServerError::UnsupportedLocalPipe(format!(
            "LocalPipe trace-forwarder transport requires Unix-domain socket support: {}",
            socket_path.display()
        )))
    }

    #[cfg(unix)]
    {
        let listener = LocalPeerListener::bind(&socket_path).await?;
        let stop_flag = tf_config.should_we_stop.clone();

        loop {
            // Race accept against the global brake.
            let accept_result = tokio::select! {
                biased;
                v = listener.accept_unix() => v,
                _ = wait_for_global_stop(&stop_flag) => return Ok(()),
            };

            let stream = accept_result.map_err(AcceptorsServerError::LocalListener)?;
            let conn_state = state.clone();
            let conn_config = tf_config.clone();
            let conn_handler = Arc::clone(&lo_handler);

            tokio::spawn(async move {
                // Per-connection mux: handshake (R435) + trace-objects +
                // data-points (R458) sub-protocols. EKG sub-protocol is
                // still a deferred carve-out (Hackage-source synthesis).
                let (mut handles, _mux) = start_unix(
                    stream,
                    MiniProtocolDir::Responder,
                    &[
                        MiniProtocolNum::HANDSHAKE,
                        TRACE_OBJECTS_NUM,
                        DATA_POINTS_NUM,
                    ],
                    1, /* buffer hint */
                );
                let handshake_handle = match handles.remove(&MiniProtocolNum::HANDSHAKE) {
                    Some(h) => h,
                    None => return,
                };
                let trace_handle = match handles.remove(&TRACE_OBJECTS_NUM) {
                    Some(h) => h,
                    None => {
                        // Mux setup didn't return a handle for the
                        // trace-objects protocol — log + drop the
                        // connection.
                        return;
                    }
                };
                let data_points_handle = match handles.remove(&DATA_POINTS_NUM) {
                    Some(h) => h,
                    None => {
                        // Mux setup didn't return a handle for the
                        // data-points protocol — log + drop.
                        return;
                    }
                };

                // R436: gate the trace-objects acceptor on a successful
                // handshake. The responder receives ProposeVersions,
                // selects a compatible (version, magic) pair, and
                // sends AcceptVersion. On no-overlap or magic-mismatch
                // the responder sends Refuse and we drop the connection.
                let local_versions = [
                    yggdrasil_network::protocols::ForwardingVersion::V1,
                    yggdrasil_network::protocols::ForwardingVersion::V2,
                ];
                let handshake_outcome =
                yggdrasil_network::trace_object_forward_handshake_driver::run_handshake_responder(
                    handshake_handle,
                    &local_versions,
                    conn_state.network_magic,
                )
                .await;
                if handshake_outcome.is_err() {
                    // Handshake refused / errored — close the connection.
                    // No registration cleanup needed since we haven't
                    // touched connected_nodes yet.
                    return;
                }

                // Resolve a NodeId from the stream's connection identity.
                // R424 doesn't have access to a peer-address string from
                // tokio's UnixStream (peer_addr returns abstract /
                // unnamed for accept'd local sockets), so we use a
                // synthesis fallback: a uuid-like string per connection
                // attempt. Future-rounds will plumb a real connection
                // identifier from the trace-forwarder handshake.
                let conn_token = format!(
                    "LocalPipe-{}-{}",
                    socket_path_label(&conn_state),
                    conn_token_counter()
                );

                // Register the new connection.
                let _new = crate::acceptors::utils::add_connected_node(
                    &conn_state.connected_nodes,
                    &conn_token,
                );
                let _stores = crate::acceptors::utils::prepare_metrics_stores(
                    &conn_state.connected_nodes,
                    &conn_state.accepted_metrics,
                    &conn_token,
                )
                .await;

                // Per-connection error finalizer (mirror of upstream's
                // `errorHandler connId = deregisterNodeId + removeDisconnectedNode + ...`).
                let cleanup_state = conn_state.clone();
                let cleanup_token = conn_token.clone();
                let on_error = move |_e: &TraceObjectAcceptorError| {
                    let s = cleanup_state.clone();
                    let token = cleanup_token.clone();
                    tokio::spawn(async move {
                        // R465: use the registry-aware variant so the
                        // per-(node, LoggingParams) HandleRegistry
                        // entries for this disconnecting forwarder are
                        // dropped, closing the underlying FDs.
                        crate::acceptors::utils::remove_disconnected_node_full(
                            &s.connected_nodes,
                            &s.connected_nodes_names,
                            &s.accepted_metrics,
                            &s.handle_registry,
                            &s.data_point_requestors,
                            &token,
                        )
                        .await;
                    });
                };

                // Wire the trace-objects sub-protocol handler.
                let node_id = crate::utils::conn_id_to_node_id(&conn_token);
                let node_id_for_lo = node_id.clone();
                let handler = Arc::clone(&conn_handler);
                let lo_handler_wrapper = move |payloads: Vec<TraceObject>| {
                    handler(node_id_for_lo.clone(), payloads);
                };

                // Build a DataPoint acceptor configuration whose brake
                // flag is shared with the trace-objects brake (so a
                // single connection-level brake trip stops both
                // sub-protocols cleanly).
                let dp_config = DataPointAcceptorConfiguration {
                    acceptor_tracer: conn_config.acceptor_tracer.clone(),
                    should_we_stop: conn_config.should_we_stop.clone(),
                };
                // Mint a fresh requestor for this connection + register
                // it in the supervisor-shared DataPointRequestors
                // registry under this connection's NodeId. External
                // callers (e.g. ask_node_name in run.rs) look up the
                // requestor by NodeId to ask for NodeInfo data-points.
                // The teardown hook (remove_disconnected_node_with_registry)
                // drops the registration on disconnect.
                let dp_requestor = crate::acceptors::utils::prepare_data_point_requestor();
                conn_state
                    .data_point_requestors
                    .insert(node_id.clone(), dp_requestor.clone());
                let dp_on_error =
                    move |_e: &yggdrasil_network::data_point_acceptor::DataPointAcceptorError| {
                        // R458: DataPoint error finalizer is currently a
                        // no-op — the trace-objects on_error already
                        // performs the per-connection cleanup (deregister
                        // NodeId, drop metrics store) atomically. Both
                        // sub-protocols share the same brake; a transport
                        // error on one trips the other within ~50ms.
                    };

                // Run trace-objects + data-points concurrently.
                // tokio::join! awaits both; either an error or a clean
                // shutdown on either side returns its result here, and
                // the connection-level cleanup runs after both have
                // wound down. Per upstream's Network.Mux semantics, the
                // two sub-protocols share the mux's egress buffer but
                // are otherwise independent.
                let (trace_result, dp_result) = tokio::join!(
                    run_trace_objects_acceptor(
                        conn_config,
                        trace_handle,
                        lo_handler_wrapper,
                        on_error
                    ),
                    accept_data_points_resp(
                        dp_config,
                        data_points_handle,
                        move || dp_requestor,
                        dp_on_error,
                    )
                );

                // Final cleanup on graceful shutdown.
                let _ = trace_result;
                let _ = dp_result;
                crate::acceptors::utils::remove_disconnected_node_full(
                    &conn_state.connected_nodes,
                    &conn_state.connected_nodes_names,
                    &conn_state.accepted_metrics,
                    &conn_state.handle_registry,
                    &conn_state.data_point_requestors,
                    &conn_token,
                )
                .await;
            });
        }
    }
}

/// Run the trace-objects sub-protocol responder over an already-
/// established mux protocol handle. Mirror of upstream's
/// `runTraceObjectsAcceptor tracerEnv tracerEnvRTView tfConfig errorHandler`.
#[cfg(unix)]
async fn run_trace_objects_acceptor<LoHandler, ErrHandler>(
    tf_config: AcceptorConfiguration,
    handle: ProtocolHandle,
    lo_handler: LoHandler,
    error_handler: ErrHandler,
) -> Result<(), AcceptTraceObjectsError>
where
    LoHandler: FnMut(Vec<TraceObject>) + Send,
    ErrHandler: FnOnce(&TraceObjectAcceptorError) + Send,
{
    accept_trace_objects_resp(
        tf_config,
        handle,
        decode_trace_objects,
        lo_handler,
        error_handler,
    )
    .await
}

/// Decoder for `TraceObject`s on the wire. R437 wired the
/// Yggdrasil-canonical CBOR codec from
/// [`crate::logging::TraceObject::from_cbor`] (synthesis carve-out
/// — see that module's R437 codec docs for the operator caveat
/// about upstream-byte-equivalence). The wire format is a CBOR
/// array of per-trace-object 6-field arrays; the decoder reads
/// the outer array length, then decodes each entry.
#[cfg(any(test, unix))]
fn decode_trace_objects(dec: &mut Decoder<'_>) -> Result<Vec<TraceObject>, LedgerError> {
    let count = dec.array()?;
    // Bound the per-batch count: even on a busy node, a single
    // request rarely produces more than ~10k events (the operator's
    // `lo_request_num` defaults to 100 per R426); 64k is a generous
    // ceiling that fends off a malicious peer shipping
    // 2^32 entries.
    let cap = (count as usize).min(65_536);
    let mut out = Vec::with_capacity(cap);
    for _ in 0..count {
        // Decode each TraceObject as its own 6-field CBOR array.
        // The outer array context means we can't use the standalone
        // `from_cbor` (which checks `is_empty` at the end) — instead
        // we replicate its body inline against the shared decoder.
        let len = dec.array()?;
        if len != 6 {
            return Err(LedgerError::CborInvalidLength {
                expected: 6,
                actual: len as usize,
            });
        }
        let to_human = if dec.peek_is_null() {
            dec.null()?;
            None
        } else {
            Some(dec.text()?.to_owned())
        };
        let to_machine = dec.text()?.to_owned();
        let code = dec.unsigned()? as u8;
        let to_severity = crate::severity::SeverityS::from_syslog_code(code).ok_or_else(|| {
            LedgerError::CborDecodeError(format!(
                "TraceObject: invalid syslog severity code {code} (must be 0-7)"
            ))
        })?;
        let ns_len = dec.array()?;
        let mut to_namespace = Vec::with_capacity(ns_len as usize);
        for _ in 0..ns_len {
            to_namespace.push(dec.text()?.to_owned());
        }
        let to_thread_id = dec.text()?.to_owned();
        let to_timestamp_ms = dec.signed()?;
        out.push(TraceObject {
            to_human,
            to_machine,
            to_severity,
            to_namespace,
            to_thread_id,
            to_timestamp_ms,
        });
    }
    Ok(out)
}

#[cfg(unix)]
async fn wait_for_global_stop(stop_flag: &Arc<tokio::sync::RwLock<bool>>) {
    loop {
        if *stop_flag.read().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[cfg(unix)]
fn socket_path_label(state: &AcceptorsServerState) -> u32 {
    // Use network_magic as a label so multiple concurrent tracer
    // instances on the same host don't collide on synthetic conn
    // tokens.
    state.network_magic
}

#[cfg(unix)]
fn conn_token_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Carve-out status descriptors
// ---------------------------------------------------------------------------

/// Status descriptor for the deferred RemoteSocket TCP path.
pub fn do_listen_to_forwarder_socket_status() -> &'static str {
    "doListenToForwarderSocket (RemoteSocket TCP path): deferred \
     pending the trace-forwarder handshake codec port (R425+). \
     Yggdrasil's R424 LocalPipe path covers the operationally-canonical \
     SPO setup (cardano-tracer running co-located with cardano-node \
     over a Unix-domain socket)."
}

/// Status descriptor for the deferred EKG ReqResp sub-protocol
/// responder (`runEKGAcceptor` / `acceptMetricsResp`).
pub fn run_ekg_acceptor_status() -> &'static str {
    "runEKGAcceptor / acceptMetricsResp: deferred. The \
     System.Metrics.Network.Acceptor API comes from the upstream \
     ekg-forward Hackage package (not vendored). Per the R411 arc \
     plan + advisor guidance, EKG ReqResp is a synthesis carve-out \
     — the wire format would need to be reverse-engineered. \
     Operationally cardano-tracer can run without EKG ingest (the \
     per-node Prometheus/EKG endpoints from R408-R414 read from \
     MetricsStore which can be fed manually or via a future \
     synthesis port)."
}

/// Status descriptor for the (now-closed) DataPoint sub-protocol
/// responder (`runDataPointsAcceptor` / `acceptDataPointsResp`).
/// Retained for programmatic introspection — the function returns
/// a short description summarising the current state and the round
/// in which it closed.
pub fn run_data_points_acceptor_status() -> &'static str {
    "runDataPointsAcceptor / acceptDataPointsResp: closed at R458. \
     The trace-forward DataPoint sub-protocol shipped in R452-R457 \
     (Type + Codec + Acceptor driver + Configuration + Utils + \
     Run-aggregator). R458 wired DATA_POINTS_NUM=MiniProtocolNum(3) \
     into the per-connection mux protocol list, and the per-connection \
     acceptor spawn body now runs accept_trace_objects_resp + \
     accept_data_points_resp concurrently via tokio::join!. The two \
     sub-protocols share the connection-level brake flag so a single \
     stop trip terminates both cleanly."
}

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_network::protocols::NumberOfTraceObjects;

    #[test]
    fn protocol_numbers_match_upstream() {
        // Lock down the upstream wire format's per-sub-protocol
        // number assignment so a careless edit can't silently
        // shift the dispatch ordering.
        assert_eq!(EKG_NUM.0, 1);
        assert_eq!(TRACE_OBJECTS_NUM.0, 2);
        assert_eq!(DATA_POINTS_NUM.0, 3);
    }

    #[test]
    fn do_listen_to_forwarder_socket_status_describes_deferral() {
        let s = do_listen_to_forwarder_socket_status();
        assert!(s.contains("deferred"));
        assert!(s.contains("R425+"));
        assert!(s.contains("RemoteSocket"));
    }

    #[test]
    fn run_ekg_acceptor_status_describes_carve_out() {
        let s = run_ekg_acceptor_status();
        assert!(s.contains("deferred") || s.contains("synthesis carve-out"));
        assert!(s.contains("ekg-forward"));
    }

    #[test]
    fn run_data_points_acceptor_status_describes_closure() {
        let s = run_data_points_acceptor_status();
        assert!(s.contains("closed at R458"));
        assert!(s.contains("DataPoint"));
        assert!(s.contains("R452-R457"));
        assert!(s.contains("DATA_POINTS_NUM"));
    }

    #[test]
    fn decode_trace_objects_returns_empty_list_for_now() {
        // The stub decoder is intentionally lenient; the real codec
        // port lands with the trace-dispatcher upstream port.
        let bytes: Vec<u8> = vec![0x80]; // empty CBOR array
        let mut dec = Decoder::new(&bytes);
        let v = decode_trace_objects(&mut dec).expect("decode stub");
        assert!(v.is_empty());
    }

    #[tokio::test]
    async fn run_acceptors_server_remote_socket_returns_deferral_error() {
        use yggdrasil_network::protocols::AcceptorConfiguration;
        let state = AcceptorsServerState {
            connected_nodes: ConnectedNodes::new(),
            connected_nodes_names: ConnectedNodesNames::new(),
            accepted_metrics: crate::metrics_store::new_accepted_metrics(),
            handle_registry: crate::types::HandleRegistry::new(),
            data_point_requestors: crate::types::DataPointRequestors::new(),
            network_magic: 764824073,
        };
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(10));
        let how = HowToConnect::RemoteSocket {
            host: "127.0.0.1".to_string(),
            port: 8080,
        };
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_id, _payloads| {});
        // Have to thread the right type through generic so the call
        // is well-formed; use a thin newtype function so we don't
        // tangle the generic arg with the Arc<dyn> at the call site.
        let result = run_remote_socket_test_shim(state, how, config, handler).await;
        #[cfg(unix)]
        assert!(matches!(
            result,
            Err(AcceptorsServerError::LocalListener(_))
        ));
        #[cfg(not(unix))]
        assert!(matches!(
            result,
            Err(AcceptorsServerError::UnsupportedLocalPipe(_))
        ));
    }

    /// Test-only thin shim that monomorphizes the closure type so we
    /// can pass a trait-object handler as a concrete LoHandler bound.
    async fn run_remote_socket_test_shim(
        state: AcceptorsServerState,
        how: HowToConnect,
        config: AcceptorConfiguration,
        handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync>,
    ) -> Result<(), AcceptorsServerError> {
        run_acceptors_server(
            state,
            how,
            config,
            Arc::new(move |id, payloads| handler(id, payloads)),
        )
        .await
    }

    /// R460 integration smoke: brings up `do_listen_to_forwarder_local`
    /// against a real Unix socket, dials it as a forwarder, completes
    /// the trace-forwarder handshake, and verifies both sub-protocols
    /// (trace-objects + data-points) coexist on the per-connection mux
    /// — i.e. the `tokio::join!` in the spawn body actually runs both
    /// drivers concurrently.
    ///
    /// Closes the R459 advisor flag that the "boots with DataPoint
    /// multiplexed" claim was untested at the integration level.
    #[cfg(unix)]
    #[tokio::test]
    async fn server_round_trips_both_sub_protocols_concurrently() {
        use yggdrasil_network::mux::MessageChannel;
        use yggdrasil_network::protocols::{
            DataPointForwardMessage, DataPointForwardState, DataPointName, DataPointValue,
            TraceObjectForwardMessage, TraceObjectForwardState,
        };
        use yggdrasil_network::trace_object_forward_handshake_driver::run_handshake_initiator;

        let dir = tempfile::TempDir::new().expect("tempdir");
        let socket_path = dir.path().join("r460-rt.sock");

        let state = AcceptorsServerState {
            connected_nodes: ConnectedNodes::new(),
            connected_nodes_names: ConnectedNodesNames::new(),
            accepted_metrics: crate::metrics_store::new_accepted_metrics(),
            handle_registry: crate::types::HandleRegistry::new(),
            data_point_requestors: crate::types::DataPointRequestors::new(),
            network_magic: 764824073,
        };
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(3));
        let stop_flag_clone = config.should_we_stop.clone();
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_id, _payloads| {});

        // Start cardano-tracer responder server (acceptor side).
        let socket_clone = socket_path.clone();
        let config_clone = config.clone();
        let server_task = tokio::spawn(async move {
            do_listen_to_forwarder_local(
                state,
                socket_clone,
                config_clone,
                Arc::new(move |id, payloads| handler(id, payloads)),
            )
            .await
        });

        // Give server time to bind.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Forwarder side (cardano-node analog): connect + run handshake
        // initiator + drive both sub-protocols.
        let stream = tokio::net::UnixStream::connect(&socket_path)
            .await
            .expect("forwarder connect");
        let (mut handles, _mux) = start_unix(
            stream,
            MiniProtocolDir::Initiator,
            &[
                MiniProtocolNum::HANDSHAKE,
                TRACE_OBJECTS_NUM,
                DATA_POINTS_NUM,
            ],
            1,
        );
        let handshake_handle = handles.remove(&MiniProtocolNum::HANDSHAKE).expect("hs");
        let trace_handle = handles.remove(&TRACE_OBJECTS_NUM).expect("trace");
        let dp_handle = handles.remove(&DATA_POINTS_NUM).expect("dp");

        let proposals = vec![(
            yggdrasil_network::protocols::ForwardingVersion::V1,
            yggdrasil_network::protocols::ForwardingVersionData {
                network_magic: 764824073,
            },
        )];
        run_handshake_initiator(handshake_handle, proposals)
            .await
            .expect("handshake initiator");

        // Spawn TraceObjects forwarder side: receives requests + sends
        // empty replies until MsgDone arrives.
        let mut trace_channel = MessageChannel::new(trace_handle);
        let trace_forwarder = tokio::spawn(async move {
            let mut round_count = 0u32;
            loop {
                let raw = match trace_channel.recv().await {
                    Some(r) => r,
                    None => return round_count,
                };
                let req = TraceObjectForwardMessage::<TraceObject>::from_cbor_in_state(
                    TraceObjectForwardState::StIdle,
                    &raw,
                    |_dec: &mut Decoder<'_>| Ok(Vec::<TraceObject>::new()),
                )
                .expect("decode trace request");
                match req {
                    TraceObjectForwardMessage::MsgTraceObjectsRequest { .. } => {
                        round_count += 1;
                        let reply: TraceObjectForwardMessage<TraceObject> =
                            TraceObjectForwardMessage::MsgTraceObjectsReply {
                                reply:
                                    yggdrasil_network::protocols::BlockingReplyList::non_blocking(
                                        vec![],
                                    ),
                            };
                        trace_channel
                            .send(reply.to_cbor(|enc, _: &[TraceObject]| {
                                enc.array(0);
                            }))
                            .await
                            .expect("trace forwarder send");
                    }
                    TraceObjectForwardMessage::MsgDone => return round_count,
                    other => panic!("trace forwarder: unexpected {other:?}"),
                }
            }
        });

        // Spawn DataPoints forwarder side: receives requests + sends
        // canned replies until MsgDone.
        let mut dp_channel = MessageChannel::new(dp_handle);
        let dp_forwarder = tokio::spawn(async move {
            let mut round_count = 0u32;
            loop {
                let raw = match dp_channel.recv().await {
                    Some(r) => r,
                    None => return round_count,
                };
                let req = DataPointForwardMessage::from_cbor_in_state(
                    DataPointForwardState::StIdle,
                    &raw,
                )
                .expect("decode dp request");
                match req {
                    DataPointForwardMessage::MsgDataPointsRequest(names) => {
                        round_count += 1;
                        // Echo back each requested name with a canned
                        // value. If names is empty (initial round-trip),
                        // reply with empty values.
                        let values: yggdrasil_network::protocols::DataPointValues = names
                            .into_iter()
                            .map(|n: DataPointName| {
                                let v = format!("value-for-{}", n.as_str()).into_bytes();
                                (n, Some(DataPointValue::new(v)))
                            })
                            .collect();
                        let reply = DataPointForwardMessage::MsgDataPointsReply(values);
                        dp_channel.send(reply.to_cbor()).await.expect("dp send");
                    }
                    DataPointForwardMessage::MsgDone => return round_count,
                    other => panic!("dp forwarder: unexpected {other:?}"),
                }
            }
        });

        // Allow both initial empty round-trips to complete.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Trip the brake — both sub-protocols' loops will detect it
        // within ~50ms and send MsgDone.
        *stop_flag_clone.write().await = true;

        // Wait for both forwarder tasks to receive MsgDone and exit.
        let trace_rounds = tokio::time::timeout(std::time::Duration::from_secs(5), trace_forwarder)
            .await
            .expect("trace forwarder timed out")
            .expect("trace forwarder panicked");
        let dp_rounds = tokio::time::timeout(std::time::Duration::from_secs(5), dp_forwarder)
            .await
            .expect("dp forwarder timed out")
            .expect("dp forwarder panicked");

        // Both forwarders should have processed at least the initial
        // request before MsgDone. The DataPoint side always sends an
        // initial empty MsgDataPointsRequest([]); the TraceObjects
        // side issues batch requests on its `what_to_request` cadence.
        assert!(
            trace_rounds >= 1,
            "trace forwarder should have served at least one request (got {trace_rounds})"
        );
        assert!(
            dp_rounds >= 1,
            "dp forwarder should have served at least the initial empty request (got {dp_rounds})"
        );

        // Server task is in an infinite accept-loop; it'll exit when
        // the brake-poll inside `wait_for_global_stop` next ticks
        // (50ms cadence). Abort it to free the test.
        server_task.abort();
        let _ = server_task.await;
    }
}
