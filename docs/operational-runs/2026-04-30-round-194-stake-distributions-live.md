## Round 194 — Live DRep / SPO stake distributions + stake-deleg deposits (Phase A.4)

Date: 2026-04-30
Branch: main
Build: `target/release/yggdrasil-node` (Cargo `release` profile)

### Goal

Phase A.4 — replace empty-map placeholders in three LSQ
queries with live data computed from yggdrasil's snapshot:
1. `query drep-stake-distribution` — `Map DRep Coin`
2. `query spo-stake-distribution` — `Map (KeyHash 'StakePool) Coin`
3. `GetStakeDelegDeposits` (tag 22) — `Map (Credential 'Staking) Coin`

All three data sources are already tracked in
`LedgerStateSnapshot` via `stake_credentials()`,
`reward_accounts()`, and `query_drep_stake_distribution()`.
Only the LSQ encoders had been hardcoded to emit empty maps.

### Code change

`node/src/local_server.rs`:

- New `encode_drep_stake_distribution_for_lsq(snapshot)` —
  uses the existing `LedgerStateSnapshot::query_drep_stake_distribution()`
  helper which iterates stake credentials, looks up each
  one's `delegated_drep`, and sums reward balances per DRep.
  Mirrors upstream
  `Cardano.Ledger.Conway.LedgerStateQuery.queryDRepStakeDistr`.
- New `encode_spo_stake_distribution_for_lsq(snapshot)` —
  iterates stake credentials, looks up each one's
  `delegated_pool`, finds the credential's reward balance via
  `RewardAccounts::iter()`, sums per pool into a
  `BTreeMap<[u8;28], u64>` keyed by 28-byte cold-key hash for
  deterministic output.
- New `encode_stake_deleg_deposits_for_lsq(snapshot)` —
  iterates stake credentials emitting `(credential, deposit)`
  map.  Reads `StakeCredentialState::deposit()` (R67's
  `rdDeposit` per upstream `UMap`).
- Three dispatcher arms updated to call the helpers
  (`GetDRepStakeDistr` / `GetSPOStakeDistr` /
  `GetStakeDelegDeposits`).

### Operational verification

After ~25s of preview sync with `YGG_LSQ_ERA_FLOOR=6`:

```
$ cardano-cli conway query drep-stake-distribution \
      --testnet-magic 2 --all-dreps
{}

$ cardano-cli conway query spo-stake-distribution \
      --testnet-magic 2 --all-spos
[
    [
        "38f4a58aaf3fec84f3410520c70ad75321fb651ada7ca026373ce486",
        0,
        null
    ],
    [
        "40d806d73c8d2a0c8d9b1e95ccb9f380e40cb4d4b23ff6e403ae1456",
        0,
        null
    ],
    [
        "d5cfc42cf67f6b637688d19fa50a4342658f63370b9e2c9e3eaf4dfe",
        0,
        null
    ]
]
```

**SPO stake distribution now surfaces all three preview-registered
pools with their real cold-key hashes** — the placeholder
empty list `[]` from R184 is replaced with live pool data.
Stake amounts remain 0 because preview's chain at slot ~3960
hasn't begun rewarding stake (no delegated stake credentials
yet).

DRep distribution returns `{}` correctly — preview's chain
has no DRep delegations.  When DRep registrations and stake
delegations occur, the same encoder will surface live values.

Regression checks pass:

```
$ cardano-cli conway query gov-state --testnet-magic 2
{ "committee": null, ... }

$ cardano-cli conway query ratify-state --testnet-magic 2
{ "enactedGovActions": [], ... }

$ cardano-cli conway query ledger-peer-snapshot --testnet-magic 2
{ "bigLedgerPools": [], ... }

$ cardano-cli conway query protocol-state --testnet-magic 2
{ "candidateNonce": null, ... }
```

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo lint                       # clean
cargo test-all                   # passed: 4744  failed: 0  ignored: 1
cargo build --release -p yggdrasil-node    # clean
```

### Open follow-ups

Continuing the data-plumbing arc:

1. **Phase A.5** — ledger-peer-snapshot pool list from peer
   governor's big-ledger ranking.
2. **Phase A.6** — `GetGenesisConfig` ShelleyGenesis serialiser.
3. **Phase A.2 (deferred)** — runtime nonce attach via Arc
   publish channel.
4. **Phase A.3 next** — gov-state OMap proposals (requires
   `GovActionState` shape adaptation).
5. **Phase B** — R91 multi-peer dispatch storage livelock.
6. **Phase C/D/E** — sync perf, deep rollback, mainnet
   rehearsal.

### References

- Plan:
  [`/home/vscode/.claude/plans/clever-shimmying-quokka.md`](/home/vscode/.claude/plans/clever-shimmying-quokka.md).
- Code:
  [`node/src/local_server.rs`](node/src/local_server.rs) —
  three new encoder helpers + three dispatcher arms updated.
- Upstream reference:
  `Cardano.Ledger.Conway.LedgerStateQuery.queryDRepStakeDistr`;
  `Cardano.Ledger.Conway.LedgerStateQuery.querySPOStakeDistr`;
  `Cardano.Ledger.Shelley.LedgerStateQuery.queryStakeDelegDeposits`.
- Yggdrasil reference:
  `LedgerStateSnapshot::query_drep_stake_distribution`,
  `stake_credentials`, `reward_accounts`,
  `StakeCredentialState::deposit`.
- Previous round:
  [`docs/operational-runs/2026-04-30-round-193-gov-relation-live.md`](2026-04-30-round-193-gov-relation-live.md).
