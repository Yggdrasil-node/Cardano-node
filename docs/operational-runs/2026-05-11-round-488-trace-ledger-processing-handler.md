---
title: 'R488: TraceLedgerProcessing handler via LedgerState::apply_block'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-488-trace-ledger-processing-handler/
---

# R488 — `TraceLedgerProcessing` handler

**Date:** 2026-05-11
**Predecessor:** R487/R487b (cardano-tracer + network AGENTS.md refresh).
**Scope:** single-round — ship the first of the 5 ledger-state-
dependent analyses via the existing
`yggdrasil_ledger::LedgerState::apply_block` seam.

## Slice scope

R475-R481 deferred all 6 ledger-state-dependent dispatch arms;
R485 carved out `CheckNoThunksEvery` as permanent (Haskell-only
laziness). R488 lands `TraceLedgerProcessing` as the first
shipped of the remaining 5. The handler:

1. Bootstraps `LedgerState::new(initial_era)` (initial era =
   first block's era, defaulting to Byron for empty input).
2. Iterates blocks; calls `state.apply_block(&block)` per block.
3. Captures per-block `Result<(), LedgerError>` as a string;
   apply failures don't abort the run.
4. Emits `AnalysisOutcome::TraceLedgerProcessing { traces,
   applied_ok, applied_err }`.

### Forensic semantics (Yggdrasil-side, not upstream byte-equivalent)

Upstream `traceLedgerProcessing` calls
`HasAnalysis::emit_traces(WithLedgerState{blk, state_before,
state_after})` per block — returning ledger-state-derived trace
strings (epoch boundary, stake delta, etc.). Yggdrasil's
`Block::emit_traces` is the R476 placeholder returning empty
Vec; we don't have the genesis-bootstrap-derived state needed to
produce upstream-equivalent trace content yet.

R488 instead surfaces the *apply outcome* per block as the
trace. The operator sees:
```
slot=N block_no=M apply=ok
slot=N+1 block_no=M+1 apply=err reason=<LedgerError Display>
trace_ledger_processing applied_ok=K applied_err=L
```

This is operationally useful for forensic chain audits without
requiring genesis-bootstrap CLI flags. Closing the trace-content
gap (producing upstream-equivalent epoch/stake/delta strings) is
a follow-on once genesis-bootstrap + protocol-params hydration
ship.

## New `AnalysisOutcome` variant

```rust
TraceLedgerProcessing {
    traces: Vec<(SlotNo, BlockNo, Result<(), String>)>,
    applied_ok: i64,
    applied_err: i64,
}
```

`traces` carries per-block outcomes in chain order;
`applied_ok + applied_err = traces.len()` invariant holds.

## Dispatch coverage matrix (post-R488)

| AnalysisName | Verdict | Round |
|--------------|---------|-------|
| `ShowSlotBlockNo` | ✅ shipped | R479 |
| `CountBlocks` | ✅ shipped | R479 |
| `CountTxOutputs` | ✅ shipped | R479 |
| `ShowBlockHeaderSize` | ✅ shipped | R479 |
| `ShowBlockTxsSize` | ✅ shipped | R480 |
| `ShowEBBs` | ✅ shipped | R480 |
| `OnlyValidation` | ✅ shipped | R480 |
| **`TraceLedgerProcessing`** | **✅ shipped (forensic Ok/Err trace)** | **R488** |
| `CheckNoThunksEvery` | ⛔ `NotApplicableToRust` | R485 (permanent) |
| `StoreLedgerStateAt` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| `BenchmarkLedgerOps` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| `ReproMempoolAndForge` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| `GetBlockApplicationMetrics` | 🚧 `RequiresLedgerStateApplyLoop` | future |

**Coverage: 8/13 shipped + 1/13 permanent carve-out = 9/13 final
verdicts.** 4/13 deferred to a future genesis-bootstrap arc.

## Tests delivered (+4 cases)

- `analysis_trace_ledger_processing_empty_chain`
- `analysis_trace_ledger_processing_byron_block_empty_state_succeeds`
- `analysis_trace_ledger_processing_per_block_trace_shape`
- `run_analysis_dispatches_trace_ledger_processing`

Plus a fix to the existing R481 integration test
`end_to_end_lib_run_propagates_ledger_state_deferral` — switched
from `AnalysisName::TraceLedgerProcessing` (now shipped) to
`AnalysisName::BenchmarkLedgerOps` (still deferred).

## Documentation cascade

- `crates/tools/db-analyser/src/status.rs::analysis_dispatch_status`:
  `status` field `block-only-shipped` → `8-of-13-shipped`;
  `deferred_round` advances `R485 → R488`; `depends_on` calls
  out the R488 shipping separately from the 4 remaining
  apply-loop deferrals.
- `crates/tools/db-analyser/AGENTS.md` dispatch-coverage matrix:
  `TraceLedgerProcessing` row flipped to ✅; carve-out inventory
  gets a new row about the trace-content gap (operator sees Ok/Err
  outcomes; upstream emits epoch/stake/delta strings).
- `AnalysisError` enum docstring: 5 → 4 ledger-state-dependent
  routes.

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,181 → 6,185
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Stop point

`TraceLedgerProcessing` ships as the 8th analysis. The R488
forensic Ok/Err outcome shape is operationally useful but not
byte-equivalent to upstream's epoch/stake/delta trace strings —
the gap is documented and closed by a future genesis-bootstrap
arc that ships the genesis CLI flags + `Block::emit_traces`
expansion.
