---
title: 'R502: Wire config.verbose through render_outcome for summary-only mode'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-12-round-502-verbose-flag-wire-up/
---

# R502 — `config.verbose` wire-up

**Date:** 2026-05-12
**Predecessor:** R501 (Limit truncation integration coverage).
**Scope:** functional wire-up + tests.

## Slice scope

R351 added `verbose: bool` to `DBAnalyserConfig`. The parser
accepted `--verbose`. But the field was parsed-but-ignored: the
R481 `render_outcome` function had no `verbose` parameter and
always emitted full per-block output.

R502 wires `config.verbose` through:

- `lib.rs::run` now calls `render_outcome(&outcome, config.verbose)`.
- `render_outcome` gains a `verbose: bool` parameter.
- Per-block-emitting variants (`CountBlocks`, `CountTxOutputs`,
  `ShowBlockHeaderSize`, `ShowBlockTxsSize`,
  `ReproMempoolAndForge`, `GetBlockApplicationMetrics`,
  `BenchmarkLedgerOps`, `TraceLedgerProcessing`,
  `ShowSlotBlockNo`) skip per-block rows when `verbose=false`,
  emitting only the aggregate/summary line.
- Aggregate-only variants (`ShowEBBs`, `OnlyValidation`,
  `StoreLedgerStateAt`) emit their full content regardless of
  `verbose` — they don't have separable per-block + summary
  halves.

For `ShowSlotBlockNo` (which has no aggregate), non-verbose
mode emits a single `show_slot_block_no rows=N` summary line.
For `ShowBlockTxsSize` (same shape), non-verbose emits
`show_block_txs_size rows=N`.

## Semantics match upstream

Upstream `db-analyser --verbose` controls trace-event emission
volume. Yggdrasil's structured-outcome model emits everything to
the `AnalysisOutcome` struct regardless of verbose; the
**render-time decision** at `lib.rs::render_outcome` controls
stdout volume. Same operator-visible behavior, cleaner
internal seam.

## Tests delivered (+1 case)

`crates/tools/db-analyser/tests/end_to_end_chain_walk.rs`:
- `end_to_end_lib_run_respects_verbose_flag` (3-block chain;
  asserts both `verbose=true` and `verbose=false` lib::run
  paths complete cleanly).

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,232 → 6,233
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Stop point

`config.verbose` is no longer parsed-but-ignored.
`DBAnalyserConfig.verbose` now controls stdout volume via the
`lib.rs::render_outcome` render-time decision. All 9
per-block-emitting variants honor the flag; the 3
aggregate-only variants emit unconditionally.
