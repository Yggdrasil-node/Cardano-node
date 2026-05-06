## Round 269o — `state.rs` per-rule split: fifteenth slice (PPUP helpers)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 fifteenth slice — strict 1:1 with upstream Haskell)

### Context

Continuing R269's per-rule carve. After fourteen prior slices, this round
extracts the **PPUP (Protocol Parameter Update Proposal) helpers** that
sat at the very top of `state.rs`. Picked option (a) from the R269n
checkpoint — monolithic single-file PPUP slice rather than a sub-split,
because the section is short and tightly cohesive (4 items, ~57 lines).

The general-purpose `encode_optional_*` / `decode_optional_*` CBOR
helper family stays in `state.rs` for now: those are used by every
sibling sub-module via `super::` and re-locating them would force a
cascade of additional `pub(super) use` statements. They could land in
their own `state/cbor_helpers.rs` slice in a future round if the
visibility-debt cleanup decides to consolidate them.

### Slice scope

Extracted ~57 source lines from `state.rs` lines 135–195 into
`crates/ledger/src/state/ppup.rs`:

- `pub struct PpupSlotContext` — slot-based context for upstream
  `getTheSlotOfNoReturn` validation (current slot, first slot of next
  epoch, stability window).
- `pub fn pv_can_follow` — upstream `pvCanFollow` predicate
  (legal-successor protocol-version check: major+1 with minor=0, or
  same major with minor+1).
- `pub fn overlay_step` — d-overlay slot-step formula
  (`offset × numerator ÷ denominator`, ceiling division). Used by the
  pre-Praos blocks-made counting rule.
- `pub fn is_overlay_slot_for_blocks_made` — predicate built on top
  of `overlay_step` to determine whether a slot belongs to the
  d-overlay window where genesis-delegate blocks are issued.

`state.rs` keeps a `pub mod ppup;` declaration with
`pub use ppup::{PpupSlotContext, pv_can_follow};` (matching the
prior `lib.rs` re-export of the public symbols) plus a second
`pub use ppup::{is_overlay_slot_for_blocks_made, overlay_step};`
re-export so existing in-state.rs unqualified callers and
`state/tests.rs::*overlay_slot_*` regressions keep working via
`use super::*;`.

### Visibility note: `pub fn` not `pub(super) fn`

`overlay_step` and `is_overlay_slot_for_blocks_made` were originally
private (`fn ...`). Rust does not allow a `pub(super) fn` item to be
re-exported across module boundaries via `pub(super) use`, so the
items had to be promoted from the planned `pub(super) fn` to plain
`pub fn` in `state/ppup.rs`. The state.rs `pub use ppup::{...}`
re-exports them at the `crate::state::*` path — slightly more
visible than strictly necessary (these are math helpers used only
internally) but workable. Worth noting for the visibility-debt
cleanup round queued in R269l: a `pub(crate) fn` in `state/ppup.rs`
combined with `pub(crate) use ppup::{...}` would tighten this
back down to crate-private once the carve settles.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/ppup.rs::PpupSlotContext` | upstream `Cardano.Ledger.Slot::getTheSlotOfNoReturn` slot context |
| `state/ppup.rs::pv_can_follow` | upstream `Cardano.Ledger.Shelley.PParams::pvCanFollow` |
| `state/ppup.rs::overlay_step` | yggdrasil-only mirror of the d-overlay step formula used by upstream `overlaySchedule` and pre-Praos blocks-made counting |
| `state/ppup.rs::is_overlay_slot_for_blocks_made` | upstream `isOverlaySlot` predicate from the d-overlay schedule |

The PPUP rule itself (`Cardano.Ledger.Shelley.Rules.Ppup`) lives at
`crates/ledger/src/state.rs::LedgerState::validate_ppup_proposal` —
not moved in this round; it stays where the bulk of `LedgerState`
apply logic lives. A future round (after `LedgerState` carve) can
move it to a per-rule `state/rules/ppup.rs` mirroring the upstream
file path.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 8,766 | 8,716 | −50 |
| `crates/ledger/src/state/ppup.rs` | (new) | 81 | +81 |

The `+31` net is the new file's module-level docstring + imports.

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
| R269m (Snapshot)        | `state/snapshot.rs` (406)         | 370   | 8,810 |
| R269n (Checkpoint)      | `state/checkpoint.rs` (70)        | 50    | 8,766 |
| **R269o (PPUP helpers)** | **`state/ppup.rs` (81)**          | **50**  | **8,716** |

Net `state.rs` reduction so far: **12,704 → 8,716 lines (−3,988, ~31 %)**
with sixteen sibling files. State.rs is approaching the structural floor
where the remaining mass is concentrated in `LedgerState` itself + its
~hundreds of methods.

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
| R269 a–n | shipped | 15 sibling state submodules carved (~3,938 lines moved) |
| **R269o** | **this round** | `state/ppup.rs` extracted: PPUP slot-context + protocol-version + d-overlay helpers (50 lines). State.rs cumulative reduction 3,988 lines (12,704 → 8,716). |

### Stop point — only structural carve work remains

After R269o, the per-type and per-rule sibling files are essentially
done. State.rs's remaining ~8,700 lines are dominated by:

- **`LedgerState` struct** (~140 lines).
- **`impl CborEncode/CborDecode for LedgerState`** (~310 lines, the big
  state-serialization codec).
- **`impl LedgerState`** method block (~7,800 lines, hundreds of
  per-era apply methods, governance helpers, query helpers, etc.).
- **A handful of private helper functions** still in state.rs body.
- **Free functions** like `enact_gov_action`'s top-level wrapper
  callers, era-min-protocol-major helpers, conway_* governance
  predicates, etc.

This is the structural orchestrator — extracting it requires a
deliberate scope decision, not another quick slice.

### Next R-round options (require user decision, not auto-proceed)

| Option | Approx effort | Surface change |
|---|---|---|
| (a) `LedgerState` carve — split impl block by era (apply_byron / apply_shelley / …) | ~3 days; explicit pre-design | ~7,800-line refactor |
| (b) `LedgerState` carve — split by Conway rule (utxo / cert / pool / deleg / …) mirroring upstream `Cardano.Ledger.Conway.Rules.*` | ~4 days; bigger pre-design | better upstream-mirror, riskier |
| (c) `LedgerState` carve — minimal: extract just the CBOR codec to `state/cbor.rs`, leave impl methods in state.rs | ~1 day; safer | only ~310-line move |
| (d) Visibility-debt cleanup (re-tighten accumulated `pub(super)` / `pub` field promotions) | ~1 day | no functional change; safer post-LedgerState carve |
| (e) Pivot back to **Gap BP root cause (R266d)** — operator-time Haskell preview sync | hours-to-days operator wall-clock | unblocks the open protocol-parity gap |
| (f) Pivot to **R267 mainnet endurance rehearsal** — operator-time | 24 h+ wall-clock | unblocks mainnet-side parity proof |

### References

- R269 a–n closures: `2026-05-06-round-269{,b,…,n}-state-*.md`
- Plan: `~/.claude/plans/dapper-giggling-haven.md`
- Upstream PPUP rule:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ppup.hs`
- Upstream pvCanFollow:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/PParams.hs`
- Upstream overlaySchedule:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Tickn.hs`
