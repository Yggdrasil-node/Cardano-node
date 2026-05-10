//! EKG-style monitoring HTTP server — listens on the operator-
//! configured `hasEKG` endpoint and serves an HTML node-list +
//! per-node monitoring page.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Metrics/Monitoring.hs.
//!
//! Direct port of upstream's `runMonitoringServer` — bounded
//! subset. Like R409's Prometheus port, the route layout +
//! content-negotiation logic ship now; the per-node EKG page body
//! defers pending the EKG-equivalent metrics surface.
//!
//! Mapping summary:
//!
//! | Upstream                                          | Yggdrasil                              |
//! |---------------------------------------------------|----------------------------------------|
//! | `runMonitoringServer`                             | [`run_monitoring_server`]              |
//! | `dummyStore <- EKG.newStore`                      | (placeholder — full EKG-equivalent pending R411+) |
//! | `renderEkg`                                       | [`handle_per_node`] (HTML placeholder) |
//! | `Network.Wai.Handler.WarpTLS`                     | (deferred — see [`super::prometheus::tls_termination_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`System.Metrics.Store` (EKG store)**: same blocker as R409
//!   Prometheus's per-node exposition — the EKG-equivalent metrics
//!   surface isn't ported. The monitoring server still runs and
//!   serves the index + per-node placeholder; full EKG page lands
//!   in R411+.
//! - **TLS termination via `tlsCertificate.epForceSSL`**: same
//!   plumbing as R409's `tls_termination_status` deferral.
//! - **`sleep 0.2` listening-banner stagger**: preserved per
//!   upstream — applied via `tokio::time::sleep` before the
//!   listener bind. Note R409 uses 0.1 seconds, R410 (this file)
//!   uses 0.2 seconds — the upstream offsets prevent
//!   listening-banner collisions on stdout when both servers
//!   start together.

use std::net::SocketAddr;

use axum::Router;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::header::{ACCEPT, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::configuration::Endpoint;
use crate::types::ConnectedNodesNames;

use super::utils::{CONTENT_HDR_JSON, CONTENT_HDR_UTF8_HTML};

/// Shared application state passed into the axum router.
#[derive(Clone)]
struct AppState {
    /// Connected-nodes-names for `/` route + `/<slug>` lookup.
    connected_nodes_names: ConnectedNodesNames,
}

/// Run the EKG-style monitoring HTTP server. Mirror of upstream
/// `runMonitoringServer :: TracerEnv -> Endpoint -> IO RouteDictionary
/// -> IO ()`.
///
/// Per the R398 plan's TracerEnv option (b), this function takes
/// the slice of state it needs (connected_nodes_names + endpoint)
/// directly rather than coupling to the full TracerEnv record.
///
/// Returns a `JoinHandle<()>` for the spawned server task; callers
/// abort it to stop the listener.
pub async fn run_monitoring_server(
    connected_nodes_names: ConnectedNodesNames,
    endpoint: Endpoint,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    // Stagger to avoid concurrent listening-banner collisions
    // (R409 Prometheus uses 0.1; R410 Monitoring uses 0.2 to keep
    // the offset from upstream's exact stagger pattern).
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let state = AppState {
        connected_nodes_names,
    };

    let app = Router::new()
        .route("/", get(handle_root))
        .route("/{slug}", get(handle_per_node))
        .with_state(state);

    let bind_addr: SocketAddr = format!("{}:{}", endpoint.host, endpoint.port)
        .parse()
        .map_err(|e: std::net::AddrParseError| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
        })?;

    super::super::http_server::serve_router(bind_addr, app).await
}

async fn handle_root(State(state): State<AppState>, headers: HeaderMap) -> Response {
    use super::utils::compute_routes;
    use crate::environment::AcceptedMetrics;

    let dict = compute_routes(&state.connected_nodes_names, &AcceptedMetrics).await;
    let accepts_json = wants_json(&headers);
    if accepts_json {
        let bytes = dict.render_json();
        ([(CONTENT_TYPE, CONTENT_HDR_JSON.1)], bytes).into_response()
    } else {
        let bytes = dict.render_html("Cardano Tracer — Monitoring");
        ([(CONTENT_TYPE, CONTENT_HDR_UTF8_HTML.1)], bytes).into_response()
    }
}

async fn handle_per_node(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    use super::utils::compute_routes;
    use crate::environment::AcceptedMetrics;

    let dict = compute_routes(&state.connected_nodes_names, &AcceptedMetrics).await;
    let matched = dict.get_route_dictionary.iter().find(|(s, _)| s == &slug);
    if matched.is_none() {
        let body = b"<html><body><p>Node not found.</p></body></html>".to_vec();
        return ([(CONTENT_TYPE, CONTENT_HDR_UTF8_HTML.1)], body).into_response();
    }
    // Per-node EKG monitoring page deferred — return an HTML
    // placeholder pointing at the deferral status.
    let body = format!(
        "<html><body><h1>Cardano Tracer Monitoring</h1>\
         <p>Node slug: {slug}</p>\
         <p>Per-node EKG monitoring page pending — see\
         <code>crate::handlers::metrics::prometheus::exposition_status</code>\
         (same EKG-equivalent dependency).</p>\
         </body></html>",
    );
    ([(CONTENT_TYPE, CONTENT_HDR_UTF8_HTML.1)], body).into_response()
}

/// Inspect the `Accept` header to decide whether the client wants
/// JSON or HTML. Mirror of R409's `wants_json` — duplicated here
/// rather than re-exported because both servers handle their own
/// content negotiation independently per upstream's per-server
/// design.
fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("application/json"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wants_json_true_for_application_json_accept() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::ACCEPT,
            "application/json".parse().expect("parse"),
        );
        assert!(wants_json(&headers));
    }

    #[test]
    fn wants_json_false_for_html_accept() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::ACCEPT,
            "text/html".parse().expect("parse"),
        );
        assert!(!wants_json(&headers));
    }

    #[tokio::test]
    async fn run_monitoring_server_binds_and_serves() {
        let names = ConnectedNodesNames::new();
        names.insert(crate::types::NodeId::new("n1"), "alpha-pool".to_string());
        let endpoint = Endpoint {
            host: "127.0.0.1".to_string(),
            port: 0,
            force_ssl: None,
        };
        let result = run_monitoring_server(names, endpoint).await;
        assert!(result.is_ok(), "server should bind: {:?}", result.err());
        if let Ok(handle) = result {
            handle.abort();
        }
    }
}
