---
title: 'R479: analysis::runner dispatch core + 4 handlers'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-479-analysis-runner-core/
---

# R479 — `analysis::runner` dispatch core + 4 shipped handlers

**Date:** 2026-05-11
**Arc:** R475-R481 (`db-analyser HasAnalysis` arc).
**Slice:** R479 = the central dispatch core ports
upstream's `Analysis.hs::runAnalysis` to Rust + ships 4
block-iteration-only handlers.

## Slice scope

R475-R478 built the per-era output-count and HasAnalysis
infrastructure; R479 wires those into a runnable dispatch core.
New file `crates/tools/db-analyser/src/analysis/runner.rs`
(strict mirror of upstream `Analysis.hs`) exports:

- `AnalysisOutcome` enum — per-analysis structured result with 4
  variants matching the 4 shipped handlers.
- `AnalysisError` enum — `RequiresLedgerStateApplyLoop` +
  `BlockOnlyHandlerPendingR480`.
- `apply_limit` — private `Vec<Block>` truncator implementing
  upstream's `take confLimit`.
- `run_analysis(config, blocks) -> Result<AnalysisOutcome,
  AnalysisError>` — central dispatch matching `AnalysisName`
  against the 13 variants.
- 4 shipped handlers:
  - `analysis_show_slot_block_no` — `(slot, block_no, header_hash)`
    per block.
  - `analysis_count_blocks` — total + first/last `(slot, block_no)`.
  - `analysis_count_tx_outputs` — cumulative + per-block tuples.
  - `analysis_show_block_header_size` — max + per-block tuples.

## Dispatch coverage (post-R479)

| AnalysisName | R479 verdict |
|--------------|-----------|
| `ShowSlotBlockNo` | ✅ shipped |
| `CountBlocks` | ✅ shipped |
| `CountTxOutputs` | ✅ shipped |
| `ShowBlockHeaderSize` | ✅ shipped |
| `ShowBlockTxsSize` | ⏳ `BlockOnlyHandlerPendingR480` |
| `ShowEBBs` | ⏳ `BlockOnlyHandlerPendingR480` |
| `OnlyValidation` | ⏳ `BlockOnlyHandlerPendingR480` |
| `StoreLedgerStateAt(_)` | 🚧 `RequiresLedgerStateApplyLoop` |
| `CheckNoThunksEvery(_)` | 🚧 `RequiresLedgerStateApplyLoop` |
| `TraceLedgerProcessing` | 🚧 `RequiresLedgerStateApplyLoop` |
| `BenchmarkLedgerOps(_)` | 🚧 `RequiresLedgerStateApplyLoop` |
| `ReproMempoolAndForge(_)` | 🚧 `RequiresLedgerStateApplyLoop` |
| `GetBlockApplicationMetrics(_)` | 🚧 `RequiresLedgerStateApplyLoop` |

7 of 13 analyses fully covered after R480. 6 await a future
ledger-state apply-loop arc (the apply-loop is a multi-round
commitment of its own — see
`crate::status::analysis_dispatch_status`).

## Design decision: takes `IntoIterator<Item = Block>` rather than a store

Upstream `runAnalysis` reads blocks from a `ChainDB` resource;
Yggdrasil's R481 wire-up will hand it
`ImmutableStore::suffix_after(&Point::Origin)` from
`yggdrasil-storage`. R479 keeps the runner crate-storage-agnostic
by accepting any `IntoIterator<Item = Block>` — the unit tests
hand it `Vec<Block>` directly. This avoids pulling `yggdrasil-
storage` into `db-analyser`'s dependency graph at R479
(deferred to R481).

The structured `AnalysisOutcome` return type lets the CLI wrapper
at `lib.rs::run()` (R481) format / render however needed without
the runner needing to know about stdout shape, CSV emitter, or
upstream-byte-equivalent rendering.

## Tests delivered (+21 cases)

| Test | Coverage |
|------|----------|
| `analysis_show_slot_block_no_empty_chain` | Empty input |
| `analysis_show_slot_block_no_per_block_emission` | 3-block emission shape |
| `analysis_count_blocks_empty_chain` | Empty input → zero count |
| `analysis_count_blocks_single_block` | First/last = the one block |
| `analysis_count_blocks_multi_block` | First/last differ, count correct |
| `analysis_count_tx_outputs_empty_chain` | Empty input |
| `analysis_count_tx_outputs_empty_blocks_yields_zero` | Blocks with no txs |
| `analysis_show_block_header_size_empty_chain` | Empty input |
| `analysis_show_block_header_size_tracks_max` | Max correctly identified across 3 blocks |
| `analysis_show_block_header_size_treats_missing_as_zero` | `header_cbor_size: None` → 0 |
| `run_analysis_dispatches_show_slot_block_no` | Dispatcher → handler routing |
| `run_analysis_dispatches_count_blocks` | Dispatcher → handler routing |
| `run_analysis_dispatches_count_tx_outputs` | Dispatcher → handler routing |
| `run_analysis_dispatches_show_block_header_size` | Dispatcher → handler routing |
| `run_analysis_show_block_txs_size_returns_pending_r480` | Pending-R480 error variant |
| `run_analysis_show_ebbs_returns_pending_r480` | Pending-R480 error variant |
| `run_analysis_benchmark_ledger_ops_returns_requires_apply_loop` | Requires-apply-loop error variant |
| `run_analysis_repro_mempool_returns_requires_apply_loop` | Requires-apply-loop error variant |
| `run_analysis_get_block_application_metrics_returns_requires_apply_loop` | Requires-apply-loop error variant |
| `run_analysis_respects_conf_limit` | `Limit::Limit(n)` truncates |
| `_shield_unused_imports` | Compile-time shield |

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,131 → 6,152
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Arc progress (R479/R481)

| Round | Status | Δ tests | Surface |
|-------|--------|---------|---------|
| R475  | shipped | +16 | per-era `TxBody::decode_output_count` + `Tx::output_count` |
| R476  | shipped | +14 | Byron EBB registry + `HasAnalysis for Block` impl |
| R477  | shipped | +6  | Allegra/Mary/Alonzo dispatch coverage |
| R478  | shipped | +6  | Babbage/Conway dispatch coverage |
| R479  | shipped | +21 | `analysis::runner::run_analysis` + 4 handlers |
| R480  | next | — | 3 more block-only handlers + 6 ledger-state deferrals (already wired by R479) |
| R481  | pending | — | Arc closeout — wire `lib.rs::run` to `run_analysis`, close `analysis_dispatch_status`, update parity-matrix |

## Stop point

R480 next: ship handler bodies for `ShowBlockTxsSize`, `ShowEBBs`,
`OnlyValidation` — converting their `BlockOnlyHandlerPendingR480`
returns into real `AnalysisOutcome` values. The 6
`RequiresLedgerStateApplyLoop` arms stay deferred until a future
arc.
