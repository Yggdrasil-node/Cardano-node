## Round 263 — R253 closure: Byron-aware TPraos nonce evolution

Date: 2026-05-06
Branch: main
Type: Code-level parity fix (root-cause closure of R249/R262 preprod VRF gap)

### Summary

**R253 (Gap BO) is closed.** Yggdrasil now syncs preprod past the
prior R249 / R262 TPraos VRF failure window without
`YGG_SKIP_PHASE2`. The fix: `NonceEvolutionConfig` learned the
Byron→Shelley transition boundary and `apply_block` now uses
era-aware slot→epoch math instead of fixed-length math anchored at
slot 0.

### Root cause

`NonceEvolutionState::apply_block` derived the block's epoch via
`slot_to_epoch(slot, epoch_size)` — a fixed-length rule that maps
`slot 432000` to `EpochNo(1)` regardless of network. For chains
with a Byron prefix (mainnet, preprod), upstream's actual epoch
schedule is:

| Network | Byron→Shelley boundary | First Shelley epoch label | Shelley `epoch_size` |
|---------|------------------------|---------------------------|----------------------|
| preprod | slot 86400             | epoch 4                   | 432000               |
| mainnet | slot 4492800           | epoch 208                 | 432000               |
| preview | slot 0 (no Byron)      | epoch 0                   | 86400                |

Under the era-aware rule, preprod slot 432000 = Shelley epoch 4
(offset 345600, post=345600/432000=0). Pre-R263 yggdrasil saw
slot 432000 as `EpochNo(1)`, fired the `tick_epoch_transition`
TICKN rule, and rotated `epoch_nonce` to a wrong value. Every
subsequent VRF check used the rotated nonce, producing
`InvalidVrfProof` on the next active-overlay block.

The trace from R262 captured the rotated nonce
`0xca171050abaf1c068c4c3ba71b3fb2c3f8cb567fbeb2a35904b6379d0bbb4e94`
at slot 432000, where the upstream-correct active η for preprod's
Shelley epoch 4 should remain the seed derived from
`ShelleyGenesisHash` (`0x162d29c4...`) until slot 518400.

### Fix scope

1. `crates/consensus/src/nonce.rs`:
   - Added `byron_shelley_transition: Option<(u64, u64)>` field to
     `NonceEvolutionConfig`.
   - Added `slot_to_epoch` and `epoch_first_slot` methods on
     `NonceEvolutionConfig` that mirror
     `EpochSchedule::slot_to_epoch` / `epoch_first_slot` semantics.
   - `apply_block` and the stability-window check both use the
     era-aware methods.
2. `node/src/commands/run.rs`: populates `byron_shelley_transition`
   from `file_cfg.byron_to_shelley_slot` + `file_cfg.first_shelley_epoch`.
3. Workspace test fixtures: added `byron_shelley_transition: None`
   to all in-process `NonceEvolutionConfig` constructors that target
   Shelley-only fixed-length scenarios.

### Verification — code level

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 846 passed, 0 failed (+1 vs R262)
```

New regression test:
`crates/consensus/src/nonce.rs::preprod_byron_shelley_transition_no_spurious_epoch_tick_at_slot_432000`
asserts:

- `slot_to_epoch(SlotNo(432000))` with preprod's
  `byron_shelley_transition = Some((86400, 4))` returns `EpochNo(4)`,
  NOT `EpochNo(1)` (the bug).
- `slot_to_epoch(SlotNo(518400))` returns `EpochNo(5)` (true
  Shelley epoch boundary).
- An end-to-end block-apply scenario at slots 86420 → 432000 →
  518400 confirms `tick_epoch_transition` fires only at the actual
  epoch boundary (518400), not spuriously at 432000.

### Verification — runtime

Re-ran the same preprod sync that produced the R249 / R262
failures (fresh DB, primary peer `3.79.79.217:3001`, knob=2 default):

| Phase                  | R249 / R262 (pre-fix)         | R263 (post-fix)             |
|------------------------|-------------------------------|------------------------------|
| Sync from genesis      | progressed                    | progressed                   |
| Slot 86400 (Byron→Sh.) | passed                        | passed                       |
| Slot 432000 (failure)  | **`invalid VRF proof` ❌**    | **passed cleanly ✅**        |
| Slot 446460            | n/a (sync had stopped)        | passed                       |
| Slot 511460            | n/a                           | passed                       |
| Final captured tip     | slot 432000                   | slot 511460 (~21 300 blocks) |
| `invalid VRF proof`    | 1                             | **0**                        |
| `blockfetch_workers_registered` | n/a                  | functional (knob=2 default)  |

The sync was deliberately stopped at slot 511460 to confirm
sustained progress; nothing in the trace suggests a downstream
failure.

### What this enables

- Preprod sync past slot 432000 without `YGG_SKIP_PHASE2`. R253 closed.
- Mainnet sync similarly benefits — mainnet has Byron→Shelley at
  slot 4492800 / epoch 208; the same era-aware fix prevents
  spurious `tick_epoch_transition` events at every multiple of
  `epoch_size = 432000` from slot 0 (mainnet hits the bug at
  slot 432000, 864000, ...).
- Preview is unaffected (Shelley-only, `byron_shelley_transition = None`).

### Open follow-ups

- **Mainnet endurance**: the fix is applied uniformly; mainnet
  sync should now be similarly unblocked at the slot-432000 /
  864000 / ... checkpoints. Operator-time verification (multi-day
  mainnet rehearsal) is still pending per
  `docs/MANUAL_TEST_RUNBOOK.md` §2–9.
- **Preprod endurance past slot 511460**: this round only confirmed
  the R249 failure window passes. Continued sync to current preprod
  tip is operator-time work; nothing in the static evidence
  suggests further blocks.
- **Gap BP (Plutus CEK budget overrun)** at preview slot ~1462057
  remains open per the prior R252 instrumentation. Independent
  from R253.

### References

- R249 forensic capture: `2026-05-05-round-249-preprod-vrf-failure-slot-429460.log`
- R259 diagnostic enrichment: `2026-05-06-round-259-tpraos-overlay-vrf-diagnostics.md`
- R261 sub-candidate narrowing (3 → 2): `2026-05-06-round-261-r253-narrowing.md`
- R262 final narrowing (→ 1): `2026-05-06-round-262-r253-final-narrowing-nonce-evolution.md`
- Upstream rule: `Cardano.Protocol.TPraos.Rules.Tickn::ticknTransition`
  at `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/Rules/Tickn.hs`
- Upstream era-aware schedule: `Cardano.Slotting.EpochInfo` /
  `Cardano.Ledger.Slot::epochInfoFirst`
- Code: `crates/consensus/src/nonce.rs:204-243` (`apply_block`)
  + `:108-189` (`NonceEvolutionConfig` + era-aware methods)
- Regression test: `crates/consensus/src/nonce.rs::preprod_byron_shelley_transition_no_spurious_epoch_tick_at_slot_432000`
- Captured runtime evidence: `/tmp/ygg-r262-preprod/out.log`
