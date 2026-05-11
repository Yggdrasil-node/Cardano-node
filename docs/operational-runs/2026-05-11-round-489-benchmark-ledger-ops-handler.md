---
title: 'R489: BenchmarkLedgerOps handler with apply-timing instrumentation'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-489-benchmark-ledger-ops-handler/
---

# R489 — `BenchmarkLedgerOps` handler

**Date:** 2026-05-11
**Predecessor:** R488 (`TraceLedgerProcessing` via apply-loop).
**Scope:** single-round — ship the 2nd of the 5 ledger-state-
dependent analyses, this one with `std::time::Instant` timing
instrumentation around `LedgerState::apply_block` plus
`SlotDataPoint` population from the R374-R376 leaf records.

## Slice scope

R485 carved out `CheckNoThunksEvery` permanently. R488 shipped
`TraceLedgerProcessing` (forensic Ok/Err trace). R489 ships
`BenchmarkLedgerOps` via the same apply-loop seam, plus:

1. `std::time::Instant::now()` measurement around each
   `state.apply_block(&block)` call.
2. Per-block `SlotDataPoint` (R374 record) population with the
   portable-subset of timing fields.
3. New `AnalysisOutcome::BenchmarkLedgerOps { slot_data_points,
   applied_ok, applied_err }` variant.

## Portable-subset filling of `SlotDataPoint`

The upstream `SlotDataPoint` has 15 fields covering wall-clock
+ GHC GC + allocation timings. Yggdrasil populates the
6 portable fields with real values and zero-fills the 9 GHC-
specific fields. Honest zeros are more useful than synthesized
GHC-side analogs:

| Field | Yggdrasil source | Status |
|-------|------------------|--------|
| `slot` | `blk.header.slot_no` | ✅ real |
| `slot_gap` | `slot - prev_slot` (0 for first block) | ✅ real |
| `total_time` | `Instant::elapsed().as_nanos() as i64` | ✅ real |
| `mut_block_apply` | mirrors `total_time` (no phase-breakdown) | ✅ real (= total) |
| `block_byte_size` | `Block::raw_cbor.as_ref().map(\|b\| b.len())` | ✅ real |
| `block_stats` | `Block::block_stats()` (R476 impl) | ✅ real |
| `mut_` (mutator non-GC time) | zero | 🚧 GHC-specific |
| `gc` (GC time) | zero | 🚧 GHC-specific |
| `maj_gc_count` | zero | 🚧 GHC-specific |
| `min_gc_count` | zero | 🚧 GHC-specific |
| `allocated_bytes` | zero | 🚧 GHC-specific |
| `mut_forecast` | zero | 🚧 phase-breakdown gap |
| `mut_header_tick` | zero | 🚧 phase-breakdown gap |
| `mut_header_apply` | zero | 🚧 phase-breakdown gap |
| `mut_block_tick` | zero | 🚧 phase-breakdown gap |

## Forensic semantics (R488 inherited)

Apply failures don't abort the run. The timing of failed applies
is still captured. `applied_ok + applied_err == slot_data_points.len()`
invariant holds.

## Dispatch coverage matrix (post-R489)

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
| **`BenchmarkLedgerOps`** | **✅ shipped (Instant timing → SlotDataPoint)** | **R489** |
| `CheckNoThunksEvery` | ⛔ `NotApplicableToRust` | R485 (permanent) |
| `StoreLedgerStateAt` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| `ReproMempoolAndForge` | 🚧 `RequiresLedgerStateApplyLoop` | future |
| `GetBlockApplicationMetrics` | 🚧 `RequiresLedgerStateApplyLoop` | future |

**Coverage: 9/13 shipped + 1/13 permanent carve-out = 10/13 final
verdicts.** 3/13 deferred to future arcs.

## Tests delivered (+4 cases)

- `run_analysis_benchmark_ledger_ops_returns_outcome` (was
  `_returns_requires_apply_loop`; now asserts the shipped Outcome).
- `analysis_benchmark_ledger_ops_empty_chain`
- `analysis_benchmark_ledger_ops_records_per_block_timing`
- `analysis_benchmark_ledger_ops_emits_slot_gap_between_blocks`

The R481 integration test
`end_to_end_lib_run_propagates_ledger_state_deferral` switched
from `BenchmarkLedgerOps` (now shipped) to `ReproMempoolAndForge`
(still deferred).

## Documentation cascade

- `status::analysis_dispatch_status`: `status` field
  `8-of-13-shipped` → `9-of-13-shipped`; `deferred_round`
  `R488 → R489`.
- `AGENTS.md` dispatch matrix: `BenchmarkLedgerOps` row flipped
  to ✅; carve-out inventory entry adjusted from "4 ledger-state-
  dependent analyses" → "3 ledger-state-dependent analyses".
- `AnalysisError` enum docstring: 4 → 3 ledger-state-dependent
  routes.

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,185 → 6,188
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Stop point

`BenchmarkLedgerOps` ships as the 9th analysis with real
`Instant`-based wall-clock timing per block. The
`SlotDataPoint` records carry the portable subset; the
GHC-specific timing breakdown stays zero-filled. The remaining
3 deferred analyses each need a focused mini-arc:
- `StoreLedgerStateAt`: a LedgerState snapshot codec.
- `ReproMempoolAndForge`: mempool + forge integration.
- `GetBlockApplicationMetrics`: richer ledger-state-delta CSV.
