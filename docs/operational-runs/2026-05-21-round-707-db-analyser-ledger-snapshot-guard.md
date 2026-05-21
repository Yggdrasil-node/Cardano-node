---
title: "Round 707 db-analyser ledger-snapshot consistency guard (A3 R3c-6 slice 3)"
parent: Reference
---

# Round 707 db-analyser ledger-snapshot consistency guard (A3 R3c-6 slice 3)

Date: 2026-05-21

## Scope

Third slice of A3 R3c-6: `db-analyser` now consumes the
persisted `LedgerStore` snapshot — as a structural consistency
guard.

## What shipped

`db-analyser::run`, when the ChainDb carries a `<db>/ledger/`
snapshot directory, loads the latest snapshot and checks its
slot is not ahead of the immutable-chain tip. A snapshot slot
past the last persisted block means a corrupt / inconsistent
ChainDb — `run` returns a structured
`RunError::LedgerSnapshotAheadOfChain { snapshot, tip }`.

The `snapshot_slot <= tip_slot` invariant holds for both
synthesizer ChainDbs (one snapshot at the final tip) and
node ChainDbs (periodic snapshots behind the tip) — unlike a
`snapshot_slot == tip_slot` check, which would falsely reject a
node ChainDb.

## Scope honesty — what did NOT ship

This is a **structural** guard. It does **not** replay the
chain against the snapshot. The original R3c-6 goal — "so
`db-analyser` can validate the synthesized chain" in the sense
of a real *ledger* validation (replay all blocks from genesis,
confirm the resulting ledger state matches the persisted
snapshot) — is **blocked on the genesis-bootstrap arc deferred
at R488**:

- `db-analyser`'s ledger-applying analyses still bootstrap an
  empty `LedgerState::new()` rather than a genesis-seeded
  state, so real blocks fail at apply time;
- the synthesizer writes its snapshot at the *final* tip (a
  post-application checkpoint), which cannot serve as an
  apply-loop *starting* state.

Closing the full validation needs CLI genesis-bootstrap flags +
protocol-params hydration — a separate arc.

## Changes

- `db-analyser/src/lib.rs` — `run` opens
  `FileLedgerStore::open_read_only(<db>/ledger)` when the
  directory exists, and rejects a snapshot ahead of the
  immutable tip. New `RunError::LedgerSnapshotAheadOfChain`
  variant.

2 new end-to-end tests:
- `end_to_end_lib_run_accepts_consistent_ledger_snapshot` —
  snapshot at the immutable tip passes.
- `end_to_end_lib_run_rejects_ledger_snapshot_ahead_of_chain` —
  snapshot past the tip is rejected with the structured error.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-db-analyser` (193 lib + 20 end-to-end
  + 2 golden — all green; +2 new tests vs the R706 baseline of
  18 end-to-end)

## Remaining (A3 R3c-6)

- Full ledger validation (replay-from-genesis vs. snapshot) —
  blocked on the R488 genesis-bootstrap arc.
