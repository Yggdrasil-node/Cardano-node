---
title: 'R503: Wire SelectDB::SelectImmutableDB(At(slot)) start-point through lib.rs::run'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-12-round-503-select-db-start-point-wire-up/
---

# R503 — `config.select_db` wire-up for `At(slot)` start point

**Date:** 2026-05-12
**Predecessor:** R502 (verbose flag wire-up).
**Scope:** functional wire-up + tests.

## Slice scope

R351 added `select_db: SelectDB` to `DBAnalyserConfig`. The
parser maps `--start-from-slot N` to
`SelectDB::SelectImmutableDB(WithOrigin::At(SlotNo(N)))`. But
the field was parsed-but-ignored: R481's `lib.rs::run` always
called `store.iter_after(&Point::Origin)` regardless of
`config.select_db`.

R503 honors the field. New `lib.rs::run` body:

```rust
let raw_iter = store.iter_after(&Point::Origin).map_err(...)?;
let blocks: Box<dyn Iterator<Item = Block>> = match config.select_db {
    SelectDB::SelectImmutableDB(WithOrigin::Origin) => raw_iter,
    SelectDB::SelectImmutableDB(WithOrigin::At(target_slot)) => {
        let target = target_slot.0;
        Box::new(raw_iter.skip_while(move |b| b.header.slot_no.0 < target))
    }
};
```

The skip is a runner-side filter because the storage layer's
`iter_after` requires a full `Point` (slot + hash), not a
slot-only starting point. Future optimization could plumb the
slot through to `FileImmutable` for streaming-from-slot, but
current operational chain sizes don't need it.

## Tests delivered (+3 cases)

- `end_to_end_lib_run_respects_select_db_origin` (3-block
  chain; `WithOrigin::Origin` → all 3 counted).
- `end_to_end_lib_run_respects_select_db_at_slot` (3-block
  chain at slots 10/20/30; `WithOrigin::At(SlotNo(20))` → 2
  counted, first=(slot=20, block_no=2), last=(slot=30,
  block_no=3)).
- `end_to_end_lib_run_select_db_at_slot_past_tip_yields_empty`
  (1-block chain at slot 10; `WithOrigin::At(SlotNo(9999))` →
  empty suffix → 0-count outcome).

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,233 → 6,236
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Stop point — config-field wire-up audit

After R502 + R503, the previously parsed-but-ignored config
fields are now wired:

| Field | Wire-up status |
|-------|----------------|
| `db_dir` | ✅ used in R481 (`FileImmutable::open`) |
| `verbose` | ✅ wired R502 (renderer skips per-block on false) |
| `select_db` | ✅ wired R503 (runner skip_while when `At(slot)`) |
| `validation: Option<ValidateBlocks>` | ❌ carve-out — FileImmutable has no per-block strict/minimal modes |
| `analysis` | ✅ used in R479 (dispatch core) |
| `conf_limit` | ✅ used in R479 (`apply_limit`) |
| `ldb_backend: LedgerDBBackend` | ❌ carve-out — Yggdrasil has single ledger-DB format |

2 fields remain as documented carve-outs (`validation`,
`ldb_backend`). Both would need storage-layer / ledger-DB
redesign to be honored — multi-arc commitments, not bounded
loop rounds.
