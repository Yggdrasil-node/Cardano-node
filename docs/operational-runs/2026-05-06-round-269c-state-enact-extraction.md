## Round 269c — `state.rs` per-rule split: third slice (Conway ENACT)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 third slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve of `crates/ledger/src/state.rs`. After
`state/mir.rs` (R269) and `state/ratify.rs` (R269b), this slice extracts
the **Conway ENACT rule** + the `EnactState` record + `EnactOutcome` enum.

### Slice scope

Extracted ~343 source lines from `state.rs` lines 1716–2058 into
`crates/ledger/src/state/enact.rs`:

- `pub struct EnactState` (the upstream `Cardano.Ledger.Conway.Governance.EnactState`
  record: constitution, committee_quorum, has_committee, prev_pparams_update,
  prev_hard_fork, prev_committee, prev_constitution).
- `impl Default for EnactState`, `impl CborEncode for EnactState`,
  `impl CborDecode for EnactState`.
- `impl EnactState` methods (`new`, `constitution`, `committee_quorum`,
  `prev_*` getters, `enacted_root` for purpose-group lineage lookup).
- `pub enum EnactOutcome` — informational result of enacting a single
  governance action.
- `pub fn enact_gov_action` — public entry point that wraps
  `enact_gov_action_at_epoch` with `EpochNo(0)`.
- `pub(super) fn enact_gov_action_at_epoch` — the actual ENACT rule
  dispatcher for each `crate::eras::conway::GovAction` variant.

`state.rs` keeps a `pub mod enact;` declaration with
`pub use enact::{EnactOutcome, EnactState, enact_gov_action};` so all
external callers (in `crate::state` namespace) keep their existing path.

`LedgerState::enact_action` (the in-`LedgerState` wrapper) now calls
`enact::enact_gov_action_at_epoch` via the qualified module path.

### Visibility adjustments

Two top-of-`state.rs` private helpers were promoted from `fn` to
`pub(super) fn` so the new submodule reaches them via `super::`:

- `encode_optional_gov_action_id`
- `decode_optional_gov_action_id`

These are used by `EnactState`'s CBOR codec to encode the four `Option<GovActionId>`
lineage fields. They're also used by other state.rs encoders (committee /
governance action state) so the `pub(super)` scope is sufficient.

Two test call sites in `state/tests.rs` updated from unqualified
`enact_gov_action_at_epoch(...)` to qualified `super::enact::enact_gov_action_at_epoch(...)`.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `crates/ledger/src/state/enact.rs::EnactState` | `Cardano.Ledger.Conway.Governance.EnactState` |
| `crates/ledger/src/state/enact.rs::EnactOutcome` | (yggdrasil-only — informational return type for tracing; upstream uses `runEnactState` directly) |
| `crates/ledger/src/state/enact.rs::enact_gov_action` | `Cardano.Ledger.Conway.Rules.Enact::enactTransition` (entry point) |
| `crates/ledger/src/state/enact.rs::enact_gov_action_at_epoch` | `Cardano.Ledger.Conway.Rules.Enact::enactTransition` (per-action dispatch) |
| `crates/ledger/src/state/enact.rs::EnactState::enacted_root` | `Cardano.Ledger.Conway.Governance::prevGovActionIds` |

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 11,945 | 11,602 | −343 |
| `crates/ledger/src/state/enact.rs` | (new) | 362 | +362 |
| `crates/ledger/src/state/tests.rs` | (unchanged) | (unchanged) | (2 unqualified call sites → qualified) |

The `+19` net (362 − 343) is the new file's module-level docstring
(`//!`) + `use` imports — the actual code body is byte-identical to
the original section.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  (MIR)    | `state/mir.rs` (123)   | 110 | 12,596 |
| R269b (Ratify) | `state/ratify.rs` (675) | 657 | 11,939 |
| **R269c (Enact)** | **`state/enact.rs` (362)** | 343 | **11,602** |

Net `state.rs` reduction so far: **12,704 → 11,602 lines (−1,102)** with
three new sibling files (`mir.rs`, `ratify.rs`, `enact.rs`).

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (unchanged from R269b)
```

Pure code-move refactor — no test changes; existing ENACT tests in
`state/tests.rs` (`outcome = enact_gov_action_at_epoch(...)`) keep
behaviour with two call sites qualified to `super::enact::...`.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 / R266b / R266c | shipped | Gap BP narrowed to deep ScriptContext field encoding (operator-time-blocked) |
| R269 (MIR)    | shipped | `state/mir.rs` extracted (110 lines moved) |
| R269b (Ratify) | shipped | `state/ratify.rs` extracted (657 lines moved) |
| **R269c (Enact)** | **this round** | `state/enact.rs` extracted: ENACT rule + `EnactState` lineage record (343 lines). State.rs cumulative reduction 1,102 lines (12,704 → 11,602). |

### Next R269 slices (queued)

1. **`state/ppup.rs`** — PPUP helpers section (`state.rs` lines
   22–~1620, ~1,600 lines). Mirrors `Cardano.Ledger.Shelley.Rules.Ppup` /
   `Cardano.Ledger.Shelley.Rules.Newpp`. Largest remaining slice.
2. **`state/phase1_validation.rs`** — Phase-1 transaction validation
   helpers (~790 lines).
3. Per-type files for `LedgerState`, `LedgerStateSnapshot`,
   `LedgerStateCheckpoint`, `PoolState`, `RewardAccounts`,
   `StakeCredentials`, `DrepState`, `CommitteeState`,
   `GovernanceActionState`. The remaining structural bulk.

### References

- R269 first slice: `2026-05-06-round-269-state-mir-extraction.md`
- R269 second slice: `2026-05-06-round-269b-state-ratify-extraction.md`
- Plan: `docs/COMPLETION_ROADMAP.md`
- Upstream ENACT rule:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Enact.hs`
- Upstream EnactState record:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance.hs`
