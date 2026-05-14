# yggdrasil-metrics — Prometheus metrics registry with EKG-parity names

## Scope

Wave 6 PR 16. The source-of-truth for the EKG-parity metric name set
SPOs key off in their Grafana dashboards / Alertmanager rules.

The crate ships:

- The 15 canonical `cardano.node.metrics.<name>.<type>` identifiers
  as `pub const`s under [`names`].
- [`install_prometheus_exporter`]: installs the global
  `metrics-exporter-prometheus` recorder + HTTP scrape listener on
  the operator-configured port (default `12798`).
- [`spawn_scrape_listener`]: tokio-task wrapper for installations
  that need the scrape listener owned by a separate task.

## Rules — Non-Negotiable

- **Tier-1 stable.** Every name in [`ALL_NAMES`] is part of the
  `docs/COMPATIBILITY.md` Tier-1 contract. Renames are semver-major;
  additions are minor.
- **`cardano.node.metrics.` prefix mandatory.** A unit test
  enforces every registered name uses this prefix verbatim so the
  Grafana variable expression `{__name__=~"cardano\\.node\\.metrics\\..*"}`
  in operator dashboards continues to match.
- **One global recorder per process.** `install_prometheus_exporter`
  installs the global `metrics` recorder. A second call returns
  the existing handle rather than panicking; the underlying
  `metrics` crate enforces single-recorder-per-process semantics.

## Naming parity

Synthesis crate. The `lib.rs` docstring carries the `## Naming parity`
stanza explaining how the 15 metric names map to upstream EKG names.

## R-arc tracking

Wave 6 PR 16. The follow-on integration PR replaces
`crates/node/tracer/src/metrics_server.rs`'s raw-TCP HTTP server
with `install_prometheus_exporter` and wires every `NodeMetrics`
update site through `metrics::*` calls.
