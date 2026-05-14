## Round 163 — Era-specific query dispatchers (stake-pools, stake-distribution, stake-address-info, genesis-config)

Date: 2026-04-28
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Extend yggdrasil's LSQ dispatcher with handlers for four more
upstream era-specific query tags so they auto-unblock once
`snapshot.current_era` reaches Babbage+ via Round 160's PV-aware
promotion.  Currently cardano-cli's per-era client gating blocks
these at era ≤ Alonzo, but having the dispatcher ready means no
further code changes are needed once a chain reaches Babbage+.

### Implementation

`crates/network/src/protocols/local_state_query_upstream.rs`:

- Extended `EraSpecificQuery` with four new variants:
  - `GetStakeDistribution` (tag 5, `[5]`)
  - `GetFilteredDelegationsAndRewardAccounts` (tag 10,
    `[10, credential_set_cbor]`)
  - `GetGenesisConfig` (tag 11, `[11]`)
  - `GetStakePools` (tag 13, `[13]`)
- Updated `decode_query_if_current` to recognise tags 5/10/11/13.

`node/src/local_server.rs`:

- New `encode_stake_pools_set(snapshot)` emits the upstream
  `Set PoolKeyHash` shape: `tag(258) [* bytes(28)]` per CIP-21
  set tag.
- New `encode_stake_distribution_map(snapshot)` returns an empty
  CBOR map `0xa0` until the live `mark`/`set`/`go` stake-snapshot
  rotation is plumbed (Phase-3 follow-up).
- New `decode_stake_credential_set(bytes)` parses the
  `GetFilteredDelegationsAndRewardAccounts` payload — CBOR set of
  `[kind, hash]` pairs (0=AddrKeyHash, 1=ScriptHash).
- New `encode_filtered_delegations_and_rewards(snapshot, creds)`
  emits the upstream 2-element list `[delegations_map,
  rewards_map]` filtered by the supplied credentials, looking up
  `delegated_pool` from `snapshot.stake_credentials()` and
  `balance` from `snapshot.reward_accounts()`.
- New `encode_stake_credential` helper emits the canonical
  `[kind, hash]` shape on the response side.
- `GetGenesisConfig` returns null for now (Phase-3 follow-up:
  serialise the loaded ShelleyGenesis per
  `Cardano.Ledger.Shelley.Genesis.encCBOR`).
- Dispatcher routes the four new variants to their encoders,
  keeping the era-mismatch envelope wrapping intact.

### Regression tests

`crates/network/src/protocols/local_state_query_upstream.rs`:

- `decode_recognises_stake_pool_distribution_genesis_tags` —
  pins decoding of `[5]` → GetStakeDistribution, `[11]` →
  GetGenesisConfig, `[13]` → GetStakePools, and `[10, creds]` →
  GetFilteredDelegationsAndRewardAccounts.

`node/src/local_server.rs`:

- `get_stake_pools_empty_snapshot_emits_tag_258_empty_set` —
  pins `0xd9 0x01 0x02 0x80` (CBOR tag 258 + empty array).
- `get_stake_distribution_empty_snapshot_emits_empty_map` —
  pins `0xa0`.
- `get_filtered_delegations_empty_snapshot_emits_two_empty_maps`
  — pins `0x82 0xa0 0xa0`.

### Test results

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4710  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4706 (Round 162) → 4710.

### Operational status

The four queries are still era-blocked client-side by cardano-cli
on snapshots reporting Alonzo or earlier:

```
$ cardano-cli query stake-pools --testnet-magic 2
Command failed: query stake-pools
Error: This query is not supported in the era: Alonzo.
```

Preview's chain at slot ~4000 has PV=(6,0) intra-era Alonzo, so
the era-promotion logic correctly reports Alonzo.  Once preview
crosses its first epoch boundary (PV bump to 7), the
PV-promotion will report Babbage and the queries will auto-unblock
through cardano-cli — yggdrasil's dispatcher will respond
directly from `pool_state` / `stake_credentials` / `reward_accounts`.

Mainnet Conway snapshots would respond with the populated pool
set from `pool_state.iter()` directly.

### Cumulative cardano-cli LSQ coverage

| Tag | Query | Round | Status |
|-----|---|---|---|
| 1 | GetEpochNo | R157 | dispatcher ready |
| 3 | GetCurrentPParams | R156 | ✓ working |
| 5 | GetStakeDistribution | **R163** | dispatcher ready (empty map) |
| 6 | GetUTxOByAddress | R157 | ✓ working |
| 7 | GetWholeUTxO | R157 | ✓ working |
| 10 | GetFilteredDelegationsAndRewardAccounts | **R163** | dispatcher ready |
| 11 | GetGenesisConfig | **R163** | dispatcher ready (null) |
| 13 | GetStakePools | **R163** | dispatcher ready |
| 15 | GetUTxOByTxIn | R157 | ✓ working |

### Open follow-ups

1. **Live stake-distribution** — thread the
   `mark`/`set`/`go` stake-snapshot rotation from
   `Cardano.Ledger.Shelley.LedgerState.PState` into the snapshot
   so `GetStakeDistribution` returns each pool's relative stake.
2. **`GetGenesisConfig`** — serialise the loaded
   ShelleyGenesis per upstream `encCBOR`.
3. **Babbage TxOut datum_inline/script_ref** — already correct
   in `BabbageTxOut::encode_cbor` but not yet exercised
   operationally.

### References

- `Cardano.Ledger.Shelley.LedgerStateQuery` — era-specific
  BlockQuery encoder (tag table for `GetStakePools`, etc).
- Previous round: `docs/operational-runs/2026-04-28-round-162-era-history-coverage.md`.
- Code: `crates/network/src/protocols/local_state_query_upstream.rs`,
  `node/src/local_server.rs`.
