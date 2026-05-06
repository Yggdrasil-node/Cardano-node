## Round 269m — `state.rs` per-rule split: thirteenth slice (`LedgerStateSnapshot`)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 thirteenth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve. After twelve prior slices, this round
extracts **`LedgerStateSnapshot`** — the read-only LSQ capture view of
ledger state. This is the largest single-type slice so far (~374 lines)
because it bundles the snapshot struct (23 fields) with its full LSQ
query-method surface (~25 query methods including `query_utxos_by_txin`,
`query_drep_stake_distribution`, `query_balance`, etc.).

This slice is option (b) from the R269l checkpoint — the natural
continuation of the "per-type extraction" pattern established in
R269 a–l, but bigger than the prior slices because it carries the LSQ
read-side query surface.

### Slice scope

Extracted ~374 source lines from `state.rs` lines 295–668 into
`crates/ledger/src/state/snapshot.rs`:

- `pub struct LedgerStateSnapshot` — 23 fields covering every ledger
  query surface: era, tip, tip block number, current epoch, expected
  network ID, governance actions, pool state, stake credentials,
  committee state, DRep state, reward accounts, dual UTxO views (legacy
  Shelley + multi-era), protocol parameters, deposit pot, treasury
  accounting, enact state, genesis delegations, stability window,
  dormant epoch counter, optional `ChainDepStateContext`, optional
  `StakeSnapshots`.
- `impl LedgerStateSnapshot` accessor methods (~28 total): `current_era`,
  `tip`, `current_epoch`, `latest_block_protocol_version`,
  `tip_block_no`, `expected_network_id`, `governance_action`,
  `governance_actions`, `pool_state`, `stake_credentials`,
  `committee_state`, `drep_state`, `reward_accounts`, `multi_era_utxo`,
  `utxo`, `protocol_params`, `deposit_pot`, `accounting`, `enact_state`,
  `gen_delegs`, `stability_window`, `num_dormant_epochs`,
  `chain_dep_state`, `stake_snapshots`.
- `impl LedgerStateSnapshot` lookup methods: `registered_pool`,
  `stake_credential_state`, `committee_member_state`, `registered_drep`,
  `reward_account_state`, `query_reward_balance`,
  `find_reward_balance_for_credential`.
- `impl LedgerStateSnapshot` LSQ query methods: `query_utxos_by_txin`
  (mirrors upstream `GetUTxOByTxIn`), `query_stake_pool_ids`
  (`GetStakePools`), `query_delegations_and_rewards`
  (`GetFilteredDelegationsAndRewardAccounts`),
  `query_drep_stake_distribution` (`GetDRepStakeDistr`),
  `query_utxos_by_address`, `query_balance`.
- `impl LedgerStateSnapshot` consensus-runtime attachers:
  `with_chain_dep_state` / `with_stake_snapshots` builder methods.

### Wiring

`state.rs` keeps a `pub mod snapshot;` declaration with
`pub use snapshot::LedgerStateSnapshot;` so all external callers
(`lib.rs` re-export, `node/src/local_server.rs::dispatch_query` LSQ
dispatcher, `state/chain_dep.rs` doc references) keep their existing
paths.

### Visibility adjustments

All 23 `LedgerStateSnapshot` private fields promoted from `field: T` to
`pub(super) field: T` so `LedgerState::snapshot(&self) -> LedgerStateSnapshot`
in state.rs can construct the struct directly via field-initialiser
syntax (the existing `LedgerStateSnapshot { current_era: …,  tip: …, … }`
pattern). This avoids introducing a `LedgerStateSnapshot::new(…)`
constructor with 23 positional args.

This continues the visibility-debt pattern called out in R269l —
worth re-tightening once `LedgerState` itself is also carved out.

### Trimmed unused imports

- `crate::eras::mary::MultiAsset` is now only referenced in
  `state/snapshot.rs::query_balance` (via
  `phase1_validation::accumulate_multi_asset(&mut asset_total, …)`)
  and is reachable from snapshot.rs's explicit `use`. Removed from
  state.rs's `use crate::eras::mary::{MultiAsset, Value};` —
  state.rs now uses only `Value`.
- `crate::eras::shelley::ShelleyTxIn` was redundantly imported by
  snapshot.rs's initial `use crate::{Era, eras::shelley::ShelleyTxIn};`
  — the body uses fully-qualified `crate::eras::shelley::ShelleyTxIn`
  in type signatures, so the unqualified import was never needed.
  Trimmed to `use crate::Era;`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/snapshot.rs::LedgerStateSnapshot` | the read-only capture view used by `Ouroboros.Consensus.Shelley.Ledger.Query` LSQ dispatch |
| `state/snapshot.rs::LedgerStateSnapshot::query_utxos_by_txin` | upstream `GetUTxOByTxIn` LSQ query |
| `state/snapshot.rs::LedgerStateSnapshot::query_stake_pool_ids` | upstream `GetStakePools` LSQ query |
| `state/snapshot.rs::LedgerStateSnapshot::query_delegations_and_rewards` | upstream `GetFilteredDelegationsAndRewardAccounts` |
| `state/snapshot.rs::LedgerStateSnapshot::query_drep_stake_distribution` | upstream `GetDRepStakeDistr` |
| `state/snapshot.rs::LedgerStateSnapshot::query_utxos_by_address` / `query_balance` | upstream LSQ helpers used by `cardano-cli query utxo --address` paths |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 9,180 | 8,810 | −370 |
| `crates/ledger/src/state/snapshot.rs` | (new) | 406 | +406 |

The `+36` net (406 − 370) is the new file's module-level docstring +
imports.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  (MIR)             | `state/mir.rs` (123)              | 110   | 12,596 |
| R269b (Ratify)          | `state/ratify.rs` (675)           | 657   | 11,939 |
| R269c (Enact)           | `state/enact.rs` (362)            | 343   | 11,602 |
| R269d (DepositPot)      | `state/deposit_pot.rs` (124)      | 106   | 11,496 |
| R269e (Phase-1)         | `state/phase1_validation.rs` (817) | 792   | 10,714 |
| R269f (PoolState)       | `state/pool_state.rs` (371)       | 349   | 10,369 |
| R269g (RewardAccounts)  | `state/reward_accounts.rs` (193) | 176   | 10,199 |
| R269h (StakeCredentials)| `state/stake_credentials.rs` (280) | 254 | 9,950 |
| R269i (DrepState)       | `state/drep_state.rs` (236)       | 218   | 9,737 |
| R269j (GovActionState)  | `state/governance_action_state.rs` (143) | 116 | 9,621 |
| R269k (CommitteeState)  | `state/committee_state.rs` (394)  | 364   | 9,257 |
| R269l (Treasury+ChainDep)| `state/{treasury,chain_dep}.rs` (49+70) | 77 | 9,180 |
| **R269m (Snapshot)** | **`state/snapshot.rs` (406)**     | **370** | **8,810** |

Net `state.rs` reduction so far: **12,704 → 8,810 lines (−3,894, ~31 %)**
with fourteen sibling files. State.rs is now under 9k lines.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged)
```

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 a–c | shipped | Gap BP narrowed (operator-time-blocked) |
| R269 a–l | shipped | 13 sibling state submodules carved (~3,524 lines moved) |
| **R269m** | **this round** | `state/snapshot.rs` extracted: LedgerStateSnapshot + LSQ query surface (~370 lines). State.rs cumulative reduction 3,894 lines (12,704 → 8,810). |

### Next R269 slices (queued)

1. **`state/checkpoint.rs`** — `LedgerStateCheckpoint` (~400 lines) —
   the natural sibling slice; rollback-safe restore view.
2. **PPUP top-of-file helpers** — still requires scope decision
   (monolithic vs sub-split per upstream rule).
3. **`LedgerState` itself** (~6,500 lines remaining) — the
   structural orchestrator carve; needs deliberate pre-design.

### References

- R269 a–l closures: `2026-05-06-round-269{,b,…,l}-state-*.md`
- Plan: `~/.claude/plans/dapper-giggling-haven.md`
- Upstream LSQ Query view:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/Ouroboros/Consensus/Shelley/Ledger/Query.hs`
