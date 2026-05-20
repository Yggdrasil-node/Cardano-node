---
title: "Round 597 ShelleyLedgerPredFailure typed payloads + Mismatch decoder (A5 Phase-2.5)"
parent: Reference
---

# Round 597 ShelleyLedgerPredFailure typed payloads + Mismatch decoder (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Wires R596's typed `Withdrawals` decoder into the
`ShelleyLedgerPredFailure` variant payload and adds the second
typed payload decoder, `IncompleteWithdrawals` for the tag-3
`NonEmptyMap AccountAddress (Mismatch RelEQ Coin)`.

After this round, the two withdrawal-related tags (2 and 3) carry
fully-typed payloads; the UTXOW (tag 0) and DELEGS (tag 1) sub-rule
failures remain raw-cbor pending their own decoder rounds.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:826-869`
  (`Mismatch (r :: Relation) a` record + custom Show + EncCBORGroup
  2-element-array encoding).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ledger.hs:130,224,245-247`
  (tag-3 `ShelleyIncompleteWithdrawals (NonEmptyMap AccountAddress
  (Mismatch RelEQ Coin))`).

## Changes

- Refactored `ShelleyLedgerPredFailure` variant payloads:
  - `ShelleyWithdrawalsMissingAccounts(Vec<u8>)` →
    `ShelleyWithdrawalsMissingAccounts(Withdrawals)` (uses
    R596's typed decoder).
  - `ShelleyIncompleteWithdrawals(Vec<u8>)` →
    `ShelleyIncompleteWithdrawals(IncompleteWithdrawals)` (new
    typed decoder).
- Removed `ShelleyLedgerPredFailure::raw_inner()` — typed payloads
  now have their own accessors via pattern match; the
  still-raw UTXOW + DELEGS variants expose bytes directly through
  the variant payload.
- Updated `ShelleyLedgerPredFailure::Display` to route typed
  payloads through their `Display`, keeping the upstream
  stock-derived envelope `ShelleyWithdrawalsMissingAccounts (...)`
  and `ShelleyIncompleteWithdrawals (fromList [...])`.

New types in `crates/tools/cardano-submit-api/src/types.rs`:

- `Mismatch<T>` generic record with `relation: MismatchRelation`,
  `supplied: T`, `expected: T`. Mirrors upstream `data Mismatch
  (r :: Relation) a` and reproduces its custom Show:
  `Mismatch (<typeRep>) {supplied: <a>, expected: <a>}`.
- `MismatchRelation` enum (RelEQ / RelLTEQ / RelGTEQ / RelSubset)
  with `type_rep()` returning the upstream `typeRep`-derived
  relation tag for Show rendering.
- `CoinShow(u64)` Display wrapper emitting `Coin <n>` (Quiet-derived
  upstream shape).
- `IncompleteWithdrawals` struct with `entries:
  BTreeMap<RewardAccount, Mismatch<u64>>`. `from_cbor` walks the
  CBOR map and decodes each entry's 2-element Mismatch array;
  rejects empty maps to honour the NonEmpty invariant.

Updated test surface:

- `shelley_ledger_pred_failure_tag_dispatch` and
  `_constructor_names` use new typed payload helpers
  `empty_withdrawals_payload()` and
  `one_entry_incomplete_withdrawals_payload()`.
- New `shelley_ledger_pred_failure_display_renders_typed_withdrawals`
  pins the typed-Display envelope.
- New `shelley_ledger_pred_failure_display_renders_typed_incomplete_withdrawals`
  pins the Mismatch-bearing envelope.
- Replaced `shelley_ledger_pred_failure_raw_inner_round_trip` with
  `incomplete_withdrawals_rejects_empty_map` (NonEmpty invariant)
  and `incomplete_withdrawals_from_cbor_round_trips_supplied_expected`
  (2-entry decode with full supplied/expected verification).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (160 lib + 4
  doctests, +3 new tests vs R596 baseline of 157)

## Remaining (A5 Phase-2.5+)

- `ShelleyUtxowPredFailure` decoder for tag 0 (10+ variants
  including InvalidWitnessesUTXOW, MissingVKeyWitnessesUTXOW,
  ScriptWitnessNotValidatingUTXOW, ...).
- `ShelleyDelegsPredFailure` decoder for tag 1 (delegates further
  into DELPL/POOL/DELEG sub-rules).
- Mirror the per-era predicate-failure tree for Allegra / Mary /
  Alonzo / Babbage / Conway. Conway's `ConwayLedgerPredFailure`
  adds 4+ governance-specific variants on top of the Babbage set.
- Hook the typed decoder into
  `TxValidationErrorInCardanoMode::Display` so operators get full
  upstream-shape rendering through the entire predicate-failure
  tree.
