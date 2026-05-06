## Round 269v — `state.rs` per-era split: twenty-second slice (Babbage apply)

Date: 2026-05-06
Branch: main
Type: Filename-mirror refactor (Phase γ R269 twenty-second slice)

### Slice scope

Extracted 476 source lines from `state.rs::impl LedgerState::apply_babbage_block`
into `crates/ledger/src/state/eras/babbage.rs` (~488 lines). Wired
`pub(super) mod babbage;` into the eras `mod.rs`.

Babbage adds reference inputs, inline datums, reference scripts, and
`collateral_return` to Alonzo's foundation. New surface compared with
Alonzo:

- `BabbageTxOutputRawSizes` companion sizes per output via
  `crate::eras::babbage::extract_babbage_tx_output_raw_sizes`.
- Reference-input UTxO presence check via
  `staged.validate_reference_inputs(ref_inputs)`.
- Script-well-formedness on the witness set via
  `crate::witnesses::validate_script_witnesses_well_formed`, and on
  reference scripts via `validate_reference_scripts_well_formed`.
- Reference-script collection via `collect_reference_script_hashes`
  (in `state::phase1_validation`, accessed via descendant visibility).
- Updated `validate_required_script_witnesses` and
  `validate_no_extraneous_script_witnesses` calls passing
  `body.reference_inputs.as_deref()` to allow ref-script satisfaction.
- Supplemental-datum check extended to walk reference inputs (collected
  as `(ShelleyTxIn, MultiEraTxOut)` pairs from `staged.get(txin)`).
- `validate_no_extra_redeemers` / `validate_no_missing_redeemers`
  extended with `body.reference_inputs.as_deref()` for redeemer
  coverage over reference scripts.
- Network validation extended to include `body.collateral_return` in
  the output set checked for matching network ID (matches upstream
  `allSizedOutputsTxBodyF`).
- `apply_collateral_only` extended with `body.collateral_return.as_ref()`
  for collateral-return refund on `is_valid=false` txs.
- TxContext for phase-2 evaluation now carries `reference_inputs:
  body.reference_inputs.clone().unwrap_or_default()` so the script
  context reflects ref inputs.

All new validators live in either `state::phase1_validation`
(`pub(super)`) or `crate::witnesses` / `crate::plutus_validation`
(public, cross-module). Visibility model unchanged from R269r:
descendants see ancestors' private items; only the apply method itself
needs `pub(in crate::state)`.

### Diff

| File | Lines before | Lines after | Δ |
|---|---|---|---|
| `crates/ledger/src/state.rs` | 7,394 | 6,916 | −478 |
| `crates/ledger/src/state/eras/mod.rs` | 22 | 23 | +1 |
| `crates/ledger/src/state/eras/babbage.rs` | (new) | ~488 | +488 |

Dropped now-unused `use crate::eras::babbage::BabbageTxBody;` from
state.rs.

### Cumulative R269 progress

| Slice | File created | Lines moved | state.rs running size |
|---|---|---|---|
| R269 – R269p | (16 sibling submodules + cbor codec) | 4,295 | 8,409 |
| R269q (Byron) | `state/eras/byron.rs` | 32 | 8,377 |
| R269r (Shelley) | `state/eras/shelley.rs` | 189 | 8,188 |
| R269s (Allegra) | `state/eras/allegra.rs` | 188 | 8,000 |
| R269t (Mary) | `state/eras/mary.rs` | 182 | 7,818 |
| R269u (Alonzo) | `state/eras/alonzo.rs` | 424 | 7,394 |
| **R269v (Babbage)** | **`state/eras/babbage.rs`** | **478** | **6,916** |

Net reduction: **12,704 → 6,916 lines (−5,788, ~46 %)** with 17 sibling
files plus `state/eras/` containing 6 era files (Byron through Babbage).

### Verification gates

```
cargo fmt --all -- --check       # clean (after rustfmt-applied tweak)
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed
```

### Stop point — Conway (R269w) is the final per-era slice

Conway (~810 lines) closes Phase γ's per-era arc. It introduces:

- Governance pipeline: `validate_conway_voters`,
  `validate_conway_vote_targets`, `validate_conway_voter_permissions`,
  `apply_conway_votes`, `validate_conway_proposals`,
  `validate_conway_current_treasury_value`,
  `collect_conway_unregistered_drep_voters`,
  `validate_unelected_committee_voters`, `validate_withdrawals_delegated`
  (all free fns currently in state.rs, accessible from descendant
  via the visibility rule).
- Reference-input / spending-input disjointness (Conway-only;
  Babbage allowed overlap).
- Treasury withdrawal validation per `current_treasury_value` field.
- DRep delegation cleanup post-block.

After R269w, `state.rs` should be in the ~6,100-line range, with the
remaining content being LedgerState struct, accessor methods,
governance helpers, and ~95 cross-cutting methods that aren't
era-specific.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` — Phase γ §R269v
- Upstream Babbage rules:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/{Bbody,Ledger,Utxow,Utxo,Utxos}.hs`
- Upstream `extractTxOutputRawSizes` analogue:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxOut.hs`
