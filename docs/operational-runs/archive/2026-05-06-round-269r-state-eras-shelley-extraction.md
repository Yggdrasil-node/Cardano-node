## Round 269r — `state.rs` per-era split: eighteenth slice (Shelley apply)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 eighteenth slice — strict 1:1 with upstream Haskell, per-era impl carve)

### Context

Following R269q's Byron validation slice, R269r moves the Shelley apply
method to its own per-era file. Shelley is the first apply method with
substantial helper-function dependencies — proves the cross-module
visibility model holds for the realistic case before iterating across
the remaining four pre-Conway eras (Allegra, Mary, Alonzo, Babbage)
plus Conway itself.

### Slice scope

Extracted 197 source lines from `state.rs::impl LedgerState::apply_shelley_block`
into `crates/ledger/src/state/eras/shelley.rs` (224 lines including
module docstring + imports + 13 `use` lines).

Wired `pub(super) mod shelley;` into `crates/ledger/src/state/eras/mod.rs`
(R269q's mod.rs).

### Visibility findings — descendants get private-ancestor access automatically

Discovered (and documented in this round) that R269q's note about
`pub(in crate::state)` being mandatory for **methods callable from
state.rs** is asymmetric: the inverse direction (state/eras/shelley.rs
calling private free fns / methods declared in state.rs) **does not
require any visibility promotion** because Rust grants descendants
access to private items in their ancestor modules. Specifically,
`apply_shelley_block` calls:

- `self.certificate_validation_context()` (private fn method in state.rs)
- `self.ppup_slot_context(slot)` (private fn method in state.rs)
- `self.mir_validation_context(slot, false)` (private fn method in state.rs)
- `apply_certificates_and_withdrawals_with_future` (private free fn at state.rs:7264)

…all reachable from `state::eras::shelley` without promotion. Only the
top-level `apply_shelley_block` itself needs `pub(in crate::state)` so
the dispatcher in `state.rs::apply_block_validated` can call it.

The phase1_validation helpers (`validate_auxiliary_data`,
`validate_pre_alonzo_tx`, `validate_witnesses_if_present`,
`validate_native_scripts_if_present`,
`validate_required_script_witnesses`,
`validate_no_extraneous_script_witnesses`,
`validate_output_network_ids`, `validate_withdrawal_network_ids`) are
already `pub(super)` from R269e and reachable via
`use super::super::phase1_validation::{…}` from shelley.rs (descendants
see ancestor's `pub(super)` items because `pub(super)` resolves to "the
parent module of phase1_validation = state", and descendants of state
inherit access).

This visibility model means subsequent per-era extractions (R269s
Allegra, R269t Mary, R269u Alonzo, R269v Babbage, R269w Conway) need
**only** the `pub(in crate::state)` promotion on the apply method itself
— no helper-function promotion required.

### Mirror mapping

| Yggdrasil | Upstream Haskell |
|---|---|
| `state/eras/shelley.rs::impl LedgerState::apply_shelley_block` | upstream `Cardano.Ledger.Shelley.Rules.Bbody` (block-level orchestration) plus `Cardano.Ledger.Shelley.Rules.Ledger` (per-tx phase-1 + state transition) plus `Cardano.Ledger.Shelley.Rules.Utxow` (witness validation rule) |

The fine-grained per-rule split (separating Bbody vs Ledger vs Utxow
within Shelley) is queued for a future round once all eras have been
extracted; this round consolidates them in one file matching the
existing structure of `apply_shelley_block`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 8,377 | 8,188 | −189 |
| `crates/ledger/src/state/eras/mod.rs` | 18 | 19 | +1 |
| `crates/ledger/src/state/eras/shelley.rs` | (new) | 224 | +224 |

The `+36` net is the new file's module-level docstring + imports + glue.
Also dropped the now-unused `ShelleyTxBody` from state.rs's import line
6 (kept `ShelleyTxIn` and `ShelleyUtxo` which are still used by
`LedgerState`'s field types and several helper methods).

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269  – R269p | (16 sibling submodules + cbor codec) | 4,295 | 8,409 |
| R269q (Byron apply)     | `state/eras/byron.rs` (60)        | 32    | 8,377 |
| **R269r (Shelley apply)** | **`state/eras/shelley.rs` (224)** | **189** | **8,188** |

Net `state.rs` reduction so far: **12,704 → 8,188 lines (−4,516, ~36 %)**
with seventeen sibling files plus a per-era subdirectory containing two
files.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean (after one doc-comment tweak: avoided
                                 #   leading `+` in a continuation line that
                                 #   triggered clippy::doc_lazy_continuation)
cargo test-all                   # 4 855 passed, 0 failed (unchanged)
```

### Stop point — Allegra (R269s) is the next slice

Allegra apply (~187 lines) is structurally near-identical to Shelley
plus auxiliary data validity-interval validation. R269s should be a
straight copy-and-paste-with-AllegraTxBody-substitution. With the
visibility model now confirmed, the remaining four eras (Mary, Alonzo,
Babbage, Conway) follow as R269t–R269w.

### References

- R269q closure: `2026-05-06-round-269q-state-eras-byron-extraction.md`
- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R269r
- Upstream Shelley rules:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/{Bbody,Ledger,Utxow,Utxo,Pool,Deleg,Cert}.hs`
