//! Top-level web server — wires REST endpoints to the HTTP listener.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Web.hs.
//!
//! Direct ports:
//!
//! - [`run_tx_submit_server`] — `runTxSubmitServer :: Trace IO TraceSubmitApi -> WebserverConfig -> ConsensusModeParams -> NetworkId -> SocketPath -> IO ()`.
//!   Outer supervisor: wraps the HTTP server in [`crate::util::log_exception`]
//!   (so any panic surfaces as an `EndpointException` trace event before
//!   being rethrown), then traces `EndpointExiting` on graceful return.
//! - [`tx_submit_app`] — `txSubmitApp :: Trace IO TraceSubmitApi -> ConsensusModeParams -> NetworkId -> SocketPath -> Application`.
//!   Returns the request-dispatch closure that gets handed to
//!   [`crate::rest::web::run_settings`].
//! - [`tx_submit_post`] — `txSubmitPost :: Trace IO TraceSubmitApi -> ConsensusModeParams -> NetworkId -> SocketPath -> ByteString -> Handler TxId`.
//!   Currently a stub returning 503 with a `TxSubmitFail
//!   (TxCmdTxSubmitConnectionError)` JSON body. R343+ replaces this
//!   body with real LocalTxSubmission integration; the JSON wire shape
//!   stays the same.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - `Cardano.Api.submitTxToNodeLocal` — replaced by the
//!   crate-internal [`crate::types::TxCmdError`] surface; concrete
//!   wiring to `crates/network/src/local_tx_submission_client.rs` lands
//!   at R343.
//! - `Cardano.Api.deserialiseFromCBOR` + multi-era `FromSomeType` table
//!   (`AsTx AsShelleyEra` / `AsTx AsAllegraEra` / ... / `AsTx AsConwayEra`)
//!   — replaced by direct dispatch to the `yggdrasil-ledger` per-era
//!   CBOR surface at R343.

use std::net::SocketAddr;
use std::sync::Arc;

use crate::cli::types::{ConsensusModeParams, NetworkId, SocketPath, TxSubmitNodeParams};
use crate::rest::types::WebserverConfig;
use crate::rest::web::{Handler, HttpRequest, HttpResponse, Tracer, run_settings};
use crate::tracing::trace_submit_api::TraceSubmitApi;
use crate::types::{EnvSocketError, TxCmdError, TxSubmitWebApiError};

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
/// [`TxSubmitNodeParams`] bundle. Convenience for callers that have
/// already validated the CLI flags.
pub async fn run_tx_submit_server_from_params(
    tracer: Tracer,
    params: TxSubmitNodeParams,
) -> std::io::Result<()> {
    run_tx_submit_server(
        tracer,
        &params.webserver_config,
        params.protocol,
        params.network_id,
        &params.socket_path,
    )
    .await
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
    Arc::new(move |req: &HttpRequest| {
        if req.path != "/api/submit/tx" {
            return HttpResponse::not_found();
        }
        if req.method != "POST" {
            return HttpResponse::method_not_allowed();
        }
        tx_submit_post(&tracer, protocol, network_id, &socket_path, &req.body)
    })
}

/// Mirror of upstream `txSubmitPost`. Currently a stub returning a
/// 503 with a structured [`TxSubmitWebApiError::TxSubmitFail`] body
/// containing a [`TxCmdError::TxCmdTxSubmitConnectionError`]. The
/// "real" LocalTxSubmission integration lands at R343.
///
/// Even at R342, the response carries upstream-byte-equivalent JSON
/// shape so client integrations can be tested against this binary
/// before the real handler ships.
pub fn tx_submit_post(
    tracer: &Tracer,
    _protocol: ConsensusModeParams,
    _network_id: NetworkId,
    _socket_path: &SocketPath,
    body: &[u8],
) -> HttpResponse {
    if body.is_empty() {
        let err = TxSubmitWebApiError::TxSubmitEmpty;
        let json = serde_json::to_vec(&err).unwrap_or_else(|_| b"{}".to_vec());
        return HttpResponse::bad_request_json(json);
    }

    // R342 placeholder: structured 503 with a connection-error stub.
    // R343 replaces this with real LocalTxSubmission wiring; the JSON
    // shape stays the same.
    let cmd_err = TxCmdError::TxCmdSocketEnvError(EnvSocketError::CliEnvVarLookup {
        message: "LocalTxSubmission integration lands at R343".to_string(),
    });
    let trace_err = cmd_err.clone();
    tracer(TraceSubmitApi::EndpointFailedToSubmitTransaction(trace_err));
    let api_err = TxSubmitWebApiError::TxSubmitFail(cmd_err);
    let json = serde_json::to_vec(&api_err).unwrap_or_else(|_| b"{}".to_vec());
    HttpResponse::service_unavailable_json(json)
}

/// Convenience: bind to the [`SocketAddr`] form of a [`WebserverConfig`]
/// without actually listening. Helper for tests and operator preflight.
pub fn resolve_bind_addr(webserver: &WebserverConfig) -> std::io::Result<SocketAddr> {
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

    #[test]
    fn tx_submit_post_empty_body_returns_400() {
        let resp = tx_submit_post(
            &nop_tracer(),
            ConsensusModeParams::CardanoMode,
            NetworkId::Mainnet,
            &SocketPath::new("/run/n.socket"),
            &[],
        );
        assert_eq!(resp.status, 400);
        assert!(String::from_utf8_lossy(&resp.body).contains("TxSubmitEmpty"));
    }

    #[test]
    fn tx_submit_post_nonempty_body_returns_503_placeholder() {
        let resp = tx_submit_post(
            &nop_tracer(),
            ConsensusModeParams::CardanoMode,
            NetworkId::Mainnet,
            &SocketPath::new("/run/n.socket"),
            &[0x83, 0xa0, 0xa0],
        );
        assert_eq!(resp.status, 503);
        let body = String::from_utf8_lossy(&resp.body);
        assert!(body.contains("TxSubmitFail"));
        assert!(body.contains("TxCmdSocketEnvError"));
    }

    #[test]
    fn tx_submit_app_routes_correctly() {
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
        let resp = app(&req);
        assert_eq!(resp.status, 404);

        // Wrong method on right path → 405
        let req = HttpRequest {
            method: "GET".to_string(),
            path: "/api/submit/tx".to_string(),
            content_type: None,
            body: Vec::new(),
        };
        let resp = app(&req);
        assert_eq!(resp.status, 405);

        // Empty body POST → 400
        let req = HttpRequest {
            method: "POST".to_string(),
            path: "/api/submit/tx".to_string(),
            content_type: Some("application/cbor".to_string()),
            body: Vec::new(),
        };
        let resp = app(&req);
        assert_eq!(resp.status, 400);

        // Non-empty body POST → 503 (placeholder)
        let req = HttpRequest {
            method: "POST".to_string(),
            path: "/api/submit/tx".to_string(),
            content_type: Some("application/cbor".to_string()),
            body: vec![0x83],
        };
        let resp = app(&req);
        assert_eq!(resp.status, 503);
    }

    #[test]
    fn tx_submit_post_traces_failed_event_on_placeholder() {
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
            &SocketPath::new("/run/n.socket"),
            &[0x83],
        );

        let events = events.lock().expect("lock");
        assert!(matches!(
            events.first(),
            Some(TraceSubmitApi::EndpointFailedToSubmitTransaction(_))
        ));
    }
}
