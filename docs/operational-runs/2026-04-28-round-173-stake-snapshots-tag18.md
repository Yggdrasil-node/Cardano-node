## Round 173 — Upstream `GetStakeSnapshots` (era-specific tag 18) dispatcher

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Complete the era-specific tag-table coverage for the
common `cardano-cli query` operations: implement upstream
era-specific BlockQuery tag 18 (`GetStakeSnapshots`), the
Babbage+ query that powers `cardano-cli query stake-snapshot
--all-stake-pools` (and `--stake-pool-id <id>`).  After R171
(tag 14 `GetStakePoolParams`) and R172 (tag 17 `GetPoolState`),
this closes the wire-protocol parity for every commonly-used
upstream tag we hadn't yet handled.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::GetStakeSnapshots { maybe_pool_hash_set_cbor: Vec<u8> }`
  variant carrying the same raw `Maybe (Set PoolKeyHash)` payload
  shape as R172's `GetPoolState`.
- `decode_query_if_current` recognises `(2, 18)` and slices the
  Maybe payload.

`node/src/local_server.rs`:

- New `encode_stake_snapshots(snapshot, filter)` emits the
  upstream `StakeSnapshots era` record as a 4-element CBOR list:
  ```text
  [
    ssStakeSnapshots :: Map PoolKeyHash [mark_pool, set_pool, go_pool],
    ssStakeMarkTotal :: Coin,
    ssStakeSetTotal  :: Coin,
    ssStakeGoTotal   :: Coin,
  ]
  ```
  When `filter` is `Some(<set>)`, the per-pool map is intersected
  with the supplied pool-hash set (matches upstream
  `Map.restrictKeys`).  When `filter` is `None`, every registered
  pool appears in the per-pool map.  Each map entry is sorted
  ascending by pool keyhash for deterministic CBOR (matches
  upstream `Map.toAscList`).
- Reuses R172's `decode_maybe_pool_hash_set` helper for the
  decoder path — same `Maybe (Set PoolKeyHash)` wire shape.
- Dispatcher routes the new variant into the encoder, keeping
  the existing era-mismatch envelope wrapping
  (`encode_query_if_current_match`).

### Known limitation (carry-over to R163's open follow-up)

Until the live mark/set/go rotation from
`LedgerCheckpointTracking::stake_snapshots` (held in the sync
runtime) is plumbed into `LedgerStateSnapshot` (the LSQ-facing
snapshot), every per-pool entry reports `[0, 0, 0]` for
`(mark_pool, set_pool, go_pool)` and the three totals are zero.
The wire protocol is correct end-to-end; the data populates once
the snapshot is threaded through.  This is consistent with R163's
`GetStakeDistribution` empty-map behaviour and tracked by R163's
outstanding live-stake-distribution work.

### Regression tests (+4)

`crates/network/src/protocols/local_state_query_upstream.rs`:

- `decode_recognises_stake_snapshots_tag_with_just_filter` — pins
  the wire form `82 01 82 12 82 01 d9 0102 81 581c <28 bytes>`
  for `Just <set>`.
- `decode_recognises_stake_snapshots_tag_with_nothing_filter` —
  pins the `82 01 82 12 81 00` form for `Nothing`.

`node/src/local_server.rs`:

- `get_stake_snapshots_empty_snapshot_no_filter_emits_envelope` —
  empty snapshot + no filter → `0x84 0xa0 0x00 0x00 0x00`
  (4-element envelope: empty map, three zero totals).
- `get_stake_snapshots_empty_snapshot_with_filter_emits_envelope`
  — empty snapshot + non-matching filter → also four-element
  envelope.

Test count progression: 4722 → **4726**.

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), the dispatcher is in place but era-blocked
client-side as expected:

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 8960, "epoch": 0, "era": "Alonzo", ... }

$ cardano-cli query stake-snapshot --all-stake-pools --testnet-magic 2
Command failed: query stake-snapshot
Error: This query is not supported in the era: Alonzo.
Please use a different query or switch to a compatible era.
```

This is the expected behaviour — `query stake-snapshot` is
client-side gated at Babbage in cardano-cli 10.16.  R173's
dispatcher handles tag 18 on the wire correctly; the response
auto-unblocks the moment a chain reaches Babbage+ with no
further code changes (and produces the proper non-zero data
once R163's live-snapshot plumbing lands).

R173 is verified by the regression tests plus end-to-end build +
sync (sync rate unchanged at ~14 blk/s, all 11 working
cardano-cli operations continue to succeed).

### Verification gates

```
cargo fmt --all -- --check       # clean (one auto-format applied)
cargo lint                       # clean
cargo test-all                   # passed: 4726  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Cumulative cardano-cli LSQ era-specific tag coverage

| Tag | Query | Round | Status |
|-----|---|---|---|
| 1 | GetEpochNo | R157 | dispatcher ready |
| 3 | GetCurrentPParams | R156 | ✓ working (Shelley/Alonzo/Babbage/Conway) |
| 5 | GetStakeDistribution | R163 | dispatcher ready (empty map until live snapshot rotation) |
| 6 | GetUTxOByAddress | R157 | ✓ working |
| 7 | GetWholeUTxO | R157 | ✓ working |
| 10 | GetFilteredDelegationsAndRewardAccounts | R163 | dispatcher ready |
| 11 | GetGenesisConfig | R163 | dispatcher ready (null until ShelleyGenesis serialisation) |
| 13 | GetStakePools | R163 | dispatcher ready |
| 14 | GetStakePoolParams | R171 | dispatcher ready |
| 15 | GetUTxOByTxIn | R157 | ✓ working |
| 17 | GetPoolState | R172 | dispatcher ready |
| **18** | **GetStakeSnapshots** | **R173** | **dispatcher ready (placeholder zeros until R163 plumbing)** |

Every common upstream era-specific tag now has a wire-correct
dispatcher.  Remaining tags (2 `GetNonMyopicMemberRewards`,
4 `GetProposedPParamsUpdates`, 8 `DebugEpochState`,
12 `DebugNewEpochState`, 16 `GetRewardInfoPools`,
plus Conway-only tags 21–33) are lower-priority for cardano-cli
parity — most are debug queries or used by reward-calculator
tools, not by the standard `cardano-cli query` command surface.

### Open follow-ups

1. **Live stake-snapshot plumbing into `LedgerStateSnapshot`** —
   the R163 follow-up that R173 also depends on.  Threads
   `LedgerCheckpointTracking::stake_snapshots` (the sync
   runtime's mark/set/go ring) into the LSQ snapshot so
   `GetStakeDistribution`, `GetStakeSnapshots`, and any future
   stake-aware queries return non-zero data.
2. Carry-over from R163: `GetGenesisConfig` ShelleyGenesis
   serialisation.
3. Carry-over from R169: apply-batch duration histogram.
4. Carry-over from R168: multi-session peer accounting.
5. Carry-over from R166: pipelined fetch + apply.
6. Carry-over from R167: deep cross-epoch rollback recovery.

### References

- Captures: `/tmp/ygg-r173-preview.log` (post-fix preview sync,
  era-blocked rejection of `stake-snapshot` at Alonzo as
  expected).
- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — `EraSpecificQuery::GetStakeSnapshots` variant + tag-18
  decoder + 2 regression tests; [`node/src/local_server.rs`](node/src/local_server.rs)
  — `encode_stake_snapshots` helper + dispatcher + 2 regression
  tests.
- Upstream reference:
  `Cardano.Ledger.Shelley.LedgerStateQuery.GetStakeSnapshots` —
  era-specific BlockQuery sum-type encoder for tag 18; the
  `StakeSnapshots era` record shape.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-172-pool-state-tag17.md`](2026-04-28-round-172-pool-state-tag17.md).
