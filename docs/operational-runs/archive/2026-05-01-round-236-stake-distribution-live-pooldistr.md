## Round 236 — Phase A.3 LSQ data plumbing: live `PoolDistr` for stake-distribution & SPO-stake-distribution

Date: 2026-05-01
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)
Phase: A.3 (LSQ data plumbing — stake distribution slice)

### Goal

Through R164 → R191 the wire-protocol surface for every Conway-era
LSQ subcommand was completed: every `cardano-cli conway query`
subcommand decodes end-to-end and the wire shapes match upstream.
What remained was data plumbing — several encoders still emitted
empty/placeholder bodies in fields that yggdrasil's runtime *already
tracks*.

R203 plumbed `StakeSnapshots` into `LedgerStateSnapshot` (loaded
from `yggdrasil_storage::load_stake_snapshots` after every snapshot
rotation).  R236 finishes the parity loop for two LSQ queries that
were still emitting the empty / approximation paths:

1. **`GetStakeDistribution` / `GetStakeDistribution2`** (Shelley
   tags 5 & 37): `encode_stake_distribution_map` was always emitting
   the canonical 2-element `[empty_map, 1-lovelace]` envelope —
   wire-correct shape but no real data.
2. **`GetSPOStakeDistr`** (Conway tag 30):
   `encode_spo_stake_distribution_for_lsq` was approximating
   per-pool stake by summing **reward-account balances** of
   delegating credentials, which materially diverges from
   upstream's `nesPd` (active stake from the `set` snapshot).

### Implementation

**`encode_stake_distribution_map`** in
[`node/src/local_server.rs`](../../node/src/local_server.rs) now
sources the pool distribution from
`snapshot.stake_snapshots().map(|s| s.set.pool_stake_distribution())`
(matching upstream `Cardano.Ledger.Conway.LedgerStateQuery`'s use
of `nesPd`, which derives from the `set` snapshot for current-epoch
leader election).  Each map entry is an upstream-faithful
`IndividualPoolStake`:

```text
[ Rational stake_share        -- tag 30 [pool_stake, total_active]
, CompactCoin pool_stake      -- uint
, VRFKeyHash 32-byte vrf      -- bytes(32)
]
```

The outer `pdTotalActiveStake` is `total_active.max(1)` (cardano-cli
rejects a zero-coin `NonZero` field with `DeserialiseFailure
"Encountered zero while trying to construct a NonZero value"`).

**`encode_spo_stake_distribution_for_lsq`** now sources per-pool
totals from the same `set.pool_stake_distribution()` when snapshots
are attached.  The R194 reward-balance approximation is preserved
as a fallback for chains that have not yet completed an epoch
boundary (snapshot rotation hasn't fired).

### Wire shape (verified via regression test)

`encode_stake_distribution_map` against a populated `set` snapshot
with two pools (pool A, 600+300 lovelace; pool B, 100 lovelace):

```text
0x82                           -- list-2 envelope
  0xa2                         -- map-2 (unPoolDistr)
    0x58 0x1c <pool_a 28>      -- key
    0x83                       -- IndividualPoolStake list-3
      0xd8 0x1e 0x82 ...       -- tag 30 [900, 1000]
      0x19 0x03 0x84           -- pool_stake = 900
      0x58 0x20 <vrf_a 32>     -- VRF key hash
    0x58 0x1c <pool_b 28>
    0x83
      0xd8 0x1e 0x82 ...       -- tag 30 [100, 1000]
      0x18 0x64                -- pool_stake = 100
      0x58 0x20 <vrf_b 32>
  0x19 0x03 0xe8               -- pdTotalActiveStake = 1000
```

The empty-snapshot fallback is preserved (`0x82 0xa0 0x01`),
matching the original R179 envelope shape.

### Verification

- New regression test
  `get_stake_distribution_with_live_snapshot_emits_individual_pool_stakes`
  builds a 3-credential / 2-pool `set` snapshot, attaches it via
  `LedgerStateSnapshot::with_stake_snapshots`, decodes the encoded
  bytes through the project CBOR decoder, and asserts:
  - 2-element envelope present
  - 2-entry inner map
  - per-pool 3-tuple shape (Rational, Coin, VRF)
  - rational denominator = total active stake
  - VRF key hash propagated from `PoolParams.vrf_keyhash`
  - `pdTotalActiveStake` = sum of delegated stake
- Existing test
  `get_stake_distribution_empty_snapshot_emits_pool_distr_envelope`
  continues to pin the empty-snapshot fallback.

Workspace gates:

```text
cargo fmt --all -- --check    PASS
cargo lint                    PASS (0 warnings)
cargo test-all                4 746 passed, 0 failed
                              (was 4 744 — added 2 tests this round)
cargo build -p yggdrasil-node PASS
```

### What this changes operationally

When yggdrasil has completed at least one epoch boundary (so
`load_stake_snapshots` finds a non-empty rotation in
`<datadir>/ledger/`), `cardano-cli conway query stake-distribution`
now renders the real per-pool active stake distribution instead of
the previous empty map.  Same applies to `cardano-cli conway query
spo-stake-distribution`, which previously approximated via reward
balances.

### Why pick the `set` snapshot (not `mark` or `go`)

Upstream `nesPd :: NewEpochState -> PoolDistr` is derived from the
`set` snapshot — the one used for **leader election in the current
epoch**.  Per `crates/ledger/src/stake.rs`:

> `set` — previous mark; used for leader election in the current
> epoch.

This is what every operator-facing query (`cardano-cli query
stake-distribution`, `pool-state`, `spo-stake-distribution`)
expects.  `mark` is the fresh boundary-time snapshot (next epoch's
leadership) and `go` is the reward-calculation snapshot (previous
epoch's leadership).

### Related deferred items

This is the first piece of the Phase A.3 cleanup arc.  Remaining
data-plumbing tasks for the LSQ surface:

- `GetDRepStakeDistr` (tag 28) — currently uses live data but the
  tally derivation could be cross-validated against upstream's
  `queryDRepStakeDistr`.
- `GetStakeSnapshots` totals — already use live data when snapshots
  are attached (R202); the `1`-coin floor for `NonZero` fields is
  preserved.
- `GetGenesisConfig` 15-element list — wired in R214 with full live
  Shelley genesis data.

### References

- Upstream: `Cardano.Ledger.Core.PoolDistr.encCBOR`
- Upstream: `Cardano.Protocol.TPraos.API.IndividualPoolStake`
- Upstream: `Cardano.Ledger.Conway.LedgerStateQuery.querySPOStakeDistr`
- Upstream: `Cardano.Ledger.Shelley.LedgerState.nesPd`
- yggdrasil:
  - `node/src/local_server.rs::encode_stake_distribution_map`
  - `node/src/local_server.rs::encode_spo_stake_distribution_for_lsq`
  - `crates/ledger/src/stake.rs::StakeSnapshot::pool_stake_distribution`
  - `crates/ledger/src/stake.rs::StakeSnapshots`
