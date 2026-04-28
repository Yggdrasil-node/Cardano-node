## Round 166 — Initial-sync rollback fix unblocks batch_size > 30

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Identify and fix the apply-path bug behind Round 165's
`PPUP wrong epoch` crashes at `--batch-size > 30`, then bump the
default to the new sweet spot.

### Root cause

Every fresh ChainSync session begins with the upstream server
confirming the requested intersect by sending `MsgRollBackward` to
that point.  When yggdrasil syncs from genesis, the very first
batch's progress reports
`[RollBackward(Origin), RollForward(blocks 1..N)]` with
`rollback_count = 1`.

`update_ledger_checkpoint_after_progress` in
[`node/src/sync.rs`](node/src/sync.rs) takes its rollback branch on
this `rollback_count > 0` and calls
`recover_ledger_state_chaindb` — which replays the entire volatile
suffix (including the new RollForward blocks) via
`LedgerState::apply_block`.  `apply_block` does **not** fire epoch
boundary processing: `current_epoch` stays at 0 even as the tip
advances through Byron epochs and into Shelley.

The first Shelley block carrying a PPUP proposal targeting epoch
4 then trips
`validate_ppup_proposal`'s `PPUP wrong epoch` check
(`current 0, target 4, expected 0 (VoteForThisEpoch)`).

The bug only manifested at `--batch-size ≥ ~50` because preprod
has only ~140 Byron blocks; smaller batches kept the Byron→Shelley
transition out of the first batch (where the rollback branch
runs), so subsequent batches' boundary-aware forward path
correctly advanced `current_epoch` block-by-block.

### Fix

`node/src/sync.rs::update_ledger_checkpoint_after_progress`:
detect the initial-sync rollback shape (rollback target `Origin`
**and** `tracking.base_ledger_state.tip == Point::Origin`) and
bypass the heavy `recover_ledger_state_chaindb` call.  Reset to the
base ledger state and let the forward portion of progress apply
through the boundary-aware path
(`advance_ledger_with_epoch_boundary`), which iterates per-block
and fires `apply_epoch_boundary` whenever
`epoch_schedule.is_new_epoch(prev, curr)` returns true.

```rust
let initial_sync_rollback_to_origin = matches!(
    progress.steps.iter().find_map(|step| match step {
        MultiEraSyncStep::RollBackward { point, .. } => Some(*point),
        _ => None,
    }),
    Some(Point::Origin)
) && tracking.base_ledger_state.tip == Point::Origin;

if initial_sync_rollback_to_origin {
    tracking.ledger_state = tracking.base_ledger_state.clone();
} else {
    tracking.ledger_state = recover_ledger_state_chaindb(...)?;
}

// ... reset stake snapshots / pool counts / ocert counters ...

if initial_sync_rollback_to_origin {
    // Apply forward portion via boundary-aware path.
    if let (Some(snapshots), Some(epoch_size)) = ... {
        epoch_events = advance_ledger_with_epoch_boundary(
            &mut tracking.ledger_state, snapshots, epoch_size,
            progress, ...,
        )?;
    } else {
        advance_ledger_state_with_progress(...)?;
    }
}
```

`recover_ledger_state` itself is not touched — it remains correct
for the startup-recovery callers (where the latest ledger
checkpoint already has the right `current_epoch`, so the volatile
replay only spans a few blocks within a single epoch).

### Verification

Behaviour at `--batch-size ∈ {30, 50, 100}` on fresh preprod
syncs from genesis (DB wiped each time):

| batch | epoch boundaries | outcome | rate (60s window) |
|---|---|---|---|
| 30 | newEpoch 0→1→2→3→4 fire across multiple batches | ✓ | ~9 blk/s |
| **50** | **all five fire in the first batch** | ✓ | **~14 blk/s** |
| 100 | all five fire in the first batch | ✓ | ~10 blk/s (peer-side fetch latency dominates) |

50 is the new default — past it, per-batch overhead is no longer
the bottleneck and the throughput plateaus.

### Code changes

- [`node/src/sync.rs`](node/src/sync.rs)
  `update_ledger_checkpoint_after_progress`: initial-sync
  rollback fast path, ~40 LOC of insertion plus a dedicated
  rustdoc paragraph anchored to
  `Ouroboros.Network.Protocol.ChainSync.Server`.
- [`node/src/main.rs`](node/src/main.rs):91 default `batch_size`
  10 → 30 → **50** with rustdoc explaining the cap is gone after
  the apply-path fix.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4710  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count unchanged (4710 → 4710): no new behaviour to pin in
unit tests — the fix is exercised end-to-end by every initial
preprod/preview sync, and adding a synthetic
`[RollBackward(Origin), RollForward(...)]`-spanning-Byron→Shelley
fixture would duplicate what the live sweep already covers.

### Parity sweep at the new default

After rebuilding and running a fresh preprod sync (DB wiped,
default `--batch-size 50`) for ~92s:

```
$ cardano-cli query tip --testnet-magic 1
{
    "block": 115440,
    "epoch": 4,
    "era": "Allegra",
    "hash": "b3fef5400b0ff7679a60a6130a3b057becdc93d65a8fb114045c5caa7183e366",
    "slot": 115440,
    "slotInEpoch": 29040,
    "slotsToEpochEnd": 402960,
    "syncProgress": "1.43"
}
```

All 11 working cardano-cli operations confirm end-to-end at the
new default — `query tip`, `query era-history`,
`query protocol-parameters`, `query slot-number 2026-12-31` →
`142992000`, `query utxo --whole-utxo`, `query tx-mempool info` /
`next-tx` / `tx-exists`.

### Open follow-ups

1. **Mid-sync rollback boundary skipping**.  When the rollback
   target is `BlockPoint(...)` (not `Origin`),
   `recover_ledger_state` still walks the volatile suffix via
   `apply_block` without firing boundaries.  This is harmless when
   the volatile depth is below `k=2160` slots and the recovery
   stays within a single epoch (the common case), but a deep
   rollback that spans an epoch boundary would corrupt
   `current_epoch`.  Plumbing
   `EpochSchedule + StakeSnapshots` into `recover_ledger_state`
   (or making `apply_block_validated` itself
   epoch-schedule-aware) is the proper long-term fix.
2. **Pipelined fetch + apply** — `sync_batch_apply_verified` /
   `apply_verified_progress_to_chaindb` currently run fetch →
   verify → apply sequentially per batch.  Pipelining
   (decode/verify next batch while the previous one is applying)
   would compound on this round's win.
3. **`.clone()` reduction in `LedgerState`** — the apply path
   carries 359 `.clone()` sites; once pipelining lands they
   become the next obvious win.
4. Carry-over from R163: live stake-distribution computation and
   `GetGenesisConfig` ShelleyGenesis serialisation.
5. Carry-over from R161: Babbage TxOut datum_inline / script_ref
   operational verification once preview crosses Alonzo.

### References

- Captures: `/tmp/ygg-r166-batch50-fixed.log` (boundaries fire at
  newEpoch=0..4 in the first batch, no PPUP error),
  `/tmp/ygg-r166-batch100.log` (same), `/tmp/ygg-r166-preprod.log`
  (parity sweep).
- Code: [`node/src/sync.rs`](node/src/sync.rs)
  `update_ledger_checkpoint_after_progress` (initial-sync
  rollback fast path), [`node/src/main.rs`](node/src/main.rs):91
  (new default).
- Upstream reference:
  `Ouroboros.Network.Protocol.ChainSync.Server` —
  `MsgRollBackward` confirmation behaviour at session start.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-165-sync-speed.md`](2026-04-28-round-165-sync-speed.md).
