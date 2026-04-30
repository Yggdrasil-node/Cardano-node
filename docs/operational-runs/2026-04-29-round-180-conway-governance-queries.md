## Round 180 — Conway governance LSQ queries (constitution, gov-state, drep-state, account-state)

Date: 2026-04-29
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Extend R179's era-blockage fix with dispatchers for the
remaining Conway-era governance queries cardano-cli surfaces
under `cardano-cli conway query ...`:

- `cardano-cli conway query constitution` → tag 23
- `cardano-cli conway query gov-state` → tag 24
- `cardano-cli conway query drep-state` → tag 25
- `cardano-cli` treasury / account-state introspection → tag 29

yggdrasil's `LedgerStateSnapshot` already tracks all four data
sources (`enact_state.constitution()`,
`governance_actions()`, `drep_state()`, `accounting()`); the
gap was just the wire dispatcher.

### Code change

`crates/network/src/protocols/local_state_query_upstream.rs`:

- New `EraSpecificQuery` variants: `GetConstitution`,
  `GetGovState`, `GetDRepState { credential_set_cbor }`,
  `GetAccountState`.
- `decode_query_if_current` recognises tags
  `(1, 23) → GetConstitution`,
  `(1, 24) → GetGovState`,
  `(2, 25) → GetDRepState`,
  `(1, 29) → GetAccountState`.

`node/src/local_server.rs`:

- Four new dispatcher arms wrapping existing snapshot
  encoders:
  - `GetConstitution` → `snapshot.enact_state().constitution().encode_cbor()`.
  - `GetGovState` → CBOR map of `governance_actions()` (id →
    `GovernanceActionState`).
  - `GetDRepState` → `snapshot.drep_state().encode_cbor()`
    (credential filter accepted but not applied; cardano-cli
    filters client-side).
  - `GetAccountState` → 2-element list
    `[treasury, reserves]` from `accounting()`.

### Operational verification

#### `cardano-cli conway query constitution` — ✓ working

After 15s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query constitution --testnet-magic 2
{
    "anchor": {
        "dataHash": "ca41a91f399259bcefe57f9858e91f6d00e1a38d6d9c63d4052914ea7bd70cb2",
        "url": "ipfs://bafkreifnwj6zpu3ixa4siz2lndqybyc5wnnt3jkwyutci4e2tmbnj3xrdm"
    },
    "script": "fa24fb305126805cf2164c161d852a0e7330cf988f1fe558cf7d4a64"
}
```

This is the actual current Conway constitution from preview's
chain state — yggdrasil tracked the post-Babbage governance
data and the dispatcher delivered it end-to-end through
cardano-cli 10.16.

#### `cardano-cli conway query stake-pools` — ✓ now returns real pools

After enough sync to reach the Shelley era, the R179 stake-pools
dispatcher returns the actual pool set:

```
$ cardano-cli conway query stake-pools --testnet-magic 2
[
    "pool18r62tz408lkgfu6pq5svwzkh2vslkeg6mf72qf3h8njgvzhx9ce",
    "pool1grvqd4eu354qervmr62uew0nsrjqedx5kglldeqr4c29vv59rku",
    ...
]
```

Confirms R179's tag-corrected stake-pools dispatcher works on
real chain data.

#### `cardano-cli conway query stake-snapshot` — ✓ working

```
$ cardano-cli conway query stake-snapshot --all-stake-pools --testnet-magic 2
{
    "pools": {
        "38f4a58aaf3fec84f3410520c70ad75321fb651ada7ca026373ce486": {
            "stakeGo": 0,
            "stakeMark": 0,
            "stakeSet": 0
        },
        ...
    }
}
```

The R173/R179 GetCBOR-wrapped GetStakeSnapshots dispatcher
returns real per-pool entries (zero placeholders for
mark/set/go pending the live snapshot rotation plumbing).

#### Pending body-shape work

`gov-state` and `committee-state` and `drep-state --all-dreps`
fail at depth 3 with `expected list len or indef` /
`expected map len or indef` — the dispatcher tags route
correctly, but yggdrasil's existing inner encoders for
`governance_actions`, `drep_state`, `committee_state` use
shapes that don't match cardano-cli 10.16's Conway decoders.

These are **shape-mismatch issues** (the wire path now
arrives), tracked as a follow-up requiring upstream Conway
governance encoder reference.  R180's dispatcher arms are
already in place; only the body shape needs adjustment.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4737  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

Test count progression: 4736 → **4737** (added
`decode_recognises_conway_governance_tags` covering all four
new tag dispatches: 23 GetConstitution, 24 GetGovState,
25 GetDRepState, 29 GetAccountState).

### Updated cumulative cardano-cli LSQ era-specific tag coverage

| Tag | Query | Round | Status |
|-----|---|---|---|
|  1 | GetEpochNo | R157 | dispatcher ready |
|  3 | GetCurrentPParams | R156 | ✓ working |
|  5 | GetStakeDistribution | R163 | dispatcher ready |
|  6 | GetUTxOByAddress | R157 | ✓ working |
|  7 | GetUTxOWhole | R157 | ✓ working |
|  9 | GetCBOR (wrapper) | R179 | ✓ working |
| 10 | GetFilteredDelegationsAndRewardAccounts | R163 | dispatcher ready |
| 11 | GetGenesisConfig | R163 | dispatcher ready (null) |
| 15 | GetUTxOByTxIn | R157 | ✓ working |
| 16 | GetStakePools | R163 (R179 tag) | ✓ working |
| 17 | GetStakePoolParams | R171 (R179 tag) | dispatcher ready |
| 19 | GetPoolState | R172 (R179 tag) | ✓ working (via GetCBOR) |
| 20 | GetStakeSnapshots | R173 (R179 tag) | ✓ working (via GetCBOR) |
| **23** | **GetConstitution** | **R180** | **✓ working** |
| **24** | **GetGovState** | **R180** | dispatcher ready (body shape TBD) |
| **25** | **GetDRepState** | **R180** | dispatcher ready (body shape TBD) |
| **29** | **GetAccountState** | **R180** | dispatcher ready (untested) |
| 37 | GetStakeDistribution2 | R179 | ✓ working |

### Open follow-ups

1. **GovState / DRepState / CommitteeState body shape** —
   align the inner encoders with cardano-cli 10.16's Conway
   decoders (currently failing at depth 3 with type-mismatch
   errors; dispatcher tags are correct, only shapes need
   work).
2. **Live stake-snapshot plumbing** for non-placeholder
   stake-distribution / stake-snapshot data (long-pending
   R163/R173 follow-up).
3. **`GetGenesisConfig` ShelleyGenesis serialisation** (R163).
4. **Apply-batch duration histogram** (R169).
5. **Multi-session peer accounting** (R168 structural).
6. **Pipelined fetch + apply** (R166).
7. **Deep cross-epoch rollback recovery** (R167).
8. Tag 21 `GetPoolDistr` / 22 `GetStakeDelegDeposits` /
   26 `GetDRepStakeDistr` / 27 `GetCommitteeMembersState` /
   30 `GetSPOStakeDistr` / 31 `GetProposals` /
   32 `GetRatifyState` / 33 `GetFuturePParams` — additional
   Conway-era dispatchers for completeness.

### References

- Code:
  [`crates/network/src/protocols/local_state_query_upstream.rs`](crates/network/src/protocols/local_state_query_upstream.rs)
  — four new `EraSpecificQuery` variants + decoder branches +
  `decode_recognises_conway_governance_tags` regression test;
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  four new dispatcher arms reusing snapshot accessors.
- Captures: `/tmp/ygg-r180-preview.log`
  (`cardano-cli conway query constitution` returns real
  Conway constitution data; `query stake-pools` returns real
  pool set; `query stake-snapshot --all-stake-pools` returns
  real per-pool entries).
- Upstream reference:
  `Ouroboros.Consensus.Shelley.Ledger.Query.encodeShelleyQuery`
  (tag table); `Cardano.Ledger.Conway.Governance.Constitution`;
  `Cardano.Ledger.Conway.LedgerStateQuery.GetDRepState`.
- Previous round:
  [`docs/operational-runs/2026-04-29-round-179-era-blockage-end-to-end.md`](2026-04-29-round-179-era-blockage-end-to-end.md).
