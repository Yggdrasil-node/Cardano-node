---
title: 'R498: cardano-submit-api AGENTS.md + parity-matrix refresh post-R344'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-498-cardano-submit-api-r344-doc-refresh/
---

# R498 — cardano-submit-api AGENTS.md + parity-matrix refresh

**Date:** 2026-05-11
**Scope:** documentation-only round.

## Slice scope

Survey of `crates/tools/cardano-submit-api/src/` shows the R344
Metrics.hs Prometheus surface is **fully shipped** (was marked
`next` in AGENTS.md):

- `metrics.rs` (282 lines, 13 tests): `MetricsRegistry` with
  lock-free `AtomicU64` `tx_submit` + `tx_submit_fail` counters,
  `register_metrics_server` with port-occupied retry up to 1000
  adjacent ports, `render_prometheus` exposition-format output.
- `web.rs::run_tx_submit_server_from_params` (line 101+): builds
  `MetricsRegistry::new()`, spawns `register_metrics_server` on
  `params.metrics_port`, wraps the operator tracer via
  `make_metrics_aware_tracer` so
  `TraceSubmitApi::ApplicationTxSubmitPostResult` events
  increment counters.

The AGENTS.md just hadn't been refreshed since R343.

## Files touched

1. **`crates/tools/cardano-submit-api/AGENTS.md`**:
   - Status field: `(post-R343 functional binary; metrics +
     integration soak + closeout remain)` → `(post-R344 functional
     binary with metrics; integration soak + closeout remain —
     operator-time gates)`.
   - Mini-arc table R344 row: status `next` → `done`; description
     expanded to enumerate the metrics.rs + web.rs wire-up.
   - R345/R346 marked as `scheduled (operator-time)` / `scheduled
     (gated on R345)` instead of just `scheduled`.
   - "Current functional surface (post-R343)" section: 2 ❌ rows
     for `/metrics` Prometheus endpoint + port-occupied retry
     flipped to ✅.

2. **`docs/parity-matrix.json::sister-tool.cardano-submit-api`**:
   - `next_milestone`: `R354` → `R346` (drops stale typed-parser-
     sweep round; advances to the actual remaining closeout
     round).
   - `rust_surface[0].role`: expanded from "R327 skeleton" to
     full R335-R344 implementation summary + R345-R346
     remaining-work pointer.

No source code touched. Tests unchanged.

## Verification log

```
cargo fmt --all -- --check                                  clean
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Stop point

cardano-submit-api documentation is now in sync with code at
HEAD. The crate ships a fully-functional operational binary
(`--help`/`--version` byte-equivalent, typed CLI dispatch, NtC
LocalTxSubmission wiring, Prometheus metrics endpoint). Only
the operator-time soak gate (R345) remains before the
`partial → verified_11_0_1` parity-matrix promotion.
