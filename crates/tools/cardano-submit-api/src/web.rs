//! Top-level web server — wires REST endpoints to the HTTP listener.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Web.hs.
//!
//! Direct ports:
//!
//! - [`run_tx_submit_server`] — `runTxSubmitServer :: Trace IO TraceSubmitApi -> WebserverConfig -> ConsensusModeParams -> NetworkId -> SocketPath -> IO ()`.
//!   Outer supervisor: binds the TCP listener, traces
//!   `EndpointListeningOnPort`, accepts requests via the dispatch
//!   closure, traces `EndpointExiting` on graceful return.
//! - [`tx_submit_app`] — `txSubmitApp :: Trace IO TraceSubmitApi -> ConsensusModeParams -> NetworkId -> SocketPath -> Application`.
//!   Returns the request-dispatch closure that gets handed to
//!   [`crate::rest::web::run_settings`].
//! - [`tx_submit_post`] — `txSubmitPost :: Trace IO TraceSubmitApi -> ConsensusModeParams -> NetworkId -> SocketPath -> ByteString -> Handler TxId`.
//!   R343 wires real LocalTxSubmission via
//!   [`yggdrasil_network::ntc_connect`] +
//!   [`yggdrasil_network::LocalTxSubmissionClient`]: open a fresh NtC
//!   connection per request, submit the raw era-tagged CBOR tx bytes,
//!   map accept/reject/connect outcomes to the canonical
//!   [`TxCmdError`] / [`TxSubmitWebApiError`] surface, then close.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - `Cardano.Api.deserialiseFromCBOR` + multi-era `FromSomeType` table
//!   (`AsTx AsShelleyEra` / `AsTx AsAllegraEra` / ... / `AsTx AsConwayEra`)
//!   — Yggdrasil's tx-submit binary forwards the raw request body bytes
//!   directly to the NtC LocalTxSubmission protocol without per-era
//!   pre-decoding. Upstream's pre-decoding is a defense-in-depth check
//!   for malformed CBOR before round-tripping the bytes through the
//!   socket; Yggdrasil delegates that check to cardano-node, which
//!   returns `MsgRejectTx` for malformed bytes. Equivalent observable
//!   behavior, simpler code path.
//! - `Cardano.Api.deserialiseFromCBOR`'s full per-era typed decode table
//!   — Yggdrasil only extracts the first raw transaction-body CBOR item
//!   to derive the accepted-response TxId. Full typed pre-decode remains
//!   unnecessary for submission because the local node validates the raw
//!   transaction bytes and returns `MsgRejectTx` for malformed payloads.

use std::future::Future;
use std::sync::Arc;

#[cfg(unix)]
use yggdrasil_network::{LocalTxSubmissionClient, LocalTxSubmissionClientError, MiniProtocolNum};

use crate::cli::types::{ConsensusModeParams, NetworkId, SocketPath, TxSubmitNodeParams};
use crate::metrics::{MetricsRegistry, register_metrics_server};
use crate::rest::types::WebserverConfig;
use crate::rest::web::{Handler, HttpRequest, HttpResponse, Tracer, run_settings};
use crate::tracing::trace_submit_api::TraceSubmitApi;
use crate::types::{DecoderError, RawCborDecodeError, TxCmdError, TxSubmitWebApiError};

/// Cardano mainnet network magic. Mirrors the constant baked into
/// upstream's `cardano-cli`/`cardano-node` runtime defaults.
pub const MAINNET_NETWORK_MAGIC: u32 = 764824073;

/// Mirror of upstream `runTxSubmitServer`. Bind the web server,
/// trace `EndpointListeningOnPort`, serve requests, then trace
/// `EndpointExiting` on shutdown.
///
/// Returns `Ok(())` only if the listener exits cleanly; otherwise
/// returns the underlying `std::io::Error`.
///
/// `tracer` is invoked from arbitrary async tasks and MUST be cheap,
/// non-blocking, and thread-safe. The `Arc<dyn Fn>` shape mirrors
/// upstream's `Trace IO TraceSubmitApi` indirection.
pub async fn run_tx_submit_server(
    tracer: Tracer,
    webserver: &WebserverConfig,
    protocol: ConsensusModeParams,
    network_id: NetworkId,
    socket_path: &SocketPath,
) -> std::io::Result<()> {
    let addr = webserver.to_socket_addr().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid webserver host '{}': {err}", webserver.host),
        )
    })?;
    let handler = tx_submit_app(
        Arc::clone(&tracer),
        protocol,
        network_id,
        socket_path.clone(),
    );

    let result = run_settings(Arc::clone(&tracer), addr, handler).await;
    tracer(TraceSubmitApi::EndpointExiting);
    result
}

/// Variant of [`run_tx_submit_server`] that takes a
/// [`TxSubmitNodeParams`] bundle and spins both the HTTP server and
/// the Prometheus metrics server concurrently.
///
/// Uses [`make_metrics_aware_tracer`] to wire counter updates into
/// the supplied [`MetricsRegistry`] before forwarding events to the
/// caller's tracer. Both servers run forever (or until either errors);
/// returns when the HTTP server's listener exits.
pub async fn run_tx_submit_server_from_params(
    tracer: Tracer,
    params: TxSubmitNodeParams,
) -> std::io::Result<()> {
    let registry = MetricsRegistry::new();
    let observing_tracer = make_metrics_aware_tracer(tracer, Arc::clone(&registry));

    let metrics_tracer = Arc::clone(&observing_tracer);
    let metrics_registry = Arc::clone(&registry);
    let metrics_port = params.metrics_port;
    let metrics_handle = tokio::spawn(async move {
        let _ = register_metrics_server(metrics_tracer, metrics_registry, metrics_port).await;
    });

    let result = run_tx_submit_server(
        observing_tracer,
        &params.webserver_config,
        params.protocol,
        params.network_id,
        &params.socket_path,
    )
    .await;

    metrics_handle.abort();
    result
}

/// Wrap a [`Tracer`] so every emitted [`TraceSubmitApi`] event also
/// updates the [`MetricsRegistry`] counters before being forwarded
/// to the inner tracer.
///
/// Mirrors upstream's `bracket`-pattern where the metrics server
/// observes the same trace stream the operator-facing logger sees.
pub fn make_metrics_aware_tracer(inner: Tracer, registry: Arc<MetricsRegistry>) -> Tracer {
    Arc::new(move |evt: TraceSubmitApi| {
        registry.observe(&evt);
        inner(evt);
    })
}

/// Build the request-dispatch handler. Mirrors upstream `txSubmitApp`.
///
/// Path routing:
///
/// - `POST /api/submit/tx` → [`tx_submit_post`].
/// - all other paths → 404 Not Found.
/// - non-POST methods on `/api/submit/tx` → 405 Method Not Allowed.
pub fn tx_submit_app(
    tracer: Tracer,
    protocol: ConsensusModeParams,
    network_id: NetworkId,
    socket_path: SocketPath,
) -> Handler {
    Arc::new(move |req: HttpRequest| {
        let tracer = Arc::clone(&tracer);
        let socket_path = socket_path.clone();
        Box::pin(async move {
            if req.path != "/api/submit/tx" {
                return HttpResponse::not_found();
            }
            if req.method != "POST" {
                return HttpResponse::method_not_allowed();
            }
            tx_submit_post(&tracer, protocol, network_id, &socket_path, req.body).await
        })
    })
}

/// Mirror of upstream `txSubmitPost`. Submit the raw era-tagged CBOR
/// transaction bytes via NtC LocalTxSubmission.
///
/// Outcome mapping:
///
/// | Outcome                                  | HTTP status | Trace event                              |
/// |------------------------------------------|-------------|------------------------------------------|
/// | empty body                               | 400         | `EndpointFailedToSubmitTransaction`      |
/// | `MsgAcceptTx`                            | 202         | `EndpointSubmittedTransaction`           |
/// | `MsgRejectTx { reason }`                 | 400         | `EndpointFailedToSubmitTransaction`      |
/// | NtC connect failure                      | 503         | `EndpointFailedToSubmitTransaction`      |
/// | NtC protocol violation                   | 503         | `EndpointFailedToSubmitTransaction`      |
///
/// Successful (202) response body is the hex transaction id, matching
/// upstream `Handler TxId` / `PostAccepted '[JSON] TxId`.
pub async fn tx_submit_post(
    tracer: &Tracer,
    _protocol: ConsensusModeParams,
    network_id: NetworkId,
    socket_path: &SocketPath,
    body: Vec<u8>,
) -> HttpResponse {
    tx_submit_post_with_submitter(tracer, body, |body| {
        submit_via_ntc(socket_path, network_id, body)
    })
    .await
}

async fn tx_submit_post_with_submitter<F, Fut>(
    tracer: &Tracer,
    body: Vec<u8>,
    submit: F,
) -> HttpResponse
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<(), TxCmdError>>,
{
    if body.is_empty() {
        let err = TxSubmitWebApiError::TxSubmitEmpty;
        let json = serde_json::to_vec(&err).unwrap_or_else(|_| b"{}".to_vec());
        return HttpResponse::bad_request_json(json);
    }

    let tx_id = match yggdrasil_ledger::compute_tx_id_from_tx_cbor(&body) {
        Ok(tx_id) => tx_id,
        Err(err) => {
            let cmd_err = TxCmdError::TxCmdTxReadError(RawCborDecodeError(vec![DecoderError(
                format!("failed to compute TxId from submitted transaction CBOR: {err}"),
            )]));
            tracer(TraceSubmitApi::EndpointFailedToSubmitTransaction(
                cmd_err.clone(),
            ));
            let api_err = TxSubmitWebApiError::TxSubmitFail(cmd_err);
            let json = serde_json::to_vec(&api_err).unwrap_or_else(|_| b"{}".to_vec());
            return HttpResponse::bad_request_json(json);
        }
    };

    match submit(body).await {
        Ok(()) => {
            tracer(TraceSubmitApi::EndpointSubmittedTransaction(
                crate::tracing::trace_submit_api::MediumTxId::from_hash_bytes(&tx_id.0),
            ));
            let json = serde_json::to_vec(&hex::encode(tx_id.0)).unwrap_or_else(|_| b"{}".to_vec());
            HttpResponse::accepted_json(json)
        }
        Err(cmd_err) => {
            tracer(TraceSubmitApi::EndpointFailedToSubmitTransaction(
                cmd_err.clone(),
            ));
            let api_err = TxSubmitWebApiError::TxSubmitFail(cmd_err.clone());
            let json = serde_json::to_vec(&api_err).unwrap_or_else(|_| b"{}".to_vec());
            match cmd_err {
                TxCmdError::TxCmdTxSubmitValidationError(_) => HttpResponse::bad_request_json(json),
                _ => HttpResponse::service_unavailable_json(json),
            }
        }
    }
}

/// Connect to the local cardano-node NtC socket and submit a tx.
///
/// Returns `Ok(())` on `MsgAcceptTx`. Maps reject / protocol /
/// connection failures into [`TxCmdError`] variants matching upstream
/// `submitTxToNodeLocal`'s outcome surface.
#[cfg(unix)]
async fn submit_via_ntc(
    socket_path: &SocketPath,
    network_id: NetworkId,
    tx_bytes: Vec<u8>,
) -> Result<(), TxCmdError> {
    let network_magic = match network_id {
        NetworkId::Mainnet => MAINNET_NETWORK_MAGIC,
        NetworkId::Testnet(magic) => magic,
    };

    let mut conn = yggdrasil_network::ntc_connect(socket_path.as_path(), network_magic, false)
        .await
        .map_err(|err| {
            TxCmdError::TxCmdTxSubmitConnectionError(format!(
                "ntc_connect to {} failed: {err}",
                socket_path.as_path().display(),
            ))
        })?;

    let tx_handle = conn
        .protocols
        .remove(&MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION)
        .ok_or_else(|| {
            TxCmdError::TxCmdTxSubmitConnectionError(
                "NTC_LOCAL_TX_SUBMISSION protocol handle missing".to_string(),
            )
        })?;
    let mut client = LocalTxSubmissionClient::new(tx_handle);

    let result = match client.submit(tx_bytes).await {
        Ok(()) => Ok(()),
        Err(LocalTxSubmissionClientError::TransactionRejected(reason)) => {
            // Preserve the raw CBOR reject bytes alongside the
            // human-readable rendering so a future structured-
            // ApplyTxError decoder can pattern-match on the inner
            // payload without re-fetching it.
            let rendered = format!("rejected: 0x{}", hex::encode(&reason));
            Err(TxCmdError::TxCmdTxSubmitValidationError(
                crate::types::TxSubmitValidationError::new(reason, rendered),
            ))
        }
        Err(LocalTxSubmissionClientError::ConnectionClosed) => Err(
            TxCmdError::TxCmdTxSubmitConnectionError("NtC connection closed by remote".to_string()),
        ),
        Err(other) => Err(TxCmdError::TxCmdTxSubmitConnectionError(other.to_string())),
    };

    let _ = client.done().await;
    result
}

#[cfg(not(unix))]
async fn submit_via_ntc(
    socket_path: &SocketPath,
    network_id: NetworkId,
    tx_bytes: Vec<u8>,
) -> Result<(), TxCmdError> {
    let _ = (network_id, tx_bytes);
    Err(TxCmdError::TxCmdTxSubmitConnectionError(format!(
        "node-to-client transaction submission requires Unix-domain socket support: {}",
        socket_path.as_path().display()
    )))
}

/// Convenience: bind to the [`std::net::SocketAddr`] form of a
/// [`WebserverConfig`] without actually listening. Helper for tests
/// and operator preflight.
pub fn resolve_bind_addr(webserver: &WebserverConfig) -> std::io::Result<std::net::SocketAddr> {
    webserver.to_socket_addr().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid webserver host '{}': {err}", webserver.host),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nop_tracer() -> Tracer {
        Arc::new(|_| {})
    }

    #[test]
    fn resolve_bind_addr_loopback() {
        let cfg = WebserverConfig::new("127.0.0.1", 8090);
        let addr = resolve_bind_addr(&cfg).expect("resolves");
        assert_eq!(addr.to_string(), "127.0.0.1:8090");
    }

    #[test]
    fn resolve_bind_addr_invalid_host_errors() {
        let cfg = WebserverConfig::new("not-an-ip", 8090);
        assert!(resolve_bind_addr(&cfg).is_err());
    }

    #[tokio::test]
    async fn tx_submit_post_empty_body_returns_400() {
        let resp = tx_submit_post(
            &nop_tracer(),
            ConsensusModeParams::CardanoMode,
            NetworkId::Mainnet,
            &SocketPath::new("/run/n.socket"),
            Vec::new(),
        )
        .await;
        assert_eq!(resp.status, 400);
        assert!(String::from_utf8_lossy(&resp.body).contains("TxSubmitEmpty"));
    }

    #[tokio::test]
    async fn tx_submit_post_unreachable_socket_returns_503() {
        // Socket path that definitely doesn't exist — exercises the
        // ntc_connect failure path.
        let resp = tx_submit_post(
            &nop_tracer(),
            ConsensusModeParams::CardanoMode,
            NetworkId::Mainnet,
            &SocketPath::new("/nonexistent/socket/path"),
            vec![0x83, 0xa0, 0xa0],
        )
        .await;
        assert_eq!(resp.status, 503);
        let body = String::from_utf8_lossy(&resp.body);
        assert!(body.contains("TxSubmitFail"));
        assert!(body.contains("TxCmdTxSubmitConnectionError"));
    }

    #[tokio::test]
    async fn tx_submit_post_traces_failed_event_on_connect_error() {
        use std::sync::Mutex;
        let events: Arc<Mutex<Vec<TraceSubmitApi>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let tracer: Tracer = Arc::new(move |evt| {
            events_clone.lock().expect("lock").push(evt);
        });

        let _ = tx_submit_post(
            &tracer,
            ConsensusModeParams::CardanoMode,
            NetworkId::Mainnet,
            &SocketPath::new("/nonexistent/socket/path"),
            vec![0x83],
        )
        .await;

        let events = events.lock().expect("lock");
        assert!(matches!(
            events.first(),
            Some(TraceSubmitApi::EndpointFailedToSubmitTransaction(_))
        ));
    }

    #[tokio::test]
    async fn tx_submit_post_success_returns_tx_id_json_and_traces_medium_id() {
        use std::sync::Mutex;
        let events: Arc<Mutex<Vec<TraceSubmitApi>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let tracer: Tracer = Arc::new(move |evt| {
            events_clone.lock().expect("lock").push(evt);
        });

        let tx = vec![0x83, 0xa0, 0xa0, 0xf6];
        let expected_tx_id = hex::encode(yggdrasil_ledger::compute_tx_id(&[0xa0]).0);

        let resp = tx_submit_post_with_submitter(&tracer, tx, |_submitted| async { Ok(()) }).await;

        assert_eq!(resp.status, 202);
        assert_eq!(
            resp.body,
            serde_json::to_vec(&expected_tx_id).expect("json txid")
        );
        let events = events.lock().expect("lock");
        assert!(matches!(
            events.first(),
            Some(TraceSubmitApi::EndpointSubmittedTransaction(tx_id))
                if tx_id.to_string() == expected_tx_id[..16]
        ));
    }

    #[tokio::test]
    async fn tx_submit_app_routes_correctly() {
        let app = tx_submit_app(
            nop_tracer(),
            ConsensusModeParams::CardanoMode,
            NetworkId::Mainnet,
            SocketPath::new("/run/n.socket"),
        );

        // Wrong path → 404
        let req = HttpRequest {
            method: "POST".to_string(),
            path: "/wrong".to_string(),
            content_type: None,
            body: Vec::new(),
        };
        let resp = app(req).await;
        assert_eq!(resp.status, 404);

        // Wrong method on right path → 405
        let req = HttpRequest {
            method: "GET".to_string(),
            path: "/api/submit/tx".to_string(),
            content_type: None,
            body: Vec::new(),
        };
        let resp = app(req).await;
        assert_eq!(resp.status, 405);

        // Empty body POST → 400
        let req = HttpRequest {
            method: "POST".to_string(),
            path: "/api/submit/tx".to_string(),
            content_type: Some("application/cbor".to_string()),
            body: Vec::new(),
        };
        let resp = app(req).await;
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn mainnet_network_magic_constant_matches_upstream() {
        assert_eq!(MAINNET_NETWORK_MAGIC, 764824073);
    }

    #[test]
    fn metrics_aware_tracer_observes_and_forwards() {
        use std::sync::Mutex;
        let registry = MetricsRegistry::new();
        let inner_events: Arc<Mutex<Vec<TraceSubmitApi>>> = Arc::new(Mutex::new(Vec::new()));
        let inner_clone = Arc::clone(&inner_events);
        let inner: Tracer = Arc::new(move |evt| inner_clone.lock().expect("lock").push(evt));

        let wrapped = make_metrics_aware_tracer(inner, Arc::clone(&registry));

        // Submitted event → tx_submit counter ++ + forwarded to inner.
        wrapped(TraceSubmitApi::EndpointSubmittedTransaction(
            crate::tracing::trace_submit_api::MediumTxId::from_rendered("a"),
        ));
        // Failed event → tx_submit_fail counter ++ + forwarded to inner.
        wrapped(TraceSubmitApi::EndpointFailedToSubmitTransaction(
            TxCmdError::TxCmdTxSubmitConnectionError("x".to_string()),
        ));
        // Listener event → no counter update + forwarded.
        wrapped(TraceSubmitApi::EndpointListeningOnPort(
            "127.0.0.1:0".parse().expect("addr"),
        ));

        assert_eq!(registry.snapshot(), (1, 1));
        assert_eq!(inner_events.lock().expect("lock").len(), 3);
    }

    #[test]
    fn metrics_aware_tracer_initialize_event_zeros_counters() {
        let registry = MetricsRegistry::new();
        registry.apply(&[
            crate::tracing::trace_submit_api::MetricUpdate::counter_set("tx_submit", 99),
            crate::tracing::trace_submit_api::MetricUpdate::counter_set("tx_submit_fail", 99),
        ]);
        assert_eq!(registry.snapshot(), (99, 99));

        let nop: Tracer = Arc::new(|_| {});
        let wrapped = make_metrics_aware_tracer(nop, Arc::clone(&registry));
        wrapped(TraceSubmitApi::ApplicationInitializeMetrics);

        assert_eq!(registry.snapshot(), (0, 0));
    }
}
