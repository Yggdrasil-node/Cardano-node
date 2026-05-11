---
title: 'R493: ReproMempoolAndForge handler via yggdrasil-consensus Mempool'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-493-repro-mempool-and-forge-handler/
---

# R493 — `ReproMempoolAndForge` handler

**Date:** 2026-05-11
**Predecessor:** R492b (db-analyser AGENTS.md header post-R491).
**Scope:** single-round — ship the last remaining ledger-state-
dependent analysis. **Closes 13/13 dispatch coverage.**

## Slice scope

The yggdrasil-consensus `Mempool::with_capacity(n)` constructor
+ `MempoolEntry` struct + `insert`/`pop_best` API are all in
place. R493 wires them through a new handler that exercises the
mempool insert + drain hot path per block.

Wire-up:

1. Add `yggdrasil-consensus = { path = "../../consensus" }` to
   `crates/tools/db-analyser/Cargo.toml`.
2. Add `AnalysisOutcome::ReproMempoolAndForge { per_block_stats,
   applied_ok, applied_err }`.
3. Replace the dispatch arm at
   `AnalysisName::ReproMempoolAndForge(_)` with a call to the
   new `analysis_repro_mempool_and_forge` handler.
4. Implement the handler.

## Handler

```rust
pub fn analysis_repro_mempool_and_forge(blocks: &[Block]) -> AnalysisOutcome
```

For each block:

1. Apply via `state.apply_block(blk)` (tracks Ok/Err).
2. Fresh `Mempool::with_capacity(1024 * 1024)` (1 MiB matching
   upstream's `MempoolCapacityBytesOverride`).
3. Phase 1 (timed via `Instant`): insert each tx as a simplified
   `MempoolEntry` (`era`/`tx_id`/`body`/`size_bytes` real;
   `fee=0`/`raw_tx=body`/`ttl=u64::MAX`/`inputs=Vec::new()`).
4. Phase 2 (timed): drain via `pop_best()` until empty.
5. Emit `(slot, block_no, insert_count, forge_count, insert_ns,
   forge_ns)`.

Insert failures (capacity-exceeded, duplicate-tx-id) are
silently skipped — the per-block insert count reflects
successful inserts only.

## Forensic carve-outs (documented, not blocking dispatch)

- **Fee = 0 in every MempoolEntry:** the mempool's fee-priority
  ordering degenerates to insertion order. Upstream's mempool
  re-ranks by fee/byte during forge; ours is FIFO.
- **TTL = u64::MAX:** no TTL eviction. Upstream's mempool drops
  expired txs at apply boundary; ours never expires.
- **Empty inputs:** no conflict-detection. Upstream's mempool
  rejects double-spends; ours accepts every tx (the only
  rejection is duplicate-tx-id, an artifact of the synthetic
  `tx_id` being a hash of body).

Closing these would require:
- Per-era fee decoder (read tx body, extract fee field).
- Per-era ttl decoder.
- Per-era input-list decoder.

All three are bounded per-era helpers that fit a follow-on
round. Not blocking the dispatch matrix.

## Dispatch coverage matrix (post-R493)

| AnalysisName | Verdict | Round |
|--------------|---------|-------|
| `ShowSlotBlockNo` | ✅ shipped | R479 |
| `CountBlocks` | ✅ shipped | R479 |
| `CountTxOutputs` | ✅ shipped | R479 |
| `ShowBlockHeaderSize` | ✅ shipped | R479 |
| `ShowBlockTxsSize` | ✅ shipped | R480 |
| `ShowEBBs` | ✅ shipped | R480 |
| `OnlyValidation` | ✅ shipped | R480 |
| `TraceLedgerProcessing` | ✅ shipped | R488 |
| `BenchmarkLedgerOps` | ✅ shipped | R489 |
| `GetBlockApplicationMetrics` | ✅ shipped | R490 |
| `StoreLedgerStateAt` | ✅ shipped | R491 |
| **`ReproMempoolAndForge`** | **✅ shipped (yggdrasil-consensus Mempool seam)** | **R493** |
| `CheckNoThunksEvery` | ⛔ `NotApplicableToRust` | R485 (permanent) |

**Coverage: 12/13 shipped + 1/13 permanent carve-out = 13/13
final verdicts.** **Zero remaining
`RequiresLedgerStateApplyLoop` deferrals.** Dispatch matrix
fully covered.

## Tests delivered (+6 cases, 2 reshaped)

New:
- `analysis_repro_mempool_and_forge_empty_chain`
- `analysis_repro_mempool_and_forge_block_with_no_txs_yields_zero_counts`
- `analysis_repro_mempool_and_forge_with_synthetic_txs_round_trips`
  (3 distinct txs → 3 inserts + 3 forges)
- `analysis_repro_mempool_and_forge_skips_duplicate_tx_ids`
  (2 txs with same body → 1 insert; duplicate skipped)
- `run_analysis_dispatches_repro_mempool_and_forge`
- `run_analysis_dispatch_matrix_no_longer_returns_apply_loop_errors`
  (pins 13/13 dispatch coverage — iterates 12 AnalysisName
  variants asserting `is_ok()`, plus `CheckNoThunksEvery`
  asserting `NotApplicableToRust`).

Reshaped:
- `end_to_end_lib_run_propagates_check_no_thunks_carve_out`
  (was `_propagates_ledger_state_deferral`; switched from
  ReproMempoolAndForge → CheckNoThunksEvery as the only error
  variant operators can hit post-R493).
- `run_analysis_repro_mempool_returns_requires_apply_loop`
  replaced by the dispatch-matrix-coverage assertion test.

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,196 → 6,201
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Documentation cascade

- `status::analysis_dispatch_status`: `status` field
  `11-of-13-shipped` → `13-of-13-final-verdicts`;
  `deferred_round` `R491 → R493`. `depends_on` field reshaped
  to "nothing — the dispatch matrix is fully covered after
  R493."
- `AnalysisError::RequiresLedgerStateApplyLoop` docstring:
  "1 of 13" → "0 of 13" — the variant is retained for future
  surface-introductions but currently unreachable.
- `AGENTS.md` Status field: `(post-R491 dispatch-coverage
  closure)` → `(post-R493 dispatch-coverage matrix complete)`;
  carve-out inventory replaces "1 ledger-state-dependent" entry
  with "0 ledger-state-dependent (post-R493)" + a new "Forensic-
  fidelity hardening" entry.
- `AGENTS.md` dispatch matrix: `ReproMempoolAndForge` row
  flipped to ✅.

## Stop point

**The db-analyser dispatch matrix is complete.** Every
`AnalysisName` variant either ships a real handler (12/13) or
is a permanent carve-out (1/13). Future work is forensic-
fidelity hardening (richer `MempoolEntry` construction, richer
`emit_traces`, stdout byte-equivalent soak), not dispatch-
coverage gaps.

The R475-R493 sequence delivered 11 new shipped handlers + 1
permanent carve-out across 23 rounds, taking the db-analyser
binary from "R481-style stub + 7 block-iteration-only
handlers" to "12 shipped handlers + 1 documented carve-out =
13/13 final verdicts."
