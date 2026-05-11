---
title: 'R491: StoreLedgerStateAt handler via LedgerStateCheckpoint CBOR codec'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-491-store-ledger-state-at-handler/
---

# R491 — `StoreLedgerStateAt` handler

**Date:** 2026-05-11
**Predecessor:** R490 (`GetBlockApplicationMetrics`).
**Scope:** single-round — ship the 4th of the 5 ledger-state-
dependent analyses by reusing the existing R269-shipped
`LedgerStateCheckpoint` CBOR codec.

## Slice scope

The `LedgerStateCheckpoint` type at
`crates/ledger/src/state/checkpoint.rs` already has
`CborEncode`/`CborDecode` impls (shipped at R269 — the
checkpoint/recovery surface for the consensus layer). R491
reuses that codec via the handler:

1. Walk blocks via `LedgerState::apply_block`.
2. At the first block whose `slot_no >= target_slot`: capture
   `state.checkpoint().to_cbor_bytes()`.
3. Continue applying remaining blocks for honest
   `applied_ok`/`applied_err` totals.
4. Return `AnalysisOutcome::StoreLedgerStateAt { target_slot,
   reached_slot, snapshot_bytes, applied_ok, applied_err }`.

**No new codec work.** R491 ships exclusively as a handler +
dispatch wire-up.

## New `AnalysisOutcome` variant

```rust
StoreLedgerStateAt {
    target_slot: SlotNo,
    reached_slot: Option<SlotNo>, // None if walk too short
    snapshot_bytes: Vec<u8>,       // CBOR-encoded LedgerStateCheckpoint
    applied_ok: i64,
    applied_err: i64,
}
```

## Stdout rendering

```
store_ledger_state_at target_slot=20 reached_slot=20 snapshot_bytes=NNN applied_ok=K applied_err=L
```

Or when target slot is never reached:
```
store_ledger_state_at target_slot=9999 reached_slot=<not_reached> snapshot_bytes=0 applied_ok=K applied_err=L
```

The `snapshot_bytes=NNN` count is the byte length of the
CBOR-encoded `LedgerStateCheckpoint`. A downstream operator can
write the bytes to disk via the structured `AnalysisOutcome`
return rather than relying on stdout (stdout only reports
length).

## Dispatch coverage matrix (post-R491)

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
| **`StoreLedgerStateAt`** | **✅ shipped (LedgerStateCheckpoint CBOR)** | **R491** |
| `CheckNoThunksEvery` | ⛔ `NotApplicableToRust` | R485 (permanent) |
| `ReproMempoolAndForge` | 🚧 `RequiresLedgerStateApplyLoop` | future |

**Coverage: 11/13 shipped + 1/13 permanent carve-out = 12/13
final verdicts.** Only 1/13 still deferred:
- `ReproMempoolAndForge`: needs a mempool+forge integration
  (multi-round commitment).

## Tests delivered (+5 cases)

- `analysis_store_ledger_state_at_empty_chain_returns_none`
- `analysis_store_ledger_state_at_target_too_high_returns_none`
- `analysis_store_ledger_state_at_captures_snapshot_at_target`
- `analysis_store_ledger_state_at_snapshot_round_trips_via_checkpoint_codec`
  (verifies `LedgerStateCheckpoint::from_cbor_bytes(&snapshot_bytes)`
  decodes the captured snapshot back successfully)
- `run_analysis_dispatches_store_ledger_state_at`

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,191 → 6,196
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Documentation cascade

- `status::analysis_dispatch_status`: `status` field
  `10-of-13-shipped` → `11-of-13-shipped`; `deferred_round`
  `R490 → R491`. `depends_on` now reads "yggdrasil's
  mempool+forge integration" (since that's the only remaining
  blocker).
- `AGENTS.md` dispatch matrix: `StoreLedgerStateAt` row flipped
  to ✅; carve-out inventory "2 ledger-state-dependent
  analyses" → "1 ledger-state-dependent analysis".
- `AnalysisError` enum docstring: 2 → 1 ledger-state-dependent
  route.

## Stop point

After R491, only `ReproMempoolAndForge` remains deferred. It
genuinely needs a mempool+forge integration which is a
multi-round commitment of its own. The R475-R491 sequence has
reduced the deferred-analysis count from 6 (post-R481) to 1
(post-R491). 12 of 13 final verdicts. Dispatch shape complete.
