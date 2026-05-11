---
title: 'R480: remaining block-only handlers (ShowBlockTxsSize / ShowEBBs / OnlyValidation)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-480-remaining-block-only-handlers/
---

# R480 — remaining block-only handlers + ledger-state deferral hardening

**Date:** 2026-05-11
**Arc:** R475-R481 (`db-analyser HasAnalysis` arc).
**Slice:** R480 = ship the 3 remaining block-iteration-only
handlers; remove the `BlockOnlyHandlerPendingR480` placeholder
error variant.

## Slice scope

R479 wired the dispatch core end-to-end with 4 shipped handlers
and 3 placeholder `BlockOnlyHandlerPendingR480` arms; R480
replaces those placeholders with real handler bodies, completing
the 7-of-13 block-iteration-only coverage matrix.

New handlers + outcome variants:

| AnalysisName | New handler | Outcome variant |
|--------------|-------------|------------------|
| `ShowBlockTxsSize` | [`analysis_show_block_txs_size`] | `AnalysisOutcome::ShowBlockTxsSize { per_block: Vec<(SlotNo, i64, u64)> }` |
| `ShowEBBs` | [`analysis_show_ebbs`] | `AnalysisOutcome::ShowEBBs { ebbs: Vec<(SlotNo, HeaderHash, Option<HeaderHash>)> }` |
| `OnlyValidation` | [`analysis_only_validation`] | `AnalysisOutcome::OnlyValidation { blocks_processed: i64 }` |

`AnalysisError` is simplified: the `BlockOnlyHandlerPendingR480`
variant is removed (all 7 block-only handlers now ship bodies);
the enum keeps the single `RequiresLedgerStateApplyLoop` variant
that the 6 ledger-state-dependent analyses route to.

## Dispatch coverage matrix (post-R480)

| AnalysisName | Verdict |
|--------------|---------|
| `ShowSlotBlockNo` | ✅ shipped (R479) |
| `CountBlocks` | ✅ shipped (R479) |
| `CountTxOutputs` | ✅ shipped (R479) |
| `ShowBlockHeaderSize` | ✅ shipped (R479) |
| `ShowBlockTxsSize` | ✅ shipped (R480) |
| `ShowEBBs` | ✅ shipped (R480) |
| `OnlyValidation` | ✅ shipped (R480) |
| `StoreLedgerStateAt(_)` | 🚧 `RequiresLedgerStateApplyLoop` |
| `CheckNoThunksEvery(_)` | 🚧 `RequiresLedgerStateApplyLoop` |
| `TraceLedgerProcessing` | 🚧 `RequiresLedgerStateApplyLoop` |
| `BenchmarkLedgerOps(_)` | 🚧 `RequiresLedgerStateApplyLoop` |
| `ReproMempoolAndForge(_)` | 🚧 `RequiresLedgerStateApplyLoop` |
| `GetBlockApplicationMetrics(_)` | 🚧 `RequiresLedgerStateApplyLoop` |

**7/13 analyses shipped at the dispatcher level.** Remaining 6
await a future ledger-state apply-loop arc.

## ShowEBBs lookup design

`analysis_show_ebbs` walks the input blocks, looks up each
block's `header.hash` in the Byron known-EBB registry from R476
(`crate::byron_ebbs::known_ebbs()`), and emits a tuple for every
hit. The registry holds the upstream-canonical 325-entry table
across all networks (mainnet + staging + testnet), so a chain
walk through a Byron mainnet chain segment correctly identifies
its EBB markers without needing to know which network the chain
belongs to.

A planted-EBB integration test
(`analysis_show_ebbs_matches_byron_genesis_successor`) confirms
the lookup matches against the first Mainnet entry. Synthetic
blocks (the test fixture) emit zero EBBs as expected.

## Tests delivered (+8 cases)

| Test | Coverage |
|------|----------|
| `analysis_show_block_txs_size_empty_chain` | Empty input |
| `analysis_show_block_txs_size_empty_blocks_yields_zero_sizes` | Block-with-no-txs row shape |
| `analysis_show_ebbs_empty_chain` | Empty input |
| `analysis_show_ebbs_no_match_emits_empty` | Synthetic hashes don't match |
| `analysis_show_ebbs_matches_byron_genesis_successor` | Real Byron EBB hash → emits row with `prev_hash = None` |
| `analysis_only_validation_empty_chain` | Empty input → 0 blocks_processed |
| `analysis_only_validation_counts_blocks` | 3-block chain → 3 |
| `run_analysis_dispatches_*` | 3 dispatcher routing tests (R479's tests for the same variants kept) |

(Net: +8 — 3 dispatcher tests were already in R479 as
`returns_pending_r480` and got reshaped to assert the new
outcome variants.)

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,152 → 6,160
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Arc progress (R480/R481)

| Round | Status | Δ tests | Surface |
|-------|--------|---------|---------|
| R475  | shipped | +16 | per-era `TxBody::decode_output_count` + `Tx::output_count` |
| R476  | shipped | +14 | Byron EBB registry + `HasAnalysis for Block` impl |
| R477  | shipped | +6  | Allegra/Mary/Alonzo dispatch coverage |
| R478  | shipped | +6  | Babbage/Conway dispatch coverage |
| R479  | shipped | +21 | `analysis::runner::run_analysis` + 4 handlers |
| R480  | shipped | +8  | 3 more block-only handlers + simplified `AnalysisError` |
| R481  | next | — | Arc closeout — wire `lib.rs::run` to `run_analysis`, close `analysis_dispatch_status`, update parity-matrix |

## Stop point

R481 next — the arc closeout. Wire `lib.rs::run` to call
`analysis::runner::run_analysis`. Close the
`analysis_dispatch_status` descriptor. Update
`docs/parity-matrix.json::sister-tool.db-analyser` to reflect
the arc-shipped surface. Author the arc-closeout doc.
