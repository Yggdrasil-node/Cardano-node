## Round 269h — `state.rs` per-rule split: eighth slice (`StakeCredentialState` + `StakeCredentials`)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 eighth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve. After
`state/{mir,ratify,enact,deposit_pot,phase1_validation,pool_state,reward_accounts}.rs`
shipped (R269 a–g), this slice extracts the **stake-credential registry**
that mirrors upstream `Cardano.Ledger.Shelley.LedgerState::DState`'s
`dsUnified` map.

### Slice scope

Extracted ~254 source lines from `state.rs` lines 257–510 into
`crates/ledger/src/state/stake_credentials.rs`:

- `pub struct StakeCredentialState` — per-credential `delegated_pool`,
  `delegated_drep`, and `deposit` (upstream `rdDeposit`).
- `impl CborEncode/CborDecode for StakeCredentialState` (3-element array
  codec; back-compat accepts legacy 2-element no-deposit form).
- `impl StakeCredentialState` accessors + setters (`new`,
  `new_with_deposit`, `deposit`, `delegated_pool`, `delegated_drep`,
  `set_delegated_pool`, `set_delegated_drep`).
- `pub struct StakeCredentials` — `BTreeMap<StakeCredential,
  StakeCredentialState>`.
- `impl CborEncode/CborDecode for StakeCredentials` (array of
  `[StakeCredential, StakeCredentialState]` pairs).
- `impl StakeCredentials` map methods + cleanup helpers:
  `new`, `get`, `get_mut`, `iter`, `len`, `is_empty`,
  `is_registered`, `register`, `register_with_deposit`, `unregister`,
  `clear_drep_delegation` (mirrors upstream
  `clearDRepDelegations`), `cleanup_dangling_drep_delegations`
  (mirrors upstream `updateDRepDelegations`),
  `clear_pool_delegations` (mirrors upstream
  `removeStakePoolDelegations`).

`state.rs` keeps a `pub mod stake_credentials;` declaration with
`pub use stake_credentials::{StakeCredentialState, StakeCredentials};` so
all external callers (`lib.rs` re-exports, `stake.rs`,
`state/ratify.rs::super::StakeCredentials`) keep their existing paths.

### Visibility adjustments

- `encode_optional_drep`, `decode_optional_drep`, `is_builtin_drep`
  (top-of-state.rs helpers) promoted from `fn` to `pub(super) fn` so the
  new submodule reaches them via `super::`.
- `StakeCredentialState`'s three private fields and
  `StakeCredentials::entries` promoted to `pub(super)` to preserve the
  parent-private visibility that sibling submodules and
  `state/tests.rs` rely on (only the `cleanup_dangling_drep_delegations`
  + `clear_*_delegations` impls need the field-level access; tests use
  accessor methods exclusively).

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/stake_credentials.rs::StakeCredentialState` | upstream `Cardano.Ledger.UMap::RDPair` (delegations + deposit triple) |
| `state/stake_credentials.rs::StakeCredentialState::deposit` | upstream `rdDeposit` |
| `state/stake_credentials.rs::StakeCredentialState::delegated_pool` | upstream `sPoolMap` value |
| `state/stake_credentials.rs::StakeCredentialState::delegated_drep` | upstream `dRepMap` value |
| `state/stake_credentials.rs::StakeCredentials` | upstream `Cardano.Ledger.Shelley.LedgerState::DState::dsUnified` |
| `state/stake_credentials.rs::StakeCredentials::clear_drep_delegation` | upstream `clearDRepDelegations` (`Cardano.Ledger.Conway.Rules.GovCert`) |
| `state/stake_credentials.rs::StakeCredentials::cleanup_dangling_drep_delegations` | upstream `updateDRepDelegations` (`Cardano.Ledger.Conway.Rules.HardFork`, PV 9→10 transition) |
| `state/stake_credentials.rs::StakeCredentials::clear_pool_delegations` | upstream `removeStakePoolDelegations` (`Cardano.Ledger.Shelley.Rules.PoolReap`) |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 10,204 | 9,950 | −254 |
| `crates/ledger/src/state/stake_credentials.rs` | (new) | 280 | +280 |

The `+26` net (280 − 254) is the new file's module-level docstring +
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
| R269g (RewardAccounts)  | `state/reward_accounts.rs` (193) | 176   | 10,199 |
| **R269h (StakeCredentials)** | **`state/stake_credentials.rs` (280)** | **254** | **9,950** |

Net `state.rs` reduction so far: **12,704 → 9,950 lines (−2,754, ~22 %)** with
eight sibling files. State.rs is now under 10k lines for the first time
since its R256 Phase H consolidation.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged)
```

Pure code-move refactor — no test changes. The 26 stake-credential tests
(register/unregister/dual-delegation/clear-on-retirement/CBOR round-trip)
keep behaviour via `super::*` re-exports.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 a–c | shipped | Gap BP narrowed (operator-time-blocked) |
| R269 a–g | shipped | 7 sibling state submodules carved (~2,533 lines moved) |
| **R269h** | **this round** | `state/stake_credentials.rs` extracted: per-credential delegation registry + 3 cleanup helpers (254 lines). State.rs cumulative reduction 2,754 lines (12,704 → 9,950). |

### Next R269 slices (queued)

1. **`state/drep_state.rs`** — `RegisteredDrep` + `DrepState` (~180 lines).
2. **`state/committee_state.rs`** — `CommitteeAuthorization`,
   `CommitteeMemberState`, `CommitteeState` (~330 lines).
3. **`state/governance_action_state.rs`** — `GovernanceActionState`
   (~250 lines).
4. **`state/treasury.rs`** — `AccountingState` (~30 lines).
5. **`state/chain_dep.rs`** — `ChainDepStateContext` (~50 lines).
6. PPUP top-of-file helpers + `LedgerState` per-type files.

### References

- R269 a–g closures: `2026-05-06-round-269{,b,c,d,e,f,g}-state-*.md`
- Plan: `~/.claude/plans/dapper-giggling-haven.md`
- Upstream DState:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState.hs`
- Upstream UMap (`RDPair` / `dsUnified`):
  `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/UMap.hs`
- Upstream cleanup helpers:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/{GovCert,HardFork}.hs`
- Upstream `removeStakePoolDelegations`:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/PoolReap.hs`
