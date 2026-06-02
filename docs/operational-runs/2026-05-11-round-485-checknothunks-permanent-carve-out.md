---
title: 'R485: CheckNoThunksEvery permanent carve-out (Rust has no thunks)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-485-checknothunks-permanent-carve-out/
---

# R485 — `CheckNoThunksEvery` permanent carve-out

**Date:** 2026-05-11
**Predecessor:** R484 (db-truncater AGENTS.md refresh).
**Scope:** single-round bounded — reclassify `CheckNoThunksEvery`
from "deferred pending future apply-loop arc" to "fundamentally
not portable to Rust".

## Slice scope

Upstream `Cardano.Tools.DBAnalyser.Analysis.checkNoThunks` uses
`NoThunks.unsafeNoThunks` to walk GHC's lazy heap for unevaluated
thunks. This is a **Haskell-specific GHC laziness concept**: it
inspects the runtime representation of values to detect spots where
the lazy-evaluation strategy hasn't been forced. Rust is **eagerly
evaluated** at the value level — there are no runtime thunks to
inspect, no lazy fields, no `WHNF` discipline.

Implementing `CheckNoThunksEvery` in Rust is therefore not
"pending a future arc"; it is **permanently not portable**. The
R475-R481 arc lumped this analysis with the 5 other
ledger-state-dependent analyses; R485 splits it off into its own
classification.

## New `AnalysisError` variant

```rust
pub enum AnalysisError {
    RequiresLedgerStateApplyLoop { analysis_name: String },
    NotApplicableToRust {
        analysis_name: String,
        reason: String,
    },
}
```

The `NotApplicableToRust` variant carries:
- `analysis_name`: the upstream `AnalysisName` rendered as a string
  (currently always `"CheckNoThunksEvery"`).
- `reason`: a human-readable explanation pinpointing the upstream
  Haskell-only feature this analysis depends on.

The `thiserror::Error::Display` impl renders:
```
yggdrasil-db-analyser: analysis 'CheckNoThunksEvery' is fundamentally
not portable to Rust (laziness/thunks are a Haskell-specific GHC
concept; Rust is eagerly evaluated). This is a permanent carve-out —
see status::analysis_dispatch_status.
```

## Dispatch coverage matrix (post-R485)

| AnalysisName | Verdict | Round |
|--------------|---------|-------|
| `ShowSlotBlockNo` | ✅ shipped | R479 |
| `CountBlocks` | ✅ shipped | R479 |
| `CountTxOutputs` | ✅ shipped | R479 |
| `ShowBlockHeaderSize` | ✅ shipped | R479 |
| `ShowBlockTxsSize` | ✅ shipped | R480 |
| `ShowEBBs` | ✅ shipped | R480 |
| `OnlyValidation` | ✅ shipped | R480 |
| `StoreLedgerStateAt` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| **`CheckNoThunksEvery`** | **⛔ `NotApplicableToRust`** | **R485 (permanent)** |
| `TraceLedgerProcessing` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| `BenchmarkLedgerOps` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| `ReproMempoolAndForge` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| `GetBlockApplicationMetrics` | 🚧 `RequiresLedgerStateApplyLoop` | future |

**Coverage: 7/13 shipped + 1/13 permanent carve-out = 8/13 final
verdicts. 5/13 await a future ledger-state apply-loop arc.**

## Tests delivered (+2 cases)

- `run_analysis_check_no_thunks_returns_not_applicable_to_rust`:
  pins the `CheckNoThunksEvery(100)` dispatch routing to the new
  `NotApplicableToRust` variant + asserts the reason field mentions
  thunks or laziness.
- `analysis_error_not_applicable_to_rust_renders_helpful_message`:
  pins the `thiserror::Display` output format (asserts the message
  contains the analysis name + "not portable to Rust" + "permanent
  carve-out").

The existing `run_analysis_benchmark_ledger_ops_returns_requires_apply_loop`
test was refactored from an irrefutable-pattern `let` to an
exhaustive `match` to cover both error variants without warnings.

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,176 → 6,178
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Documentation cascade

- `crates/tools/db-analyser/src/status.rs::analysis_dispatch_status`:
  reshaped `depends_on` to call out the R485 carve-out separately
  from the 5-analysis apply-loop dependency; `deferred_round`
  advances `R481 → R485`.
- `crates/tools/db-analyser/AGENTS.md` dispatch-coverage matrix:
  `CheckNoThunksEvery` row changes verdict from
  `🚧 RequiresLedgerStateApplyLoop` to `⛔ NotApplicableToRust
  (R485 permanent carve-out)`. Carve-out inventory grows a new
  `CheckNoThunksEvery (permanent)` row pointing at the upstream
  `checkNoThunks`/`unsafeNoThunks` Haskell-only call chain.
- `crates/tools/db-analyser/src/analysis/runner.rs` module
  docstring + `AnalysisError` enum docstring updated to reflect
  the 5-analysis (down from 6) `RequiresLedgerStateApplyLoop` set.

## Stop point

`CheckNoThunksEvery` is now classified as a permanent carve-out
with a clear operator-readable error message. The dispatch-
coverage matrix has 8/13 final verdicts (7 shipped + 1 permanent
carve-out); only 5 remaining variants await the future ledger-
state apply-loop arc.
