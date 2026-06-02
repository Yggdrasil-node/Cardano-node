---
title: 'R347: storage — ImmutableStore::trim_after_slot extension (db-truncater unblock)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-10-round-347-storage-trim-after-slot/
---

# Round 347 — ImmutableStore::trim_after_slot extension

**Date:** 2026-05-10
**Branch:** `main`
**Predecessor:** [`R346`](#) (closure-status refresh; cardano-submit-api Phase A.2 closure pending operator soak)
**Plan:** Sister-Tools Pure-Rust Port (R326–R459), Phase Prep for Phase B.1 (db-truncater).

## Summary

R347 extends the `ImmutableStore` trait with a `trim_after_slot`
primitive — the inverse of the existing `trim_before_slot` GC method.
This unblocks Phase B.1 (db-truncater) by providing the storage-level
truncation primitive that `db-truncater`'s `Run.hs`-equivalent
implementation needs at R388+ per the plan.

**Why now.** The plan's R386-R390 db-truncater mini-arc skeleton is
already shipped. The implementation rounds (R388 Types + Parsers,
R389 Run.hs equivalent) need a `trim_after_slot` method on the
storage layer. Adding the primitive in a focused round (rather than
inline at R389) keeps the storage trait change reviewable
independently from the `db-truncater` binary plumbing.

## API surface

Trait method on `ImmutableStore`:

```rust
/// Removes all immutable blocks with slots strictly **after** `slot`.
///
/// Returns the number of blocks removed. Blocks at `slot` or earlier
/// are retained. Inverse of `trim_before_slot`.
fn trim_after_slot(&mut self, slot: SlotNo) -> Result<usize, StorageError>;
```

Implementations on the two existing backends:

- `InMemoryImmutable::trim_after_slot` — `self.blocks.retain(|b| b.header.slot_no <= slot)`.
- `FileImmutable::trim_after_slot` — full crash-safe variant with
  mark-dirty / file-delete / mark-clean ceremony, mirroring the
  existing `trim_before_slot` pattern; deletes both CBOR and legacy-
  JSON block files for each removed block.

ChainDb wrapper:

```rust
/// Truncates the immutable chain to slots <= `slot`, removing all
/// blocks with strictly larger slot numbers.
///
/// Storage primitive used by the `db-truncater` operator tool to
/// rewind a ChainDB to an earlier point.
pub fn truncate_immutable_after_slot(
    &mut self,
    slot: SlotNo,
) -> Result<usize, StorageError>;
```

The wrapper documents the warning that callers must coordinate with
volatile/ledger state — the ChainDb-level primitive is intentionally
narrow (just delegates to ImmutableStore::trim_after_slot); the
db-truncater binary at R389 will wrap this with the appropriate
cleanup.

## Diff inventory

- `crates/storage/src/immutable_db.rs` — `trait ImmutableStore` gains
  `trim_after_slot` method. `InMemoryImmutable` impl provided.
- `crates/storage/src/file_immutable.rs` — `FileImmutable` impl
  provided (mark-dirty / delete CBOR + legacy-JSON / mark-clean).
- `crates/storage/src/chain_db.rs` — `ChainDb::truncate_immutable_after_slot`
  helper added, mirroring the `gc_immutable_before_slot` pattern.
- `crates/storage/tests/integration.rs` — 11 new tests:
  - 7 `InMemoryImmutable` cases (`removes_newer_blocks`,
    `beyond_tip_is_noop`, `zero_clears_all_unless_origin_block`,
    `on_empty_store`, `exact_boundary`, `updates_tip`,
    `inverse_of_trim_before_slot`).
  - 2 `FileImmutable` cases (`removes_newer_blocks` with crash-safe
    re-open verification, `on_empty`).
  - 2 `ChainDb` cases (`truncate_immutable_after_slot` happy path +
    volatile/ledger isolation contract).

## Test inventory

| Layer                                | Tests added |
|--------------------------------------|-------------|
| `ImmutableStore` (trait, two impls)  | 9           |
| `ChainDb::truncate_immutable_after_slot` | 2       |
| **Round contribution**               | **+11**     |

Workspace contribution: 5,115 → 5,126 (+11).

## Verification

```bash
cargo fmt --all -- --check                          # clean
cargo check-all                                     # clean
cargo test-all                                      # 5,126 passed
cargo lint                                          # clean
python3 dev/test/check-strict-mirror.py --fail-on-violation   # 0 violations
python3 dev/test/check-parity-matrix.py              # clean (20 entries vs tag 11.0.1)
python3 dev/test/check-fixture-manifest.py           # clean
cargo test -p yggdrasil-storage --test integration  # 99 tests pass
```

## Round roadmap (Phase B.1 — db-truncater)

| Round | Scope                                                              | Status      |
|-------|--------------------------------------------------------------------|-------------|
| R335  | db-truncater bulk skeleton (file-mirror tree + parser + golden)    | done        |
| R347  | ImmutableStore::trim_after_slot + ChainDb wrapper                  | **this**    |
| R388  | Types + Parsers (CLI args → typed truncate-target)                 | next        |
| R389  | Run.hs equivalent: open ChainDB + dispatch truncate + commit       | scheduled   |
| R390  | Closeout: AGENTS.md + parity-matrix promotion to verified_11_0_1   | scheduled   |

## Notes for future readers

The decision to keep the ChainDb wrapper narrow (just delegating to
ImmutableStore::trim_after_slot, no volatile/ledger cleanup) was
made because:

1. **Composability.** `db-truncater` is one consumer; future tools
   may have different cleanup requirements (e.g. preserve a single
   ledger snapshot vs clear all of them; reset volatile to empty vs
   truncate to a specific volatile-tip).
2. **Test surface.** The narrow primitive is exercised by 11 tests;
   adding the cleanup ceremony at this layer would entangle the
   tests with multi-store consistency invariants.
3. **Upstream parallel.** Upstream's `Tools/DBTruncater/Run.hs`
   doesn't bake the cleanup into the storage layer either — it
   constructs the cleanup orchestration in the tool's `main`
   procedure.

The `trim_after_slot(SlotNo(0))` corner case removes every block
unless one happens to be at slot 0 exactly (none are in our test
fixtures because slot 0 is reserved for the genesis pseudo-tip).
The test name `immutable_trim_after_slot_zero_clears_all_unless_origin_block`
documents this deliberately to flag for future readers that this is
intentional behavior, not a bug.

The `inverse_of_trim_before_slot` invariant test pins the
mathematical relationship: at any given slot boundary, applying
`trim_before(N+1)` to one chain and `trim_after(N)` to a copy
produces complementary halves.
