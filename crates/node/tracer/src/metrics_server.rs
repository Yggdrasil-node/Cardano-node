//! Prometheus + JSON metrics HTTP server.
//!
//! Spawned by `run_node` when the operator passes `--metrics-port`.
//! Serves a small set of routes for monitoring agents:
//!
//!  - `GET /metrics`            — Prometheus text exposition (`text/plain; version=0.0.4`)
//!  - `GET /metrics/json`       — JSON snapshot of [`NodeMetrics`]
//!  - `GET /health`             — terse health JSON
//!  - `GET /debug/{health,metrics,metrics/json,metrics/prometheus}` — debug aliases
//!
//! Mirrors upstream `Cardano.Node.Tracing.Tracers.PrometheusEndpoint`
//! (cardano-node uses `ekg` + `prometheus` over `wai`/`warp`); Yggdrasil
//! avoids the HTTP-framework dependency by implementing the tiny request
//! routing on raw tokio TCP. Only `Connection: close` responses are
//! emitted — no pipelining — which is fine for monitoring scrapers.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Tracing/Tracers/Startup.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side Prometheus metrics HTTP server. Exposes `/metrics` endpoint with the workspace's `MetricsSnapshot` exposition. Upstream's equivalent is `cardano-tracer` (separate process via the trace-forwarder mini-protocol); Yggdrasil's binary-side server is in-process for operational simplicity.

use std::sync::Arc;

use crate::NodeMetrics;

/// Serve a tiny HTTP endpoint exposing node metrics + health on the
/// loopback interface. Routes:
///
/// - `GET /metrics`            (Prometheus text)
/// - `GET /metrics/json`       (JSON snapshot)
/// - `GET /health`             (health JSON)
/// - `GET /debug/health`       (health JSON)
///
/// Uses raw tokio TCP — no HTTP framework dependency required.
pub async fn serve_metrics(port: u16, metrics: Arc<NodeMetrics>) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    loop {
        let (mut stream, _addr) = listener.accept().await?;
        let metrics = Arc::clone(&metrics);
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let n = match stream.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            let request = String::from_utf8_lossy(&buf[..n]);
            let (status, content_type, body) = metrics_http_response(&request, &metrics);

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}

pub fn metrics_http_response(
    request: &str,
    metrics: &NodeMetrics,
) -> (&'static str, &'static str, String) {
    // Route order matters: more-specific prefixes MUST be tested before
    // less-specific ones.  Before the fix, `GET /metrics` was checked
    // before `GET /metrics/json`, so every JSON request matched the
    // Prometheus-text prefix first and never reached the JSON arm,
    // silently turning `/metrics/json` into dead code.
    if request.starts_with("GET /health") || request.starts_with("GET /debug/health") {
        let snap = metrics.snapshot();
        let body = serde_json::json!({
            "status": "ok",
            "uptime_seconds": snap.uptime_ms / 1000,
            "blocks_synced": snap.blocks_synced,
            "current_slot": snap.current_slot,
        })
        .to_string();
        ("200 OK", "application/json", body)
    } else if request.starts_with("GET /metrics/json")
        || request.starts_with("GET /debug/metrics/json")
        || request.starts_with("GET /debug/metrics ")
        || request.starts_with("GET /debug ")
    {
        // JSON first — must precede the `/metrics` / `/debug/metrics`
        // Prometheus text arms below.
        let snap = metrics.snapshot();
        match serde_json::to_string_pretty(&snap) {
            Ok(json) => ("200 OK", "application/json", json),
            Err(_) => (
                "500 Internal Server Error",
                "text/plain",
                "serialization error".to_owned(),
            ),
        }
    } else if request.starts_with("GET /debug/metrics/prometheus")
        || request.starts_with("GET /debug/metrics")
        || request.starts_with("GET /metrics")
    {
        // Wave 6 PR 16 follow-on: emit BOTH the legacy `yggdrasil_*`
        // metrics and the EKG-parity `cardano_node_metrics_*` names
        // so SPOs migrating from upstream cardano-node 11.0.1 keep
        // their Grafana dashboards / Alertmanager rules unchanged.
        // The legacy block retires at v1.0 per docs/COMPATIBILITY.md.
        let snap = metrics.snapshot();
        let mut body = snap.to_prometheus_text();
        body.push_str(&snap.to_ekg_parity_prometheus_text());
        ("200 OK", "text/plain; version=0.0.4; charset=utf-8", body)
    } else {
        ("404 Not Found", "text/plain", "not found\n".to_owned())
    }
}
