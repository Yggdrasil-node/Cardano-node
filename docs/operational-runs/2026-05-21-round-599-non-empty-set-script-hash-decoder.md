---
title: "Round 599 NonEmptySet ScriptHash decoder (A5 Phase-2.5)"
parent: Reference
---

# Round 599 NonEmptySet ScriptHash decoder (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `NonEmptySet ScriptHash` typed decoder and wires it into
`ShelleyUtxowPredFailure` tags 2 / 3 / 10. Tags 0 / 1 / 4 / 5 still
carry raw bytes pending their own decoder rounds.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-data/src/Data/Set/NonEmpty.hs:23-40`
  (`newtype NonEmptySet a = NonEmptySet (Set a)` + decCBOR
  invariant rejecting empty sets).
- `.reference-haskell-cardano-node/deps/cardano-base/cardano-binary/src/Cardano/Binary/ToCBOR.hs:779-808`
  (`encodeSetSkel` prefixing every Set with CBOR tag 258).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-binary/src/Cardano/Ledger/Binary/Decoding/Decoder.hs:904-925`
  (`decodeSet` — protocol-version ≥ 9 accepts but does not enforce
  the 258 tag prefix).

## Changes

- Added `ScriptHash([u8; 28])` newtype with Display matching
  upstream stock-derived `ScriptHash "<hex>"`.
- Added `NonEmptySetScriptHash` struct (`BTreeSet<ScriptHash>`).
  `from_cbor` decoder:
  - Tag-258-tolerant: peeks the major type, consumes tag 258 if
    present, otherwise proceeds to the array.
  - Walks the CBOR array, decoding each 28-byte entry into
    `ScriptHash`.
  - Rejects empty sets with `NonEmptySet requires at least one
    entry`.
  - Rejects wrong hash lengths with explicit error.
  - Iteration follows upstream `Data.Set.toAscList` byte-lex order
    via BTreeSet.
- Display: `NonEmptySet (fromList [ScriptHash "<hex>", ...])`
  matching upstream's deriving-stock Show on the NonEmptySet
  newtype.
- Refactored `ShelleyUtxowPredFailure` variants:
  - `MissingScriptWitnessesUTXOW(Vec<u8>)` →
    `MissingScriptWitnessesUTXOW(NonEmptySetScriptHash)`.
  - `ScriptWitnessNotValidatingUTXOW(Vec<u8>)` →
    `ScriptWitnessNotValidatingUTXOW(NonEmptySetScriptHash)`.
  - `ExtraneousScriptWitnessesUTXOW(Vec<u8>)` →
    `ExtraneousScriptWitnessesUTXOW(NonEmptySetScriptHash)`.
- Updated `from_cbor` dispatcher: tag 2/3/10 now decode the
  payload through `NonEmptySetScriptHash::from_cbor`; tags 0/1/4/5
  remain on the raw-bytes carrier.
- Updated `Display` to route the three typed variants through
  their typed Display instead of the raw-cbor marker.

5 focused unit tests:
- `non_empty_set_script_hash_decodes_tag258_form` (canonical
  encoder shape).
- `non_empty_set_script_hash_decodes_bare_list` (tag-tolerant).
- `non_empty_set_script_hash_rejects_empty_set` (NonEmpty
  invariant).
- `non_empty_set_script_hash_rejects_wrong_hash_length`.
- `shelley_utxow_pred_failure_missing_script_witnesses_decodes_tag2`
  end-to-end.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (171 lib + 4
  doctests, +5 new tests vs R598 baseline of 166)

## Remaining (A5 Phase-2.5+)

- `NonEmpty (VKey Witness)` decoder for tag 0
  (`InvalidWitnessesUTXOW`).
- `NonEmptySet (KeyHash Witness)` decoder for tag 1
  (`MissingVKeyWitnessesUTXOW`).
- `Set (KeyHash Witness)` decoder for tag 5
  (`MIRInsufficientGenesisSigsUTXOW`).
- `ShelleyUtxoPredFailure` decoder for tag 4 (nested sub-rule).
- Wire the typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- Mirror the predicate-failure tree for Allegra/Mary/Alonzo/Babbage/
  Conway eras.
