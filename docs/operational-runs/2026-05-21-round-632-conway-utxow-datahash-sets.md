---
title: "Round 632 Typed Conway UTXOW DataHash-set variants (A5 Phase-2.5)"
parent: Reference
---

# Round 632 Typed Conway UTXOW DataHash-set variants (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXOW tags 11 (`MissingRequiredDatums`) and 12
(`NotAllowedSupplementalDatums`) by adding the `DataHash`,
`SetDataHash`, and `NonEmptySetDataHash` carriers. After R632,
**14 of 19 Conway UTXOW variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxow.hs:83-92`
  (`MissingRequiredDatums (NonEmptySet DataHash) (Set DataHash)`
  and `NotAllowedSupplementalDatums` with the same shape).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Hashes.hs:149`
  (`type DataHash = SafeHash EraIndependentData` — a type alias,
  so Show is the bare `SafeHash "<hex>"`).

## Changes

- Added `DataHash([u8; 32])` newtype. Display: `SafeHash
  "<hex>"` (DataHash is a SafeHash type alias upstream, no
  wrapper constructor).
- Added `decode_data_hash_set` helper — walks the optional
  tag-258 prefix then an array of 32-byte byte-strings,
  returning a `BTreeSet<DataHash>` (byte-lex order matching
  upstream `Data.Set.toAscList`).
- Added `SetDataHash` (possibly-empty) — Display `fromList
  [...]`.
- Added `NonEmptySetDataHash` (rejects empty at decode time) —
  Display `NonEmptySet (fromList [...])`.
- Refactored `ConwayUtxowPredFailure` variants 11/12 from
  `Vec<u8>` to struct shapes:
  - Tag 11 `MissingRequiredDatums { missing:
    NonEmptySetDataHash, received: SetDataHash }`.
  - Tag 12 `NotAllowedSupplementalDatums { unallowed:
    NonEmptySetDataHash, acceptable: SetDataHash }`.
- `from_cbor` decodes both as 3-element envelopes `[tag,
  NonEmptySet, Set]`. Display routes the typed nested payloads.

3 new focused unit tests:
- `_missing_required_datums_tag11` — NonEmptySet (tag-258) +
  Set (bare-array) round-trip.
- `_not_allowed_supplemental_datums_tag12_empty_set` — exercises
  the empty acceptable-set path.
- `_missing_required_datums_rejects_empty_nonempty_set` —
  NonEmptySet empty-array rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (281 lib + 4
  doctests + 1 main, +3 new tests vs R631 baseline of 278)

## Remaining (A5 Phase-2.5+)

- Conway UTXOW raw variants: tag 10 (MissingRedeemers —
  `NonEmpty (PlutusPurpose AsItem, ScriptHash)`), tag 13
  (PPViewHashesDontMatch — `Mismatch (StrictMaybe
  ScriptIntegrityHash)`), tag 15 (ExtraRedeemers — `NonEmpty
  (PlutusPurpose AsIx)`), tag 18 (ScriptIntegrityHashMismatch).
- Conway UTXO raw variants (Value, ExUnits, ValidityInterval,
  DeltaCoin, NonEmptyMap, triple/pair encodings).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
