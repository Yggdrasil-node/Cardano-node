# yggdrasil-node-tracer — node tracer + metrics + cardano-tracer forwarder

## Scope

Extracted from `yggdrasil-node` in Wave 5 PR 7 (alongside config /
genesis) so the trace + metrics surface is consumable by sister tools
and Wave 6 observability scaffolding without linking the runtime.

The crate ships:

- `NodeTracer` — the tracing dispatch entry point used by every
  runtime sub-loop (sync, mempool, block-producer, governor, …).
- `NodeMetrics` + `MetricsSnapshot` + `MetricsSnapshot::to_prometheus_text` —
  the atomic-counter store and Prometheus-text serializer that
  Wave 6 PR 16 swaps to `metrics-exporter-prometheus`.
- `trace_fields` — the field-name constants source-of-truth
  (mirrored by `yggdrasil-telemetry::trace_fields` in Wave 2).
- `metrics_server` — the raw-TCP HTTP server on the operator-
  configurable `--metrics-port` (Wave 6 PR 16 replaces this with
  `PrometheusBuilder::with_http_listener`).
- `trace_forwarder` — the `TraceObject` CBOR codec Layer 1 used
  by the cardano-tracer Unix-socket forwarder (Layers 2/3 finish
  in Wave 6 PR 17, R502).

## Rules — Non-Negotiable

- **Tier-1 stability for trace_fields + EKG-parity metric names.**
  Operators key off these. See `docs/COMPATIBILITY.md`.
- **No runtime dependency on sibling node crates** (sync, mempool,
  block-producer, runtime). The tracer must be addable from any
  sub-loop without re-introducing the coupling Wave 5 broke.
- **Depends on yggdrasil-node-config only for NodeConfigFile /
  TraceNamespaceConfig.** Adding a deeper config dependency
  re-introduces the monolithic coupling.

## Naming parity

The lib.rs (former `node/src/tracer.rs`) carries the parity stanza.
`metrics_server.rs` and `trace_forwarder.rs` are synthesis (no
upstream mirror); the `## Naming parity` blocks in those files
declare so.

## R-arc tracking

Wave 5 PR 7 (extracted). Wave 6 PR 14-17 (R502) refactors the
metrics_server + trace_forwarder surfaces to use `metrics-exporter-
prometheus` and finish the cardano-tracer Mux Layer 2/3 protocol.
