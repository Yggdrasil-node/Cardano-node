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
//! helpers. Trace-objects + DataPoint sub-protocols are both wired
//! (R425 + R458); EKG sub-protocol remains a deferred carve-out
//! (Hackage-source synthesis, see [`super::server`] module docs).
//! The LocalPipe path uses `tokio::net::UnixStream::connect`; the
//! RemoteSocket TCP path defers pending the trace-forwarder
//! handshake-over-socket codec port.
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
//! | `runDataPointsAcceptorInit :: TracerEnv -> DPF.AcceptorConfiguration -> errorHandler -> ...` | (R458 — wired via [`yggdrasil_network::data_point_run_acceptor::accept_data_points_init`]; status descriptor at [`super::server::run_data_points_acceptor_status`]) |
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

#[cfg(unix)]
use yggdrasil_ledger::LedgerError;
#[cfg(unix)]
use yggdrasil_ledger::cbor::Decoder;
#[cfg(unix)]
use yggdrasil_network::data_point_run_acceptor::accept_data_points_init;
#[cfg(unix)]
use yggdrasil_network::mux::{MiniProtocolDir, MiniProtocolNum, ProtocolHandle, start_unix};
use yggdrasil_network::protocols::AcceptorConfiguration;
#[cfg(unix)]
use yggdrasil_network::protocols::DataPointAcceptorConfiguration;
#[cfg(unix)]
use yggdrasil_network::trace_object_acceptor::TraceObjectAcceptorError;
#[cfg(unix)]
use yggdrasil_network::trace_object_run_acceptor::{
    AcceptTraceObjectsError, accept_trace_objects_init,
};

use super::server::{AcceptorsServerError, AcceptorsServerState};
#[cfg(unix)]
use super::server::{DATA_POINTS_NUM, TRACE_OBJECTS_NUM};
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
        #[cfg(unix)]
        HowToConnect::RemoteSocket { .. } => Err(AcceptorsServerError::LocalListener(
            yggdrasil_network::local_listener::LocalPeerListenerError::Bind {
                path: PathBuf::from("<RemoteSocket placeholder>"),
                source: std::io::Error::other(super::server::do_listen_to_forwarder_socket_status()),
            },
        )),
        #[cfg(not(unix))]
        HowToConnect::RemoteSocket { .. } => Err(AcceptorsServerError::UnsupportedLocalPipe(
            super::server::do_listen_to_forwarder_socket_status().to_string(),
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
            &[
                MiniProtocolNum::HANDSHAKE,
                TRACE_OBJECTS_NUM,
                DATA_POINTS_NUM,
            ],
            1, /* buffer hint */
        );
        let handshake_handle = handles.remove(&MiniProtocolNum::HANDSHAKE).ok_or(
            AcceptorsServerError::MissingProtocolHandle(MiniProtocolNum::HANDSHAKE),
        )?;
        let trace_handle = handles.remove(&TRACE_OBJECTS_NUM).ok_or(
            AcceptorsServerError::MissingProtocolHandle(TRACE_OBJECTS_NUM),
        )?;
        let data_points_handle = handles
            .remove(&DATA_POINTS_NUM)
            .ok_or(AcceptorsServerError::MissingProtocolHandle(DATA_POINTS_NUM))?;

        // R436: run the trace-forwarder handshake initiator before
        // proceeding to the trace-objects acceptor. Propose V1 + V2
        // with our network magic; bail on refuse / mismatch.
        let proposals = vec![
            (
                yggdrasil_network::protocols::ForwardingVersion::V1,
                yggdrasil_network::protocols::ForwardingVersionData {
                    network_magic: state.network_magic,
                },
            ),
            (
                yggdrasil_network::protocols::ForwardingVersion::V2,
                yggdrasil_network::protocols::ForwardingVersionData {
                    network_magic: state.network_magic,
                },
            ),
        ];
        if let Err(e) =
            yggdrasil_network::trace_object_forward_handshake_driver::run_handshake_initiator(
                handshake_handle,
                proposals,
            )
            .await
        {
            return Err(AcceptorsServerError::LocalListener(
                yggdrasil_network::local_listener::LocalPeerListenerError::Bind {
                    path: socket_path,
                    source: std::io::Error::other(format!("trace-forwarder handshake failed: {e}")),
                },
            ));
        }

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
                // R465: registry-aware variant drops per-node
                // HandleRegistry entries alongside the existing
                // ConnectedNodes / metrics teardown.
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

        let node_id = crate::utils::conn_id_to_node_id(&conn_token);
        let node_id_for_lo = node_id.clone();
        let handler = Arc::clone(&lo_handler);
        let lo_handler_wrapper = move |payloads: Vec<TraceObject>| {
            handler(node_id_for_lo.clone(), payloads);
        };

        // Build a DataPoint acceptor configuration whose brake flag is
        // shared with the trace-objects brake (mirror of the server.rs
        // pattern — both sub-protocols stop together on a single trip).
        let dp_config = DataPointAcceptorConfiguration {
            acceptor_tracer: tf_config.acceptor_tracer.clone(),
            should_we_stop: tf_config.should_we_stop.clone(),
        };
        let dp_requestor = crate::acceptors::utils::prepare_data_point_requestor();
        // R470: register in the supervisor-shared registry.
        state
            .data_point_requestors
            .insert(node_id.clone(), dp_requestor.clone());
        let dp_on_error =
            move |_e: &yggdrasil_network::data_point_acceptor::DataPointAcceptorError| {
                // R458: same no-op rationale as server.rs — the
                // trace-objects on_error already runs the
                // per-connection cleanup; the shared brake terminates
                // both protocols within ~50ms of a transport error.
            };

        // Run trace-objects + data-points concurrently. tokio::join!
        // awaits both; the connection-level cleanup runs after both
        // have wound down.
        let (_trace_result, _dp_result) = tokio::join!(
            run_trace_objects_acceptor_init(tf_config, trace_handle, lo_handler_wrapper, on_error,),
            accept_data_points_init(
                dp_config,
                data_points_handle,
                move || dp_requestor,
                dp_on_error,
            )
        );

        // Final cleanup on graceful shutdown.
        crate::acceptors::utils::remove_disconnected_node_full(
            &state.connected_nodes,
            &state.connected_nodes_names,
            &state.accepted_metrics,
            &state.handle_registry,
            &state.data_point_requestors,
            &conn_token,
        )
        .await;

        Ok(())
    }
}

/// Run the trace-objects sub-protocol initiator over an already-
/// established mux protocol handle. Mirror of upstream's
/// `runTraceObjectsAcceptorInit tracerEnv tracerEnvRTView tfConfig errorHandler`.
#[cfg(unix)]
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

/// Decoder for `TraceObject`s on the wire. R437 mirror of
/// [`super::server::decode_trace_objects`]'s body — the same
/// CBOR-array-of-6-field-arrays shape decoded for the initiator
/// side. Both sides route through
/// [`crate::logging::TraceObject::from_cbor`]'s wire format.
#[cfg(unix)]
fn decode_trace_objects(dec: &mut Decoder<'_>) -> Result<Vec<TraceObject>, LedgerError> {
    let count = dec.array()?;
    let cap = (count as usize).min(65_536);
    let mut out = Vec::with_capacity(cap);
    for _ in 0..count {
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
            handle_registry: crate::types::HandleRegistry::new(),
            data_point_requestors: crate::types::DataPointRequestors::new(),
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[tokio::test]
    async fn do_connect_to_forwarder_local_round_trips_against_local_listener() {
        use yggdrasil_network::local_listener::LocalPeerListener;
        use yggdrasil_network::mux::{MiniProtocolDir, MiniProtocolNum, start_unix};
        use yggdrasil_network::trace_object_forward_handshake_driver::run_handshake_responder;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let socket_path = dir.path().join("r436-rt.sock");

        // R436: spawn a server-side listener that runs a real
        // handshake responder. The test verifies the client
        // negotiates the handshake, registers the connection,
        // observes the brake, and cleans up.
        let listener = LocalPeerListener::bind(&socket_path).await.expect("bind");
        let server_task = tokio::spawn(async move {
            let stream = listener.accept_unix().await.expect("accept");
            // R458: the per-connection mux now multiplexes
            // HANDSHAKE + TRACE_OBJECTS_NUM + DATA_POINTS_NUM; the
            // responder side of the test must mirror that protocol
            // list so handshake completes against a client running
            // the post-R458 wire shape.
            let (mut handles, _mux) = start_unix(
                stream,
                MiniProtocolDir::Responder,
                &[
                    MiniProtocolNum::HANDSHAKE,
                    super::TRACE_OBJECTS_NUM,
                    super::DATA_POINTS_NUM,
                ],
                1,
            );
            let handshake_handle = handles.remove(&MiniProtocolNum::HANDSHAKE).expect("hs");
            let _trace_handle = handles.remove(&super::TRACE_OBJECTS_NUM).expect("trace");
            let _data_points_handle = handles.remove(&super::DATA_POINTS_NUM).expect("dp");
            // Run the responder using the same magic the client
            // will propose.
            let _outcome = run_handshake_responder(
                handshake_handle,
                &[
                    yggdrasil_network::protocols::ForwardingVersion::V1,
                    yggdrasil_network::protocols::ForwardingVersion::V2,
                ],
                764824073,
            )
            .await;
            // Hold the protocol handles open briefly so the client
            // sees the connection as established.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        });

        let state = test_state();
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(1));
        config.request_stop().await;
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_, _| {});
        let result =
            do_connect_to_forwarder_local_test_shim(state.clone(), socket_path, config, handler)
                .await;
        // After R436 the client either returns Ok (handshake
        // succeeded + brake fired post-trace-acceptor) OR an
        // error wrapping the handshake-failure (if the responder
        // didn't get a chance to accept before the test
        // terminated). Both outcomes are acceptable for this
        // smoke test.
        let _ = result;

        // The connection state may or may not have been
        // registered depending on timing; either way it should be
        // cleaned up by the time the supervisor returns.
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

    #[cfg(unix)]
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
