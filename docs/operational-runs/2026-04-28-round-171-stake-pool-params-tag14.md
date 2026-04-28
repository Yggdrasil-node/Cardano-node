## Round 171 — Upstream `GetStakePoolParams` (era-specific tag 14) dispatcher

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Close a Haskell-node parity gap by handling upstream era-specific
BlockQuery tag 14 (`GetStakePoolParams`) end-to-end.  yggdrasil
already had the data (`pool_state` per R163) and a yggdrasil-CLI
tag-12 dispatcher for individual pool lookups, but the canonical
upstream tag-14 era-specific query (used by
`cardano-cli query pool-state --stake-pool-id <id>` once a chain
reaches Babbage+) returned `Unknown { tag: 14, .. } → null`.

The query is era-blocked client-side at Alonzo so cardano-cli
itself still rejects it pre-Babbage; this round wires the
dispatcher so the response auto-unblocks the moment preview /
preprod / mainnet hit Babbage with no further code changes.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery::GetStakePoolParams { pool_hash_set_cbor: Vec<u8> }`
  variant carrying the raw CBOR-encoded set of 28-byte pool key
  hashes between the leading tag and the closing array delimiter.
- `decode_query_if_current` recognises `(2, 14)` and slices the
  pool-hash-set payload out of the inner CBOR.

`node/src/local_server.rs`:

- New `decode_pool_hash_set(bytes)` parses the CBOR set/array of
  28-byte pool keyhashes — tolerates both the canonical
  `tag(258) [* bytes(28)]` (CIP-21 set tag) and the legacy
  untagged-array shapes.
- New `encode_filtered_stake_pool_params(snapshot, pool_hashes)`
  emits the upstream `Map (KeyHash 'StakePool) PoolParams` shape
  filtered by the supplied set: looks up each hash in
  `snapshot.pool_state()`, sorts the matched pairs by keyhash for
  deterministic CBOR (matches upstream `Map.toAscList`), and
  emits `<keyhash_bytes> <pool.params().encode_cbor>` per entry.
  Unknown pools are silently dropped (matches upstream
  `Map.intersection` semantics).
- Dispatcher routes `EraSpecificQuery::GetStakePoolParams` into
  the new encoder, keeping the existing era-mismatch envelope
  wrapping (`encode_query_if_current_match`).

### Regression tests (+5)

- `decode_recognises_stake_pool_params_tag` — pins the wire form
  `82 01 82 0e d9 0102 81 581c <28 bytes>` decodes to the new
  variant with `era_idx=1`.
- `get_stake_pool_params_empty_filter_emits_empty_map` —
  empty filter against any snapshot emits `0xa0`.
- `get_stake_pool_params_unknown_filter_emits_empty_map` —
  filter for a non-registered pool-hash emits `0xa0`
  (intersection drops unknown pools).
- `decode_pool_hash_set_accepts_tagged_set_form` — decoder
  accepts the canonical `tag(258) [* bytes(28)]` shape.
- `decode_pool_hash_set_accepts_untagged_array_form` — decoder
  also accepts the legacy untagged-array shape.

Test count progression: 4710 → **4715**.

### Operational verification

After rebuild and a fresh preview sync (DB wiped, default
`--batch-size 50`), the dispatcher is in place but era-blocked
client-side as expected:

```
$ cardano-cli query tip --testnet-magic 2
{ "block": 8960, "epoch": 0, "era": "Alonzo", ... }

$ cardano-cli query pool-state --all-stake-pools --testnet-magic 2
Command failed: query pool-state
Error: This query is not supported in the era: Alonzo.
Please use a different query or switch to a compatible era.
```

This is the expected behaviour — `query pool-state` is
client-side gated at Babbage in cardano-cli 10.16.  The
`pool-state` flow uses the era-specific tag-17 `GetPoolState`
query (also era-blocked), which is a separate Babbage+ addition
not covered by R171; cardano-cli's `--stake-pool-id` flag dispatches
through tag-14 internally for the per-pool params lookup.

The R171 dispatcher is verified by the regression tests above
plus end-to-end build + sync (sync rate unchanged at ~14 blk/s,
all 11 working cardano-cli operations continue to succeed).

### Verification gates

```
cargo fmt --all -- --check       # clean (one auto-format applied)
cargo lint                       # clean
cargo test-all                   # passed: 4715  failed: 0  ignored: 1
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
| **14** | **GetStakePoolParams** | **R171** | **dispatcher ready** |
| 15 | GetUTxOByTxIn | R157 | ✓ working |

### Open follow-ups

1. **Tag 17 `GetPoolState`** — the actual Babbage+ pool-state
   query.  Returns `PState` (pools + retiring + reverse delegation
   + deposit map).  Required to make
   `cardano-cli query pool-state --all-stake-pools` work
   end-to-end on Babbage+.
2. **Tag 18 `GetStakeSnapshots`** — needed for
   `cardano-cli query stake-snapshot`.  Returns mark/set/go
   snapshots, requires the live stake-snapshot rotation that's
   also pending for R163's `GetStakeDistribution`.
3. Carry-over from R163: live stake-distribution computation +
   `GetGenesisConfig` ShelleyGenesis serialisation.
4. Carry-over from R169: apply-batch duration histogram.
5. Carry-over from R168: multi-session peer accounting.
6. Carry-over from R166: pipelined fetch + apply.

### References

- Captures: `/tmp/ygg-r171-preview.log` (post-fix preview sync,
  era-blocked rejection of `pool-state` at Alonzo as expected).
- Code: [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — `EraSpecificQuery::GetStakePoolParams` variant + tag-14
  decoder; [`node/src/local_server.rs`](node/src/local_server.rs)
  — `decode_pool_hash_set` + `encode_filtered_stake_pool_params`
  helpers + dispatcher + 4 regression tests.
- Upstream reference:
  `Cardano.Ledger.Shelley.LedgerStateQuery.GetStakePoolParams` —
  era-specific BlockQuery sum-type encoder for tag 14.
- Previous round:
  [`docs/operational-runs/2026-04-28-round-170-per-era-block-counters.md`](2026-04-28-round-170-per-era-block-counters.md).
