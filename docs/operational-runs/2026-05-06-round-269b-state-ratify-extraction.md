## Round 269b — `state.rs` per-rule split: second slice (Conway RATIFY)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 second slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve of `crates/ledger/src/state.rs`. The
first slice (R269) extracted `InstantaneousRewards` (MIR state) to
`state/mir.rs`. This second slice extracts the entire **Conway RATIFY
rule tally engine** to `state/ratify.rs`.

### Slice scope

Extracted ~669 source lines from `state.rs` lines 11937–12604 into
`crates/ledger/src/state/ratify.rs`:

- `pub struct VoteTally` + `meets_threshold` impl
- `count_active_committee_members` (private helper)
- `pub(crate) fn tally_committee_votes`
- `pub(crate) fn tally_drep_votes`
- `pub(crate) enum DefaultVote`
- `pub(crate) fn default_stake_pool_vote`
- `pub(crate) fn tally_spo_votes`
- `pub(crate) fn drep_threshold_for_action`
- `pub(crate) fn spo_threshold_for_action`
- `pub(crate) fn accepted_by_committee`
- `pub(crate) fn accepted_by_dreps`
- `pub(crate) fn accepted_by_spo`
- `pub(crate) fn ratify_action` (the combined ratification predicate)

`state.rs` keeps a `pub mod ratify;` declaration with a `pub(crate) use
ratify::{...}` re-export of every symbol so all in-crate callers and
the `state/tests.rs` harness keep their existing call paths.

Two private helpers in `state.rs` that the Ratify code calls
(`conway_drep_parameter_change_threshold`,
`conway_parameter_change_has_spo_security_vote_group`) were promoted from
`fn` to `pub(super) fn` so the `state/ratify.rs` sub-module can reach them
via `super::`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/ledger/src/state/ratify.rs::ratify_action` | `Cardano.Ledger.Conway.Rules.Ratify::ratifyTransition` |
| `crates/ledger/src/state/ratify.rs::accepted_by_committee` | `Cardano.Ledger.Conway.Governance.Internal::committeeAccepted` |
| `crates/ledger/src/state/ratify.rs::accepted_by_dreps` | `Cardano.Ledger.Conway.Rules.Ratify::dRepAccepted` |
| `crates/ledger/src/state/ratify.rs::accepted_by_spo` | `Cardano.Ledger.Conway.Rules.Ratify::spoAccepted` |
| `crates/ledger/src/state/ratify.rs::tally_committee_votes` | `Cardano.Ledger.Conway.Rules.Ratify::ccVotesSatisfied` |
| `crates/ledger/src/state/ratify.rs::tally_drep_votes` | `Cardano.Ledger.Conway.Rules.Ratify::dRepVotesSatisfied` |
| `crates/ledger/src/state/ratify.rs::tally_spo_votes` | `Cardano.Ledger.Conway.Rules.Ratify::spoVotesSatisfied` |
| `crates/ledger/src/state/ratify.rs::default_stake_pool_vote` | `Cardano.Ledger.Conway.Governance::defaultStakePoolVote` |
| `crates/ledger/src/state/ratify.rs::drep_threshold_for_action` | `Cardano.Ledger.Conway.Governance.Internal::votingDRepThresholdInternal` |
| `crates/ledger/src/state/ratify.rs::spo_threshold_for_action` | `Cardano.Ledger.Conway.Governance.Internal::votingPoolThresholdInternal` |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 12,596 | 11,939 | −657 |
| `crates/ledger/src/state/ratify.rs` | (new) | 675 | +675 |
| `crates/ledger/src/state/tests.rs` | (unchanged) | +1 | +1 (+ `PoolVotingThresholds` to explicit imports) |

The `+18` net (675 − 657) is the new file's module-level docstring
(`//!`) + `use` imports — the actual code body is byte-identical to
the original section.

A second small change: the implicit `PoolVotingThresholds` re-export
that `state/tests.rs` was relying on (via the parent module's
`use crate::protocol_params::{... PoolVotingThresholds};` inside the
extracted Ratify section) is now an explicit `use` in
`state/tests.rs`. This is upstream-cleaner: each test file declares
the imports it actually depends on.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269 (MIR) | `state/mir.rs` (123 lines) | 110 | 12,596 |
| **R269b (Ratify)** | `state/ratify.rs` (675 lines) | 657 | **11,939** |

Net `state.rs` reduction so far: **12,704 → 11,939 lines (−765)** with
two new sibling files mirroring upstream rule modules.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged from R269)
```

No regression test added or modified — pure code-move refactor; existing
ratification tests in `state/tests.rs` pass unchanged.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 / R266b / R266c | shipped | Gap BP narrowed to deep ScriptContext field encoding (operator-time-blocked) |
| R269 first slice | shipped | `state/mir.rs` extracted (110 lines moved) |
| **R269b** | **this round** | `state/ratify.rs` extracted: full Conway RATIFY tally engine (~657 lines) mirrors upstream `Cardano.Ledger.Conway.Rules.Ratify`. State.rs cumulative reduction 765 lines (12,704 → 11,939). |

### Next R269 slices (queued)

1. **`state/enact.rs`** — Conway ENACT rule (`enact_gov_action` family
   + `enact_gov_action_at_epoch` at `state.rs` lines ~1898–2095).
   Mirrors `Cardano.Ledger.Conway.Rules.Enact`. ~200 lines.
2. **`state/snap.rs`** / **`state/rupd.rs`** / **`state/newepoch.rs`**
   — once epoch-boundary orchestration is similarly carved (currently
   all in `epoch_boundary.rs`).
3. **`state/ppup.rs`** — PPUP helpers section (`state.rs` lines
   22–1691, ~1,670 lines). Mirrors `Cardano.Ledger.Shelley.Rules.Ppup`.
4. **`state/phase1_validation.rs`** — Phase-1 transaction validation
   helpers (`state.rs` lines 11242–12033, ~790 lines).
5. **`state/types.rs`** (or per-type files) — `LedgerState`,
   `LedgerStateSnapshot`, `LedgerStateCheckpoint`, `PoolState`,
   `RewardAccounts`, `StakeCredentials`, `DrepState`, `CommitteeState`,
   `GovernanceActionState` — the bulk of `state.rs`. Larger, requires
   careful submodule layout.

Each remaining slice continues the strict 1:1 filename-mirror refactor
under per-round approval.

### References

- R269 first slice: `2026-05-06-round-269-state-mir-extraction.md`
- Plan: `docs/COMPLETION_ROADMAP.md`
- Upstream RATIFY rule:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Ratify.hs`
- Upstream RATIFY internals:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Internal.hs`
