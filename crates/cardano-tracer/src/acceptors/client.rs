//! Trace-forwarder initiator-mode entry point —
//! `runAcceptorsClient` analog. Connects outbound to a forwarder
//! (cardano-node) over a Unix pipe, runs the per-connection sub-
//! protocol drivers, and orchestrates teardown via the
//! `error_handler`.
//!
//! Cardano-tracer can run in two modes — acceptor-as-server
//! (R424's `server.rs`, where cardano-tracer binds the socket and
//! cardano-node connects to it) or acceptor-as-client (this
//! module, where cardano-tracer connects to a socket cardano-node
//! has bound). Both are configured via `acceptAt` / `connectTo` in
//! the operator config file.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Acceptors/Client.hs.
//!
//! Direct port of upstream's `runAcceptorsClient` + supporting
//! helpers, scoped to the trace-objects sub-protocol path. EKG +
//! DataPoint sub-protocols are deferred carve-outs (see
//! [`super::server`] module docs); the LocalPipe path lands now
//! using `tokio::net::UnixStream::connect`; the RemoteSocket TCP
//! path defers pending the trace-forwarder handshake codec port
//! (R426+).
//!
//! Mapping summary:
//!
//! | Upstream                                                                    | Yggdrasil                                |
//! |-----------------------------------------------------------------------------|------------------------------------------|
//! | `runAcceptorsClient :: TracerEnv -> TracerEnvRTView -> HowToConnect -> ... -> IO ()` | [`run_acceptors_client`]      |
//! | `doConnectToForwarderLocal :: ... -> LocalAddress -> ... -> IO ()`          | [`do_connect_to_forwarder_local`]        |
//! | `doConnectToForwarderSocket :: ... -> Socket.SockAddr -> ... -> IO ()`      | (deferred — see [`super::server::do_listen_to_forwarder_socket_status`]) |
//! | `appInitiator protocolsWithNums :: OuroborosApplication ...`                | (collapsed — Yggdrasil's mux dispatches by `MiniProtocolNum` directly via `start_unix`) |
//! | `runEKGAcceptorInit :: TracerEnv -> EKGF.AcceptorConfiguration -> errorHandler -> ...` | (deferred — see [`super::server::run_ekg_acceptor_status`]) |
//! | `runTraceObjectsAcceptorInit :: TracerEnv -> TracerEnvRTView -> TF.AcceptorConfiguration TraceObject -> errorHandler -> ...` | [`run_trace_objects_acceptor_init`] |
//! | `runDataPointsAcceptorInit :: TracerEnv -> DPF.AcceptorConfiguration -> errorHandler -> ...` | (deferred — see [`super::server::run_data_points_acceptor_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - All carve-outs from [`super::server`] apply equivalently —
//!   the initiator side mirrors the responder side's deferral
//!   structure 1:1.
//! - **`appInitiator` vs `appResponder`**: collapses since both
//!   sides route through `mux::start_unix` with their respective
//!   `MiniProtocolDir`. The initiator side passes
//!   `MiniProtocolDir::Initiator` instead of `Responder`.

use std::path::PathBuf;
use std::sync::Arc;

use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::Decoder;
use yggdrasil_network::mux::{MiniProtocolDir, ProtocolHandle, start_unix};
use yggdrasil_network::protocols::AcceptorConfiguration;
use yggdrasil_network::trace_object_acceptor::TraceObjectAcceptorError;
use yggdrasil_network::trace_object_run_acceptor::{
    AcceptTraceObjectsError, accept_trace_objects_init,
};

use super::server::{AcceptorsServerError, AcceptorsServerState, TRACE_OBJECTS_NUM};
use crate::configuration::HowToConnect;
use crate::logging::TraceObject;

// ---------------------------------------------------------------------------
// Top-level client entry
// ---------------------------------------------------------------------------

/// Run the trace-forwarder initiator-mode client. Mirror of
/// upstream's `runAcceptorsClient tracerEnv tracerEnvRTView
/// howToConnect (ekg, tf, dpf)`.
///
/// `lo_handler` is invoked once per inbound `MsgTraceObjectsReply`
/// batch; the canonical operator implementation routes through
/// `crate::handlers::logs::trace_objects::trace_objects_handler`.
///
/// LocalPipe is the only wire path currently supported; RemoteSocket
/// surfaces a deferral error (see
/// [`super::server::do_listen_to_forwarder_socket_status`]).
pub async fn run_acceptors_client<LoHandler>(
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
            do_connect_to_forwarder_local(state, local_pipe, tf_config, lo_handler).await
        }
        HowToConnect::RemoteSocket { .. } => Err(AcceptorsServerError::LocalListener(
            yggdrasil_network::local_listener::LocalPeerListenerError::Bind {
                path: PathBuf::from("<RemoteSocket placeholder>"),
                source: std::io::Error::other(super::server::do_listen_to_forwarder_socket_status()),
            },
        )),
    }
}

/// Connect to a forwarder over a Unix pipe + run the trace-objects
/// sub-protocol initiator. Mirror of upstream's
/// `doConnectToForwarderLocal snocket address netMagic timeLimits app`.
///
/// Once the connection is established, the per-connection mux is
/// initialized as `Initiator` and the trace-objects sub-protocol
/// driver runs via R421's `accept_trace_objects_init`.
pub async fn do_connect_to_forwarder_local<LoHandler>(
    state: AcceptorsServerState,
    socket_path: PathBuf,
    tf_config: AcceptorConfiguration,
    lo_handler: Arc<LoHandler>,
) -> Result<(), AcceptorsServerError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    let stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .map_err(|e| {
            AcceptorsServerError::LocalListener(
                yggdrasil_network::local_listener::LocalPeerListenerError::Bind {
                    path: socket_path.clone(),
                    source: e,
                },
            )
        })?;

    let (mut handles, _mux) = start_unix(
        stream,
        MiniProtocolDir::Initiator,
        &[TRACE_OBJECTS_NUM],
        1, /* buffer hint */
    );
    let trace_handle =
        handles
            .remove(&TRACE_OBJECTS_NUM)
            .ok_or(AcceptorsServerError::MissingProtocolHandle(
                TRACE_OBJECTS_NUM,
            ))?;

    // Build a stable connection token for this client's outbound
    // connection. R425 uses the socket path (since unlike the
    // server side the client knows where it connected to) — this
    // gives a deterministic NodeId per cardano-node it dials, even
    // across reconnects.
    let conn_token = format!(
        "ConnectTo-{}-magic{}",
        socket_path.display(),
        state.network_magic
    );

    // Register the new connection.
    let _new = crate::acceptors::utils::add_connected_node(&state.connected_nodes, &conn_token);
    let _stores = crate::acceptors::utils::prepare_metrics_stores(
        &state.connected_nodes,
        &state.accepted_metrics,
        &conn_token,
    )
    .await;

    let cleanup_state = state.clone();
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

    let node_id = crate::utils::conn_id_to_node_id(&conn_token);
    let handler = Arc::clone(&lo_handler);
    let lo_handler_wrapper = move |payloads: Vec<TraceObject>| {
        handler(node_id.clone(), payloads);
    };

    let _result: Result<(), AcceptTraceObjectsError> =
        run_trace_objects_acceptor_init(tf_config, trace_handle, lo_handler_wrapper, on_error)
            .await;

    // Final cleanup on graceful shutdown.
    crate::acceptors::utils::remove_disconnected_node(
        &state.connected_nodes,
        &state.connected_nodes_names,
        &state.accepted_metrics,
        &conn_token,
    )
    .await;

    Ok(())
}

/// Run the trace-objects sub-protocol initiator over an already-
/// established mux protocol handle. Mirror of upstream's
/// `runTraceObjectsAcceptorInit tracerEnv tracerEnvRTView tfConfig errorHandler`.
async fn run_trace_objects_acceptor_init<LoHandler, ErrHandler>(
    tf_config: AcceptorConfiguration,
    handle: ProtocolHandle,
    lo_handler: LoHandler,
    error_handler: ErrHandler,
) -> Result<(), AcceptTraceObjectsError>
where
    LoHandler: FnMut(Vec<TraceObject>) + Send,
    ErrHandler: FnOnce(&TraceObjectAcceptorError) + Send,
{
    accept_trace_objects_init(
        tf_config,
        handle,
        decode_trace_objects,
        lo_handler,
        error_handler,
    )
    .await
}

/// Stub decoder for `TraceObject`s on the wire — same placeholder
/// as `super::server::decode_trace_objects`. The real codec lands
/// when the trace-dispatcher upstream package is ported.
fn decode_trace_objects(_dec: &mut Decoder<'_>) -> Result<Vec<TraceObject>, LedgerError> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConnectedNodes, ConnectedNodesNames};
    use yggdrasil_network::protocols::NumberOfTraceObjects;

    fn test_state() -> AcceptorsServerState {
        AcceptorsServerState {
            connected_nodes: ConnectedNodes::new(),
            connected_nodes_names: ConnectedNodesNames::new(),
            accepted_metrics: crate::metrics_store::new_accepted_metrics(),
            network_magic: 764824073,
        }
    }

    #[tokio::test]
    async fn run_acceptors_client_remote_socket_returns_deferral_error() {
        let state = test_state();
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(10));
        let how = HowToConnect::RemoteSocket {
            host: "127.0.0.1".to_string(),
            port: 8080,
        };
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_id, _payloads| {});
        let result = run_remote_socket_test_shim(state, how, config, handler).await;
        assert!(matches!(
            result,
            Err(AcceptorsServerError::LocalListener(_))
        ));
    }

    #[tokio::test]
    async fn do_connect_to_forwarder_local_errors_on_missing_socket() {
        let state = test_state();
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(5));
        let nonexistent = PathBuf::from("/tmp/yggdrasil-r425-nonexistent.sock");
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_, _| {});
        let result =
            do_connect_to_forwarder_local_test_shim(state, nonexistent.clone(), config, handler)
                .await;
        match result {
            Err(AcceptorsServerError::LocalListener(
                yggdrasil_network::local_listener::LocalPeerListenerError::Bind { path, .. },
            )) => {
                assert_eq!(path, nonexistent);
            }
            other => panic!("expected Bind error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn do_connect_to_forwarder_local_round_trips_against_local_listener() {
        use yggdrasil_network::local_listener::LocalPeerListener;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let socket_path = dir.path().join("r425-rt.sock");

        // Spawn a server-side listener; the client connects to it
        // and the test ends as soon as the connection is accepted +
        // the per-connection mux is ready.
        let listener = LocalPeerListener::bind(&socket_path).await.expect("bind");
        let server_task = tokio::spawn(async move {
            let _stream = listener.accept_unix().await.expect("accept");
            // Hold the stream open for a moment so the client can
            // complete its mux setup.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        });

        let state = test_state();
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(1));
        // Engage brake immediately — the client should connect,
        // initialize the mux, register the conn, then the loop
        // observes the brake and shuts down.
        config.request_stop().await;
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_, _| {});
        let result =
            do_connect_to_forwarder_local_test_shim(state.clone(), socket_path, config, handler)
                .await;
        // The connect succeeded; the trace-objects loop may have
        // errored out due to the immediately-engaged brake but the
        // overall server-error path should NOT bubble up.
        assert!(result.is_ok(), "client should return Ok: {result:?}");

        // The connection should have been registered and then
        // cleaned up by the final `remove_disconnected_node` call.
        let connected = state.connected_nodes.snapshot();
        assert!(connected.is_empty(), "connection cleaned up after run");

        server_task.await.expect("server task");
    }

    /// Test-only thin shim that monomorphizes the closure type so
    /// we can pass a trait-object handler as a concrete LoHandler
    /// bound.
    async fn run_remote_socket_test_shim(
        state: AcceptorsServerState,
        how: HowToConnect,
        config: AcceptorConfiguration,
        handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync>,
    ) -> Result<(), AcceptorsServerError> {
        run_acceptors_client(
            state,
            how,
            config,
            Arc::new(move |id, payloads| handler(id, payloads)),
        )
        .await
    }

    async fn do_connect_to_forwarder_local_test_shim(
        state: AcceptorsServerState,
        socket_path: PathBuf,
        config: AcceptorConfiguration,
        handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync>,
    ) -> Result<(), AcceptorsServerError> {
        do_connect_to_forwarder_local(
            state,
            socket_path,
            config,
            Arc::new(move |id, payloads| handler(id, payloads)),
        )
        .await
    }
}
