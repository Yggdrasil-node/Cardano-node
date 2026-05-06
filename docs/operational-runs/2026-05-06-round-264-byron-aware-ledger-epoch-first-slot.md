## Round 264 — Byron-aware ledger `epoch_first_slot` audit (R263 follow-on)

Date: 2026-05-06
Branch: main
Type: Code-level parity fix (same bug class as R263, different sites)

### Goal

R263 fixed one site that used fixed-length `slot_to_epoch(slot,
epoch_size)` for chains with a Byron prefix (mainnet, preprod). Per
the advisor's note, the same bug class (fixed-length epoch math
anchored at slot 0) likely existed at other sites in the same shape.

This round audits the workspace, identifies remaining sites, and
fixes them in one coherent change.

### Audit findings

`grep` for the bug pattern across the workspace (excluding
`EpochSchedule::*` which is era-aware by construction) turned up
**three additional sites in `crates/ledger/src/state.rs`**, all
computing `(current_epoch + N) * slots_per_epoch` against an
absolute slot number:

| File:line | Site | Effect of bug |
|---|---|---|
| `state.rs:3957` | `should_count_block_producer` (was `current_epoch * slots_per_epoch`) | Shelley-overlay blocks (issued by genesis delegates under d=1) on preprod/mainnet incorrectly counted in `nesBcur`, distorting reward-cycle pool-performance math |
| `state.rs:4100` | `mir_validation_context` (was `(current_epoch + 1) * slots_per_epoch`) | MIR cert deadline-slot check uses wrong stability-window boundary, may admit/reject MIR certs differently than upstream |
| `state.rs:3538` | `validate_ppup_proposal` via `PpupSlotContext.epoch_size` | PPUP slot-of-no-return uses wrong epoch boundary; protocol-update proposals may get accepted/rejected differently than upstream |

All three would have produced runtime drift on chains with a Byron
prefix; preview (Shelley-only) is unaffected.

### Fix

1. Added field `byron_shelley_transition: Option<(u64, u64)>` to
   `LedgerState` (matching the `(boundary_slot, first_shelley_epoch)`
   shape of `EpochSchedule::byron_shelley_transition`).
2. Added `set_byron_shelley_transition(...)` setter.
3. Added `epoch_first_slot(epoch) -> u64` method on `LedgerState`
   that mirrors `EpochSchedule::epoch_first_slot` semantics
   (era-aware first-slot lookup).
4. Replaced the three buggy sites with `self.epoch_first_slot(...)`.
5. Restructured `PpupSlotContext` to carry pre-resolved
   `first_slot_next_epoch: u64` instead of `epoch_size: u64`, so
   the validator no longer recomputes the boundary; the era-aware
   resolution happens once in `ppup_slot_context`.
6. Wired the setter from `node/src/startup.rs` so chains with
   `byron_to_shelley_slot` configured (mainnet, preprod) populate
   the field at boot.

### Regression tests

Added two new unit tests in `crates/ledger/src/state/tests.rs`:

- `preprod_byron_shelley_aware_epoch_first_slot` — pins
  `state.epoch_first_slot(EpochNo(4)) == 86_400` and
  `EpochNo(5) == 518_400` for preprod's
  `byron_shelley_transition = Some((86_400, 4))`. Asserts that
  the buggy fixed-length value `4 * 432_000 = 1_728_000` is NOT
  returned. Also pins Shelley-only (preview) fallback to
  fixed-length math anchored at slot 0.
- `mainnet_byron_shelley_aware_epoch_first_slot` — pins the
  equivalent for mainnet's `byron_shelley_transition =
  Some((4_492_800, 208))`.

### Updated PPUP test fixtures

Five test fixtures in
`crates/ledger/tests/integration/ppup_validation.rs` constructed
`PpupSlotContext` with `epoch_size: 432_000`. Updated to pass
pre-resolved `first_slot_next_epoch: 4_752_000` (the same value the
old math would have produced for `current_epoch=10, epoch_size=432_000`).
The tests are Shelley-only synthetic fixtures so the value is
unchanged; only the field name reflects the new contract.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 848 passed, 0 failed (+2 vs R263)
```

### What remains in the same bug class

The grep audit also surfaced two more sites worth noting:

- `crates/consensus/src/epoch.rs::is_new_epoch` (free function) —
  used as a fallback for `EpochSchedule::is_new_epoch` when no
  schedule is available. Era-aware path goes through
  `EpochSchedule`, fixed-length path is the explicit operator
  fallback. Not a bug; intended behaviour.
- `crates/ledger/src/eras/byron.rs::absolute_slot` — Byron's own
  internal `epoch * slots_per_epoch + slot_in_epoch` — operates
  on Byron's 21600 slots/epoch, no Shelley involvement. Not a bug.

After this round, no production-code site computes Shelley-side
`current_epoch * slots_per_epoch` against an absolute slot.

### What this enables

- Mainnet sync no longer carries silent reward-cycle drift on
  the slot-432000 / slot-864000 / slot-1296000 / ... boundaries
  inside its first Shelley epoch (epoch 208).
- Preprod sync no longer carries silent PPUP/MIR/blocks_made
  drift around its slot-432000 / 864000 / 1296000 / ... boundaries
  inside Shelley epoch 4.
- Preview unaffected (Shelley-only).

### References

- R263 closure: `2026-05-06-round-263-r253-fix-byron-aware-nonce.md`
- Code: `crates/ledger/src/state.rs::epoch_first_slot` (R264 era-aware lookup)
- Tests: `crates/ledger/src/state/tests.rs::{preprod,mainnet}_byron_shelley_aware_epoch_first_slot`
- Upstream rule: `Cardano.Slotting.EpochInfo` /
  `Cardano.Ledger.Slot::epochInfoFirst` at
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Slot.hs`
