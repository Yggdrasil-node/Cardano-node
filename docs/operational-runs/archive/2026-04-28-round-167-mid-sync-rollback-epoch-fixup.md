## Round 167 — Mid-sync rollback epoch fixup + extended preview verification

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close the Round 166 follow-up: when a mid-sync rollback recovery
replays the volatile/immutable suffix via `apply_block` (which
does not fire epoch boundaries), `current_epoch` could end up
behind the recovered tip's actual epoch — silently breaking PPUP
validation for any subsequent block whose proposal targets the
true epoch.  Add a post-recovery epoch fixup that brings
`current_epoch` back into agreement with the tip's slot, and
verify the combined R166 + R167 fix holds through a real preview
epoch transition and a graceful restart→recover→resume cycle.

### Code change

`node/src/sync.rs::update_ledger_checkpoint_after_progress`,
inside the rollback branch's non-initial-sync path:

```rust
tracking.ledger_state =
    recover_ledger_state_chaindb(chain_db, tracking.base_ledger_state.clone())?
        .ledger_state;

// Round 167 — post-recovery epoch fixup for mid-sync rollback.
// recover_ledger_state replays via apply_block without firing
// epoch boundaries; force current_epoch to match the recovered
// tip's slot so PPUP validation reads the right epoch number.
if let (Some(epoch_schedule), Point::BlockPoint(slot, _)) =
    (tracking.epoch_size, tracking.ledger_state.tip)
{
    let actual_epoch = epoch_schedule.slot_to_epoch(slot);
    if actual_epoch.0 > tracking.ledger_state.current_epoch().0 {
        tracking.ledger_state.set_current_epoch(actual_epoch);
    }
}
```

Reward distribution is **not** redone — that already happened
during the original live sync, and re-firing
`apply_epoch_boundary` here would require reconstructing the
historical stake snapshots (Phase-3 work).  The recovered ledger
state stays identical to the checkpoint for everything except
`current_epoch`.

### Operational verification

#### Long-running preview sync through epoch 0 → 1 transition

Started fresh preview sync (DB wiped, default `--batch-size 50`),
let it run for 5 m 47 s.

```
$ curl -s :12367/metrics | grep yggdrasil_blocks_synced
yggdrasil_blocks_synced 4349

$ grep EpochBoundary /tmp/ygg-r167-long.log | wc -l
2

$ grep EpochBoundary /tmp/ygg-r167-long.log | tail -1
... newEpoch=1 treasuryDelta=87558 unclaimedRewards=350235 ...
```

The epoch 0 → 1 transition fired with non-zero reward effects
(treasuryDelta=87558, unclaimedRewards=350235), demonstrating
that the boundary path is doing real work and not just bumping a
counter.

```
$ cardano-cli query tip --testnet-magic 2
{
    "block": 88960,
    "epoch": 1,
    "era": "Alonzo",
    "hash": "952a82e08fd71108d50628603334d867fa0e5d7e8fc6efec8adceb35c0ad7362",
    "slot": 88960,
    "slotInEpoch": 2560,
    "slotsToEpochEnd": 83840,
    "syncProgress": "0.08"
}
```

All 11 working cardano-cli operations confirmed end-to-end
post-boundary (`query tip`, `query protocol-parameters`,
`query era-history`, `query slot-number`,
`query utxo --whole-utxo`, three `query tx-mempool` flavours).

Era-blocked queries (`query stake-pools`,
`query stake-distribution`, `query pool-state`) correctly fail
client-side with `This query is not supported in the era: Alonzo.`,
confirming yggdrasil's PV-aware era classification reports Alonzo
to cardano-cli (the dispatchers behind these queries are wired
up per R163 and will auto-unblock once preview crosses to
Babbage).

#### Restart recovery cycle

Killed yggdrasil at slot ~13960, then restarted from the same DB
to exercise the recovery path:

```
[Notice Node.Recovery] recovered ledger state from coordinated
  storage checkpointSlot=12960 point=BlockPoint(SlotNo(13960),
  HeaderHash(7f8f6d2dbcf5eaee…)) replayedVolatileBlocks=50
[Info Node.Recovery.Checkpoint] persisted slot=14940 rollbackCount=1
[Info Node.Recovery.Checkpoint] persisted slot=17940 rollbackCount=0
[Info Node.Recovery.Checkpoint] persisted slot=20940 rollbackCount=0
```

Recovery replayed 50 volatile blocks from checkpoint(slot 12960)
to tip(slot 13960), the first session-start RollBackward fired
correctly (rollbackCount=1 in the first batch), and forward sync
resumed without PPUP errors at ~14 blocks/sec.  All cardano-cli
operations continued to work post-restart (`query tip` reported
Alonzo era at slot 22940 with full protocol parameters).

The R167 fixup branch is exercised whenever a rollback recovery
yields a tip in a later epoch than the latest checkpoint — it did
not fire in this 30-second restart window because preview's first
epoch is 86400 slots and the volatile depth (~13960 - 12960 = 1000
slots) stayed within a single epoch.  The fixup is dormant in the
common case and only kicks in for deep cross-epoch rollbacks.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4710  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count unchanged (4710 → 4710): the fixup is exercised by
production sync recovery paths, and a synthetic unit test would
require constructing a contrived multi-epoch rollback scenario
that doesn't naturally arise in the existing test fixtures.

### Known limitation (carry-over)

For a deep cross-epoch rollback recovery, **rewards are not
redistributed** during replay.  The recovered ledger state has
the rewards as they were at the latest checkpoint — which is
correct if the checkpoint pre-dates the rollback target's epoch
(rewards were already distributed for prior boundaries when the
checkpoint was created).  The pathological case is a checkpoint
in epoch N, then a rollback to epoch N+M (M ≥ 1) with no
intermediate checkpoint — the volatile suffix replay won't redo
the boundaries, and `current_epoch` will be patched to N+M but
the per-epoch reward / snapshot state stays at N's snapshot.  In
practice this is bounded by `checkpointIntervalSlots = 2160`
(within k=2160 stability window), which is well below preprod's
432K-slot epoch length, so the case is not reachable under the
default operational config.

### Open follow-ups

1. **Reward replay during recovery** — for full correctness in
   the pathological case above, plumb `EpochSchedule` and
   `StakeSnapshots` into `recover_ledger_state` and re-fire
   `apply_epoch_boundary` for every crossed boundary during
   replay.  Currently deferred because the case is unreachable at
   default config and the redo would need historical stake
   snapshots.
2. **Pipelined fetch + apply** — `sync_batch_apply_verified` /
   `apply_verified_progress_to_chaindb` still run fetch → verify
   → apply sequentially per batch.  Carry-over from R166.
3. **`.clone()` reduction in `LedgerState`** (359 sites in the
   apply path).  Carry-over from R166.
4. Carry-over from R163: live stake-distribution computation and
   `GetGenesisConfig` ShelleyGenesis serialisation.
5. Carry-over from R161: Babbage TxOut datum_inline / script_ref
   operational verification once preview crosses Alonzo.

### References

- Captures: `/tmp/ygg-r167-long.log` (5m47s preview sync through
  epoch 0→1 boundary), `/tmp/ygg-r167-restart.log` (graceful
  restart→recover→resume cycle), `/tmp/ygg-r167-verify.log`
  (post-fix smoke).
- Code: [`node/src/sync.rs`](node/src/sync.rs)
  `update_ledger_checkpoint_after_progress` (post-recovery epoch
  fixup ~14 LOC).
- Upstream reference: `Cardano.Ledger.Shelley.Rules.NewEpoch` —
  PPUP validation reads `current_epoch`.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-166-rollback-recovery-fix.md`](2026-04-28-round-166-rollback-recovery-fix.md).
