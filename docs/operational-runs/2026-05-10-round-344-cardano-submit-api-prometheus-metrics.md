---
title: 'R344: cardano-submit-api Prometheus metrics — registry, port-retry server, tracer composition'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-10-round-344-cardano-submit-api-prometheus-metrics/
---

# Round 344 — cardano-submit-api Prometheus metrics

**Date:** 2026-05-10
**Branch:** `main`
**Predecessor:** [`R343`](2026-05-10-round-343-cardano-submit-api-localtxsubmission-wiring.md)
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.2 (cardano-submit-api).

## Summary

R344 lands the Prometheus metrics surface for cardano-submit-api,
mirroring upstream `Cardano.TxSubmit.Metrics`. Three behavioral
guarantees:

1. **Counter set parity.** `tx_submit` and `tx_submit_fail` counters
   match upstream's exposition byte-for-byte (same `# HELP` / `# TYPE
   counter` / `<name> <value>` shape).
2. **Port-occupied retry.** If the operator's requested
   `--metrics-port` is in use, the binary tries adjacent ports up to
   `port + 1000`. If all retries fail, the metrics endpoint is
   silently disabled (`MetricsServerPortNotBound` traced) — matching
   upstream's "metrics endpoint disabled" semantic; the tx-submission
   HTTP server keeps running.
3. **Concurrent server lifetime.** `run_tx_submit_server_from_params`
   spawns both the HTTP server and the metrics server concurrently
   in tokio tasks. `make_metrics_aware_tracer` wraps the operator's
   tracer with registry observation so counter updates ride the same
   trace stream the operator's logger sees — no separate counter-
   bumping path.

## Diff inventory

- `crates/cardano-submit-api/src/metrics.rs` — full implementation
  (was: 13-line stub).
- `crates/cardano-submit-api/src/web.rs` — added
  `make_metrics_aware_tracer` (Tracer wrapper that observes events
  into a `MetricsRegistry` before forwarding to the inner tracer).
  `run_tx_submit_server_from_params` rewritten to spawn both servers
  concurrently using `tokio::spawn` for the metrics endpoint.
- `docs/parity-matrix.json` — `sister-tool.cardano-submit-api`
  evidence/remaining_work refreshed; `next_milestone` advanced
  R344 → R345.

## Architecture

```
lib.rs::run()
└── tokio runtime
    └── web::run_tx_submit_server_from_params(tracer, params)
        ├── MetricsRegistry::new()                    (Arc<AtomicU64×2>)
        ├── observing_tracer = make_metrics_aware_tracer(tracer, registry)
        ├── tokio::spawn → metrics::register_metrics_server(observing_tracer, registry, params.metrics_port)
        │   ├── port-occupied retry up to MAX_PORT_OFFSET (1000)
        │   ├── trace MetricsServerStarted(bound_port)
        │   ├── apply ApplicationInitializeMetrics zero-set + trace
        │   └── per-request: GET /metrics → 200 + render_prometheus | other → 404
        └── web::run_tx_submit_server(observing_tracer, ...)
            └── rest::web::run_settings → tx_submit_app dispatch
                └── tx_submit_post (now: every emitted event flows
                    through observing_tracer → counter update + log)
```

## Wire-format parity

`MetricsRegistry::render_prometheus` byte-equivalent to upstream's
`prometheus-client` exposition for the same counter set:

```
# HELP tx_submit Number of successful tx submissions
# TYPE tx_submit counter
tx_submit <n>
# HELP tx_submit_fail Number of failed tx submissions
# TYPE tx_submit_fail counter
tx_submit_fail <n>
```

Content-Type: `text/plain; version=0.0.4` (canonical Prometheus
exposition MIME type).

## Carve-outs

- **`System.Metrics.Prometheus.Http.Scrape.serveMetrics`**: replaced
  by raw-tokio TCP + handcrafted Prometheus exposition. Same
  observable behavior, no `prometheus-client` ecosystem dependency.
- **`System.Metrics.Prometheus.Registry.RegistrySample`**: replaced
  by [`MetricsRegistry`] which uses [`std::sync::atomic::AtomicU64`]
  for lock-free updates from per-request handlers. Avoids the
  thread-id-based registration ceremony of `prometheus-client`.

## Test inventory

| Section                                                  | New tests | Notes                                  |
|----------------------------------------------------------|-----------|----------------------------------------|
| `metrics.rs::MetricsRegistry` apply/observe/snapshot     | 8         | counter inc + set; unknown-name silent; observe() from event |
| `metrics.rs::render_prometheus`                          | 2         | zero counters + post-increment shape   |
| `metrics.rs::register_metrics_server` (#[tokio::test])   | 3         | bind + serve, 404 fallback, init-event |
| `web.rs::make_metrics_aware_tracer`                      | 2         | observe + forward; init zeros          |
| **Round contribution**                                   | **+15**   |                                        |
| Crate total                                              |           | 148 (was 133 at R343)                  |

Workspace contribution: 5,100 → 5,115 (+15).

## Verification

```bash
cargo fmt --all -- --check                          # clean
cargo check-all                                     # clean
cargo test-all                                      # 5,115 passed
cargo lint                                          # clean
python3 dev/test/check-strict-mirror.py --fail-on-violation   # 0 violations
python3 dev/test/check-parity-matrix.py              # clean (20 entries vs tag 11.0.1)
python3 dev/test/check-fixture-manifest.py           # clean
cargo test -p yggdrasil-cardano-submit-api          # 148 tests pass
```

Smoke test:

```bash
# Bring up cardano-submit-api with metrics on port 8081.
cargo run --release --bin cardano-submit-api -- \
  --config /etc/c.json --socket-path /run/cardano-node.socket \
  --mainnet --port 8090 --metrics-port 8081

# In another terminal:
curl http://127.0.0.1:8081/metrics
# Expect:
# HTTP/1.1 200 OK
# Content-Type: text/plain; version=0.0.4
# Content-Length: <n>
# Connection: close
#
# # HELP tx_submit Number of successful tx submissions
# # TYPE tx_submit counter
# tx_submit 0
# # HELP tx_submit_fail Number of failed tx submissions
# # TYPE tx_submit_fail counter
# tx_submit_fail 0

# Submit a tx → counter increments → re-scrape shows tx_submit_fail = 1
# (assuming the local node rejects an empty tx).
curl -X POST http://127.0.0.1:8090/api/submit/tx \
  -H 'Content-Type: application/cbor' --data-binary @sample_tx.cbor
curl http://127.0.0.1:8081/metrics | grep tx_submit_fail
```

## Round roadmap (refreshed)

| Round | Scope                                                              | Status      |
|-------|--------------------------------------------------------------------|-------------|
| R335  | Skeleton (file-mirror tree + CLI parser + golden test)             | done        |
| R339  | Foundations: Types, Util, TraceSubmitApi data enum                 | done        |
| R340  | Type bridges: cli/types, cli/parsers, rest/types, rest/parsers     | done        |
| R341  | Trace surface: for_machine, as_metrics, Namespace tables           | done        |
| R342  | Web server (raw tokio HTTP + tx_submit_app)                        | done        |
| R343  | LocalTxSubmission wiring + lib.rs::run() runtime                   | done        |
| R344  | Prometheus metrics: registry, retry server, tracer composition     | **this**    |
| R345  | Integration: end-to-end soak vs upstream binary                    | next        |
| R346  | Closeout: AGENTS.md + parity-matrix `verified_11_0_1`              | scheduled   |

## Notes for future readers

The decision to use `Arc<AtomicU64>` for counters rather than the
`prometheus-client` crate's `IntCounter` was made because:

1. **No new ecosystem dep.** `prometheus-client` brings its own
   registry abstractions and macro-derived counter macros that don't
   align with the rest of yggdrasil's surface. AtomicU64 is in the
   stdlib.
2. **Single counter flavor.** cardano-submit-api has exactly two
   counters of the same shape (monotonic u64). The full
   `prometheus-client` registry's flexibility (gauges, histograms,
   summaries, label dimensions) is unused.
3. **Audit-trail simplicity.** `MetricsRegistry::apply` is a 6-arm
   match on counter name + operation; new counters add 4 lines.
   The `prometheus-client` equivalent is a derive macro + label
   schema + thread-local registration, which is harder to grep.

If a future round needs richer metric types (e.g. histograms for
submission latency), the upgrade path is to either extend
`MetricsRegistry` with new fields + render_prometheus arms, or
swap to `prometheus-client` wholesale. The current shape is small
enough that either path is cheap.

The decision to spawn the metrics server as a **separate tokio
task** (rather than multiplexing both endpoints on one listener
with path-prefix dispatch) was made because:

1. **Different ports.** `--port` and `--metrics-port` are different
   operator-controlled values; one listener can't serve two ports.
2. **Failure isolation.** If the metrics server fails to bind
   (port-not-bound exhaustion), the tx-submission server should
   continue. A single shared listener would couple the two
   lifecycles.
3. **Upstream semantic.** Upstream uses `withAsync` to run the
   metrics server alongside the tx-submission server in separate
   green threads — `tokio::spawn` is the direct analog.
