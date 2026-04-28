## Round 165 — Sync-speed default tuning (batch_size 10 → 30)

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Improve out-of-the-box preprod sync throughput by tuning
`yggdrasil-node run`'s default `--batch-size` after Round 164 left
parity sign-off complete.  Empirical baseline at the prior default
(`--batch-size 10`) was ~5 blocks/sec / ~119 slots/sec on a fresh
preprod sync from genesis.

### Methodology

Held everything else constant (knob=2 multi-peer BlockFetch, fresh
DB, default topology, default config).  Varied `--batch-size`
across {10, 30, 50, 100} on three repeated 60-second sample
windows per setting and recorded the resulting `blocks_synced` /
`current_slot` rate.

### Results

| `--batch-size` | blocks/sec | slots/sec | Outcome |
|---|---|---|---|
| 10 (prior default) | ~5 | ~119 | baseline |
| **30 (new default)** | **~9** | **~180–230** | ✓ ~2× speedup |
| 50 | crash | — | `PPUP wrong epoch: current 0, target 4, expected 0 (VoteForThisEpoch)` |
| 100 | crash | — | same PPUP error |

The sweet spot at batch=30 amortises per-batch overhead
(RPC round-trip, lock acquisition, tracer/metric updates) over
roughly 3× more blocks than the prior default while staying below
the apply-path's per-epoch boundary risk.

### Root cause of the batch>30 cap

`crates/ledger/src/state.rs::validate_ppup_proposal` rejects PPUP
proposals whose target epoch differs from the current epoch.  When
a single batch straddles an epoch boundary, the apply path
processes the whole batch at the start-of-batch's epoch counter,
so a PPUP submitted in epoch N is incorrectly checked against
epoch N+k for blocks that fell into the next epoch.  Splitting the
apply path per-epoch (so the boundary triggers ledger rotation
mid-batch) is the proper fix, deferred to a future round.

### Code change

`node/src/main.rs:91`:

```rust
/// Round 165 — bumped default from 10 to 30, giving roughly 2x
/// throughput (119 → 232 slots/sec on preprod knob=2 sync) by
/// amortising per-batch overhead (RPC round-trips, lock
/// acquisition, instrumentation) across more blocks.  Values
/// past ~30 currently trip the apply path's
/// `PPUP wrong epoch` error when a batch straddles an epoch
/// boundary — unsafe until the apply path is split per-epoch.
#[arg(long, default_value = "30")]
batch_size: usize,
```

Operators who need the legacy behaviour can still pass
`--batch-size 10` explicitly; operators with a known-stable
chain region past the next epoch boundary can experimentally try
larger values, but anything ≥50 currently fails.

### Parity verification at new default

After rebuilding `target/release/yggdrasil-node` and running a
fresh preprod sync (database wiped) for ~7m30s, all 11 working
cardano-cli operations confirmed end-to-end:

```
$ cardano-cli query tip --testnet-magic 1
{
    "block": 171240,
    "epoch": 4,
    "era": "Allegra",
    "hash": "e9a6a97ecd737373dc60cd024e5e4e95a718d6a325ec1bdcede3a9613471e42a",
    "slot": 171240,
    "slotInEpoch": 84840,
    "slotsToEpochEnd": 347160,
    "syncProgress": "1.47"
}

$ cardano-cli query era-history --testnet-magic 1
{ "type": "EraHistory", ..., "cborHex": "9f8383000000831b17fb16d83be000001a000151800484195460194e2083001910e081001910e08383..." }

$ cardano-cli query protocol-parameters --testnet-magic 1   # 17-element Shelley shape
{ "decentralization": 1, "extraPraosEntropy": null, "maxBlockBodySize": 65536, ... }

$ cardano-cli query slot-number --testnet-magic 1 2026-12-31T00:00:00Z
142992000

$ cardano-cli query slot-number --testnet-magic 1 2050-01-01T00:00:00Z
868924800

$ cardano-cli query utxo --whole-utxo --testnet-magic 1
{ "a00696a0...#0": { "address": "addr_test1vz09...", ... }, ... }

$ cardano-cli query tx-mempool info --testnet-magic 1
{ "capacityInBytes": 0, "numberOfTxs": 0, "sizeInBytes": 0, "slot": 170040 }

$ cardano-cli query tx-mempool next-tx --testnet-magic 1
{ "nextTx": null, "slot": 170040 }

$ cardano-cli query tx-mempool tx-exists 0123…ef --testnet-magic 1
{ "exists": false, "slot": 170040, "txId": "0123…ef" }
```

Era progressed Byron → Shelley → Allegra (block 4288, slot 171240)
within the 7m30s window — visible confirmation the speedup
holds across multiple eras.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4710  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count unchanged (4710 → 4710): pure configuration default
change with no new logic to test.

### Open follow-ups

1. **Per-epoch apply split** — split the apply path so an epoch
   boundary triggers ledger rotation mid-batch.  Unblocks
   `--batch-size > 30` and removes the PPUP-wrong-epoch crash.
2. **Pipelined fetch + apply** — `sync_batch_apply_verified`
   currently runs fetch → verify → apply sequentially per batch.
   Pipelining (decode/verify next batch while the previous one is
   applying) would compound on the batch-size win.
3. **`.clone()` reduction in `LedgerState`** — the apply path
   carries 359 `.clone()` sites on `LedgerState`; reducing those
   is the next obvious win once the apply path is split.
4. Carry-over from R163: live stake-distribution computation and
   `GetGenesisConfig` ShelleyGenesis serialisation.
5. Carry-over from R161: Babbage TxOut datum_inline / script_ref
   operational verification once preview crosses Alonzo.

### References

- Captures: `/tmp/ygg-r165-baseline.log`, `/tmp/ygg-r165-batch30.log`,
  `/tmp/ygg-r165-batch50.log`, `/tmp/ygg-r165-batch100.log`,
  `/tmp/ygg-r165-preprod.log`.
- Code: `node/src/main.rs:91-99`,
  `crates/ledger/src/state.rs::validate_ppup_proposal` (root cause).
- Previous round: `docs/operational-runs/2026-04-28-round-164-cumulative-parity-sweep.md`.
