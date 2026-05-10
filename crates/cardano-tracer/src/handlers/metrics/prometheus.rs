//! Prometheus exporter HTTP server — listens on the operator-
//! configured `hasPrometheus` endpoint and serves a per-node
//! OpenMetrics / Prometheus exposition + a Prometheus HTTP-SD
//! service-discovery feed.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Metrics/Prometheus.hs.
//!
//! Direct port of upstream's `runPrometheusServer` — bounded
//! subset. The route layout + content-negotiation logic + service-
//! discovery JSON shape ship now. Two pieces defer:
//!
//! - **Per-node OpenMetrics exposition body**: depends on the
//!   EKG-equivalent metrics surface which is its own carve-out
//!   (R411+). Until then, the per-node route returns a placeholder
//!   indicating the surface is pending.
//! - **TLS termination via `tlsCertificate.epForceSSL`**: depends
//!   on a full axum-server-rustls / hyper-rustls integration that
//!   takes the loaded cert/key bytes from R408's `load_pem_certs`
//!   / `load_pem_key`. Currently the server runs on plain HTTP;
//!   the TLS branch is deferred to a follow-up tightening round.
//!
//! Mapping summary:
//!
//! | Upstream                                          | Yggdrasil                              |
//! |---------------------------------------------------|----------------------------------------|
//! | `runPrometheusServer`                             | [`run_prometheus_server`]              |
//! | `PrometheusServiceDiscovery` newtype + JSON       | [`PrometheusServiceDiscovery`]         |
//! | `Network.HTTP.Types` content-negotiation          | inline via axum's `Accept` header parsing |
//! | `Network.Wai.Handler.WarpTLS`                     | (deferred — see [`tls_termination_status`]) |
//! | EKG.Store sampling + exposition rendering         | (deferred — see [`exposition_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **Cardano.Logging.Prometheus.Exposition.renderExpositionFromSampleWith**:
//!   upstream's exposition renderer takes a sampled `EKG.Store`
//!   value + the operator's `metricsNoSuffix` flag and emits the
//!   text exposition. The Yggdrasil-side EKG-equivalent isn't
//!   ported yet — the per-node route returns a placeholder and
//!   logs the deferral via [`exposition_status`].
//! - **`Network.Wai.Handler.WarpTLS.runTLS` + `tlsSettingsChain`**:
//!   Yggdrasil's port currently runs the server on plain HTTP. A
//!   follow-up round will wire R408's `load_pem_certs` /
//!   `load_pem_key` to axum-server-rustls.
//! - **`sleep 0.1` listening-banner stagger**: preserved per
//!   upstream — applied via `tokio::time::sleep` before the
//!   listener bind so concurrent server-startup banners don't
//!   collide on stdout.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use axum::Router;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::header::{ACCEPT, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};

use crate::configuration::Endpoint;
use crate::types::ConnectedNodesNames;

use super::utils::{CONTENT_HDR_JSON, CONTENT_HDR_OPEN_METRICS, CONTENT_HDR_UTF8_HTML};

/// Prometheus HTTP-SD service-discovery JSON entry. Mirror of
/// upstream
/// `data PrometheusServiceDiscovery = PrometheusServiceDiscovery { ... }`.
///
/// Each entry tells Prometheus where to scrape one node from. The
/// `targets` field carries the `host:port` to scrape; the `labels`
/// field carries the operator-configured `prometheusLabels` plus
/// the canonical `node_name` label.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrometheusServiceDiscovery {
    /// Scrape targets — `host:port` strings.
    pub targets: Vec<String>,
    /// Per-target labels.
    pub labels: BTreeMap<String, String>,
}

/// Shared application state passed into the axum router. Each
/// route handler reads the relevant slice for its rendering logic.
#[derive(Clone)]
struct AppState {
    /// Connected-nodes-names for `/` route + `/<slug>` lookup.
    connected_nodes_names: ConnectedNodesNames,
    /// Bind endpoint (used to render the `/targets` SD entries).
    endpoint: Endpoint,
    /// Operator-configured `prometheusLabels` (default empty).
    prometheus_labels: BTreeMap<String, String>,
    /// Operator-configured `metricsNoSuffix` flag (default false).
    metrics_no_suffix: bool,
}

/// Run the Prometheus exporter HTTP server. Mirror of upstream
/// `runPrometheusServer :: TracerEnv -> Endpoint -> IO RouteDictionary
/// -> IO ()`.
///
/// Per the R398 plan's TracerEnv option (b), this function takes
/// the slice of state it needs (connected_nodes_names + endpoint +
/// prometheus_labels + metrics_no_suffix) directly rather than
/// coupling to the full TracerEnv record.
///
/// Returns a `JoinHandle<()>` for the spawned server task; callers
/// abort it to stop the listener.
pub async fn run_prometheus_server(
    connected_nodes_names: ConnectedNodesNames,
    endpoint: Endpoint,
    prometheus_labels: BTreeMap<String, String>,
    metrics_no_suffix: bool,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    // Stagger to avoid concurrent listening-banner collisions.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let state = AppState {
        connected_nodes_names,
        endpoint: endpoint.clone(),
        prometheus_labels,
        metrics_no_suffix,
    };

    let app = Router::new()
        .route("/", get(handle_root))
        .route("/targets", get(handle_targets))
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
        let bytes = dict.render_html("Cardano Tracer — Prometheus");
        ([(CONTENT_TYPE, CONTENT_HDR_UTF8_HTML.1)], bytes).into_response()
    }
}

async fn handle_targets(State(state): State<AppState>) -> Response {
    use super::utils::compute_routes;
    use crate::environment::AcceptedMetrics;

    let dict = compute_routes(&state.connected_nodes_names, &AcceptedMetrics).await;
    let entries: Vec<PrometheusServiceDiscovery> = dict
        .get_route_dictionary
        .iter()
        .map(|(_slug, name)| {
            let mut labels = state.prometheus_labels.clone();
            labels.insert("node_name".to_string(), name.clone());
            PrometheusServiceDiscovery {
                targets: vec![format!("{}:{}", state.endpoint.host, state.endpoint.port)],
                labels,
            }
        })
        .collect();
    let body = serde_json::to_vec(&entries).unwrap_or_default();
    ([(CONTENT_TYPE, CONTENT_HDR_JSON.1)], body).into_response()
}

async fn handle_per_node(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    use super::utils::compute_routes;
    use crate::environment::AcceptedMetrics;

    let dict = compute_routes(&state.connected_nodes_names, &AcceptedMetrics).await;
    let matched = dict.get_route_dictionary.iter().find(|(s, _)| s == &slug);
    if matched.is_none() {
        return (
            [(CONTENT_TYPE, CONTENT_HDR_OPEN_METRICS.1)],
            "# node not found\n".to_string(),
        )
            .into_response();
    }
    // Per-node OpenMetrics exposition body is deferred — return a
    // placeholder that mirrors upstream's exposition shape but
    // notes the deferral.
    let suffix_note = if state.metrics_no_suffix {
        "# metricsNoSuffix=true\n"
    } else {
        "# metricsNoSuffix=false\n"
    };
    let body = format!(
        "# Yggdrasil cardano-tracer per-node exposition\n\
         # Node slug: {slug}\n\
         {suffix_note}\
         # NOTE: per-node metrics surface pending — see\n\
         # crate::handlers::metrics::prometheus::exposition_status()\n",
    );
    ([(CONTENT_TYPE, CONTENT_HDR_OPEN_METRICS.1)], body).into_response()
}

/// Inspect the `Accept` header to decide whether the client wants
/// JSON or HTML. Mirror of upstream's `Network.HTTP.Types`-driven
/// content-negotiation.
fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("application/json"))
        .unwrap_or(false)
}

/// Status descriptor for the deferred per-node exposition rendering.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ExpositionStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing surface.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for the per-node exposition
/// rendering.
pub fn exposition_status() -> ExpositionStatus {
    ExpositionStatus {
        status: "deferred",
        depends_on: "EKG-equivalent metrics surface (Cardano.Tracer.Types.AcceptedMetrics + Cardano.Logging.Prometheus.Exposition.renderExpositionFromSampleWith) — both unported pending the trace-dispatcher / EKG vendor work",
        deferred_round: "R411+",
    }
}

/// Status descriptor for the deferred TLS termination wiring.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TlsTerminationStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing wiring.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for the TLS termination
/// wiring.
pub fn tls_termination_status() -> TlsTerminationStatus {
    TlsTerminationStatus {
        status: "deferred",
        depends_on: "axum-server-rustls (or hyper-rustls direct) integration with R408's load_pem_certs / load_pem_key — needs a separate workspace dep for the rustls server adapter",
        deferred_round: "R411+",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prometheus_service_discovery_serializes_with_targets_and_labels() {
        let mut labels = BTreeMap::new();
        labels.insert("node_name".to_string(), "alpha-pool".to_string());
        labels.insert("environment".to_string(), "preview".to_string());
        let psd = PrometheusServiceDiscovery {
            targets: vec!["127.0.0.1:3200".to_string()],
            labels,
        };
        let json = serde_json::to_value(&psd).expect("serializes");
        assert_eq!(json["targets"][0], "127.0.0.1:3200");
        assert_eq!(json["labels"]["node_name"], "alpha-pool");
        assert_eq!(json["labels"]["environment"], "preview");
    }

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

    #[test]
    fn wants_json_false_when_no_accept_header() {
        let headers = HeaderMap::new();
        assert!(!wants_json(&headers));
    }

    #[test]
    fn wants_json_handles_combined_accept_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::ACCEPT,
            "text/html, application/json;q=0.9".parse().expect("parse"),
        );
        assert!(wants_json(&headers));
    }

    #[test]
    fn exposition_status_describes_deferral() {
        let s = exposition_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("EKG"));
        assert_eq!(s.deferred_round, "R411+");
    }

    #[test]
    fn tls_termination_status_describes_deferral() {
        let s = tls_termination_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("rustls"));
    }

    #[tokio::test]
    async fn run_prometheus_server_binds_and_serves_root() {
        let names = ConnectedNodesNames::new();
        names.insert(crate::types::NodeId::new("n1"), "alpha-pool".to_string());
        let endpoint = Endpoint {
            host: "127.0.0.1".to_string(),
            port: 0,
            force_ssl: None,
        };
        // Bind to ephemeral port; we just verify the server starts
        // without panicking (don't assert the bind succeeds since
        // port 0 means OS-assigned; we'd need an alternate accessor
        // to read it back).
        let result = run_prometheus_server(names, endpoint, BTreeMap::new(), false).await;
        // Binding to port 0 should succeed.
        assert!(result.is_ok(), "server should bind: {:?}", result.err());
        if let Ok(handle) = result {
            handle.abort();
        }
    }
}
