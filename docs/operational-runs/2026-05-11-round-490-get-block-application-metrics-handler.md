---
title: 'R490: GetBlockApplicationMetrics handler via R476 column closures'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-490-get-block-application-metrics-handler/
---

# R490 — `GetBlockApplicationMetrics` handler

**Date:** 2026-05-11
**Predecessor:** R489 (`BenchmarkLedgerOps` via Instant timing).
**Scope:** single-round — ship the 3rd of the 5 ledger-state-
dependent analyses by leveraging R476's
`HasAnalysis::block_application_metrics()` column closures.

## Slice scope

The R476 `Block::block_application_metrics()` impl returns 4
column closures: `slot`, `block_no`, `era`, `tx_count`. All are
block-derived (no ledger-state read), so they can ship without
genesis bootstrap. R490 wires them through a handler with
`every_n_blocks` sampling.

## Handler

```rust
pub fn analysis_get_block_application_metrics(
    blocks: &[Block],
    every_n_blocks: u64,
) -> AnalysisOutcome
```

Procedure:
1. Bootstrap `LedgerState::new(initial_era)` (mirrors R488/R489
   apply-loop semantics).
2. For each block:
   - Call `state.apply_block(blk)`, track Ok/Err counter.
   - If `idx.is_multiple_of(every_n_blocks)`: invoke each
     `block_application_metrics()` closure via a
     `WithLedgerState::new(blk.clone(), CardanoLedgerStateValues,
     CardanoLedgerStateValues)`, collect `(column_name,
     column_value)` per closure.
3. Return `AnalysisOutcome::GetBlockApplicationMetrics { rows,
   every_n_blocks, applied_ok, applied_err }`.

`every_n_blocks=1` → row per block. `every_n_blocks=N` → only
rows where `idx % N == 0` (matches upstream's
`NumberOfBlocks(N)` cadence).

## New `AnalysisOutcome` variant

```rust
GetBlockApplicationMetrics {
    rows: Vec<Vec<(String, String)>>,
    every_n_blocks: u64,
    applied_ok: i64,
    applied_err: i64,
}
```

Each `row` is `Vec<(column_name, column_value)>` matching the
4-column shape from R476's `block_application_metrics`.

## Stdout rendering

```
slot=N block_no=M era=E tx_count=K
slot=N+1 block_no=M+1 era=E tx_count=K
...
get_block_application_metrics every_n_blocks=1 applied_ok=K applied_err=L
```

Per-row format mirrors upstream's space-separated `name=value`
shape from `block_application_metrics` CSV mode.

## Dispatch coverage matrix (post-R490)

| AnalysisName | Verdict | Round |
|--------------|---------|-------|
| `ShowSlotBlockNo` | ✅ shipped | R479 |
| `CountBlocks` | ✅ shipped | R479 |
| `CountTxOutputs` | ✅ shipped | R479 |
| `ShowBlockHeaderSize` | ✅ shipped | R479 |
| `ShowBlockTxsSize` | ✅ shipped | R480 |
| `ShowEBBs` | ✅ shipped | R480 |
| `OnlyValidation` | ✅ shipped | R480 |
| `TraceLedgerProcessing` | ✅ shipped (forensic Ok/Err trace) | R488 |
| `BenchmarkLedgerOps` | ✅ shipped (Instant timing → SlotDataPoint) | R489 |
| **`GetBlockApplicationMetrics`** | **✅ shipped (R476 column closures)** | **R490** |
| `CheckNoThunksEvery` | ⛔ `NotApplicableToRust` | R485 (permanent) |
| `StoreLedgerStateAt` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| `ReproMempoolAndForge` | 🚧 `RequiresLedgerStateApplyLoop` | future |

**Coverage: 10/13 shipped + 1/13 permanent carve-out = 11/13 final
verdicts.** Only 2/13 still deferred:
- `StoreLedgerStateAt`: needs a LedgerState snapshot CBOR codec
  (multi-round).
- `ReproMempoolAndForge`: needs a mempool+forge integration
  (multi-round).

## Tests delivered (+3 cases)

- `run_analysis_get_block_application_metrics_returns_outcome`
  (was `_returns_requires_apply_loop`; now asserts shipped
  Outcome).
- `analysis_get_block_application_metrics_empty_chain`
- `analysis_get_block_application_metrics_every_block`
  (every_n_blocks=1, validates per-block row shape with the 4
  R476 columns).
- `analysis_get_block_application_metrics_samples_every_n`
  (every_n_blocks=3 on 10 blocks → rows at indices 0/3/6/9).

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean (after .is_multiple_of() fix)
cargo test-all                                               6,188 → 6,191
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

Clippy `manual-is-multiple-of` lint required switching from
`% != 0` to `!is_multiple_of()`.

## Documentation cascade

- `status::analysis_dispatch_status`: `status` field
  `9-of-13-shipped` → `10-of-13-shipped`; `deferred_round`
  `R489 → R490`.
- `AGENTS.md` dispatch matrix: `GetBlockApplicationMetrics` row
  flipped to ✅; carve-out inventory "3 ledger-state-dependent
  analyses" → "2 ledger-state-dependent analyses".
- `AnalysisError` enum docstring: 3 → 2 ledger-state-dependent
  routes.

## Stop point

`GetBlockApplicationMetrics` ships as the 10th analysis. Only 2
of the 13 upstream variants remain deferred — both require
substantial follow-on commitments (snapshot codec / mempool
integration). The R475-R490 sequence has reduced the deferred-
analysis count from 6 (post-R481) to 2.
