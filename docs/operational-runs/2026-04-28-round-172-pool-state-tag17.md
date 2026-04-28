## Round 172 — Upstream `GetPoolState` (era-specific tag 17) dispatcher

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Continue the Haskell-node parity arc started in R171: implement
upstream era-specific BlockQuery tag 17 (`GetPoolState`),
the actual Babbage+ query that powers
`cardano-cli query pool-state --all-stake-pools` (and
`--stake-pool-id <id>`).  yggdrasil already tracked all four
PState components — `psStakePoolParams`,
`psFutureStakePoolParams`, `psRetiring`, `psDeposits` — in its
`pool_state`, but the canonical era-specific tag-17 query
returned `Unknown { tag: 17 } → null`.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::GetPoolState { maybe_pool_hash_set_cbor: Vec<u8> }`
  variant carrying the raw CBOR `Maybe (Set PoolKeyHash)` payload.
- `decode_query_if_current` recognises `(2, 17)` and slices the
  Maybe-payload out of the inner CBOR.

`node/src/local_server.rs`:

- New `decode_maybe_pool_hash_set(bytes)` parses the Maybe wrapper
  per upstream's encoding:
  - `[0]` → `Nothing` (return state for all pools)
  - `[1, set]` → `Just <set>` (filter to the supplied pool hashes)
  - bare `null` (CBOR major 7) — also accepted as `Nothing` for
    forward-compatibility with upstream encoders that skip the
    list wrapper.
- New `encode_pool_state(snapshot, filter)` emits the upstream
  `PState` 4-tuple as a 4-element CBOR list:
  ```text
  [
    psStakePoolParams       :: Map PoolKeyHash PoolParams,
    psFutureStakePoolParams :: Map PoolKeyHash PoolParams,
    psRetiring              :: Map PoolKeyHash EpochNo,
    psDeposits              :: Map PoolKeyHash Coin,
  ]
  ```
  Each component is sorted ascending by pool keyhash for
  deterministic CBOR (matches upstream `Map.toAscList`).
  When `filter` is `Some(<set>)`, every map is intersected with
  the supplied pool-hash set (matches upstream's
  `maybe id Map.restrictKeys`).  When `filter` is `None`, every
  registered pool appears.
- Dispatcher routes the new variant into the encoder, keeping
  the existing era-mismatch envelope wrapping
  (`encode_query_if_current_match`).

The `psFutureStakePoolParams` component pulls from
`pool_state.future_params()` (already maintained by yggdrasil's
SNAP rule per `register_with_deposit` staging — see
[`crates/ledger/src/state.rs`](crates/ledger/src/state.rs)
`PoolState.future_params`).

### Regression tests (+7)

`crates/network/src/protocols/local_state_query_upstream.rs`:

- `decode_recognises_pool_state_tag_with_just_filter` — pins the
  wire form `82 01 82 11 82 01 d9 0102 81 581c <28 bytes>` for
  `Just <set>`.
- `decode_recognises_pool_state_tag_with_nothing_filter` — pins
  the `82 01 82 11 81 00` form for `Nothing`.

`node/src/local_server.rs`:

- `get_pool_state_empty_snapshot_no_filter_emits_four_empty_maps`
  — empty snapshot + no filter → `0x84 0xa0 0xa0 0xa0 0xa0`.
- `get_pool_state_empty_snapshot_with_filter_emits_four_empty_maps`
  — empty snapshot + non-matching filter → also four empty maps.
- `decode_maybe_pool_hash_set_accepts_zero_discriminator` —
  `[0]` decodes to `None`.
- `decode_maybe_pool_hash_set_accepts_one_discriminator_with_set`
  — `[1, tag(258)[bytes(28)]]` decodes to `Some(<set>)` with the
  expected hash.
- `decode_maybe_pool_hash_set_accepts_null_as_nothing` — bare
  CBOR `null` (`0xf6`) also decodes to `None`.

Test count progression: 4715 → **4722**.

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), the dispatcher is in place but era-blocked
client-side as expected:

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 5960, "epoch": 0, "era": "Alonzo", ... }

$ cardano-cli query pool-state --all-stake-pools --testnet-magic 2
Command failed: query pool-state
Error: This query is not supported in the era: Alonzo.
Please use a different query or switch to a compatible era.
```

This is the expected behaviour — `query pool-state` is
client-side gated at Babbage in cardano-cli 10.16.  R172's
dispatcher handles tag 17 on the wire correctly; the response
auto-unblocks the moment a chain reaches Babbage+ with no
further code changes.

R172 is verified by the regression tests plus end-to-end build +
sync (sync rate unchanged at ~14 blk/s, all 11 working
cardano-cli operations continue to succeed).

### Verification gates

```
cargo fmt --all -- --check       # clean (one auto-format applied)
cargo lint                       # clean
cargo test-all                   # passed: 4722  failed: 0  ignored: 1
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
| **17** | **GetPoolState** | **R172** | **dispatcher ready** |

### Open follow-ups

1. **Tag 18 `GetStakeSnapshots`** — needed for
   `cardano-cli query stake-snapshot`.  Returns
   `(SnapShots era)` (mark/set/go); requires the live
   stake-snapshot rotation also pending for R163's
   `GetStakeDistribution`.
2. Carry-over from R163: live stake-distribution computation +
   `GetGenesisConfig` ShelleyGenesis serialisation.
3. Carry-over from R169: apply-batch duration histogram.
4. Carry-over from R168: multi-session peer accounting.
5. Carry-over from R166: pipelined fetch + apply.
6. Carry-over from R167: deep cross-epoch rollback recovery.

### References

- Captures: `/tmp/ygg-r172-preview.log` (post-fix preview sync,
  era-blocked rejection of `pool-state` at Alonzo as expected).
- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — `EraSpecificQuery::GetPoolState` variant + tag-17 decoder + 2
  regression tests; [`node/src/local_server.rs`](node/src/local_server.rs)
  — `decode_maybe_pool_hash_set` + `encode_pool_state` helpers +
  dispatcher + 5 regression tests.
- Upstream reference:
  `Cardano.Ledger.Shelley.LedgerStateQuery.GetPoolState` —
  era-specific BlockQuery sum-type encoder for tag 17;
  `Cardano.Ledger.Shelley.LedgerState.PState` — record shape.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-171-stake-pool-params-tag14.md`](2026-04-28-round-171-stake-pool-params-tag14.md).
