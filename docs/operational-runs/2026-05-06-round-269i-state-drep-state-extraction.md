## Round 269i — `state.rs` per-rule split: ninth slice (`RegisteredDrep` + `DrepState`)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 ninth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve. After eight prior slices
(`mir`, `ratify`, `enact`, `deposit_pot`, `phase1_validation`, `pool_state`,
`reward_accounts`, `stake_credentials`), this slice extracts the
**Conway DRep registry** that mirrors upstream
`Cardano.Ledger.Conway.Governance::DRepState`.

### Slice scope

Extracted ~218 source lines from `state.rs` lines 263–480 into
`crates/ledger/src/state/drep_state.rs`:

- `pub struct RegisteredDrep` — per-DRep `anchor` (metadata pointer),
  `deposit`, and `last_active_epoch` (Conway DRep activity tracking).
- `impl CborEncode/CborDecode for RegisteredDrep` (3-element array
  codec; back-compat accepts legacy 2-element no-activity form).
- `impl RegisteredDrep` accessors + setters (`new`, `new_active`,
  `anchor`, `deposit`, `last_active_epoch`, `touch_activity`,
  `set_anchor`).
- `pub struct DrepState` — `BTreeMap<DRep, RegisteredDrep>`.
- `impl CborEncode/CborDecode for DrepState` (array of `[DRep,
  RegisteredDrep]` pairs).
- `impl DrepState` map methods + `inactive_dreps` (mirrors upstream
  `drepExpiry` from `Cardano.Ledger.Conway.Rules.Epoch`).

`state.rs` keeps a `pub mod drep_state;` declaration with
`pub use drep_state::{DrepState, RegisteredDrep};` so all external
callers (`lib.rs` re-exports, `state/ratify.rs::super::DrepState`) keep
their existing paths.

### Visibility adjustments

- `encode_optional_anchor`, `decode_optional_anchor` (top-of-state.rs
  helpers) promoted from `fn` to `pub(super) fn`.
- `RegisteredDrep`'s three private fields and `DrepState::entries`
  promoted to `pub(super)` so `state/ratify.rs::tally_drep_votes`
  (which inspects `reg.last_active_epoch` directly) and
  `state/tests.rs` continue to compile.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/drep_state.rs::RegisteredDrep` | `Cardano.Ledger.Conway.Governance::DRepState` |
| `state/drep_state.rs::RegisteredDrep::deposit` | upstream `drepDeposit` |
| `state/drep_state.rs::RegisteredDrep::anchor` | upstream `drepAnchor` |
| `state/drep_state.rs::RegisteredDrep::last_active_epoch` | upstream `drepExpiry` (re-shaped: yggdrasil tracks last-active; upstream tracks expires-at) |
| `state/drep_state.rs::DrepState` | upstream `VState::vsDReps` |
| `state/drep_state.rs::DrepState::inactive_dreps` | upstream `drepExpiry` predicate from `Cardano.Ledger.Conway.Rules.Epoch` |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 9,955 | 9,737 | −218 |
| `crates/ledger/src/state/drep_state.rs` | (new) | 236 | +236 |

The `+18` net (236 − 218) is the new file's module-level docstring +
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
| **R269i (DrepState)** | **`state/drep_state.rs` (236)**     | **218** | **9,737** |

Net `state.rs` reduction so far: **12,704 → 9,737 lines (−2,967, ~23 %)**
with nine sibling files.

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
| R269 a–h | shipped | 8 sibling state submodules carved (~2,787 lines moved) |
| **R269i** | **this round** | `state/drep_state.rs` extracted: Conway DRep registry + `inactive_dreps` predicate (218 lines). State.rs cumulative reduction 2,967 lines (12,704 → 9,737). |

### Next R269 slices (queued)

1. **`state/committee_state.rs`** — `CommitteeAuthorization`,
   `CommitteeMemberState`, `CommitteeState` (~330 lines).
2. **`state/governance_action_state.rs`** — `GovernanceActionState`
   (~250 lines).
3. **`state/treasury.rs`** — `AccountingState` (~30 lines).
4. **`state/chain_dep.rs`** — `ChainDepStateContext` (~50 lines).
5. PPUP top-of-file helpers + `LedgerState` per-type files.

### References

- R269 a–h closures: `2026-05-06-round-269{,b,c,d,e,f,g,h}-state-*.md`
- Plan: `~/.claude/plans/dapper-giggling-haven.md`
- Upstream DRepState:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance.hs`
- Upstream `drepExpiry` rule:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Epoch.hs`
