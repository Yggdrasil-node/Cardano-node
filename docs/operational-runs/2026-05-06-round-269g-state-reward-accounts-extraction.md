## Round 269g — `state.rs` per-rule split: seventh slice (`RewardAccountState` + `RewardAccounts`)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 seventh slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve. After
`state/{mir,ratify,enact,deposit_pot,phase1_validation,pool_state}.rs`
shipped (R269 a–f), this slice extracts the **reward-account container
types** that mirror upstream `Cardano.Ledger.State.AccountState` and the
DState reward-account portion of the Shelley LedgerState.

### Slice scope

Extracted ~176 source lines from `state.rs` lines 240–415 into
`crates/ledger/src/state/reward_accounts.rs`:

- `pub struct RewardAccountState` — per-account `balance` + optional
  `delegated_pool` reference.
- `impl CborEncode for RewardAccountState` (2-element array codec).
- `impl CborDecode for RewardAccountState`.
- `impl RewardAccountState` accessor + setter methods (`new`, `balance`,
  `delegated_pool`, `set_balance`, `set_delegated_pool`).
- `pub struct RewardAccounts` — `BTreeMap<RewardAccount, RewardAccountState>`.
- `impl CborEncode/CborDecode for RewardAccounts` (array of `[RewardAccount,
  RewardAccountState]` pairs).
- `impl RewardAccounts` map methods (`new`, `get`, `get_mut`, `iter`,
  `len`, `is_empty`, `insert`, `balance`,
  `find_account_by_credential`, `credit_by_credential`).

`state.rs` keeps a `pub mod reward_accounts;` declaration with
`pub use reward_accounts::{RewardAccountState, RewardAccounts};` so all
external callers — `lib.rs`, `stake.rs`, sibling submodules — keep their
existing paths.

### Visibility adjustments

- `encode_optional_pool_key_hash` and `decode_optional_pool_key_hash`
  (top-of-state.rs helpers) promoted from `fn` to `pub(super) fn` so the
  new submodule reaches them via `super::`.
- `RewardAccountState::balance`, `RewardAccountState::delegated_pool`,
  `RewardAccounts::entries` private fields promoted to `pub(super)` to
  preserve the parent-private visibility that `state/tests.rs` relied on.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/reward_accounts.rs::RewardAccountState` | upstream `AccountState` (per-credential balance + delegation) |
| `state/reward_accounts.rs::RewardAccounts` | upstream DState `dsUnified` reward-account map (`Cardano.Ledger.UMap.UMap`) |
| `state/reward_accounts.rs::RewardAccounts::find_account_by_credential` | upstream `lookupAccountState dState dsUnified` |
| `state/reward_accounts.rs::RewardAccounts::credit_by_credential` | upstream reward-distribution helper used by Reward step |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 10,375 | 10,199 | −176 |
| `crates/ledger/src/state/reward_accounts.rs` | (new) | 193 | +193 |

The `+17` net (193 − 176) is the new file's module-level docstring +
imports — actual code body is byte-identical to the original section.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  (MIR)             | `state/mir.rs` (123)              | 110   | 12,596 |
| R269b (Ratify)          | `state/ratify.rs` (675)           | 657   | 11,939 |
| R269c (Enact)           | `state/enact.rs` (362)            | 343   | 11,602 |
| R269d (DepositPot)      | `state/deposit_pot.rs` (124)      | 106   | 11,496 |
| R269e (Phase-1)         | `state/phase1_validation.rs` (817) | 792   | 10,714 |
| R269f (PoolState)       | `state/pool_state.rs` (371)       | 349   | 10,369 |
| **R269g (RewardAccounts)** | **`state/reward_accounts.rs` (193)** | **176** | **10,199** |

Net `state.rs` reduction so far: **12,704 → 10,199 lines (−2,505, ~20 %)** with
seven sibling files.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged)
```

Pure code-move refactor — no test changes; the existing reward-account
CBOR round-trip and `find_account_by_credential` /
`credit_by_credential` tests pass via `super::*` re-exports.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 a–c | shipped | Gap BP narrowed (operator-time-blocked) |
| R269 a–f | shipped | 6 sibling state submodules carved (~2,357 lines moved) |
| **R269g** | **this round** | `state/reward_accounts.rs` extracted: reward-account state + map (176 lines). State.rs cumulative reduction 2,505 lines (12,704 → 10,199). |

### Next R269 slices (queued)

1. **`state/stake_credentials.rs`** — `StakeCredentialState` +
   `StakeCredentials` (~260 lines).
2. **`state/drep_state.rs`** — `RegisteredDrep` + `DrepState` (~180 lines).
3. **`state/committee_state.rs`** — `CommitteeAuthorization`,
   `CommitteeMemberState`, `CommitteeState` (~330 lines).
4. **`state/governance_action_state.rs`** — `GovernanceActionState`
   (~250 lines).
5. **`state/treasury.rs`** — `AccountingState` (~30 lines).
6. **`state/chain_dep.rs`** — `ChainDepStateContext` (~50 lines).
7. PPUP top-of-file helpers, then `LedgerState` per-type files.

### References

- R269 a–f closures: `2026-05-06-round-269{,b,c,d,e,f}-state-*.md`
- Plan: `docs/COMPLETION_ROADMAP.md`
- Upstream AccountState:
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/State/AccountState.hs`
- Upstream DState reward-account map:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs`
