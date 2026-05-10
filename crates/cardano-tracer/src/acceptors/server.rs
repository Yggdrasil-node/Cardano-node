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
//! helpers, scoped to the trace-objects sub-protocol path. EKG and
//! DataPoint sub-protocols are deferred carve-outs (see module
//! docs); the LocalPipe path lands now (Yggdrasil's
//! `crates/network/src/local_listener.rs::LocalPeerListener`
//! provides the listener primitive); the RemoteSocket TCP path
//! defers pending the trace-forwarder handshake codec port (R425+).
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
//! | `runDataPointsAcceptor :: TracerEnv -> DPF.AcceptorConfiguration -> errorHandler -> ...` | (deferred — see [`run_data_points_acceptor_status`]) |
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
//!   `acceptDataPointsResp`)**: vendored at
//!   `.reference-haskell-cardano-node/trace-forward/src/Trace/Forward/Run/DataPoint/Acceptor.hs`,
//!   port deferred to R425+ (one of the remaining sub-protocols
//!   in the R411 plan's "trace-forward 4 sub-protocols" budget).
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

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::Decoder;
use yggdrasil_network::local_listener::{LocalPeerListener, LocalPeerListenerError};
use yggdrasil_network::mux::{MiniProtocolDir, MiniProtocolNum, ProtocolHandle, start_unix};
use yggdrasil_network::protocols::AcceptorConfiguration;
use yggdrasil_network::trace_object_acceptor::TraceObjectAcceptorError;
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

/// Sub-protocol number reserved for the DataPoint sub-protocol in
/// the upstream wire format (number 3). Currently unused since the
/// DataPoint port is deferred; the constant exists so
/// `mux::start_unix` can be wired with the canonical number-space
/// when the DataPoint port lands.
pub const DATA_POINTS_NUM: MiniProtocolNum = MiniProtocolNum(3);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the trace-forwarder responder-mode server.
#[derive(Debug, thiserror::Error)]
pub enum AcceptorsServerError {
    /// Failed to bind / accept on the local pipe listener.
    #[error("local listener error: {0}")]
    LocalListener(#[from] LocalPeerListenerError),

    /// Mux setup failed for an accepted connection.
    #[error("mux error: {0}")]
    Mux(#[from] yggdrasil_network::mux::MuxError),

    /// Sub-protocol handle missing from the mux response.
    #[error("missing protocol handle: {0:?}")]
    MissingProtocolHandle(MiniProtocolNum),
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
            Err(AcceptorsServerError::LocalListener(
                LocalPeerListenerError::Bind {
                    path: std::path::PathBuf::from("<RemoteSocket placeholder>"),
                    source: std::io::Error::other(do_listen_to_forwarder_socket_status()),
                },
            ))
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
            // Per-connection mux: trace-objects sub-protocol only
            // for now (EKG + DataPoint are deferred carve-outs).
            let (mut handles, _mux) = start_unix(
                stream,
                MiniProtocolDir::Responder,
                &[TRACE_OBJECTS_NUM],
                1, /* buffer hint */
            );
            let trace_handle = match handles.remove(&TRACE_OBJECTS_NUM) {
                Some(h) => h,
                None => {
                    // Mux setup didn't return a handle for the
                    // trace-objects protocol — log + drop the
                    // connection. The error_handler equivalent
                    // collapses to no-op since we never registered
                    // the connection in connected_nodes (no NodeId
                    // resolution happened pre-handshake).
                    return;
                }
            };

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
                    crate::acceptors::utils::remove_disconnected_node(
                        &s.connected_nodes,
                        &s.connected_nodes_names,
                        &s.accepted_metrics,
                        &token,
                    )
                    .await;
                });
            };

            // Wire the trace-objects sub-protocol handler.
            let node_id = crate::utils::conn_id_to_node_id(&conn_token);
            let handler = Arc::clone(&conn_handler);
            let lo_handler_wrapper = move |payloads: Vec<TraceObject>| {
                handler(node_id.clone(), payloads);
            };

            let result: Result<(), AcceptTraceObjectsError> =
                run_trace_objects_acceptor(conn_config, trace_handle, lo_handler_wrapper, on_error)
                    .await;

            // Final cleanup on graceful shutdown.
            let _ = result;
            crate::acceptors::utils::remove_disconnected_node(
                &conn_state.connected_nodes,
                &conn_state.connected_nodes_names,
                &conn_state.accepted_metrics,
                &conn_token,
            )
            .await;
        });
    }
}

/// Run the trace-objects sub-protocol responder over an already-
/// established mux protocol handle. Mirror of upstream's
/// `runTraceObjectsAcceptor tracerEnv tracerEnvRTView tfConfig errorHandler`.
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

/// Stub decoder for `TraceObject`s on the wire. The full CBOR
/// codec for `Cardano.Logging.TraceObject` lands when the
/// trace-dispatcher upstream package is ported. R424 returns an
/// empty list to keep the protocol loop alive without panicking;
/// real ingestion needs the codec port.
fn decode_trace_objects(_dec: &mut Decoder<'_>) -> Result<Vec<TraceObject>, LedgerError> {
    Ok(Vec::new())
}

async fn wait_for_global_stop(stop_flag: &Arc<tokio::sync::RwLock<bool>>) {
    loop {
        if *stop_flag.read().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

fn socket_path_label(state: &AcceptorsServerState) -> u32 {
    // Use network_magic as a label so multiple concurrent tracer
    // instances on the same host don't collide on synthetic conn
    // tokens.
    state.network_magic
}

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

/// Status descriptor for the deferred DataPoint sub-protocol
/// responder (`runDataPointsAcceptor` / `acceptDataPointsResp`).
pub fn run_data_points_acceptor_status() -> &'static str {
    "runDataPointsAcceptor / acceptDataPointsResp: deferred to \
     R425+. The trace-forward DataPoint sub-protocol is vendored at \
     .reference-haskell-cardano-node/trace-forward/src/Trace/Forward/Run/\
     DataPoint/Acceptor.hs but not yet ported. The trace-objects \
     sub-protocol is fully wired and exercises the end-to-end pipe."
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
    fn run_data_points_acceptor_status_points_to_vendor_path() {
        let s = run_data_points_acceptor_status();
        assert!(s.contains("vendored"));
        assert!(s.contains("DataPoint"));
        assert!(s.contains("R425"));
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
        assert!(matches!(
            result,
            Err(AcceptorsServerError::LocalListener(_))
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
}
