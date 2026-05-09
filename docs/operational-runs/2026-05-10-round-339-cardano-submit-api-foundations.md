---
title: 'R339: cardano-submit-api foundations — Types, Util, TraceSubmitApi data enum'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-10-round-339-cardano-submit-api-foundations/
---

# Round 339 — cardano-submit-api foundations

**Date:** 2026-05-10
**Branch:** `main`
**Predecessor:** [`R338`](2026-05-09-round-338-sister-tool-agents-md-refresh.md) *(if separate file; otherwise R338 is in-tree as the AGENTS.md refresh commit `1507217`)*
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase A.2 (cardano-submit-api).

## Summary

R339 lands the dependency-closed foundation of the cardano-submit-api
crate ahead of the R340 web round:

1. **`types.rs`** — full upstream port of `Cardano.TxSubmit.Types`
   (TxSubmitPort, DecoderError newtype, RawCborDecodeError,
   EnvSocketError, TxCmdError, TxSubmitWebApiError, render_tx_cmd_error).
2. **`tracing/trace_submit_api.rs`** — data-only `TraceSubmitApi`
   enum + `MediumTxId` (mirrors `renderMediumTxId` / `renderMediumHash`)
   + `render_human()` rendering that matches upstream `forHuman`
   strings byte-for-byte. **`LogFormatting`/`MetaTrace`/`asMetrics`
   tables intentionally deferred to R340** when the trace receiver
   wiring is decided.
3. **`util.rs`** — `log_exception` generic over a `FnOnce(TraceSubmitApi)`
   tracer + `FnOnce() -> Result<T, E>` action; preserves upstream's
   trace-then-rethrow semantic.

Servant API types (`TxSubmitApi`, `TxSubmitApiRecord`, `CBORStream`)
are documented in `types.rs` as a synthesis carve-out: axum's
router-based design has no Servant analog, and CBOR content-type
negotiation is handled inline at handler in R340.

The Servant carve-out is the only deviation from a strict 1:1 type
port. All other declarations and JSON shapes are byte-equivalent to
upstream.

## JSON shape parity

Upstream Aeson-derived `ToJSON` and Yggdrasil's serde implementations
produce byte-identical output for every error variant. The full table
lives in `crates/cardano-submit-api/src/types.rs` (module docstring),
and round-trip golden tests pin every shape in the unit test block.

| Upstream constructor               | Aeson shape                                                            | Serde mechanism                              |
|------------------------------------|------------------------------------------------------------------------|----------------------------------------------|
| `TxSubmitDecodeHex`                | `{"tag":"TxSubmitDecodeHex"}`                                          | `#[serde(tag, content)]` unit variant        |
| `TxSubmitFail err`                 | `{"tag":"TxSubmitFail","contents":<TxCmdError>}`                       | `#[serde(tag, content)]` payload variant     |
| `TxCmdSocketEnvError s`            | `{"tag":"TxCmdSocketEnvError","contents":{"message":"<msg>"}}`         | nested untagged struct-variant               |
| `RawCborDecodeError`               | `["<DecoderError>"...]`                                                | `#[serde(transparent)]` newtype              |
| `EnvSocketError`                   | `{"message":"<msg>"}` (no tag)                                         | `#[serde(untagged)]` enum                    |

## Diff inventory

- `crates/cardano-submit-api/Cargo.toml` — added `serde` + `serde_json`
  to `[dependencies]`.
- `crates/cardano-submit-api/src/types.rs` — full implementation
  (was: 13-line stub).
- `crates/cardano-submit-api/src/tracing/trace_submit_api.rs` — full
  implementation (was: 13-line stub).
- `crates/cardano-submit-api/src/util.rs` — full implementation (was:
  13-line stub).
- `docs/parity-matrix.json` — `sister-tool.cardano-submit-api`
  evidence/remaining_work refreshed to reflect R339 progress.

## Carve-outs

- **Servant API types** (`TxSubmitApi`, `TxSubmitApiRecord`,
  `CBORStream`): no Rust analog under axum; the type-level Servant
  API is replaced by an axum `Router` in R340. Synthesis docstring
  in `types.rs` records the rationale.
- **TxValidationErrorInCardanoMode mapping**: the
  `TxCmdTxSubmitValidationError(String)` variant currently carries a
  pre-rendered string. R340+ structured `ApplyError` mapping is
  tracked in the parity-matrix entry's `remaining_work` field — not
  a parity defect at this round, but flagged so future readers don't
  assume validation-detail parity is verified yet.

## Test inventory

| File                                    | Tests | Surface                                     |
|-----------------------------------------|-------|---------------------------------------------|
| `types.rs`                              | 24    | JSON shapes, Display, render_tx_cmd_error   |
| `tracing/trace_submit_api.rs`           | 13    | render_human + MediumTxId truncation        |
| `util.rs`                               | 4     | log_exception ok/err/context propagation    |
| `parser.rs` *(unchanged R335)*          | 10    | flag parsing                                |
| `tests/cli_help_golden.rs` *(R335)*     | 4     | byte-equivalence vs upstream binary         |
| Doctest *(util.rs::log_exception)*      | 1     | usage example compiles + runs               |
| **Crate total**                         | **56**|                                             |

Workspace contribution: 4,982 → 5,023 (+41).

## Verification

```bash
cargo fmt --all -- --check                          # clean
cargo check-all                                     # clean
cargo test-all                                      # 5,023 passed
cargo lint                                          # clean
python3 scripts/check-strict-mirror.py --fail-on-violation   # 0 violations
python3 scripts/check-parity-matrix.py              # clean (20 entries vs tag 11.0.1)
python3 scripts/check-fixture-manifest.py           # clean
cargo test -p yggdrasil-cardano-submit-api          # 56 tests pass
```

## Round roadmap

| Round | Scope                                                              | Status      |
|-------|--------------------------------------------------------------------|-------------|
| R335  | Skeleton (file-mirror tree + CLI parser + golden test)             | done        |
| R339  | Foundations: Types, Util, TraceSubmitApi data enum                 | **this**    |
| R340  | Rest/{Types,Parsers,Web}: axum router; CBOR content-type; LocalTxSubmission client wiring; full TraceSubmitApi `LogFormatting`/`MetaTrace`/`asMetrics` tables | next        |
| R341  | Metrics.hs Prometheus surface (port-occupied retry)                | scheduled   |
| R342  | Integration: end-to-end soak vs upstream binary                    | scheduled   |
| R343  | Closeout: AGENTS.md + CHANGELOG + parity-matrix `verified_11_0_1`  | scheduled   |

## Notes for future readers

The decision to land `tracing/trace_submit_api.rs` *with* this round
(instead of waiting for R340 per the original plan) was driven by a
hard dep-DAG constraint: upstream `Util.hs` imports
`Cardano.TxSubmit.Tracing.TraceSubmitApi (TraceSubmitApi (..))`. If
util.rs had landed without the data enum, log_exception would not
type-check.

The split-of-instances (data enum here, tracing instances at R340) is
documented at the top of `tracing/trace_submit_api.rs`. A future
reader looking at that file partway through R340 should not be
surprised to find no `LogFormatting` impl yet.
