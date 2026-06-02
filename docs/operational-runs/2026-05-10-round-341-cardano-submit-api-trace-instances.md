---
title: 'R341: cardano-submit-api trace surface — for_machine, as_metrics, Namespace tables'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-10-round-341-cardano-submit-api-trace-instances/
---

# Round 341 — cardano-submit-api trace surface

**Date:** 2026-05-10
**Branch:** `main`
**Predecessor:** [`R340`](2026-05-10-round-340-cardano-submit-api-type-bridges.md)
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.2 (cardano-submit-api).

## Summary

R341 completes the trace surface for cardano-submit-api by porting the
upstream `LogFormatting` and `MetaTrace` instance methods. R339 had
landed the data-only `TraceSubmitApi` enum + `render_human` mirror of
upstream `forHuman`; R341 adds:

| Upstream method                           | Yggdrasil mirror                               |
|-------------------------------------------|------------------------------------------------|
| `LogFormatting.forMachine _ event`        | `TraceSubmitApi::for_machine`                  |
| `LogFormatting.asMetrics event`           | `TraceSubmitApi::as_metrics`                   |
| `MetaTrace.namespaceFor event`            | `TraceSubmitApi::namespace_for`                |
| `MetaTrace.severityFor namespace _`       | `Namespace::severity`                          |
| `MetaTrace.metricsDocFor namespace`       | `Namespace::metrics_doc`                       |
| `MetaTrace.allNamespaces`                 | `ALL_NAMESPACES` const                         |
| (segments() inherent method)              | `Namespace::segments`                          |

Plus three supporting types:

- **`Severity`** (Debug | Info | Warning | Error) mirrors
  `Cardano.Logging.SeverityS`.
- **`Namespace`** (11-variant enum) mirrors upstream's
  `Namespace [] [...]` paths; segments() returns the inner string
  array.
- **`MetricUpdate`** (CounterInc { name } | CounterSet { name, value })
  mirrors upstream `Cardano.Logging.MetricM`'s `CounterM name (Maybe v)`
  shape.

The Rust port intentionally does NOT implement `LogFormatting` or
`MetaTrace` typeclasses because Yggdrasil's tracing integration is
backend-agnostic at this layer. Callers wanting structured-trace
forwarding can map the returned values into whatever backend
(`tracing`, `slog`, cardano-tracer NtN protocol) is wired at runtime.

## JSON shape parity

`for_machine` produces output byte-equivalent to upstream Aeson:

| Variant                                | Upstream Aeson            | Yggdrasil JSON output                                      |
|----------------------------------------|---------------------------|------------------------------------------------------------|
| `ApplicationStopping`                  | `mempty`                  | `{}`                                                       |
| `ApplicationInitializeMetrics`         | `mempty`                  | `{}`                                                       |
| `EndpointListeningOnPort addr`         | `singleton "addr" ...`    | `{"addr":"<addr>"}`                                        |
| `EndpointException txt e`              | `mconcat ["txt"...]`      | `{"txt":"<txt>","exception":"<exception>"}`                |
| `EndpointFailedToSubmitTransaction err`| `singleton "error" ...`   | `{"error":"<rendered TxCmdError>"}`                        |
| `EndpointSubmittedTransaction txid`    | `singleton "txId" ...`    | `{"txId":"<medium-form txid>"}`                            |
| `EndpointExiting`                      | `mempty`                  | `{}`                                                       |
| `MetricsServerStarted port`            | `singleton "port" ...`    | `{"port":<port>}`                                          |
| `MetricsServerError except`            | `singleton "exception"...`| `{"exception":"<msg>"}`                                    |
| `MetricsServerPortOccupied port`       | `singleton "port" ...`    | `{"port":<port>}`                                          |
| `MetricsServerPortNotBound port`       | `singleton "port" ...`    | `{"port":<port>}`                                          |

## Severity / metrics-doc tables

| Namespace                           | Severity   | Metrics doc                                                                 |
|-------------------------------------|------------|-----------------------------------------------------------------------------|
| `Application/Stopping`              | Info       | —                                                                           |
| `Application/InitializeMetrics`     | Debug      | tx_submit_fail (init 0), tx_submit (init 0)                                 |
| `Endpoint/ListeningOnPort`          | Info       | —                                                                           |
| `Endpoint/Exception`                | Error      | —                                                                           |
| `Endpoint/Exiting`                  | Info       | —                                                                           |
| `Endpoint/FailedToSubmitTransaction`| Info       | tx_submit_fail (counter)                                                    |
| `Endpoint/SubmittedTransaction`     | Info       | tx_submit (counter)                                                         |
| `Metrics/Started`                   | Info       | —                                                                           |
| `Metrics/Error`                     | Warning    | —                                                                           |
| `Metrics/PortOccupied`              | Warning    | —                                                                           |
| `Metrics/PortNotBound`              | Error      | —                                                                           |

## Diff inventory

- `crates/cardano-submit-api/src/tracing/trace_submit_api.rs` — added
  `for_machine`, `as_metrics`, `namespace_for` inherent methods on
  `TraceSubmitApi`; added `Namespace` enum + `Severity` enum +
  `MetricUpdate` enum + `ALL_NAMESPACES` const + per-namespace
  inherent `segments()` / `severity()` / `metrics_doc()` accessors.
- `docs/parity-matrix.json` — `sister-tool.cardano-submit-api`
  evidence/remaining_work refreshed; `next_milestone` advanced
  R341 → R342.

## Test inventory

| Section                            | New tests | Total in trace_submit_api |
|------------------------------------|-----------|---------------------------|
| `for_machine`                      | 7         |                           |
| `as_metrics`                       | 4         |                           |
| `namespace_for` + ALL_NAMESPACES   | 3         |                           |
| `severity` / `metrics_doc`         | 8         |                           |
| `MetricUpdate`                     | 2         |                           |
| (R339 carry-over)                  |           | 14                        |
| **Round contribution**             | **+24**   |                           |
| **trace_submit_api total**         |           | **38**                    |

Workspace contribution: 5,052 → 5,076 (+24).
Crate total: 85 → 109.

## Verification

```bash
cargo fmt --all -- --check                          # clean
cargo check-all                                     # clean
cargo test-all                                      # 5,076 passed
cargo lint                                          # clean
python3 dev/test/check-strict-mirror.py --fail-on-violation   # 0 violations
python3 dev/test/check-parity-matrix.py              # clean (20 entries vs tag 11.0.1)
python3 dev/test/check-fixture-manifest.py           # clean
cargo test -p yggdrasil-cardano-submit-api          # 109 tests pass
```

## Round roadmap (refreshed)

| Round | Scope                                                              | Status      |
|-------|--------------------------------------------------------------------|-------------|
| R335  | Skeleton (file-mirror tree + CLI parser + golden test)             | done        |
| R339  | Foundations: Types, Util, TraceSubmitApi data enum                 | done        |
| R340  | Type bridges: cli/types, cli/parsers, rest/types, rest/parsers     | done        |
| R341  | Trace surface: for_machine, as_metrics, Namespace tables           | **this**    |
| R342  | Rest/Web + Web.hs (axum router; CBOR; LocalTxSubmission)           | next        |
| R343  | Metrics.hs Prometheus surface (port-occupied retry)                | scheduled   |
| R344  | Integration: end-to-end soak vs upstream binary                    | scheduled   |
| R345  | Closeout: AGENTS.md + CHANGELOG + parity-matrix `verified_11_0_1`  | scheduled   |

## Notes for future readers

The decision to encode `Namespace` as a Rust enum (rather than a
`(Vec<&str>, Vec<&str>)` tuple matching upstream's `Namespace [outer]
[inner]` shape) was made because:

1. The outer list is *always* empty for cardano-submit-api's
   namespaces — upstream emits `Namespace [] [...]` in every case.
2. The inner list values come from a closed set (11 namespaces); a
   sum type captures that closedness compile-time.
3. Pattern-matching on the enum is more ergonomic than tuple-shape
   destructuring in Rust.

If a future round needs to emit non-empty outer lists (e.g. when
trace events forward from sub-modules), the upgrade path is to
introduce `Namespace::Inner(...)` / `Namespace::Outer(...)` variants
or a `(Vec<...>, Inner)` newtype.

Wildcard match on `MetricsServerStarted(port) | MetricsServerPortOccupied(port) | MetricsServerPortNotBound(port)`
in `for_machine` collapses three variants that emit identical JSON
shape (`{"port":<n>}`); this matches the upstream collapse where
all three are handled by a single `singleton "port" (toJSON port)`
arm.
