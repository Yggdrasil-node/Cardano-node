## Round 269q — `state.rs` per-era split: seventeenth slice (Byron apply)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 seventeenth slice — strict 1:1 with upstream Haskell, **per-era impl carve begins**)

### Context

R269p closed the per-type-and-codec sibling carve (16 sibling submodules
under `state/*.rs`). The remaining ~7,800-line `impl LedgerState` block
in `state.rs` is dominated by per-era apply methods (`apply_byron_block`,
`apply_shelley_block`, …, `apply_conway_block`) plus governance helpers
plus query helpers. Per the user-confirmed plan in
`~/.claude/plans/playful-tickling-plum.md`, R269q starts the per-era
impl split — option (a) from R269o's stop-point menu — by extracting
the smallest era first as a validation slice for the split pattern.

### Slice scope

Extracted ~33 source lines from `state.rs::impl LedgerState` into a new
`crates/ledger/src/state/eras/byron.rs` (60 lines including module
docstring + imports).

Created the per-era directory infrastructure:

- `crates/ledger/src/state/eras/mod.rs` — declares submodules and
  documents the `pub(in crate::state)` visibility convention.
- `crates/ledger/src/state/eras/byron.rs` — `impl LedgerState { pub(in crate::state) fn apply_byron_block(...) }`.

`state.rs` keeps a `pub(super) mod eras;` declaration in the
module-declaration block (immediately after `pub(super) mod cbor;`).

### Visibility note: `pub(in crate::state)` is the minimum

State.rs is the **grandparent** of `state/eras/byron.rs` (path
`crate::state` vs `crate::state::eras::byron`). `pub(super) fn` would
expose the method only to `state::eras` (the parent module of byron),
which is too narrow — the dispatcher in `state.rs::apply_block_validated`
sits at `crate::state` and would not have access. The minimum
visibility that lets the dispatcher call the per-era method is
`pub(in crate::state)`. `pub(crate)` would also work but exposes the
method beyond what's needed; the tighter `pub(in crate::state)` keeps
the per-era apply API strictly within the state subsystem.

This visibility rule applies to every subsequent per-era extraction
(R269r through R269w).

### Why Byron first

Byron apply is the smallest per-era block (33 lines: cloned-and-commit
multi-era UTxO update + pre-computed `Tx.id` reuse). It has no
certificate, governance, or PPUP helpers. The dependency on the
`ByronTx` type lives in `state/eras/byron.rs` directly. State.rs no
longer needs to import `ByronTx`, so the previously-needed
`use crate::eras::byron::ByronTx;` at state.rs:4 was removed alongside
the move (otherwise it would warn unused).

Subsequent rounds (Shelley onwards) carry more helper-function
dependencies (`validate_auxiliary_data`, `validate_pre_alonzo_tx`,
`apply_certificates_and_withdrawals_with_future`, witness collection,
PPUP proposal validation, MIR accumulation) that will require
`pub(in crate::state)` promotion of each helper before the per-era
file can call them. Doing them one era at a time keeps each round
bounded and reviewable.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/eras/byron.rs::impl LedgerState::apply_byron_block` | upstream `Cardano.Chain.Block.Validation.applyBlock` (Byron block transition over `ChainValidationState`) |

Byron upstream's `Cardano.Chain.UTxO.UTxO.applyTxAux` is the direct
analogue of yggdrasil's `MultiEraUtxo::apply_byron_tx_with_id` (already
factored into `crates/ledger/src/utxo.rs`); R269q only moves the
block-level orchestration.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 8,409 | 8,377 | −32 |
| `crates/ledger/src/state/eras/mod.rs` | (new) | 18 | +18 |
| `crates/ledger/src/state/eras/byron.rs` | (new) | 60 | +60 |

The `+46` net is the new files' module-level docstrings + imports.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  (MIR)             | `state/mir.rs` (123)              | 110   | 12,596 |
| R269b (Ratify)          | `state/ratify.rs` (675)           | 657   | 11,939 |
| R269c (Enact)           | `state/enact.rs` (362)            | 343   | 11,602 |
| R269d (DepositPot)      | `state/deposit_pot.rs` (124)      | 106   | 11,496 |
| R269e (Phase-1)         | `state/phase1_validation.rs` (825) | 792   | 10,714 |
| R269f (PoolState)       | `state/pool_state.rs` (371)       | 349   | 10,369 |
| R269g (RewardAccounts)  | `state/reward_accounts.rs` (193) | 176   | 10,199 |
| R269h (StakeCredentials)| `state/stake_credentials.rs` (280) | 254 | 9,950 |
| R269i (DrepState)       | `state/drep_state.rs` (236)       | 218   | 9,737 |
| R269j (GovActionState)  | `state/governance_action_state.rs` (143) | 116 | 9,621 |
| R269k (CommitteeState)  | `state/committee_state.rs` (394)  | 364   | 9,257 |
| R269l (Treasury+ChainDep)| `state/{treasury,chain_dep}.rs` (49+70) | 77 | 9,180 |
| R269m (Snapshot)        | `state/snapshot.rs` (407)         | 370   | 8,810 |
| R269n (Checkpoint)      | `state/checkpoint.rs` (70)        | 50    | 8,766 |
| R269o (PPUP helpers)    | `state/ppup.rs` (77)              | 50    | 8,716 |
| R269p (LedgerState CBOR)| `state/cbor.rs` (342)             | 307   | 8,409 |
| **R269q (Byron apply)** | **`state/eras/{mod,byron}.rs` (18+60)** | **32**  | **8,377** |

Net `state.rs` reduction so far: **12,704 → 8,377 lines (−4,327, ~34 %)**
with seventeen sibling files plus a per-era subdirectory.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed (unchanged from R266d)
```

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 a–c | shipped | Gap BP narrowed (operator-time-blocked) |
| R266d | shipped | Gap BP further narrowed: cost-model parsing + variant selection ruled out |
| R269 a–p | shipped | 17 sibling state submodules carved (~4,295 lines moved) |
| **R269q (this round)** | **shipped** | `state/eras/byron.rs` extracted: Byron block apply (33 lines). State.rs cumulative reduction 4,327 lines (12,704 → 8,377). Validates the per-era split pattern with the smallest first slice. |

### Stop point — per-era extraction continues with Shelley (R269r)

R269r will extract `apply_shelley_block` (~199 lines) plus the helper
functions it calls that aren't already factored out. Expected helpers
needing `pub(in crate::state)` promotion:

- `validate_auxiliary_data` (around state.rs:1860)
- `validate_pre_alonzo_tx` (in `phase1_validation`, may already be
  visible via the `use phase1_validation::*` glob)
- `validate_output_network_ids` / `validate_withdrawal_network_ids`
  (likewise)
- `validate_witnesses_if_present` / `validate_native_scripts_if_present`
  / `validate_required_script_witnesses` /
  `validate_no_extraneous_script_witnesses` (likewise)
- `apply_certificates_and_withdrawals_with_future` (state.rs:7317 free
  function)
- `accumulate_mir_from_certs` (state.rs:7250 free function)

Most are already accessible via either `self.method()` or via the
existing glob-import; the free functions `apply_certificates_and_withdrawals_with_future`
and `accumulate_mir_from_certs` are at module-private visibility and
will need `pub(in crate::state)` promotion.

### References

- R269 a–p closures: `2026-05-06-round-269{,b,…,p}-state-*.md`
- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R269q
- Strategic envelope: `~/.claude/plans/dapper-giggling-haven.md` §R272
  per-era ledger rules split (R269q is the in-state-scoped precursor
  to that broader per-era rules carve)
- Upstream Byron block validation:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/byron/chain/executable-spec/src/Cardano/Chain/Block/Validation.hs`
- Upstream Byron UTxO apply:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/byron/chain/executable-spec/src/Cardano/Chain/UTxO.hs`
