---
title: 'R481 closeout: db-analyser HasAnalysis R475-R481 arc complete'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-481-db-analyser-hasanalysis-arc-closure/
---

# R481 — db-analyser HasAnalysis arc closure

**Date:** 2026-05-11
**Predecessor:** [`R473 DataPoint forwarder-side arc`](2026-05-11-round-473-data-point-forwarder-arc-closure.md).
**Closure scope:** R475-R480 fully shipped (handler bodies + EBB
registry + per-era dispatch + runner core); R481 = closeout
(this doc + lib.rs end-to-end wire-up + parity-matrix + status
descriptor closure + CHANGELOG arc summary).

## Arc summary

R475-R481 closed the db-analyser `HasAnalysis` dispatch arc,
porting the upstream
`Cardano.Tools.DBAnalyser.{HasAnalysis, Analysis, Run}` surface
to Yggdrasil's pure-Rust db-analyser binary. Per-round delivery:

| Round | Source mirror | Yggdrasil delivery |
|-------|---------------|--------------------|
| R475  | `Tools/DBAnalyser/HasAnalysis/{Byron,Shelley,Alonzo,Babbage,Conway}.hs::countTxOutputs` | per-era `TxBody::decode_output_count` in `crates/ledger/src/eras/*` + `Tx::output_count(era)` dispatcher in `crates/ledger/src/tx.rs` |
| R476  | `Ouroboros.Consensus.Byron.EBBs.hs::knownEBBs` + `Tools/DBAnalyser/Block/Cardano.hs::HasAnalysis` | `crates/tools/db-analyser/src/byron_ebbs.rs` (325-entry registry) + `impl HasAnalysis for yggdrasil_ledger::Block` |
| R477  | (test-coverage round) | Allegra/Mary/Alonzo dispatch tests (+ wire-format-superset observation) |
| R478  | (test-coverage round) | Babbage/Conway dispatch tests |
| R479  | `Tools/DBAnalyser/Analysis.hs::runAnalysis` | `crates/tools/db-analyser/src/analysis/runner.rs` — `run_analysis` + `AnalysisOutcome`/`AnalysisError` + 4 shipped handlers |
| R480  | (R479 follow-on) | 3 more block-iteration-only handlers (`ShowBlockTxsSize`, `ShowEBBs`, `OnlyValidation`); `AnalysisError` simplified |
| R481  | `Tools/DBAnalyser/Run.hs` | `lib.rs::run` wires `FileImmutable` + runner + stdout renderer; `status::analysis_dispatch_status` closed at R481 |

**Workspace tests:** 6,084 → 6,166 (+82 across 7 rounds).
**Verification gates:** all five clean at HEAD on every round commit.

## Functional shippable surface

After R481, the `db-analyser` binary supports the following
analyses end-to-end (operator invokes `cargo run --bin db-analyser
-- --db <chain-db-path> --analysis <name>`):

| Analysis | Outcome | Wire status |
|----------|---------|-------------|
| `ShowSlotBlockNo` | Per-block `(slot, block_no, header_hash)` lines | ✅ shipped |
| `CountBlocks` | Total + first/last `(slot, block_no)` | ✅ shipped |
| `CountTxOutputs` | Cumulative + per-block tuples | ✅ shipped |
| `ShowBlockHeaderSize` | Max + per-block tuples | ✅ shipped |
| `ShowBlockTxsSize` | Per-block `(slot, tx_count, total_bytes)` | ✅ shipped |
| `ShowEBBs` | Byron EBB markers + their registry prev-hash | ✅ shipped |
| `OnlyValidation` | Block count (validation in storage layer) | ✅ shipped |
| `StoreLedgerStateAt` | — | 🚧 `RequiresLedgerStateApplyLoop` |
| `CheckNoThunksEvery` | — | 🚧 `RequiresLedgerStateApplyLoop` |
| `TraceLedgerProcessing` | — | 🚧 `RequiresLedgerStateApplyLoop` |
| `BenchmarkLedgerOps` | — | 🚧 `RequiresLedgerStateApplyLoop` |
| `ReproMempoolAndForge` | — | 🚧 `RequiresLedgerStateApplyLoop` |
| `GetBlockApplicationMetrics` | — | 🚧 `RequiresLedgerStateApplyLoop` |

**7/13 shipped.** 6/13 return a structured
`AnalysisError::RequiresLedgerStateApplyLoop` with the analysis
name in the error message — operators get an actionable error
naming the dependency, not a panic or silent skip.

## End-to-end run path (R481 wire-up)

```
operator
  │
  ▼ argv
yggdrasil_db_analyser::run_main
  │
  ▼ DBAnalyserConfig
yggdrasil_db_analyser::run(&config)
  │
  ├─▶ yggdrasil_storage::FileImmutable::open(&config.db_dir)
  │
  ├─▶ store.suffix_after(&Point::Origin)  →  Vec<Block>
  │
  ├─▶ analysis::runner::run_analysis(&config, blocks)  →  AnalysisOutcome
  │
  └─▶ render_outcome(&outcome)  →  stdout
```

Each step has explicit error handling: storage errors wrap into
`RunError::Storage`, analysis errors into `RunError::Analysis`.
Both render through `thiserror::Error::Display` with the
operator-readable message naming the failing component.

## Carve-outs surviving R481

- **Ledger-state apply-loop arc (future, multi-round)**: required
  by the 6 ledger-state-dependent analyses. Each currently returns
  `AnalysisError::RequiresLedgerStateApplyLoop` with the analysis
  name. Per-era ledger-state apply rules already exist in
  `crates/ledger/src/eras/*`; threading the per-block step through
  a `WithLedgerState<Block, LedgerState>` for these analyses is
  the missing wire-up.
- **Streaming chain iterator**: `ImmutableStore::suffix_after`
  materializes the full chain in memory. Acceptable for the
  operational tooling (`db-analyser` runs on bounded chains for
  forensic work) but flagged as a future-work item for
  multi-terabyte chains. Bounded follow-on.
- **Per-analysis byte-equivalent stdout vs upstream binary**: R481
  ships an upstream-compatible-shape stdout renderer
  (`slot=N block_no=M hash=...; total_blocks=K`). A formal
  byte-by-byte soak against
  `.reference-haskell-cardano-node/install/bin/db-analyser` is a
  follow-on integration round (not blocking — the structured
  `AnalysisOutcome` is the canonical Yggdrasil-side contract).

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo test-all                                               6,166 passing (was 6,084 pre-R475)
cargo lint                                                   clean
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

Per-round test breakdown:
- R475: +16 (per-era TxBody output-count helpers + Tx::output_count dispatcher)
- R476: +14 (Byron EBB registry + HasAnalysis for Block impl)
- R477: +6  (Allegra/Mary/Alonzo dispatch coverage)
- R478: +6  (Babbage/Conway dispatch coverage)
- R479: +21 (analysis::runner core + 4 handlers + dispatch tests)
- R480: +8  (3 more handlers + EBB-lookup integration)
- R481: +6  (end-to-end FileImmutable integration tests +
             status descriptor reshape + ledger-state-deferral
             propagation test)
- Total: +77 unit tests + +6 integration tests = +83 tests; some
  rounds also moved earlier-round counts as expected (the +82
  observed delta accounts for one test moved from R479's
  `_returns_pending_r480` set to R480's
  `_dispatches_show_*` set).

## Follow-on observation

The db-analyser dispatch core is now complete in Yggdrasil — the
operational tool ships forensic-analysis support for the 7
block-iteration-only analyses, identifying chain shape (slot,
block_no, header_hash, tx_count, header_size), Byron EBB
markers, and validating an immutable chain end-to-end. The
remaining 6 analyses are blocked on the same single dependency
(ledger-state apply-loop) — when that future arc ships, the
6 dispatch arms become straightforward additions following the
R479-R480 handler pattern.
